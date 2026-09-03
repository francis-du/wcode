use crate::auth::AuthState;
use crate::harness::ToolHarness;
use crate::mcp::AppState;
use crate::monitor::{MonitorConfig, MonitorRenderer, OperatorMessageKind, TaskMonitor};
use crate::tunnel::{
    normalize_public_url, public_endpoint_health_loop, spawn_tunnel_supervisor,
    wait_for_public_endpoint, ActiveTunnel, TunnelEvent, TunnelProvider,
};
use crate::workspace::{WorkspaceSecurity, Workspaces};
use crate::{
    agent_install, agent_plugin, auth, design, mcp, mcp_stdio, power, resource, semantic_runtime,
};
use crate::{AUTHOR_HANDLE, AUTHOR_URL, PROJECT_URL};
use anyhow::{bail, Context, Result};
use clap::{ArgAction, Parser};
use serde_json::{json, Value};
use std::future::pending;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::{Command as StdCommand, Stdio as StdStdio};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::AbortHandle;
use tokio::time::{sleep, timeout, Duration};
use tracing_subscriber::EnvFilter;

const DEFAULT_INPUT_TOKEN_PRICE_PER_MILLION_USD: f64 = 5.0;

#[path = "commands.rs"]
mod commands;
use commands::ControlCommand;
#[path = "intelligence.rs"]
mod intelligence;
use intelligence::{run_intelligence_cli, run_verification_cli};
#[path = "resources.rs"]
mod resources;
use resources::{ResourceArgs, SetupGuideOptions};
#[path = "setup.rs"]
mod setup;
#[path = "tunnel_lifecycle.rs"]
mod tunnel_lifecycle;
#[path = "update.rs"]
mod update;
const HELP_FOOTER: &str = r#"
╭─ WCode ───────────────────────────────────────────────────╮
│ __          __    _____    ____    _____    ______        │
│ \ \        / /   / ____|  / __ \  |  __ \  |  ____|       │
│  \ \  /\  / /   | |      | |  | | | |  | | | |__          │
│   \ \/  \/ /    | |      | |  | | | |  | | |  __|         │
│    \  /\  /     | |____  | |__| | | |__| | | |____        │
│     \/  \/       \_____|  \____/  |_____/  |______|       │
├───────────────────────────────────────────────────────────┤
│ Software intelligence + safe coding tools for AI agents   │
│ Repository  https://github.com/francis-du/wcode           │
│ Docs        https://wcode.francis.run/                    │
│ Author      @francis-du                                   │
╰───────────────────────────────────────────────────────────╯

QUICK START
  wcode                         Start WCode for the current project.
  wcode setup                   Set up WCode for detected coding agents.
  wcode mcp-stdio               Connect an MCP Host; its current directory becomes the project.
  wcode intelligence            Inspect project intelligence and LSP readiness.
  wcode intelligence --refresh-semantic
                                Discover and initialize available language servers.
  wcode verification            Inspect project verification state.
  wcode update                  Update WCode, then reconnect running MCP Host sessions.

The current directory is used automatically. Most users do not need --workspace.
Language servers are discovered automatically; WCode asks before running providers that require approval.
Use `wcode <COMMAND> --help` only when you need command-specific options.
"#;

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
    about = "Software intelligence and governed coding tools for AI agents",
    long_about = "Run WCode in the current project, connect coding agents over MCP, discover language-server semantics, inspect project intelligence and verification, or update the installed binary. The current directory is the default Workspace, so most users do not need extra path arguments.",
    disable_help_subcommand = true,
    after_help = HELP_FOOTER
)]
struct Args {
    #[command(subcommand)]
    command: Option<ControlCommand>,

