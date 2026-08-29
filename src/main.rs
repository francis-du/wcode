#[path = "integrations/agent_plugin.rs"]
mod agent_plugin;
#[path = "integrations/auth.rs"]
mod auth;
#[path = "workspace/authorization.rs"]
mod authorization;
#[path = "graph/code_index.rs"]
mod code_index;
#[path = "workspace/conventions.rs"]
mod conventions;
#[path = "design/mod.rs"]
pub mod design;
#[path = "evidence/mod.rs"]
pub mod evidence;
#[path = "evidence/store.rs"]
mod evidence_store;
#[path = "graph/mod.rs"]
pub mod graph;
#[path = "graph/graph_provider_store.rs"]
mod graph_provider_store;
#[path = "graph/graph_store.rs"]
mod graph_store;
#[path = "runtime/harness.rs"]
mod harness;
#[path = "intelligence/mod.rs"]
pub mod intelligence;
#[path = "intelligence/types.rs"]
mod intelligence_types;
#[path = "ui/intelligence_web.rs"]
mod intelligence_web;
#[path = "integrations/mcp.rs"]
mod mcp;
#[path = "integrations/mcp_catalog.rs"]
mod mcp_catalog;
#[path = "integrations/mcp_stdio.rs"]
mod mcp_stdio;
#[path = "integrations/mcp_tasks.rs"]
mod mcp_tasks;
#[path = "ui/monitor.rs"]
mod monitor;
#[path = "runtime/power.rs"]
mod power;
#[path = "verification/quality_catalog.rs"]
mod quality_catalog;
#[path = "verification/quality_catalog_extended.rs"]
mod quality_catalog_extended;
#[path = "verification/quality_provider.rs"]
mod quality_provider;
#[path = "reconciliation/mod.rs"]
pub mod reconcile;
#[path = "reconciliation/execution_store.rs"]
mod reconciliation_execution_store;
#[path = "reconciliation/store.rs"]
mod reconciliation_store;
#[path = "intelligence/risk.rs"]
pub mod risk;
#[path = "runtime/control.rs"]
mod runtime_control;
#[path = "workspace/scheduler.rs"]
mod scheduler;
#[path = "scopes/mod.rs"]
mod scopes;
#[path = "semantics/mod.rs"]
pub mod semantic;
#[path = "semantics/provider.rs"]
mod semantic_provider;
#[path = "semantics/store.rs"]
mod semantic_store;
#[path = "verification/stage_executor.rs"]
mod stage_executor;
#[path = "integrations/task_store.rs"]
mod task_store;
#[path = "runtime/tunnel.rs"]
mod tunnel;
#[path = "verification/mod.rs"]
pub mod verification;
#[path = "verification/store.rs"]
mod verification_store;
#[path = "workspace/mod.rs"]
mod workspace;

use anyhow::{bail, Context, Result};
use auth::AuthState;
use clap::{ArgAction, Parser, Subcommand};
use harness::ToolHarness;
use mcp::AppState;
use monitor::{MonitorConfig, MonitorRenderer, TaskMonitor};
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
use tunnel::{
    normalize_public_url, public_endpoint_health_loop, spawn_tunnel_supervisor,
    wait_for_public_endpoint, ActiveTunnel, TunnelEvent, TunnelProvider,
};
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
const DEFAULT_MIN_PARALLEL_TOOLS: usize = 96;
const DEFAULT_MAX_PARALLEL_TOOLS: usize = 192;
const DEFAULT_INPUT_TOKEN_PRICE_PER_MILLION_USD: f64 = 5.0;
const HELP_FOOTER: &str = r#"
╭─ wcode ────────────────────────────────────────────────────────────╮
│ __          __    _____    ____    _____    ______                 │
│ \ \        / /   / ____|  / __ \  |  __ \  |  ____|                │
│  \ \  /\  / /   | |      | |  | | | |  | | | |__                   │
│   \ \/  \/ /    | |      | |  | | | |  | | |  __|                  │
│    \  /\  /     | |____  | |__| | | |__| | | |____                 │
│     \/  \/       \_____|  \____/  |_____/  |______|                │
├────────────────────────────────────────────────────────────────────┤
│ Software Intelligence Runtime for AI-native development            │
│                                                                    │
│ Repository  https://github.com/francis-du/wcode                    │
│ Docs        https://francis-du.github.io/wcode/                    │
│ Author      @francis-du                                            │
╰────────────────────────────────────────────────────────────────────╯
"#;

