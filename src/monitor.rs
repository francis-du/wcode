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
}

#[derive(Clone)]
struct TaskRecord {
    id: u64,
    workspace: String,
    tool: String,
    detail: String,
    status: TaskStatus,
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

#[derive(Clone, Copy, PartialEq, Eq)]
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
    pub local_url: String,
    pub mcp_url: String,
    pub setup_url: String,
    pub project_url: String,
    pub author_url: String,
    pub author_handle: String,
    pub pairing_code: String,
    pub max_parallel: usize,
    pub input_token_price_per_million_usd: f64,
    pub workspaces: Vec<(String, String, bool)>,
}

pub struct MonitorRenderer {
    stop: watch::Sender<bool>,
    interrupted: watch::Receiver<bool>,
    join: tokio::task::JoinHandle<()>,
}

impl TaskMonitor {
    pub fn new(workspaces: impl IntoIterator<Item = String>) -> Self {
        let workspaces = workspaces
            .into_iter()
            .map(|id| (id, WorkspaceStats::default()))
            .collect();
        Self {
            state: Arc::new(Mutex::new(MonitorState {
                next_id: 1,
                started_at: Instant::now(),
                workspaces,
                tasks: VecDeque::new(),
                traffic: VecDeque::new(),
                oauth_client_registered: false,
                oauth_authorized: false,
                chatgpt_connected: false,
                initialize_count: 0,
                last_initialize: None,
                last_mcp_seen: None,
                public_endpoint: None,
                public_url_healthy: None,
                public_url_last_checked: None,
                public_url_consecutive_failures: 0,
                public_url_error: None,
                tunnel_running: None,
                tunnel_error: None,
                active_total: 0,
                peak_active: 0,
                observed_active: 0,
                observed_queued: 0,
            })),
        }
    }

    pub fn queue(
        &self,
        workspace: impl Into<String>,
        tool: impl Into<String>,
        detail: impl Into<String>,
        request_bytes: u64,
    ) -> TaskTicket {
        let workspace = workspace.into();
        let tool = tool.into();
        let detail = detail.into();
        let now = Instant::now();
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        let id = state.next_id;
        state.next_id = state.next_id.saturating_add(1);
        let stats = state.workspaces.entry(workspace.clone()).or_default();
        stats.queued = stats.queued.saturating_add(1);
        stats.calls = stats.calls.saturating_add(1);
        stats.request_bytes = stats.request_bytes.saturating_add(request_bytes);
        state.traffic.push_back(TrafficEvent {
            at: now,
            requests: 1,
            request_bytes,
            response_bytes: 0,
            context_bytes_avoided: 0,
        });
        state.tasks.push_back(TaskRecord {
            id,
            workspace,
            tool,
            detail,
            status: TaskStatus::Queued,
            queued_at: now,
            started_at: None,
            finished_at: None,
            request_bytes,
            response_bytes: 0,
            context_bytes_avoided: 0,
        });
        trim_history(&mut state, now);
        TaskTicket {
            monitor: self.clone(),
            id,
            finished: false,
        }
    }

    pub fn spawn_renderer(&self, config: MonitorConfig, enabled: bool) -> Option<MonitorRenderer> {
        if !enabled || !io::stdout().is_terminal() {
            return None;
        }

        let monitor = self.clone();
        let (stop, stop_rx) = watch::channel(false);
        let (interrupt_tx, interrupted) = watch::channel(false);
        let join = tokio::task::spawn_blocking(move || {
            if let Err(error) = run_dashboard(monitor, config, stop_rx, interrupt_tx) {
                eprintln!("wcode dashboard stopped: {error}");
            }
        });
        Some(MonitorRenderer {
            stop,
            interrupted,
            join,
        })
    }

    pub fn mark_oauth_client_registered(&self) {
        self.state
            .lock()
            .expect("task monitor lock poisoned")
            .oauth_client_registered = true;
    }

    pub fn mark_oauth_authorized(&self) {
        self.state
            .lock()
            .expect("task monitor lock poisoned")
            .oauth_authorized = true;
    }

    pub fn mark_mcp_seen(&self) {
        self.state
            .lock()
            .expect("task monitor lock poisoned")
            .last_mcp_seen = Some(Instant::now());
    }

    pub fn mark_chatgpt_connected(&self) {
        let now = Instant::now();
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        state.chatgpt_connected = true;
        state.initialize_count = state.initialize_count.saturating_add(1);
        state.last_initialize = Some(now);
        state.last_mcp_seen = Some(now);
    }

    pub fn mark_public_url_check(&self, success: bool, error: Option<String>) {
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        state.public_url_last_checked = Some(Instant::now());
        if success {
            state.public_url_healthy = Some(true);
            state.public_url_consecutive_failures = 0;
            state.public_url_error = None;
        } else {
            state.public_url_consecutive_failures =
                state.public_url_consecutive_failures.saturating_add(1);
            state.public_url_error = error;
            if state.public_url_consecutive_failures >= 3 {
                state.public_url_healthy = Some(false);
            }
        }
    }

    pub fn mark_public_endpoint(&self, mode: impl Into<String>, tunnel_running: Option<bool>) {
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        state.public_endpoint = Some(mode.into());
        state.tunnel_running = tunnel_running;
        state.tunnel_error = None;
    }

    pub fn mark_tunnel_stopped(&self, error: impl Into<String>) {
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        state.tunnel_running = Some(false);
        state.tunnel_error = Some(error.into());
    }

