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
    let task_owner = owner.clone();
    let record = TaskRecord::working(
        owner,
        workspace_id,
        tool_name,
        state.auth.instance_id().to_owned(),
    );
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
        )
        .await;
    });
    state
        .tasks
        .register(task_id, record.workspace.clone(), join.abort_handle());
    let _ = start_tx.send(());
    Ok(modern_result(record.create_result()))
}

async fn run_task_worker(
    state: Arc<AppState>,
    workspace: Workspace,
    task_id: String,
    params: Value,
    owner: String,
    deadline: Instant,
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
        let mut tools = JoinSet::new();
        tools.spawn(async move {
            let call = call_tool_owned(&tool_state, params, &owner);
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
    if record.status == TaskStatus::Working
        && record.runtime_instance_id != state.auth.instance_id()
    {
        record.fail(
            -32603,
            "task worker was interrupted by a runtime restart".to_owned(),
        );
        task_store::persist(&workspace, &record)
            .map_err(|error| TaskRpcError::internal(error.to_string()))?;
        state.tasks.remove(task_id);
    } else if record.status == TaskStatus::Working && record.expired(task_store_now_ms()) {
        state.tasks.abort(task_id);
        record.fail(-32603, "task exceeded its durable TTL".to_owned());
        task_store::persist(&workspace, &record)
            .map_err(|error| TaskRpcError::internal(error.to_string()))?;
    } else if record.status == TaskStatus::Working && !state.tasks.running(task_id) {
        record.fail(
            -32603,
            "task worker ended without a durable result; inspect actual effects before retrying"
                .to_owned(),
        );
        task_store::persist(&workspace, &record)
            .map_err(|error| TaskRpcError::internal(error.to_string()))?;
        state.tasks.remove(task_id);
    }
    if record.status.terminal() {
        state.tasks.remove(task_id);
    }
    Ok(modern_result(record.get_result()))
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
    // Once ownership is verified, a disk error must not keep the tool running.
    state.tasks.abort(task_id);
    if !record.status.terminal() {
        record.cancel();
        task_store::persist(&workspace, &record)
            .map_err(|error| TaskRpcError::internal(error.to_string()))?;
    }
    Ok(modern_result(json!({"resultType":"complete"})))
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
