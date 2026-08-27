use crate::auth::AuthState;
use crate::authorization::AuthorizationStatus;
use crate::graph::GraphProviderImport;
use crate::graph_store::{GraphDiffInput, GraphQueryInput};
use crate::harness::ToolHarness;
use crate::mcp_catalog;
pub(crate) use crate::mcp_tasks::TaskRuntime;
#[cfg(test)]
use crate::mcp_tasks::TASK_EXTENSION_ID;
use crate::mcp_tasks::{
    cancel_task, capabilities as task_capabilities, client_supports_tasks, create_tool_task,
    get_task, task_augmented_tool, task_rpc_error, update_task, TaskRpcError,
};
use crate::monitor::TaskMonitor;
use crate::reconcile::{ReconciliationTaskKind, ReconciliationTaskSubmission};
use crate::scheduler;
use crate::semantic::SemanticCandidateInput;
use crate::verification::{ReviewSubmission, ReviewerRole, StageSubmission};
use crate::workspace::{
    CreateFileRequest, FileEditRequest, MovePathRequest, TextEdit, Workspace, Workspaces,
};
use crate::{
    AUTHOR_HANDLE, AUTHOR_URL, CHATGPT_CONNECTOR_SETUP_URL, CLAUDE_CONNECTOR_SETUP_URL, DOCS_URL,
    GROK_CONNECTOR_SETUP_URL, MISTRAL_CONNECTOR_SETUP_URL, PROJECT_URL,
};
use anyhow::{anyhow, Result as AnyResult};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Arc;
use tokio::task::{JoinError, JoinHandle, JoinSet};

pub(crate) const MODERN_PROTOCOL_VERSION: &str = "2026-07-28";
pub(crate) const LEGACY_PROTOCOL_VERSIONS: &[&str] =
    &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &[
    MODERN_PROTOCOL_VERSION,
    "2025-11-25",
    "2025-06-18",
    "2025-03-26",
    "2024-11-05",
];
const DISCOVER_PROTOCOL_VERSIONS: &[&str] = &[MODERN_PROTOCOL_VERSION];
const MAX_BATCH_ITEMS: usize = 128;
const MAX_PARALLEL_FANOUT_ITEMS: usize = 128;
const MAX_PARALLEL_FANOUT_ITEM_BYTES: usize = 512 * 1024;
const MAX_PARALLEL_FANOUT_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MEDIA_CONTENT_EXTENSION_ID: &str = "run.francis.wcode/media-content";
const PARALLEL_READ_TOOLS: &[&str] = &[
    "workspace_info",
    "design_status",
    "convention_status",
    "scope_status",
    "software_graph",
    "graph_provider_status",
    "semantic_provider_status",
    "language_quality_status",
    "graph_history",
    "graph_query",
    "graph_diff",
    "traceability_status",
    "software_context",
    "agent_context",
    "semantic_status",
    "semantic_query",
    "evidence_status",
    "verification_status",
    "verification_history",
    "verification_executor_status",
    "reconciliation_status",
    "reconciliation_history",
    "reconciliation_execution_status",
    "project_context",
    "list_files",
    "search_code",
    "search_many",
    "file_outline",
    "find_symbol",
    "symbol_context",
    "read_file",
    "read_files",
    "path_info",
];
const PARALLEL_WRITE_TOOLS: &[&str] = &[
    "replace_text",
    "apply_edits",
    "write_file",
    "create_directory",
    "create_file",
    "create_files",
    "apply_file_edits",
    "move_path",
    "move_paths",
    "delete_path",
];

#[derive(Clone)]
pub struct AppState {
    pub auth: Arc<AuthState>,
    pub workspaces: Workspaces,
    pub harness: ToolHarness,
    pub monitor: TaskMonitor,
    pub(crate) tasks: TaskRuntime,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(setup_page))
        .route("/healthz", get(health))
        .route("/intelligence", get(intelligence_page))
        .route("/intelligence/app.css", get(intelligence_styles))
        .route("/intelligence/app.js", get(intelligence_script))
        .route("/intelligence/project", get(intelligence_web_project))
        .route("/intelligence/revision", get(intelligence_web_revision))
        .route(
            "/intelligence/semantic-refresh",
            post(intelligence_web_refresh_semantics),
        )
        .route(
            "/intelligence/workspaces",
            get(intelligence_web_workspaces).post(intelligence_web_add_workspace),
        )
        .route(
            "/intelligence/commands",
            get(intelligence_web_commands)
                .post(intelligence_web_allow_command)
                .delete(intelligence_web_revoke_command),
        )
        .route(
            "/intelligence/authorizations",
            get(intelligence_web_authorizations)
                .post(intelligence_web_approve_authorization)
                .delete(intelligence_web_deny_authorization),
        )
        .route(
            "/intelligence/command-operations",
            post(intelligence_web_authorize_command_operation),
        )
        .route("/intelligence/status", get(intelligence_web_status))
        .route("/intelligence/scopes", get(intelligence_web_scopes))
        .route("/intelligence/graph", get(intelligence_web_graph))
        .route("/mcp", get(mcp_get).post(mcp))
        .with_state(state)
}

async fn mcp_get(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if !origin_allowed(&state.auth.public_url(), &headers) {
        return forbidden_origin_response();
    }
    if !state.auth.authorized(&headers) {
        return state.auth.unauthorized_response();
    }
    state.monitor.mark_mcp_seen();
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [("allow", "POST")],
        "wcode does not expose an MCP SSE listening stream; use Streamable HTTP POST",
    )
        .into_response()
}

