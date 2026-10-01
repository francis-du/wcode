//! Ordinary tools/call compatibility for the existing durable command and verification Tasks.
//! Transport-derived owner and exact selected Workspace are mandatory. This
//! module creates no registry and grants no command or filesystem authority.
use super::*;
use crate::mcp::command_task_result;
use serde_json::Map;

const COMMAND_FIELDS: &[&str] = &[
    "workspace",
    "program",
    "args",
    "cwd",
    "env",
    "timeout_seconds",
    "task_mode",
];

pub(crate) async fn command_task_tool(
    state: Arc<AppState>,
    args: Value,
    owner: &str,
) -> Result<Value, TaskRpcError> {
    let object = args
        .as_object()
        .ok_or_else(|| TaskRpcError::invalid("command_task arguments must be an object"))?;
    let action = object
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            TaskRpcError::invalid("command_task action must be create, status, result or cancel")
        })?;
    match action {
        "create" => {
            validate_fields(object, COMMAND_FIELDS, &["action"])?;
            let mut command = object.clone();
            command.remove("action");
            validate_command_shape(&command)?;
            command.insert("task_mode".into(), Value::Bool(true));
            create_ordinary_command_task(
                state,
                json!({"name":"run_command","arguments":command}),
                owner,
            )
            .await
        }
        "status" | "result" | "cancel" => {
            validate_fields(object, &["workspace", "task_id"], &["action"])?;
            let id = object
                .get("task_id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty() && id.len() <= 96)
                .ok_or_else(|| {
                    TaskRpcError::invalid("task_id is required and must be a bounded string")
                })?;
            let (workspace_id, workspace) =
                selected_workspace(&state, &args).map_err(TaskRpcError::invalid)?;
            let _guard = state
                .tasks
                .state_lock
                .lock()
                .map_err(|_| TaskRpcError::internal("MCP task state lock poisoned"))?;
            let mut record =
                load_task_record(&workspace, &workspace_id, id, owner, CONDITIONAL_TASK_TOOL)?;
            if action == "cancel" {
                cancel_task_record(&state, &workspace, &mut record)?;
            } else {
                reconcile_task_record(&state, &workspace, &mut record)?;
            }
            Ok(command_task_result(command_projection(
                &record,
                action == "result",
            )))
        }
        _ => Err(TaskRpcError::invalid(
            "command_task action must be create, status, result or cancel",
        )),
    }
}

pub(crate) async fn create_ordinary_command_task(
    state: Arc<AppState>,
    params: Value,
    owner: &str,
) -> Result<Value, TaskRpcError> {
    let args = params
        .get("arguments")
        .and_then(Value::as_object)
        .ok_or_else(|| TaskRpcError::invalid("run_command arguments must be an object"))?;
    validate_fields(args, COMMAND_FIELDS, &[])?;
    validate_command_shape(args)?;
    // Once the worker is committed, return its receipt without any fallible
    // reload/projection step that could lose the task ID and encourage reruns.
    let record = start_tool_task(state, params, owner.to_owned()).await?;
    Ok(command_task_result(command_projection(&record, false)))
}

fn load_task_record(
    workspace: &Workspace,
    workspace_id: &str,
    task_id: &str,
    owner: &str,
    expected_tool: &str,
) -> Result<TaskRecord, TaskRpcError> {
    let record = task_store::load(workspace, task_id)
        .map_err(|_| TaskRpcError::internal("task store is unavailable"))?
        .ok_or_else(|| TaskRpcError::invalid("unknown task_id in selected workspace"))?;
    if record.task_id != task_id
        || record.workspace != workspace_id
        || record.owner != owner
        || record.tool_name != expected_tool
    {
        return Err(TaskRpcError::invalid(
            "unknown task_id in selected workspace",
        ));
    }
    Ok(record)
}

