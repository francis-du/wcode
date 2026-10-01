use crate::mcp::{call_tool_owned, jsonrpc_error, modern_result, selected_workspace, AppState};
use crate::task_store::{self, TaskRecord, TaskStatus};
use crate::workspace::{CommandOutputChunk, Workspace};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;
use tokio::task::{AbortHandle, JoinSet};
use tokio::time::Instant;

#[path = "task_tools.rs"]
mod compatibility;
#[path = "task_observation.rs"]
pub(crate) mod observation;
pub(super) use compatibility::{create_ordinary_command_task, ordinary_task_tool};

tokio::task_local! {
    static MONITOR_DURABLE_TASK_ID: String;
}

pub(super) fn bind_monitor_task(tool: &str, ticket: &crate::monitor::TaskTicket) {
    if tool == CONDITIONAL_TASK_TOOL {
        let _ = MONITOR_DURABLE_TASK_ID.try_with(|id| ticket.bind_command_job(id));
    }
}

pub(super) fn register_monitor_bridge(state: &Arc<AppState>) {
    state
        .monitor
        .register_job_access(Arc::new(MonitorTaskAccess {
            state: Arc::downgrade(state),
        }));
}

struct MonitorTaskAccess {
    state: std::sync::Weak<AppState>,
}

// This namespace is chosen by the server, never from MCP arguments, OAuth
// client IDs or claimed actor labels. Existing UI has no command-launch route.
pub(crate) fn monitor_ui_owner(state: &AppState) -> String {
    monitor_ui_owner_for_instance(state.auth.instance_id())
}

fn monitor_ui_owner_for_instance(instance_id: &str) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "{:x}",
        Sha256::digest(format!("ui:{instance_id}").as_bytes())
    )
}

impl MonitorTaskAccess {
    fn load_exact(
        state: &AppState,
        workspace_id: &str,
        job_id: &str,
    ) -> anyhow::Result<(Workspace, TaskRecord)> {
        let (selected, workspace) = state.workspaces.select(Some(workspace_id))?;
        let record = task_store::load_for_observation(&workspace, job_id)?
            .ok_or_else(|| anyhow::anyhow!("Unknown command job in this workspace"))?;
        if selected != workspace_id
            || record.workspace != workspace_id
            || record.task_id != job_id
            || record.tool_name != CONDITIONAL_TASK_TOOL
        {
            anyhow::bail!("Command job workspace or identity mismatch");
        }
        Ok((workspace, record))
    }
}

impl crate::monitor_jobs::MonitorJobAccess for MonitorTaskAccess {
    fn observe(
        &self,
        workspace_id: &str,
        job_id: &str,
    ) -> anyhow::Result<crate::monitor_jobs::MonitorJobSnapshot> {
        let state = self
            .state
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("Command job runtime disconnected; state unknown"))?;
        let _guard = state
            .tasks
            .state_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("Command job state lock unavailable"))?;
        let (workspace, mut record) = Self::load_exact(&state, workspace_id, job_id)?;
        reconcile_task_record(&state, &workspace, &mut record)
            .map_err(|error| anyhow::anyhow!(error.message))?;
        let ui_origin = record.owner == monitor_ui_owner_for_instance(&record.runtime_instance_id);
        let ui_owned = ui_origin && record.runtime_instance_id == state.auth.instance_id();
        let output = record
            .result
            .as_ref()
            .and_then(|result| result.get("structuredContent"))
            .or(record.live_output.as_ref());
        let mut error = record
            .error
            .as_ref()
            .and_then(|error| error["message"].as_str())
            .map(|error| crate::workspace::redact_sensitive_text(error).0);
        if output.is_none() && error.is_none() {
            error = Some("No command log snapshot retained; output capture is unknown".into());
        }
        let stdout = monitor_job_stream(output, "stdout")?;
        let stderr = monitor_job_stream(output, "stderr")?;
        let success = output.and_then(|output| output["success"].as_bool());
        let exit_code = output
            .and_then(|output| output["exit_code"].as_i64())
            .and_then(|value| i32::try_from(value).ok());
        Ok(crate::monitor_jobs::MonitorJobSnapshot {
            job_id: record.task_id,
            workspace: record.workspace,
            status: match record.status {
                TaskStatus::Working => "working",
                TaskStatus::InputRequired => "input_required",
                TaskStatus::Completed => "completed",
                TaskStatus::Cancelled => "cancelled",
                TaskStatus::Failed => "failed",
            }
            .into(),
            origin: if ui_origin {
                crate::monitor_jobs::MonitorJobOrigin::Ui
            } else {
                crate::monitor_jobs::MonitorJobOrigin::Mcp
            },
            can_cancel: ui_owned && record.status == TaskStatus::Working,
            stdout,
            stderr,
            exit_code,
            success,
            error,
        })
    }

    fn cancel(&self, workspace_id: &str, job_id: &str) -> anyhow::Result<()> {
        let state = self
            .state
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("Command job runtime disconnected; stop unavailable"))?;
        let _guard = state
            .tasks
            .state_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("Command job state lock unavailable"))?;
        let (workspace, mut record) = Self::load_exact(&state, workspace_id, job_id)?;
        if record.owner != monitor_ui_owner(&state)
            || record.runtime_instance_id != state.auth.instance_id()
        {
            anyhow::bail!("MCP-owned jobs are observation-only; UI cancellation denied");
        }
        cancel_task_record(&state, &workspace, &mut record)
            .map_err(|error| anyhow::anyhow!(error.message))?;
        Ok(())
    }
}

