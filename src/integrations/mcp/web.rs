use super::*;
use crate::authorization::AuthorizationStatus;
#[path = "web_change_impact.rs"]
pub(crate) mod web_change_impact;
#[path = "web_execution.rs"]
mod web_execution;
#[path = "web_graph.rs"]
mod web_graph;
#[path = "web_historical.rs"]
mod web_historical;
pub(super) use web_execution::intelligence_web_revision;
#[path = "web_authorizations.rs"]
mod authorizations;
pub(super) use authorizations::intelligence_web_approve_authorization_group;

use web_execution::{
    intelligence_execution_snapshot, public_worklist_revision, worklist_snapshot_key,
};
#[path = "web_jobs.rs"]
mod web_jobs;
#[path = "web_refresh.rs"]
mod web_refresh;
pub(super) use web_refresh::{
    intelligence_web_graph, intelligence_web_project, intelligence_web_refresh_semantics,
    intelligence_web_scopes, intelligence_web_status,
};
#[path = "web_status.rs"]
pub(super) mod web_status;
pub(super) use web_jobs::{
    intelligence_web_job, intelligence_web_job_cancel, intelligence_web_jobs,
    intelligence_web_verification_tasks,
};
#[path = "web_verification.rs"]
mod web_verification;
pub(super) use web_change_impact::intelligence_web_change_impact;
#[cfg(test)]
pub(super) use web_change_impact::IntelligenceChangeImpactQuery;
#[cfg(test)]
pub(super) use web_graph::{
    changed_symbol_impact, IntelligenceChangeQuery, IntelligenceCodeGraphQuery,
    IntelligenceCodeSourceQuery,
};
pub(super) use web_graph::{
    intelligence_web_change_detail, intelligence_web_code_graph, intelligence_web_code_source,
    intelligence_web_edit_source,
};
pub(super) use web_historical::{intelligence_web_historical_source, intelligence_web_revisions};
use web_verification::no_store;
pub(super) use web_verification::{
    intelligence_web_verification_cancel, intelligence_web_verification_result,
    intelligence_web_verification_run, intelligence_web_verification_status,
};

pub(super) async fn setup_page(
    State(state): State<Arc<AppState>>,
    _headers: HeaderMap,
) -> Response {
    let capabilities = state.workspaces.capabilities();
    let roots = capabilities["workspaces"].as_array().map_or(0, Vec::len);
    let workspace = capabilities["default_workspace"]
        .as_str()
        .unwrap_or("unknown");
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let page = crate::setup_web::render(workspace, roots, state.harness.max_parallel(), &nonce);
    let mut response = (
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::REFERRER_POLICY, "no-referrer"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        axum::response::Html(page),
    )
        .into_response();
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        format!("default-src 'none'; script-src 'nonce-{nonce}'; style-src 'nonce-{nonce}'; connect-src 'self'; img-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'")
            .parse().expect("UUID nonce produces a valid CSP header"),
    );
    response
}

// The public connection guide needs no command catalog, paths, credentials or
// full Harness serialization on every poll. Detailed diagnostics require UI authorization.
pub(super) async fn setup_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let connection = state.monitor.connection_status();
    let public_url = state
        .auth
        .request_public_url(&headers)
        .unwrap_or_else(|| state.auth.public_url());
    let tunnels = connection
        .tunnels
        .iter()
        .filter_map(|tunnel| {
            let url = tunnel.url.as_ref()?;
            Some(json!({
                "provider": tunnel.provider,
                "mcp_url": format!("{url}/mcp"),
                "role": tunnel.role,
                "state": tunnel.state,
                "lease_age_seconds": tunnel.lease_age_seconds,
            }))
        })
        .collect::<Vec<_>>();
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "ok": true,
            "mcp_url": format!("{public_url}/mcp"),
            "public_endpoint": connection.public_endpoint,
            "public_url_healthy": connection.public_url_healthy,
            "mcp_initialized": connection.chatgpt_initialized,
            "mcp_last_seen_seconds_ago": connection.last_mcp_seen_seconds_ago,
            "tunnels": tunnels,
        })),
    )
        .into_response()
}

pub(super) async fn intelligence_page() -> Response {
    (
        [
            (header::CACHE_CONTROL, "no-store"),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
            ),
            (header::REFERRER_POLICY, "no-referrer"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        axum::response::Html(crate::intelligence_web::INTELLIGENCE_APP_PAGE),
    )
        .into_response()
}

pub(super) async fn intelligence_styles() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        crate::intelligence_web::INTELLIGENCE_CSS,
    )
        .into_response()
}

pub(super) async fn intelligence_script() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        crate::intelligence_web::INTELLIGENCE_JS,
    )
        .into_response()
}