fn default_max_parallel_tools() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(8)
        .saturating_mul(12)
        .clamp(DEFAULT_MIN_PARALLEL_TOOLS, DEFAULT_MAX_PARALLEL_TOOLS)
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
    about = "Software Intelligence Runtime for AI-native development",
    after_help = HELP_FOOTER
)]
struct Args {
    #[command(subcommand)]
    command: Option<ControlCommand>,

    /// Local code directory exposed to connected MCP clients. Repeat to expose multiple roots.
    #[arg(
        short = 'w',
        long,
        value_name = "PATH",
        default_value = ".",
        help_heading = "Workspace"
    )]
    workspace: Vec<PathBuf>,

    /// Local interface for the MCP server.
    #[arg(
        short = 'H',
        long,
        default_value = "127.0.0.1",
        help_heading = "Connection"
    )]
    host: String,

    /// Local port for the MCP server.
    #[arg(short = 'p', long, default_value_t = 8765, help_heading = "Connection")]
    port: u16,

    /// Use an existing public base URL instead of starting a managed tunnel.
    #[arg(long, help_heading = "Connection")]
    public_url: Option<String>,

    /// Managed tunnel provider. auto falls back across free providers when startup or health checks fail.
    #[arg(long, value_enum, default_value_t = TunnelProvider::Auto, help_heading = "Connection")]
    tunnel_provider: TunnelProvider,

    /// Send every tunnel URL to this phone number or email over local iMessage (macOS only).
    #[arg(long, help_heading = "Connection")]
    imessage_to: Option<String>,

    /// Keep the server local and do not start a public tunnel.
    #[arg(long, help_heading = "Connection")]
    no_tunnel: bool,

    /// Disable file modification tools.
    #[arg(long = "read-only", action = ArgAction::SetFalse, default_value_t = true, help_heading = "Safety")]
    allow_write: bool,

    /// Disable build/test/status command execution.
    #[arg(long = "no-exec", action = ArgAction::SetFalse, default_value_t = true, help_heading = "Safety")]
    allow_exec: bool,

    /// Allow arbitrary model-facing repository-aware commands beyond the Harness verification allowlist.
    #[arg(long, help_heading = "Safety")]
    allow_risky_exec: bool,

    /// Allow replacements that empty a file or remove most of its content.
    #[arg(long, help_heading = "Safety")]
    allow_destructive_writes: bool,

    /// Allow nested or parent/child workspace roots in one process.
    #[arg(long, help_heading = "Safety")]
    allow_overlapping_workspaces: bool,

    /// Allow exposing a filesystem root or the current user's home directory.
    #[arg(long, help_heading = "Safety")]
    allow_broad_workspace: bool,

    /// Global cap for concurrently running child tool bodies.
    #[arg(short = 'j', long, default_value_t = default_max_parallel_tools(), help_heading = "Runtime")]
    max_parallel_tools: usize,

    /// Estimated USD cost per million input tokens used for the TUI savings estimate.
    #[arg(long, default_value_t = DEFAULT_INPUT_TOKEN_PRICE_PER_MILLION_USD, help_heading = "Runtime")]
    input_token_price_per_million_usd: f64,

    /// Disable the live terminal task monitor.
    #[arg(long = "no-monitor", action = ArgAction::SetFalse, default_value_t = true, help_heading = "Experience")]
    monitor: bool,

    /// Do not offer to install a missing managed-tunnel dependency.
    #[arg(long, help_heading = "Experience")]
    no_install: bool,

    /// Open wcode's client-neutral setup hub in the browser after startup. Links stay available in the TUI without this.
    #[arg(long = "open", default_value_t = false, help_heading = "Experience")]
    open_setup: bool,

    /// Allow idle system sleep while wcode is running.
    #[arg(long, help_heading = "Runtime")]
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

