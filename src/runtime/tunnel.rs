use crate::monitor::TaskMonitor;
use anyhow::{anyhow, bail, Context, Result};
use clap::ValueEnum;
use std::process::{Command as StdCommand, Stdio as StdStdio};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{sleep, timeout, Duration};
use url::{Host, Url};

const PUBLIC_HEALTH_INTERVAL: Duration = Duration::from_secs(25);
const PUBLIC_HEALTH_TIMEOUT: Duration = Duration::from_secs(5);
const PUBLIC_STARTUP_HEALTH_ATTEMPTS: usize = 6;
const AUTO_PROVIDER_START_TIMEOUT: Duration = Duration::from_secs(12);
const QUICK_TUNNEL_HOST_SUFFIXES: &[&str] = &[
    ".trycloudflare.com",
    ".localhost.run",
    ".lhr.life",
    ".pinggy.link",
    ".pinggy-free.link",
    ".pinggy.net",
];
const QUICK_TUNNEL_DENIED_HOSTS: &[&str] = &[
    "api.trycloudflare.com",
    "admin.localhost.run",
    "www.localhost.run",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub(crate) enum TunnelProvider {
    #[default]
    Auto,
    Cloudflare,
    #[value(name = "localhost-run")]
    LocalhostRun,
    Pinggy,
}

impl TunnelProvider {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Cloudflare => "cloudflare",
            Self::LocalhostRun => "localhost.run",
            Self::Pinggy => "pinggy",
        }
    }

    pub(crate) fn auto_candidates() -> [Self; 3] {
        [Self::Cloudflare, Self::LocalhostRun, Self::Pinggy]
    }
}

pub(crate) struct ActiveTunnel {
    child: Child,
    public_url: String,
    provider: TunnelProvider,
}

impl ActiveTunnel {
    pub(crate) fn public_url(&self) -> &str {
        &self.public_url
    }

    pub(crate) fn provider_label(&self) -> &'static str {
        self.provider.label()
    }

    pub(crate) fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.child.try_wait()
    }

    pub(crate) async fn stop(&mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
    }
}