pub(super) async fn intelligence_logo() -> Response {
    (
        [
            (header::CONTENT_TYPE, "image/svg+xml; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        crate::intelligence_web::INTELLIGENCE_LOGO_SVG,
    )
        .into_response()
}

pub(super) fn intelligence_ui_authorized(
    state: &AppState,
    headers: &HeaderMap,
) -> std::result::Result<(), Box<Response>> {
    if state.auth.request_public_url(headers).is_none() {
        return Err(Box::new(forbidden_host_response()));
    }
    if !origin_allowed(&state.auth, headers) {
        return Err(Box::new(forbidden_origin_response()));
    }
    if !state.auth.ui_authorized(headers) {
        return Err(Box::new(
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error":"local intelligence UI authorization required"})),
            )
                .into_response(),
        ));
    }
    Ok(())
}

pub(super) fn requested_intelligence_workspace(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("x-wcode-workspace")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
}

pub(super) fn intelligence_ui_workspace(
    state: &AppState,
    headers: &HeaderMap,
) -> std::result::Result<(String, Workspace), Box<Response>> {
    intelligence_ui_authorized(state, headers)?;
    state
        .workspaces
        .select(requested_intelligence_workspace(headers))
        .map_err(|error| {
            Box::new(
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": error.to_string()})),
                )
                    .into_response(),
            )
        })
}

pub(super) fn intelligence_workspace_options(state: &AppState) -> Value {
    json!(state
        .workspaces
        .roots()
        .into_iter()
        .map(|(id, root)| json!({"id":id,"root":root}))
        .collect::<Vec<_>>())
}

pub(super) fn intelligence_bad_request(error: impl ToString) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error": error.to_string()})),
    )
        .into_response()
}

pub(super) async fn intelligence_web_workspaces(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = intelligence_ui_authorized(&state, &headers) {
        return *response;
    }
    let selected = requested_intelligence_workspace(&headers);
    match state.workspaces.workspace_access(selected) {
        Ok(workspace) => (
            StatusCode::OK,
            Json(json!({
                "workspace": workspace,
                "workspace_options": intelligence_workspace_options(&state),
            })),
        )
            .into_response(),
        Err(error) => intelligence_bad_request(error),
    }
}

pub(super) async fn intelligence_web_add_workspace(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    if let Err(response) = intelligence_ui_authorized(&state, &headers) {
        return *response;
    }
    let Some(root) = payload.get("root").and_then(Value::as_str) else {
        return intelligence_bad_request("workspace root is required");
    };
    match state
        .workspaces
        .add_workspace_from(requested_intelligence_workspace(&headers), root)
    {
        Ok((id, _)) => {
            state.monitor.register_workspace(id.clone());
            match state.workspaces.workspace_access(Some(&id)) {
                Ok(workspace) => (
                    StatusCode::OK,
                    Json(json!({
                        "workspace": workspace,
                        "workspace_options": intelligence_workspace_options(&state),
                    })),
                )
                    .into_response(),
                Err(error) => intelligence_bad_request(error),
            }
        }
        Err(error) => intelligence_bad_request(error),
    }
}

pub(super) async fn intelligence_web_commands(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = intelligence_ui_authorized(&state, &headers) {
        return *response;
    }
    match state
        .workspaces
        .workspace_access(requested_intelligence_workspace(&headers))
    {
        Ok(workspace) => (StatusCode::OK, Json(workspace)).into_response(),
        Err(error) => intelligence_bad_request(error),
    }
}

pub(super) async fn intelligence_web_allow_command(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    if let Err(response) = intelligence_ui_authorized(&state, &headers) {
        return *response;
    }
    let Some(program) = payload.get("program").and_then(Value::as_str) else {
        return intelligence_bad_request("program is required");
    };
    match state
        .workspaces
        .allow_command(requested_intelligence_workspace(&headers), program)
    {
        Ok(workspace) => (StatusCode::OK, Json(workspace)).into_response(),
        Err(error) => intelligence_bad_request(error),
    }
}

pub(super) async fn intelligence_web_enable_all_commands(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = intelligence_ui_authorized(&state, &headers) {
        return *response;
    }
    match state
        .workspaces
        .set_all_commands_authorized(requested_intelligence_workspace(&headers), true)
    {
        Ok(workspace) => (StatusCode::OK, Json(workspace)).into_response(),
        Err(error) => intelligence_bad_request(error),
    }
}

pub(super) async fn intelligence_web_disable_all_commands(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = intelligence_ui_authorized(&state, &headers) {
        return *response;
    }
    match state
        .workspaces
        .set_all_commands_authorized(requested_intelligence_workspace(&headers), false)
    {
        Ok(workspace) => (StatusCode::OK, Json(workspace)).into_response(),
        Err(error) => intelligence_bad_request(error),
    }
}

pub(super) async fn intelligence_web_revoke_command(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    if let Err(response) = intelligence_ui_authorized(&state, &headers) {
        return *response;
    }
    let Some(program) = payload.get("program").and_then(Value::as_str) else {
        return intelligence_bad_request("program is required");
    };
    match state
        .workspaces
        .revoke_command(requested_intelligence_workspace(&headers), program)
    {
        Ok(workspace) => (StatusCode::OK, Json(workspace)).into_response(),
        Err(error) => intelligence_bad_request(error),
    }
}