#[derive(Clone, Debug, PartialEq, Eq, Subcommand)]
enum ControlCommand {
    /// Restart the running wcode instance with its original arguments.
    Restart,
    /// Stop the running wcode instance.
    Stop,
    /// Export a portable Agent Plugins 1.0 Skill package inside the selected workspace.
    AgentPlugin {
        /// Repository-relative output directory. Existing files are never overwritten.
        #[arg(long, default_value = "wcode-agent-plugin")]
        output: String,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Serve the same MCP runtime over stdin/stdout for local coding agents and Agent Plugins.
    McpStdio,
    /// Inspect local Design, Graph, Semantic, Evidence, and Reconciliation runtime state.
    Intelligence {
        /// Refresh detected first-party LSP semantic providers before rendering status.
        #[arg(long)]
        refresh_semantic: bool,
        /// Fail closed when Design, Traceability, Product Scope, or required convention gates are incomplete.
        #[arg(long)]
        check: bool,
        /// Emit machine-readable JSON instead of the compact terminal summary.
        #[arg(long)]
        json: bool,
    },
    /// Inspect persisted Verification Plans and their current readiness gates.
    Verification {
        /// Inspect one specific Verification Plan ID. Omit to list recent plans.
        #[arg(long = "plan-id", alias = "plan")]
        plan: Option<String>,
        /// Execute configured or auto-discovered Property/Mutation/Fuzz/Canary stages first.
        #[arg(long)]
        execute_stages: bool,
        /// Emit machine-readable JSON instead of the compact terminal summary.
        #[arg(long)]
        json: bool,
    },
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
        let action = match command {
            ControlCommand::Restart => Some(runtime_control::ControlAction::Restart),
            ControlCommand::Stop => Some(runtime_control::ControlAction::Stop),
            ControlCommand::AgentPlugin { .. }
            | ControlCommand::McpStdio
            | ControlCommand::Intelligence { .. }
            | ControlCommand::Verification { .. } => None,
        };
        if let Some(action) = action {
            runtime_control::send(action)?;
            println!("Requested wcode {}.", action.as_str());
            return Ok(());
        }
    }
    let open_setup = args.open_setup;
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
    if let Some(command) = args.command.as_ref() {
        match command {
            ControlCommand::AgentPlugin { output, json } => {
                let (_workspace_id, workspace) = workspaces.select(None)?;
                let exported = agent_plugin::export(&workspace, output)?;
                if *json {
                    println!("{}", serde_json::to_string_pretty(&exported)?);
                } else {
                    println!("Exported wcode Agent Plugin to {}", exported.root);
                    println!("Configure MCP separately with an explicit repository workspace.");
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
            ControlCommand::Restart | ControlCommand::Stop => {}
        }
    }
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
        tasks: mcp::TaskRuntime::default(),
    });
    let app = auth::router(auth.clone())
        .merge(mcp::router(app_state))
        .merge(control_router);
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
        println!("  ↗ tunnel       requesting HTTPS endpoint in the background");
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
    auth.set_public_url(public_url.clone());

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
        workspaces: workspaces.clone(),
    };
    if open_setup {
        let mut settled = tunnel_settled_rx.clone();
        let url_slot = shared_public_url.clone();
        tokio::spawn(async move {
            if !*settled.borrow() {
                // Providers keep retrying in the background; do not hold the
                // opener hostage if none of them connects quickly.
                let _ = timeout(Duration::from_secs(120), settled.changed()).await;
            }
            let base = url_slot.read().unwrap().clone();
            open_setup_hub(&base, &format!("{base}/mcp"));
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
            _ = wait_for_terminate_signal() => break,
            _ = wait_for_monitor_interrupt(monitor_interrupt.clone()) => break,
            Some(action) = control_rx.recv() => {
                restart_requested = action == runtime_control::ControlAction::Restart;
                break;
            },
            event = tunnel_event_rx.recv() => {
                if let Some(TunnelEvent::Connected(active)) = event {
                        if tunnels.is_empty() {
                            // No live tunnel yet: this one becomes the primary
                            // endpoint. Reconnected tunnels never take over an
                            // existing primary, so retries cannot disturb the
                            // tunnels that are already up.
                            let _ = tunnel_settled_tx.send(true);
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
                                eprintln!("  ! imessage      {error:#}");
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
                let dead_index = if health_failed {
                    Some(0usize)
                } else {
                    (0..tunnels.len()).find(|&index| {
                        matches!(tunnels[index].try_wait(), Ok(Some(_)) | Err(_))
                    })
                };
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
                    eprintln!("  ! tunnel       {reason}; respawning {}", dead_provider.label());
                    let mut dead = tunnels.remove(index);
                    dead.stop().await;
                    monitor.remove_tunnel(&dead_url);
                    if tunnels.is_empty() {
                        monitor.mark_tunnel_stopped(reason);
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
                    let deaths = death_counts
                        .entry(dead_provider)
                        .and_modify(|count| *count += 1)
                        .or_insert(1);
                    let backoff_secs = (15u64 << (*deaths - 1).min(4)).min(300);
                    eprintln!(
                        "  ! tunnel       {} respawns in {backoff_secs}s (death #{deaths})",
                        dead_provider.label()
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
    if restart_requested {
        drop(runtime_registration);
        drop(_awake_guard);
        drop(_server_abort);
        spawn_replacement()?;
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

async fn run_intelligence_cli(
    workspaces: &Workspaces,
    harness: &ToolHarness,
    monitor: &TaskMonitor,
    refresh_semantic: bool,
    enforce_check: bool,
    emit_json: bool,
) -> Result<()> {
    let mut entries = Vec::new();
    for (workspace_id, root) in workspaces.roots() {
        let (_, workspace) = workspaces.select(Some(&workspace_id))?;
        let semantic_refresh = if refresh_semantic {
            Some(
                harness
                    .semantic_provider_refresh(&workspace, ".", 128, 1_000)
                    .await?,
            )
        } else {
            None
        };
        let design = harness.design_status(workspace_id.clone(), &workspace)?;
        let design_load = design::load_design(&workspace)?;
        let product_scope_required = design_load
            .state
            .constraints
            .contains_key("CONSTRAINT-PRODUCT-SCOPE-CANONICAL");
        let traceability = harness.traceability_status(workspace_id.clone(), &workspace)?;
        let scope_status = harness.product_scope_status(&workspace)?;
        let conventions = harness.convention_status(&workspace)?;
        let semantics = harness.semantic_status(&workspace_id, &workspace, 100)?;
        let graph_history = harness.graph_history(&workspace, 10)?;
        let graph_diff = if graph_history.len() >= 2 {
            harness
                .graph_diff(
                    &workspace,
                    &crate::graph_store::GraphDiffInput {
                        from_snapshot_id: None,
                        to_snapshot_id: None,
                        limit: 20,
                    },
                )
                .ok()
        } else {
            None
        };
        let providers = harness.graph_provider_status(&workspace)?;
        let semantic_providers = harness.semantic_provider_status(&workspace)?;
        let verification_executors = harness.verification_executor_status(&workspace)?;
        let evidence = harness.evidence_status(&workspace_id, &workspace, None, 100)?;
        let reconciliation = harness.reconciliation_history(&workspace, 20)?;
        let verification = harness.verification_history(&workspace_id, &workspace, 20)?;
        let risk = if workspace.exec_enabled() && workspace.root().join(".git").is_dir() {
            match harness
                .review_changes(workspace_id.clone(), &workspace, 30, monitor)
                .await
            {
                Ok(review) => harness
                    .risk_status(workspace_id.clone(), &workspace, &review)
                    .ok()
                    .and_then(|status| serde_json::to_value(status).ok()),
                Err(_) => None,
            }
        } else {
            None
        };
        entries.push(json!({
            "workspace": workspace_id,
            "root": root,
            "design": design,
            "traceability": traceability,
            "product_scope_required": product_scope_required,
            "scope_status": scope_status,
            "conventions": conventions,
            "semantics": semantics,
            "graph_history": graph_history,
            "graph_diff": graph_diff,
            "graph_providers": providers,
            "semantic_providers": semantic_providers,
            "semantic_refresh": semantic_refresh,
            "verification_executors": verification_executors,
            "risk": risk,
            "evidence": evidence,
            "reconciliation": reconciliation,
            "verification": verification,
        }));
    }
    let check_failures = intelligence_check_failures(&entries);
    let check_passed = check_failures.is_empty();
    let check_error = (!check_passed).then(|| check_failures.join("; "));
    let value = json!({
        "runtime": "wcode-software-intelligence",
        "version": env!("CARGO_PKG_VERSION"),
        "check": {
            "passed": check_passed,
            "failures": check_failures,
        },
        "workspaces": entries,
    });
    if emit_json {
        println!("{}", serde_json::to_string_pretty(&value)?);
        if enforce_check {
            if let Some(error) = check_error {
                bail!("Software Intelligence check failed: {error}");
            }
        }
        return Ok(());
    }
    println!(
        "wcode Software Intelligence Runtime {}",
        env!("CARGO_PKG_VERSION")
    );
    for workspace in value["workspaces"].as_array().into_iter().flatten() {
        let id = workspace["workspace"].as_str().unwrap_or("workspace");
        let root = workspace["root"].as_str().unwrap_or(".");
        let design = &workspace["design"];
        let trace = &workspace["traceability"];
        let scope_status = &workspace["scope_status"];
        let conventions = &workspace["conventions"];
        let semantics = &workspace["semantics"];
        let evidence = &workspace["evidence"];
        let verification = workspace["verification"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let reconciliation = workspace["reconciliation"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0);
        let graph_history = workspace["graph_history"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0);
        println!("\n{id}  {root}");
        println!(
            "  Design        {} · {} req · {} components",
            if design["valid"].as_bool().unwrap_or(false) {
                "valid"
            } else if design["initialized"].as_bool().unwrap_or(false) {
                "invalid"
            } else {
                "uninitialized"
            },
            design["requirements"].as_u64().unwrap_or(0),
            design["components"].as_u64().unwrap_or(0)
        );
        println!(
            "  Traceability  implementation {}% · verification {}%",
            trace["design_to_implementation"]["percent"]
                .as_u64()
                .unwrap_or(0),
            trace["acceptance_to_verification"]["percent"]
                .as_u64()
                .unwrap_or(0)
        );
        println!(
            "  Product Scope {}/{} mapped · {} unmapped",
            scope_status["mapped_files"].as_u64().unwrap_or(0),
            scope_status["source_files"].as_u64().unwrap_or(0),
            scope_status["unmapped_files"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0)
        );
        println!(
            "  Conventions   {} errors · {} warnings",
            conventions["errors"].as_u64().unwrap_or(0),
            conventions["warnings"].as_u64().unwrap_or(0)
        );
        println!(
            "  Semantics     {} confirmed · {} candidates",
            semantics["confirmed"].as_u64().unwrap_or(0),
            semantics["candidates"].as_u64().unwrap_or(0)
        );
        let provider_count = workspace["semantic_providers"]
            .as_array()
            .map(|providers| {
                providers
                    .iter()
                    .filter(|provider| provider["runnable"].as_bool() == Some(true))
                    .count()
            })
            .unwrap_or(0);
        let executor_count = workspace["verification_executors"]["executors"]
            .as_array()
            .map(|executors| {
                executors
                    .iter()
                    .filter(|executor| executor["available"].as_bool() == Some(true))
                    .count()
            })
            .unwrap_or(0);
        println!(
            "  Providers     {provider_count} semantic LSP · {executor_count} verification executors"
        );
        if let Some(diff) = workspace["graph_diff"].as_object() {
            println!(
                "  Graph Δ       nodes +{}/-{}/~{} · edges +{}/-{}/~{}",
                diff.get("added_node_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                diff.get("removed_node_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                diff.get("changed_node_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                diff.get("added_edge_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                diff.get("removed_edge_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                diff.get("changed_edge_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            );
        }
        if let Some(risk) = workspace["risk"].as_object() {
            println!(
                "  Risk          {} · {} findings",
                risk.get("level")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown"),
                risk.get("risks")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0)
            );
        } else {
            println!("  Risk          unavailable (Git/exec review not available)");
        }
        println!(
            "  Evidence      {} total · {} failed · {} disagreed",
            evidence["total"].as_u64().unwrap_or(0),
            evidence["failed"].as_u64().unwrap_or(0),
            evidence["disagreed"].as_u64().unwrap_or(0)
        );
        let ready = verification
            .iter()
            .filter(|status| status["ready"].as_bool() == Some(true))
            .count();
        println!(
            "  Runtime       {graph_history} graph revisions · {reconciliation} reconciliation plans · {}/{} verification ready",
            ready,
            verification.len()
        );
    }
    println!(
        "\n  Check         {}",
        if check_passed { "PASS" } else { "FAIL" }
    );
    if enforce_check {
        if let Some(error) = check_error {
            bail!("Software Intelligence check failed: {error}");
        }
    }
    Ok(())
}

fn intelligence_check_failures(workspaces: &[Value]) -> Vec<String> {
    let mut failures = Vec::new();
    for workspace in workspaces {
        let id = workspace["workspace"].as_str().unwrap_or("workspace");
        let design = &workspace["design"];
        if design["initialized"].as_bool() != Some(true) {
            failures.push(format!("{id}: Design State is uninitialized"));
        } else if design["valid"].as_bool() != Some(true) {
            failures.push(format!("{id}: Design State is invalid"));
        }

        let traceability = &workspace["traceability"];
        for (key, label) in [
            ("requirement_to_component", "requirement→component"),
            ("design_to_implementation", "design→implementation"),
            ("acceptance_to_verification", "acceptance→verification"),
        ] {
            if traceability[key]["percent"].as_u64() != Some(100) {
                failures.push(format!("{id}: {label} traceability is incomplete"));
            }
        }

        if workspace["product_scope_required"].as_bool() == Some(true) {
            let scope_status = &workspace["scope_status"];
            if scope_status["truncated"].as_bool() == Some(true) {
                failures.push(format!("{id}: Product Scope audit was truncated"));
            }
            let source_files = scope_status["source_files"].as_u64();
            let mapped_files = scope_status["mapped_files"].as_u64();
            let unmapped_files = scope_status["unmapped_files"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0);
            if source_files.is_none()
                || mapped_files.is_none()
                || source_files != mapped_files
                || unmapped_files > 0
            {
                failures.push(format!("{id}: Product Scope source mapping is incomplete"));
            }
        }

        let conventions = &workspace["conventions"];
        if conventions["truncated"].as_bool() == Some(true) {
            failures.push(format!("{id}: Convention audit was truncated"));
        }
        if conventions["errors"].as_u64().unwrap_or(0) > 0 {
            failures.push(format!(
                "{id}: Convention audit contains required-policy errors"
            ));
        }
    }
    failures
}

async fn run_verification_cli(
    workspaces: &Workspaces,
    harness: &ToolHarness,
    requested_plan: Option<&str>,
    execute_stages: bool,
    emit_json: bool,
) -> Result<()> {
    if execute_stages && requested_plan.is_none() {
        bail!("--execute-stages requires --plan-id so wcode never guesses which plan to run");
    }
    let mut entries = Vec::new();
    for (workspace_id, root) in workspaces.roots() {
        let (_, workspace) = workspaces.select(Some(&workspace_id))?;
        let mut execution = None;
        let statuses = if let Some(plan_id) = requested_plan {
            match harness.verification_status(&workspace_id, &workspace, plan_id) {
                Ok(_) => {
                    if execute_stages {
                        execution = Some(
                            harness
                                .verification_execute_stages(&workspace_id, &workspace, plan_id)
                                .await?,
                        );
                    }
                    vec![harness.verification_status(&workspace_id, &workspace, plan_id)?]
                }
                Err(_) => Vec::new(),
            }
        } else {
            harness.verification_history(&workspace_id, &workspace, 50)?
        };
        if requested_plan.is_none() || !statuses.is_empty() {
            entries.push(json!({
                "workspace": workspace_id,
                "root": root,
                "execution": execution,
                "plans": statuses,
            }));
        }
    }
    if requested_plan.is_some() && entries.is_empty() {
        bail!("verification plan was not found in any configured workspace");
    }
    let value = json!({
        "runtime": "wcode-verification",
        "version": env!("CARGO_PKG_VERSION"),
        "workspaces": entries,
    });
    if emit_json {
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    println!("wcode Verification Mesh {}", env!("CARGO_PKG_VERSION"));
    for workspace in value["workspaces"].as_array().into_iter().flatten() {
        println!(
            "\n{}  {}",
            workspace["workspace"].as_str().unwrap_or("workspace"),
            workspace["root"].as_str().unwrap_or(".")
        );
        let plans = workspace["plans"].as_array().cloned().unwrap_or_default();
        if plans.is_empty() {
            println!("  no persisted verification plans");
            continue;
        }
        for status in plans {
            let plan = &status["plan"];
            let blockers = status["blockers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>();
            println!(
                "  {}  risk={}  ready={}  reviewers={}/{}  blockers={}",
                plan["id"].as_str().unwrap_or("plan"),
                plan["risk_level"].as_str().unwrap_or("unknown"),
                status["ready"].as_bool().unwrap_or(false),
                status["submitted"].as_u64().unwrap_or(0),
                plan["job_ids"].as_array().map(Vec::len).unwrap_or(0),
                if blockers.is_empty() {
                    "none".to_owned()
                } else {
                    blockers.join(",")
                }
            );
        }
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
    use crate::tunnel::{
        extract_cloudflare_tunnel_url as extract_tunnel_url, extract_ssh_tunnel_url,
        is_quick_tunnel_url, validate_health_response,
    };
    use clap::CommandFactory;

    #[test]
    fn help_shows_ascii_brand_and_author_links() {
        let help = Args::command().render_long_help().to_string();
        assert!(help.contains("__          __"));
        assert!(help.contains("Software Intelligence Runtime"));
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
        let stdio = Args::try_parse_from(["wcode", "mcp-stdio"]).expect("stdio MCP command parses");
        assert_eq!(stdio.command, Some(ControlCommand::McpStdio));
        let plugin = Args::try_parse_from([
            "wcode",
            "agent-plugin",
            "--output",
            "dist/wcode-plugin",
            "--json",
        ])
        .expect("agent plugin command parses");
        assert_eq!(
            plugin.command,
            Some(ControlCommand::AgentPlugin {
                output: "dist/wcode-plugin".to_owned(),
                json: true,
            })
        );
        let intelligence = Args::try_parse_from([
            "wcode",
            "intelligence",
            "--refresh-semantic",
            "--check",
            "--json",
        ])
        .expect("intelligence command parses");
        assert_eq!(
            intelligence.command,
            Some(ControlCommand::Intelligence {
                refresh_semantic: true,
                check: true,
                json: true,
            })
        );
        let verification = Args::try_parse_from([
            "wcode",
            "verification",
            "--plan-id",
            "VP-fixture",
            "--execute-stages",
            "--json",
        ])
        .expect("verification command parses");
        assert_eq!(
            verification.command,
            Some(ControlCommand::Verification {
                plan: Some("VP-fixture".to_owned()),
                execute_stages: true,
                json: true,
            })
        );

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
    fn intelligence_check_requires_valid_complete_scoped_state() {
        let healthy = vec![json!({
            "workspace": "demo",
            "design": {"initialized": true, "valid": true},
            "traceability": {
                "requirement_to_component": {"percent": 100},
                "design_to_implementation": {"percent": 100},
                "acceptance_to_verification": {"percent": 100}
            },
            "product_scope_required": true,
            "scope_status": {
                "source_files": 12,
                "mapped_files": 12,
                "unmapped_files": [],
                "truncated": false
            },
            "conventions": {"errors": 0, "warnings": 2, "truncated": false}
        })];
        assert!(intelligence_check_failures(&healthy).is_empty());

        let broken = vec![json!({
            "workspace": "demo",
            "design": {"initialized": true, "valid": false},
            "traceability": {
                "requirement_to_component": {"percent": 100},
                "design_to_implementation": {"percent": 99},
                "acceptance_to_verification": {"percent": 100}
            },
            "product_scope_required": true,
            "scope_status": {
                "source_files": 12,
                "mapped_files": 11,
                "unmapped_files": ["src/orphan.rs"],
                "truncated": false
            },
            "conventions": {"errors": 1, "warnings": 0, "truncated": false}
        })];
        let failures = intelligence_check_failures(&broken);
        assert!(failures
            .iter()
            .any(|failure| failure.contains("Design State is invalid")));
        assert!(failures
            .iter()
            .any(|failure| failure.contains("design→implementation")));
        assert!(failures
            .iter()
            .any(|failure| failure.contains("Product Scope")));
        assert!(failures
            .iter()
            .any(|failure| failure.contains("Convention")));

        let generic = vec![json!({
            "workspace": "third-party",
            "design": {"initialized": true, "valid": true},
            "traceability": {
                "requirement_to_component": {"percent": 100},
                "design_to_implementation": {"percent": 100},
                "acceptance_to_verification": {"percent": 100}
            },
            "product_scope_required": false,
            "scope_status": {
                "source_files": 12,
                "mapped_files": 0,
                "unmapped_files": ["src/lib.rs"],
                "truncated": false
            },
            "conventions": {"errors": 0, "warnings": 0, "truncated": false}
        })];
        assert!(intelligence_check_failures(&generic).is_empty());
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
            extract_tunnel_url(line).as_deref(),
            Some("https://bright-demo.trycloudflare.com")
        );
        assert_eq!(
            extract_tunnel_url("request https://api.trycloudflare.com/tunnel\": failed"),
            None
        );
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
            extract_ssh_tunnel_url(TunnelProvider::LocalhostRun, "connect localhost.run"),
            None
        );
        assert_eq!(
            extract_ssh_tunnel_url(TunnelProvider::Pinggy, "connect free.pinggy.io"),
            None
        );
        assert_eq!(
            TunnelProvider::auto_candidates(),
            vec![
                TunnelProvider::Cloudflare,
                TunnelProvider::LocalhostRun,
                TunnelProvider::Pinggy,
                TunnelProvider::Tailscale
            ]
        );
        assert!(is_quick_tunnel_url("https://5d993e65a9d400.lhr.life/mcp"));
        assert!(!is_quick_tunnel_url("https://admin.localhost.run/mcp"));
    }
}