pub(crate) fn normalize_public_url(value: &str) -> Result<String> {
    let url = Url::parse(value).context("--public-url must be a valid absolute URL")?;
    if url.cannot_be_a_base()
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!(
            "--public-url must be an origin URL without a path, user information, query string, or fragment"
        );
    }
    let allowed = match (url.scheme(), url.host()) {
        ("https", Some(_)) => true,
        ("http", Some(Host::Domain(domain))) => domain.eq_ignore_ascii_case("localhost"),
        ("http", Some(Host::Ipv4(address))) => address.is_loopback(),
        ("http", Some(Host::Ipv6(address))) => address.is_loopback(),
        _ => false,
    };
    if !allowed {
        bail!("--public-url must use HTTPS, except loopback HTTP is allowed for local testing");
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

pub(crate) async fn start_managed_tunnel(
    selected: TunnelProvider,
    local_url: &str,
    instance_id: &str,
    install_missing: bool,
) -> Result<ActiveTunnel> {
    println!("  ↗ tunnel       requesting HTTPS endpoint");
    let candidates = if selected == TunnelProvider::Auto {
        TunnelProvider::auto_candidates().to_vec()
    } else {
        vec![selected]
    };
    let attempts_per_provider = if selected == TunnelProvider::Auto {
        1
    } else {
        3
    };
    let mut failures = Vec::new();

    for provider in candidates {
        for attempt in 1..=attempts_per_provider {
            println!("  · tunnel       trying {}", provider.label());
            let allow_install = install_missing && selected != TunnelProvider::Auto;
            let start_result = if selected == TunnelProvider::Auto {
                match timeout(
                    AUTO_PROVIDER_START_TIMEOUT,
                    start_tunnel_provider_once(provider, local_url, allow_install),
                )
                .await
                {
                    Ok(result) => result,
                    Err(_) => Err(anyhow!(
                        "{} startup timed out after {} seconds",
                        provider.label(),
                        AUTO_PROVIDER_START_TIMEOUT.as_secs()
                    )),
                }
            } else {
                start_tunnel_provider_once(provider, local_url, allow_install).await
            };
            match start_result {
                Ok((mut child, public_url)) => {
                    match verify_tunnel_candidate(&public_url, instance_id).await {
                        Ok(()) => {
                            println!(
                                "  ✓ tunnel       {} connected · {public_url}",
                                provider.label()
                            );
                            return Ok(ActiveTunnel {
                                child,
                                public_url,
                                provider,
                            });
                        }
                        Err(error) => {
                            let _ = child.start_kill();
                            let _ = child.wait().await;
                            failures.push(format!("{} health: {error}", provider.label()));
                            eprintln!(
                                "  ! tunnel       {} URL was not reachable · {}",
                                provider.label(),
                                truncate_diagnostic(&error, 160)
                            );
                        }
                    }
                }
                Err(error) => {
                    let detail = format!("{error:#}");
                    failures.push(format!("{}: {detail}", provider.label()));
                    eprintln!(
                        "  ! tunnel       {} attempt {attempt}/{attempts_per_provider} failed · {}",
                        provider.label(),
                        truncate_diagnostic(detail.lines().next().unwrap_or("unknown error"), 160)
                    );
                }
            }
            if attempt < attempts_per_provider {
                sleep(Duration::from_secs(attempt as u64)).await;
            }
        }
    }

    bail!(
        "no managed tunnel provider became reachable. Tried {}. Use `--tunnel-provider cloudflare|localhost-run|pinggy`, or pass a stable `--public-url https://…`.\n{}",
        if selected == TunnelProvider::Auto {
            "cloudflare, localhost.run, pinggy"
        } else {
            selected.label()
        },
        failures.join("\n")
    )
}

pub(crate) async fn public_endpoint_health_loop(
    public_url: String,
    instance_id: String,
    monitor: TaskMonitor,
    mut stop: watch::Receiver<bool>,
) {
    loop {
        if *stop.borrow() {
            return;
        }
        match check_public_endpoint(&public_url, &instance_id).await {
            Ok(()) => monitor.mark_public_url_check(true, None),
            Err(error) => monitor.mark_public_url_check(false, Some(error)),
        }
        tokio::select! {
            _ = sleep(PUBLIC_HEALTH_INTERVAL) => {},
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    return;
                }
            }
        }
    }
}

pub(crate) async fn wait_for_public_endpoint(
    public_url: &str,
    instance_id: &str,
    monitor: &TaskMonitor,
) -> Result<(), String> {
    println!("  · endpoint     verifying this wcode instance");
    let mut last_error = String::new();
    for attempt in 1..=PUBLIC_STARTUP_HEALTH_ATTEMPTS {
        match check_public_endpoint(public_url, instance_id).await {
            Ok(()) => {
                monitor.mark_public_url_check(true, None);
                println!("  ✓ endpoint     reachable and instance-matched");
                return Ok(());
            }
            Err(error) => {
                last_error = error;
                monitor.mark_public_url_check(false, Some(last_error.clone()));
                eprintln!(
                    "  ! endpoint     attempt {attempt}/{PUBLIC_STARTUP_HEALTH_ATTEMPTS} failed · {}",
                    truncate_diagnostic(&last_error, 180)
                );
                if attempt < PUBLIC_STARTUP_HEALTH_ATTEMPTS {
                    sleep(Duration::from_secs(attempt.min(3) as u64)).await;
                }
            }
        }
    }
    Err(last_error)
}