    pub fn connection_status(&self) -> MonitorConnectionStatus {
        let state = self.state.lock().expect("task monitor lock poisoned");
        MonitorConnectionStatus {
            oauth_client_registered: state.oauth_client_registered,
            oauth_authorized: state.oauth_authorized,
            chatgpt_initialized: state.chatgpt_connected,
            initialize_count: state.initialize_count,
            last_initialize_seconds_ago: state.last_initialize.map(|seen| seen.elapsed().as_secs()),
            last_mcp_seen_seconds_ago: state.last_mcp_seen.map(|seen| seen.elapsed().as_secs()),
            public_endpoint: state.public_endpoint.clone(),
            public_url_healthy: state.public_url_healthy,
            public_url_last_checked_seconds_ago: state
                .public_url_last_checked
                .map(|seen| seen.elapsed().as_secs()),
            public_url_consecutive_failures: state.public_url_consecutive_failures,
            public_url_error: state.public_url_error.clone(),
            tunnel_running: state.tunnel_running,
            tunnel_error: state.tunnel_error.clone(),
            active_tasks: state.active_total,
            queued_tasks: state.workspaces.values().map(|stats| stats.queued).sum(),
            peak_active_tasks: state.peak_active,
        }
    }

    fn start(&self, id: u64) {
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        let Some(index) = state.tasks.iter().position(|task| task.id == id) else {
            return;
        };
        if state.tasks[index].status != TaskStatus::Queued {
            return;
        }
        let queued_long_enough = state.tasks[index].queued_at.elapsed() >= Duration::from_millis(5);
        let queued_total = state.workspaces.values().map(|stats| stats.queued).sum();
        let workspace = state.tasks[index].workspace.clone();
        if let Some(stats) = state.workspaces.get_mut(&workspace) {
            stats.queued = stats.queued.saturating_sub(1);
            stats.active = stats.active.saturating_add(1);
        }
        if queued_long_enough {
            state.observed_queued = state.observed_queued.max(queued_total);
        }
        state.active_total = state.active_total.saturating_add(1);
        state.peak_active = state.peak_active.max(state.active_total);
        state.observed_active = state.observed_active.max(state.active_total);
        let task = &mut state.tasks[index];
        task.status = TaskStatus::Running;
        task.started_at = Some(Instant::now());
    }

    fn finish(&self, id: u64, success: bool, response_bytes: u64, context_bytes_avoided: u64) {
        let now = Instant::now();
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        let Some(index) = state.tasks.iter().position(|task| task.id == id) else {
            return;
        };
        if matches!(
            state.tasks[index].status,
            TaskStatus::Completed | TaskStatus::Failed
        ) {
            return;
        }

        let workspace = state.tasks[index].workspace.clone();
        let was_running = state.tasks[index].status == TaskStatus::Running;
        let was_queued = state.tasks[index].status == TaskStatus::Queued;
        if let Some(stats) = state.workspaces.get_mut(&workspace) {
            if was_running {
                stats.active = stats.active.saturating_sub(1);
            }
            if was_queued {
                stats.queued = stats.queued.saturating_sub(1);
            }
            stats.response_bytes = stats.response_bytes.saturating_add(response_bytes);
            stats.context_bytes_avoided = stats
                .context_bytes_avoided
                .saturating_add(context_bytes_avoided);
            if success {
                stats.completed = stats.completed.saturating_add(1);
            } else {
                stats.failed = stats.failed.saturating_add(1);
            }
        }
        if was_running {
            state.active_total = state.active_total.saturating_sub(1);
        }
        state.traffic.push_back(TrafficEvent {
            at: now,
            requests: 0,
            request_bytes: 0,
            response_bytes,
            context_bytes_avoided,
        });
        let task = &mut state.tasks[index];
        task.status = if success {
            TaskStatus::Completed
        } else {
            TaskStatus::Failed
        };
        task.finished_at = Some(now);
        task.response_bytes = response_bytes;
        task.context_bytes_avoided = context_bytes_avoided;
        trim_history(&mut state, now);
    }

    fn snapshot(&self) -> MonitorSnapshot {
        let now = Instant::now();
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        trim_history(&mut state, now);
        let queued_now = state.workspaces.values().map(|stats| stats.queued).sum();
        let observed_active = state.observed_active.max(state.active_total);
        let observed_queued = state.observed_queued.max(queued_now);
        let snapshot = MonitorSnapshot {
            started_at: state.started_at,
            workspaces: state.workspaces.clone(),
            tasks: state.tasks.iter().cloned().collect(),
            traffic: state.traffic.iter().cloned().collect(),
            oauth_client_registered: state.oauth_client_registered,
            oauth_authorized: state.oauth_authorized,
            chatgpt_connected: state.chatgpt_connected,
            initialize_count: state.initialize_count,
            last_initialize: state.last_initialize,
            last_mcp_seen: state.last_mcp_seen,
            public_endpoint: state.public_endpoint.clone(),
            public_url_healthy: state.public_url_healthy,
            public_url_last_checked: state.public_url_last_checked,
            public_url_consecutive_failures: state.public_url_consecutive_failures,
            public_url_error: state.public_url_error.clone(),
            tunnel_running: state.tunnel_running,
            tunnel_error: state.tunnel_error.clone(),
            peak_active: state.peak_active,
            observed_active,
            observed_queued,
        };
        state.observed_active = state.active_total;
        state.observed_queued = queued_now;
        snapshot
    }
}

fn trim_history(state: &mut MonitorState, now: Instant) {
    while state
        .traffic
        .front()
        .is_some_and(|event| now.saturating_duration_since(event.at) > TRAFFIC_WINDOW)
    {
        state.traffic.pop_front();
    }
    while state.traffic.len() > MAX_TRAFFIC_EVENTS {
        state.traffic.pop_front();
    }
    while state.tasks.len() > MAX_RECENT_TASKS {
        let removable = state
            .tasks
            .iter()
            .position(|task| matches!(task.status, TaskStatus::Completed | TaskStatus::Failed));
        if let Some(index) = removable {
            state.tasks.remove(index);
        } else {
            break;
        }
    }
}

impl TaskTicket {
    pub fn start(&mut self) {
        self.monitor.start(self.id);
    }

