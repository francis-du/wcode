use crate::authorization::{AuthorizationRequest, AuthorizationStatus};
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

#[path = "monitor_detail.rs"]
mod monitor_detail;
use monitor_detail::*;

#[path = "monitor_state.rs"]
mod monitor_state;
#[cfg(test)]
use monitor_state::trim_history;

const BG: Color = Color::Rgb(11, 13, 17);
const PANEL: Color = Color::Rgb(17, 20, 26);
const PANEL_ALT: Color = Color::Rgb(22, 26, 34);
const PANEL_ACTIVE: Color = Color::Rgb(25, 31, 41);
const GREEN: Color = Color::Rgb(100, 210, 139);
const CYAN: Color = Color::Rgb(96, 200, 224);
const BLUE: Color = Color::Rgb(125, 159, 255);
const PURPLE: Color = Color::Rgb(178, 145, 255);
const YELLOW: Color = Color::Rgb(232, 190, 92);
const RED: Color = Color::Rgb(235, 112, 112);
const GRAY: Color = Color::Rgb(133, 139, 153);
const DIM: Color = Color::Rgb(88, 94, 108);
const BORDER: Color = Color::Rgb(53, 59, 72);
const WHITE: Color = Color::Rgb(235, 238, 244);

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
    pub mcp_url: String,
    pub setup_url: String,
    pub intelligence_url: String,
    pub project_url: String,
    pub author_url: String,
    pub author_handle: String,
    pub pairing_code: String,
    pub max_parallel: usize,
    pub input_token_price_per_million_usd: f64,
    pub workspaces: Workspaces,
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

fn intelligence_url_for_workspace(base: &str, workspace: &str) -> String {
    let encoded = url::form_urlencoded::byte_serialize(workspace.as_bytes()).collect::<String>();
    let separator = if base.contains('#') { '&' } else { '#' };
    format!("{base}{separator}workspace={encoded}")
}

fn pending_authorizations(config: &MonitorConfig) -> Vec<AuthorizationRequest> {
    config
        .workspaces
        .authorization_requests(256)
        .into_iter()
        .filter(|request| request.status == AuthorizationStatus::Pending)
        .collect()
}

fn configured_workspaces(config: &MonitorConfig) -> Vec<(String, String, bool)> {
    config
        .workspaces
        .roots()
        .into_iter()
        .map(|(id, root)| {
            let is_default = id == config.workspaces.default_id();
            (id, root.display().to_string(), is_default)
        })
        .collect()
}

