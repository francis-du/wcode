use crate::authorization::{AuthorizationRequest, AuthorizationStatus};
use crate::harness::ToolHarness;
use crate::workspace::Workspaces;
use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, Padding, Paragraph};
use ratatui::{Frame, Terminal};
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::io::{self, IsTerminal};
use std::process::{Command as StdCommand, Stdio as StdStdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

const MAX_RECENT_TASKS: usize = 48;
const MAX_TRAFFIC_EVENTS: usize = 4096;
const TRAFFIC_WINDOW: Duration = Duration::from_secs(60);
const ACTIVE_REFRESH_INTERVAL: Duration = Duration::from_millis(150);
const IDLE_REFRESH_INTERVAL: Duration = Duration::from_millis(500);
const ESTIMATED_BYTES_PER_TOKEN: f64 = 4.0;
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[path = "i18n.rs"]
mod i18n;
use i18n::UiLanguage;

#[path = "detail.rs"]
mod monitor_detail;
use monitor_detail::*;

#[path = "intelligence.rs"]
mod monitor_intelligence;
use monitor_intelligence::*;

#[path = "commands.rs"]
mod monitor_commands;
use monitor_commands::*;

#[path = "overlays.rs"]
mod monitor_overlays;
use monitor_overlays::*;

#[path = "runtime.rs"]
mod monitor_runtime;
use monitor_runtime::*;

#[path = "shell.rs"]
mod monitor_shell;
use monitor_shell::*;

#[path = "metrics.rs"]
mod monitor_metrics;
use monitor_metrics::*;

#[path = "state.rs"]
mod monitor_state;
#[cfg(test)]
use monitor_state::trim_history;

#[path = "theme.rs"]
mod monitor_theme;
use monitor_theme::*;

#[derive(Clone)]
pub struct TaskMonitor {
    state: Arc<Mutex<MonitorState>>,
}

struct MonitorState {
    next_id: u64,
    started_at: Instant,
    workspaces: BTreeMap<String, WorkspaceStats>,
    intelligence: BTreeMap<String, IntelligenceStats>,
    tasks: VecDeque<TaskRecord>,
    traffic: VecDeque<TrafficEvent>,
    oauth_client_registered: bool,
    oauth_authorized: bool,
    chatgpt_connected: bool,
    initialize_count: u64,
    last_initialize: Option<Instant>,
    last_mcp_seen: Option<Instant>,
    public_endpoint: Option<String>,
    tunnels: Vec<(String, String)>,
    public_url_healthy: Option<bool>,
    public_url_last_checked: Option<Instant>,
    public_url_consecutive_failures: u8,
    public_url_error: Option<String>,
    tunnel_running: Option<bool>,
    tunnel_error: Option<String>,
    active_total: u64,
    peak_active: u64,
    observed_active: u64,
    observed_queued: u64,
}

#[derive(Clone, Default)]
struct WorkspaceStats {
    queued: u64,
    active: u64,
    completed: u64,
    failed: u64,
    calls: u64,
    request_bytes: u64,
    response_bytes: u64,
    context_bytes_avoided: u64,
    agent_context_calls: u64,
    agent_context_model_bytes: u64,
    agent_context_bytes_avoided: u64,
    agent_repo_map_cache_hits: u64,
}

#[derive(Clone, Default)]
struct IntelligenceStats {
    design_state: Option<String>,
    requirements: u64,
    components: u64,
    implementation_coverage: Option<u64>,
    verification_coverage: Option<u64>,
    scope_source_files: u64,
    scope_mapped_files: u64,
    scope_unmapped_files: u64,
    graph_nodes: u64,
    graph_edges: u64,
    graph_precision: Option<String>,
    graph_added_nodes: u64,
    graph_removed_nodes: u64,
    graph_changed_nodes: u64,
    graph_added_edges: u64,
    graph_removed_edges: u64,
    graph_changed_edges: u64,
    semantic_confirmed: u64,
    semantic_candidates: u64,
    lsp_available: u64,
    lsp_launch_ready: u64,
    lsp_validated: u64,
    lsp_automatic: u64,
    lsp_runnable: u64,
    lsp_fresh: u64,
    lsp_stale: u64,
    lsp_sessions: u64,
    lsp_documents: u64,
    lsp_starts: u64,
    lsp_requests: u64,
    drift_findings: u64,
    risk_level: Option<String>,
    evidence_total: u64,
    evidence_failed: u64,
    evidence_disagreed: u64,
    verification_ready: Option<bool>,
    verification_blockers: u64,
    reconciliation_converged: Option<bool>,
    reconciliation_pending: u64,
    updated_at: Option<Instant>,
    refreshing: bool,
    refresh_error: Option<String>,
}

#[derive(Clone)]
struct TaskRecord {
    id: u64,
    workspace: String,
    tool: String,
    detail: String,
    status: TaskStatus,
    slot_counted: bool,
    queued_at: Instant,
    started_at: Option<Instant>,
    finished_at: Option<Instant>,
    request_bytes: u64,
    response_bytes: u64,
    context_bytes_avoided: u64,
}

#[derive(Clone)]
struct TrafficEvent {
    at: Instant,
    requests: u64,
    request_bytes: u64,
    response_bytes: u64,
    context_bytes_avoided: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TaskStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

pub struct TaskTicket {
    monitor: TaskMonitor,
    id: u64,
    finished: bool,
}

pub struct MonitorConfig {
    pub version: String,
    pub instance_id: String,
    pub local_health_url: String,
    /// Live public origin; shared with the tunnel supervisor so the rendered
    /// MCP/setup URLs update when a tunnel connects after startup.
    pub public_url: Arc<std::sync::RwLock<String>>,
    pub intelligence_url: String,
    pub project_url: String,
    pub author_url: String,
    pub author_handle: String,
    pub pairing_code: String,
    pub max_parallel: usize,
    pub input_token_price_per_million_usd: f64,
    pub semantic_auto: bool,
    pub workspaces: Workspaces,
    pub harness: ToolHarness,
}

impl MonitorConfig {
    pub(crate) fn public_url(&self) -> String {
        self.public_url
            .read()
            .expect("public url lock poisoned")
            .clone()
    }

    pub(crate) fn mcp_url(&self) -> String {
        format!("{}/mcp", self.public_url())
    }

    pub(crate) fn setup_url(&self) -> String {
        self.public_url()
    }
}

pub struct MonitorRenderer {
    stop: watch::Sender<bool>,
    interrupted: watch::Receiver<bool>,
    join: tokio::task::JoinHandle<()>,
}

pub struct MonitorConnectionStatus {
    pub oauth_client_registered: bool,
    pub oauth_authorized: bool,
    pub chatgpt_initialized: bool,
    pub initialize_count: u64,
    pub last_initialize_seconds_ago: Option<u64>,
    pub last_mcp_seen_seconds_ago: Option<u64>,
    pub public_endpoint: Option<String>,
    pub public_url_healthy: Option<bool>,
    pub public_url_last_checked_seconds_ago: Option<u64>,
    pub public_url_consecutive_failures: u8,
    pub public_url_error: Option<String>,
    pub tunnel_running: Option<bool>,
    pub tunnel_error: Option<String>,
    pub active_tasks: u64,
    pub queued_tasks: u64,
    pub peak_active_tasks: u64,
}

struct MonitorSnapshot {
    started_at: Instant,
    workspaces: BTreeMap<String, WorkspaceStats>,
    intelligence: BTreeMap<String, IntelligenceStats>,
    tasks: Vec<TaskRecord>,
    traffic: Vec<TrafficEvent>,
    oauth_client_registered: bool,
    oauth_authorized: bool,
    chatgpt_connected: bool,
    initialize_count: u64,
    last_initialize: Option<Instant>,
    last_mcp_seen: Option<Instant>,
    public_endpoint: Option<String>,
    tunnels: Vec<(String, String)>,
    public_url_healthy: Option<bool>,
    public_url_last_checked: Option<Instant>,
    public_url_consecutive_failures: u8,
    public_url_error: Option<String>,
    tunnel_running: Option<bool>,
    tunnel_error: Option<String>,
    peak_active: u64,
    observed_active: u64,
    observed_queued: u64,
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalSession {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableMouseCapture, Hide) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        let backend = CrosstermBackend::new(stdout);
        match Terminal::new(backend) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(error) => {
                let mut stdout = io::stdout();
                let _ = execute!(stdout, DisableMouseCapture, Show, LeaveAlternateScreen);
                let _ = disable_raw_mode();
                Err(error)
            }
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableMouseCapture,
            Show,
            LeaveAlternateScreen
        );
        let _ = self.terminal.show_cursor();
    }
}

#[derive(Default)]
struct DashboardState {
    workspace_focus: usize,
    workspace_offset: usize,
    help_open: bool,
    intelligence_open: bool,
    commands_open: bool,
    command_offset: usize,
    workspace_input: Option<String>,
    workspace_message: Option<String>,
    authorization_focus: usize,
    language: UiLanguage,
}

impl DashboardState {
    fn clamp(&mut self, total: usize, visible: usize) {
        if total == 0 {
            self.workspace_focus = 0;
            self.workspace_offset = 0;
            return;
        }
        let visible = visible.max(1).min(total);
        self.workspace_focus = self.workspace_focus.min(total - 1);
        if self.workspace_focus < self.workspace_offset {
            self.workspace_offset = self.workspace_focus;
        }
        if self.workspace_focus >= self.workspace_offset + visible {
            self.workspace_offset = self.workspace_focus + 1 - visible;
        }
        self.workspace_offset = self.workspace_offset.min(total.saturating_sub(visible));
    }

    fn clamp_authorizations(&mut self, total: usize) {
        self.authorization_focus = if total == 0 {
            0
        } else {
            self.authorization_focus.min(total - 1)
        };
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/ui/monitor.rs"]
mod tests;