async fn setup_page(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let capabilities = state.workspaces.capabilities();
    let workspace_count = capabilities["workspaces"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default();
    let default_workspace = capabilities["default_workspace"]
        .as_str()
        .unwrap_or("unknown");
    let base = state.auth.public_url();
    let mcp_url = format!("{base}/mcp");
    axum::response::Html(format!(
        r##"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="dark"><title>wcode · Software Intelligence Runtime</title>
<style>*{{box-sizing:border-box}}body{{margin:0;min-height:100vh;display:grid;place-items:center;background:radial-gradient(800px 450px at 50% -10%,#24242b,#09090b 65%);color:#f4f4f5;font:14px/1.55 ui-sans-serif,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;padding:24px}}main{{width:min(100%,720px)}}.brand{{display:flex;align-items:center;gap:11px;margin:0 0 18px 4px}}.logo{{width:34px;height:34px;border:1px solid #3a3a42;border-radius:10px;display:grid;place-items:center;background:#151518;font:700 14px ui-monospace,monospace}}.muted{{color:#8d8d98}}.card{{border:1px solid #29292f;border-radius:18px;background:linear-gradient(180deg,#151519,#101013);padding:26px;box-shadow:0 28px 80px #0008}}h1{{margin:0 0 6px;font-size:23px}}.status{{display:inline-flex;align-items:center;gap:7px;color:#a7f3bd;font-size:12px;margin-bottom:22px}}.dot{{width:7px;height:7px;background:#5ee28a;border-radius:50%;box-shadow:0 0 12px #5ee28a88}}.endpoint{{display:flex;align-items:center;justify-content:space-between;gap:15px;padding:13px 15px;border:1px solid #29292f;border-radius:12px;background:#09090b;font:12px ui-monospace,SFMono-Regular,Menlo,monospace;overflow:auto}}.grid{{display:grid;grid-template-columns:repeat(4,1fr);gap:10px;margin-top:12px}}.stat{{padding:13px;border:1px solid #28282e;border-radius:12px;background:#111114}}.stat b{{display:block;font-size:18px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}}.stat span{{font-size:11px;color:#84848f}}.clients{{display:grid;grid-template-columns:repeat(5,1fr);gap:8px;margin-top:18px}}.client{{display:flex;align-items:center;justify-content:center;min-height:44px;padding:0 10px;border:1px solid #323239;border-radius:11px;background:#0c0c0f;color:#f4f4f5;font-weight:650;font-size:12px;text-decoration:none}}.client:hover{{border-color:#696973;background:#17171b}}.hint{{margin-top:12px;color:#73737d;font-size:11px}}footer{{display:flex;justify-content:space-between;gap:14px;flex-wrap:wrap;margin-top:15px;padding:0 4px;color:#72727d;font-size:12px}}a{{color:#b8b8c0;text-decoration:none}}a:hover{{color:#fff}}@media(max-width:720px){{.grid{{grid-template-columns:repeat(2,1fr)}}.clients{{grid-template-columns:repeat(2,1fr)}}}}@media(max-width:520px){{.grid{{grid-template-columns:1fr}}.clients{{grid-template-columns:1fr}}}}</style></head>
<body><main><div class="brand"><div class="logo">WC</div><div><strong>wcode</strong><div class="muted">Software Intelligence Runtime</div></div></div><section class="card"><div class="status"><i class="dot"></i>Runtime ready</div><h1>Connect a model executor</h1><p class="muted">wcode owns the local Design State, software context, risk, verification, and evidence layer. Connect any supported MCP model or agent below as a replaceable executor.</p><div style="height:20px"></div><div class="endpoint">{mcp_url}</div><div class="clients"><a class="client" href="{grok_url}" target="_blank" rel="noreferrer">Grok ↗</a><a class="client" href="{claude_url}" target="_blank" rel="noreferrer">Claude ↗</a><a class="client" href="{chatgpt_url}" target="_blank" rel="noreferrer">ChatGPT ↗</a><a class="client" href="{mistral_url}" target="_blank" rel="noreferrer">Mistral ↗</a><a class="client" href="{docs_url}#clients" target="_blank" rel="noreferrer">Other MCP ↗</a></div><div class="hint">MCP is the model access layer. Runtime state and software intelligence remain provider-neutral inside wcode.</div><div class="grid"><div class="stat"><b>{workspace_count}</b><span>workspace roots</span></div><div class="stat"><b>{}</b><span>parallel slots</span></div><div class="stat"><b>{intelligence_capability_count}</b><span>intelligence capabilities</span></div><div class="stat"><b>{default_workspace}</b><span>default workspace</span></div></div></section><footer><a href="{docs_url}" target="_blank" rel="noreferrer">Docs ↗</a><a href="{project_url}" target="_blank" rel="noreferrer">Project ↗</a><a href="{author_url}" target="_blank" rel="noreferrer">{author_handle} ↗</a></footer></main></body></html>"##,
        state.harness.max_parallel(),
        intelligence_capability_count = state.harness.intelligence_capability_count(),
        chatgpt_url = CHATGPT_CONNECTOR_SETUP_URL,
        grok_url = GROK_CONNECTOR_SETUP_URL,
        claude_url = CLAUDE_CONNECTOR_SETUP_URL,
        mistral_url = MISTRAL_CONNECTOR_SETUP_URL,
        docs_url = DOCS_URL,
        project_url = PROJECT_URL,
        author_url = AUTHOR_URL,
        author_handle = AUTHOR_HANDLE,
    ))
}

async fn intelligence_page() -> Response {
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

async fn intelligence_styles() -> Response {
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

async fn intelligence_script() -> Response {
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

fn intelligence_ui_authorized(
    state: &AppState,
    headers: &HeaderMap,
) -> std::result::Result<(), Box<Response>> {
    if !origin_allowed(&state.auth.public_url(), headers) {
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

fn requested_intelligence_workspace(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("x-wcode-workspace")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
}

fn intelligence_ui_workspace(
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

fn intelligence_workspace_options(state: &AppState) -> Value {
    json!(state
        .workspaces
        .roots()
        .into_iter()
        .map(|(id, root)| json!({"id":id,"root":root}))
        .collect::<Vec<_>>())
}

fn intelligence_bad_request(error: impl ToString) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error": error.to_string()})),
    )
        .into_response()
}

async fn intelligence_web_workspaces(
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

async fn intelligence_web_add_workspace(
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
    match state.workspaces.add_workspace(root.trim()) {
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

async fn intelligence_web_commands(
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

async fn intelligence_web_allow_command(
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

async fn intelligence_web_revoke_command(
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

fn intelligence_pending_authorizations(state: &AppState) -> Value {
    json!(state
        .workspaces
        .authorization_requests(256)
        .into_iter()
        .filter(|request| request.status == AuthorizationStatus::Pending)
        .collect::<Vec<_>>())
}

async fn intelligence_web_authorizations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = intelligence_ui_authorized(&state, &headers) {
        return *response;
    }
    (
        StatusCode::OK,
        Json(json!({"pending": intelligence_pending_authorizations(&state)})),
    )
        .into_response()
}

async fn intelligence_web_approve_authorization(
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
    match state.workspaces.approve_authorization_session_result(id) {
        Ok(request) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "request": request,
                "pending": intelligence_pending_authorizations(&state)
            })),
        )
            .into_response(),
        Err(error) => intelligence_bad_request(error),
    }
}

async fn intelligence_web_authorize_command_operation(
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
    match state.workspaces.authorize_command_operation(
        requested_intelligence_workspace(&headers),
        program,
        &args,
        cwd,
    ) {
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
                    "pending": intelligence_pending_authorizations(&state)
                })),
            )
                .into_response(),
            Err(error) => intelligence_bad_request(error),
        },
        Err(error) => intelligence_bad_request(error),
    }
}

async fn intelligence_web_deny_authorization(
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
    if !state.workspaces.deny_authorization(id) {
        return intelligence_bad_request("authorization request is missing or no longer pending");
    }
    (
        StatusCode::OK,
        Json(json!({"ok":true,"pending":intelligence_pending_authorizations(&state)})),
    )
        .into_response()
}

async fn intelligence_web_project(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let (workspace_id, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return *response,
    };
    let review = if workspace.exec_enabled() && workspace.root().join(".git").is_dir() {
        state
            .harness
            .review_changes(workspace_id.clone(), &workspace, 30, &state.monitor)
            .await
            .ok()
    } else {
        None
    };
    let harness = state.harness.clone();
    let workspace_for_read = workspace.clone();
    let workspace_id_for_read = workspace_id.clone();
    let project = mcp_tools::run_blocking(move || {
        let project = harness.project_observatory(
            workspace_id_for_read,
            &workspace_for_read,
            review.as_ref(),
        )?;
        serde_json::to_value(project).map_err(Into::into)
    })
    .await;
    match project {
        Ok(mut value) => {
            value["workspace_options"] = intelligence_workspace_options(&state);
            value["pending_authorizations"] = json!(state
                .workspaces
                .authorization_requests(256)
                .into_iter()
                .filter(|request| request.status == AuthorizationStatus::Pending)
                .count());
            (StatusCode::OK, Json(value)).into_response()
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
        )
            .into_response(),
    }
}

async fn intelligence_web_revision(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let (_workspace_id, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return *response,
    };
    let revision = match state.harness.observatory_revision_signal(&workspace).await {
        Ok(revision) => revision,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": error.to_string()})),
            )
                .into_response()
        }
    };
    let graph_revision = state
        .harness
        .graph_history(&workspace, 1)
        .ok()
        .and_then(|history| history.into_iter().next())
        .map(|entry| entry.id);
    (
        StatusCode::OK,
        Json(json!({
            "fingerprint": revision.fingerprint,
            "changed_files": revision.changed_files,
            "truncated": revision.truncated,
            "full_refresh_required": revision.full_refresh_required,
            "graph_revision": graph_revision,
            "pending_authorizations": state
                .workspaces
                .authorization_requests(256)
                .into_iter()
                .filter(|request| request.status == AuthorizationStatus::Pending)
                .count()
        })),
    )
        .into_response()
}

async fn intelligence_web_refresh_semantics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let (_workspace_id, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return *response,
    };
    match state
        .harness
        .semantic_provider_refresh(&workspace, ".", 128, 1_000)
        .await
    {
        Ok(refresh) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "runs": refresh.runs,
                "failures": refresh.failures,
                "truncated": refresh.truncated
            })),
        )
            .into_response(),
        Err(error) => intelligence_bad_request(error),
    }
}

async fn intelligence_web_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let (workspace_id, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return *response,
    };
    let harness = state.harness.clone();
    let workspace_for_read = workspace.clone();
    let workspace_id_for_read = workspace_id.clone();
    let base = match tokio::task::spawn_blocking(move || -> AnyResult<Value> {
        let design = harness.design_status(workspace_id_for_read.clone(), &workspace_for_read)?;
        let traceability =
            harness.traceability_status(workspace_id_for_read.clone(), &workspace_for_read)?;
        let semantics =
            harness.semantic_status(&workspace_id_for_read, &workspace_for_read, 100)?;
        let scope_status = harness.product_scope_status(&workspace_for_read)?;
        let graph_history = harness.graph_history(&workspace_for_read, 20)?;
        let graph_diff = if graph_history.len() >= 2 {
            harness
                .graph_diff(
                    &workspace_for_read,
                    &GraphDiffInput {
                        from_snapshot_id: None,
                        to_snapshot_id: None,
                        limit: 20,
                    },
                )
                .ok()
        } else {
            None
        };
        let graph_providers = harness.graph_provider_status(&workspace_for_read)?;
        let semantic_providers = harness.semantic_provider_status(&workspace_for_read)?;
        let verification_executors = harness.verification_executor_status(&workspace_for_read)?;
        let evidence =
            harness.evidence_status(&workspace_id_for_read, &workspace_for_read, None, 100)?;
        let reconciliation = harness.reconciliation_history(&workspace_for_read, 20)?;
        let verification =
            harness.verification_history(&workspace_id_for_read, &workspace_for_read, 20)?;
        let mut reconciliation_execution = Vec::new();
        for plan in reconciliation.iter().take(20) {
            if let Ok(status) = harness.reconciliation_execution_status(
                &workspace_id_for_read,
                &workspace_for_read,
                &plan.id,
            ) {
                reconciliation_execution.push(status);
            }
        }
        Ok(json!({
            "design": design,
            "traceability": traceability,
            "semantics": semantics,
            "scope_status": scope_status,
            "graph_history": graph_history,
            "graph_diff": graph_diff,
            "graph_providers": graph_providers,
            "semantic_providers": semantic_providers,
            "verification_executors": verification_executors,
            "evidence": evidence,
            "reconciliation": reconciliation,
            "reconciliation_execution": reconciliation_execution,
            "verification": verification,
        }))
    })
    .await
    {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": error.to_string()})),
            )
                .into_response()
        }
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("intelligence dashboard task failed: {error}")})),
            )
                .into_response()
        }
    };
    let risk = if workspace.exec_enabled() && workspace.root().join(".git").is_dir() {
        match state
            .harness
            .review_changes(workspace_id.clone(), &workspace, 30, &state.monitor)
            .await
        {
            Ok(review) => state
                .harness
                .risk_status(workspace_id.clone(), &workspace, &review)
                .ok()
                .and_then(|risk| serde_json::to_value(risk).ok()),
            Err(_) => None,
        }
    } else {
        None
    };
    let mut value = base;
    value["workspace"] = json!(workspace_id);
    value["root"] = json!(workspace.root());
    value["risk"] = risk.unwrap_or(Value::Null);
    value["workspace_options"] = intelligence_workspace_options(&state);
    if let Some(design) = value.get("design") {
        state
            .monitor
            .record_intelligence_result(&workspace_id, "design_status", design);
    }
    if let Some(traceability) = value.get("traceability") {
        state.monitor.record_intelligence_result(
            &workspace_id,
            "traceability_status",
            traceability,
        );
    }
    if let Some(scope_status) = value.get("scope_status") {
        state
            .monitor
            .record_intelligence_result(&workspace_id, "scope_status", scope_status);
    }
    if let Some(semantics) = value.get("semantics") {
        state
            .monitor
            .record_intelligence_result(&workspace_id, "semantic_status", semantics);
    }
    if let Some(evidence) = value.get("evidence") {
        state
            .monitor
            .record_intelligence_result(&workspace_id, "evidence_status", evidence);
    }
    if let Some(risk) = value.get("risk").filter(|risk| !risk.is_null()) {
        state
            .monitor
            .record_intelligence_result(&workspace_id, "risk_status", risk);
    }
    (StatusCode::OK, Json(value)).into_response()
}