fn validate_fields(
    object: &Map<String, Value>,
    allowed: &[&str],
    extra: &[&str],
) -> Result<(), TaskRpcError> {
    if object
        .keys()
        .any(|key| !allowed.contains(&key.as_str()) && !extra.contains(&key.as_str()))
    {
        return Err(TaskRpcError::invalid(
            "unknown task-control field; owner and actor are chosen by the server",
        ));
    }
    Ok(())
}

fn validate_command_shape(object: &Map<String, Value>) -> Result<(), TaskRpcError> {
    let text = |key: &str, max: usize, required: bool| -> Result<(), TaskRpcError> {
        match object.get(key) {
            None if !required => Ok(()),
            Some(Value::String(value))
                if !value.is_empty() && value.len() <= max && !value.contains('\0') =>
            {
                Ok(())
            }
            _ => Err(TaskRpcError::invalid(format!(
                "{key} must be a nonempty bounded string"
            ))),
        }
    };
    text("program", 1024, true)?;
    text("cwd", 1024, false)?;
    text("workspace", 200, false)?;
    if let Some(args) = object.get("args") {
        let valid = args.as_array().is_some_and(|args| {
            args.len() <= 256
                && args.iter().all(|value| {
                    value
                        .as_str()
                        .is_some_and(|value| value.len() <= 16 * 1024 && !value.contains('\0'))
                })
        });
        if !valid {
            return Err(TaskRpcError::invalid(
                "args must contain at most 256 bounded strings",
            ));
        }
    }
    if let Some(timeout) = object.get("timeout_seconds") {
        if !timeout
            .as_u64()
            .is_some_and(|value| (1..=1800).contains(&value))
        {
            return Err(TaskRpcError::invalid(
                "timeout_seconds must be an integer between 1 and 1800",
            ));
        }
    }
    if let Some(mode) = object.get("task_mode") {
        if mode != &Value::Bool(true) {
            return Err(TaskRpcError::invalid(
                "durable command tasks require task_mode=true",
            ));
        }
    }
    if let Some(env) = object.get("env") {
        let valid = env.as_object().is_some_and(|env| {
            env.len() <= 5
                && env.iter().all(|(key, value)| {
                    ["HOST", "LOG_LEVEL", "NODE_ENV", "PORT", "RUST_LOG"].contains(&key.as_str())
                        && value
                            .as_str()
                            .is_some_and(|value| value.len() <= 4096 && !value.contains('\0'))
                })
        });
        if !valid {
            return Err(TaskRpcError::invalid(
                "env accepts at most five supported bounded string overrides",
            ));
        }
    }
    Ok(())
}

fn command_projection(record: &TaskRecord, include_result: bool) -> Value {
    let verification = record.tool_name == "verify_project";
    let polling_tool = if verification {
        "verify_project"
    } else {
        "command_task"
    };
    let kind = if verification {
        "verification_task"
    } else {
        "command_task"
    };
    let mut value = json!({
        "schema_version":1, "kind":kind, "task_id":record.task_id,
        "workspace":record.workspace, "tool":record.tool_name,
        "status":record.status, "status_message":record.status_message,
        "created_at_ms":record.created_at_ms, "updated_at_ms":record.updated_at_ms,
        "ttl_ms":record.ttl_ms, "poll_interval_ms":record.poll_interval_ms,
        "terminal":record.status.terminal(),
        "result_available":record.status == TaskStatus::Completed && record.result.is_some(),
        "completion_is_not_success":true,
        "next_poll":{"tool":polling_tool,"arguments":{
            "action":"status","task_id":record.task_id,"workspace":record.workspace
        }},
        "result_request":{"tool":polling_tool,"arguments":{
            "action":"result","task_id":record.task_id,"workspace":record.workspace
        }},
        "cancel_request":{"tool":polling_tool,"arguments":{
            "action":"cancel","task_id":record.task_id,"workspace":record.workspace
        }},
    });
    if verification {
        value["current_acceptance"] = Value::Bool(false);
        value["result_scope"] = json!("recorded_execution_only");
    }
    if let Some(output) = &record.live_output {
        value["live_output"] = output.clone();
    }
    if let Some(error) = &record.error {
        value["error"] = error.clone();
    }
    if include_result && record.status == TaskStatus::Completed {
        if let Some(result) = &record.result {
            value["result"] = result.clone();
        }
    }
    value
}

