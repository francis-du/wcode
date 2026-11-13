use super::*;

#[derive(Debug, Parser)]
#[command(
    name = "wcode",
    author,
    version,
    about = "Repository tools, task tracking and checks",
    long_about = "Run WCode in the current project, connect coding agents over MCP, discover language-server semantics, inspect project intelligence and verification, or update the installed binary. The current directory is the default Workspace, so most users do not need extra path arguments.",
    disable_help_subcommand = true,
    after_help = HELP_FOOTER
)]
pub(super) struct Args {
    #[command(subcommand)]
    pub(super) command: Option<ControlCommand>,

    /// Project directory to use instead of the current directory. Usually unnecessary.
    /// May be repeated. When omitted, roots are inherited from the agent session
    /// (`WCODE_WORKSPACES` or `WCODE_WORKSPACES_FILE`), then the current directory.
    #[arg(
        short = 'w',
        long,
        value_name = "PATH",
        help_heading = "Project",
        global = true
    )]
    pub(super) workspace: Vec<PathBuf>,

    /// Local interface for the MCP server.
    #[arg(
        short = 'H',
        long,
        default_value = "127.0.0.1",
        help_heading = "Connection",
        hide = true
    )]
    pub(super) host: String,

    /// Local port for the MCP server.
    #[arg(
        short = 'p',
        long,
        default_value_t = 8765,
        help_heading = "Connection",
        hide = true
    )]
    pub(super) port: u16,

    /// Use an existing public base URL instead of starting a managed tunnel.
    #[arg(
        long,
        global = true,
        conflicts_with = "no_tunnel",
        help_heading = "Connection",
        hide = true
    )]
    pub(super) public_url: Option<String>,

    /// Fixed password accepted by the OAuth authorization page in addition to the pairing code.
    #[arg(
        long,
        value_name = "PASSWORD",
        help_heading = "Connection",
        hide = true
    )]
    pub(super) password: Option<String>,

    /// Managed tunnel provider. auto falls back across free providers when startup or health checks fail.
    #[arg(long, value_enum, default_value_t = TunnelProvider::Auto, help_heading = "Connection", hide = true)]
    pub(super) tunnel_provider: TunnelProvider,

    /// Existing persistent Microsoft Dev Tunnel ID used by the explicit dev-tunnel provider.
    #[arg(
        long,
        required_if_eq("tunnel_provider", "dev-tunnel"),
        help_heading = "Connection",
        hide = true
    )]
    pub(super) dev_tunnel_id: Option<String>,

    /// Send every tunnel URL to this phone number or email over local iMessage (macOS only).
    #[arg(long, help_heading = "Connection", hide = true)]
    pub(super) imessage_to: Option<String>,

    /// Keep WCode local and disable the managed public tunnel.
    #[arg(long, global = true, help_heading = "Connection")]
    pub(super) no_tunnel: bool,

    /// Read-only mode: disable file modification tools.
    #[arg(long = "read-only", global = true, action = ArgAction::SetFalse, default_value_t = true, help_heading = "Safety")]
    pub(super) allow_write: bool,

    /// Do not let WCode run build, test, or repository commands.
    #[arg(long = "no-exec", global = true, action = ArgAction::SetFalse, default_value_t = true, help_heading = "Safety")]
    pub(super) allow_exec: bool,

    /// Do not discover or run language servers.
    #[arg(long = "no-semantic", global = true, action = ArgAction::SetFalse, default_value_t = true, help_heading = "Safety")]
    pub(super) allow_semantic: bool,

    /// Allow approved tools to work across your Home directory and run commands without WCode command-policy filtering. Filesystem tools still keep their path-integrity boundaries.
    #[arg(long, global = true, conflicts_with_all = ["allow_write", "allow_exec", "allow_semantic"], help_heading = "Safety")]
    pub(super) full_access: bool,

    /// Pre-authorize non-command RiskyExecution lanes such as unprofiled LSP or advanced stage executors. Normal run_command execution does not require this flag.
    #[arg(long, help_heading = "Safety", hide = true)]
    pub(super) allow_risky_exec: bool,

    /// Allow replacements that empty a file or remove most of its content.
    #[arg(long, help_heading = "Safety", hide = true)]
    pub(super) allow_destructive_writes: bool,

    /// Allow nested or parent/child workspace roots in one process.
    #[arg(long, help_heading = "Safety", hide = true)]
    pub(super) allow_overlapping_workspaces: bool,

    /// Allow exposing a filesystem root or the current user's home directory.
    #[arg(long, help_heading = "Safety", hide = true)]
    pub(super) allow_broad_workspace: bool,

    #[command(flatten)]
    pub(super) resources: ResourceArgs,

    /// Print resolved configuration as JSON and exit without starting or changing anything.
    #[arg(long, global = true, help_heading = "Experience")]
    pub(super) show_config: bool,

    /// Estimated USD cost per million input tokens used for the TUI savings estimate.
    #[arg(long, default_value_t = DEFAULT_INPUT_TOKEN_PRICE_PER_MILLION_USD, help_heading = "Runtime", hide = true)]
    pub(super) input_token_price_per_million_usd: f64,

    /// Do not start the native menu bar companion. CI and SSH sessions skip it automatically.
    #[arg(long, global = true, help_heading = "Experience")]
    pub(super) no_menu_bar: bool,

    /// Hide the live terminal activity view.
    #[arg(long = "no-monitor", global = true, action = ArgAction::SetFalse, default_value_t = true, help_heading = "Experience")]
    pub(super) monitor: bool,

    /// Do not offer to install a missing managed-tunnel dependency.
    #[arg(long, help_heading = "Experience", hide = true)]
    pub(super) no_install: bool,

    /// Open the WCode setup page in your browser after startup.
    #[arg(long = "open", default_value_t = false, help_heading = "Experience")]
    pub(super) open_setup: bool,

    /// Open the Observatory in your browser once the runtime is reachable.
    #[arg(
        long = "open-observatory",
        default_value_t = false,
        help_heading = "Experience"
    )]
    pub(super) open_observatory: bool,

    /// Do not open the Observatory automatically on an interactive terminal.
    #[arg(
        long = "no-open-observatory",
        default_value_t = false,
        help_heading = "Experience"
    )]
    pub(super) no_open_observatory: bool,

    /// Allow idle system sleep while wcode is running.
    #[arg(long, help_heading = "Runtime", hide = true)]
    pub(super) allow_sleep: bool,

    /// Deprecated alias, accepted and ignored; the setup hub no longer auto-opens.
    #[arg(
        long = "no-install-chatgpt",
        action = ArgAction::SetFalse,
        default_value_t = true,
        hide = true
    )]
    pub(super) legacy_open_setup: bool,
}