    pub fn finish(mut self, success: bool, response_bytes: u64) {
        self.monitor.finish(self.id, success, response_bytes, 0);
        self.finished = true;
    }

    pub fn finish_with_context_savings(
        mut self,
        success: bool,
        response_bytes: u64,
        context_bytes_avoided: u64,
    ) {
        self.monitor
            .finish(self.id, success, response_bytes, context_bytes_avoided);
        self.finished = true;
    }
}

impl Drop for TaskTicket {
    fn drop(&mut self) {
        if !self.finished {
            self.monitor.finish(self.id, false, 0, 0);
            self.finished = true;
        }
    }
}

impl MonitorRenderer {
    pub fn interrupt_receiver(&self) -> watch::Receiver<bool> {
        self.interrupted.clone()
    }

    pub async fn stop(self) {
        let _ = self.stop.send(true);
        let _ = self.join.await;
    }
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
        let visible = workspace_column_count(size.width, config.workspaces.len());
        ui.clamp(config.workspaces.len(), visible);
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
                    match key.code {
                        KeyCode::Esc => ui.help_open = false,
                        KeyCode::Char('?') if key.kind == KeyEventKind::Press => {
                            ui.help_open = true
                        }
                        KeyCode::Char('o') | KeyCode::Char('O')
                            if key.kind == KeyEventKind::Press =>
                        {
                            let _ = open_external_url(&config.setup_url);
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
                        KeyCode::Left => {
                            let step = if key.modifiers.contains(KeyModifiers::SHIFT) {
                                visible.max(1)
                            } else {
                                1
                            };
                            ui.workspace_focus = ui.workspace_focus.saturating_sub(step);
                            ui.clamp(config.workspaces.len(), visible);
                        }
                        KeyCode::Right => {
                            let step = if key.modifiers.contains(KeyModifiers::SHIFT) {
                                visible.max(1)
                            } else {
                                1
                            };
                            ui.workspace_focus = ui
                                .workspace_focus
                                .saturating_add(step)
                                .min(config.workspaces.len().saturating_sub(1));
                            ui.clamp(config.workspaces.len(), visible);
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
                (9, config.local_url.as_str()),
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
                (10, config.local_url.as_str()),
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

        let shortcuts = " ←/→  workspace   O  setup   G  project   A  author   ?  help   ^C  stop";
        let shortcuts_width = shortcuts.chars().count() as u16;
        let shortcuts_x = columns[1]
            .x
            .saturating_add(columns[1].width.saturating_sub(shortcuts_width));
        let setup_prefix = " ←/→  workspace  ";
        let setup_x = shortcuts_x.saturating_add(setup_prefix.chars().count() as u16);
        let setup_rect = Rect::new(setup_x, footer.y, " O  setup  ".chars().count() as u16, 1);
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
    let header_height = 7;
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
        render_too_small(frame, area);
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
    render_header(frame, rows[row], snapshot, config, tick, compact);
    row += 1;
    if setup_height > 0 {
        render_setup(frame, rows[row], snapshot, config, compact);
        row += 1;
    }
    render_overview(frame, rows[row], snapshot, config, compact);
    row += 1;
    render_workspace_activity(
        frame,
        rows[row],
        snapshot,
        config,
        tick,
        ui.workspace_offset,
        ui.workspace_focus,
    );
    row += 1;
    if throughput_height > 0 {
        render_throughput(frame, rows[row], snapshot, config);
        row += 1;
    }
    render_footer(frame, rows[row], config);

    if ui.help_open {
        render_help_overlay(frame, area, config);
    }
}

fn render_too_small(frame: &mut Frame<'_>, area: Rect) {
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
        ]));
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Terminal needs a little more room",
                Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!("current size  {} × {}", area.width, area.height),
                Style::default().fg(GRAY),
            )),
            Line::from(Span::styled(
                "resize the window to restore the live dashboard",
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
) {
    let totals = totals(snapshot);
    let idle = snapshot
        .last_mcp_seen
        .is_some_and(|seen| seen.elapsed() >= Duration::from_secs(300));
    let (icon, state, detail, color) = if snapshot.tunnel_running == Some(false) {
        (
            "×",
            "CLOUDFLARED PROCESS EXITED",
            snapshot
                .tunnel_error
                .as_deref()
                .map(|error| truncate_end(error, 54))
                .unwrap_or_else(|| "cloudflared is no longer running".to_owned()),
            RED,
        )
    } else if snapshot.public_url_healthy == Some(false) {
        (
            "×",
            "PUBLIC URL UNAVAILABLE",
            format!(
                "{} consecutive health checks failed",
                snapshot.public_url_consecutive_failures
            ),
            RED,
        )
    } else if snapshot.chatgpt_connected && idle {
        (
            "◐",
            "ChatGPT idle",
            format!(
                "last seen {} · Chat mode",
                last_seen_text(snapshot.last_mcp_seen)
            ),
            YELLOW,
        )
    } else if snapshot.chatgpt_connected {
        (
            "●",
            "ChatGPT connected",
            format!(
                "last seen {} · Chat mode",
                last_seen_text(snapshot.last_mcp_seen)
            ),
            GREEN,
        )
    } else if snapshot.oauth_authorized {
        (
            "◐",
            "OAuth authorized",
            "waiting for ChatGPT initialize".to_owned(),
            YELLOW,
        )
    } else {
        (
            "○",
            "Setup required",
            "press O to open Connector setup".to_owned(),
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
        ]));
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

fn tunnel_status_text(snapshot: &MonitorSnapshot) -> &'static str {
    match snapshot.tunnel_running {
        Some(true) => "● RUNNING",
        Some(false) => "× EXITED",
        None => "○ EXTERNAL / LOCAL",
    }
}

