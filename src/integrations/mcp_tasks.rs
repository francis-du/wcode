use crate::mcp::{call_tool, jsonrpc_error, modern_result, selected_workspace, AppState};
use crate::task_store::{self, TaskRecord, TaskStatus};
use crate::workspace::Workspace;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;
use tokio::task::AbortHandle;

pub(crate) const TASK_EXTENSION_ID: &str = "io.modelcontextprotocol/tasks";
const TASK_AUGMENTED_TOOLS: &[&str] = &["semantic_provider_refresh", "verification_execute_stages"];

pub(super) fn capabilities() -> Value {
    task_store::capabilities()
}

#[derive(Clone, Default)]
pub(crate) struct TaskRuntime {
    workers: Arc<Mutex<HashMap<String, AbortHandle>>>,
    state_lock: Arc<Mutex<()>>,
}

impl TaskRuntime {
    fn register(&self, task_id: String, handle: AbortHandle) {
        self.workers
            .lock()
            .expect("MCP task worker lock poisoned")
            .insert(task_id, handle);
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
            .is_some_and(|handle| {
                handle.abort();
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
        .is_some_and(|name| TASK_AUGMENTED_TOOLS.contains(&name))
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
    if !TASK_AUGMENTED_TOOLS.contains(&tool_name.as_str()) {
        return Err(TaskRpcError::invalid(
            "tool does not support task augmentation",
        ));
    }
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let (workspace_id, workspace) =
        selected_workspace(&state, &args).map_err(TaskRpcError::invalid)?;
    let record = TaskRecord::working(
        owner,
        workspace_id,
        tool_name,
        state.auth.instance_id().to_owned(),
    );
    {
        let _guard = state
            .tasks
            .state_lock
            .lock()
            .map_err(|_| TaskRpcError::internal("MCP task state lock poisoned"))?;
        task_store::persist(&workspace, &record)
            .map_err(|error| TaskRpcError::internal(error.to_string()))?;
    }

    let task_id = record.task_id.clone();
    let worker_task_id = task_id.clone();
    let worker_state = state.clone();
    let worker_workspace = workspace.clone();
    let (start_tx, start_rx) = oneshot::channel::<()>();
    let join = tokio::spawn(async move {
        let _ = start_rx.await;
        let tool_state = worker_state.clone();
        let outcome = match tokio::spawn(async move { call_tool(&tool_state, params).await }).await
        {
            Ok(outcome) => outcome,
            Err(error) => Err(if error.is_cancelled() {
                "task tool worker was cancelled".to_owned()
            } else if error.is_panic() {
                "task tool worker panicked".to_owned()
            } else {
                format!("task tool worker failed to join: {error}")
            }),
        };
        let Ok(_guard) = worker_state.tasks.state_lock.lock() else {
            worker_state.tasks.remove(&worker_task_id);
            return;
        };
        let Ok(Some(mut current)) = task_store::load(&worker_workspace, &worker_task_id) else {
            worker_state.tasks.remove(&worker_task_id);
            return;
        };
        if current.status != TaskStatus::Working {
            worker_state.tasks.remove(&worker_task_id);
            return;
        }
        match outcome {
            Ok(value) => current.complete(modern_result(value)),
            Err(error) => current.fail(-32602, error),
        }
        let _ = task_store::persist(&worker_workspace, &current);
        worker_state.tasks.remove(&worker_task_id);
    });
    state.tasks.register(task_id, join.abort_handle());
    let _ = start_tx.send(());
    Ok(modern_result(record.create_result()))
}

fn load_owned_task(
    state: &AppState,
    task_id: &str,
    owner: &str,
) -> Result<(String, Workspace, TaskRecord), TaskRpcError> {
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
        record.fail(-32603, "task exceeded its durable TTL".to_owned());
        task_store::persist(&workspace, &record)
            .map_err(|error| TaskRpcError::internal(error.to_string()))?;
        state.tasks.abort(task_id);
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
    if !record.status.terminal() {
        record.cancel();
        task_store::persist(&workspace, &record)
            .map_err(|error| TaskRpcError::internal(error.to_string()))?;
    }
    state.tasks.abort(task_id);
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