async fn intelligence_web_scopes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let (workspace_id, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return *response,
    };
    let harness = state.harness.clone();
    match mcp_tools::run_blocking(move || {
        harness
            .product_scope_status(&workspace)
            .and_then(|status| serde_json::to_value(status).map_err(Into::into))
    })
    .await
    {
        Ok(status) => {
            state
                .monitor
                .record_intelligence_result(&workspace_id, "scope_status", &status);
            (
                StatusCode::OK,
                Json(json!({"workspace": workspace_id, "scope_status": status})),
            )
                .into_response()
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
        )
            .into_response(),
    }
}

async fn intelligence_web_graph(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let (_, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return *response,
    };
    let harness = state.harness.clone();
    let graph = tokio::task::spawn_blocking(move || {
        harness.graph_query(
            &workspace,
            &GraphQueryInput {
                snapshot_id: None,
                node_id: None,
                kind: None,
                label_contains: None,
                related_to: None,
                edge_kind: None,
                direction: None,
                limit: 300,
            },
        )
    })
    .await;
    match graph {
        Ok(Ok(graph)) => (StatusCode::OK, Json(json!(graph))).into_response(),
        Ok(Err(error)) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": error.to_string()})),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("graph dashboard task failed: {error}")})),
        )
            .into_response(),
    }
}

async fn health(State(state): State<Arc<AppState>>) -> Json<Value> {
    let connection = state.monitor.connection_status();
    Json(json!({
        "ok": true,
        "name": "wcode",
        "instance_id": state.auth.instance_id(),
        "version": env!("CARGO_PKG_VERSION"),
        "workspaces": state.workspaces.capabilities(),
        "max_parallel_tools": state.harness.max_parallel(),
        "harness": state.harness.capabilities(),
        "mcp_url": format!("{}/mcp", state.auth.public_url()),
        "project_url": PROJECT_URL,
        "author_url": AUTHOR_URL,
        "oauth_client_registered": connection.oauth_client_registered,
        "oauth_authorized": connection.oauth_authorized,
        "mcp_connected": connection.chatgpt_initialized,
        // Backward-compatible alias retained for existing health consumers.
        "chatgpt_initialized": connection.chatgpt_initialized,
        "supported_protocol_versions": SUPPORTED_PROTOCOL_VERSIONS,
        "modern_protocol_version": MODERN_PROTOCOL_VERSION,
        "initialize_count": connection.initialize_count,
        "last_initialize_seconds_ago": connection.last_initialize_seconds_ago,
        "last_mcp_seen_seconds_ago": connection.last_mcp_seen_seconds_ago,
        "public_endpoint": connection.public_endpoint,
        "public_url_healthy": connection.public_url_healthy,
        "public_url_last_checked_seconds_ago": connection.public_url_last_checked_seconds_ago,
        "public_url_consecutive_failures": connection.public_url_consecutive_failures,
        "public_url_error": connection.public_url_error,
        "tunnel_running": connection.tunnel_running,
        "tunnel_error": connection.tunnel_error,
        "active_tasks": connection.active_tasks,
        "queued_tasks": connection.queued_tasks,
        "peak_active_tasks": connection.peak_active_tasks,
    }))
}

async fn mcp(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    if !origin_allowed(&state.auth.public_url(), &headers) {
        return forbidden_origin_response();
    }
    let Some(owner) = state.auth.authorized_client_fingerprint(&headers) else {
        return state.auth.unauthorized_response();
    };

    let protocol = request_protocol(&headers, &payload);
    if !SUPPORTED_PROTOCOL_VERSIONS.contains(&protocol.as_str()) {
        return unsupported_protocol_response(&payload, &protocol);
    }
    let modern = protocol == MODERN_PROTOCOL_VERSION;
    if modern {
        if payload.is_array() {
            return modern_bad_request(
                &payload,
                "JSON-RPC batches are not supported on the stateless 2026 protocol path",
            );
        }
        if let Err(error) = validate_modern_request(&headers, &payload) {
            return modern_bad_request(&payload, error);
        }
        state.monitor.mark_mcp_connected();
    }
    state.monitor.mark_mcp_seen();

    let response = if let Some(items) = payload.as_array() {
        if let Some(error) = mcp_tools::batch_validation_error(items.len()) {
            Some(error)
        } else {
            let mut tasks = JoinSet::new();
            for item in items.iter().cloned() {
                let state = state.clone();
                let protocol = protocol.clone();
                let owner = owner.clone();
                tasks.spawn(async move { handle_message(state, item, &protocol, &owner).await });
            }

            let mut responses = Vec::new();
            while let Some(joined) = tasks.join_next().await {
                match joined {
                    Ok(Some(response)) => responses.push(response),
                    Ok(None) => {}
                    Err(error) => responses.push(jsonrpc_error(
                        Value::Null,
                        -32603,
                        join_error_message("batch request", &error),
                    )),
                }
            }
            if responses.is_empty() {
                None
            } else {
                Some(Value::Array(responses))
            }
        }
    } else {
        handle_message_isolated(state, payload, &protocol, &owner).await
    };

    match response {
        Some(value) => {
            let status = mcp_response_status(&value, modern);
            (status, [("mcp-protocol-version", protocol)], Json(value)).into_response()
        }
        None => StatusCode::ACCEPTED.into_response(),
    }
}

fn mcp_response_status(value: &Value, modern: bool) -> StatusCode {
    match value.pointer("/error/code").and_then(Value::as_i64) {
        Some(-32022..=-32020) if modern => StatusCode::BAD_REQUEST,
        Some(-32601) if modern => StatusCode::NOT_FOUND,
        _ => StatusCode::OK,
    }
}

fn request_protocol(headers: &HeaderMap, payload: &Value) -> String {
    if let Some(value) = headers
        .get("mcp-protocol-version")
        .and_then(|value| value.to_str().ok())
    {
        return value.to_owned();
    }
    if payload
        .pointer("/params/_meta/io.modelcontextprotocol~1protocolVersion")
        .and_then(Value::as_str)
        == Some(MODERN_PROTOCOL_VERSION)
        || payload
            .get("params")
            .and_then(Value::as_object)
            .and_then(|params| params.get("_meta"))
            .and_then(Value::as_object)
            .and_then(|meta| meta.get("io.modelcontextprotocol/protocolVersion"))
            .and_then(Value::as_str)
            == Some(MODERN_PROTOCOL_VERSION)
    {
        return MODERN_PROTOCOL_VERSION.to_owned();
    }
    payload
        .pointer("/params/protocolVersion")
        .and_then(Value::as_str)
        .filter(|version| LEGACY_PROTOCOL_VERSIONS.contains(version))
        .unwrap_or("2025-03-26")
        .to_owned()
}

fn origin_allowed(public_url: &str, headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get("origin").and_then(|value| value.to_str().ok()) else {
        return true;
    };
    let (Ok(origin), Ok(public)) = (url::Url::parse(origin), url::Url::parse(public_url)) else {
        return false;
    };
    origin.query().is_none()
        && origin.fragment().is_none()
        && origin.path() == "/"
        && origin.scheme() == public.scheme()
        && origin.host_str().is_some_and(|host| {
            public
                .host_str()
                .is_some_and(|public_host| host.eq_ignore_ascii_case(public_host))
        })
        && origin.port_or_known_default() == public.port_or_known_default()
}

fn forbidden_origin_response() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(jsonrpc_error(
            Value::Null,
            -32600,
            "Origin does not match the configured public MCP origin",
        )),
    )
        .into_response()
}