fn tunnel_status_color(snapshot: &MonitorSnapshot) -> Color {
    match snapshot.tunnel_running {
        Some(true) => GREEN,
        Some(false) => RED,
        None => GRAY,
    }
}

fn initialize_status_text(snapshot: &MonitorSnapshot) -> String {
    let last = snapshot
        .last_initialize
        .map(|seen| last_seen_text(Some(seen)))
        .unwrap_or_else(|| "never".to_owned());
    format!("#{} · last {last}", snapshot.initialize_count)
}

fn render_setup(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
    compact: bool,
) {
    let lifecycle = if is_quick_tunnel(&config.mcp_url) {
        "TEMPORARY URL"
    } else {
        "FIXED ENDPOINT"
    };
    let endpoint_mode = snapshot.public_endpoint.as_deref().unwrap_or("pending");
    let endpoint_ready = match endpoint_mode {
        "quick-tunnel" => {
            snapshot.tunnel_running == Some(true) && snapshot.public_url_healthy != Some(false)
        }
        "external" => snapshot.public_url_healthy == Some(true),
        "local-only" => true,
        "pending" => false,
        _ => false,
    };
    let endpoint_detail = if let Some(error) = snapshot.tunnel_error.as_deref() {
        format!("stopped · {}", truncate_end(error, 28))
    } else {
        match endpoint_mode {
            "quick-tunnel" | "external" => public_url_health_text(snapshot),
            "local-only" => "local only".to_owned(),
            _ => "waiting".to_owned(),
        }
    };
    let mcp_seen = snapshot.last_mcp_seen.is_some();
    let mcp_detail = if mcp_seen {
        format!("last seen {}", last_seen_text(snapshot.last_mcp_seen))
    } else {
        "waiting for request".to_owned()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(YELLOW))
        .style(Style::default().bg(PANEL))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            " SETUP ",
            Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
        ))
        .title(
            Line::from(Span::styled(
                format!(" {lifecycle} "),
                Style::default().fg(DIM),
            ))
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if compact || inner.width < 78 || inner.height < 7 {
        let lines = vec![
            setup_step(1, "Enable Developer mode in ChatGPT"),
            setup_step(2, "Create a Connector and choose OAuth"),
            Line::from(vec![
                Span::styled("  MCP  ", Style::default().fg(DIM)),
                Span::styled(
                    truncate_middle(&config.mcp_url, inner.width.saturating_sub(8) as usize),
                    Style::default().fg(BLUE),
                ),
            ]),
            Line::from(vec![
                Span::styled("  VERIFY CODE ", Style::default().fg(DIM)),
                Span::styled(
                    &config.pairing_code,
                    Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "   ·   Connector works in Chat mode",
                    Style::default().fg(GRAY),
                ),
            ]),
            setup_state(
                snapshot.oauth_authorized,
                "OAuth",
                if snapshot.oauth_authorized {
                    "authorized"
                } else {
                    "waiting"
                },
            ),
        ];
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(inner);
    let left = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(BORDER))
        .padding(Padding::new(0, 2, 0, 0));
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "GET CONNECTED",
                Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
            )),
            setup_step(1, "Open ChatGPT Settings → Connectors"),
            setup_step(2, "Enable Developer mode"),
            setup_step(3, "Create Connector · Auth: OAuth"),
            Line::from(vec![
                Span::styled("  MCP   ", Style::default().fg(DIM)),
                Span::styled(
                    truncate_middle(
                        &config.mcp_url,
                        columns[0].width.saturating_sub(10) as usize,
                    ),
                    Style::default().fg(BLUE),
                ),
            ]),
            Line::from(vec![
                Span::styled("  VERIFY CODE  ", Style::default().fg(DIM)),
                Span::styled(
                    &config.pairing_code,
                    Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
                ),
                Span::styled("   press O to reopen setup", Style::default().fg(GRAY)),
            ]),
        ])
        .block(left),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "CONNECTION",
                Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
            )),
            setup_state(true, "Local server", "ready"),
            setup_state(endpoint_ready, "Public endpoint", &endpoint_detail),
            setup_state(
                snapshot.oauth_client_registered,
                "OAuth client",
                if snapshot.oauth_client_registered {
                    "registered"
                } else {
                    "waiting"
                },
            ),
            setup_state(
                snapshot.oauth_authorized,
                "OAuth",
                if snapshot.oauth_authorized {
                    "authorized"
                } else {
                    "waiting"
                },
            ),
            setup_state(mcp_seen, "MCP", &mcp_detail),
            setup_state(snapshot.chatgpt_connected, "ChatGPT", "initialize"),
            Line::from(vec![
                Span::styled("  LAST  ", Style::default().fg(DIM)),
                Span::styled(
                    last_seen_text(snapshot.last_mcp_seen),
                    Style::default().fg(GRAY),
                ),
                Span::styled("   ·   Chat mode", Style::default().fg(PURPLE)),
            ]),
        ]),
        columns[1],
    );
}

fn setup_step(number: u8, label: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(" {number} "),
            Style::default()
                .fg(BG)
                .bg(PURPLE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("  {label}"), Style::default().fg(WHITE)),
    ])
}

fn setup_state(done: bool, label: &str, detail: &str) -> Line<'static> {
    let color = if done { GREEN } else { DIM };
    Line::from(vec![
        Span::styled(
            if done { "  ● " } else { "  ○ " },
            Style::default().fg(color),
        ),
        Span::styled(format!("{label:<14}"), Style::default().fg(WHITE)),
        Span::styled(detail.to_owned(), Style::default().fg(color)),
    ])
}