fn run_dashboard(
    monitor: TaskMonitor,
    config: MonitorConfig,
    stop_rx: watch::Receiver<bool>,
    interrupt_tx: watch::Sender<bool>,
) -> io::Result<()> {
    let mut session = TerminalSession::enter()?;
    let mut tick = 0usize;
    let mut ui = DashboardState::default();

    loop {
        if *stop_rx.borrow() {
            break;
        }

        let size = session.terminal.size()?;
        let workspace_count = config.workspaces.roots().len();
        let visible = workspace_column_count(size.width, workspace_count);
        ui.clamp(workspace_count, visible);
        ui.clamp_authorizations(pending_authorizations(&config).len());
        let snapshot = monitor.snapshot();
        session
            .terminal
            .draw(|frame| draw_dashboard(frame, &snapshot, &config, tick, &ui))?;
        tick = tick.wrapping_add(1);

        let refresh_interval = dashboard_refresh_interval(&snapshot);
        if event::poll(refresh_interval)? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
                    {
                        let _ = interrupt_tx.send(true);
                        break;
                    }
                    if ui.workspace_input.is_some() {
                        match key.code {
                            KeyCode::Esc => {
                                ui.workspace_input = None;
                                ui.workspace_message =
                                    Some(ui.language.tr("workspace add cancelled").to_owned());
                            }
                            KeyCode::Enter => {
                                let path = ui.workspace_input.take().unwrap_or_default();
                                if path.trim().is_empty() {
                                    ui.workspace_message = Some(
                                        ui.language.tr("workspace path cannot be empty").to_owned(),
                                    );
                                } else {
                                    match config.workspaces.add_workspace(path.trim()) {
                                        Ok((id, root)) => {
                                            monitor.register_workspace(id.clone());
                                            ui.workspace_message = Some(format!(
                                                "authorized workspace {id}: {}",
                                                root.display()
                                            ));
                                            let count = config.workspaces.roots().len();
                                            ui.workspace_focus = count.saturating_sub(1);
                                            ui.clamp(
                                                count,
                                                workspace_column_count(size.width, count),
                                            );
                                        }
                                        Err(error) => {
                                            ui.workspace_message =
                                                Some(format!("workspace rejected: {error}"));
                                        }
                                    }
                                }
                            }
                            KeyCode::Backspace => {
                                if let Some(input) = ui.workspace_input.as_mut() {
                                    input.pop();
                                }
                            }
                            KeyCode::Char(character)
                                if !key.modifiers.contains(KeyModifiers::CONTROL)
                                    && !key.modifiers.contains(KeyModifiers::ALT) =>
                            {
                                if let Some(input) = ui.workspace_input.as_mut() {
                                    if input.chars().count() < 1024 {
                                        input.push(character);
                                    }
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }
                    match key.code {
                        KeyCode::Esc => {
                            ui.help_open = false;
                            ui.intelligence_open = false;
                            ui.workspace_message = None;
                        }
                        KeyCode::Char('?') if key.kind == KeyEventKind::Press => {
                            ui.help_open = true;
                            ui.intelligence_open = false;
                        }
                        KeyCode::Char('i') | KeyCode::Char('I')
                            if key.kind == KeyEventKind::Press =>
                        {
                            ui.intelligence_open = !ui.intelligence_open;
                            ui.help_open = false;
                        }
                        KeyCode::Char('l') | KeyCode::Char('L')
                            if key.kind == KeyEventKind::Press =>
                        {
                            ui.language = ui.language.toggle();
                            ui.workspace_message = Some(format!(
                                "{}: {}",
                                ui.language.tr("LANGUAGE"),
                                ui.language.name()
                            ));
                        }
                        KeyCode::Char('o') | KeyCode::Char('O')
                            if key.kind == KeyEventKind::Press =>
                        {
                            let _ = open_external_url(&config.setup_url);
                        }
                        KeyCode::Char('w') | KeyCode::Char('W')
                            if key.kind == KeyEventKind::Press =>
                        {
                            let workspaces = configured_workspaces(&config);
                            let url = workspaces
                                .get(ui.workspace_focus.min(workspaces.len().saturating_sub(1)))
                                .map(|workspace| {
                                    intelligence_url_for_workspace(
                                        &config.intelligence_url,
                                        &workspace.0,
                                    )
                                })
                                .unwrap_or_else(|| config.intelligence_url.clone());
                            let _ = open_external_url(&url);
                        }
                        KeyCode::Char('g') | KeyCode::Char('G')
                            if key.kind == KeyEventKind::Press =>
                        {
                            let _ = open_external_url(&config.project_url);
                        }
                        KeyCode::Char('a') | KeyCode::Char('A')
                            if key.kind == KeyEventKind::Press =>
                        {
                            let _ = open_external_url(&config.author_url);
                        }
                        KeyCode::Char('+') if key.kind == KeyEventKind::Press => {
                            ui.workspace_input = Some(String::new());
                            ui.workspace_message = None;
                            ui.help_open = false;
                            ui.intelligence_open = false;
                        }
                        KeyCode::Char('y') | KeyCode::Char('Y')
                            if key.kind == KeyEventKind::Press =>
                        {
                            let pending = pending_authorizations(&config);
                            if let Some(request) = pending.get(ui.authorization_focus) {
                                let approved =
                                    config.workspaces.approve_authorization_session(&request.id);
                                ui.workspace_message = Some(if approved {
                                    format!(
                                        "{} {} · {}",
                                        ui.language.tr("approved"),
                                        request.id,
                                        ui.language.tr("retry the tool")
                                    )
                                } else {
                                    format!(
                                        "{} {}",
                                        request.id,
                                        ui.language.tr("authorization is no longer pending")
                                    )
                                });
                                ui.clamp_authorizations(pending_authorizations(&config).len());
                            }
                        }
                        KeyCode::Char('n') | KeyCode::Char('N')
                            if key.kind == KeyEventKind::Press =>
                        {
                            let pending = pending_authorizations(&config);
                            if let Some(request) = pending.get(ui.authorization_focus) {
                                let denied = config.workspaces.deny_authorization(&request.id);
                                ui.workspace_message = Some(if denied {
                                    format!("{} {}", ui.language.tr("denied"), request.id)
                                } else {
                                    format!(
                                        "{} {}",
                                        request.id,
                                        ui.language.tr("authorization is no longer pending")
                                    )
                                });
                                ui.clamp_authorizations(pending_authorizations(&config).len());
                            }
                        }
                        KeyCode::Up
                            if key.kind == KeyEventKind::Press
                                && !ui.help_open
                                && !ui.intelligence_open =>
                        {
                            let total = pending_authorizations(&config).len();
                            if total > 0 {
                                ui.authorization_focus = ui.authorization_focus.saturating_sub(1);
                            }
                        }
                        KeyCode::Down
                            if key.kind == KeyEventKind::Press
                                && !ui.help_open
                                && !ui.intelligence_open =>
                        {
                            let total = pending_authorizations(&config).len();
                            if total > 0 {
                                ui.authorization_focus =
                                    ui.authorization_focus.saturating_add(1).min(total - 1);
                            }
                        }
                        KeyCode::Left => {
                            let step = if key.modifiers.contains(KeyModifiers::SHIFT) {
                                visible.max(1)
                            } else {
                                1
                            };
                            ui.workspace_focus = ui.workspace_focus.saturating_sub(step);
                            ui.clamp(config.workspaces.roots().len(), visible);
                        }
                        KeyCode::Right => {
                            let step = if key.modifiers.contains(KeyModifiers::SHIFT) {
                                visible.max(1)
                            } else {
                                1
                            };
                            let count = config.workspaces.roots().len();
                            ui.workspace_focus = ui
                                .workspace_focus
                                .saturating_add(step)
                                .min(count.saturating_sub(1));
                            ui.clamp(count, visible);
                        }
                        _ => {}
                    }
                }
                Event::Mouse(mouse) => {
                    if let Some(url) =
                        dashboard_link_at(&mouse, size.width, size.height, &ui, &config)
                    {
                        let _ = open_external_url(url);
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }

    Ok(())
}

fn dashboard_link_at<'a>(
    mouse: &MouseEvent,
    width: u16,
    height: u16,
    ui: &DashboardState,
    config: &'a MonitorConfig,
) -> Option<&'a str> {
    if mouse.kind != MouseEventKind::Down(MouseButton::Left) || width == 0 || height == 0 {
        return None;
    }
    let point = (mouse.column, mouse.row);
    let area = Rect::new(0, 0, width, height);

    if ui.help_open {
        let popup_width = area.width.saturating_sub(6).clamp(36, 98);
        let popup_height = area.height.saturating_sub(4).clamp(12, 18);
        let popup = Rect::new(
            area.x + area.width.saturating_sub(popup_width) / 2,
            area.y + area.height.saturating_sub(popup_height) / 2,
            popup_width,
            popup_height,
        );
        let inner = Rect::new(
            popup.x.saturating_add(2),
            popup.y.saturating_add(1),
            popup.width.saturating_sub(4),
            popup.height.saturating_sub(2),
        );
        if popup_width < 74 || popup_height < 16 {
            for (row, url) in [
                (6u16, config.project_url.as_str()),
                (7, config.author_url.as_str()),
                (8, config.setup_url.as_str()),
                (9, config.local_health_url.as_str()),
            ] {
                if point_in_rect(point, Rect::new(inner.x, inner.y + row, inner.width, 1)) {
                    return Some(url);
                }
            }
        } else {
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
                .split(inner);
            for (row, url) in [
                (7u16, config.project_url.as_str()),
                (8, config.author_url.as_str()),
                (9, config.setup_url.as_str()),
                (10, config.local_health_url.as_str()),
            ] {
                if point_in_rect(
                    point,
                    Rect::new(columns[1].x, columns[1].y + row, columns[1].width, 1),
                ) {
                    return Some(url);
                }
            }
        }
    }

    if width >= 124 && mouse.row == height.saturating_sub(1) {
        let footer = Rect::new(0, height.saturating_sub(1), width, 1);
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
            .split(footer);
        let project = config
            .project_url
            .strip_prefix("https://")
            .unwrap_or(&config.project_url)
            .trim_end_matches('/');
        let project_x = columns[0].x.saturating_add("  wcode  ".len() as u16);
        let project_rect = Rect::new(project_x, footer.y, project.len() as u16, 1);
        if point_in_rect(point, project_rect) {
            return Some(&config.project_url);
        }
        let author_x = project_rect
            .x
            .saturating_add(project_rect.width)
            .saturating_add("  by  ".len() as u16);
        let author_rect = Rect::new(author_x, footer.y, config.author_handle.len() as u16, 1);
        if point_in_rect(point, author_rect) {
            return Some(&config.author_url);
        }

        let pending_authorizations = config
            .workspaces
            .authorization_requests(256)
            .iter()
            .filter(|request| request.status == AuthorizationStatus::Pending)
            .count();
        let key_width = |key: &str| key.chars().count() as u16 + 2;
        let label_width = |label: &str| label.chars().count() as u16 + 3;
        let pending_width = if pending_authorizations > 0 {
            pending_authorizations.to_string().chars().count() as u16 + 2
        } else {
            1
        };
        let shortcuts_width = key_width("←/→")
            + label_width(ui.language.tr("workspace"))
            + key_width("O")
            + label_width(ui.language.tr("setup"))
            + key_width("W")
            + label_width(ui.language.tr("web"))
            + key_width("L")
            + label_width(ui.language.name())
            + key_width("+")
            + 1
            + key_width("Y/N")
            + pending_width
            + key_width("?")
            + 1
            + key_width("^C");
        let shortcuts_x = columns[1]
            .x
            .saturating_add(columns[1].width.saturating_sub(shortcuts_width));
        let setup_prefix_width = key_width("←/→") + label_width(ui.language.tr("workspace"));
        let setup_x = shortcuts_x.saturating_add(setup_prefix_width);
        let setup_rect = Rect::new(
            setup_x,
            footer.y,
            key_width("O") + label_width(ui.language.tr("setup")),
            1,
        );
        if point_in_rect(point, setup_rect) {
            return Some(&config.setup_url);
        }
    }
    None
}

fn point_in_rect((x, y): (u16, u16), rect: Rect) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

fn dashboard_refresh_interval(snapshot: &MonitorSnapshot) -> Duration {
    if snapshot
        .tasks
        .iter()
        .any(|task| matches!(task.status, TaskStatus::Queued | TaskStatus::Running))
    {
        ACTIVE_REFRESH_INTERVAL
    } else {
        IDLE_REFRESH_INTERVAL
    }
}

fn draw_dashboard(
    frame: &mut Frame<'_>,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
    tick: usize,
    ui: &DashboardState,
) {
    let area = frame.area();
    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    let compact = area.width < 92;
    let dense = area.height < 28;
    let header_height = 8;
    let setup_height = if snapshot.chatgpt_connected {
        0
    } else if compact || dense {
        7
    } else {
        10
    };
    let overview_height = if compact || dense { 4 } else { 6 };
    let minimum_activity_height = 4;
    let fixed_height = header_height + setup_height + overview_height + minimum_activity_height + 1;

    if area.width < 40 || area.height < fixed_height {
        render_too_small(frame, area, config, ui.language);
        return;
    }

    let throughput_height = if area.height >= fixed_height.saturating_add(4) {
        4
    } else {
        0
    };
    let mut constraints = vec![Constraint::Length(header_height)];
    if setup_height > 0 {
        constraints.push(Constraint::Length(setup_height));
    }
    constraints.push(Constraint::Length(overview_height));
    constraints.push(Constraint::Min(minimum_activity_height));
    if throughput_height > 0 {
        constraints.push(Constraint::Length(throughput_height));
    }
    constraints.push(Constraint::Length(1));
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut row = 0usize;
    render_header(
        frame,
        rows[row],
        snapshot,
        config,
        tick,
        compact,
        ui.language,
    );
    row += 1;
    if setup_height > 0 {
        render_setup(frame, rows[row], snapshot, config, compact, ui.language);
        row += 1;
    }
    render_overview(frame, rows[row], snapshot, config, compact, ui.language);
    row += 1;
    render_workspace_activity(frame, rows[row], snapshot, config, tick, ui);
    row += 1;
    if throughput_height > 0 {
        render_throughput(frame, rows[row], snapshot, config, ui.language);
        row += 1;
    }
    render_footer(frame, rows[row], config, ui.language);

    if ui.intelligence_open {
        render_intelligence_overlay(
            frame,
            area,
            snapshot,
            config,
            ui.workspace_focus,
            ui.language,
        );
    } else if ui.help_open {
        render_help_overlay(frame, area, config, ui.language);
    }
    if !ui.help_open && !ui.intelligence_open && ui.workspace_input.is_none() {
        let pending = pending_authorizations(config);
        if !pending.is_empty() {
            render_authorization_overlay(
                frame,
                area,
                &pending,
                ui.authorization_focus,
                ui.language,
            );
        }
    }
    if let Some(input) = ui.workspace_input.as_deref() {
        render_workspace_input_overlay(frame, area, input, ui.language);
    }
    if let Some(message) = ui.workspace_message.as_deref() {
        render_status_message(frame, area, message, ui.language);
    }
}

fn render_too_small(
    frame: &mut Frame<'_>,
    area: Rect,
    config: &MonitorConfig,
    language: UiLanguage,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(PANEL))
        .padding(Padding::uniform(1))
        .title(Line::from(vec![
            Span::styled(
                " WC ",
                Style::default()
                    .fg(BG)
                    .bg(PURPLE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " wcode ",
                Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
            ),
        ]))
        .title(
            Line::from(Span::styled(
                format!(" INSTANCE {} ", truncate_end(&config.instance_id, 8)),
                Style::default().fg(DIM),
            ))
            .right_aligned(),
        );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                language.tr("Terminal needs a little more room"),
                Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!("current size  {} × {}", area.width, area.height),
                Style::default().fg(GRAY),
            )),
            Line::from(Span::styled(
                language.tr("resize the window to restore the live dashboard"),
                Style::default().fg(DIM),
            )),
        ])
        .alignment(ratatui::layout::Alignment::Center)
        .block(block),
        area,
    );
}