async fn check_public_endpoint(public_url: &str, expected_instance_id: &str) -> Result<(), String> {
    let health_url = format!("{public_url}/healthz");
    let mut command = Command::new("curl");
    command
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--max-time",
            "5",
            &health_url,
        ])
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let output = timeout(PUBLIC_HEALTH_TIMEOUT, command.output())
        .await
        .map_err(|_| {
            format!(
                "health check timed out after {}s",
                PUBLIC_HEALTH_TIMEOUT.as_secs()
            )
        })?
        .map_err(|error| format!("curl could not run: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "curl exited with {}{}",
            output.status,
            if stderr.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", truncate_diagnostic(stderr.trim(), 180))
            }
        ));
    }
    validate_health_response(&output.stdout, expected_instance_id)
}

pub(crate) fn validate_health_response(
    body: &[u8],
    expected_instance_id: &str,
) -> Result<(), String> {
    let payload: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| format!("health endpoint returned invalid JSON: {error}"))?;
    let actual = payload
        .get("instance_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "health response is missing instance_id".to_owned())?;
    if actual != expected_instance_id {
        return Err(format!(
            "health response belongs to a different wcode instance ({})",
            truncate_diagnostic(actual, 12)
        ));
    }
    if payload.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err("health response did not report ok=true".to_owned());
    }
    Ok(())
}

fn truncate_diagnostic(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let prefix: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

async fn start_tunnel_provider_once(
    provider: TunnelProvider,
    local_url: &str,
    install_missing: bool,
) -> Result<(Child, String)> {
    match provider {
        TunnelProvider::Auto => bail!("auto is a tunnel selection policy, not a concrete provider"),
        TunnelProvider::Cloudflare => {
            ensure_cloudflared(install_missing)?;
            start_cloudflared_once(local_url).await
        }
        TunnelProvider::LocalhostRun | TunnelProvider::Pinggy => {
            ensure_ssh()?;
            start_ssh_tunnel_once(provider, local_url).await
        }
    }
}

fn ensure_ssh() -> Result<()> {
    if command_succeeds("ssh", &["-V"]) {
        return Ok(());
    }
    bail!("OpenSSH client is unavailable; localhost.run and Pinggy require `ssh` on PATH")
}

async fn verify_tunnel_candidate(public_url: &str, instance_id: &str) -> Result<(), String> {
    let mut last_error = String::new();
    for attempt in 1..=2 {
        match check_public_endpoint(public_url, instance_id).await {
            Ok(()) => return Ok(()),
            Err(error) => last_error = error,
        }
        if attempt < 2 {
            sleep(Duration::from_secs(1)).await;
        }
    }
    Err(last_error)
}

async fn start_ssh_tunnel_once(
    provider: TunnelProvider,
    local_url: &str,
) -> Result<(Child, String)> {
    let local = Url::parse(local_url).context("invalid local tunnel target URL")?;
    let host = local
        .host_str()
        .context("local tunnel target is missing a host")?;
    let port = local
        .port_or_known_default()
        .context("local tunnel target is missing a port")?;
    let forward_host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    let (remote, remote_forward, extra_args): (&str, String, &[&str]) = match provider {
        TunnelProvider::LocalhostRun => (
            "nokey@localhost.run",
            format!("80:{forward_host}:{port}"),
            &[],
        ),
        TunnelProvider::Pinggy => (
            "free.pinggy.io",
            format!("0:{forward_host}:{port}"),
            &["-p", "443"],
        ),
        _ => bail!("SSH tunnel requested for non-SSH provider"),
    };

    let mut command = Command::new("ssh");
    command
        .args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=2",
            "-o",
            "StrictHostKeyChecking=accept-new",
        ])
        .args(extra_args)
        .arg("-R")
        .arg(&remote_forward)
        .arg(remote)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to start {} SSH tunnel", provider.label()))?;
    let stdout = child
        .stdout
        .take()
        .context("SSH tunnel stdout is unavailable")?;
    let stderr = child
        .stderr
        .take()
        .context("SSH tunnel stderr is unavailable")?;
    let (line_sender, mut line_receiver) = mpsc::unbounded_channel::<String>();
    spawn_tunnel_output_reader(stdout, line_sender.clone(), provider.label());
    spawn_tunnel_output_reader(stderr, line_sender, provider.label());

    let wait_for_url = async {
        let mut recent_logs = Vec::new();
        while let Some(line) = line_receiver.recv().await {
            if recent_logs.len() >= 16 {
                recent_logs.remove(0);
            }
            recent_logs.push(line.clone());
            if let Some(url) = extract_ssh_tunnel_url(provider, &line) {
                return Ok(url);
            }
        }
        Err(if recent_logs.is_empty() {
            format!("{} SSH tunnel exited without output", provider.label())
        } else {
            recent_logs.join("\n")
        })
    };
    let public_url = match timeout(Duration::from_secs(20), wait_for_url).await {
        Ok(Ok(url)) => url,
        Ok(Err(details)) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            bail!(
                "{} exited before producing a public URL:\n{details}",
                provider.label()
            );
        }
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            bail!("timed out waiting for {} public URL", provider.label());
        }
    };
    Ok((child, public_url))
}

