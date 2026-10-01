use super::*;
pub(crate) use crate::monitor_jobs::{
    MonitorJobAccess, MonitorJobOrigin, MonitorJobSnapshot, MonitorJobStream,
};

const JOB_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const JOB_STREAM_LIMIT: usize = 32 * 1024;

impl TaskMonitor {
    pub(crate) fn register_job_access(&self, access: Arc<dyn MonitorJobAccess>) {
        *self
            .job_access
            .lock()
            .expect("monitor job bridge lock poisoned") = Some(access);
    }

    fn job_access(&self) -> Option<Arc<dyn MonitorJobAccess>> {
        self.job_access.lock().ok()?.clone()
    }
}

impl TaskTicket {
    pub(crate) fn bind_command_job(&self, job_id: &str) {
        if job_id.is_empty() || job_id.len() > 160 || job_id.chars().any(char::is_control) {
            return;
        }
        let mut state = self
            .inner
            .monitor
            .state
            .lock()
            .expect("task monitor lock poisoned");
        if let Some(task) = state.tasks.iter_mut().find(|task| task.id == self.inner.id) {
            if task.command_job_id.is_none() {
                task.command_job_id = Some(job_id.to_owned());
            }
        }
    }
}

#[derive(Default)]
pub(super) struct CommandJobView {
    selection: Option<(u64, String, String)>,
    snapshot: Option<MonitorJobSnapshot>,
    observed_at: Option<Instant>,
    error: Option<String>,
    requested_at: Option<Instant>,
    pending: Option<std::sync::mpsc::Receiver<Result<MonitorJobSnapshot, String>>>,
}

fn selected_command_task<'a>(
    ui: &DashboardState,
    snapshot: &'a MonitorSnapshot,
    config: &MonitorConfig,
) -> Option<&'a TaskRecord> {
    if !ui.intelligence_open || ui.console_tab != ConsoleTab::Tasks {
        return None;
    }
    let workspace = console_workspace(ui, snapshot, config)?;
    let tasks = console_tasks(snapshot, &workspace);
    tasks.get(task_selection(ui, &tasks)).copied()
}