pub(crate) fn validate_modern_payload(payload: &Value) -> Result<(), &'static str> {
    let method = payload
        .get("method")
        .and_then(Value::as_str)
        .ok_or("missing JSON-RPC method")?;
    if method == "server/discover" {
        return Ok(());
    }
    let meta = payload
        .pointer("/params/_meta")
        .and_then(Value::as_object)
        .ok_or("missing 2026 request _meta envelope")?;
    if meta
        .get("io.modelcontextprotocol/protocolVersion")
        .and_then(Value::as_str)
        != Some(MODERN_PROTOCOL_VERSION)
    {
        return Err("request _meta protocolVersion does not match MCP-Protocol-Version");
    }
    if !meta
        .get("io.modelcontextprotocol/clientCapabilities")
        .is_some_and(Value::is_object)
    {
        return Err("missing io.modelcontextprotocol/clientCapabilities metadata");
    }
    match method {
        "tools/call"
            if payload
                .pointer("/params/name")
                .and_then(Value::as_str)
                .is_none() =>
        {
            return Err("tools/call is missing params.name")
        }
        "prompts/get"
            if payload
                .pointer("/params/name")
                .and_then(Value::as_str)
                .is_none() =>
        {
            return Err("prompts/get is missing params.name")
        }
        "resources/read"
            if payload
                .pointer("/params/uri")
                .and_then(Value::as_str)
                .is_none() =>
        {
            return Err("resources/read is missing params.uri")
        }
        _ => {}
    }
    Ok(())
}

fn modern_mcp_name<'a>(method: &str, payload: &'a Value) -> Result<Option<&'a str>, &'static str> {
    match method {
        "tools/call" => payload
            .pointer("/params/name")
            .and_then(Value::as_str)
            .map(Some)
            .ok_or("tools/call is missing params.name"),
        "prompts/get" => payload
            .pointer("/params/name")
            .and_then(Value::as_str)
            .map(Some)
            .ok_or("prompts/get is missing params.name"),
        "resources/read" => payload
            .pointer("/params/uri")
            .and_then(Value::as_str)
            .map(Some)
            .ok_or("resources/read is missing params.uri"),
        _ => Ok(None),
    }
}

fn decode_mcp_header_value(value: &str) -> Result<String, &'static str> {
    const PREFIX: &str = "=?base64?";
    const SUFFIX: &str = "?=";
    if value.starts_with(PREFIX) && value.ends_with(SUFFIX) {
        let encoded = &value[PREFIX.len()..value.len() - SUFFIX.len()];
        let decoded = STANDARD
            .decode(encoded)
            .map_err(|_| "invalid Mcp-Name base64 sentinel encoding")?;
        return String::from_utf8(decoded).map_err(|_| "invalid Mcp-Name base64 sentinel UTF-8");
    }
    if value.trim_matches([' ', '\t']) != value
        || !value
            .bytes()
            .all(|byte| byte == b' ' || (0x21..=0x7e).contains(&byte))
    {
        return Err("invalid plain Mcp-Name header value");
    }
    Ok(value.to_owned())
}

fn validate_modern_request(headers: &HeaderMap, payload: &Value) -> Result<(), &'static str> {
    let method = payload
        .get("method")
        .and_then(Value::as_str)
        .ok_or("missing JSON-RPC method")?;
    let header_protocol = headers
        .get("mcp-protocol-version")
        .and_then(|value| value.to_str().ok())
        .ok_or("missing MCP-Protocol-Version header")?;
    if header_protocol != MODERN_PROTOCOL_VERSION {
        return Err("MCP-Protocol-Version header does not match request _meta protocolVersion");
    }
    let header_method = headers
        .get("mcp-method")
        .and_then(|value| value.to_str().ok())
        .ok_or("missing Mcp-Method header")?;
    if header_method != method {
        return Err("Mcp-Method header does not match the JSON-RPC method");
    }
    validate_modern_payload(payload)?;
    if let Some(expected) = modern_mcp_name(method, payload)? {
        let encoded = headers
            .get("mcp-name")
            .and_then(|value| value.to_str().ok())
            .ok_or("missing Mcp-Name header")?;
        let decoded = decode_mcp_header_value(encoded)?;
        if decoded != expected {
            return Err("Mcp-Name header does not match request body value");
        }
    }
    Ok(())
}

fn modern_validation_error_code(message: &str) -> i64 {
    if message.contains("MCP-Protocol-Version")
        || message.contains("Mcp-Method")
        || message.contains("Mcp-Name")
    {
        -32020
    } else if message.contains("_meta")
        || message.contains("clientCapabilities")
        || message.contains("params.name")
        || message.contains("params.uri")
    {
        -32602
    } else {
        -32600
    }
}

fn modern_bad_request(payload: &Value, message: impl Into<String>) -> Response {
    let message = message.into();
    let code = modern_validation_error_code(&message);
    (
        StatusCode::BAD_REQUEST,
        [("mcp-protocol-version", MODERN_PROTOCOL_VERSION)],
        Json(jsonrpc_error(
            payload.get("id").cloned().unwrap_or(Value::Null),
            code,
            message,
        )),
    )
        .into_response()
}

fn unsupported_protocol_response(payload: &Value, requested: &str) -> Response {
    let error = json!({
        "jsonrpc": "2.0",
        "id": payload.get("id").cloned().unwrap_or(Value::Null),
        "error": {
            "code": -32022,
            "message": "Unsupported protocol version",
            "data": {
                "supported": SUPPORTED_PROTOCOL_VERSIONS,
                "requested": requested,
            }
        }
    });
    (StatusCode::BAD_REQUEST, Json(error)).into_response()
}

const SERVER_INSTRUCTIONS: &str = "Work only inside configured workspace roots and never bypass authorization or path protections. For coding call agent_context first and omit budget unless a fixed cap is required. Follow readiness/next_actions; do not reread hot_source. Use symbol_context only for missing bodies, apply_edits for one direct target file or apply_file_edits for multiple, then review_changes and verify_project. Reuse existing helpers before adding layers. Tree-sitter is syntax precision unless fresh stronger provider evidence says otherwise. Use deeper design/risk/reconciliation/language-quality tools only when required. Never fabricate Evidence, stage success, semantic precision, or HumanApproval.";

fn join_error_message(scope: &str, error: &JoinError) -> String {
    let kind = if error.is_cancelled() {
        "was cancelled"
    } else if error.is_panic() {
        "panicked"
    } else {
        "failed to join"
    };
    format!("{scope} {kind}; the MCP session remains available")
}

async fn join_message_task(id: Option<Value>, task: JoinHandle<Option<Value>>) -> Option<Value> {
    match task.await {
        Ok(response) => response,
        Err(error) => {
            id.map(|id| jsonrpc_error(id, -32603, join_error_message("tool request", &error)))
        }
    }
}

pub(crate) async fn handle_message_isolated(
    state: Arc<AppState>,
    message: Value,
    protocol: &str,
    owner: &str,
) -> Option<Value> {
    let id = message.get("id").cloned();
    let protocol = protocol.to_owned();
    let owner = owner.to_owned();
    let task = tokio::spawn(async move { handle_message(state, message, &protocol, &owner).await });
    join_message_task(id, task).await
}