fn render_overview(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
    compact: bool,
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
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(PANEL))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            " OVERVIEW ",
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
                    compact_metric("RUN", observed_active, CYAN),
                    Span::raw("   "),
                    compact_metric("WAIT", observed_queued, YELLOW),
                    Span::raw("   "),
                    compact_metric("DONE", totals.completed, GREEN),
                    Span::raw("   "),
                    compact_metric(
                        "FAIL",
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
        "ACTIVE",
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
        "QUEUED",
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
        "COMPLETED",
        totals.completed.to_string(),
        format!("{success:.1}% success"),
        GREEN,
    );
    render_metric_card(
        frame,
        cards[3],
        "FAILED",
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
        "TOKEN ECONOMY · TOTAL",
        format!("~{} saved", short_tokens(saved_tokens)),
        format!(
            "CTX {} · SAVE {}",
            short_usd(estimated_context_cost),
            short_usd(estimated_savings)
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

fn split_rects_with_gap(area: Rect, count: usize, gap: u16) -> Vec<Rect> {
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

fn render_workspace_activity(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
    tick: usize,
    requested_offset: usize,
    focus: usize,
) {
    let total = config.workspaces.len();
    let visible = workspace_column_count(area.width, total);
    let offset = requested_offset.min(total.saturating_sub(visible));
    let end = (offset + visible).min(total);
    let range = if total == 0 {
        "0 / 0".to_owned()
    } else {
        format!("{}–{} / {}", offset + 1, end, total)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(PANEL))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            " WORKSPACE ACTIVITY ",
            Style::default().fg(GRAY).add_modifier(Modifier::BOLD),
        ))
        .title(
            Line::from(vec![
                Span::styled("VIEW  ", Style::default().fg(DIM)),
                Span::styled(
                    range,
                    Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
                ),
                Span::styled("  ← → ", Style::default().fg(DIM)),
            ])
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if total == 0 || inner.width == 0 || inner.height == 0 {
        if inner.width > 0 && inner.height > 0 {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(Span::styled(
                        "No workspaces configured",
                        Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
                    )),
                    Line::from(Span::styled(
                        "restart wcode with one or more --workspace paths",
                        Style::default().fg(GRAY),
                    )),
                ])
                .alignment(ratatui::layout::Alignment::Center),
                inner,
            );
        }
        return;
    }

    let column_count = end.saturating_sub(offset).max(1);
    let columns = split_rects_with_gap(inner, column_count, 1);
    let now = Instant::now();

    for (column, workspace_index) in columns.iter().zip(offset..end) {
        let (id, path, is_default) = &config.workspaces[workspace_index];
        let stats = snapshot.workspaces.get(id).cloned().unwrap_or_default();
        let active = stats.active > 0;
        let queued = stats.queued > 0;
        let focused = workspace_index == focus;
        let status_color = if active {
            CYAN
        } else if queued {
            YELLOW
        } else if stats.failed > 0 {
            RED
        } else {
            DIM
        };
        let border_color = if focused {
            BLUE
        } else if active {
            CYAN
        } else {
            BORDER
        };
        let summary = if active || queued {
            format!("{} run · {} wait", stats.active, stats.queued)
        } else {
            "idle".to_owned()
        };
        let title_width = column.width.saturating_sub(10) as usize;
        let card = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(if focused { PANEL_ACTIVE } else { PANEL_ALT }))
            .padding(Padding::horizontal(1))
            .title(Line::from(vec![
                Span::styled(
                    if focused { " ▸ " } else { "   " },
                    Style::default().fg(BLUE),
                ),
                Span::styled("● ", Style::default().fg(status_color)),
                Span::styled(
                    truncate_end(id, title_width),
                    Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
                ),
            ]))
            .title_bottom(Line::from(Span::styled(
                format!(" {summary} "),
                Style::default().fg(status_color),
            )))
            .title_bottom(
                Line::from(Span::styled(
                    if *is_default { " DEFAULT " } else { " " },
                    Style::default().fg(if *is_default { PURPLE } else { DIM }),
                ))
                .right_aligned(),
            );
        let card_inner = card.inner(*column);
        frame.render_widget(card, *column);

        let capacity = card_inner.height as usize;
        let tasks = workspace_activity_tasks(snapshot, id, capacity);
        if tasks.is_empty() && capacity > 0 {
            let lines = if card_inner.height >= 2 {
                vec![
                    Line::from(Span::styled("quiet", Style::default().fg(GRAY))),
                    Line::from(Span::styled(
                        truncate_middle(path, card_inner.width as usize),
                        Style::default().fg(DIM),
                    )),
                ]
            } else {
                vec![Line::from(Span::styled("quiet", Style::default().fg(GRAY)))]
            };
            frame.render_widget(
                Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center),
                card_inner,
            );
            continue;
        }

        let items = tasks
            .into_iter()
            .map(|task| activity_item(task, tick, now, card_inner.width as usize))
            .collect::<Vec<_>>();
        frame.render_widget(List::new(items), card_inner);
    }
}

fn workspace_activity_tasks<'a>(
    snapshot: &'a MonitorSnapshot,
    workspace: &str,
    capacity: usize,
) -> Vec<&'a TaskRecord> {
    let mut tasks = snapshot
        .tasks
        .iter()
        .filter(|task| task.workspace == workspace)
        .collect::<Vec<_>>();
    tasks.sort_by(|a, b| {
        activity_rank(a.status)
            .cmp(&activity_rank(b.status))
            .then_with(|| task_time(b).cmp(&task_time(a)))
    });
    tasks.truncate(capacity);
    tasks
}

fn activity_rank(status: TaskStatus) -> u8 {
    match status {
        TaskStatus::Running => 0,
        TaskStatus::Queued => 1,
        TaskStatus::Completed | TaskStatus::Failed => 2,
    }
}

fn task_time(task: &TaskRecord) -> Instant {
    task.finished_at
        .or(task.started_at)
        .unwrap_or(task.queued_at)
}