fn render_header(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
    tick: usize,
    compact: bool,
    language: UiLanguage,
) {
    let totals = totals(snapshot);
    let idle = snapshot
        .last_mcp_seen
        .is_some_and(|seen| seen.elapsed() >= Duration::from_secs(300));
    let (icon, state, detail, color) = if snapshot.tunnel_running == Some(false) {
        (
            "×",
            language.tr("CLOUDFLARED PROCESS EXITED"),
            snapshot
                .tunnel_error
                .as_deref()
                .map(|error| truncate_end(error, 54))
                .unwrap_or_else(|| {
                    language
                        .tr("tunnel process is no longer running")
                        .to_owned()
                }),
            RED,
        )
    } else if snapshot.public_url_healthy == Some(false) {
        (
            "×",
            language.tr("PUBLIC URL UNAVAILABLE"),
            format!(
                "{} consecutive health checks failed",
                snapshot.public_url_consecutive_failures
            ),
            RED,
        )
    } else if snapshot.chatgpt_connected && idle {
        (
            "◐",
            language.tr("MCP client idle"),
            format!(
                "last seen {} · Remote MCP",
                last_seen_text(snapshot.last_mcp_seen)
            ),
            YELLOW,
        )
    } else if snapshot.chatgpt_connected {
        (
            "●",
            language.tr("MCP client connected"),
            format!(
                "last seen {} · Remote MCP",
                last_seen_text(snapshot.last_mcp_seen)
            ),
            GREEN,
        )
    } else if snapshot.oauth_authorized {
        (
            "◐",
            language.tr("OAuth authorized"),
            language.tr("waiting for MCP handshake").to_owned(),
            YELLOW,
        )
    } else {
        (
            "○",
            language.tr("Setup required"),
            language.tr("press O to open Connector setup").to_owned(),
            GRAY,
        )
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if snapshot.chatgpt_connected {
            color
        } else {
            BORDER
        }))
        .style(Style::default().bg(PANEL))
        .padding(Padding::horizontal(1))
        .title(Line::from(vec![
            Span::styled(
                " WC ",
                Style::default()
                    .fg(BG)
                    .bg(PURPLE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" wcode {} ", config.version),
                Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
            ),
        ]))
        .title(
            Line::from(Span::styled(
                format!(" INSTANCE {} ", truncate_end(&config.instance_id, 8)),
                Style::default().fg(DIM),
            ))
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if compact || inner.width < 76 {
        let lines = vec![
            Line::from(vec![
                Span::styled(format!("{icon} "), Style::default().fg(color)),
                Span::styled(
                    state,
                    Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("  {detail}"), Style::default().fg(GRAY)),
            ]),
            Line::from(vec![
                Span::styled("MCP     ", Style::default().fg(DIM)),
                Span::styled(
                    truncate_middle(&config.mcp_url, inner.width.saturating_sub(8) as usize),
                    Style::default().fg(BLUE),
                ),
            ]),
            Line::from(vec![
                Span::styled("HEALTH  ", Style::default().fg(DIM)),
                Span::styled(
                    truncate_middle(
                        &config.local_health_url,
                        inner.width.saturating_sub(8) as usize,
                    ),
                    Style::default().fg(CYAN),
                ),
            ]),
            Line::from(vec![
                Span::styled("URL     ", Style::default().fg(DIM)),
                Span::styled(
                    public_url_health_text(snapshot),
                    Style::default().fg(public_url_health_color(snapshot)),
                ),
                Span::styled("   AUTH  ", Style::default().fg(DIM)),
                Span::styled("● ACCESS TOKEN · NO EXPIRY", Style::default().fg(GREEN)),
            ]),
            Line::from(vec![
                Span::styled("TUNNEL  ", Style::default().fg(DIM)),
                Span::styled(
                    tunnel_status_text(snapshot),
                    Style::default().fg(tunnel_status_color(snapshot)),
                ),
                Span::styled("   INIT  ", Style::default().fg(DIM)),
                Span::styled(
                    initialize_status_text(snapshot),
                    Style::default().fg(PURPLE),
                ),
            ]),
            Line::from(vec![
                Span::styled("SLOTS ", Style::default().fg(DIM)),
                Span::styled(
                    format!("{} / {}", totals.active, config.max_parallel),
                    Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
                ),
                Span::styled("   PEAK ", Style::default().fg(DIM)),
                Span::styled(
                    snapshot.peak_active.to_string(),
                    Style::default().fg(PURPLE),
                ),
                Span::styled("   VERIFY CODE ", Style::default().fg(DIM)),
                Span::styled(
                    config.pairing_code.clone(),
                    Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
                ),
            ]),
        ];
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(64), Constraint::Percentage(36)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(format!("{icon} "), Style::default().fg(color)),
                Span::styled(
                    state,
                    Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("  {detail}"), Style::default().fg(GRAY)),
            ]),
            Line::from(vec![
                Span::styled("MCP     ", Style::default().fg(DIM)),
                Span::styled(
                    truncate_middle(&config.mcp_url, columns[0].width.saturating_sub(9) as usize),
                    Style::default().fg(BLUE),
                ),
            ]),
            Line::from(vec![
                Span::styled("HEALTH  ", Style::default().fg(DIM)),
                Span::styled(
                    truncate_middle(
                        &config.local_health_url,
                        columns[0].width.saturating_sub(9) as usize,
                    ),
                    Style::default().fg(CYAN),
                ),
            ]),
            Line::from(vec![
                Span::styled("URL     ", Style::default().fg(DIM)),
                Span::styled(
                    public_url_health_text(snapshot),
                    Style::default().fg(public_url_health_color(snapshot)),
                ),
            ]),
            Line::from(vec![
                Span::styled("AUTH    ", Style::default().fg(DIM)),
                Span::styled("● ACCESS TOKEN · NO EXPIRY", Style::default().fg(GREEN)),
            ]),
        ]),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(spinner_frame(tick), Style::default().fg(CYAN)),
                Span::styled(
                    " LIVE",
                    Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  UPTIME {}", short_duration(snapshot.started_at.elapsed())),
                    Style::default().fg(GRAY),
                ),
            ])
            .right_aligned(),
            Line::from(vec![
                Span::styled("SLOTS ", Style::default().fg(DIM)),
                Span::styled(
                    format!("{} / {}", totals.active, config.max_parallel),
                    Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
                ),
                Span::styled("   PEAK ", Style::default().fg(DIM)),
                Span::styled(
                    snapshot.peak_active.to_string(),
                    Style::default().fg(PURPLE),
                ),
                Span::styled("   VERIFY CODE ", Style::default().fg(DIM)),
                Span::styled(
                    config.pairing_code.clone(),
                    Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
                ),
            ])
            .right_aligned(),
            Line::from(vec![
                Span::styled("TUNNEL  ", Style::default().fg(DIM)),
                Span::styled(
                    tunnel_status_text(snapshot),
                    Style::default().fg(tunnel_status_color(snapshot)),
                ),
            ])
            .right_aligned(),
            Line::from(vec![
                Span::styled("INIT  ", Style::default().fg(DIM)),
                Span::styled(
                    initialize_status_text(snapshot),
                    Style::default().fg(PURPLE),
                ),
            ])
            .right_aligned(),
            Line::from(Span::styled(
                if snapshot.public_url_healthy == Some(false)
                    || snapshot.tunnel_running == Some(false)
                {
                    "Restart wcode, then update the Connector URL"
                } else {
                    "PUBLIC URL MONITORING ACTIVE"
                },
                Style::default().fg(
                    if snapshot.public_url_healthy == Some(false)
                        || snapshot.tunnel_running == Some(false)
                    {
                        RED
                    } else {
                        DIM
                    },
                ),
            ))
            .right_aligned(),
        ]),
        columns[1],
    );
}