pub(super) fn refresh_command_job(
    ui: &mut DashboardState,
    monitor: &TaskMonitor,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
) {
    let selection = selected_command_task(ui, snapshot, config).and_then(|task| {
        task.command_job_id
            .as_ref()
            .map(|id| (task.id, task.workspace.clone(), id.clone()))
    });
    if ui.command_job.selection != selection {
        ui.command_job = CommandJobView {
            selection,
            ..Default::default()
        };
    }
    let view = &mut ui.command_job;
    if let Some(receiver) = view.pending.as_ref() {
        match receiver.try_recv() {
            Ok(result) => {
                view.pending = None;
                view.observed_at = Some(Instant::now());
                match result {
                    Ok(observation) => {
                        let exact = view.selection.as_ref().is_some_and(|(_, workspace, id)| {
                            observation.workspace == *workspace && observation.job_id == *id
                        });
                        if exact {
                            view.snapshot = Some(observation);
                            view.error = None;
                        } else {
                            view.snapshot = None;
                            view.error = Some("Job identity mismatch; observation rejected".into());
                        }
                    }
                    Err(error) => view.error = Some(console_clean(&error)),
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                view.pending = None;
                view.error = Some("Job observer disconnected; state unknown".into());
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }
    if view.pending.is_some()
        || view
            .requested_at
            .is_some_and(|at| at.elapsed() < JOB_REFRESH_INTERVAL)
    {
        return;
    }
    let Some((_, workspace, job_id)) = view.selection.clone() else {
        return;
    };
    view.requested_at = Some(Instant::now());
    let Some(access) = monitor.job_access() else {
        view.error = Some("Command log observer unavailable; state unknown".into());
        return;
    };
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        view.error = Some("Command log runtime unavailable; state unknown".into());
        return;
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    view.pending = Some(receiver);
    runtime.spawn_blocking(move || {
        let result = access
            .observe(&workspace, &job_id)
            .map_err(|error| error.to_string());
        let _ = sender.send(result);
    });
}

pub(super) fn handle_command_job_key(
    key: event::KeyEvent,
    ui: &mut DashboardState,
    area: Rect,
    monitor: &TaskMonitor,
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
) -> bool {
    if !matches!(key.code, KeyCode::Char('x' | 'X'))
        || key.kind != KeyEventKind::Press
        || !key.modifiers.difference(KeyModifiers::SHIFT).is_empty()
        || ui.help_open
        || ui.commands_open
        || ui.full_access_confirm
        || ui.workspace_input.is_some()
        || area.width < 40
        || area.height < 16
    {
        return false;
    }
    let Some(task) = selected_command_task(ui, snapshot, config) else {
        return false;
    };
    let Some(observation) = ui.command_job.snapshot.as_ref() else {
        ui.workspace_message = Some("Job state unknown; refresh before requesting stop".into());
        return true;
    };
    let exact = task.command_job_id.as_ref() == Some(&observation.job_id)
        && task.workspace == observation.workspace
        && ui
            .command_job
            .selection
            .as_ref()
            .is_some_and(|(id, _, _)| *id == task.id);
    if !exact
        || ui.command_job.error.is_some()
        || observation.origin != MonitorJobOrigin::Ui
        || !observation.can_cancel
    {
        ui.workspace_message =
            Some("This task is observation-only; stop is restricted to an owned UI job".into());
        return true;
    }
    let Some(access) = monitor.job_access() else {
        ui.workspace_message = Some("Job control unavailable; stop was not requested".into());
        return true;
    };
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        ui.workspace_message =
            Some("Job control runtime unavailable; stop was not requested".into());
        return true;
    };
    let workspace = observation.workspace.clone();
    let job_id = observation.job_id.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    ui.command_job.pending = Some(receiver);
    ui.command_job.requested_at = Some(Instant::now());
    ui.workspace_message =
        Some("Stop requested for the owned UI job; awaiting runtime confirmation".into());
    runtime.spawn_blocking(move || {
        let result = access
            .cancel(&workspace, &job_id)
            .and_then(|()| access.observe(&workspace, &job_id))
            .map_err(|error| error.to_string());
        let _ = sender.send(result);
    });
    true
}

fn stream_lines(label: &str, stream: &MonitorJobStream) -> Vec<Line<'static>> {
    let safe = console_clean(&stream.text);
    let mut start = safe.len().saturating_sub(JOB_STREAM_LIMIT);
    while !safe.is_char_boundary(start) {
        start += 1;
    }
    let tail = &safe[start..];
    let truncated = stream.truncated || start > 0;
    let mut lines = vec![Line::from(Span::styled(
        format!(
            "{label} · {}B observed{}{}",
            stream.total_bytes,
            if truncated { " · tail truncated" } else { "" },
            if stream.redacted { " · redacted" } else { "" }
        ),
        Style::default().fg(TEXT_DIM),
    ))];
    if tail.is_empty() {
        lines.push(Line::from("No output observed in this stream."));
    } else {
        lines.extend(tail.lines().map(|line| Line::from(line.to_owned())));
    }
    lines
}

pub(super) fn command_job_lines(task: &TaskRecord, ui: &DashboardState) -> Vec<Line<'static>> {
    let Some(job_id) = task.command_job_id.as_ref() else {
        return vec![Line::from(
            "Command logs unavailable for this call; no durable job was bound.",
        )];
    };
    let view = &ui.command_job;
    if !view.selection.as_ref().is_some_and(|(id, workspace, job)| {
        *id == task.id && workspace == &task.workspace && job == job_id
    }) {
        return vec![Line::from(
            "Command log observation pending · state unknown",
        )];
    }
    let mut lines = vec![Line::from(Span::styled(
        format!(
            "JOB {} · observed {}",
            console_clean(job_id),
            view.observed_at
                .map(|at| format!("{}s ago", at.elapsed().as_secs()))
                .unwrap_or_else(|| "unknown".into())
        ),
        Style::default().fg(ACCENT),
    ))];
    if let Some(error) = &view.error {
        lines.push(Line::from(Span::styled(
            format!("Observation unavailable: {}", console_clean(error)),
            Style::default().fg(WARNING),
        )));
        if view.snapshot.is_some() {
            lines.push(Line::from(
                "Last observed output follows; current job state is unknown.",
            ));
        }
    }
    let Some(observation) = &view.snapshot else {
        if view.error.is_none() {
            lines.push(Line::from("Loading command logs · state unknown"));
        }
        return lines;
    };
    let owner = match observation.origin {
        MonitorJobOrigin::Ui => "UI-owned",
        MonitorJobOrigin::Mcp => "MCP-owned · observation-only",
    };
    let outcome = match observation.success {
        Some(true) => "command passed",
        Some(false) => "command failed",
        None => "command outcome unknown",
    };
    lines.push(Line::from(format!(
        "{} · {} · {outcome} · exit {}",
        console_clean(&observation.status),
        owner,
        observation
            .exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "unknown".into())
    )));
    if observation.origin == MonitorJobOrigin::Ui && observation.can_cancel && view.error.is_none()
    {
        lines.push(Line::from(
            "X request stop for this owned UI job · PgUp/PgDn logs",
        ));
    }
    if let Some(error) = &observation.error {
        lines.push(Line::from(Span::styled(
            console_clean(error),
            Style::default().fg(WARNING),
        )));
    }
    lines.extend(stream_lines("STDOUT", &observation.stdout));
    lines.extend(stream_lines("STDERR", &observation.stderr));
    lines
}

#[cfg(test)]
#[path = "../../../tests/unit/ui/monitor/job_view.rs"]
mod tests;