/// Workspace roots inherited from the calling agent's session.
///
/// An agent that already knows its project context can hand the whole set to
/// wcode instead of configuring one root at a time:
///
/// - `WCODE_WORKSPACES`: a path list. Entries are separated by the platform
///   path-list separator (`:` on Unix, `;` on Windows) and/or by newlines.
/// - `WCODE_WORKSPACES_FILE`: a file holding one absolute or relative root per
///   line (`#` starts a comment), convenient when the session already persists
///   its context somewhere.
///
/// An explicit `--workspace` always wins. When neither is present, the current
/// directory is used, exactly as before.
pub(super) fn parse_workspace_list(raw: &str) -> Vec<PathBuf> {
    let separator = if cfg!(windows) { ';' } else { ':' };
    raw.split(['\n', '\r', separator])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(PathBuf::from)
        .collect()
}

pub(super) fn parse_workspace_file(contents: &str) -> Vec<PathBuf> {
    contents
        .lines()
        .map(str::trim)
        .filter(|entry| !entry.is_empty() && !entry.starts_with('#'))
        .map(PathBuf::from)
        .collect()
}

pub(super) fn session_workspace_roots() -> Option<Vec<PathBuf>> {
    if let Ok(raw) = std::env::var("WCODE_WORKSPACES") {
        let roots = parse_workspace_list(&raw);
        if !roots.is_empty() {
            return Some(roots);
        }
    }
    let Ok(path) = std::env::var("WCODE_WORKSPACES_FILE") else {
        return None;
    };
    let Ok(contents) = std::fs::read_to_string(path.trim()) else {
        return None;
    };
    let roots = parse_workspace_file(&contents);
    if roots.is_empty() {
        None
    } else {
        Some(roots)
    }
}

/// Session inheritance can supply several roots. Naming them turns a dead end
/// into the `-w` choice the operator needs; the caller adds its own suffix.
pub(super) fn single_workspace<'a>(roots: &'a [PathBuf], what: &str) -> Result<&'a PathBuf> {
    if roots.len() == 1 {
        return Ok(&roots[0]);
    }
    let list = roots
        .iter()
        .map(|root| root.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    bail!("{what} accepts one project workspace; inherited {list}")
}

pub(super) fn resolved_workspace_roots(explicit: &[PathBuf]) -> Vec<PathBuf> {
    if !explicit.is_empty() {
        return explicit.to_vec();
    }
    session_workspace_roots().unwrap_or_else(|| vec![PathBuf::from(".")])
}