    /// Project directory to use instead of the current directory. Usually unnecessary.
    #[arg(
        short = 'w',
        long,
        value_name = "PATH",
        default_value = ".",
        help_heading = "Project"
    )]
    workspace: Vec<PathBuf>,

    /// Local interface for the MCP server.
    #[arg(
        short = 'H',
        long,
        default_value = "127.0.0.1",
        help_heading = "Connection",
        hide = true
    )]
    host: String,

    /// Local port for the MCP server.
    #[arg(
        short = 'p',
        long,
        default_value_t = 8765,
        help_heading = "Connection",
        hide = true
    )]
    port: u16,

    /// Use an existing public base URL instead of starting a managed tunnel.
    #[arg(long, help_heading = "Connection", hide = true)]
    public_url: Option<String>,

    /// Managed tunnel provider. auto falls back across free providers when startup or health checks fail.
    #[arg(long, value_enum, default_value_t = TunnelProvider::Auto, help_heading = "Connection", hide = true)]
    tunnel_provider: TunnelProvider,

    /// Send every tunnel URL to this phone number or email over local iMessage (macOS only).
    #[arg(long, help_heading = "Connection", hide = true)]
    imessage_to: Option<String>,

    /// Keep WCode local and disable the managed public tunnel.
    #[arg(long, help_heading = "Connection")]
    no_tunnel: bool,

    /// Read-only mode: disable file modification tools.
    #[arg(long = "read-only", action = ArgAction::SetFalse, default_value_t = true, help_heading = "Safety")]
    allow_write: bool,

    /// Do not let WCode run build, test, or repository commands.
    #[arg(long = "no-exec", action = ArgAction::SetFalse, default_value_t = true, help_heading = "Safety")]
    allow_exec: bool,

    /// Do not discover or run language servers.
    #[arg(long = "no-semantic", action = ArgAction::SetFalse, default_value_t = true, help_heading = "Safety")]
    allow_semantic: bool,

    /// Allow approved tools to work across your Home directory. Filesystem root, credentials, symlinks/hard-links, and shell execution remain blocked.
    #[arg(long, help_heading = "Safety")]
    full_access: bool,

    /// Allow arbitrary model-facing repository-aware commands beyond the Harness verification allowlist.
    #[arg(long, help_heading = "Safety", hide = true)]
    allow_risky_exec: bool,

    /// Allow replacements that empty a file or remove most of its content.
    #[arg(long, help_heading = "Safety", hide = true)]
    allow_destructive_writes: bool,

    /// Allow nested or parent/child workspace roots in one process.
    #[arg(long, help_heading = "Safety", hide = true)]
    allow_overlapping_workspaces: bool,

    /// Allow exposing a filesystem root or the current user's home directory.
    #[arg(long, help_heading = "Safety", hide = true)]
    allow_broad_workspace: bool,

    #[command(flatten)]
    resources: ResourceArgs,

    /// Estimated USD cost per million input tokens used for the TUI savings estimate.
    #[arg(long, default_value_t = DEFAULT_INPUT_TOKEN_PRICE_PER_MILLION_USD, help_heading = "Runtime", hide = true)]
    input_token_price_per_million_usd: f64,

    /// Hide the live terminal activity view.
    #[arg(long = "no-monitor", action = ArgAction::SetFalse, default_value_t = true, help_heading = "Experience")]
    monitor: bool,

    /// Do not offer to install a missing managed-tunnel dependency.
    #[arg(long, help_heading = "Experience", hide = true)]
    no_install: bool,

    /// Open the WCode setup page in your browser after startup.
    #[arg(long = "open", default_value_t = false, help_heading = "Experience")]
    open_setup: bool,

    /// Allow idle system sleep while wcode is running.
    #[arg(long, help_heading = "Runtime", hide = true)]
    allow_sleep: bool,

    /// Deprecated alias, accepted and ignored; the setup hub no longer auto-opens.
    #[arg(
        long = "no-install-chatgpt",
        action = ArgAction::SetFalse,
        default_value_t = true,
        hide = true
    )]
    legacy_open_setup: bool,
}

