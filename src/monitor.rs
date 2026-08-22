use std::collections::{BTreeMap, VecDeque};
use std::io::{self, IsTerminal, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

const MAX_RECENT_TASKS: usize = 48;

#[derive(Clone)]
pub struct TaskMonitor {
    state: Arc<Mutex<MonitorState>>,
}

struct MonitorState {
    next_id: u64,
    workspaces: BTreeMap<String, WorkspaceStats>,
    tasks: VecDeque<TaskRecord>,
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
}

#[derive(Clone)]
struct TaskRecord {
    id: u64,
    workspace: String,
    tool: String,
    status: TaskStatus,
    queued_at: Instant,
    started_at: Option<Instant>,
    finished_at: Option<Instant>,
    request_bytes: u64,
    response_bytes: u64,
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
    pub pairing_code: String,
    pub max_parallel: usize,
    pub workspaces: Vec<(String, String, bool)>,
}

pub struct MonitorRenderer {
    stop: watch::Sender<bool>,
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
                workspaces,
                tasks: VecDeque::new(),
            })),
        }
    }

    pub fn queue(
        &self,
        workspace: impl Into<String>,
        tool: impl Into<String>,
        request_bytes: u64,
    ) -> TaskTicket {
        let workspace = workspace.into();
        let tool = tool.into();
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        let id = state.next_id;
        state.next_id = state.next_id.saturating_add(1);
        let stats = state.workspaces.entry(workspace.clone()).or_default();
        stats.queued += 1;
        stats.calls += 1;
        stats.request_bytes = stats.request_bytes.saturating_add(request_bytes);
        state.tasks.push_back(TaskRecord {
            id,
            workspace,
            tool,
            status: TaskStatus::Queued,
            queued_at: Instant::now(),
            started_at: None,
            finished_at: None,
            request_bytes,
            response_bytes: 0,
        });
        while state.tasks.len() > MAX_RECENT_TASKS {
            if state
                .tasks
                .front()
                .is_some_and(|task| matches!(task.status, TaskStatus::Running | TaskStatus::Queued))
            {
                break;
            }
            state.tasks.pop_front();
        }
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
        let (stop, mut stop_rx) = watch::channel(false);
        let join = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(250));
            loop {
                tokio::select! {
                    _ = interval.tick() => render(&monitor, &config),
                    changed = stop_rx.changed() => {
                        if changed.is_err() || *stop_rx.borrow() {
                            break;
                        }
                    }
                }
            }
            render(&monitor, &config);
            println!();
        });
        Some(MonitorRenderer { stop, join })
    }

    fn start(&self, id: u64) {
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        let Some(index) = state.tasks.iter().position(|task| task.id == id) else {
            return;
        };
        let workspace = state.tasks[index].workspace.clone();
        if let Some(stats) = state.workspaces.get_mut(&workspace) {
            stats.queued = stats.queued.saturating_sub(1);
            stats.active += 1;
        }
        let task = &mut state.tasks[index];
        task.status = TaskStatus::Running;
        task.started_at = Some(Instant::now());
    }

    fn finish(&self, id: u64, success: bool, response_bytes: u64) {
        let mut state = self.state.lock().expect("task monitor lock poisoned");
        let Some(index) = state.tasks.iter().position(|task| task.id == id) else {
            return;
        };
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
            if success {
                stats.completed += 1;
            } else {
                stats.failed += 1;
            }
        }
        let task = &mut state.tasks[index];
        task.status = if success {
            TaskStatus::Completed
        } else {
            TaskStatus::Failed
        };
        task.finished_at = Some(Instant::now());
        task.response_bytes = response_bytes;
    }

    fn snapshot(&self) -> MonitorSnapshot {
        let state = self.state.lock().expect("task monitor lock poisoned");
        MonitorSnapshot {
            workspaces: state.workspaces.clone(),
            tasks: state.tasks.iter().cloned().collect(),
        }
    }
}

impl TaskTicket {
    pub fn start(&mut self) {
        self.monitor.start(self.id);
    }

    pub fn finish(mut self, success: bool, response_bytes: u64) {
        self.monitor.finish(self.id, success, response_bytes);
        self.finished = true;
    }
}

impl Drop for TaskTicket {
    fn drop(&mut self) {
        if !self.finished {
            self.monitor.finish(self.id, false, 0);
            self.finished = true;
        }
    }
}

impl MonitorRenderer {
    pub async fn stop(self) {
        let _ = self.stop.send(true);
        let _ = self.join.await;
    }
}

struct MonitorSnapshot {
    workspaces: BTreeMap<String, WorkspaceStats>,
    tasks: Vec<TaskRecord>,
}