fn public_url_health_text(snapshot: &MonitorSnapshot) -> String {
    let checked = snapshot
        .public_url_last_checked
        .map(|seen| last_seen_text(Some(seen)))
        .unwrap_or_else(|| "not checked yet".to_owned());
    match snapshot.public_url_healthy {
        Some(true) => format!("● HEALTHY · checked {checked}"),
        Some(false) => format!(
            "× UNAVAILABLE · {} failures · {}",
            snapshot.public_url_consecutive_failures,
            snapshot
                .public_url_error
                .as_deref()
                .map(|error| truncate_end(error, 28))
                .unwrap_or(checked)
        ),
        None if snapshot.public_url_consecutive_failures > 0 => format!(
            "◐ CHECKING · {} failure(s) · checked {checked}",
            snapshot.public_url_consecutive_failures
        ),
        None => format!("○ PENDING · {checked}"),
    }
}

fn public_url_health_color(snapshot: &MonitorSnapshot) -> Color {
    match snapshot.public_url_healthy {
        Some(true) => GREEN,
        Some(false) => RED,
        None if snapshot.public_url_consecutive_failures > 0 => YELLOW,
        None => GRAY,
    }
}

fn render_overview(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
    compact: bool,
    language: UiLanguage,
) {
    let totals = totals(snapshot);
    let observed_active = snapshot.observed_active.max(totals.active);
    let observed_queued = snapshot.observed_queued.max(totals.queued);
    let success = success_rate(totals.completed, totals.failed);
    let (requests, rx_30s, tx_30s) = window_totals(snapshot, Duration::from_secs(30));
    let rate = requests as f64 / 30.0;
    let context_tokens = estimated_tokens(totals.response_bytes);
    let saved_tokens = estimated_tokens(totals.context_bytes_avoided);
    let estimated_context_cost = estimated_cost_usd(
        totals.response_bytes,
        config.input_token_price_per_million_usd,
    );
    let estimated_savings = estimated_cost_usd(
        totals.context_bytes_avoided,
        config.input_token_price_per_million_usd,
    );
    let agent_avg_tokens = estimated_tokens(totals.agent_context_model_bytes)
        .checked_div(totals.agent_context_calls)
        .unwrap_or(0);
    let agent_cache_hit_rate = if totals.agent_context_calls == 0 {
        0.0
    } else {
        totals.agent_repo_map_cache_hits as f64 * 100.0 / totals.agent_context_calls as f64
    };
    let agent_saved_tokens = estimated_tokens(totals.agent_context_bytes_avoided);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(PANEL))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            format!(" {} ", language.tr("OVERVIEW")),
            Style::default().fg(GRAY).add_modifier(Modifier::BOLD),
        ))
        .title(
            Line::from(Span::styled(
                format!(
                    " 30S  {rate:.1} req/s · RX {} · TX {} ",
                    short_bytes(rx_30s),
                    short_bytes(tx_30s)
                ),
                Style::default().fg(DIM),
            ))
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if compact || inner.width < 82 || inner.height < 3 {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    compact_metric(language.tr("RUN"), observed_active, CYAN),
                    Span::raw("   "),
                    compact_metric(language.tr("WAIT"), observed_queued, YELLOW),
                    Span::raw("   "),
                    compact_metric(language.tr("DONE"), totals.completed, GREEN),
                    Span::raw("   "),
                    compact_metric(
                        language.tr("FAIL"),
                        totals.failed,
                        if totals.failed > 0 { RED } else { DIM },
                    ),
                ]),
                Line::from(vec![
                    Span::styled("SUCCESS ", Style::default().fg(DIM)),
                    Span::styled(format!("{success:.1}%"), Style::default().fg(GREEN)),
                    Span::styled("   CTX ~", Style::default().fg(DIM)),
                    Span::styled(short_tokens(context_tokens), Style::default().fg(BLUE)),
                    Span::styled(
                        format!(" · COST {}", short_usd(estimated_context_cost)),
                        Style::default().fg(BLUE),
                    ),
                    Span::styled("   SAVED ~", Style::default().fg(DIM)),
                    Span::styled(short_tokens(saved_tokens), Style::default().fg(PURPLE)),
                    Span::styled(
                        format!(" · SAVE {}", short_usd(estimated_savings)),
                        Style::default().fg(GREEN),
                    ),
                ]),
            ]),
            inner,
        );
        return;
    }

    let cards = split_rects_with_gap(inner, 5, 1);
    render_metric_card(
        frame,
        cards[0],
        language.tr("ACTIVE"),
        observed_active.to_string(),
        if observed_active > totals.active {
            format!("now {} · peak {}", totals.active, snapshot.peak_active)
        } else {
            format!("peak {}", snapshot.peak_active)
        },
        CYAN,
    );
    render_metric_card(
        frame,
        cards[1],
        language.tr("QUEUED"),
        observed_queued.to_string(),
        if observed_queued > totals.queued {
            format!("now {} · recent peak", totals.queued)
        } else {
            "waiting".to_owned()
        },
        YELLOW,
    );
    render_metric_card(
        frame,
        cards[2],
        language.tr("COMPLETED"),
        totals.completed.to_string(),
        format!("{success:.1}% success"),
        GREEN,
    );
    render_metric_card(
        frame,
        cards[3],
        language.tr("FAILED"),
        totals.failed.to_string(),
        if totals.failed == 0 {
            "clean".to_owned()
        } else {
            "inspect".to_owned()
        },
        if totals.failed > 0 { RED } else { DIM },
    );
    render_metric_card(
        frame,
        cards[4],
        language.tr("TOKEN ECONOMY · TOTAL"),
        format!("~{} saved", short_tokens(saved_tokens)),
        format!(
            "AG {} · AVG ~{} · MAP {:.0}% · SAVE ~{}",
            totals.agent_context_calls,
            short_tokens(agent_avg_tokens),
            agent_cache_hit_rate,
            short_tokens(agent_saved_tokens)
        ),
        PURPLE,
    );
}