fn monitor_job_stream(
    output: Option<&Value>,
    name: &str,
) -> anyhow::Result<crate::monitor_jobs::MonitorJobStream> {
    let Some(output) = output else {
        return Ok(crate::monitor_jobs::MonitorJobStream::default());
    };
    let text = output[name]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Command {name} log is unavailable; state unknown"))?;
    // Redact the complete bounded persisted text before taking a UTF-8 tail.
    let (mut safe, newly_redacted) = crate::workspace::redact_sensitive_text(text);
    if newly_redacted {
        // The shared redactor normalizes lines. Restore only the non-secret
        // trailing terminators, including blank lines, for faithful log tails.
        let ending = &text[text.trim_end_matches(['\r', '\n']).len()..];
        safe.truncate(safe.trim_end_matches(['\r', '\n']).len());
        safe.push_str(ending);
    } else {
        // Preserve unmodified logs exactly, including CRLF and progress returns.
        safe = text.to_owned();
    }
    let mut start = safe.len().saturating_sub(MAX_LIVE_COMMAND_STREAM_BYTES);
    while !safe.is_char_boundary(start) {
        start += 1;
    }
    let dropped = output[format!("{name}DroppedPrefixBytes")]
        .as_u64()
        .unwrap_or(0);
    Ok(crate::monitor_jobs::MonitorJobStream {
        text: safe[start..].to_owned(),
        total_bytes: (text.len() as u64).saturating_add(dropped),
        truncated: start > 0
            || output["truncated"].as_bool() == Some(true)
            || output[format!("{name}Truncated")].as_bool() == Some(true),
        redacted: newly_redacted || output["redacted"].as_bool() == Some(true),
    })
}

pub(crate) const TASK_EXTENSION_ID: &str = "io.modelcontextprotocol/tasks";
const TASK_AUGMENTED_TOOLS: &[&str] = &[
    "semantic_provider_install",
    "semantic_provider_refresh",
    "verification_execute_stages",
    "verify_project",
];
const CONDITIONAL_TASK_TOOL: &str = "run_command";
const MAX_LIVE_COMMAND_STREAM_BYTES: usize = 32 * 1024;
const MAX_LIVE_COMMAND_LINE_BYTES: usize = 64 * 1024;
const COMMAND_OUTPUT_PROGRESS_CHANNEL_CAPACITY: usize = 32;
const LIVE_COMMAND_PERSIST_INTERVAL: Duration = Duration::from_millis(100);