fn render(monitor: &TaskMonitor, config: &MonitorConfig) {
    let snapshot = monitor.snapshot();
    let now = Instant::now();
    let mut output = String::with_capacity(4096);
    output.push_str("\x1b[2J\x1b[H");
    output.push_str(&format!(
        "╭─ wcode {} · live ─────────────────────────────────────────────────────\n",
        config.version
    ));
    output.push_str(&format!("│ MCP        {}\n", config.mcp_url));
    output.push_str(&format!("│ Local      {}\n", config.local_url));
    output.push_str(&format!("│ Pair code  {}\n", config.pairing_code));
    output.push_str(&format!(
        "│ Parallel   {} tool calls\n",
        config.max_parallel
    ));
    output.push_str("├─ Workspaces ──────────────────────────────────────────────────────────\n");
    output
        .push_str("│ workspace          calls queue active  done fail    req/resp       current\n");

    for (id, path, is_default) in &config.workspaces {
        let stats = snapshot.workspaces.get(id).cloned().unwrap_or_default();
        let current = snapshot
            .tasks
            .iter()
            .filter(|task| task.workspace == *id && task.status == TaskStatus::Running)
            .take(2)
            .map(|task| {
                let elapsed = task.started_at.unwrap_or(task.queued_at).elapsed();
                format!("{} {}", task.tool, short_duration(elapsed))
            })
            .collect::<Vec<_>>()
            .join(", ");
        let marker = if *is_default { "*" } else { " " };
        output.push_str(&format!(
            "│ {marker}{id:<17} {calls:>5} {queued:>5} {active:>6} {done:>5} {failed:>4}  {io:>12}   {current}\n",
            calls = stats.calls,
            queued = stats.queued,
            active = stats.active,
            done = stats.completed,
            failed = stats.failed,
            io = format!("{}/{}", short_bytes(stats.request_bytes), short_bytes(stats.response_bytes)),
        ));
        if current.is_empty() {
            output.push_str(&format!("│   {:<17} {}\n", "", path));
        }
    }

    output.push_str("├─ Recent tasks ────────────────────────────────────────────────────────\n");
    let mut shown = 0usize;
    for task in snapshot.tasks.iter().rev().take(8).rev() {
        shown += 1;
        let (icon, label) = match task.status {
            TaskStatus::Queued => ("…", "queued"),
            TaskStatus::Running => ("▶", "running"),
            TaskStatus::Completed => ("✓", "done"),
            TaskStatus::Failed => ("×", "failed"),
        };
        let end = task.finished_at.unwrap_or(now);
        let start = task.started_at.unwrap_or(task.queued_at);
        let elapsed = end.saturating_duration_since(start);
        output.push_str(&format!(
            "│ #{:<4} {} {:<10} {:<18} {:<7} {:>7}  {}/{}\n",
            task.id,
            icon,
            task.workspace,
            task.tool,
            label,
            short_duration(elapsed),
            short_bytes(task.request_bytes),
            short_bytes(task.response_bytes),
        ));
    }
    if shown == 0 {
        output.push_str("│ No MCP tasks yet. Waiting for requests…\n");
    }
    output.push_str("├──────────────────────────────────────────────────────────────────────\n");
    output.push_str("│ * default workspace   GitHub: https://github.com/francis-du/wcode\n");
    output.push_str("╰─ Ctrl-C to stop ─────────────────────────────────────────────────────\n");

    let mut stdout = io::stdout().lock();
    let _ = stdout.write_all(output.as_bytes());
    let _ = stdout.flush();
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
    if bytes >= 1024 * 1024 {
        format!("{:.1}M", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1}K", bytes as f64 / 1024.0)
    } else {
        format!("{bytes}B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_task_lifecycle_per_workspace() {
        let monitor = TaskMonitor::new(["api".to_owned(), "web".to_owned()]);
        let mut ticket = monitor.queue("web", "read_file", 128);
        assert_eq!(monitor.snapshot().workspaces["web"].queued, 1);
        ticket.start();
        assert_eq!(monitor.snapshot().workspaces["web"].active, 1);
        ticket.finish(true, 512);
        let snapshot = monitor.snapshot();
        assert_eq!(snapshot.workspaces["web"].active, 0);
        assert_eq!(snapshot.workspaces["web"].completed, 1);
        assert_eq!(snapshot.workspaces["api"].completed, 0);
    }

    #[test]
    fn dropped_ticket_is_failed() {
        let monitor = TaskMonitor::new(["api".to_owned()]);
        let mut ticket = monitor.queue("api", "search_code", 64);
        ticket.start();
        drop(ticket);
        let snapshot = monitor.snapshot();
        assert_eq!(snapshot.workspaces["api"].active, 0);
        assert_eq!(snapshot.workspaces["api"].failed, 1);
    }
}
