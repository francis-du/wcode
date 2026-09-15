use crate::monitor::{OperatorMessageKind, TaskMonitor};
use anyhow::{anyhow, bail, Context, Result};
use clap::ValueEnum;
use std::process::{Command as StdCommand, Stdio as StdStdio};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{sleep, timeout, Duration};
use url::{Host, Url};

const PUBLIC_HEALTH_INTERVAL: Duration = Duration::from_secs(15);
const PUBLIC_RECOVERY_HEALTH_INTERVAL: Duration = Duration::from_secs(2);
const PUBLIC_HEALTH_TIMEOUT: Duration = Duration::from_secs(3);
const PUBLIC_TRANSIENT_RECHECKS: usize = 2;
const PUBLIC_TRANSIENT_RECHECK_DELAY: Duration = Duration::from_millis(250);
const PUBLIC_STARTUP_HEALTH_ATTEMPTS: usize = 4;
const AUTO_PROVIDER_START_TIMEOUT: Duration = Duration::from_secs(15);

#[path = "cloudflare.rs"]
mod cloudflare;
use cloudflare::{command_succeeds, ensure_cloudflared, start_cloudflared_once};
#[path = "devtunnel.rs"]
mod devtunnel;
#[cfg(test)]
use devtunnel::extract_devtunnel_url;
use devtunnel::{ensure_devtunnel, start_devtunnel_once};
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, ValueEnum)]
pub(crate) enum TunnelProvider {
    #[default]
    Auto,
    Cloudflare,
    #[value(name = "localhost-run")]
    LocalhostRun,
    Pinggy,
    Tailscale,
    #[value(name = "dev-tunnel")]
    DevTunnel,
}

impl TunnelProvider {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Cloudflare => "cloudflare",
            Self::LocalhostRun => "localhost.run",
            Self::Pinggy => "pinggy",
            Self::Tailscale => "tailscale",
            Self::DevTunnel => "dev-tunnel",
        }
    }

    pub(crate) fn auto_candidates() -> Vec<Self> {
        vec![
            Self::Cloudflare,
            Self::LocalhostRun,
            Self::Pinggy,
            Self::Tailscale,
        ]
    }

    pub(crate) fn has_stable_endpoint(self) -> bool {
        matches!(self, Self::Tailscale | Self::DevTunnel)
    }
}

pub(crate) struct ActiveTunnel {
    // Stable providers can survive a wcode restart independently of the
    // process that originally configured them. Reusing an instance-matched
    // endpoint avoids killing and recreating a healthy funnel during startup.
    child: Option<Child>,
    public_url: String,
    provider: TunnelProvider,
    connected_at: std::time::Instant,
}

impl ActiveTunnel {
    pub(crate) fn public_url(&self) -> &str {
        &self.public_url
    }

    pub(crate) fn provider_label(&self) -> &'static str {
        self.provider.label()
    }

    pub(crate) fn provider(&self) -> TunnelProvider {
        self.provider
    }

    pub(crate) fn stable_for(&self, duration: Duration) -> bool {
        self.connected_at.elapsed() >= duration
    }

    pub(crate) fn connected_for(&self) -> Duration {
        self.connected_at.elapsed()
    }

    pub(crate) fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        match self.child.as_mut() {
            Some(child) => child.try_wait(),
            None => Ok(None),
        }
    }

    pub(crate) async fn stop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        // Tunnel providers may wrap themselves in shell scripts (the macOS
        // tailscale CLI is a #!/bin/sh shim), so killing only the direct
        // child leaves the real provider binary orphaned. Tunnels spawn in
        // their own process group; take the whole group down.
        #[cfg(unix)]
        if let Some(pid) = child.id() {
            let _ = StdCommand::new("kill")
                .args(["-9", &format!("-{pid}")])
                .stdin(StdStdio::null())
                .stdout(StdStdio::null())
                .stderr(StdStdio::null())
                .status();
        }
        let _ = child.start_kill();
        let _ = child.wait().await;
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

pub(crate) enum TunnelEvent {
    Connected(ActiveTunnel),
    ReconnectFailed {
        provider: TunnelProvider,
        error: String,
    },
}

const PROVIDER_RETRY_INTERVAL: Duration = Duration::from_secs(3);
const PROVIDER_STARTUP_MAX_DELAY: Duration = Duration::from_secs(30);