fn compact_metric(label: &'static str, value: u64, color: Color) -> Span<'static> {
    Span::styled(
        format!("{label} {value}"),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn render_metric_card(
    frame: &mut Frame<'_>,
    area: Rect,
    label: &str,
    value: String,
    detail: String,
    color: Color,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(PANEL_ALT))
        .padding(Padding::horizontal(1));
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                value,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )),
            Line::from(vec![
                Span::styled(label.to_owned(), Style::default().fg(DIM)),
                Span::styled(format!("  {detail}"), Style::default().fg(GRAY)),
            ]),
        ])
        .block(block),
        area,
    );
}

pub(super) fn split_rects_with_gap(area: Rect, count: usize, gap: u16) -> Vec<Rect> {
    if count == 0 {
        return Vec::new();
    }
    let gap_total = gap.saturating_mul(count.saturating_sub(1) as u16);
    let usable = area.width.saturating_sub(gap_total);
    let base = usable / count as u16;
    let remainder = usable % count as u16;
    let mut x = area.x;
    let mut rects = Vec::with_capacity(count);
    for index in 0..count {
        let width = base + u16::from((index as u16) < remainder);
        rects.push(Rect::new(x, area.y, width, area.height));
        x = x.saturating_add(width).saturating_add(gap);
    }
    rects
}

fn render_throughput(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
    language: UiLanguage,
) {
    let totals = totals(snapshot);
    let bins = request_bins(snapshot, 12, Duration::from_secs(3));
    let sparkline = sparkline(&bins);
    let (requests, rx, tx) = window_totals(snapshot, Duration::from_secs(30));
    let avoided_30s = window_context_avoided(snapshot, Duration::from_secs(30));
    let req_rate = requests as f64 / 30.0;
    let context_tokens_30s = estimated_tokens(tx);
    let saved_tokens_30s = estimated_tokens(avoided_30s);
    let context_cost_30s = estimated_cost_usd(tx, config.input_token_price_per_million_usd);
    let savings_30s = estimated_cost_usd(avoided_30s, config.input_token_price_per_million_usd);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(PANEL))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            format!(" {} ", language.tr("THROUGHPUT")),
            Style::default().fg(GRAY).add_modifier(Modifier::BOLD),
        ))
        .title(Line::from(Span::styled(" 30S WINDOW ", Style::default().fg(DIM))).right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(inner);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("REQUESTS  ", Style::default().fg(DIM)),
                Span::styled(sparkline, Style::default().fg(CYAN)),
                Span::styled(format!("  {req_rate:.1}/s"), Style::default().fg(WHITE)),
                Span::styled("   RX ", Style::default().fg(DIM)),
                Span::styled(short_bytes(rx), Style::default().fg(BLUE)),
                Span::styled("  TX ", Style::default().fg(DIM)),
                Span::styled(short_bytes(tx), Style::default().fg(PURPLE)),
            ]),
            Line::from(vec![
                Span::styled("CTX ~", Style::default().fg(DIM)),
                Span::styled(short_tokens(context_tokens_30s), Style::default().fg(BLUE)),
                Span::styled(
                    format!(" · COST {}", short_usd(context_cost_30s)),
                    Style::default().fg(BLUE),
                ),
                Span::styled("   SAVED ~", Style::default().fg(DIM)),
                Span::styled(short_tokens(saved_tokens_30s), Style::default().fg(PURPLE)),
                Span::styled(
                    format!(" · SAVE {}", short_usd(savings_30s)),
                    Style::default().fg(GREEN),
                ),
            ]),
        ]),
        columns[0],
    );

    let bar_width = columns[1].width.saturating_sub(23).clamp(6, 18) as usize;
    let (filled, empty, color) = slot_bar(totals.active, config.max_parallel as u64, bar_width);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("SLOT UTILIZATION", Style::default().fg(DIM))).right_aligned(),
            Line::from(vec![
                Span::styled(
                    filled,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(empty, Style::default().fg(BORDER)),
                Span::styled(
                    format!(
                        "  {} / {} · peak {}",
                        totals.active, config.max_parallel, snapshot.peak_active
                    ),
                    Style::default().fg(WHITE),
                ),
            ])
            .right_aligned(),
        ]),
        columns[1],
    );
}

pub(super) fn sparkline(values: &[u64]) -> String {
    const LEVELS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let maximum = values.iter().copied().max().unwrap_or(0);
    values
        .iter()
        .map(|value| {
            if maximum == 0 {
                LEVELS[0]
            } else {
                let index = (*value as usize * (LEVELS.len() - 1)) / maximum as usize;
                LEVELS[index]
            }
        })
        .collect()
}

pub(super) fn spinner_frame(tick: usize) -> &'static str {
    SPINNER_FRAMES[tick % SPINNER_FRAMES.len()]
}

pub(super) fn short_duration(duration: Duration) -> String {
    if duration.as_secs() >= 60 {
        format!(
            "{}m{:02}s",
            duration.as_secs() / 60,
            duration.as_secs() % 60
        )
    } else if duration.as_secs() >= 1 {
        format!("{:.1}s", duration.as_secs_f64())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

pub(super) fn short_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1}G", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1}M", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1}K", bytes as f64 / 1024.0)
    } else {
        format!("{bytes}B")
    }
}

pub(super) fn estimated_tokens(bytes: u64) -> u64 {
    (bytes as f64 / ESTIMATED_BYTES_PER_TOKEN).ceil() as u64
}

pub(super) fn short_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000_000 {
        format!("{:.1}B", tokens as f64 / 1_000_000_000.0)
    } else if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

pub(super) fn estimated_cost_usd(context_bytes: u64, price_per_million: f64) -> f64 {
    estimated_tokens(context_bytes) as f64 * price_per_million.max(0.0) / 1_000_000.0
}