/// Route ordinary Task control before standard Task augmentation. Returning None
/// preserves the existing synchronous/standard-Task behavior for normal verification.
pub(crate) async fn ordinary_task_tool(
    state: Arc<AppState>,
    params: &Value,
    owner: &str,
) -> Option<Result<Value, TaskRpcError>> {
    let name = params.get("name").and_then(Value::as_str)?;
    if !matches!(name, "command_task" | "verify_project") {
        return None;
    }
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if name == "command_task" {
        return Some(command_task_tool(state, args, owner).await);
    }
    match verification_task_action(&args) {
        Err(error) => Some(Err(error)),
        Ok("run") => None,
        Ok(_) => Some(verification_task_tool(state, args, owner).await),
    }
}

/// Validate lifecycle routing before either ordinary or standard Task dispatch.
/// Native worker arguments never contain start/poll fields, avoiding recursive launches.
fn verification_task_action(args: &Value) -> Result<&str, TaskRpcError> {
    let object = args
        .as_object()
        .ok_or_else(|| TaskRpcError::invalid("verification arguments must be an object"))?;
    let action = match object.get("action") {
        None => "run",
        Some(Value::String(action)) => action.as_str(),
        Some(_) => {
            return Err(TaskRpcError::invalid(
                "verification action must be run, start, status, result or cancel",
            ))
        }
    };
    match action {
        "run" | "start" => {
            validate_fields(
                object,
                &["workspace", "level", "fail_fast", "timeout_seconds"],
                &["action"],
            )?;
            let mut options = object.clone();
            options.remove("action");
            crate::mcp::verification_options(&Value::Object(options))
                .map_err(TaskRpcError::invalid)?;
        }
        "status" | "result" | "cancel" => {
            validate_fields(object, &["workspace", "task_id"], &["action"])?;
            if !object
                .get("task_id")
                .and_then(Value::as_str)
                .is_some_and(|id| !id.is_empty() && id.len() <= 96)
            {
                return Err(TaskRpcError::invalid(
                    "task_id is required and must be a bounded string",
                ));
            }
        }
        _ => {
            return Err(TaskRpcError::invalid(
                "verification action must be run, start, status, result or cancel",
            ))
        }
    }
    Ok(action)
}

/// Ordinary clients explicitly start or observe canonical verify_project work.
/// No replacement verifier, evidence import, or new worker registry is introduced.
async fn verification_task_tool(
    state: Arc<AppState>,
    args: Value,
    owner: &str,
) -> Result<Value, TaskRpcError> {
    let action = verification_task_action(&args)?;
    if action == "start" {
        let mut options = args.as_object().expect("validated object").clone();
        options.remove("action");
        let record = start_tool_task(
            state,
            json!({"name":"verify_project","arguments":options}),
            owner.to_owned(),
        )
        .await?;
        return Ok(command_task_result(command_projection(&record, false)));
    }
    if action == "run" {
        return Err(TaskRpcError::invalid(
            "run uses the canonical verification entrypoint",
        ));
    }
    let id = args["task_id"].as_str().expect("validated task ID");
    let (workspace_id, workspace) =
        selected_workspace(&state, &args).map_err(TaskRpcError::invalid)?;
    let _guard = state
        .tasks
        .state_lock
        .lock()
        .map_err(|_| TaskRpcError::internal("MCP task state lock poisoned"))?;
    let mut record = load_task_record(&workspace, &workspace_id, id, owner, "verify_project")?;
    if action == "cancel" {
        cancel_task_record(&state, &workspace, &mut record)?;
    } else {
        reconcile_task_record(&state, &workspace, &mut record)?;
    }
    Ok(command_task_result(command_projection(
        &record,
        action == "result",
    )))
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/mcp/task_tools.rs"]
mod tests;