fn provider_retry_delay(provider: TunnelProvider, attempt: usize) -> Duration {
    let provider_seed = match provider {
        TunnelProvider::Auto => 0,
        TunnelProvider::Cloudflare => 1,
        TunnelProvider::LocalhostRun => 2,
        TunnelProvider::Pinggy => 0,
        TunnelProvider::Tailscale => 3,
        TunnelProvider::DevTunnel => 1,
    };
    let exponent = attempt.saturating_sub(1).min(3) as u32;
    let base = PROVIDER_RETRY_INTERVAL
        .checked_mul(1u32 << exponent)
        .unwrap_or(PROVIDER_STARTUP_MAX_DELAY)
        .min(PROVIDER_STARTUP_MAX_DELAY);
    let jitter = (provider_seed + attempt as u64) % 4;
    base.saturating_add(Duration::from_secs(jitter))
        .min(PROVIDER_STARTUP_MAX_DELAY)
}

/// Starts every candidate provider concurrently in the background. Each
/// provider retries with bounded exponential backoff plus provider-specific
/// deterministic jitter until it becomes reachable, so persistent provider
/// outages do not create a reconnect storm. Every verified tunnel is reported
/// through the returned channel and stays running.
pub(crate) fn spawn_tunnel_supervisor(
    selected: TunnelProvider,
    local_url: &str,
    instance_id: &str,
    install_missing: bool,
    dev_tunnel_id: Option<&str>,
    monitor: TaskMonitor,
    retry_forever: bool,
) -> mpsc::Receiver<TunnelEvent> {
    let (event_tx, event_rx) = mpsc::channel(4);
    let local_url = local_url.to_owned();
    let instance_id = instance_id.to_owned();
    let dev_tunnel_id = dev_tunnel_id.map(str::to_owned);
    tokio::spawn(async move {
        let candidates: Vec<TunnelProvider> = if selected == TunnelProvider::Auto {
            TunnelProvider::auto_candidates()
        } else {
            vec![selected]
        };
        let allow_install = install_missing && selected != TunnelProvider::Auto;
        let (result_tx, mut result_rx) =
            mpsc::unbounded_channel::<(TunnelProvider, Result<ActiveTunnel, String>)>();
        for &provider in &candidates {
            monitor.operator_message(
                OperatorMessageKind::Info,
                "tunnel",
                format!("trying {}", provider.label()),
            );
            let local_url = local_url.clone();
            let instance_id = instance_id.clone();
            let result_tx = result_tx.clone();
            let provider_monitor = monitor.clone();
            let dev_tunnel_id = dev_tunnel_id.clone();
            tokio::spawn(async move {
                let mut attempt = 0usize;
                loop {
                    attempt += 1;
                    match try_start_provider(
                        provider,
                        selected,
                        &local_url,
                        &instance_id,
                        allow_install,
                        dev_tunnel_id.as_deref(),
                        &provider_monitor,
                    )
                    .await
                    {
                        Ok(active) => {
                            let _ = result_tx.send((provider, Ok(active)));
                            return;
                        }
                        Err(error) => {
                            let detail = format!("{error:#}");
                            if !retry_forever {
                                let _ = result_tx.send((provider, Err(detail)));
                                return;
                            }
                            provider_monitor.operator_message(
                                OperatorMessageKind::Warning,
                                "tunnel",
                                format!(
                                    "{} attempt {attempt} failed · retrying in {}s · {}",
                                    provider.label(),
                                    provider_retry_delay(provider, attempt).as_secs(),
                                    truncate_diagnostic(
                                        detail.lines().next().unwrap_or("unknown error"),
                                        160
                                    )
                                ),
                            );
                        }
                    }
                    sleep(provider_retry_delay(provider, attempt)).await;
                }
            });
        }
        drop(result_tx);
        while let Some((provider, result)) = result_rx.recv().await {
            let event = match result {
                Ok(active) => TunnelEvent::Connected(active),
                Err(error) => TunnelEvent::ReconnectFailed { provider, error },
            };
            // The app registers verified endpoints in AuthState before
            // publishing their links. Primary selection stays app-owned.
            if event_tx.send(event).await.is_err() {
                return;
            }
        }
    });
    event_rx
}