fn spawn_tunnel_output_reader<R>(
    reader: R,
    sender: mpsc::UnboundedSender<String>,
    provider: &'static str,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    let _ = sender.send(line.clone());
                    if line.contains("ERR") || line.to_ascii_lowercase().contains("error") {
                        tracing::debug!(target: "wcode::tunnel", provider, "{line}");
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    tracing::debug!(target: "wcode::tunnel", provider, "failed to read tunnel output: {error}");
                    break;
                }
            }
        }
    });
}

pub(crate) fn is_quick_tunnel_url(mcp_url: &str) -> bool {
    let Ok(url) = Url::parse(mcp_url) else {
        return false;
    };
    url.host_str().is_some_and(is_quick_tunnel_host)
}

fn is_quick_tunnel_host(host: &str) -> bool {
    host_matches_public_tunnel(host, QUICK_TUNNEL_HOST_SUFFIXES, QUICK_TUNNEL_DENIED_HOSTS)
}

fn host_matches_public_tunnel(host: &str, suffixes: &[&str], denied: &[&str]) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if denied.iter().any(|denied_host| host == *denied_host) {
        return false;
    }
    suffixes
        .iter()
        .any(|suffix| host.ends_with(suffix) && host.len() > suffix.len())
}

pub(crate) fn extract_ssh_tunnel_url(provider: TunnelProvider, line: &str) -> Option<String> {
    let expected_suffixes: &[&str] = match provider {
        TunnelProvider::LocalhostRun => &[".localhost.run", ".lhr.life"],
        TunnelProvider::Pinggy => &[".pinggy.link", ".pinggy-free.link", ".pinggy.net"],
        _ => return None,
    };
    for token in line.split(|ch: char| {
        ch.is_whitespace() || matches!(ch, '|' | '`' | '"' | '<' | '>' | ')' | ']' | '}' | ',')
    }) {
        let candidate = token
            .trim_matches(|ch: char| matches!(ch, '(' | '[' | '{' | ':' | ';'))
            .trim_end_matches('/')
            .trim_end_matches('.');
        if candidate.is_empty() {
            continue;
        }
        let host = candidate
            .strip_prefix("https://")
            .or_else(|| candidate.strip_prefix("http://"))
            .unwrap_or(candidate)
            .split('/')
            .next()
            .unwrap_or_default()
            .trim_end_matches('.');
        if host_matches_public_tunnel(host, expected_suffixes, QUICK_TUNNEL_DENIED_HOSTS) {
            return Some(format!("https://{host}"));
        }
    }
    None
}