pub(super) fn capabilities() -> Value {
    let mut capabilities = task_store::capabilities();
    let mut tools = TASK_AUGMENTED_TOOLS.to_vec();
    tools.push(CONDITIONAL_TASK_TOOL);
    capabilities["task_augmented_tools"] = json!(tools);
    capabilities["task_augmented_conditions"] = json!({
        "run_command": "arguments.task_mode=true"
    });
    capabilities["live_command_output"] = json!({
        "window": "stream_tail",
        "max_bytes_per_stream": MAX_LIVE_COMMAND_STREAM_BYTES,
        "max_pending_line_bytes": MAX_LIVE_COMMAND_LINE_BYTES,
        "progress_channel_capacity": COMMAND_OUTPUT_PROGRESS_CHANNEL_CAPACITY,
        "redacted": true,
        "durable": true,
        "final_result_contract": "bounded_prefix_unchanged",
    });
    capabilities
}

#[derive(Clone, Default)]
pub(crate) struct TaskRuntime {
    workers: Arc<Mutex<HashMap<String, TaskWorker>>>,
    state_lock: Arc<Mutex<()>>,
}

struct TaskWorker {
    workspace: String,
    handle: AbortHandle,
}

impl TaskRuntime {
    fn register(&self, task_id: String, workspace: String, handle: AbortHandle) {
        self.workers
            .lock()
            .expect("MCP task worker lock poisoned")
            .insert(task_id, TaskWorker { workspace, handle });
    }

    fn running(&self, task_id: &str) -> bool {
        self.workers
            .lock()
            .expect("MCP task worker lock poisoned")
            .get(task_id)
            .is_some_and(|worker| !worker.handle.is_finished())
    }

    fn workspace_for(&self, task_id: &str) -> Option<String> {
        self.workers
            .lock()
            .expect("MCP task worker lock poisoned")
            .get(task_id)
            .map(|worker| worker.workspace.clone())
    }

    fn remove(&self, task_id: &str) {
        self.workers
            .lock()
            .expect("MCP task worker lock poisoned")
            .remove(task_id);
    }

    fn abort(&self, task_id: &str) -> bool {
        self.workers
            .lock()
            .expect("MCP task worker lock poisoned")
            .remove(task_id)
            .is_some_and(|worker| {
                worker.handle.abort();
                true
            })
    }
}

#[derive(Debug)]
pub(super) struct TaskRpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

impl TaskRpcError {
    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    #[cfg(test)]
    pub(super) fn code(&self) -> i64 {
        self.code
    }

    pub(super) fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
            data: None,
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: message.into(),
            data: None,
        }
    }

    pub(super) fn missing_capability() -> Self {
        Self {
            code: -32021,
            message: "Missing required client capability".to_owned(),
            data: Some(json!({
                "requiredCapabilities": {
                    "extensions": {(TASK_EXTENSION_ID): {}}
                }
            })),
        }
    }
}

pub(super) fn task_rpc_error(id: Value, error: TaskRpcError) -> Value {
    let mut value = jsonrpc_error(id, error.code, error.message);
    if let Some(data) = error.data {
        if let Some(object) = value
            .get_mut("error")
            .and_then(serde_json::Value::as_object_mut)
        {
            object.insert("data".to_owned(), data);
        }
    }
    value
}