async fn try_start_provider(
    provider: TunnelProvider,
    selected: TunnelProvider,
    local_url: &str,
    instance_id: &str,
    allow_install: bool,
    dev_tunnel_id: Option<&str>,
    monitor: &TaskMonitor,
) -> Result<ActiveTunnel> {
    if provider == TunnelProvider::Tailscale {
        if let Some(public_url) = reusable_tailscale_endpoint(instance_id).await {
            monitor.operator_message(
                OperatorMessageKind::Success,
                "tailscale",
                format!("reusing healthy stable endpoint · {public_url}"),
            );
            return Ok(ActiveTunnel {
                child: None,
                public_url,
                provider,
                connected_at: std::time::Instant::now(),
            });
        }
    }
    let start_result = if selected == TunnelProvider::Auto {
        match timeout(
            AUTO_PROVIDER_START_TIMEOUT,
            start_tunnel_provider_once(provider, local_url, allow_install, dev_tunnel_id, monitor),
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
        start_tunnel_provider_once(provider, local_url, allow_install, dev_tunnel_id, monitor).await
    };
    let (mut child, public_url) = start_result?;
    if let Err(error) = verify_tunnel_candidate(&public_url, instance_id).await {
        let _ = child.start_kill();
        let _ = child.wait().await;
        bail!("{} URL was not reachable: {error}", provider.label());
    }
    Ok(ActiveTunnel {
        child: Some(child),
        public_url,
        provider,
        connected_at: std::time::Instant::now(),
    })
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
        let applied = match check_public_endpoint_resilient(&public_url, &instance_id).await {
            Ok(()) => monitor.mark_managed_public_url_check(&public_url, true, None),
            Err(error) => monitor.mark_managed_public_url_check(&public_url, false, Some(error)),
        };
        if !applied {
            return;
        }
        let status = monitor.connection_status();
        let interval = public_health_interval(
            status.public_url_healthy,
            status.public_url_consecutive_failures,
        );
        tokio::select! {
            _ = sleep(interval) => {},
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    return;
                }
            }
        }
    }
}

fn public_health_interval(healthy: Option<bool>, consecutive_failures: u8) -> Duration {
    if healthy == Some(true) && consecutive_failures == 0 {
        PUBLIC_HEALTH_INTERVAL
    } else {
        PUBLIC_RECOVERY_HEALTH_INTERVAL
    }
}

pub(crate) async fn wait_for_public_endpoint(
    public_url: &str,
    instance_id: &str,
    monitor: &TaskMonitor,
) -> Result<(), String> {
    monitor.operator_message(
        OperatorMessageKind::Info,
        "endpoint",
        "verifying this wcode instance",
    );
    let mut last_error = String::new();
    for attempt in 1..=PUBLIC_STARTUP_HEALTH_ATTEMPTS {
        match check_public_endpoint(public_url, instance_id).await {
            Ok(()) => {
                monitor.mark_public_url_check(true, None);
                monitor.operator_message(
                    OperatorMessageKind::Success,
                    "endpoint",
                    "reachable and instance-matched",
                );
                return Ok(());
            }
            Err(error) => {
                last_error = error;
                monitor.mark_public_url_check(false, Some(last_error.clone()));
                monitor.operator_message(
                    OperatorMessageKind::Warning,
                    "endpoint",
                    format!(
                        "attempt {attempt}/{PUBLIC_STARTUP_HEALTH_ATTEMPTS} failed · {}",
                        truncate_diagnostic(&last_error, 180)
                    ),
                );
                if attempt < PUBLIC_STARTUP_HEALTH_ATTEMPTS {
                    sleep(Duration::from_secs(attempt.min(3) as u64)).await;
                }
            }
        }
    }
    Err(last_error)
}

pub(crate) async fn check_public_endpoint_resilient(
    public_url: &str,
    expected_instance_id: &str,
) -> Result<(), String> {
    match check_public_endpoint(public_url, expected_instance_id).await {
        Ok(()) => Ok(()),
        Err(mut last_error) => {
            for _ in 0..PUBLIC_TRANSIENT_RECHECKS {
                sleep(PUBLIC_TRANSIENT_RECHECK_DELAY).await;
                match check_public_endpoint(public_url, expected_instance_id).await {
                    Ok(()) => return Ok(()),
                    Err(error) => last_error = error,
                }
            }
            Err(last_error)
        }
    }
}