fn activity_item(task: &TaskRecord, tick: usize, now: Instant, width: usize) -> ListItem<'static> {
    let (icon, color, highlighted) = match task.status {
        TaskStatus::Queued => ("◌".to_owned(), YELLOW, true),
        TaskStatus::Running => (
            spinner_frame(tick.wrapping_add(task.id as usize)).to_owned(),
            CYAN,
            true,
        ),
        TaskStatus::Completed => ("✓".to_owned(), GREEN, false),
        TaskStatus::Failed => ("×".to_owned(), RED, true),
    };
    let end = task.finished_at.unwrap_or(now);
    let started = task.started_at.unwrap_or(task.queued_at);
    let elapsed = short_duration(end.saturating_duration_since(started));
    let elapsed_width = 7usize;
    let tool_width = if width >= 52 {
        18
    } else {
        width.saturating_sub(elapsed_width + 4).clamp(8, 22)
    };
    let mut spans = vec![
        Span::styled(format!("{icon} "), Style::default().fg(color)),
        Span::styled(
            format!("{:<tool_width$}", truncate_end(&task.tool, tool_width)),
            Style::default()
                .fg(if highlighted { WHITE } else { GRAY })
                .add_modifier(if highlighted {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ),
    ];
    if width >= 52 {
        let detail_width = width.saturating_sub(tool_width + elapsed_width + 7);
        let detail = if width >= 72 {
            format!(
                "{} · {}/{}",
                task.detail,
                short_bytes(task.request_bytes),
                short_bytes(task.response_bytes)
            )
        } else {
            task.detail.clone()
        };
        spans.push(Span::styled(
            format!(" · {}", truncate_end(&detail, detail_width)),
            Style::default().fg(DIM),
        ));
    }
    spans.push(Span::styled(
        format!(" {elapsed:>elapsed_width$}"),
        Style::default().fg(color),
    ));

    ListItem::new(Line::from(spans)).style(Style::default().bg(if highlighted {
        PANEL_ACTIVE
    } else {
        PANEL_ALT
    }))
}

fn workspace_column_count(width: u16, total: usize) -> usize {
    let inner = width.saturating_sub(4);
    let columns = (inner / 31).max(1) as usize;
    columns.min(total.max(1))
}

fn last_seen_text(last_seen: Option<Instant>) -> String {
    let Some(last_seen) = last_seen else {
        return "—".to_owned();
    };
    let elapsed = last_seen.elapsed();
    if elapsed < Duration::from_secs(2) {
        "just now".to_owned()
    } else if elapsed < Duration::from_secs(60) {
        format!("{}s ago", elapsed.as_secs())
    } else if elapsed < Duration::from_secs(3600) {
        format!("{}m ago", elapsed.as_secs() / 60)
    } else {
        format!("{}h ago", elapsed.as_secs() / 3600)
    }
}

fn is_quick_tunnel(mcp_url: &str) -> bool {
    mcp_url.contains(".trycloudflare.com/")
}

fn open_external_url(url: &str) -> io::Result<()> {
    let mut command = if cfg!(target_os = "macos") {
        let mut command = StdCommand::new("open");
        command.arg(url);
        command
    } else if cfg!(target_os = "windows") {
        let mut command = StdCommand::new("explorer.exe");
        command.arg(url);
        command
    } else {
        let mut command = StdCommand::new("xdg-open");
        command.arg(url);
        command
    };
    command
        .stdin(StdStdio::null())
        .stdout(StdStdio::null())
        .stderr(StdStdio::null())
        .spawn()
        .map(|_| ())
}

fn render_help_overlay(frame: &mut Frame<'_>, area: Rect, config: &MonitorConfig) {
    let width = area.width.saturating_sub(6).clamp(36, 98);
    let height = area.height.saturating_sub(4).clamp(12, 18);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PURPLE))
        .style(Style::default().bg(PANEL_ACTIVE))
        .padding(Padding::horizontal(1))
        .title(Line::from(vec![
            Span::styled(
                " ? ",
                Style::default()
                    .fg(BG)
                    .bg(PURPLE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " HELP & LINKS ",
                Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
            ),
        ]))
        .title(
            Line::from(Span::styled(" ESC TO CLOSE ", Style::default().fg(DIM))).right_aligned(),
        );
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if width < 74 || height < 16 {
        frame.render_widget(
            Paragraph::new(vec![
                help_hint_line("←/→", "move workspace"),
                help_hint_line("⇧←/→", "move one page"),
                help_hint_line("O", "Connector setup"),
                help_hint_line("G / A", "project / author"),
                help_hint_line("? / Esc", "toggle help"),
                help_hint_line("^C", "stop wcode"),
                help_link_line("Project", &config.project_url, inner.width),
                help_link_line("Author", &config.author_url, inner.width),
                help_link_line("Setup", &config.setup_url, inner.width),
                help_link_line("Local", &config.local_url, inner.width),
            ]),
            inner,
        );
        return;
    }

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "SHORTCUTS",
                Style::default().fg(DIM).add_modifier(Modifier::BOLD),
            )),
            help_hint_line("← / →", "move workspace focus"),
            help_hint_line("Shift + ← / →", "move one workspace page"),
            help_hint_line("O", "open Connector setup"),
            help_hint_line("G", "open project repository"),
            help_hint_line("A", "open author profile"),
            help_hint_line("? / Esc", "open or close help"),
            help_hint_line("Ctrl-C", "stop wcode"),
        ])
        .block(
            Block::default()
                .borders(Borders::RIGHT)
                .border_style(Style::default().fg(BORDER))
                .padding(Padding::new(0, 2, 0, 0)),
        ),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "RUNTIME",
                Style::default().fg(DIM).add_modifier(Modifier::BOLD),
            )),
            Line::from(vec![
                Span::styled("Slots  ", Style::default().fg(WHITE)),
                Span::styled("active child tasks / cap", Style::default().fg(GRAY)),
            ]),
            Line::from(vec![
                Span::styled("Peak   ", Style::default().fg(WHITE)),
                Span::styled(
                    "real concurrency high-water mark",
                    Style::default().fg(GRAY),
                ),
            ]),
            Line::from(vec![
                Span::styled("Fan-out", Style::default().fg(WHITE)),
                Span::styled(
                    "  parallel_tools · review · verify",
                    Style::default().fg(GRAY),
                ),
            ]),
            Line::from(vec![
                Span::styled("CTX    ", Style::default().fg(WHITE)),
                Span::styled("estimated tool-output tokens", Style::default().fg(GRAY)),
            ]),
            Line::from(vec![
                Span::styled("Saved  ", Style::default().fg(WHITE)),
                Span::styled(
                    format!(
                        "AST context avoided · EST at ${:.2}/M",
                        config.input_token_price_per_million_usd
                    ),
                    Style::default().fg(GRAY),
                ),
            ]),
            Line::from(""),
            help_link_line("Project", &config.project_url, columns[1].width),
            help_link_line("Author", &config.author_url, columns[1].width),
            help_link_line("Setup", &config.setup_url, columns[1].width),
            help_link_line("Local", &config.local_url, columns[1].width),
        ]),
        columns[1],
    );
}

