mod auth;
mod code_index;
mod harness;
mod mcp;
mod monitor;
mod power;
mod runtime_control;
mod workspace;

use anyhow::{bail, Context, Result};
use auth::AuthState;
use clap::{ArgAction, Parser, Subcommand};
use harness::ToolHarness;
use mcp::AppState;
use monitor::{MonitorConfig, MonitorRenderer, TaskMonitor};
use std::future::pending;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::{Command as StdCommand, Stdio as StdStdio};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;
use tokio::process::{Child, Command};
use tokio::sync::{oneshot, watch};
use tokio::task::AbortHandle;
use tokio::time::{sleep, timeout, Duration};
use tracing_subscriber::EnvFilter;
use url::{Host, Url};
use workspace::{WorkspaceSecurity, Workspaces};

pub(crate) const CHATGPT_CONNECTOR_SETUP_URL: &str =
    "https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins";
pub(crate) const GROK_CONNECTOR_SETUP_URL: &str = "https://grok.com/connectors";
pub(crate) const CLAUDE_CONNECTOR_SETUP_URL: &str = "https://claude.ai/customize/connectors";
pub(crate) const MISTRAL_CONNECTOR_SETUP_URL: &str = "https://chat.mistral.ai/";
pub(crate) const PROJECT_URL: &str = "https://github.com/francis-du/wcode";
pub(crate) const DOCS_URL: &str = "https://francis-du.github.io/wcode/";
pub(crate) const AUTHOR_URL: &str = "https://github.com/francis-du";
pub(crate) const AUTHOR_HANDLE: &str = "@francis-du";
const DEFAULT_MIN_PARALLEL_TOOLS: usize = 64;
const DEFAULT_MAX_PARALLEL_TOOLS: usize = 128;
const PUBLIC_HEALTH_INTERVAL: Duration = Duration::from_secs(25);
const PUBLIC_HEALTH_TIMEOUT: Duration = Duration::from_secs(5);
const PUBLIC_STARTUP_HEALTH_ATTEMPTS: usize = 6;
const DEFAULT_INPUT_TOKEN_PRICE_PER_MILLION_USD: f64 = 5.0;
const HELP_BANNER: &str = r#"
 __        __   ____    ___    ____    _____
 \ \      / /  / ___|  / _ \  |  _ \  | ____|
  \ \ /\ / /  | |     | | | | | | | | |  _|
   \ V  V /   | |___  | |_| | | |_| | | |___
    \_/\_/     \____|  \___/  |____/  |_____|

 Lightweight Code Agent plugin for Remote MCP clients
"#;
const HELP_FOOTER: &str = "Author  @francis-du  https://github.com/francis-du\nProject https://github.com/francis-du/wcode\nDocs    https://francis-du.github.io/wcode/";

fn default_max_parallel_tools() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(8)
        .saturating_mul(8)
        .clamp(DEFAULT_MIN_PARALLEL_TOOLS, DEFAULT_MAX_PARALLEL_TOOLS)
}

fn normalize_public_url(value: &str) -> Result<String> {
    let url = Url::parse(value).context("--public-url must be a valid absolute URL")?;
    if url.cannot_be_a_base()
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!(
            "--public-url must be a base URL without user information, a query string, or a fragment"
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

#[derive(Clone, Copy)]
struct SetupGuideOptions {
    local_only: bool,
    max_parallel_tools: usize,
    input_token_price_per_million_usd: f64,
    security: WorkspaceSecurity,
}

struct AbortTaskOnDrop(AbortHandle);

impl Drop for AbortTaskOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "wcode",
    author,
    version,
    about = "Lightweight Code Agent plugin for Remote MCP clients",
    before_help = HELP_BANNER,
    after_help = HELP_FOOTER
)]
struct Args {
    #[command(subcommand)]
    command: Option<ControlCommand>,

    /// Local code directory exposed to connected MCP clients. Repeat to expose multiple roots.
    #[arg(short = 'w', long, value_name = "PATH", default_value = ".")]
    workspace: Vec<PathBuf>,

