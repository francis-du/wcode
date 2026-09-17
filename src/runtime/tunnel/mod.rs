use crate::monitor::{OperatorMessageKind, TaskMonitor};
use anyhow::{anyhow, bail, Context, Result};
use clap::ValueEnum;
use std::process::{Command as StdCommand, Stdio as StdStdio};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{sleep, timeout, Duration};
use url::{Host, Url};

#[path = "health.rs"]
mod health;
pub(crate) use health::{
    check_public_endpoint, check_public_endpoint_resilient, wait_for_public_endpoint,
};
#[cfg(test)]
use health::{validate_health_response, PUBLIC_HEALTH_PARALLELISM};

#[path = "cloudflare.rs"]
mod cloudflare;
pub(crate) use cloudflare::provider_failure_cooldown;
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

// Own the process group from spawn, including startup and cancellation paths.
struct TunnelChild {
    child: Child,
    #[cfg(unix)]
    group: Option<i32>,
}

impl TunnelChild {
    fn spawn(command: &mut Command) -> std::io::Result<Self> {
        command.kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let child = command.spawn()?;
        Ok(Self {
            #[cfg(unix)]
            group: child.id().map(|pid| pid as i32),
            child,
        })
    }

    fn start_kill(&mut self) -> std::io::Result<()> {
        #[cfg(unix)]
        if let Some(group) = self.group.take() {
            // SAFETY: this is the dedicated group of a child we spawned.
            unsafe { libc::kill(-group, libc::SIGKILL) };
        }
        self.child.start_kill()
    }
}

impl std::ops::Deref for TunnelChild {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.child
    }
}

impl std::ops::DerefMut for TunnelChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

impl Drop for TunnelChild {
    fn drop(&mut self) {
        let _ = self.start_kill();
    }
}

pub(crate) struct ActiveTunnel {
    // Stable providers can survive a wcode restart independently of the
    // process that originally configured them. Reusing an instance-matched
    // endpoint avoids killing and recreating a healthy funnel during startup.
    child: Option<TunnelChild>,
    // Captured by the publisher, never borrowed from a later same-URL owner.
    pub(crate) endpoint_epoch: Option<u64>,
    public_url: String,
    provider: TunnelProvider,
    connected_at: std::time::Instant,
}

impl ActiveTunnel {
    #[cfg(test)]
    pub(crate) fn test_fixture(provider: TunnelProvider, public_url: String) -> Self {
        Self {
            child: None,
            endpoint_epoch: None,
            public_url,
            provider,
            connected_at: std::time::Instant::now(),
        }
    }

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
    Connected(Box<ActiveTunnel>),
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
                    let result = tokio::select! {
                        _ = result_tx.closed() => return,
                        result = try_start_provider(
                        provider,
                        &local_url,
                        &instance_id,
                        allow_install,
                        dev_tunnel_id.as_deref(),
                        &provider_monitor,
                    ) => result,
                    };
                    let delay = match result {
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
                            let delay = provider_retry_delay(provider, attempt)
                                .max(provider_failure_cooldown(provider, &detail));
                            provider_monitor.mark_tunnel_retry(
                                provider.label(),
                                attempt.min(u32::MAX as usize) as u32,
                                delay >= Duration::from_secs(300),
                                delay,
                                false,
                            );
                            provider_monitor.operator_message(
                                OperatorMessageKind::Warning,
                                "tunnel",
                                format!(
                                    "{} attempt {attempt} failed · retrying in {}s · {}",
                                    provider.label(),
                                    delay.as_secs(),
                                    truncate_diagnostic(&detail, 500)
                                ),
                            );
                            delay
                        }
                    };
                    tokio::select! {
                        _ = result_tx.closed() => return,
                        _ = sleep(delay) => {}
                    }
                }
            });
        }
        drop(result_tx);
        loop {
            let received = tokio::select! {
                _ = event_tx.closed() => return,
                received = result_rx.recv() => received,
            };
            let Some((provider, result)) = received else {
                return;
            };
            let event = match result {
                Ok(active) => TunnelEvent::Connected(Box::new(active)),
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
                endpoint_epoch: None,
                public_url,
                provider,
                connected_at: std::time::Instant::now(),
            });
        }
    }
    // Each provider owns its startup deadline and diagnostics. An outer auto
    // deadline used to cancel Funnel before certificate provisioning finished.
    let (mut child, public_url) =
        start_tunnel_provider_once(provider, local_url, allow_install, dev_tunnel_id, monitor)
            .await?;
    if let Err(error) = verify_tunnel_candidate(&public_url, instance_id).await {
        let _ = child.start_kill();
        let _ = child.wait().await;
        bail!("{} URL was not reachable: {error}", provider.label());
    }
    Ok(ActiveTunnel {
        child: Some(child),
        endpoint_epoch: None,
        public_url,
        provider,
        connected_at: std::time::Instant::now(),
    })
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
) -> Result<(TunnelChild, String)> {
    match provider {
        TunnelProvider::Auto => bail!("auto is a tunnel selection policy, not a concrete provider"),
        TunnelProvider::Cloudflare => {
            let dependency_monitor = monitor.clone();
            run_blocking_tunnel_setup("cloudflared dependency setup", move || {
                ensure_cloudflared(install_missing, &dependency_monitor)
            })
            .await?;
            start_cloudflared_once(local_url).await
        }
        TunnelProvider::LocalhostRun | TunnelProvider::Pinggy => {
            run_blocking_tunnel_setup("OpenSSH dependency check", ensure_ssh).await?;
            start_ssh_tunnel_once(provider, local_url).await
        }
        TunnelProvider::Tailscale => {
            monitor.operator_message(
                OperatorMessageKind::Info,
                "tailscale",
                "requires CLI installed · `tailscale up` logged in · Funnel enabled",
            );
            run_blocking_tunnel_setup("tailscale dependency check", ensure_tailscale).await?;
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
            run_blocking_tunnel_setup("devtunnel dependency check", ensure_devtunnel).await?;
            start_devtunnel_once(local_url, tunnel_id, monitor).await
        }
    }
}