pub async fn run() -> Result<()> {
    let args = Args::parse();
    if let Some(command) = args.command.as_ref() {
        match command {
            ControlCommand::Update => {
                update::run()?;
                return Ok(());
            }
            ControlCommand::Setup {
                dry_run,
                global,
                project,
                json,
            } => {
                let root = args
                    .workspace
                    .first()
                    .map(PathBuf::as_path)
                    .unwrap_or_else(|| std::path::Path::new("."));
                setup::run(root, *dry_run, *json, *global, *project)?;
                return Ok(());
            }
            ControlCommand::AgentPlugin { .. }
            | ControlCommand::McpStdio
            | ControlCommand::Intelligence { .. }
            | ControlCommand::Verification { .. } => {}
        }
    }
    let open_setup = args.open_setup;
    if !args.input_token_price_per_million_usd.is_finite()
        || args.input_token_price_per_million_usd < 0.0
    {
        bail!("--input-token-price-per-million-usd must be a finite non-negative number");
    }
    let resource_limits = args.resources.activate()?;
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
    let keep_awake =
        args.command.is_none() || matches!(args.command.as_ref(), Some(ControlCommand::McpStdio));
    let _awake_guard = if args.allow_sleep || !keep_awake {
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
    let allow_write = args.allow_write || args.full_access;
    let allow_exec = args.allow_exec || args.full_access;
    let allow_semantic = args.allow_semantic || args.full_access;
    let security = WorkspaceSecurity {
        allow_risky_exec: args.allow_risky_exec || args.full_access,
        allow_semantic_exec: allow_semantic,
        allow_destructive_writes: args.allow_destructive_writes || args.full_access,
        allow_overlapping_workspaces: args.allow_overlapping_workspaces || args.full_access,
        allow_user_home_workspace: args.full_access,
        allow_broad_workspace: args.allow_broad_workspace,
    };
    let workspaces =
        Workspaces::new_with_security(&args.workspace, allow_write, allow_exec, security)?;
    if args.full_access {
        workspaces.grant_full_user_access()?;
    }
    let harness = ToolHarness::new(resource_limits.effective_parallel_tools)?;
    let monitor = TaskMonitor::new(workspaces.roots().into_iter().map(|(id, _)| id));
    if resource_limits.requested_parallel_tools != resource_limits.effective_parallel_tools {
        monitor.operator_message(
            OperatorMessageKind::Info,
            "resources",
            format!(
                "tool slots clamped from {} to {} by the memory safety policy",
                resource_limits.requested_parallel_tools, resource_limits.effective_parallel_tools
            ),
        );
    }
    let resource_task = resource::spawn_monitor(harness.clone(), monitor.clone());
    let _resource_abort = AbortTaskOnDrop(resource_task.abort_handle());
    let semantic_task = if allow_semantic
        && allow_exec
        && (args.command.is_none()
            || matches!(args.command.as_ref(), Some(ControlCommand::McpStdio)))
    {
        Some(semantic_runtime::spawn(
            workspaces.clone(),
            harness.clone(),
            monitor.clone(),
        ))
    } else {
        None
    };
    let _semantic_abort = semantic_task
        .as_ref()
        .map(|task| AbortTaskOnDrop(task.abort_handle()));
    if let Some(command) = args.command.as_ref() {
        match command {
            ControlCommand::Setup { .. } => {
                unreachable!("setup returns before runtime initialization")
            }
            ControlCommand::AgentPlugin {
                output,
                profile,
                remote_url,
                install_all,
                dry_run,
                json,
            } => {
                let (_workspace_id, workspace) = workspaces.select(None)?;
                if *install_all {
                    let plan = agent_install::plan_install(&workspace);
                    let summary = agent_install::apply_install(&workspace, plan, *dry_run);
                    if *json {
                        println!("{}", serde_json::to_string_pretty(&summary)?);
                    } else {
                        agent_install::print_human(&summary);
                    }
                    if !summary.failed.is_empty() {
                        bail!(
                            "{} Agent integration(s) failed safe configuration",
                            summary.failed.len()
                        );
                    }
                } else {
                    let exported =
                        agent_plugin::export(&workspace, output, *profile, remote_url.as_deref())?;
                    if *json {
                        println!("{}", serde_json::to_string_pretty(&exported)?);
                    } else {
                        println!("Exported wcode Agent Plugin to {}", exported.root);
                        println!("{}", exported.note);
                    }
                }
                return Ok(());
            }
            ControlCommand::McpStdio => {
                mcp_stdio::serve(workspaces.clone(), harness.clone(), monitor.clone()).await?;
                return Ok(());
            }
            ControlCommand::Intelligence {
                refresh_semantic,
                check,
                json,
            } => {
                run_intelligence_cli(
                    &workspaces,
                    &harness,
                    &monitor,
                    *refresh_semantic,
                    allow_semantic && allow_exec,
                    *check,
                    *json,
                )
                .await?;
                return Ok(());
            }
            ControlCommand::Verification {
                plan,
                execute_stages,
                json,
            } => {
                run_verification_cli(
                    &workspaces,
                    &harness,
                    plan.as_deref(),
                    *execute_stages,
                    *json,
                )
                .await?;
                return Ok(());
            }
            ControlCommand::Update => {}
        }
    }
    let listener = TcpListener::bind(format!("{}:{}", args.host, args.port))
        .await
        .with_context(|| format!("cannot bind {}:{}", args.host, args.port))?;
    let local_url = format!("http://{}:{}", args.host, listener.local_addr()?.port());
    let auth = Arc::new(AuthState::new_with_monitor(
        local_url.clone(),
        monitor.clone(),
        &workspaces.configured_roots(),
    )?);
    let app_state = Arc::new(AppState {
        auth: auth.clone(),
        workspaces: workspaces.clone(),
        harness: harness.clone(),
        monitor: monitor.clone(),
        tasks: mcp::TaskRuntime::default(),
    });
    let app = auth::router(auth.clone()).merge(mcp::router(app_state));
    let mut server_task = tokio::spawn(async move { axum::serve(listener, app).await });
    let _server_abort = AbortTaskOnDrop(server_task.abort_handle());

    let mut tunnels: Vec<ActiveTunnel> = Vec::new();
    let shared_public_url = Arc::new(std::sync::RwLock::new(local_url.clone()));
    let (tunnel_settled_tx, tunnel_settled_rx) = watch::channel(false);
    let (tunnel_event_tx, mut tunnel_event_rx) = tokio::sync::mpsc::channel::<TunnelEvent>(8);
    let mut imessage_sent: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut pending_respawns: Vec<(TunnelProvider, std::time::Instant)> = Vec::new();
    let mut death_counts: std::collections::HashMap<TunnelProvider, u32> =
        std::collections::HashMap::new();
    let public_url = if let Some(url) = args.public_url.as_deref() {
        let url = normalize_public_url(url)?;
        *shared_public_url.write().unwrap() = url.clone();
        monitor.mark_public_endpoint("external", None);
        let _ = tunnel_settled_tx.send(true);
        url
    } else if args.no_tunnel {
        monitor.mark_public_endpoint("local-only", None);
        let _ = tunnel_settled_tx.send(true);
        local_url.clone()
    } else {
        monitor.operator_message(
            OperatorMessageKind::Info,
            "tunnel",
            "requesting HTTPS endpoint in the background",
        );
        monitor.mark_public_endpoint("pending", None);
        {
            let forward_tx = tunnel_event_tx.clone();
            let local_url = local_url.clone();
            let instance_id = auth.instance_id().to_owned();
            let install_missing = !args.no_install;
            let monitor = monitor.clone();
            let settled_tx = tunnel_settled_tx.clone();
            let url_slot = shared_public_url.clone();
            let auth = auth.clone();
            tokio::spawn(async move {
                let mut events = spawn_tunnel_supervisor(
                    args.tunnel_provider,
                    &local_url,
                    &instance_id,
                    install_missing,
                    monitor,
                );
                let mut first = true;
                while let Some(event) = events.recv().await {
                    if first {
                        // Announce the endpoint the moment the first tunnel
                        // lands so setup-guide/open waits are not blocked on
                        // the supervision loop starting later.
                        let _ = settled_tx.send(true);
                        let TunnelEvent::Connected(active) = &event;
                        let public_url = active.public_url().to_owned();
                        *url_slot.write().unwrap() = public_url.clone();
                        auth.set_public_url(public_url);
                        first = false;
                    }
                    if forward_tx.send(event).await.is_err() {
                        return;
                    }
                }
            });
        }
        local_url.clone()
    };
    // Managed tunnels publish their verified URL from the supervisor task.
    // Do not race that update by writing the initial localhost URL back here.
    if args.public_url.is_some() || args.no_tunnel {
        auth.set_public_url(public_url.clone());
    }

    if args.public_url.is_some() {
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
                    bail!("public endpoint did not become ready: {error}");
                }
            }
        }
    }

    let (public_health_stop, _) = watch::channel(false);
    let mut public_health_task: Option<tokio::task::JoinHandle<()>> = None;

    let intelligence_url = format!("{local_url}/intelligence#token={}", auth.ui_token());
    let monitor_config = MonitorConfig {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        instance_id: auth.instance_id().to_owned(),
        local_health_url: format!("{local_url}/healthz"),
        public_url: shared_public_url.clone(),
        intelligence_url: intelligence_url.clone(),
        project_url: PROJECT_URL.to_owned(),
        author_url: AUTHOR_URL.to_owned(),
        author_handle: AUTHOR_HANDLE.to_owned(),
        pairing_code: auth.pairing_code().to_owned(),
        max_parallel: harness.max_parallel(),
        input_token_price_per_million_usd: args.input_token_price_per_million_usd,
        semantic_auto: allow_semantic && allow_exec,
        workspaces: workspaces.clone(),
        harness: harness.clone(),
    };
    if open_setup {
        let mut settled = tunnel_settled_rx.clone();
        let url_slot = shared_public_url.clone();
        let setup_monitor = monitor.clone();
        tokio::spawn(async move {
            if !*settled.borrow() {
                // Providers keep retrying in the background; do not hold the
                // opener hostage if none of them connects quickly.
                let _ = timeout(Duration::from_secs(120), settled.changed()).await;
            }
            let base = url_slot.read().unwrap().clone();
            open_setup_hub(&base, &format!("{base}/mcp"), &setup_monitor);
        });
    }
    let renderer = monitor.spawn_renderer(monitor_config, args.monitor);
    if renderer.is_none() {
        let mut settled = tunnel_settled_rx.clone();
        if !*settled.borrow() {
            let _ = timeout(Duration::from_secs(120), settled.changed()).await;
        }
        let public_url_now = shared_public_url.read().unwrap().clone();
        print_setup_guide(
            &workspaces,
            &local_url,
            &public_url_now,
            auth.pairing_code(),
            &intelligence_url,
            SetupGuideOptions {
                local_only: args.no_tunnel,
                max_parallel_tools: harness.max_parallel(),
                max_cpu_percent: resource_limits.max_cpu_percent,
                max_memory_mb: resource_limits.max_memory_bytes / (1024 * 1024),
                input_token_price_per_million_usd: args.input_token_price_per_million_usd,
                security,
            },
        );
    }

    let monitor_interrupt = renderer.as_ref().map(MonitorRenderer::interrupt_receiver);
    let mut server_task_finished = false;
    loop {
        tokio::select! {
            result = &mut server_task => {
                result.context("local MCP server task failed")??;
                server_task_finished = true;
                break;
            },
            _ = tokio::signal::ctrl_c() => break,
            _ = wait_for_terminate_signal() => break,
            _ = wait_for_monitor_interrupt(monitor_interrupt.clone()) => break,
            event = tunnel_event_rx.recv() => {
                if let Some(TunnelEvent::Connected(active)) = event {
                        let recovered_provider = active.provider();
                        tunnel_lifecycle::record_recovered_provider(&mut death_counts, recovered_provider);
                        pending_respawns.retain(|(provider, _)| *provider != recovered_provider);
                        auth.register_public_url(active.public_url().to_owned());
                        if tunnels.is_empty() {
                            // No live tunnel yet: this one becomes the primary
                            // endpoint. Reconnected tunnels never take over an
                            // existing primary, so retries cannot disturb the
                            // tunnels that are already up.
                            let _ = tunnel_settled_tx.send(true);
                            monitor.mark_public_endpoint("quick-tunnel", Some(true));
                            monitor.mark_public_url_check(true, None);
                            let public_url = active.public_url().to_owned();
                            *shared_public_url.write().unwrap() = public_url.clone();
                            auth.set_public_url(public_url.clone());
                            if let Some(task) = public_health_task.take() {
                                task.abort();
                            }
                            public_health_task = Some(tokio::spawn(public_endpoint_health_loop(
                                public_url,
                                auth.instance_id().to_owned(),
                                monitor.clone(),
                                public_health_stop.subscribe(),
                            )));
                        }
                        if let Some(recipient) = args.imessage_to.as_deref() {
                            let provider = active.provider_label();
                            let public_url = active.public_url().to_owned();
                            // One message per provider link: reconnecting with
                            // the same URL never spams; a changed URL is news.
                            if imessage_sent.get(provider) == Some(&public_url) {
                                tunnels.push(active);
                                continue;
                            }
                            imessage_sent.insert(provider.to_owned(), public_url.clone());
                            let message = format!(
                                "wcode {provider} tunnel ready\n{public_url}\nMCP {public_url}/mcp\nWeb UI {public_url}/intelligence#token={}\nVerify code {}",
                                auth.ui_token(),
                                auth.pairing_code()
                            );
                            if let Err(error) = send_imessage(recipient, &message) {
                                monitor.operator_message(
                                    OperatorMessageKind::Warning,
                                    "imessage",
                                    format!("{error:#}"),
                                );
                            }
                        }
                        tunnels.push(active);
                    }
            },
            _ = sleep(Duration::from_secs(1)) => {
                // A dead tunnel only takes itself down: after an exponential
                // backoff, respawn just that provider; everything else stays.
                pending_respawns.retain(|&(provider, due)| {
                    if std::time::Instant::now() < due {
                        return true;
                    }
                    let forward_tx = tunnel_event_tx.clone();
                    let local_url = local_url.clone();
                    let instance_id = auth.instance_id().to_owned();
                    let install_missing = !args.no_install;
                    let monitor = monitor.clone();
                    tokio::spawn(async move {
                        let mut events = spawn_tunnel_supervisor(
                            provider,
                            &local_url,
                            &instance_id,
                            install_missing,
                            monitor,
                        );
                        while let Some(event) = events.recv().await {
                            if forward_tx.send(event).await.is_err() {
                                return;
                            }
                        }
                    });
                    false
                });
                let health_failed = monitor.connection_status().public_url_healthy == Some(false);
                let tunnel_count = tunnels.len();
                let dead_index = tunnel_lifecycle::dead_tunnel_index(
                    health_failed,
                    tunnel_count,
                    |index| matches!(tunnels[index].try_wait(), Ok(Some(_)) | Err(_)),
                );
                if let Some(index) = dead_index {
                    let reason = if health_failed {
                        "primary endpoint health failed three times".to_owned()
                    } else {
                        format!(
                            "{} tunnel exited with {}",
                            tunnels[index].provider_label(),
                            tunnels[index]
                                .try_wait()
                                .ok()
                                .flatten()
                                .map(|status| status.to_string())
                                .unwrap_or_else(|| "unknown status".to_owned())
                        )
                    };
                    let dead_provider = tunnels[index].provider();
                    let dead_url = tunnels[index].public_url().to_owned();
                    let was_primary = index == 0;
                    monitor.operator_message(
                        OperatorMessageKind::Warning,
                        "tunnel",
                        format!("{reason}; respawning {}", dead_provider.label()),
                    );
                    let mut dead = tunnels.remove(index);
                    dead.stop().await;
                    monitor.remove_tunnel(&dead_url);
                    auth.unregister_public_url(&dead_url);
                    if tunnels.is_empty() {
                        monitor.mark_tunnel_stopped(reason);
                        *shared_public_url.write().unwrap() = local_url.clone();
                        auth.set_public_url(local_url.clone());
                    }
                    if was_primary {
                        if let Some(task) = public_health_task.take() {
                            task.abort();
                        }
                        if let Some(next) = tunnels.first() {
                            let next_url = next.public_url().to_owned();
                            *shared_public_url.write().unwrap() = next_url.clone();
                            auth.set_public_url(next_url.clone());
                            monitor.mark_public_url_check(true, None);
                            public_health_task = Some(tokio::spawn(public_endpoint_health_loop(
                                next_url,
                                auth.instance_id().to_owned(),
                                monitor.clone(),
                                public_health_stop.subscribe(),
                            )));
                        }
                    }
                    let backoff_secs =
                        tunnel_lifecycle::record_dead_provider(&mut death_counts, dead_provider);
                    let deaths = death_counts.get(&dead_provider).copied().unwrap_or(1);
                    monitor.operator_message(
                        OperatorMessageKind::Warning,
                        "tunnel",
                        format!(
                            "{} respawns in {backoff_secs}s (death #{deaths})",
                            dead_provider.label()
                        ),
                    );
                    pending_respawns.push((
                        dead_provider,
                        std::time::Instant::now() + Duration::from_secs(backoff_secs),
                    ));
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

    for mut active in tunnels.drain(..) {
        active.stop().await;
    }
    if !server_task_finished {
        server_task.abort();
        let _ = server_task.await;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn send_imessage(recipient: &str, text: &str) -> anyhow::Result<()> {
    let recipient = recipient.trim();
    if recipient.is_empty() {
        anyhow::bail!("--imessage-to needs a phone number or email address");
    }
    let script = format!(
        "tell application \"Messages\" to send {} to buddy {} of (service 1 whose service type is iMessage)",
        applescript_string(text),
        applescript_string(recipient)
    );
    let status = StdCommand::new("osascript")
        .args(["-e", &script])
        .stdin(StdStdio::null())
        .stdout(StdStdio::null())
        .stderr(StdStdio::piped())
        .status()
        .map_err(|error| anyhow::anyhow!("failed to launch osascript: {error}"))?;
    if !status.success() {
        anyhow::bail!(
            "Messages could not deliver to {recipient}; make sure the Messages app is signed in and the recipient uses iMessage"
        );
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn applescript_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(not(target_os = "macos"))]
fn send_imessage(recipient: &str, _text: &str) -> anyhow::Result<()> {
    anyhow::bail!("--imessage-to is only supported on macOS (got {recipient})")
}

#[cfg(unix)]
async fn wait_for_terminate_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let Ok(mut stream) = signal(SignalKind::terminate()) else {
        std::future::pending::<()>().await;
        return;
    };
    stream.recv().await;
}

#[cfg(not(unix))]
async fn wait_for_terminate_signal() {
    std::future::pending::<()>().await;
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

fn open_setup_hub(setup_url: &str, mcp_url: &str, monitor: &TaskMonitor) {
    monitor.operator_message(
        OperatorMessageKind::Info,
        "setup",
        "opening wcode setup hub",
    );
    monitor.operator_message(OperatorMessageKind::Info, "MCP URL", mcp_url);
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
        monitor.operator_message(
            OperatorMessageKind::Warning,
            "setup",
            format!("could not open the browser ({error}); visit {setup_url}"),
        );
    }
}

fn print_setup_guide(
    workspaces: &Workspaces,
    local_url: &str,
    public_url: &str,
    pairing_code: &str,
    intelligence_url: &str,
    options: SetupGuideOptions,
) {
    let SetupGuideOptions {
        local_only,
        max_parallel_tools,
        max_cpu_percent,
        max_memory_mb,
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
    println!("│  Dashboard  {intelligence_url}");
    println!("│  Verify code  {pairing_code}");
    println!("│  Slots cap  {max_parallel_tools} concurrent tool bodies");
    println!("│  Resources  BG {max_cpu_percent:.1}% CPU target · burst-friendly · {max_memory_mb} MiB soft RSS");
    println!("│  Token EST  ~4 bytes/token · ${input_token_price_per_million_usd:.2}/M input");
    println!(
        "│  Security   semantic {} · risky-exec {} · destructive {} · overlap {} · broad {}",
        if security.allow_semantic_exec {
            "auto"
        } else {
            "off"
        },
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