pub(crate) async fn handle_message(
    state: Arc<AppState>,
    message: Value,
    protocol: &str,
    owner: &str,
) -> Option<Value> {
    let id = message.get("id").cloned()?;
    let method = message.get("method").and_then(Value::as_str)?;
    let modern = protocol == MODERN_PROTOCOL_VERSION;
    if modern && matches!(method, "tasks/get" | "tasks/update" | "tasks/cancel") {
        if !client_supports_tasks(&message) {
            return Some(task_rpc_error(id, TaskRpcError::missing_capability()));
        }
        let task_id = message
            .pointer("/params/taskId")
            .and_then(Value::as_str)
            .filter(|task_id| !task_id.is_empty() && task_id.len() <= 96);
        let Some(task_id) = task_id else {
            return Some(task_rpc_error(
                id,
                TaskRpcError::invalid("taskId is required"),
            ));
        };
        let result = match method {
            "tasks/get" => get_task(&state, task_id, owner),
            "tasks/update" => update_task(&state, task_id, owner),
            "tasks/cancel" => cancel_task(&state, task_id, owner),
            _ => unreachable!(),
        };
        return Some(match result {
            Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
            Err(error) => task_rpc_error(id, error),
        });
    }
    if modern && method == "tools/call" && client_supports_tasks(&message) {
        let params = message.get("params").cloned().unwrap_or_default();
        if task_augmented_tool(&params) {
            return Some(
                match create_tool_task(state, params, owner.to_owned()).await {
                    Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
                    Err(error) => task_rpc_error(id, error),
                },
            );
        }
    }
    let result = match method {
        "server/discover" if modern => Ok(modern_cacheable_result(json!({
            "supportedVersions": DISCOVER_PROTOCOL_VERSIONS,
            "capabilities": {
                "tools": {"listChanged": false},
                "prompts": {"listChanged": false},
                "resources": {"listChanged": false, "subscribe": false},
                "extensions": {
                    "io.modelcontextprotocol/tasks": {},
                    (MEDIA_CONTENT_EXTENSION_ID): {
                        "contentTypes": ["image", "audio"],
                        "optInPerCall": true,
                        "metadataOnlyWithoutCapability": true
                    }
                }
            },
            "instructions": SERVER_INSTRUCTIONS,
        }))),
        "initialize" if !modern => {
            state.monitor.mark_mcp_initialized();
            let requested = message
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str)
                .filter(|version| LEGACY_PROTOCOL_VERSIONS.contains(version))
                .unwrap_or("2025-11-25");
            Ok(json!({
                "protocolVersion": requested,
                "capabilities": {
                    "tools": {"listChanged": false},
                    "prompts": {"listChanged": false},
                    "resources": {"listChanged": false, "subscribe": false}
                },
                "serverInfo": {"name": "wcode", "version": env!("CARGO_PKG_VERSION")},
                "instructions": SERVER_INSTRUCTIONS,
            }))
        }
        "ping" => Ok(if modern {
            modern_result(json!({}))
        } else {
            json!({})
        }),
        "tools/list" => {
            let catalog = mcp_tools::tools();
            Ok(if modern {
                modern_cacheable_result(json!({"tools": catalog}))
            } else {
                json!({"tools": catalog})
            })
        }
        "prompts/list" => Ok(if modern {
            modern_cacheable_result(mcp_catalog::prompts_list())
        } else {
            mcp_catalog::prompts_list()
        }),
        "prompts/get" => {
            let params = message.get("params").cloned().unwrap_or_default();
            match params.get("name").and_then(Value::as_str) {
                Some(name) => mcp_catalog::prompt_get(name, params.get("arguments")).map(|value| {
                    if modern {
                        modern_result(value)
                    } else {
                        value
                    }
                }),
                None => Err("prompts/get is missing params.name".to_owned()),
            }
        }
        "resources/list" => Ok(if modern {
            modern_cacheable_result(mcp_catalog::resources_list())
        } else {
            mcp_catalog::resources_list()
        }),
        "resources/read" => {
            let params = message.get("params").cloned().unwrap_or_default();
            match params.get("uri").and_then(Value::as_str) {
                Some(uri) => mcp_catalog::resource_read(uri).map(|value| {
                    if modern {
                        modern_cacheable_result(value)
                    } else {
                        value
                    }
                }),
                None => Err("resources/read is missing params.uri".to_owned()),
            }
        }
        "tools/call" => call_tool(&state, message.get("params").cloned().unwrap_or_default())
            .await
            .map(|value| if modern { modern_result(value) } else { value }),
        _ => {
            return Some(jsonrpc_error(
                id,
                -32601,
                format!("Method not found: {method}"),
            ))
        }
    };
    Some(match result {
        Ok(value) => json!({"jsonrpc": "2.0", "id": id, "result": value}),
        Err(error) => jsonrpc_error(id, -32602, error),
    })
}

pub(crate) fn modern_result(mut value: Value) -> Value {
    let Some(object) = value.as_object_mut() else {
        return value;
    };
    object
        .entry("resultType".to_owned())
        .or_insert_with(|| Value::String("complete".to_owned()));
    let meta = object
        .entry("_meta".to_owned())
        .or_insert_with(|| json!({}));
    if let Some(meta) = meta.as_object_mut() {
        meta.entry("io.modelcontextprotocol/serverInfo".to_owned())
            .or_insert_with(|| json!({"name": "wcode", "version": env!("CARGO_PKG_VERSION")}));
    }
    value
}

fn modern_cacheable_result(value: Value) -> Value {
    let mut value = modern_result(value);
    if let Some(object) = value.as_object_mut() {
        object.insert("ttlMs".to_owned(), json!(300_000));
        object.insert("cacheScope".to_owned(), json!("private"));
    }
    value
}

#[path = "mcp_dispatch.rs"]
mod mcp_dispatch;
#[path = "mcp_tools.rs"]
mod mcp_tools;
pub(crate) use mcp_dispatch::call_tool;
#[cfg(test)]
use mcp_dispatch::{estimated_context_bytes_avoided, parallel_item_from_response};

pub(crate) fn selected_workspace(
    state: &AppState,
    args: &Value,
) -> Result<(String, Workspace), String> {
    state
        .workspaces
        .select(string_arg(args, "workspace"))
        .map_err(|error| error.to_string())
}

pub(crate) fn jsonrpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