fn ensure_cloudflared(install_missing: bool) -> Result<()> {
    println!("  · cloudflared  checking dependency");
    if command_succeeds("cloudflared", &["--version"]) {
        println!("  ✓ cloudflared  available");
        return Ok(());
    }
    if !install_missing {
        bail!(
            "cloudflared is missing; {} Remove --no-install to allow the supported installer.",
            cloudflared_install_hint()
        );
    }

    #[cfg(target_os = "macos")]
    {
        if !command_succeeds("brew", &["--version"]) {
            bail!(
                "cloudflared is missing and Homebrew is unavailable; {}",
                cloudflared_install_hint()
            );
        }
        run_installer("brew", &["install", "cloudflared"], "Homebrew")?;
    }

    #[cfg(target_os = "windows")]
    {
        if !command_succeeds("winget", &["--version"]) {
            bail!(
                "cloudflared is missing and winget is unavailable. Install it from https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/ or place cloudflared.exe on PATH."
            );
        }
        run_installer(
            "winget",
            &[
                "install",
                "--id",
                "Cloudflare.cloudflared",
                "--exact",
                "--accept-package-agreements",
                "--accept-source-agreements",
            ],
            "winget",
        )?;
    }

    #[cfg(target_os = "linux")]
    {
        bail!(
            "cloudflared is missing. {} Automatic distro installation is intentionally disabled because cloudflared is not consistently available in default repositories.",
            cloudflared_install_hint()
        );
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        bail!("cloudflared is missing; install it from Cloudflare and place it on PATH");
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        if !command_succeeds("cloudflared", &["--version"]) {
            bail!(
                "the installer completed but cloudflared is still unavailable on PATH; restart the terminal or install it manually"
            );
        }
        println!("  ✓ cloudflared  installed");
        Ok(())
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn run_installer(program: &str, args: &[&str], label: &str) -> Result<()> {
    println!("  ↓ cloudflared  installing with {label}");
    let status = StdCommand::new(program)
        .args(args)
        .stdin(StdStdio::inherit())
        .stdout(StdStdio::inherit())
        .stderr(StdStdio::inherit())
        .status()
        .with_context(|| format!("failed to launch {label}"))?;
    if !status.success() {
        bail!(
            "{label} could not install cloudflared; {}",
            cloudflared_install_hint()
        );
    }
    Ok(())
}

fn cloudflared_install_hint() -> String {
    #[cfg(target_os = "macos")]
    {
        return "Run `brew install cloudflared`.".to_owned();
    }
    #[cfg(target_os = "windows")]
    {
        return "Run `winget install --id Cloudflare.cloudflared` or download the official Windows binary.".to_owned();
    }
    #[cfg(target_os = "linux")]
    {
        let manager = ["apt-get", "dnf", "yum", "pacman"]
            .into_iter()
            .find(|program| command_succeeds(program, &["--version"]))
            .unwrap_or("your distribution package manager");
        return format!(
            "Detected {manager}; follow Cloudflare's repository instructions at https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/."
        );
    }
    #[allow(unreachable_code)]
    "Install cloudflared from https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/ and place it on PATH.".to_owned()
}

fn command_succeeds(program: &str, args: &[&str]) -> bool {
    StdCommand::new(program)
        .args(args)
        .stdin(StdStdio::null())
        .stdout(StdStdio::null())
        .stderr(StdStdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

async fn start_cloudflared_once(local_url: &str) -> Result<(Child, String)> {
    let mut child = Command::new("cloudflared")
        .args([
            "tunnel",
            "--url",
            local_url,
            "--protocol",
            "http2",
            "--no-autoupdate",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("failed to start cloudflared")?;
    let stderr = child
        .stderr
        .take()
        .context("cloudflared stderr is unavailable")?;
    let (url_sender, url_receiver) = oneshot::channel::<Result<String, String>>();
    tokio::spawn(async move {
        let mut url_sender = Some(url_sender);
        let mut recent_logs: Vec<String> = Vec::new();
        let mut lines = BufReader::new(stderr).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    recent_logs.push(line.clone());
                    if recent_logs.len() > 12 {
                        recent_logs.remove(0);
                    }
                    if let Some(url) = extract_cloudflare_tunnel_url(&line) {
                        if let Some(sender) = url_sender.take() {
                            let _ = sender.send(Ok(url));
                        }
                    }
                    if line.contains("ERR") || line.contains("error") {
                        tracing::debug!(target: "wcode::tunnel", "{line}");
                    }
                }
                Ok(None) => {
                    if let Some(sender) = url_sender.take() {
                        let details = if recent_logs.is_empty() {
                            "cloudflared exited without output".to_owned()
                        } else {
                            recent_logs.join("\n")
                        };
                        let _ = sender.send(Err(details));
                    }
                    break;
                }
                Err(error) => {
                    tracing::debug!(target: "wcode::tunnel", "failed to read logs: {error}");
                    if let Some(sender) = url_sender.take() {
                        let _ =
                            sender.send(Err(format!("failed to read cloudflared logs: {error}")));
                    }
                    break;
                }
            }
        }
    });
    let public_url = match timeout(Duration::from_secs(15), url_receiver).await {
        Ok(Ok(Ok(url))) => url,
        Ok(Ok(Err(details))) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            bail!("cloudflared exited before producing a public URL:\n{details}");
        }
        Ok(Err(_)) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            bail!("cloudflared log channel closed unexpectedly");
        }
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            bail!("timed out after 15 seconds waiting for Cloudflare Tunnel URL");
        }
    };
    Ok((child, public_url))
}