fn help_hint_line(key: &str, label: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(" {key:<15}"),
            Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(label.to_owned(), Style::default().fg(GRAY)),
    ])
}

fn help_link_line(label: &str, url: &str, width: u16) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(DIM)),
        Span::styled(
            truncate_middle(url, width.saturating_sub(10) as usize),
            Style::default().fg(BLUE).add_modifier(Modifier::UNDERLINED),
        ),
    ])
}

fn render_throughput(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
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
            " THROUGHPUT ",
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

fn slot_bar(active: u64, capacity: u64, width: usize) -> (String, String, Color) {
    let capacity = capacity.max(1);
    let ratio = (active as f64 / capacity as f64).clamp(0.0, 1.0);
    let filled = ((ratio * width as f64).round() as usize).min(width);
    let color = if ratio >= 0.85 {
        RED
    } else if ratio >= 0.6 {
        YELLOW
    } else {
        CYAN
    };
    (
        "━".repeat(filled),
        "·".repeat(width.saturating_sub(filled)),
        color,
    )
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, config: &MonitorConfig) {
    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);
    let project = config
        .project_url
        .strip_prefix("https://")
        .unwrap_or(&config.project_url)
        .trim_end_matches('/');

    if area.width >= 124 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
            .split(area);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "  wcode  ",
                    Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    project.to_owned(),
                    Style::default().fg(BLUE).add_modifier(Modifier::UNDERLINED),
                ),
                Span::styled("  by  ", Style::default().fg(DIM)),
                Span::styled(
                    config.author_handle.clone(),
                    Style::default()
                        .fg(PURPLE)
                        .add_modifier(Modifier::UNDERLINED),
                ),
            ])),
            columns[0],
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                keycap("←/→"),
                Span::styled(" workspace  ", Style::default().fg(GRAY)),
                keycap("O"),
                Span::styled(" setup  ", Style::default().fg(GRAY)),
                keycap("G"),
                Span::styled(" project  ", Style::default().fg(GRAY)),
                keycap("A"),
                Span::styled(" author  ", Style::default().fg(GRAY)),
                keycap("?"),
                Span::styled(" help  ", Style::default().fg(GRAY)),
                keycap("^C"),
                Span::styled(" stop", Style::default().fg(GRAY)),
            ]))
            .alignment(ratatui::layout::Alignment::Right),
            columns[1],
        );
        return;
    }

    let line = if area.width >= 78 {
        Line::from(vec![
            Span::raw(" "),
            keycap("←/→"),
            Span::styled(" workspace  ", Style::default().fg(GRAY)),
            keycap("O"),
            Span::styled(" setup  ", Style::default().fg(GRAY)),
            keycap("G"),
            Span::styled(" repo  ", Style::default().fg(GRAY)),
            keycap("A"),
            Span::styled(" author  ", Style::default().fg(GRAY)),
            keycap("?"),
            Span::styled(" help  ", Style::default().fg(GRAY)),
            keycap("^C"),
        ])
    } else {
        Line::from(vec![
            Span::raw(" "),
            keycap("←/→"),
            Span::raw("  "),
            keycap("O"),
            Span::raw("  "),
            keycap("G"),
            Span::raw("  "),
            keycap("?"),
            Span::raw("  "),
            keycap("^C"),
        ])
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn keycap(key: &str) -> Span<'static> {
    Span::styled(
        format!(" {key} "),
        Style::default()
            .fg(WHITE)
            .bg(PANEL_ALT)
            .add_modifier(Modifier::BOLD),
    )
}

fn totals(snapshot: &MonitorSnapshot) -> WorkspaceStats {
    snapshot
        .workspaces
        .values()
        .fold(WorkspaceStats::default(), |mut total, stats| {
            total.queued = total.queued.saturating_add(stats.queued);
            total.active = total.active.saturating_add(stats.active);
            total.completed = total.completed.saturating_add(stats.completed);
            total.failed = total.failed.saturating_add(stats.failed);
            total.calls = total.calls.saturating_add(stats.calls);
            total.request_bytes = total.request_bytes.saturating_add(stats.request_bytes);
            total.response_bytes = total.response_bytes.saturating_add(stats.response_bytes);
            total.context_bytes_avoided = total
                .context_bytes_avoided
                .saturating_add(stats.context_bytes_avoided);
            total
        })
}

fn success_rate(completed: u64, failed: u64) -> f64 {
    let finished = completed.saturating_add(failed);
    if finished == 0 {
        100.0
    } else {
        completed as f64 * 100.0 / finished as f64
    }
}