pub(crate) async fn check_public_endpoint(
    public_url: &str,
    expected_instance_id: &str,
) -> Result<(), String> {
    let health_url = format!("{public_url}/healthz");
    let mut command = Command::new("curl");
    command
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--connect-timeout",
            "2",
            "--max-time",
            "3",
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
    dev_tunnel_id: Option<&str>,
    monitor: &TaskMonitor,
) -> Result<(Child, String)> {
    match provider {
        TunnelProvider::Auto => bail!("auto is a tunnel selection policy, not a concrete provider"),
        TunnelProvider::Cloudflare => {
            ensure_cloudflared(install_missing, monitor)?;
            start_cloudflared_once(local_url).await
        }
        TunnelProvider::LocalhostRun | TunnelProvider::Pinggy => {
            ensure_ssh()?;
            start_ssh_tunnel_once(provider, local_url).await
        }
        TunnelProvider::Tailscale => {
            monitor.operator_message(
                OperatorMessageKind::Info,
                "tailscale",
                "requires CLI installed · `tailscale up` logged in · Funnel enabled",
            );
            ensure_tailscale()?;
            start_tailscale_funnel_once(local_url, monitor).await
        }
        TunnelProvider::DevTunnel => {
            let tunnel_id = dev_tunnel_id
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    anyhow!(
                        "dev-tunnel requires --dev-tunnel-id with an existing persistent tunnel"
                    )
                })?;
            ensure_devtunnel()?;
            start_devtunnel_once(local_url, tunnel_id, monitor).await
        }
    }
}

fn ensure_tailscale() -> Result<()> {
    if command_succeeds("tailscale", &["version"]) {
        return Ok(());
    }
    bail!(
        "tailscale CLI is unavailable. Requirements: 1) install it from https://tailscale.com/download, 2) log in with `tailscale up`, 3) enable Funnel on your tailnet from the admin console (Tailscale ACL node attribute `funnel`)"
    )
}