pub(super) fn intelligence_workspace_id(
    state: &AppState,
    headers: &HeaderMap,
) -> AnyResult<String> {
    state
        .workspaces
        .select(requested_intelligence_workspace(headers))
        .map(|(id, _)| id)
}

pub(super) fn intelligence_pending_authorizations(state: &AppState, workspace_id: &str) -> Value {
    json!(state
        .workspaces
        .authorization_requests(256)
        .into_iter()
        .filter(|request| {
            request.status == AuthorizationStatus::Pending && request.workspace == workspace_id
        })
        .collect::<Vec<_>>())
}

pub(super) async fn intelligence_web_authorizations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = intelligence_ui_authorized(&state, &headers) {
        return *response;
    }
    let workspace_id = match intelligence_workspace_id(&state, &headers) {
        Ok(id) => id,
        Err(error) => return intelligence_bad_request(error),
    };
    (
        StatusCode::OK,
        Json(json!({"pending": intelligence_pending_authorizations(&state, &workspace_id)})),
    )
        .into_response()
}

const MAX_GROUPED_AUTHORIZATIONS: usize = 64;

pub(super) async fn intelligence_web_approve_authorization(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    if let Err(response) = intelligence_ui_authorized(&state, &headers) {
        return *response;
    }
    let Some(id) = payload.get("id").and_then(Value::as_str) else {
        return intelligence_bad_request("authorization id is required");
    };
    let workspace_id = match intelligence_workspace_id(&state, &headers) {
        Ok(id) => id,
        Err(error) => return intelligence_bad_request(error),
    };
    if state
        .workspaces
        .authorization_request(id)
        .is_none_or(|request| request.workspace != workspace_id)
    {
        return intelligence_bad_request(
            "authorization request does not belong to the selected workspace",
        );
    }
    match state.workspaces.approve_authorization_session_result(id) {
        Ok(request) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "request": request,
                "pending": intelligence_pending_authorizations(&state, &workspace_id)
            })),
        )
            .into_response(),
        Err(error) => intelligence_bad_request(error),
    }
}

pub(super) async fn intelligence_web_authorize_command_operation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    if let Err(response) = intelligence_ui_authorized(&state, &headers) {
        return *response;
    }
    let Some(program) = payload.get("program").and_then(Value::as_str) else {
        return intelligence_bad_request("program is required");
    };
    let args = match payload.get("args") {
        None => Vec::new(),
        Some(Value::Array(values)) => {
            let mut args = Vec::with_capacity(values.len());
            for value in values {
                let Some(value) = value.as_str() else {
                    return intelligence_bad_request("command args must be strings");
                };
                args.push(value.to_owned());
            }
            args
        }
        Some(_) => return intelligence_bad_request("command args must be an array"),
    };
    let cwd = payload.get("cwd").and_then(Value::as_str).unwrap_or(".");
    let workspace_id = match intelligence_workspace_id(&state, &headers) {
        Ok(id) => id,
        Err(error) => return intelligence_bad_request(error),
    };
    match state
        .workspaces
        .authorize_command_operation(Some(&workspace_id), program, &args, cwd)
    {
        Ok(request) => match state
            .workspaces
            .workspace_access(requested_intelligence_workspace(&headers))
        {
            Ok(workspace) => (
                StatusCode::OK,
                Json(json!({
                    "ok": true,
                    "request": request,
                    "workspace": workspace,
                    "pending": intelligence_pending_authorizations(&state, &workspace_id)
                })),
            )
                .into_response(),
            Err(error) => intelligence_bad_request(error),
        },
        Err(error) => intelligence_bad_request(error),
    }
}

pub(super) async fn intelligence_web_deny_authorization(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    if let Err(response) = intelligence_ui_authorized(&state, &headers) {
        return *response;
    }
    let Some(id) = payload.get("id").and_then(Value::as_str) else {
        return intelligence_bad_request("authorization id is required");
    };
    let workspace_id = match intelligence_workspace_id(&state, &headers) {
        Ok(id) => id,
        Err(error) => return intelligence_bad_request(error),
    };
    if state
        .workspaces
        .authorization_request(id)
        .is_none_or(|request| request.workspace != workspace_id)
    {
        return intelligence_bad_request(
            "authorization request does not belong to the selected workspace",
        );
    }
    if !state.workspaces.deny_authorization(id) {
        return intelligence_bad_request("authorization request is missing or no longer pending");
    }
    (
        StatusCode::OK,
        Json(
            json!({"ok":true,"pending":intelligence_pending_authorizations(&state, &workspace_id)}),
        ),
    )
        .into_response()
}

pub(super) async fn intelligence_web_activity(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let (workspace_id, _) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return *response,
    };
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "workspace": workspace_id,
            "activity": state.monitor.observatory_activity(&workspace_id),
            "resources": crate::resource::capabilities(),
            "harness": state.harness.capabilities(),
            "resource_scope": "whole_process",
            "pending_authorizations": intelligence_pending_authorizations(&state, &workspace_id).as_array().map_or(0, Vec::len),
        })),
    ).into_response()
}