    /// Local interface for the MCP server.
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    host: String,

    /// Local port for the MCP server.
    #[arg(short = 'p', long, default_value_t = 8765)]
    port: u16,

    /// Use an existing public base URL instead of starting Cloudflare Tunnel.
    #[arg(long)]
    public_url: Option<String>,

    /// Keep the server local and do not start a public tunnel.
    #[arg(long)]
    no_tunnel: bool,

    /// Disable file modification tools.
    #[arg(long = "read-only", action = ArgAction::SetFalse, default_value_t = true)]
    allow_write: bool,

    /// Disable build/test/status command execution.
    #[arg(long = "no-exec", action = ArgAction::SetFalse, default_value_t = true)]
    allow_exec: bool,

    /// Allow arbitrary model-facing repository-aware commands beyond the Harness verification allowlist.
    #[arg(long)]
    allow_risky_exec: bool,

    /// Allow replacements that empty a file or remove most of its content.
    #[arg(long)]
    allow_destructive_writes: bool,

    /// Allow nested or parent/child workspace roots in one process.
    #[arg(long)]
    allow_overlapping_workspaces: bool,

    /// Allow exposing a filesystem root or the current user's home directory.
    #[arg(long)]
    allow_broad_workspace: bool,

    /// Global cap for concurrently running child tool bodies.
    #[arg(short = 'j', long, default_value_t = default_max_parallel_tools())]
    max_parallel_tools: usize,

    /// Estimated USD cost per million input tokens used for the TUI savings estimate.
    #[arg(long, default_value_t = DEFAULT_INPUT_TOKEN_PRICE_PER_MILLION_USD)]
    input_token_price_per_million_usd: f64,

    /// Disable the live terminal task monitor.
    #[arg(long = "no-monitor", action = ArgAction::SetFalse, default_value_t = true)]
    monitor: bool,

    /// Do not offer to install a missing cloudflared dependency.
    #[arg(long)]
    no_install: bool,

    /// Do not open wcode's client-neutral setup hub after startup.
    #[arg(long = "no-open", action = ArgAction::SetFalse, default_value_t = true)]
    open_setup: bool,

    /// Allow idle system sleep while wcode is running.
    #[arg(long)]
    allow_sleep: bool,

    /// Deprecated alias for --no-open, kept for CLI compatibility.
    #[arg(
        long = "no-install-chatgpt",
        action = ArgAction::SetFalse,
        default_value_t = true,
        hide = true
    )]
    legacy_open_setup: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Subcommand)]
enum ControlCommand {
    /// Restart the running wcode instance with its original arguments.
    Restart,
    /// Stop the running wcode instance.
    Stop,
}