pub(crate) fn extract_cloudflare_tunnel_url(line: &str) -> Option<String> {
    for (start, _) in line.match_indices("https://") {
        let candidate = line[start..]
            .split(|ch: char| {
                ch.is_whitespace()
                    || matches!(ch, '|' | '`' | '"' | '<' | '>' | ')' | ']' | '}' | ',')
            })
            .next()
            .unwrap_or_default()
            .trim_end_matches('/');
        let Ok(url) = Url::parse(candidate) else {
            continue;
        };
        let Some(host) = url.host_str() else { continue };
        if host.ends_with(".trycloudflare.com") && host != "api.trycloudflare.com" {
            return Some(candidate.to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_health_response_must_match_the_current_instance() {
        let body = br#"{"ok":true,"instance_id":"instance-a"}"#;
        assert!(validate_health_response(body, "instance-a").is_ok());

        let mismatch = validate_health_response(body, "instance-b").unwrap_err();
        assert!(mismatch.contains("different wcode instance"));
        assert!(validate_health_response(br#"{"ok":true}"#, "instance-a")
            .unwrap_err()
            .contains("missing instance_id"));
        assert!(validate_health_response(
            br#"{"ok":false,"instance_id":"instance-a"}"#,
            "instance-a"
        )
        .unwrap_err()
        .contains("ok=true"));
    }

    #[test]
    fn public_url_requires_https_or_loopback_http() {
        assert_eq!(
            normalize_public_url("https://example.com/").unwrap(),
            "https://example.com"
        );
        assert_eq!(
            normalize_public_url("http://127.0.0.1:8765/").unwrap(),
            "http://127.0.0.1:8765"
        );
        assert_eq!(
            normalize_public_url("http://[::1]:8765/").unwrap(),
            "http://[::1]:8765"
        );
        for value in [
            "http://example.com",
            "https://example.com/gateway",
            "ftp://example.com",
            "https://user@example.com",
            "https://example.com?mode=test",
            "https://example.com/#fragment",
            "not-a-url",
        ] {
            assert!(
                normalize_public_url(value).is_err(),
                "unexpectedly accepted {value}"
            );
        }
    }

    #[test]
    fn parses_quick_tunnel_url() {
        let line = "2026 INF | https://bright-demo.trycloudflare.com |";
        assert_eq!(
            extract_cloudflare_tunnel_url(line).as_deref(),
            Some("https://bright-demo.trycloudflare.com")
        );
        assert_eq!(
            extract_cloudflare_tunnel_url("request https://api.trycloudflare.com/tunnel\": failed"),
            None
        );
    }

    #[test]
    fn detects_managed_quick_tunnel_mcp_urls() {
        for url in [
            "https://bright-demo.trycloudflare.com/mcp",
            "https://bright-demo.localhost.run/mcp",
            "https://5d993e65a9d400.lhr.life/mcp",
            "https://rndm-abcd1234.pinggy.link/mcp",
            "https://rndm.run.pinggy-free.link/mcp",
            "https://rndm.free.pinggy.net/mcp",
        ] {
            assert!(is_quick_tunnel_url(url), "expected quick tunnel {url}");
        }
        for url in [
            "https://admin.localhost.run/mcp",
            "https://www.localhost.run/mcp",
            "https://api.trycloudflare.com/mcp",
            "https://example.com/mcp",
            "http://127.0.0.1:8765/mcp",
        ] {
            assert!(
                !is_quick_tunnel_url(url),
                "unexpectedly treated {url} as a quick tunnel"
            );
        }
    }

    #[test]
    fn parses_free_ssh_tunnel_urls_without_accepting_provider_hosts() {
        assert_eq!(
            extract_ssh_tunnel_url(
                TunnelProvider::LocalhostRun,
                "https://bright-demo.localhost.run tunneled with tls termination"
            )
            .as_deref(),
            Some("https://bright-demo.localhost.run")
        );
        assert_eq!(
            extract_ssh_tunnel_url(
                TunnelProvider::LocalhostRun,
                "5d993e65a9d400.lhr.life tunneled with tls termination, https://5d993e65a9d400.lhr.life"
            )
            .as_deref(),
            Some("https://5d993e65a9d400.lhr.life")
        );
        assert_eq!(
            extract_ssh_tunnel_url(
                TunnelProvider::LocalhostRun,
                "https://5d993e65a9d400.lhr.life"
            )
            .as_deref(),
            Some("https://5d993e65a9d400.lhr.life")
        );
        assert_eq!(
            extract_ssh_tunnel_url(
                TunnelProvider::LocalhostRun,
                "To set up and manage custom domains go to https://admin.localhost.run/"
            ),
            None
        );
        assert_eq!(
            extract_ssh_tunnel_url(
                TunnelProvider::LocalhostRun,
                "https://admin.localhost.run tunneled with tls termination"
            ),
            None
        );
        assert_eq!(
            extract_ssh_tunnel_url(TunnelProvider::Pinggy, "Host: rndm-abcd1234.pinggy.link")
                .as_deref(),
            Some("https://rndm-abcd1234.pinggy.link")
        );
        assert_eq!(
            extract_ssh_tunnel_url(
                TunnelProvider::Pinggy,
                "Forwarding HTTPS traffic from https://rndm.run.pinggy-free.link"
            )
            .as_deref(),
            Some("https://rndm.run.pinggy-free.link")
        );
        assert_eq!(
            extract_ssh_tunnel_url(
                TunnelProvider::Pinggy,
                "Forwarding HTTPS traffic from https://rndm.free.pinggy.net"
            )
            .as_deref(),
            Some("https://rndm.free.pinggy.net")
        );
        assert_eq!(
            extract_ssh_tunnel_url(TunnelProvider::LocalhostRun, "connect localhost.run"),
            None
        );
        assert_eq!(
            extract_ssh_tunnel_url(TunnelProvider::LocalhostRun, "connect www.localhost.run"),
            None
        );
        assert_eq!(
            extract_ssh_tunnel_url(TunnelProvider::Pinggy, "connect free.pinggy.io"),
            None
        );
        assert_eq!(
            TunnelProvider::auto_candidates(),
            [
                TunnelProvider::Cloudflare,
                TunnelProvider::LocalhostRun,
                TunnelProvider::Pinggy
            ]
        );
    }
}