pub(super) fn short_usd(value: f64) -> String {
    if !value.is_finite() || value <= 0.0 {
        "$0".to_owned()
    } else if value >= 1_000.0 {
        format!("${:.1}K", value / 1_000.0)
    } else if value >= 1.0 {
        format!("${value:.2}")
    } else if value >= 0.01 {
        format!("${value:.3}")
    } else if value >= 0.000001 {
        format!("${value:.6}")
    } else {
        "<$0.000001".to_owned()
    }
}

pub(super) fn truncate_end(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        value.to_owned()
    } else if max_chars <= 1 {
        "…".to_owned()
    } else {
        let mut output = value.chars().take(max_chars - 1).collect::<String>();
        output.push('…');
        output
    }
}

pub(super) fn truncate_middle(value: &str, max_chars: usize) -> String {
    let len = value.chars().count();
    if len <= max_chars {
        return value.to_owned();
    }
    if max_chars <= 3 {
        return "…".to_owned();
    }
    let left = (max_chars - 1) / 2;
    let right = max_chars - left - 1;
    let start = value.chars().take(left).collect::<String>();
    let end = value
        .chars()
        .rev()
        .take(right)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("{start}…{end}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn monitor_test_workspaces(names: &[&str]) -> (tempfile::TempDir, Workspaces) {
        let root = tempfile::tempdir().unwrap();
        let paths = names
            .iter()
            .map(|name| {
                let path = root.path().join(name);
                std::fs::create_dir(&path).unwrap();
                path
            })
            .collect::<Vec<_>>();
        let workspaces = Workspaces::new(&paths, true, true).unwrap();
        (root, workspaces)
    }

    #[test]
    fn tracks_task_lifecycle_per_workspace_and_bytes() {
        let monitor = TaskMonitor::new(["api".to_owned(), "web".to_owned()]);
        let mut ticket = monitor.queue("web", "read_file", "src/main.rs · lines 1-80", 128);
        let snapshot = monitor.snapshot();
        assert_eq!(snapshot.workspaces["web"].queued, 1);
        assert_eq!(snapshot.workspaces["web"].calls, 1);
        assert_eq!(snapshot.workspaces["web"].request_bytes, 128);
        assert_eq!(snapshot.workspaces["api"].calls, 0);

        ticket.start();
        assert_eq!(monitor.snapshot().workspaces["web"].active, 1);
        ticket.finish_with_context_savings(true, 512, 4_096);

        let snapshot = monitor.snapshot();
        assert_eq!(snapshot.workspaces["web"].active, 0);
        assert_eq!(snapshot.workspaces["web"].completed, 1);
        assert_eq!(snapshot.workspaces["web"].response_bytes, 512);
        assert_eq!(snapshot.workspaces["web"].context_bytes_avoided, 4_096);
        assert_eq!(snapshot.workspaces["api"].completed, 0);
        monitor.record_agent_context_metrics("web", 800, 3_200, true);
        let snapshot = monitor.snapshot();
        assert_eq!(snapshot.workspaces["web"].agent_context_calls, 1);
        assert_eq!(snapshot.workspaces["web"].agent_context_model_bytes, 800);
        assert_eq!(
            snapshot.workspaces["web"].agent_context_bytes_avoided,
            3_200
        );
        assert_eq!(snapshot.workspaces["web"].agent_repo_map_cache_hits, 1);
        assert_eq!(totals(&snapshot).calls, 1);
        assert_eq!(estimated_tokens(totals(&snapshot).response_bytes), 128);
        assert_eq!(
            estimated_tokens(totals(&snapshot).context_bytes_avoided),
            1_024
        );
        assert!((estimated_cost_usd(512, 5.0) - 0.00064).abs() < f64::EPSILON);
        assert!((estimated_cost_usd(4_096, 5.0) - 0.00512).abs() < f64::EPSILON);
    }

    #[test]
    fn scope_status_updates_operator_intelligence_state() {
        let monitor = TaskMonitor::new(["api".to_owned()]);
        monitor.record_intelligence_result(
            "api",
            "scope_status",
            &serde_json::json!({"source_files": 12, "mapped_files": 10, "unmapped_files": ["src/a.rs", "src/b.rs"]}),
        );
        let snapshot = monitor.snapshot();
        let stats = &snapshot.intelligence["api"];
        assert_eq!(stats.scope_source_files, 12);
        assert_eq!(stats.scope_mapped_files, 10);
        assert_eq!(stats.scope_unmapped_files, 2);
        assert!(stats.updated_at.is_some());
    }

    #[test]
    fn orchestration_tasks_are_visible_without_consuming_execution_slots() {
        let monitor = TaskMonitor::new(["api".to_owned()]);
        let mut ticket =
            monitor.queue_orchestration("api", "verification_plan", "orchestrate child checks", 64);

        ticket.start();
        let running = monitor.snapshot();
        assert_eq!(running.tasks.len(), 1);
        assert_eq!(running.tasks[0].status, TaskStatus::Running);
        assert_eq!(running.workspaces["api"].active, 0);
        assert_eq!(running.peak_active, 0);

        ticket.finish(true, 128);
        let completed = monitor.snapshot();
        assert_eq!(completed.tasks[0].status, TaskStatus::Completed);
        assert_eq!(completed.workspaces["api"].completed, 1);
        assert_eq!(completed.workspaces["api"].active, 0);
    }

    #[test]
    fn tracks_current_slots_and_peak_parallelism() {
        let monitor = TaskMonitor::new(["api".to_owned(), "web".to_owned()]);
        let mut first = monitor.queue("api", "read_file", "one", 1);
        let mut second = monitor.queue("web", "read_file", "two", 1);

        first.start();
        second.start();
        let running = monitor.connection_status();
        assert_eq!(running.active_tasks, 2);
        assert_eq!(running.queued_tasks, 0);
        assert_eq!(running.peak_active_tasks, 2);
        assert_eq!(monitor.snapshot().peak_active, 2);

        first.finish(true, 1);
        let draining = monitor.connection_status();
        assert_eq!(draining.active_tasks, 1);
        assert_eq!(draining.peak_active_tasks, 2);

        second.finish(true, 1);
        let idle = monitor.connection_status();
        assert_eq!(idle.active_tasks, 0);
        assert_eq!(idle.peak_active_tasks, 2);
    }

    #[test]
    fn snapshots_preserve_short_lived_activity_between_draws() {
        let monitor = TaskMonitor::new(["api".to_owned()]);
        let _ = monitor.snapshot();
        let mut ticket = monitor.queue("api", "read_file", "short task", 1);
        std::thread::sleep(Duration::from_millis(6));
        ticket.start();
        ticket.finish(true, 1);

        let observed = monitor.snapshot();
        assert_eq!(totals(&observed).active, 0);
        assert_eq!(totals(&observed).queued, 0);
        assert_eq!(observed.observed_active, 1);
        assert_eq!(observed.observed_queued, 1);

        let next = monitor.snapshot();
        assert_eq!(next.observed_active, 0);
        assert_eq!(next.observed_queued, 0);
    }

    #[test]
    fn dropped_ticket_is_failed() {
        let monitor = TaskMonitor::new(["api".to_owned()]);
        let mut ticket = monitor.queue("api", "search_code", ". · query 8 chars", 64);
        ticket.start();
        drop(ticket);
        let snapshot = monitor.snapshot();
        assert_eq!(snapshot.workspaces["api"].active, 0);
        assert_eq!(snapshot.workspaces["api"].failed, 1);
    }

    #[test]
    fn queued_ticket_can_fail_before_start() {
        let monitor = TaskMonitor::new(["api".to_owned()]);
        let ticket = monitor.queue("api", "run_command", "cargo test · cwd .", 16);
        ticket.finish(false, 32);
        let snapshot = monitor.snapshot();
        assert_eq!(snapshot.workspaces["api"].queued, 0);
        assert_eq!(snapshot.workspaces["api"].failed, 1);
        assert_eq!(snapshot.workspaces["api"].response_bytes, 32);
    }

    #[test]
    fn spinner_advances_and_wraps() {
        assert_ne!(spinner_frame(0), spinner_frame(1));
        assert_eq!(spinner_frame(0), spinner_frame(SPINNER_FRAMES.len()));
    }

    #[test]
    fn formatters_are_compact() {
        assert_eq!(short_duration(Duration::from_millis(382)), "382ms");
        assert_eq!(short_duration(Duration::from_millis(1200)), "1.2s");
        assert_eq!(short_bytes(8_806), "8.6K");
        assert_eq!(short_usd(0.0), "$0");
        assert_eq!(short_usd(0.000005), "$0.000005");
        assert_eq!(short_usd(0.00512), "$0.005120");
        assert_eq!(short_usd(2.1), "$2.10");
    }

    #[test]
    fn dashboard_refreshes_less_often_while_idle() {
        let monitor = TaskMonitor::new(["api".to_owned()]);
        assert_eq!(
            dashboard_refresh_interval(&monitor.snapshot()),
            IDLE_REFRESH_INTERVAL
        );
        let ticket = monitor.queue("api", "read_file", "src/lib.rs", 32);
        assert_eq!(
            dashboard_refresh_interval(&monitor.snapshot()),
            ACTIVE_REFRESH_INTERVAL
        );
        drop(ticket);
        assert_eq!(
            dashboard_refresh_interval(&monitor.snapshot()),
            IDLE_REFRESH_INTERVAL
        );
    }

    #[test]
    fn traffic_history_is_bounded_under_bursts() {
        let monitor = TaskMonitor::new(["api".to_owned()]);
        let now = Instant::now();
        let mut state = monitor.state.lock().unwrap();
        for _ in 0..MAX_TRAFFIC_EVENTS + 17 {
            state.traffic.push_back(TrafficEvent {
                at: now,
                requests: 1,
                request_bytes: 1,
                response_bytes: 1,
                context_bytes_avoided: 1,
            });
        }
        trim_history(&mut state, now);
        assert_eq!(state.traffic.len(), MAX_TRAFFIC_EVENTS);
    }

    #[test]
    fn connection_tracks_public_endpoint_and_tunnel_failure() {
        let monitor = TaskMonitor::new(["backend".to_owned()]);
        monitor.mark_public_endpoint("quick-tunnel", Some(true));
        let ready = monitor.connection_status();
        assert_eq!(ready.public_endpoint.as_deref(), Some("quick-tunnel"));
        assert_eq!(ready.tunnel_running, Some(true));
        assert!(ready.tunnel_error.is_none());

        monitor.mark_tunnel_stopped("cloudflared exited");
        let stopped = monitor.connection_status();
        assert_eq!(stopped.tunnel_running, Some(false));
        assert_eq!(stopped.tunnel_error.as_deref(), Some("cloudflared exited"));
    }

    #[test]
    fn mouse_hit_testing_opens_footer_and_help_links() {
        let (_workspace_root, workspaces) = monitor_test_workspaces(&["backend"]);
        let config = MonitorConfig {
            version: "0.1.0".to_owned(),
            instance_id: "instance-one".to_owned(),
            local_health_url: "http://127.0.0.1:8765/healthz".to_owned(),
            mcp_url: "https://example.trycloudflare.com/mcp".to_owned(),
            setup_url: "https://chatgpt.com/plugins#settings/Connectors".to_owned(),
            intelligence_url: "http://127.0.0.1:8765/intelligence#token=test".to_owned(),
            project_url: "https://github.com/francis-du/wcode".to_owned(),
            author_url: "https://github.com/francis-du".to_owned(),
            author_handle: "@francis-du".to_owned(),
            pairing_code: "123456".to_owned(),
            max_parallel: 16,
            input_token_price_per_million_usd: 5.0,
            workspaces,
        };
        let footer_click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: 31,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            dashboard_link_at(&footer_click, 140, 32, &DashboardState::default(), &config,),
            Some(config.project_url.as_str())
        );
        let setup_hit = (0..140).any(|column| {
            let click = MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row: 31,
                modifiers: KeyModifiers::NONE,
            };
            dashboard_link_at(&click, 140, 32, &DashboardState::default(), &config)
                == Some(config.setup_url.as_str())
        });
        assert!(
            setup_hit,
            "wide footer must expose a clickable setup region"
        );

        let help_click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 6,
            row: 8,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            dashboard_link_at(
                &help_click,
                70,
                14,
                &DashboardState {
                    help_open: true,
                    ..DashboardState::default()
                },
                &config,
            ),
            Some(config.project_url.as_str())
        );
        let health_click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 6,
            row: 11,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            dashboard_link_at(
                &health_click,
                70,
                14,
                &DashboardState {
                    help_open: true,
                    ..DashboardState::default()
                },
                &config,
            ),
            Some(config.local_health_url.as_str())
        );
    }

    #[test]
    fn narrow_and_tiny_layouts_do_not_panic() {
        let monitor = TaskMonitor::new(["backend".to_owned(), "frontend".to_owned()]);
        let (_workspace_root, workspaces) = monitor_test_workspaces(&["backend", "frontend"]);
        let config = MonitorConfig {
            version: "0.1.0".to_owned(),
            instance_id: "instance-one".to_owned(),
            local_health_url: "http://127.0.0.1:8765/healthz".to_owned(),
            mcp_url: "https://example.trycloudflare.com/mcp".to_owned(),
            setup_url: "https://chatgpt.com/plugins#settings/Connectors".to_owned(),
            intelligence_url: "http://127.0.0.1:8765/intelligence#token=test".to_owned(),
            project_url: "https://github.com/francis-du/wcode".to_owned(),
            author_url: "https://github.com/francis-du".to_owned(),
            author_handle: "@francis-du".to_owned(),
            pairing_code: "123456".to_owned(),
            max_parallel: 16,
            input_token_price_per_million_usd: 5.0,
            workspaces,
        };

        for (width, height) in [(20, 5), (40, 10), (60, 18), (100, 32)] {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            let snapshot = monitor.snapshot();
            let ui = DashboardState::default();
            terminal
                .draw(|frame| draw_dashboard(frame, &snapshot, &config, 0, &ui))
                .expect("layout renders");
        }
    }

    #[test]
    fn help_and_footer_render_project_and_author_links() {
        let monitor = TaskMonitor::new(["backend".to_owned()]);
        let (_workspace_root, workspaces) = monitor_test_workspaces(&["backend"]);
        monitor.mark_mcp_initialized();
        let mut saved = monitor.queue("backend", "symbol_context", "saved context", 1);
        saved.start();
        saved.finish_with_context_savings(true, 400, 4_000);
        let mut first = monitor.queue("backend", "read_file", "one", 1);
        let mut second = monitor.queue("backend", "search_code", "two", 1);
        first.start();
        second.start();
        let config = MonitorConfig {
            version: "0.1.0".to_owned(),
            instance_id: "instance-one".to_owned(),
            local_health_url: "http://127.0.0.1:8765/healthz".to_owned(),
            mcp_url: "https://example.trycloudflare.com/mcp".to_owned(),
            setup_url: "https://chatgpt.com/plugins#settings/Connectors".to_owned(),
            intelligence_url: "https://example.trycloudflare.com/intelligence#token=fixture"
                .to_owned(),
            project_url: "https://github.com/francis-du/wcode".to_owned(),
            author_url: "https://github.com/francis-du".to_owned(),
            author_handle: "@francis-du".to_owned(),
            pairing_code: "123456".to_owned(),
            max_parallel: 8,
            input_token_price_per_million_usd: 5.0,
            workspaces,
        };

        let backend = TestBackend::new(140, 32);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                draw_dashboard(
                    frame,
                    &monitor.snapshot(),
                    &config,
                    0,
                    &DashboardState::default(),
                )
            })
            .expect("wide footer renders");
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("github.com/francis-du/wcode"));
        assert!(text.contains("@francis-du"));
        assert!(text.contains("SLOTS 2 / 8"));
        assert!(text.contains("PEAK 2"));
        assert!(text.contains("VERIFY CODE 123456"));
        assert!(text.contains("INSTANCE"));
        assert!(text.contains("127.0.0.1:8765/healthz"));
        assert!(text.contains("OVERVIEW"));
        assert!(text.contains("WORKSPACE ACTIVITY"));
        assert!(text.contains("THROUGHPUT"));
        assert!(text.contains("SLOT UTILIZATION"));
        assert!(text.contains("TOKEN ECONOMY · TOTAL"));
        assert!(text.contains("~1.0K saved"));
        assert!(text.contains('╭'));

        let backend = TestBackend::new(70, 18);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let ui = DashboardState {
            help_open: true,
            ..DashboardState::default()
        };
        terminal
            .draw(|frame| draw_dashboard(frame, &monitor.snapshot(), &config, 0, &ui))
            .expect("compact help renders");
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("Project:"));
        assert!(text.contains("Author:"));
        assert!(text.contains("Setup:"));
        assert!(text.contains("Health:"));
        assert!(text.contains("127.0.0.1:8765/healthz"));
    }

    #[test]
    fn authorization_overlay_shows_selectable_requests_and_actions() {
        let requests = vec![
            AuthorizationRequest {
                id: "AUTH-00000002".to_owned(),
                workspace: "backend".to_owned(),
                kind: crate::authorization::AuthorizationKind::CommandAccess,
                summary: "authorize command: git".to_owned(),
                program: Some("git".to_owned()),
                fingerprint: "sha256:git".to_owned(),
                status: AuthorizationStatus::Pending,
                created_at_ms: 2,
                decided_at_ms: None,
            },
            AuthorizationRequest {
                id: "AUTH-00000001".to_owned(),
                workspace: "backend".to_owned(),
                kind: crate::authorization::AuthorizationKind::DestructiveDelete,
                summary: "delete file: src/obsolete.rs".to_owned(),
                program: None,
                fingerprint: "sha256:delete".to_owned(),
                status: AuthorizationStatus::Pending,
                created_at_ms: 1,
                decided_at_ms: None,
            },
        ];
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render_authorization_overlay(frame, frame.area(), &requests, 1, UiLanguage::En)
            })
            .expect("authorization overlay renders");
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("AUTHORIZATION REQUIRED"));
        assert!(text.contains("AUTH-00000002"));
        assert!(text.contains("AUTH-00000001"));
        assert!(text.contains("Y"));
        assert!(text.contains("approve selected"));
        assert!(text.contains("N"));
        assert!(text.contains("deny selected"));
    }

    #[test]
    fn intelligence_url_keeps_ui_token_and_targets_focused_workspace() {
        let url = intelligence_url_for_workspace(
            "http://127.0.0.1:8765/intelligence#token=secret",
            "frontend app",
        );
        assert_eq!(
            url,
            "http://127.0.0.1:8765/intelligence#token=secret&workspace=frontend+app"
        );
    }

    #[test]
    fn workspace_columns_scale_from_one_to_four() {
        assert_eq!(workspace_column_count(45, 6), 1);
        assert_eq!(workspace_column_count(70, 6), 2);
        assert_eq!(workspace_column_count(130, 6), 4);
        assert_eq!(workspace_column_count(200, 1), 1);
    }

    #[test]
    fn workspace_viewport_follows_focus_and_pages() {
        let mut ui = DashboardState {
            workspace_focus: 4,
            ..DashboardState::default()
        };
        ui.clamp(7, 3);
        assert_eq!(ui.workspace_offset, 2);
        ui.workspace_focus = 6;
        ui.clamp(7, 3);
        assert_eq!(ui.workspace_offset, 4);
        ui.workspace_focus = 1;
        ui.clamp(7, 3);
        assert_eq!(ui.workspace_offset, 1);
    }

    #[test]
    fn connection_stages_and_setup_collapse_render() {
        let monitor = TaskMonitor::new(["backend".to_owned()]);
        let (_workspace_root, workspaces) = monitor_test_workspaces(&["backend"]);
        let config = MonitorConfig {
            version: "0.1.0".to_owned(),
            instance_id: "instance-one".to_owned(),
            local_health_url: "http://127.0.0.1:8765/healthz".to_owned(),
            mcp_url: "https://example.trycloudflare.com/mcp".to_owned(),
            setup_url: "https://chatgpt.com/plugins#settings/Connectors".to_owned(),
            intelligence_url: "https://example.trycloudflare.com/intelligence#token=fixture"
                .to_owned(),
            project_url: "https://github.com/francis-du/wcode".to_owned(),
            author_url: "https://github.com/francis-du".to_owned(),
            author_handle: "@francis-du".to_owned(),
            pairing_code: "123456".to_owned(),
            max_parallel: 8,
            input_token_price_per_million_usd: 5.0,
            workspaces,
        };
        let backend = TestBackend::new(100, 32);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let ui = DashboardState::default();
        terminal
            .draw(|frame| draw_dashboard(frame, &monitor.snapshot(), &config, 0, &ui))
            .expect("expanded setup renders");

        monitor.mark_oauth_client_registered();
        monitor.mark_oauth_authorized();
        monitor.mark_mcp_seen();
        monitor.mark_mcp_initialized();
        monitor.mark_mcp_initialized();
        let status = monitor.connection_status();
        assert!(status.oauth_client_registered);
        assert!(status.oauth_authorized);
        assert!(status.chatgpt_initialized);
        assert_eq!(status.initialize_count, 2);
        assert!(status.last_initialize_seconds_ago.is_some());
        assert!(status.last_mcp_seen_seconds_ago.is_some());
        terminal
            .draw(|frame| draw_dashboard(frame, &monitor.snapshot(), &config, 1, &ui))
            .expect("collapsed setup renders");
    }

    #[test]
    fn public_url_health_requires_three_failures_and_recovers_on_success() {
        let monitor = TaskMonitor::new(["backend".to_owned()]);
        monitor.mark_public_url_check(false, Some("first".to_owned()));
        let status = monitor.connection_status();
        assert_eq!(status.public_url_healthy, None);
        assert_eq!(status.public_url_consecutive_failures, 1);

        monitor.mark_public_url_check(false, Some("second".to_owned()));
        assert_eq!(
            monitor.connection_status().public_url_consecutive_failures,
            2
        );
        assert_eq!(monitor.connection_status().public_url_healthy, None);

        monitor.mark_public_url_check(false, Some("third".to_owned()));
        let status = monitor.connection_status();
        assert_eq!(status.public_url_healthy, Some(false));
        assert_eq!(status.public_url_consecutive_failures, 3);
        assert_eq!(status.public_url_error.as_deref(), Some("third"));
        assert!(status.public_url_last_checked_seconds_ago.is_some());

        monitor.mark_public_url_check(true, None);
        let status = monitor.connection_status();
        assert_eq!(status.public_url_healthy, Some(true));
        assert_eq!(status.public_url_consecutive_failures, 0);
        assert!(status.public_url_error.is_none());
    }
}
