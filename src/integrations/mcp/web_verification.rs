//! Protected WebUI entry points for the existing durable native verification task.
use super::*;
use crate::evidence::Revision;
use crate::task_store::{TaskRecord, TaskStatus};
use axum::extract::Path;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WebVerificationRequest {
    level: String,
    snapshot_revision: String,
    revision: WebRevision,
    timeout_seconds: Option<u64>,
    fail_fast: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WebRevision {
    code: String,
    design: Option<String>,
}

fn complete_revision(revision: &Revision) -> bool {
    let full = |value: &str| {
        value.strip_prefix("sha256:").is_some_and(|hash| {
            hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    };
    full(&revision.code) && revision.design.as_deref().is_none_or(full)
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}

fn failure(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({"error":message,"code":code})),
    )
        .into_response()
}

async fn current_revision(state: &AppState, workspace: &Workspace) -> AnyResult<Revision> {
    let harness = state.harness.clone();
    let workspace = workspace.clone();
    mcp_tools::run_blocking(move || harness.current_revision(&workspace)).await
}

async fn snapshot_key(
    state: &AppState,
    workspace_id: &str,
    workspace: &Workspace,
) -> AnyResult<String> {
    let observation = web_status::revision_state(&state.harness, workspace_id, workspace).await?;
    let workspace = workspace.clone();
    let mut worklist = mcp_tools::run_blocking(move || crate::worklist::status(&workspace)).await?;
    worklist["available"] = json!(true);
    Ok(worklist_snapshot_key(
        &observation.full_snapshot_key,
        &json!({"worklist": worklist}),
    ))
}

pub(crate) async fn intelligence_web_verification_run(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    let (workspace_id, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return no_store(*response),
    };
    if !workspace.exec_enabled() {
        return failure(
            StatusCode::FORBIDDEN,
            "execution_disabled",
            "Verification requires execution permission in the selected workspace",
        );
    }
    let request: WebVerificationRequest = match serde_json::from_value(payload) {
        Ok(request) => request,
        Err(_) => return failure(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Expected level, snapshot_revision, revision and optional timeout_seconds/fail_fast",
        ),
    };
    let expected = Revision {
        code: request.revision.code,
        design: request.revision.design,
    };
    if !complete_revision(&expected)
        || request.snapshot_revision.is_empty()
        || request.snapshot_revision.len() > 16 * 1024
    {
        return failure(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "A complete current revision and bounded snapshot_revision are required",
        );
    }
    let args = json!({"workspace":workspace_id, "level":request.level,
        "timeout_seconds":request.timeout_seconds.unwrap_or(600),
        "fail_fast":request.fail_fast.unwrap_or(true)});
    if verification_options(&args).is_err() {
        return failure(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Use quick/full and a timeout between 1 and 1800 seconds",
        );
    }
    let before = match current_revision(&state, &workspace).await {
        Ok(revision) if complete_revision(&revision) => revision,
        _ => {
            return failure(
                StatusCode::CONFLICT,
                "snapshot_unavailable",
                "A complete current repository revision could not be captured",
            )
        }
    };
    let before_git = match state.harness.execution_git_binding(&workspace).await {
        Ok(binding) => binding,
        Err(_) => {
            return failure(
                StatusCode::CONFLICT,
                "snapshot_unavailable",
                "Current Git identity could not be captured",
            )
        }
    };
    let observed_key = snapshot_key(&state, &workspace_id, &workspace).await;
    let after = current_revision(&state, &workspace).await;
    let after_git = state.harness.execution_git_binding(&workspace).await;
    if before != expected
        || after.ok().as_ref() != Some(&before)
        || after_git.ok().as_ref() != Some(&before_git)
        || observed_key.ok().as_deref() != Some(request.snapshot_revision.as_str())
    {
        return failure(
            StatusCode::CONFLICT,
            "stale_snapshot",
            "The project snapshot changed; refresh before starting verification",
        );
    }
    let owner = crate::mcp_tasks::monitor_ui_owner(&state);
    match crate::mcp_tasks::start_tool_task_bound(
        state.clone(),
        json!({"name":"verify_project","arguments":args}),
        owner,
        Some(crate::mcp_tasks::VerificationTaskBinding {
            revision: before.clone(),
            git: before_git,
        }),
    )
    .await
    {
        Ok(record) => (
            StatusCode::ACCEPTED,
            [(header::CACHE_CONTROL, "no-store")],
            Json(projection(
                &record,
                Some(&before),
                false,
                record.verification_git_binding.as_ref().map(|_| true),
            )),
        )
            .into_response(),
        Err(_) => failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "task_start_failed",
            "Verification task could not be started; inspect actual state before retrying",
        ),
    }
}

pub(crate) async fn intelligence_web_verification_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> Response {
    task_response(state, headers, task_id, false, false).await
}

pub(crate) async fn intelligence_web_verification_result(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> Response {
    task_response(state, headers, task_id, true, false).await
}

pub(crate) async fn intelligence_web_verification_cancel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> Response {
    task_response(state, headers, task_id, false, true).await
}

async fn task_response(
    state: Arc<AppState>,
    headers: HeaderMap,
    task_id: String,
    include_result: bool,
    cancel: bool,
) -> Response {
    let (workspace_id, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return no_store(*response),
    };
    let task_state = state.clone();
    let selected = workspace_id.clone();
    let task_workspace = workspace.clone();
    // The shared task transition locks only bounded store work, never an await.
    let record = mcp_tools::run_blocking(move || {
        crate::mcp_tasks::web_verification_task(
            &task_state,
            &selected,
            &task_workspace,
            &task_id,
            cancel,
        )
        .map_err(|error| anyhow!(error.message().to_owned()))
    })
    .await;
    let record = match record {
        Ok(record) => record,
        Err(error) if error.to_string() == "unknown verification task" => {
            return failure(
                StatusCode::NOT_FOUND,
                "unknown_task",
                "Unknown verification task",
            )
        }
        Err(_) => {
            return failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "task_unavailable",
                "Verification task state is unavailable; do not rerun an unknown task",
            )
        }
    };
    let current = current_revision(&state, &workspace).await.ok();
    let git_current = match (
        &record.verification_git_binding,
        state.harness.execution_git_binding(&workspace).await,
    ) {
        (Some(expected), Ok(Some(observed))) => Some(expected == &observed),
        (Some(_), Ok(None)) => Some(false),
        _ => None,
    };
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(projection(
            &record,
            current.as_ref(),
            include_result,
            git_current,
        )),
    )
        .into_response()
}