async fn run_blocking_tunnel_setup<T, F>(label: &'static str, operation: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .with_context(|| format!("{label} worker failed"))?
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
    _monitor: &TaskMonitor,
) -> Result<(TunnelChild, String)> {
    let public_url = tailscale_funnel_url().await?;
    let mut command = Command::new("tailscale");
    command
        .args(["funnel", local_url])
        .stdin(std::process::Stdio::null())
        .stdout(StdStdio::piped())
        .stderr(StdStdio::piped())
        .kill_on_drop(true);
    let mut child = TunnelChild::spawn(&mut command).context("failed to start tailscale funnel")?;
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
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while tokio::time::Instant::now() < deadline {
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
        // The CLI publishes the URL once Serve is configured. The common
        // instance-health gate then verifies it once with the full cold TLS budget.
        if logs.iter().any(|line| line.trim().starts_with(&public_url))
            || funnel_serving(&public_url).await
        {
            return Ok((child, public_url));
        }
        sleep(Duration::from_secs(1)).await;
    }
    let _ = child.start_kill();
    let _ = child.wait().await;
    bail!(
        "tailscale funnel did not become reachable within 60 seconds: {}",
        log_text(&recent_logs.lock().expect("tunnel log lock poisoned"))
    );
}

async fn funnel_serving(public_url: &str) -> bool {
    let health_url = format!("{public_url}/healthz/probe");
    let output = Command::new("curl")
        .args([
            "--silent",
            "--output",
            "/dev/null",
            "--write-out",
            "%{http_code}",
            "--max-time",
            "15",
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

async fn tailscale_funnel_url() -> Result<String> {
    let output = timeout(
        Duration::from_secs(10),
        Command::new("tailscale")
            .args(["status", "--json"])
            .stdin(StdStdio::null())
            .stderr(StdStdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("tailscale status timed out")?
    .context("failed to run `tailscale status`")?;
    if !output.status.success() {
        bail!("tailscale status failed: {}", output.status);
    }
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
    let public_url = tailscale_funnel_url().await.ok()?;
    check_public_endpoint_resilient(&public_url, instance_id)
        .await
        .ok()?;
    Some(public_url)
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
) -> Result<(TunnelChild, String)> {
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
    let mut child = TunnelChild::spawn(&mut command)
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