async fn start_tailscale_funnel_once(
    local_url: &str,
    monitor: &TaskMonitor,
) -> Result<(Child, String)> {
    let public_url = tailscale_funnel_url()?;
    reclaim_stale_funnel(local_url, monitor);
    let mut command = Command::new("tailscale");
    command
        .args(["funnel", local_url])
        .stdin(std::process::Stdio::null())
        .stdout(StdStdio::piped())
        .stderr(StdStdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .spawn()
        .context("failed to start tailscale funnel")?;
    let recent_logs = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    // Funnel prints setup guidance (for example the "not enabled" notice and
    // its enable URL) on stdout, so both streams must be drained.
    {
        let (line_tx, mut line_rx) = mpsc::unbounded_channel::<String>();
        if let Some(stdout) = child.stdout.take() {
            spawn_tunnel_output_reader(stdout, line_tx.clone(), "tailscale");
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_tunnel_output_reader(stderr, line_tx.clone(), "tailscale");
        }
        drop(line_tx);
        let logs = recent_logs.clone();
        tokio::spawn(async move {
            while let Some(line) = line_rx.recv().await {
                let mut logs = logs.lock().expect("tunnel log lock poisoned");
                if logs.len() >= 16 {
                    logs.remove(0);
                }
                logs.push(line);
            }
        });
    }
    let log_text = |logs: &[String]| logs.join("\n");
    // Funnel may need a moment to provision its certificate; wait until it
    // serves, exits, or reports that the tailnet has not enabled Funnel.
    for _ in 0..25 {
        match child.try_wait() {
            Ok(Some(status)) => bail!(
                "tailscale funnel exited with {status}: {}",
                log_text(&recent_logs.lock().expect("tunnel log lock poisoned"))
            ),
            Err(error) => bail!("tailscale funnel status check failed: {error}"),
            Ok(None) => {}
        }
        let logs = recent_logs
            .lock()
            .expect("tunnel log lock poisoned")
            .clone();
        if logs
            .iter()
            .any(|line| line.to_lowercase().contains("not enabled"))
        {
            let _ = child.start_kill();
            let _ = child.wait().await;
            let enable_url = logs
                .iter()
                .find_map(|line| {
                    let start = line.find("https://")?;
                    Some(line[start..].trim().to_owned())
                })
                .unwrap_or_default();
            bail!("tailscale Funnel is not enabled on your tailnet; enable it at {enable_url}");
        }
        if funnel_serving(&public_url).await {
            return Ok((child, public_url));
        }
        sleep(Duration::from_secs(1)).await;
    }
    let _ = child.start_kill();
    let _ = child.wait().await;
    bail!(
        "tailscale funnel did not become reachable within 25 seconds: {}",
        log_text(&recent_logs.lock().expect("tunnel log lock poisoned"))
    );
}

async fn funnel_serving(public_url: &str) -> bool {
    let health_url = format!("{public_url}/healthz");
    let output = Command::new("curl")
        .args([
            "--silent",
            "--output",
            "/dev/null",
            "--write-out",
            "%{http_code}",
            "--max-time",
            "3",
            &health_url,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(StdStdio::piped())
        .kill_on_drop(true)
        .output()
        .await;
    matches!(&output, Ok(result) if result.status.success()
        && String::from_utf8_lossy(&result.stdout).trim() != "000")
}

/// A hard-killed wcode can leave an orphaned `tailscale funnel` child that
/// still holds the node's only 443 listener, permanently blocking every
/// funnel attempt of the next run. Reclaim orphans that forward to this
/// exact local URL; anything else belongs to another operator process and
/// must not be touched.
fn reclaim_stale_funnel(local_url: &str, monitor: &TaskMonitor) {
    let Ok(output) = StdCommand::new("ps")
        .args(["-axo", "pid=,ppid=,command="])
        .stdin(StdStdio::null())
        .stdout(StdStdio::piped())
        .stderr(StdStdio::null())
        .output()
    else {
        return;
    };
    let processes = String::from_utf8_lossy(&output.stdout);
    for line in processes.lines() {
        let mut fields = line.split_whitespace();
        let (Some(pid), Some(ppid)) = (fields.next(), fields.next()) else {
            continue;
        };
        if ppid != "1" || !line.contains("tailscale funnel") || !line.contains(local_url) {
            continue;
        }
        let Ok(pid) = pid.parse::<i32>() else {
            continue;
        };
        monitor.operator_message(
            OperatorMessageKind::Warning,
            "tailscale",
            format!("reclaiming stale funnel process {pid} for {local_url}"),
        );
        let _ = StdCommand::new("kill")
            .arg(pid.to_string())
            .stdin(StdStdio::null())
            .stdout(StdStdio::null())
            .stderr(StdStdio::null())
            .status();
    }
}

fn tailscale_funnel_url() -> Result<String> {
    let output = StdCommand::new("tailscale")
        .args(["status", "--json"])
        .stdin(StdStdio::null())
        .stderr(StdStdio::null())
        .output()
        .context("failed to run `tailscale status`")?;
    let payload: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("tailscale status returned invalid JSON")?;
    let dns_name = payload["Self"]["DNSName"]
        .as_str()
        .unwrap_or_default()
        .trim_end_matches('.');
    if dns_name.is_empty() {
        bail!("tailscale is not logged in; run `tailscale up` first");
    }
    Ok(format!("https://{dns_name}"))
}

async fn reusable_tailscale_endpoint(instance_id: &str) -> Option<String> {
    let public_url = tokio::task::spawn_blocking(tailscale_funnel_url)
        .await
        .ok()?
        .ok()?;
    check_public_endpoint_resilient(&public_url, instance_id)
        .await
        .ok()?;
    Some(public_url)
}

pub(crate) async fn recover_existing_stable_endpoint(
    selected: TunnelProvider,
    instance_id: &str,
) -> Option<(TunnelProvider, String)> {
    if !matches!(selected, TunnelProvider::Auto | TunnelProvider::Tailscale) {
        return None;
    }
    reusable_tailscale_endpoint(instance_id)
        .await
        .map(|public_url| (TunnelProvider::Tailscale, public_url))
}

fn ensure_ssh() -> Result<()> {
    if command_succeeds("ssh", &["-V"]) {
        return Ok(());
    }
    bail!("OpenSSH client is unavailable; localhost.run and Pinggy require `ssh` on PATH")
}

async fn verify_tunnel_candidate(public_url: &str, instance_id: &str) -> Result<(), String> {
    let mut last_error = String::new();
    for attempt in 1..=3 {
        match check_public_endpoint(public_url, instance_id).await {
            Ok(()) => return Ok(()),
            Err(error) => last_error = error,
        }
        if attempt < 3 {
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
    #[cfg(unix)]
    command.process_group(0);
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
    let public_url = match timeout(Duration::from_secs(10), wait_for_url).await {
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

#[cfg(test)]
#[path = "../../../tests/unit/runtime/tunnel/mod.rs"]
mod tests;