fn window_totals(snapshot: &MonitorSnapshot, window: Duration) -> (u64, u64, u64) {
    let now = Instant::now();
    snapshot
        .traffic
        .iter()
        .filter(|event| now.saturating_duration_since(event.at) <= window)
        .fold((0, 0, 0), |(requests, rx, tx), event| {
            (
                requests.saturating_add(event.requests),
                rx.saturating_add(event.request_bytes),
                tx.saturating_add(event.response_bytes),
            )
        })
}

fn window_context_avoided(snapshot: &MonitorSnapshot, window: Duration) -> u64 {
    let now = Instant::now();
    snapshot
        .traffic
        .iter()
        .filter(|event| now.saturating_duration_since(event.at) <= window)
        .fold(0u64, |total, event| {
            total.saturating_add(event.context_bytes_avoided)
        })
}

fn request_bins(snapshot: &MonitorSnapshot, count: usize, width: Duration) -> Vec<u64> {
    let now = Instant::now();
    let mut bins = vec![0u64; count];
    for event in &snapshot.traffic {
        let age = now.saturating_duration_since(event.at);
        let index_from_end = (age.as_secs_f64() / width.as_secs_f64()) as usize;
        if index_from_end < count {
            let index = count - 1 - index_from_end;
            bins[index] = bins[index].saturating_add(event.requests);
        }
    }
    bins
}

fn sparkline(values: &[u64]) -> String {
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

fn spinner_frame(tick: usize) -> &'static str {
    SPINNER_FRAMES[tick % SPINNER_FRAMES.len()]
}

fn short_duration(duration: Duration) -> String {
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

fn short_bytes(bytes: u64) -> String {
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

fn estimated_tokens(bytes: u64) -> u64 {
    (bytes as f64 / ESTIMATED_BYTES_PER_TOKEN).ceil() as u64
}

fn short_tokens(tokens: u64) -> String {
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

fn estimated_cost_usd(context_bytes: u64, price_per_million: f64) -> f64 {
    estimated_tokens(context_bytes) as f64 * price_per_million.max(0.0) / 1_000_000.0
}

fn short_usd(value: f64) -> String {
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

fn truncate_end(value: &str, max_chars: usize) -> String {
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

fn truncate_middle(value: &str, max_chars: usize) -> String {
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
        let config = MonitorConfig {
            version: "0.1.0".to_owned(),
            local_url: "http://127.0.0.1:8765".to_owned(),
            mcp_url: "https://example.trycloudflare.com/mcp".to_owned(),
            setup_url: "https://chatgpt.com/plugins#settings/Connectors".to_owned(),
            project_url: "https://github.com/francis-du/wcode".to_owned(),
            author_url: "https://github.com/francis-du".to_owned(),
            author_handle: "@francis-du".to_owned(),
            pairing_code: "123456".to_owned(),
            max_parallel: 16,
            input_token_price_per_million_usd: 5.0,
            workspaces: vec![("backend".to_owned(), "/code/backend".to_owned(), true)],
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
    }

    #[test]
    fn narrow_and_tiny_layouts_do_not_panic() {
        let monitor = TaskMonitor::new(["backend".to_owned(), "frontend".to_owned()]);
        let config = MonitorConfig {
            version: "0.1.0".to_owned(),
            local_url: "http://127.0.0.1:8765".to_owned(),
            mcp_url: "https://example.trycloudflare.com/mcp".to_owned(),
            setup_url: "https://chatgpt.com/plugins#settings/Connectors".to_owned(),
            project_url: "https://github.com/francis-du/wcode".to_owned(),
            author_url: "https://github.com/francis-du".to_owned(),
            author_handle: "@francis-du".to_owned(),
            pairing_code: "123456".to_owned(),
            max_parallel: 16,
            input_token_price_per_million_usd: 5.0,
            workspaces: vec![
                ("backend".to_owned(), "/code/backend".to_owned(), true),
                ("frontend".to_owned(), "/code/frontend".to_owned(), false),
            ],
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
        monitor.mark_chatgpt_connected();
        let mut saved = monitor.queue("backend", "symbol_context", "saved context", 1);
        saved.start();
        saved.finish_with_context_savings(true, 400, 4_000);
        let mut first = monitor.queue("backend", "read_file", "one", 1);
        let mut second = monitor.queue("backend", "search_code", "two", 1);
        first.start();
        second.start();
        let config = MonitorConfig {
            version: "0.1.0".to_owned(),
            local_url: "http://127.0.0.1:8765".to_owned(),
            mcp_url: "https://example.trycloudflare.com/mcp".to_owned(),
            setup_url: "https://chatgpt.com/plugins#settings/Connectors".to_owned(),
            project_url: "https://github.com/francis-du/wcode".to_owned(),
            author_url: "https://github.com/francis-du".to_owned(),
            author_handle: "@francis-du".to_owned(),
            pairing_code: "123456".to_owned(),
            max_parallel: 8,
            input_token_price_per_million_usd: 5.0,
            workspaces: vec![("backend".to_owned(), "/code/backend".to_owned(), true)],
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
        assert!(text.contains("Local:"));
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
        let config = MonitorConfig {
            version: "0.1.0".to_owned(),
            local_url: "http://127.0.0.1:8765".to_owned(),
            mcp_url: "https://example.trycloudflare.com/mcp".to_owned(),
            setup_url: "https://chatgpt.com/plugins#settings/Connectors".to_owned(),
            project_url: "https://github.com/francis-du/wcode".to_owned(),
            author_url: "https://github.com/francis-du".to_owned(),
            author_handle: "@francis-du".to_owned(),
            pairing_code: "123456".to_owned(),
            max_parallel: 8,
            input_token_price_per_million_usd: 5.0,
            workspaces: vec![("backend".to_owned(), "/code/backend".to_owned(), true)],
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
        monitor.mark_chatgpt_connected();
        monitor.mark_chatgpt_connected();
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