#[tokio::main]
async fn main() -> Result<()> {
    if let Some(delay) = std::env::var_os("WCODE_INTERNAL_RESTART_DELAY_MS") {
        std::env::remove_var("WCODE_INTERNAL_RESTART_DELAY_MS");
        let delay = delay
            .to_string_lossy()
            .parse::<u64>()
            .unwrap_or(750)
            .min(5_000);
        sleep(Duration::from_millis(delay)).await;
    }
    let args = Args::parse();
    if let Some(command) = args.command.as_ref() {
        let action = match *command {
            ControlCommand::Restart => runtime_control::ControlAction::Restart,
            ControlCommand::Stop => runtime_control::ControlAction::Stop,
        };
        runtime_control::send(action)?;
        println!("Requested wcode {}.", action.as_str());
        return Ok(());
    }
    let open_setup = args.open_setup && args.legacy_open_setup;
    if !args.input_token_price_per_million_usd.is_finite()
        || args.input_token_price_per_million_usd < 0.0
    {
        bail!("--input-token-price-per-million-usd must be a finite non-negative number");
    }
    let tui_active = args.monitor && io::stdout().is_terminal();
    if tui_active {
        tracing_subscriber::fmt()
            .compact()
            .without_time()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("wcode=warn")),
            )
            .with_target(false)
            .with_writer(io::sink)
            .init();
    } else {
        tracing_subscriber::fmt()
            .compact()
            .without_time()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("wcode=info")),
            )
            .with_target(false)
            .init();
    }
    let _awake_guard = if args.allow_sleep {
        None
    } else {
        match power::prevent_idle_sleep() {
            Ok(guard) => Some(guard),
            Err(error) => {
                eprintln!("  ! keep-awake   unavailable: {error}");
                None
            }
        }
    };
    let security = WorkspaceSecurity {
        allow_risky_exec: args.allow_risky_exec,
        allow_destructive_writes: args.allow_destructive_writes,
        allow_overlapping_workspaces: args.allow_overlapping_workspaces,
        allow_broad_workspace: args.allow_broad_workspace,
    };
    let workspaces = Workspaces::new_with_security(
        &args.workspace,
        args.allow_write,
        args.allow_exec,
        security,
    )?;
    let harness = ToolHarness::new(args.max_parallel_tools)?;
    let monitor = TaskMonitor::new(workspaces.roots().into_iter().map(|(id, _)| id));
    let listener = TcpListener::bind(format!("{}:{}", args.host, args.port))
        .await
        .with_context(|| format!("cannot bind {}:{}", args.host, args.port))?;
    let local_url = format!("http://{}:{}", args.host, listener.local_addr()?.port());
    let (control_router, mut control_rx, runtime_registration) =
        runtime_control::register(listener.local_addr()?)?;
    let auth = Arc::new(AuthState::new_with_monitor(
        local_url.clone(),
        monitor.clone(),
    ));
    let app_state = Arc::new(AppState {
        auth: auth.clone(),
        workspaces: workspaces.clone(),
        harness: harness.clone(),
        monitor: monitor.clone(),
    });
    let app = auth::router(auth.clone())
        .merge(mcp::router(app_state))
        .merge(control_router);
    let mut server_task = tokio::spawn(async move { axum::serve(listener, app).await });
    let _server_abort = AbortTaskOnDrop(server_task.abort_handle());

    let mut tunnel: Option<Child> = None;
    let public_url = if let Some(url) = args.public_url.as_deref() {
        let url = normalize_public_url(url)?;
        monitor.mark_public_endpoint("external", None);
        url
    } else if args.no_tunnel {
        monitor.mark_public_endpoint("local-only", None);
        local_url.clone()
    } else {
        ensure_cloudflared(!args.no_install)?;
        let (child, url) = start_cloudflared(&local_url).await?;
        monitor.mark_public_endpoint("quick-tunnel", Some(true));
        tunnel = Some(child);
        url
    };
    auth.set_public_url(public_url.clone());

    if !args.no_tunnel {
        tokio::select! {
            result = &mut server_task => {
                result.context("local MCP server task failed")??;
                bail!("local MCP server stopped during public endpoint startup");
            }
            result = wait_for_public_endpoint(
                &public_url,
                auth.instance_id(),
                &monitor,
            ) => {
                if let Err(error) = result {
                    if let Some(mut child) = tunnel.take() {
                        let _ = child.start_kill();
                        let _ = child.wait().await;
                    }
                    bail!("public endpoint did not become ready: {error}");
                }
            }
        }
    }

    let (public_health_stop, public_health_stop_rx) = watch::channel(false);
    let public_health_task = if args.no_tunnel {
        None
    } else {
        Some(tokio::spawn(public_endpoint_health_loop(
            public_url.clone(),
            auth.instance_id().to_owned(),
            monitor.clone(),
            public_health_stop_rx,
        )))
    };

    let monitor_config = MonitorConfig {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        instance_id: auth.instance_id().to_owned(),
        local_health_url: format!("{local_url}/healthz"),
        mcp_url: format!("{public_url}/mcp"),
        setup_url: public_url.clone(),
        project_url: PROJECT_URL.to_owned(),
        author_url: AUTHOR_URL.to_owned(),
        author_handle: AUTHOR_HANDLE.to_owned(),
        pairing_code: auth.pairing_code().to_owned(),
        max_parallel: harness.max_parallel(),
        input_token_price_per_million_usd: args.input_token_price_per_million_usd,
        workspaces: workspaces
            .roots()
            .into_iter()
            .map(|(id, root)| {
                let is_default = id == workspaces.default_id();
                (id, root.display().to_string(), is_default)
            })
            .collect(),
    };
    let mcp_url = format!("{public_url}/mcp");
    if open_setup {
        open_setup_hub(&public_url, &mcp_url);
    }
    let renderer = monitor.spawn_renderer(monitor_config, args.monitor);
    if renderer.is_none() {
        print_setup_guide(
            &workspaces,
            &local_url,
            &public_url,
            auth.pairing_code(),
            SetupGuideOptions {
                local_only: args.no_tunnel,
                max_parallel_tools: harness.max_parallel(),
                input_token_price_per_million_usd: args.input_token_price_per_million_usd,
                security,
            },
        );
    }

    let monitor_interrupt = renderer.as_ref().map(MonitorRenderer::interrupt_receiver);
    let mut restart_requested = false;
    let mut server_task_finished = false;
    loop {
        tokio::select! {
            result = &mut server_task => {
                result.context("local MCP server task failed")??;
                server_task_finished = true;
                break;
            },
            _ = tokio::signal::ctrl_c() => break,
            _ = wait_for_monitor_interrupt(monitor_interrupt.clone()) => break,
            Some(action) = control_rx.recv() => {
                restart_requested = action == runtime_control::ControlAction::Restart;
                break;
            },
            _ = sleep(Duration::from_secs(1)) => {
                let health_failed = monitor.connection_status().public_url_healthy == Some(false);
                let stopped = if health_failed {
                    if let Some(mut child) = tunnel.take() {
                        let _ = child.start_kill();
                        let _ = child.wait().await;
                    }
                    Some("public endpoint health failed three times; restarting wcode".to_owned())
                } else {
                    match tunnel.as_mut().map(|child| child.try_wait()) {
                        Some(Ok(Some(status))) => Some(format!("cloudflared exited with {status}")),
                        Some(Err(error)) => Some(format!("cloudflared status check failed: {error}")),
                        _ => None,
                    }
                };
                if let Some(reason) = stopped {
                    monitor.mark_tunnel_stopped(reason);
                    tunnel = None;
                    restart_requested = true;
                    break;
                }
            },
        }
    }
    let _ = public_health_stop.send(true);
    if let Some(task) = public_health_task {
        let _ = task.await;
    }
    if let Some(renderer) = renderer {
        renderer.stop().await;
    }
    println!("  ◼ wcode stopped");

    if let Some(mut child) = tunnel.take() {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
    if !server_task_finished {
        server_task.abort();
        let _ = server_task.await;
    }
    if restart_requested {
        drop(runtime_registration);
        drop(_awake_guard);
        drop(_server_abort);
        spawn_replacement()?;
    }
    Ok(())
}