fn projection(
    record: &TaskRecord,
    current: Option<&Revision>,
    include_result: bool,
    git_current: Option<bool>,
) -> Value {
    let freshness = match (record.verification_revision.as_ref(), current) {
        (Some(expected), Some(current)) if complete_revision(current) => {
            if expected != current || git_current == Some(false) {
                "stale"
            } else if git_current == Some(true) {
                "current"
            } else {
                "unknown"
            }
        }
        _ => "unknown",
    };
    let result = record.result.as_ref();
    let is_error = record.status == TaskStatus::Failed
        || result.is_some_and(|result| result["isError"].as_bool() != Some(false));
    let report = include_result
        .then(|| {
            result.and_then(|result| {
                result
                    .get("structuredContent")
                    .filter(|report| report["checks"].is_array())
                    .map(report_projection)
            })
        })
        .flatten();
    let mut value = json!({
        "schema_version":1, "kind":"verification_task", "task_id":record.task_id,
        "workspace":record.workspace, "tool":"verify_project", "status":record.status,
        "status_message":safe_text(&record.status_message, 2000),
        "created_at_ms":record.created_at_ms, "updated_at_ms":record.updated_at_ms,
        "ttl_ms":record.ttl_ms, "poll_interval_ms":record.poll_interval_ms,
        "terminal":record.status.terminal(),
        "result_available":record.status == TaskStatus::Completed && record.result.is_some(),
        "completion_is_not_success":true, "requested_revision":record.verification_revision,
        "current_revision":current, "freshness":freshness, "is_error":is_error,
        "git_freshness":match git_current { Some(true)=>"current",Some(false)=>"stale",None=>"unknown" },
        "report":report, "acceptance_ready":false,
        "next_action": if freshness != "current" { "refresh_project" } else if record.status.terminal() {
            "inspect_result_and_refresh_project"
        } else { "poll_status" },
    });
    // Error text and authorization data are allowlisted; no TaskRecord/JSON/context dump.
    let data = result.and_then(|result| result.get("structuredContent"));
    value["authorization_required"] = json!(data.is_some_and(|data| data
        ["authorization_required"]
        .as_bool()
        == Some(true)
        || data["authorizationRequestId"].is_string()));
    value["error"] = record
        .error
        .as_ref()
        .and_then(|error| error["message"].as_str())
        .or_else(|| data.and_then(|data| data["error"].as_str()))
        .map(|text| json!(safe_text(text, 2000)))
        .unwrap_or(Value::Null);
    value
}

fn safe_text(text: &str, max: usize) -> String {
    let safe = crate::workspace::redact_sensitive_text(text).0;
    let mut end = safe.len().min(max);
    while !safe.is_char_boundary(end) {
        end -= 1;
    }
    safe[..end].to_owned()
}

fn report_projection(report: &Value) -> Value {
    let checks = report["checks"]
        .as_array()
        .expect("caller checked report checks");
    let rows = checks.iter().take(64).map(|check| json!({
        "id":check["id"].as_str().map(|v|safe_text(v,160)),
        "phase":check["phase"].as_u64(), "success":check["success"].as_bool(),
        "execution":check["execution"].as_str().filter(|v|matches!(*v,"executed"|"unavailable"|"unknown")),
        "reused":check["reused"].as_bool(), "exit_code":check["exit_code"].as_i64(),
        "elapsed_ms":check["elapsed_ms"].as_u64(),
        "command":check["command"].as_str().map(|v|safe_text(v,1024)),
        "stdout_tail":check["stdout_tail"].as_str().map(|v|safe_text(v,1024)),
        "stderr_tail":check["stderr_tail"].as_str().map(|v|safe_text(v,1024)),
        "output_truncated":check["output_truncated"].as_bool() == Some(true)
            || check["stdout_tail"].as_str().is_some_and(|v|v.len()>1024)
            || check["stderr_tail"].as_str().is_some_and(|v|v.len()>1024),
        "evidence_id":check["evidence_id"].as_str().map(|v|safe_text(v,160)),
    })).collect::<Vec<_>>();
    json!({"workspace":report["workspace"].as_str(), "level":report["level"].as_str(),
        "passed":report["passed"].as_bool(), "checks_run":report["checks_run"].as_u64(),
        "checks_failed":report["checks_failed"].as_u64(), "checks_reused":report["checks_reused"].as_u64(),
        "elapsed_ms":report["elapsed_ms"].as_u64(),
        "summary":report["summary"].as_str().map(|v|safe_text(v,2000)),
        "skipped_checks":report["skipped_checks"].as_array().map(|items|items.iter().take(64)
            .filter_map(Value::as_str).map(|v|safe_text(v,160)).collect::<Vec<_>>()),
        "checks":rows, "checks_total":checks.len(), "checks_truncated":checks.len()>64})
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/mcp/web_verification.rs"]
mod tests;