pub(super) fn client_supports_tasks(message: &Value) -> bool {
    message
        .pointer("/params/_meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("io.modelcontextprotocol/clientCapabilities"))
        .and_then(Value::as_object)
        .and_then(|capabilities| capabilities.get("extensions"))
        .and_then(Value::as_object)
        .and_then(|extensions| extensions.get(TASK_EXTENSION_ID))
        .is_some_and(Value::is_object)
}

pub(super) fn task_augmented_tool(params: &Value) -> bool {
    params
        .get("name")
        .and_then(Value::as_str)
        .is_some_and(|name| {
            TASK_AUGMENTED_TOOLS.contains(&name)
                || (name == CONDITIONAL_TASK_TOOL && run_command_task_mode(params))
        })
}

pub(super) fn requires_task_capability(params: &Value) -> bool {
    params.get("name").and_then(Value::as_str) == Some(CONDITIONAL_TASK_TOOL)
        && run_command_task_mode(params)
}

fn run_command_task_mode(params: &Value) -> bool {
    params
        .pointer("/arguments/task_mode")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(super) async fn create_tool_task(
    state: Arc<AppState>,
    params: Value,
    owner: String,
) -> Result<Value, TaskRpcError> {
    Ok(modern_result(
        start_tool_task(state, params, owner).await?.create_result(),
    ))
}

async fn start_tool_task(
    state: Arc<AppState>,
    params: Value,
    owner: String,
) -> Result<TaskRecord, TaskRpcError> {
    start_tool_task_bound(state, params, owner, None).await
}

pub(crate) struct VerificationTaskBinding {
    pub revision: crate::evidence::Revision,
    pub git: Option<crate::verification::change::ExecutionGitBinding>,
}

pub(crate) async fn start_tool_task_bound(
    state: Arc<AppState>,
    params: Value,
    owner: String,
    verification_binding: Option<VerificationTaskBinding>,
) -> Result<TaskRecord, TaskRpcError> {
    let tool_name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| TaskRpcError::invalid("tools/call is missing params.name"))?
        .to_owned();
    if !task_augmented_tool(&params) {
        return Err(TaskRpcError::invalid(
            "tool does not support task augmentation for these arguments",
        ));
    }
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let (workspace_id, workspace) =
        selected_workspace(&state, &args).map_err(TaskRpcError::invalid)?;
    if tool_name == "verify_project" {
        crate::mcp::verification_options(&args).map_err(TaskRpcError::invalid)?;
    }
    if verification_binding.is_some()
        && (tool_name != "verify_project" || !workspace.exec_enabled())
    {
        return Err(TaskRpcError::invalid(
            "bound verification requires enabled execution",
        ));
    }
    let task_owner = owner.clone();
    let mut record = TaskRecord::working(
        owner,
        workspace_id,
        tool_name,
        state.auth.instance_id().to_owned(),
    );
    record.verification_revision = verification_binding
        .as_ref()
        .map(|binding| binding.revision.clone());
    record.verification_git_binding = verification_binding
        .as_ref()
        .and_then(|binding| binding.git.clone());
    let deadline = Instant::now() + Duration::from_millis(record.ttl_ms);
    // Creation and registration are one transition: a concurrent poll must
    // never observe a working record before its worker has been registered.
    let _guard = state
        .tasks
        .state_lock
        .lock()
        .map_err(|_| TaskRpcError::internal("MCP task state lock poisoned"))?;
    task_store::persist(&workspace, &record)
        .map_err(|error| TaskRpcError::internal(error.to_string()))?;

    let task_id = record.task_id.clone();
    let worker_task_id = task_id.clone();
    let worker_state = state.clone();
    let worker_workspace = workspace.clone();
    let (start_tx, start_rx) = oneshot::channel::<()>();
    let join = tokio::spawn(async move {
        if start_rx.await.is_err() {
            worker_state.tasks.remove(&worker_task_id);
            return;
        }
        run_task_worker(
            worker_state,
            worker_workspace,
            worker_task_id,
            params,
            task_owner,
            deadline,
            verification_binding,
        )
        .await;
    });
    state
        .tasks
        .register(task_id, record.workspace.clone(), join.abort_handle());
    let _ = start_tx.send(());
    Ok(record)
}

async fn run_task_worker(
    state: Arc<AppState>,
    workspace: Workspace,
    task_id: String,
    params: Value,
    owner: String,
    deadline: Instant,
    verification_binding: Option<VerificationTaskBinding>,
) {
    // Disconnection does not own this worker. Its deadline and tasks/cancel do.
    // Drop the child set before persistence so queued work is cancelled even
    // when nobody polls or the terminal snapshot cannot be written.
    let outcome = if Instant::now() >= deadline {
        Err(TaskRpcError::internal("task exceeded its durable TTL"))
    } else {
        let progress_enabled = run_command_task_mode(&params);
        let (progress_tx, mut progress_rx) =
            tokio::sync::mpsc::channel(COMMAND_OUTPUT_PROGRESS_CHANNEL_CAPACITY);
        let tool_state = state.clone();
        let monitor_task_id = task_id.clone();
        let bound_workspace = workspace.clone();
        let mut tools = JoinSet::new();
        tools.spawn(async move {
            if let Some(expected) = verification_binding {
                let git = tool_state.harness.execution_git_binding(&bound_workspace).await
                    .map_err(|_| "verification Git inspection failed".to_owned())?;
                let harness = tool_state.harness.clone();
                let current = tokio::task::spawn_blocking(move || harness.current_revision(&bound_workspace))
                    .await.map_err(|_| "verification revision inspection failed".to_owned())?
                    .map_err(|_| "verification revision inspection failed".to_owned())?;
                if current != expected.revision || git != expected.git {
                    return Err("verification snapshot changed before execution; refresh before starting a new task".to_owned());
                }
            }
            let call = MONITOR_DURABLE_TASK_ID.scope(
                monitor_task_id,
                call_tool_owned(&tool_state, params, &owner),
            );
            if progress_enabled {
                crate::workspace::with_command_output_progress(progress_tx, call).await
            } else {
                drop(progress_tx);
                call.await
            }
        });
        let mut stdout = LiveCommandStream::default();
        let mut stderr = LiveCommandStream::default();
        let mut progress_open = progress_enabled;
        let mut progress_dirty = false;
        let mut persist_tick = tokio::time::interval(LIVE_COMMAND_PERSIST_INTERVAL);
        persist_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let _ = persist_tick.tick().await;
        loop {
            tokio::select! {
                joined = tools.join_next() => {
                    while let Ok(chunk) = progress_rx.try_recv() {
                        apply_live_command_chunk(&mut stdout, &mut stderr, chunk);
                        progress_dirty = true;
                    }
                    if progress_dirty {
                        persist_live_command_output(&state, &workspace, &task_id, &stdout, &stderr);
                    }
                    break match joined {
                        Some(Ok(outcome)) => outcome.map_err(TaskRpcError::invalid),
                        Some(Err(error)) => Err(TaskRpcError::internal(format!(
                            "task tool worker failed to join: {error}"
                        ))),
                        None => Err(TaskRpcError::internal("task tool worker did not start")),
                    };
                }
                chunk = progress_rx.recv(), if progress_open => {
                    match chunk {
                        Some(chunk) => {
                            apply_live_command_chunk(&mut stdout, &mut stderr, chunk);
                            progress_dirty = true;
                        }
                        None => {
                            progress_open = false;
                            if progress_dirty {
                                persist_live_command_output(&state, &workspace, &task_id, &stdout, &stderr);
                                progress_dirty = false;
                            }
                        }
                    }
                }
                _ = persist_tick.tick(), if progress_open && progress_dirty => {
                    persist_live_command_output(&state, &workspace, &task_id, &stdout, &stderr);
                    progress_dirty = false;
                }
                _ = tokio::time::sleep_until(deadline) => {
                    if progress_dirty {
                        persist_live_command_output(&state, &workspace, &task_id, &stdout, &stderr);
                    }
                    break Err(TaskRpcError::internal("task exceeded its durable TTL"));
                }
            }
        }
    };
    let Ok(_guard) = state.tasks.state_lock.lock() else {
        state.tasks.remove(&task_id);
        return;
    };
    let Ok(Some(mut current)) = task_store::load(&workspace, &task_id) else {
        state.tasks.remove(&task_id);
        return;
    };
    if current.status != TaskStatus::Working {
        state.tasks.remove(&task_id);
        return;
    }
    match outcome {
        // Completed describes delivery, not a successful tool/check outcome.
        Ok(value) => current.complete(modern_result(value)),
        Err(error) => current.fail(error.code, error.message),
    }
    if let Err(error) = persist_task_result(&workspace, &mut current) {
        tracing::warn!(%task_id, %error, "MCP task final state was not persisted");
    }
    state.tasks.remove(&task_id);
}

#[derive(Default)]
struct LiveCommandStream {
    tail: String,
    pending_line: Vec<u8>,
    dropped_prefix_bytes: usize,
    redacted: bool,
    oversized_line: bool,
    in_private_key: bool,
    capture_truncated: bool,
}

impl LiveCommandStream {
    fn push(&mut self, bytes: &[u8], capture_truncated: bool) {
        self.capture_truncated |= capture_truncated;
        let mut offset = 0usize;
        while offset < bytes.len() {
            if self.oversized_line {
                let Some(relative_end) = bytes[offset..].iter().position(|byte| *byte == b'\n')
                else {
                    return;
                };
                self.append_visible("[wcode: oversized live output line redacted]\n");
                self.redacted = true;
                self.oversized_line = false;
                offset += relative_end + 1;
                continue;
            }

            let newline = bytes[offset..].iter().position(|byte| *byte == b'\n');
            let end = newline.map_or(bytes.len(), |relative| offset + relative);
            let segment = &bytes[offset..end];
            if self.pending_line.len().saturating_add(segment.len()) > MAX_LIVE_COMMAND_LINE_BYTES {
                self.pending_line.clear();
                self.redacted = true;
                self.oversized_line = true;
                if newline.is_some() {
                    self.append_visible("[wcode: oversized live output line redacted]\n");
                    self.oversized_line = false;
                    offset = end + 1;
                    continue;
                }
                return;
            }
            self.pending_line.extend_from_slice(segment);
            if newline.is_some() {
                self.finish_line();
                offset = end + 1;
            } else {
                return;
            }
        }
    }

    fn finish_line(&mut self) {
        let line = String::from_utf8_lossy(&self.pending_line).into_owned();
        self.pending_line.clear();
        let upper = line.to_ascii_uppercase();
        if self.in_private_key {
            self.redacted = true;
            if upper.contains("-----END") && upper.contains("PRIVATE KEY") {
                self.in_private_key = false;
            }
            return;
        }
        if upper.contains("-----BEGIN") && upper.contains("PRIVATE KEY") {
            self.redacted = true;
            self.in_private_key = true;
            self.append_visible("[REDACTED PRIVATE KEY]\n");
            return;
        }
        let (safe, redacted) = crate::workspace::redact_sensitive_text(&line);
        self.redacted |= redacted;
        self.append_visible(&safe);
        self.append_visible("\n");
    }

    fn append_visible(&mut self, text: &str) {
        self.tail.push_str(text);
        if self.tail.len() <= MAX_LIVE_COMMAND_STREAM_BYTES {
            return;
        }
        let mut start = self.tail.len() - MAX_LIVE_COMMAND_STREAM_BYTES;
        while !self.tail.is_char_boundary(start) {
            start += 1;
        }
        self.tail.drain(..start);
        self.dropped_prefix_bytes = self.dropped_prefix_bytes.saturating_add(start);
    }

    fn snapshot(&self) -> (String, usize, bool) {
        let (pending, pending_redacted) = if self.oversized_line {
            (
                "[wcode: oversized live output line redacted]".to_owned(),
                true,
            )
        } else if self.in_private_key {
            (String::new(), true)
        } else {
            let pending = String::from_utf8_lossy(&self.pending_line);
            let upper = pending.to_ascii_uppercase();
            if upper.contains("-----BEGIN") && upper.contains("PRIVATE KEY") {
                ("[REDACTED PRIVATE KEY]".to_owned(), true)
            } else {
                crate::workspace::redact_sensitive_text(&pending)
            }
        };
        let mut visible = self.tail.clone();
        visible.push_str(&pending);
        let mut dropped = self.dropped_prefix_bytes;
        if visible.len() > MAX_LIVE_COMMAND_STREAM_BYTES {
            let mut start = visible.len() - MAX_LIVE_COMMAND_STREAM_BYTES;
            while !visible.is_char_boundary(start) {
                start += 1;
            }
            visible = visible[start..].to_owned();
            dropped = dropped.saturating_add(start);
        }
        (visible, dropped, self.redacted || pending_redacted)
    }
}

fn apply_live_command_chunk(
    stdout: &mut LiveCommandStream,
    stderr: &mut LiveCommandStream,
    chunk: CommandOutputChunk,
) {
    let stream = if chunk.stderr { stderr } else { stdout };
    stream.push(&chunk.bytes, chunk.truncated);
}

fn persist_live_command_output(
    state: &AppState,
    workspace: &Workspace,
    task_id: &str,
    stdout: &LiveCommandStream,
    stderr: &LiveCommandStream,
) {
    let Ok(_guard) = state.tasks.state_lock.lock() else {
        return;
    };
    let Ok(Some(mut current)) = task_store::load(workspace, task_id) else {
        return;
    };
    if current.status != TaskStatus::Working {
        return;
    }
    let (stdout_text, stdout_dropped_prefix_bytes, stdout_redacted) = stdout.snapshot();
    let (stderr_text, stderr_dropped_prefix_bytes, stderr_redacted) = stderr.snapshot();
    current.update_command_output(json!({
        "stdout": stdout_text,
        "stderr": stderr_text,
        "stdoutTruncated": stdout.capture_truncated || stdout_dropped_prefix_bytes > 0,
        "stderrTruncated": stderr.capture_truncated || stderr_dropped_prefix_bytes > 0,
        "stdoutCaptureTruncated": stdout.capture_truncated,
        "stderrCaptureTruncated": stderr.capture_truncated,
        "stdoutDroppedPrefixBytes": stdout_dropped_prefix_bytes,
        "stderrDroppedPrefixBytes": stderr_dropped_prefix_bytes,
        "window": "stream_tail",
        "maxBytesPerStream": MAX_LIVE_COMMAND_STREAM_BYTES,
        "maxPendingLineBytes": MAX_LIVE_COMMAND_LINE_BYTES,
        "redacted": stdout_redacted || stderr_redacted,
    }));
    if let Err(error) = task_store::persist(workspace, &current) {
        tracing::warn!(%task_id, %error, "MCP live command output was not persisted");
    }
}

fn persist_task_result(workspace: &Workspace, record: &mut TaskRecord) -> anyhow::Result<()> {
    if let Err(error) = task_store::persist(workspace, record) {
        tracing::warn!(task_id = %record.task_id, %error, "MCP task result was not persisted");
        record.fail(
            -32603,
            "task result could not be persisted; inspect actual effects before retrying the tool"
                .to_owned(),
        );
        task_store::persist(workspace, record)?;
    }
    Ok(())
}

fn load_owned_task(
    state: &AppState,
    task_id: &str,
    owner: &str,
) -> Result<(String, Workspace, TaskRecord), TaskRpcError> {
    if let Some(workspace_id) = state.tasks.workspace_for(task_id) {
        let (_, workspace) = state
            .workspaces
            .select(Some(&workspace_id))
            .map_err(|error| TaskRpcError::internal(error.to_string()))?;
        if let Some(record) = task_store::load(&workspace, task_id)
            .map_err(|error| TaskRpcError::internal(error.to_string()))?
        {
            if record.owner != owner {
                return Err(TaskRpcError::invalid("unknown taskId"));
            }
            return Ok((workspace_id, workspace, record));
        }
    }
    let found = task_store::find(&state.workspaces, task_id)
        .map_err(|error| TaskRpcError::internal(error.to_string()))?;
    let Some((workspace_id, workspace, record)) = found else {
        return Err(TaskRpcError::invalid("unknown taskId"));
    };
    if record.owner != owner {
        return Err(TaskRpcError::invalid("unknown taskId"));
    }
    Ok((workspace_id, workspace, record))
}

pub(crate) fn web_verification_task(
    state: &AppState,
    workspace_id: &str,
    workspace: &Workspace,
    task_id: &str,
    cancel: bool,
) -> Result<TaskRecord, TaskRpcError> {
    let _guard = state
        .tasks
        .state_lock
        .lock()
        .map_err(|_| TaskRpcError::internal("MCP task state lock poisoned"))?;
    let mut record = task_store::load_for_observation(workspace, task_id)
        .map_err(|_| TaskRpcError::internal("verification task store unavailable"))?
        .ok_or_else(|| TaskRpcError::invalid("unknown verification task"))?;
    if record.task_id != task_id
        || record.workspace != workspace_id
        || record.owner != monitor_ui_owner(state)
        || record.tool_name != "verify_project"
    {
        return Err(TaskRpcError::invalid("unknown verification task"));
    }
    reconcile_task_record(state, workspace, &mut record)?;
    if cancel {
        cancel_task_record(state, workspace, &mut record)?;
    }
    Ok(record)
}

pub(super) fn get_task(
    state: &AppState,
    task_id: &str,
    owner: &str,
) -> Result<Value, TaskRpcError> {
    let _guard = state
        .tasks
        .state_lock
        .lock()
        .map_err(|_| TaskRpcError::internal("MCP task state lock poisoned"))?;
    let (_workspace_id, workspace, mut record) = load_owned_task(state, task_id, owner)?;
    reconcile_task_record(state, &workspace, &mut record)?;
    Ok(modern_result(record.get_result()))
}

fn reconcile_task_record(
    state: &AppState,
    workspace: &Workspace,
    record: &mut TaskRecord,
) -> Result<(), TaskRpcError> {
    let task_id = record.task_id.clone();
    if record.status == TaskStatus::Working
        && record.runtime_instance_id != state.auth.instance_id()
    {
        record.fail(
            -32603,
            "task worker was interrupted by a runtime restart".to_owned(),
        );
        task_store::persist(workspace, record)
            .map_err(|error| TaskRpcError::internal(error.to_string()))?;
        state.tasks.remove(&task_id);
    } else if record.status == TaskStatus::Working && record.expired(task_store_now_ms()) {
        state.tasks.abort(&task_id);
        record.fail(-32603, "task exceeded its durable TTL".to_owned());
        task_store::persist(workspace, record)
            .map_err(|error| TaskRpcError::internal(error.to_string()))?;
    } else if record.status == TaskStatus::Working && !state.tasks.running(&task_id) {
        record.fail(
            -32603,
            "task worker ended without a durable result; inspect actual effects before retrying"
                .to_owned(),
        );
        task_store::persist(workspace, record)
            .map_err(|error| TaskRpcError::internal(error.to_string()))?;
        state.tasks.remove(&task_id);
    }
    if record.status.terminal() {
        state.tasks.remove(&task_id);
    }
    Ok(())
}

pub(super) fn cancel_task(
    state: &AppState,
    task_id: &str,
    owner: &str,
) -> Result<Value, TaskRpcError> {
    let _guard = state
        .tasks
        .state_lock
        .lock()
        .map_err(|_| TaskRpcError::internal("MCP task state lock poisoned"))?;
    let (_workspace_id, workspace, mut record) = load_owned_task(state, task_id, owner)?;
    cancel_task_record(state, &workspace, &mut record)?;
    Ok(modern_result(json!({"resultType":"complete"})))
}

fn cancel_task_record(
    state: &AppState,
    workspace: &Workspace,
    record: &mut TaskRecord,
) -> Result<(), TaskRpcError> {
    // Each caller checks its own authority before entering this shared transition.
    // Once ownership is verified, a disk error must not keep the tool running.
    state.tasks.abort(&record.task_id);
    if !record.status.terminal() {
        record.cancel();
        task_store::persist(workspace, record)
            .map_err(|error| TaskRpcError::internal(error.to_string()))?;
    }
    Ok(())
}

pub(super) fn update_task(
    state: &AppState,
    task_id: &str,
    owner: &str,
) -> Result<Value, TaskRpcError> {
    let _guard = state
        .tasks
        .state_lock
        .lock()
        .map_err(|_| TaskRpcError::internal("MCP task state lock poisoned"))?;
    let _ = load_owned_task(state, task_id, owner)?;
    // wcode's task-augmented tools do not currently issue task inputRequests.
    // Per SEP-2663, responses to non-outstanding keys are ignored and the update is ack-only.
    Ok(modern_result(json!({"resultType":"complete"})))
}

fn task_store_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/mcp/tasks.rs"]
mod tests;