async fn wait_for_monitor_interrupt(receiver: Option<watch::Receiver<bool>>) {
    let Some(mut receiver) = receiver else {
        pending::<()>().await;
        return;
    };
    loop {
        if *receiver.borrow() {
            return;
        }
        if receiver.changed().await.is_err() {
            pending::<()>().await;
            return;
        }
    }
}

async fn public_endpoint_health_loop(
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

async fn wait_for_public_endpoint(
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

fn validate_health_response(body: &[u8], expected_instance_id: &str) -> Result<(), String> {
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

fn open_setup_hub(setup_url: &str, mcp_url: &str) {
    println!("  ↗ setup        opening wcode setup hub");
    println!("  · MCP URL      {mcp_url}");
    let mut command = if cfg!(target_os = "macos") {
        let mut command = StdCommand::new("open");
        command.arg(setup_url);
        command
    } else if cfg!(target_os = "windows") {
        let mut command = StdCommand::new("explorer.exe");
        command.arg(setup_url);
        command
    } else {
        let mut command = StdCommand::new("xdg-open");
        command.arg(setup_url);
        command
    };
    if let Err(error) = command
        .stdin(StdStdio::null())
        .stdout(StdStdio::null())
        .stderr(StdStdio::null())
        .spawn()
    {
        eprintln!("  ! setup        could not open the browser ({error}); visit {setup_url}");
    }
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

async fn start_cloudflared(local_url: &str) -> Result<(Child, String)> {
    println!("  ↗ tunnel       requesting HTTPS endpoint");
    let mut last_error = String::new();
    for attempt in 1..=3 {
        match start_cloudflared_once(local_url).await {
            Ok(result) => {
                println!("  ✓ tunnel       connected");
                return Ok(result);
            }
            Err(error) => {
                last_error = format!("{error:#}");
                let summary = last_error.lines().next().unwrap_or("unknown error");
                eprintln!("  ! tunnel       attempt {attempt}/3 failed · {summary}");
                if attempt < 3 {
                    sleep(Duration::from_secs(attempt * 2)).await;
                }
            }
        }
    }
    bail!(
        "Cloudflare Quick Tunnel failed after 3 attempts.\n\
         Last error: {last_error}\n\
         Check network/VPN access to https://api.trycloudflare.com, or use a stable reverse proxy and pass \
         `--public-url https://your-host.example`."
    )
}

#[cfg(unix)]
fn spawn_replacement() -> Result<()> {
    use std::os::unix::process::CommandExt;
    let executable = std::env::current_exe().context("cannot locate wcode for restart")?;
    println!("  ↻ wcode restarting");
    let error = StdCommand::new(executable)
        .args(std::env::args_os().skip(1))
        .exec();
    Err(error).context("cannot replace the wcode process during restart")
}

#[cfg(windows)]
fn spawn_replacement() -> Result<()> {
    let executable = std::env::current_exe().context("cannot locate wcode for restart")?;
    println!("  ↻ wcode restarting");
    StdCommand::new(executable)
        .args(std::env::args_os().skip(1))
        .env("WCODE_INTERNAL_RESTART_DELAY_MS", "750")
        .stdin(StdStdio::inherit())
        .stdout(StdStdio::inherit())
        .stderr(StdStdio::inherit())
        .spawn()
        .context("cannot restart wcode")?;
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn spawn_replacement() -> Result<()> {
    bail!("automatic restart is supported on Unix-like systems and Windows")
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
                    if let Some(url) = extract_tunnel_url(&line) {
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
    let public_url = match timeout(Duration::from_secs(30), url_receiver).await {
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
            bail!("timed out after 30 seconds waiting for Cloudflare Tunnel URL");
        }
    };
    Ok((child, public_url))
}

fn extract_tunnel_url(line: &str) -> Option<String> {
    for (start, _) in line.match_indices("https://") {
        let candidate = line[start..]
            .split(|ch: char| {
                ch.is_whitespace()
                    || matches!(ch, '|' | '`' | '"' | '<' | '>' | ')' | ']' | '}' | ',')
            })
            .next()
            .unwrap_or_default()
            .trim_end_matches('/');
        let Ok(url) = url::Url::parse(candidate) else {
            continue;
        };
        let Some(host) = url.host_str() else { continue };
        if host.ends_with(".trycloudflare.com") && host != "api.trycloudflare.com" {
            return Some(candidate.to_owned());
        }
    }
    None
}

fn print_setup_guide(
    workspaces: &Workspaces,
    local_url: &str,
    public_url: &str,
    pairing_code: &str,
    options: SetupGuideOptions,
) {
    let SetupGuideOptions {
        local_only,
        max_parallel_tools,
        input_token_price_per_million_usd,
        security,
    } = options;
    println!(
        "\n╭─ wcode {} ─────────────────────────────────────────",
        env!("CARGO_PKG_VERSION")
    );
    println!("│");
    println!("│  MCP        {public_url}/mcp");
    println!("│  Local      {local_url}");
    println!("│  Verify code  {pairing_code}");
    println!("│  Slots cap  {max_parallel_tools} concurrent child tasks");
    println!("│  Token EST  ~4 bytes/token · ${input_token_price_per_million_usd:.2}/M input");
    println!(
        "│  Security   risky-exec {} · destructive {} · overlap {} · broad {}",
        if security.allow_risky_exec {
            "on"
        } else {
            "off"
        },
        if security.allow_destructive_writes {
            "on"
        } else {
            "off"
        },
        if security.allow_overlapping_workspaces {
            "on"
        } else {
            "off"
        },
        if security.allow_broad_workspace {
            "on"
        } else {
            "off"
        },
    );
    println!("│  Fan-out    parallel_tools / review_changes / verify_project");
    println!("│");
    println!("│  Workspaces");
    for (id, root) in workspaces.roots() {
        let default = if id == workspaces.default_id() {
            "  default"
        } else {
            ""
        };
        println!("│    • {id:<16} {}{default}", root.display());
    }
    println!("│");
    if local_only {
        println!("│  Local-only mode: remote MCP clients cannot reach this endpoint.");
        println!("│  Restart without --no-tunnel or pass --public-url.");
    } else {
        println!("│  Connect");
        println!("│    1  Open the wcode setup hub: {public_url}");
        println!("│    2  Pick Grok, Claude, ChatGPT, Mistral, or Other MCP client");
        println!("│    3  The shared MCP URL is {public_url}/mcp");
        println!("│    4  Auth  OAuth · enter pairing code {pairing_code}");
    }
    println!("│");
    println!("│  Project    {PROJECT_URL}");
    println!("│  Author     {AUTHOR_HANDLE} · {AUTHOR_URL}");
    println!("╰─ Ctrl-C to stop ─────────────────────────────────────\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn help_shows_ascii_brand_and_author_links() {
        let help = Args::command().render_long_help().to_string();
        assert!(help.contains("__        __"));
        assert!(help.contains("Lightweight Code Agent plugin"));
        assert!(help.contains("https://github.com/francis-du"));
        assert!(help.contains("https://github.com/francis-du/wcode"));
        assert!(help.contains("https://francis-du.github.io/wcode/"));
    }

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
    fn adaptive_default_parallelism_is_large_and_bounded() {
        let default = default_max_parallel_tools();
        assert!((DEFAULT_MIN_PARALLEL_TOOLS..=DEFAULT_MAX_PARALLEL_TOOLS).contains(&default));

        let args = Args::try_parse_from(["wcode"]).expect("default CLI arguments parse");
        assert_eq!(args.max_parallel_tools, default);
        assert_eq!(
            args.input_token_price_per_million_usd,
            DEFAULT_INPUT_TOKEN_PRICE_PER_MILLION_USD
        );
        assert!(!args.allow_risky_exec);
        assert!(!args.allow_destructive_writes);
        assert!(!args.allow_overlapping_workspaces);
        assert!(!args.allow_broad_workspace);
        assert!(!args.allow_sleep);
        assert!(args.command.is_none());

        let stop = Args::try_parse_from(["wcode", "stop"]).expect("stop command parses");
        assert_eq!(stop.command, Some(ControlCommand::Stop));
        let restart = Args::try_parse_from(["wcode", "restart"]).expect("restart command parses");
        assert_eq!(restart.command, Some(ControlCommand::Restart));

        let overridden = Args::try_parse_from([
            "wcode",
            "--max-parallel-tools",
            "192",
            "--input-token-price-per-million-usd",
            "10",
            "--allow-risky-exec",
        ])
        .expect("explicit performance, economics, and trust overrides parse");
        assert_eq!(overridden.max_parallel_tools, 192);
        assert_eq!(overridden.input_token_price_per_million_usd, 10.0);
        assert!(overridden.allow_risky_exec);
    }

    #[test]
    fn public_url_requires_https_or_loopback_http() {
        assert_eq!(
            normalize_public_url("https://example.com/gateway/").unwrap(),
            "https://example.com/gateway"
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
            extract_tunnel_url(line).as_deref(),
            Some("https://bright-demo.trycloudflare.com")
        );
        assert_eq!(
            extract_tunnel_url("request https://api.trycloudflare.com/tunnel\": failed"),
            None
        );
    }
}
