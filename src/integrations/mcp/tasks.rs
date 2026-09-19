use crate::mcp::{call_tool_owned, jsonrpc_error, modern_result, selected_workspace, AppState};
use crate::task_store::{self, TaskRecord, TaskStatus};
use crate::workspace::Workspace;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;
use tokio::task::{AbortHandle, JoinSet};
use tokio::time::{timeout_at, Instant};

pub(crate) const TASK_EXTENSION_ID: &str = "io.modelcontextprotocol/tasks";
const TASK_AUGMENTED_TOOLS: &[&str] = &[
    "semantic_provider_install",
    "semantic_provider_refresh",
    "verification_execute_stages",
    "verify_project",
];

pub(super) fn capabilities() -> Value {
    let mut capabilities = task_store::capabilities();
    capabilities["task_augmented_tools"] = json!(TASK_AUGMENTED_TOOLS);
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
        let tool_state = state.clone();
        let mut tools = JoinSet::new();
        tools.spawn(async move { call_tool_owned(&tool_state, params, &owner).await });
        match timeout_at(deadline, tools.join_next()).await {
            Ok(Some(Ok(outcome))) => outcome.map_err(TaskRpcError::invalid),
            Ok(Some(Err(error))) => Err(TaskRpcError::internal(format!(
                "task tool worker failed to join: {error}"
            ))),
            Ok(None) => Err(TaskRpcError::internal("task tool worker did not start")),
            Err(_) => Err(TaskRpcError::internal("task exceeded its durable TTL")),
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