fn string_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_store;
    use crate::workspace::WorkspaceSecurity;
    use std::fs;

    fn modern_headers(method: &str, name: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "mcp-protocol-version",
            MODERN_PROTOCOL_VERSION.parse().unwrap(),
        );
        headers.insert("mcp-method", method.parse().unwrap());
        if let Some(name) = name {
            headers.insert("mcp-name", name.parse().unwrap());
        }
        headers
    }

    fn modern_request(method: &str, params: Value) -> Value {
        let mut request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": {
                "_meta": {
                    "io.modelcontextprotocol/protocolVersion": MODERN_PROTOCOL_VERSION,
                    "io.modelcontextprotocol/clientCapabilities": {}
                }
            }
        });
        if let Some(object) = params.as_object() {
            for (key, value) in object {
                request["params"][key] = value.clone();
            }
        }
        request
    }

    fn task_capable(mut request: Value) -> Value {
        request["params"]["_meta"]["io.modelcontextprotocol/clientCapabilities"] = json!({
            "extensions": {(TASK_EXTENSION_ID): {}}
        });
        request
    }

    #[tokio::test]
    async fn isolated_message_turns_panics_and_cancellation_into_jsonrpc_errors() {
        let panic_task = tokio::spawn(async move {
            panic!("synthetic MCP child panic");
            #[allow(unreachable_code)]
            None::<Value>
        });
        let panic_response = join_message_task(Some(json!(41)), panic_task)
            .await
            .unwrap();
        assert_eq!(panic_response["error"]["code"], -32603);
        assert!(panic_response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("panicked"));

        let cancelled_task =
            tokio::spawn(async move { std::future::pending::<Option<Value>>().await });
        cancelled_task.abort();
        let cancelled_response = join_message_task(Some(json!(42)), cancelled_task)
            .await
            .unwrap();
        assert_eq!(cancelled_response["error"]["code"], -32603);
        assert!(cancelled_response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("cancelled"));

        let dir = tempfile::tempdir().unwrap();
        let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
        let workspace_id = workspaces.default_id().to_owned();
        let state = Arc::new(AppState {
            auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
            workspaces,
            harness: ToolHarness::new(2).unwrap(),
            monitor: TaskMonitor::new([workspace_id]),
            tasks: TaskRuntime::default(),
        });
        let response = handle_message_isolated(
            state,
            modern_request(
                "tools/call",
                json!({"name":"workspace_info","arguments":{}}),
            ),
            MODERN_PROTOCOL_VERSION,
            &"a".repeat(64),
        )
        .await
        .unwrap();
        assert!(response.get("result").is_some());
    }

    #[test]
    fn protocol_detection_supports_modern_and_legacy_clients() {
        let modern = modern_headers("tools/list", None);
        assert_eq!(
            request_protocol(&modern, &json!({})),
            MODERN_PROTOCOL_VERSION
        );

        let legacy = json!({"params": {"protocolVersion": "2024-11-05"}});
        assert_eq!(request_protocol(&HeaderMap::new(), &legacy), "2024-11-05");
        assert_eq!(
            request_protocol(&HeaderMap::new(), &json!({})),
            "2025-03-26"
        );
        assert_eq!(
            request_protocol(&HeaderMap::new(), &modern_request("tools/list", json!({}))),
            MODERN_PROTOCOL_VERSION
        );
    }

    #[test]
    fn origin_validation_accepts_same_origin_and_rejects_cross_origin() {
        let mut headers = HeaderMap::new();
        assert!(origin_allowed("https://example.com/gateway", &headers));

        headers.insert("origin", "https://example.com".parse().unwrap());
        assert!(origin_allowed("https://example.com/gateway", &headers));

        headers.insert("origin", "https://grok.com".parse().unwrap());
        assert!(!origin_allowed("https://example.com/gateway", &headers));
    }

    #[test]
    fn modern_requests_require_routing_headers_and_metadata() {
        let request = modern_request("tools/call", json!({"name": "workspace_info"}));
        let headers = modern_headers("tools/call", Some("workspace_info"));
        assert!(validate_modern_request(&headers, &request).is_ok());

        let mut wrong_name = headers.clone();
        wrong_name.insert("mcp-name", "read_file".parse().unwrap());
        assert_eq!(
            validate_modern_request(&wrong_name, &request),
            Err("Mcp-Name header does not match request body value")
        );

        let missing_meta = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}});
        assert!(
            validate_modern_request(&modern_headers("tools/list", None), &missing_meta).is_err()
        );

        let bootstrap = json!({"jsonrpc": "2.0", "id": 2, "method": "server/discover"});
        assert!(
            validate_modern_request(&modern_headers("server/discover", None), &bootstrap).is_ok()
        );
    }

    #[test]
    fn modern_name_headers_cover_tools_prompts_resources_and_base64_sentinel() {
        let prompt = modern_request("prompts/get", json!({"name":"wcode-review"}));
        assert!(validate_modern_request(
            &modern_headers("prompts/get", Some("wcode-review")),
            &prompt
        )
        .is_ok());
        assert_eq!(
            validate_modern_request(&modern_headers("prompts/get", None), &prompt),
            Err("missing Mcp-Name header")
        );

        let uri = "wcode://runtime/安全";
        let encoded = format!("=?base64?{}?=", STANDARD.encode(uri.as_bytes()));
        let resource = modern_request("resources/read", json!({"uri":uri}));
        assert!(validate_modern_request(
            &modern_headers("resources/read", Some(&encoded)),
            &resource
        )
        .is_ok());
        assert_eq!(decode_mcp_header_value(&encoded).unwrap(), uri);
        assert!(decode_mcp_header_value("=?base64?not%%%?=").is_err());
        assert!(decode_mcp_header_value(" padded ").is_err());
        assert!(decode_mcp_header_value("a\tb").is_err());
        assert_eq!(
            decode_mcp_header_value("=?base64?literal").unwrap(),
            "=?base64?literal"
        );

        let mut missing_protocol = modern_headers("tools/list", None);
        missing_protocol.remove("mcp-protocol-version");
        assert_eq!(
            validate_modern_request(&missing_protocol, &modern_request("tools/list", json!({}))),
            Err("missing MCP-Protocol-Version header")
        );
    }

    #[test]
    fn modern_validation_uses_final_2026_error_codes() {
        assert_eq!(
            modern_validation_error_code("Mcp-Method header does not match the JSON-RPC method"),
            -32020
        );
        assert_eq!(
            modern_validation_error_code("missing MCP-Protocol-Version header"),
            -32020
        );
        assert_eq!(
            modern_validation_error_code("invalid Mcp-Name base64 sentinel encoding"),
            -32020
        );
        assert_eq!(
            modern_validation_error_code("missing 2026 request _meta envelope"),
            -32602
        );
        assert_eq!(
            modern_validation_error_code("JSON-RPC batches are not supported"),
            -32600
        );
        assert_eq!(TaskRpcError::missing_capability().code(), -32021);
        assert_eq!(
            mcp_response_status(&json!({"error":{"code":-32021}}), true),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            mcp_response_status(&json!({"error":{"code":-32601}}), true),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            mcp_response_status(&json!({"error":{"code":-32021}}), false),
            StatusCode::OK
        );
    }

    #[test]
    fn modern_results_include_server_identity_and_private_cache_hints() {
        let result = modern_cacheable_result(json!({"tools": []}));
        assert_eq!(result["resultType"], "complete");
        assert_eq!(result["cacheScope"], "private");
        assert_eq!(result["ttlMs"], 300_000);
        assert_eq!(
            result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
            "wcode"
        );
    }

    #[tokio::test]
    async fn task_capability_is_per_request_and_advertised_by_discovery() {
        let plain = modern_request(
            "tools/call",
            json!({"name":"semantic_provider_refresh","arguments":{}}),
        );
        assert!(!client_supports_tasks(&plain));
        let capable = task_capable(plain);
        assert!(client_supports_tasks(&capable));

        let dir = tempfile::tempdir().unwrap();
        let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
        let workspace_id = workspaces.default_id().to_owned();
        let state = Arc::new(AppState {
            auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
            workspaces,
            harness: ToolHarness::new(2).unwrap(),
            monitor: TaskMonitor::new([workspace_id]),
            tasks: TaskRuntime::default(),
        });
        let response = handle_message(
            state,
            modern_request("server/discover", json!({})),
            MODERN_PROTOCOL_VERSION,
            &"a".repeat(64),
        )
        .await
        .unwrap();
        assert!(response["result"]["capabilities"]["extensions"][TASK_EXTENSION_ID].is_object());
        assert!(MEDIA_CONTENT_EXTENSION_ID.starts_with("run.francis.wcode/"));
        assert_eq!(
            response["result"]["capabilities"]["extensions"][MEDIA_CONTENT_EXTENSION_ID]
                ["contentTypes"],
            json!(["image", "audio"])
        );
        assert_eq!(
            response["result"]["capabilities"]["tools"]["listChanged"],
            false
        );
        assert_eq!(
            response["result"]["capabilities"]["prompts"]["listChanged"],
            false
        );
        assert_eq!(
            response["result"]["capabilities"]["resources"]["subscribe"],
            false
        );
    }

    #[tokio::test]
    async fn task_augmented_tool_is_durable_pollable_and_owner_scoped() {
        let dir = tempfile::tempdir().unwrap();
        let workspaces = Workspaces::new_with_security(
            [dir.path()],
            false,
            true,
            WorkspaceSecurity {
                allow_risky_exec: true,
                ..WorkspaceSecurity::default()
            },
        )
        .unwrap();
        let workspace_id = workspaces.default_id().to_owned();
        let state = Arc::new(AppState {
            auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
            workspaces,
            harness: ToolHarness::new(2).unwrap(),
            monitor: TaskMonitor::new([workspace_id.clone()]),
            tasks: TaskRuntime::default(),
        });
        let owner = "a".repeat(64);
        let created = create_tool_task(
            state.clone(),
            json!({
                "name":"semantic_provider_refresh",
                "arguments":{"workspace":workspace_id}
            }),
            owner.clone(),
        )
        .await
        .unwrap();
        assert_eq!(created["resultType"], "task");
        assert_eq!(created["status"], "working");
        let task_id = created["taskId"].as_str().unwrap().to_owned();
        let found = task_store::find(&state.workspaces, &task_id)
            .unwrap()
            .expect("task must be durable before its handle is returned");
        assert_eq!(found.2.owner, owner);

        let mut completed = None;
        for _ in 0..100 {
            let current = get_task(&state, &task_id, &owner).unwrap();
            if current["status"] == "completed" {
                completed = Some(current);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let completed = completed.expect("background semantic refresh should complete");
        assert_eq!(completed["resultType"], "complete");
        assert_eq!(completed["result"]["isError"], false);
        assert_eq!(
            completed["result"]["structuredContent"]["runs"]
                .as_array()
                .unwrap()
                .len(),
            0
        );

        let other_owner = "b".repeat(64);
        let error = get_task(&state, &task_id, &other_owner).unwrap_err();
        assert_eq!(error.code(), -32602);
    }

    #[tokio::test]
    async fn task_methods_require_the_extension_on_each_request() {
        let dir = tempfile::tempdir().unwrap();
        let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
        let workspace_id = workspaces.default_id().to_owned();
        let state = Arc::new(AppState {
            auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
            workspaces,
            harness: ToolHarness::new(2).unwrap(),
            monitor: TaskMonitor::new([workspace_id]),
            tasks: TaskRuntime::default(),
        });
        let response = handle_message(
            state,
            modern_request("tasks/get", json!({"taskId":"TASK-unknown"})),
            MODERN_PROTOCOL_VERSION,
            &"a".repeat(64),
        )
        .await
        .unwrap();
        assert_eq!(response["error"]["code"], -32021);
        assert!(
            response["error"]["data"]["requiredCapabilities"]["extensions"][TASK_EXTENSION_ID]
                .is_object()
        );
    }

    #[test]
    fn task_details_explain_work_without_exposing_payloads() {
        let search = json!({"path": "src", "query": "private implementation text"});
        let detail = mcp_tools::task_detail("search_code", &search);
        assert!(detail.contains("src"));
        assert!(detail.contains("27 chars"));
        assert!(!detail.contains("private implementation text"));

        let command = json!({
            "program": "cargo",
            "args": ["test", "--token", "very-secret", "--locked"],
            "cwd": "crates/core"
        });
        let detail = mcp_tools::task_detail("run_command", &command);
        assert!(detail.contains("cargo test"));
        assert!(detail.contains("[REDACTED]"));
        assert!(!detail.contains("very-secret"));
        assert!(detail.contains("crates/core"));

        let parallel = json!({
            "tasks": [
                {"tool": "read_file", "arguments": {"path": "src/lib.rs"}},
                {"tool": "search_code", "arguments": {"query": "secret payload"}}
            ]
        });
        let detail = mcp_tools::task_detail("parallel_tools", &parallel);
        assert_eq!(detail, "2 independent tool requests");
        assert!(!detail.contains("secret payload"));

        for orchestration_tool in [
            "review_changes",
            "drift_status",
            "risk_status",
            "impact_analysis",
            "verification_plan",
            "reconciliation_plan",
        ] {
            assert_ne!(
                mcp_tools::task_detail(orchestration_tool, &json!({})),
                "unknown tool request"
            );
        }
    }

    #[test]
    fn estimates_context_avoided_only_for_precision_tools() {
        let value = json!({"source_bytes": 10_000});
        assert_eq!(
            estimated_context_bytes_avoided("symbol_context", &value, 2_000),
            8_000
        );
        assert_eq!(
            estimated_context_bytes_avoided(
                "agent_context",
                &json!({"context_bytes_avoided": 24_000}),
                3_000
            ),
            24_000
        );
        assert_eq!(
            estimated_context_bytes_avoided("file_outline", &value, 12_000),
            0
        );
        assert_eq!(
            estimated_context_bytes_avoided("read_file", &value, 2_000),
            0
        );
    }

    #[tokio::test]
    async fn prompt_and_resource_catalog_flow_through_modern_mcp() {
        let dir = tempfile::tempdir().unwrap();
        let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
        let workspace_id = workspaces.default_id().to_owned();
        let state = Arc::new(AppState {
            auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
            workspaces,
            harness: ToolHarness::new(4).unwrap(),
            monitor: TaskMonitor::new([workspace_id]),
            tasks: TaskRuntime::default(),
        });
        let owner = "a".repeat(64);

        let prompts = handle_message(
            state.clone(),
            modern_request("prompts/list", json!({})),
            MODERN_PROTOCOL_VERSION,
            &owner,
        )
        .await
        .unwrap();
        assert_eq!(prompts["result"]["prompts"].as_array().unwrap().len(), 3);

        let resource = handle_message(
            state,
            modern_request("resources/read", json!({"uri":"wcode://runtime/security"})),
            MODERN_PROTOCOL_VERSION,
            &owner,
        )
        .await
        .unwrap();
        assert!(resource["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("workspace-scoped"));
        assert_eq!(resource["result"]["cacheScope"], "private");
        assert_eq!(resource["result"]["ttlMs"], 300_000);
    }

    #[tokio::test]
    async fn routes_tools_to_selected_workspace() {
        let root = tempfile::tempdir().unwrap();
        let api = root.path().join("api");
        let web = root.path().join("web");
        fs::create_dir_all(&api).unwrap();
        fs::create_dir_all(&web).unwrap();
        fs::write(api.join("name.txt"), "api\n").unwrap();
        fs::write(web.join("name.txt"), "web\n").unwrap();

        let state = AppState {
            auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
            workspaces: Workspaces::new([&api, &web], true, false).unwrap(),
            harness: ToolHarness::new(4).unwrap(),
            monitor: TaskMonitor::new(["api".to_owned(), "web".to_owned()]),
            tasks: TaskRuntime::default(),
        };
        let result = call_tool(
            &state,
            json!({
                "name": "read_file",
                "arguments": {"workspace": "web", "path": "name.txt"}
            }),
        )
        .await
        .unwrap();
        assert_eq!(result["structuredContent"]["workspace"], "web");
        assert_eq!(result["structuredContent"]["content"], "web");

        let sha = result["structuredContent"]["sha256"].as_str().unwrap();
        let edit = call_tool(
            &state,
            json!({
                "name": "replace_text",
                "arguments": {
                    "workspace": "web",
                    "path": "name.txt",
                    "old_text": "web",
                    "new_text": "frontend",
                    "expected_sha256": sha
                }
            }),
        )
        .await
        .unwrap();
        assert_eq!(edit["structuredContent"]["workspace"], "web");
        assert_eq!(
            fs::read_to_string(web.join("name.txt")).unwrap(),
            "frontend\n"
        );
        assert_eq!(fs::read_to_string(api.join("name.txt")).unwrap(), "api\n");
    }

    #[tokio::test]
    async fn workspace_info_exposes_parallel_scheduling_guidance() {
        let dir = tempfile::tempdir().unwrap();
        let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
        let workspace_id = workspaces.default_id().to_owned();
        let state = AppState {
            auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
            workspaces,
            harness: ToolHarness::new(16).unwrap(),
            monitor: TaskMonitor::new([workspace_id]),
            tasks: TaskRuntime::default(),
        };

        let result = call_tool(&state, json!({"name": "workspace_info", "arguments": {}}))
            .await
            .unwrap();
        let info = &result["structuredContent"];
        assert_eq!(info["scheduling"]["max_parallel"], 16);
        assert_eq!(info["scheduling"]["semantics"], "global-cap-not-target");
        assert_eq!(info["scheduling"]["fanout_tool"], "parallel_tools");
        assert!(info["mcp"]["transports"]
            .as_array()
            .unwrap()
            .iter()
            .any(|transport| transport == "stdio"));
        assert!(info["mcp"]["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|capability| capability == "prompts"));
        assert_eq!(info["harness"]["phased_parallel_verification"], true);
        assert_eq!(
            info["harness"]["verification_exec_without_risky_flag"],
            true
        );
    }

    #[tokio::test]
    async fn product_scope_status_flows_through_mcp() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/runtime")).unwrap();
        fs::write(
            dir.path().join("src/runtime/control.rs"),
            "pub fn control() {}\n",
        )
        .unwrap();
        let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
        let workspace_id = workspaces.default_id().to_owned();
        let state = AppState {
            auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
            workspaces,
            harness: ToolHarness::new(4).unwrap(),
            monitor: TaskMonitor::new([workspace_id]),
            tasks: TaskRuntime::default(),
        };

        let response = call_tool(&state, json!({"name": "scope_status", "arguments": {}}))
            .await
            .unwrap();
        assert_eq!(response["isError"], false);
        assert_eq!(response["structuredContent"]["source_files"], 1);
        assert_eq!(response["structuredContent"]["mapped_files"], 1);
        assert_eq!(response["structuredContent"]["counts"]["runtime"], 1);
    }

    #[tokio::test]
    async fn bulk_read_isolates_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("one.txt"), "one\n").unwrap();
        let state = AppState {
            auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
            workspaces: Workspaces::new([dir.path()], false, false).unwrap(),
            harness: ToolHarness::new(4).unwrap(),
            monitor: TaskMonitor::new([dir
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()]),
            tasks: TaskRuntime::default(),
        };
        let result = call_tool(
            &state,
            json!({
                "name": "read_files",
                "arguments": {"paths": ["one.txt", "missing.txt"]}
            }),
        )
        .await
        .unwrap();
        let files = result["structuredContent"]["files"].as_array().unwrap();
        assert_eq!(files[0]["ok"], true);
        assert_eq!(files[1]["ok"], false);
    }

    #[tokio::test]
    async fn parallel_fanout_uses_child_slots_without_parent_deadlock() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("one.txt"), "one\n").unwrap();
        fs::write(dir.path().join("two.txt"), "two\n").unwrap();
        let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
        let workspace_id = workspaces.default_id().to_owned();
        let state = AppState {
            auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
            workspaces,
            harness: ToolHarness::new(1).unwrap(),
            monitor: TaskMonitor::new([workspace_id]),
            tasks: TaskRuntime::default(),
        };

        let response = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            call_tool(
                &state,
                json!({
                    "name": "parallel_tools",
                    "arguments": {
                        "tasks": [
                            {"id": "one", "tool": "read_file", "arguments": {"path": "one.txt"}},
                            {"id": "missing", "tool": "read_file", "arguments": {"path": "missing.txt"}},
                            {"id": "two", "tool": "read_file", "arguments": {"path": "two.txt"}}
                        ]
                    }
                }),
            ),
        )
        .await
        .expect("fan-out must not deadlock with one semaphore slot")
        .unwrap();

        let result = &response["structuredContent"];
        assert_eq!(result["execution"], "parallel-fanout");
        assert_eq!(result["tasks"], 3);
        assert_eq!(result["succeeded"], 2);
        assert_eq!(result["failed"], 1);
    }

    #[tokio::test]
    async fn parallel_fanout_runs_writes_and_coalesces_same_file_edits() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("shared.txt"), "same\nmiddle\nsame\n").unwrap();
        let workspaces = Workspaces::new([dir.path()], true, false).unwrap();
        let workspace_id = workspaces.default_id().to_owned();
        let state = AppState {
            auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
            workspaces,
            harness: ToolHarness::new(4).unwrap(),
            monitor: TaskMonitor::new([workspace_id]),
            tasks: TaskRuntime::default(),
        };
        let read = call_tool(
            &state,
            json!({"name":"read_file","arguments":{"path":"shared.txt"}}),
        )
        .await
        .unwrap();
        let sha = read["structuredContent"]["sha256"].as_str().unwrap();

        let response = call_tool(
            &state,
            json!({
                "name":"parallel_tools",
                "arguments":{"tasks":[
                    {"id":"create-a","tool":"create_file","arguments":{"path":"a.txt","content":"a\n"}},
                    {"id":"create-b","tool":"create_file","arguments":{"path":"b.txt","content":"b\n"}},
                    {"id":"edit-first","tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":sha,"edits":[{"old_text":"same","new_text":"FIRST","start_line":1,"end_line":1}]}},
                    {"id":"edit-last","tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":sha,"edits":[{"old_text":"same","new_text":"LAST","start_line":3,"end_line":3}]}}
                ]}
            }),
        )
        .await
        .unwrap();

        let result = &response["structuredContent"];
        assert_eq!(result["succeeded"], 4);
        assert_eq!(result["failed"], 0);
        assert_eq!(result["coalesced_same_file_edits"], 1);
        assert_eq!(
            fs::read_to_string(dir.path().join("shared.txt")).unwrap(),
            "FIRST\nmiddle\nLAST\n"
        );
        assert_eq!(fs::read_to_string(dir.path().join("a.txt")).unwrap(), "a\n");
        assert_eq!(fs::read_to_string(dir.path().join("b.txt")).unwrap(), "b\n");
    }

    #[tokio::test]
    async fn parallel_fanout_orders_real_directory_dependencies() {
        let dir = tempfile::tempdir().unwrap();
        let workspaces = Workspaces::new([dir.path()], true, false).unwrap();
        let workspace_id = workspaces.default_id().to_owned();
        let state = AppState {
            auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
            workspaces,
            harness: ToolHarness::new(4).unwrap(),
            monitor: TaskMonitor::new([workspace_id]),
            tasks: TaskRuntime::default(),
        };

        let response = call_tool(
            &state,
            json!({
                "name":"parallel_tools",
                "arguments":{"tasks":[
                    {"id":"child","tool":"create_file","arguments":{"path":"src/domain/model.rs","content":"pub struct Model;\n"}},
                    {"id":"parent","tool":"create_directory","arguments":{"path":"src/domain"}}
                ]}
            }),
        )
        .await
        .unwrap();

        let result = &response["structuredContent"];
        assert_eq!(result["scheduler"], "dependency-graph");
        assert_eq!(result["dependency_edges"], 1);
        assert_eq!(result["dependency_layers"], 2);
        assert_eq!(result["succeeded"], 2);
        assert!(dir.path().join("src/domain/model.rs").is_file());
    }

    #[tokio::test]
    async fn syntax_index_tools_flow_through_mcp() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("service.rs"),
            "pub struct Service;\nimpl Service { pub fn run(&self) { helper(); } }\nfn helper() {}\n",
        )
        .unwrap();
        let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
        let workspace_id = workspaces.default_id().to_owned();
        let state = AppState {
            auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
            workspaces,
            harness: ToolHarness::new(4).unwrap(),
            monitor: TaskMonitor::new([workspace_id]),
            tasks: TaskRuntime::default(),
        };

        let outline = call_tool(
            &state,
            json!({
                "name": "file_outline",
                "arguments": {"path": "service.rs"}
            }),
        )
        .await
        .unwrap();
        assert_eq!(outline["isError"], false);
        assert_eq!(outline["structuredContent"]["language"], "rust");
        assert!(outline["structuredContent"]["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["qualified_name"] == "Service::run"));

        let search = call_tool(
            &state,
            json!({
                "name": "find_symbol",
                "arguments": {"query": "Service::run"}
            }),
        )
        .await
        .unwrap();
        let symbol_id = search["structuredContent"]["results"][0]["id"]
            .as_str()
            .unwrap();
        let context = call_tool(
            &state,
            json!({
                "name": "symbol_context",
                "arguments": {"symbol_id": symbol_id, "max_body_lines": 50}
            }),
        )
        .await
        .unwrap();
        assert_eq!(context["isError"], false);
        assert!(context["structuredContent"]["body"]["content"]
            .as_str()
            .unwrap()
            .contains("pub fn run"));
        assert!(context["structuredContent"]["syntax_calls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|call| call["name"] == "helper"));
    }

    #[tokio::test]
    async fn positive_harness_tools_flow_through_mcp() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("AGENTS.md"),
            "# Guidance\nRead tests before editing.\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/large_module.rs"),
            "// fixture\n".repeat(2_001),
        )
        .unwrap();
        let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
        let workspace_id = workspaces.default_id().to_owned();
        let state = AppState {
            auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
            workspaces,
            harness: ToolHarness::new(4).unwrap(),
            monitor: TaskMonitor::new([workspace_id.clone()]),
            tasks: TaskRuntime::default(),
        };

        let context = call_tool(&state, json!({"name": "project_context", "arguments": {}}))
            .await
            .unwrap();
        assert_eq!(context["isError"], false);
        assert_eq!(context["structuredContent"]["workspace"], workspace_id);
        assert!(context["structuredContent"]["project_types"]
            .as_array()
            .unwrap()
            .iter()
            .any(|kind| kind == "rust"));
        assert_eq!(
            context["structuredContent"]["guidance"][0]["path"],
            "AGENTS.md"
        );
        assert!(context["structuredContent"]["conventions"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["code"] == "oversized-source-module"));

        let agent = call_tool(
            &state,
            json!({
                "name": "agent_context",
                "arguments": {"query": "large module", "budget": 1000}
            }),
        )
        .await
        .unwrap();
        assert_eq!(agent["isError"], false);
        assert!(agent["structuredContent"].get("estimated_tokens").is_none());
        assert!(agent["structuredContent"].get("timing").is_none());
        assert!(agent["structuredContent"]
            .get("context_bytes_avoided")
            .is_none());
        let telemetry = &agent["_meta"]["dev.wcode/agentContextTelemetry"];
        assert!(telemetry["model_estimated_tokens"]
            .as_u64()
            .is_some_and(|tokens| tokens <= 1_000));
        assert!(telemetry["context_bytes_avoided"].as_u64().unwrap() > 0);
        assert!(telemetry["timing"].is_object());
        assert!(agent["content"][0]["text"]
            .as_str()
            .is_some_and(|text| !text.contains("context_reduction_percent")));
        assert!(agent["structuredContent"]["checks"]
            .as_array()
            .is_some_and(|checks| !checks.is_empty()));

        let convention = call_tool(
            &state,
            json!({"name": "convention_status", "arguments": {}}),
        )
        .await
        .unwrap();
        assert_eq!(convention["isError"], false);
        assert_eq!(
            convention["structuredContent"]["policies"]
                .as_array()
                .unwrap()
                .len(),
            22
        );
        assert!(convention["structuredContent"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["code"] == "oversized-source-module"));

        let verification = call_tool(
            &state,
            json!({"name": "verify_project", "arguments": {"level": "quick"}}),
        )
        .await
        .unwrap();
        assert_eq!(verification["isError"], true);
        assert!(verification["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("requires command execution"));
    }

    #[test]
    fn parallel_fanout_rejects_oversized_child_results() {
        let response = mcp_tools::tool_result(
            json!({"content": "x".repeat(MAX_PARALLEL_FANOUT_ITEM_BYTES + 1)}),
            false,
        );
        let (item, bytes) =
            parallel_item_from_response("large".to_owned(), "read_file".to_owned(), response);
        assert_eq!(item["ok"], false);
        assert!(item["error"]
            .as_str()
            .unwrap()
            .contains("fan-out item limit"));
        assert!(bytes < MAX_PARALLEL_FANOUT_ITEM_BYTES);
    }

    #[test]
    fn batch_limits_reject_empty_and_oversized_requests() {
        assert_eq!(
            mcp_tools::batch_validation_error(0).unwrap()["error"]["code"],
            -32600
        );
        assert!(mcp_tools::batch_validation_error(1).is_none());
        assert!(mcp_tools::batch_validation_error(MAX_BATCH_ITEMS).is_none());
        assert_eq!(
            mcp_tools::batch_validation_error(MAX_BATCH_ITEMS + 1).unwrap()["error"]["code"],
            -32600
        );
    }

    #[test]
    fn tool_list_exposes_bulk_index_and_positive_harness_tools() {
        let catalog = mcp_tools::tools();
        let names = catalog
            .iter()
            .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        let context_tool = catalog
            .iter()
            .find(|tool| tool["name"] == "software_context")
            .unwrap();
        let context_scopes = context_tool["_meta"]["dev.wcode/productScopes"]
            .as_array()
            .unwrap();
        for expected_scope in ["design", "graph", "semantics", "traceability", "risk"] {
            assert!(context_scopes.iter().any(|scope| scope == expected_scope));
        }
        let agent_tool = catalog
            .iter()
            .find(|tool| tool["name"] == "agent_context")
            .unwrap();
        let agent_scopes = agent_tool["_meta"]["dev.wcode/productScopes"]
            .as_array()
            .unwrap();
        for expected_scope in [
            "design",
            "graph",
            "traceability",
            "verification",
            "workspace",
        ] {
            assert!(agent_scopes.iter().any(|scope| scope == expected_scope));
        }
        assert!(SERVER_INSTRUCTIONS.contains("call agent_context first"));
        assert!(SERVER_INSTRUCTIONS.len() < 1_000);
        let read_tool = catalog
            .iter()
            .find(|tool| tool["name"] == "read_file")
            .unwrap();
        assert_eq!(
            read_tool["_meta"]["dev.wcode/productScopes"],
            json!(["workspace"])
        );
        for expected in [
            "design_status",
            "convention_status",
            "scope_status",
            "design_init",
            "software_graph",
            "graph_provider_import",
            "graph_provider_status",
            "semantic_provider_status",
            "language_quality_status",
            "language_quality_run",
            "semantic_provider_refresh",
            "graph_history",
            "graph_query",
            "graph_diff",
            "traceability_status",
            "drift_status",
            "risk_status",
            "impact_analysis",
            "software_context",
            "agent_context",
            "semantic_status",
            "semantic_query",
            "semantic_record",
            "semantic_confirm",
            "semantic_retire",
            "verification_plan",
            "verification_claim",
            "verification_submit",
            "verification_executor_status",
            "verification_execute_stages",
            "verification_stage_submit",
            "verification_approve",
            "verification_status",
            "verification_history",
            "evidence_status",
            "reconciliation_plan",
            "reconciliation_status",
            "reconciliation_history",
            "reconciliation_execution_status",
            "reconciliation_claim",
            "reconciliation_submit",
            "reconciliation_retry",
            "read_files",
            "path_info",
            "replace_text",
            "apply_edits",
            "write_file",
            "create_directory",
            "create_file",
            "create_files",
            "apply_file_edits",
            "move_path",
            "move_paths",
            "delete_path",
            "search_many",
            "file_outline",
            "find_symbol",
            "symbol_context",
            "project_context",
            "review_changes",
            "verify_project",
            "parallel_tools",
        ] {
            assert!(
                names.contains(&expected.to_owned()),
                "missing MCP tool {expected}"
            );
        }
    }
}
