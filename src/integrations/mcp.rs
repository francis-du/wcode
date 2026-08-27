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
#[cfg(test)]
use crate::scopes;
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
use tokio::task::JoinSet;

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
                        format!("batch task failed: {error}"),
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
        handle_message(state, payload, &protocol, &owner).await
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

const SERVER_INSTRUCTIONS: &str = "Work only inside configured workspace roots. Start substantial coding work with workspace_info, design_status, project_context, and language_quality_status when language-specific quality gates matter. Treat always-on guidance as a short map and retrieve detailed Design State, Product Scope, symbol, semantic, and language-quality context on demand. When Design State is initialized, prefer software_context for the requested requirement, behavior, or subsystem before broad source reads, and use traceability_status when you need the Requirement -> Component -> implementation/test chain. semantic_record may capture user/design/conversation/provider candidates, but candidates are non-authoritative; only call semantic_confirm or semantic_retire after explicit human confirmation. Tree-sitter graph and symbol relationships are syntax precision, not compiler semantics. Use semantic_provider_status / semantic_provider_refresh to auto-discover first-party LSP semantic providers across all indexed languages; language support is a capability matrix rather than a boolean. Prefer repository-declared or language-native quality providers and use language_quality_run only for entries reported as declared/available/check-only; it preserves the normal authorization boundary, never runs formatter fix/write mode, and records current-revision Evidence. External SCIP/compiler/runtime graph facts may also enter through graph_provider_import. Every provider must retain its real precision and revision; never relabel syntax facts as semantic. Deletion is restricted rather than unrestricted: delete_path only removes a regular file or empty directory after an exact one-shot human approval in the TUI or protected local WebUI; recursive deletion, workspace-root deletion, protected credential/VCS paths, symlink aliases, and hard-linked files remain blocked. Overlapping roots are rejected by default, and destructive replacements retain their safety policy. Do not try to bypass the selected workspace through absolute paths, parent traversal, command options, shell interpreters, or package-script redirection. Git mutation is restricted to exact human-approved add/commit/push shapes; force/delete/mirror and other mutation subcommands remain blocked. run_command uses direct program+argument execution without a shell. A small safe command set is pre-authorized; when a model requests another valid bare executable name, wcode creates a per-Workspace CommandAccess authorization request and the operator must explicitly approve it in the TUI or protected WebUI before retry. Repository-aware argument shapes can still require a separate exact RiskyExecution approval because project metadata can redirect reads or execute code. verify_project uses a separate exact-shape verification lane: only Harness-inferred quality checks are eligible, so normal check/test/Clippy/build gates can run without the global risky flag while arbitrary command arguments remain blocked. Read relevant implementation and tests before editing. Prefer find_symbol, file_outline, and symbol_context when navigating supported source languages so the model receives precise syntax ranges instead of broad file dumps. Prefer read_files or search_many when one bulk operation can answer the question efficiently; when two or more independent read/discovery operations are already known, use parallel_tools so they consume separate semaphore slots and appear separately in the monitor. Batch known mutations too: use one apply_edits for multiple changes in one file, apply_file_edits for independent existing files, and create_files for independent new files. Keep dependent edits sequential, use sha256 preconditions, and never parallelize operations whose inputs depend on earlier results. After changes, call review_changes first. When the selected workspace is a Git root with command execution enabled, follow it with drift_status, impact_analysis, and risk_status; create reconciliation_plan when drift or traceability gaps remain. Then run verify_project at the recommended level and inspect evidence_status. verification_plan/verification_claim/verification_submit can add blind independent model review; disagreement is evidence and must not be hidden by majority voting. Required property/mutation/fuzz/runtime stages stay blocked until real stage Evidence exists. Prefer verification_executor_status / verification_execute_stages for configured or auto-discovered cross-language runners, or verification_stage_submit for an external system; never fabricate stage success. Reconciliation Plans have durable execution state: use reconciliation_execution_status, reconciliation_claim, reconciliation_submit, and reconciliation_retry to advance dependency-safe Design/Implementation/Review tasks. Source edits must still go through the normal workspace write tools with their SHA/path safeguards; Verification and HumanApproval tasks are system evidence gates and must never be manually marked complete. Report checks actually run, evidence produced, open disagreements, and any failures or assumptions that remain.";

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
            #[cfg(test)]
            debug_assert_eq!(
                catalog,
                tool_catalog_golden_snapshot(),
                "canonical MCP tool catalog drifted from the test-only golden contract"
            );
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

// Test-only golden contract retained during the catalog extraction. Production discovery
// compiles and executes only `mcp_tools::tools()`.
#[cfg(test)]
fn tool_catalog_golden_snapshot() -> Vec<Value> {
    vec![
        tool("workspace_info", "Show configured workspace IDs, roots, capabilities, scheduling guidance, and the active security policy. Inspect this before selecting a workspace or command strategy.", json!({"type":"object","properties":{},"additionalProperties":false}), true, false),
        tool("design_status", "Load and validate the structured Desired Software State under .wcode/. Returns project identity, requirement/component/constraint/decision/acceptance counts, and bounded diagnostics without reading implementation source into the model context.", schema(json!({}), &[]), true, false),
        tool("convention_status", "Inspect cross-language convention policies and repository architecture findings through the Harness. Reports file naming, architecture-domain classification, unclassified root source files, oversized modules, flat Rust domain growth, detected languages, bounded counts, and truncation state.", schema(json!({}), &[]), true, false),
        tool("scope_status", "Audit the current repository against wcode's canonical Product Scope registry. Returns per-scope source counts, mapped and unmapped source totals, bounded unmapped paths, and the same scope registry used by context retrieval, semantics, MCP metadata, and conventions.", schema(json!({}), &[]), true, false),
        tool("design_init", "Initialize sparse structured Design State for an uninitialized workspace. Creates .wcode/project.yaml and design/product.yaml; requirement/component/constraint/acceptance/decision collections remain absent until meaningful content exists. Existing design state is never overwritten.", schema(json!({"name":{"type":"string","minLength":1,"maxLength":200},"description":{"type":"string","maxLength":1000}}), &[]), false, false),
        tool("software_graph", "Build and persist a bounded composite Software Graph from declared Design State, Tree-sitter syntax facts, and the latest imported external semantic/runtime provider facts. Every edge retains its own provider/precision/revision; syntax facts are never promoted to compiler semantics.", schema(json!({"path":{"type":"string","default":"."},"max_files":{"type":"integer","minimum":1,"maximum":5000,"default":500},"max_symbols":{"type":"integer","minimum":1,"maximum":5000,"default":1000}}), &[]), true, false),
        tool("graph_provider_import", "Persist one bounded external Software Graph provider revision. This is the provider-neutral adapter for SCIP/LSP/compiler/runtime indexers: the external producer supplies nodes/edges plus provider, precision and revision, and wcode overlays the latest revision without pretending Tree-sitter produced semantic facts.", schema(json!({"provider_graph":{"type":"object","properties":{"provider":{"type":"string","minLength":1,"maxLength":128},"precision":{"type":"string","enum":["semantic","runtime","deterministic","heuristic"]},"revision":{"type":"string","minLength":1,"maxLength":256},"nodes":{"type":"array","maxItems":10000,"items":{"type":"object","properties":{"id":{"type":"string","minLength":1,"maxLength":512},"kind":{"type":"string","enum":["product","requirement","acceptance_criterion","constraint","decision","component","package","module","file","symbol","function","struct","trait","class","interface","api","database","queue","config","test","verification","risk","evidence"]},"label":{"type":"string","minLength":1,"maxLength":500},"attributes":{"type":"object"}},"required":["id","kind","label"],"additionalProperties":false}},"edges":{"type":"array","maxItems":50000,"items":{"type":"object","properties":{"from":{"type":"string","minLength":1,"maxLength":512},"to":{"type":"string","minLength":1,"maxLength":512},"kind":{"type":"string","enum":["contains","defines","references","calls","imports","depends_on","implements","extends","implements_requirement","constrained_by","tested_by","verified_by","guards_against","produces_evidence","runtime_calls","conflicts_with"]}},"required":["from","to","kind"],"additionalProperties":false}}},"required":["provider","precision","revision"],"additionalProperties":false}}), &["provider_graph"]), false, false),
        tool("graph_provider_status", "List the latest persisted external semantic/runtime graph provider revisions for the selected workspace, including precision, revision, node/edge counts, and import time.", schema(json!({}), &[]), true, false),
        tool("semantic_provider_status", "Auto-detect source languages and first-party LSP semantic providers for every language supported by wcode's syntax index. Reports provider availability, policy readiness, and honest syntax fallback when no semantic server is available.", schema(json!({}), &[]), true, false),
        tool("language_quality_status", "Inspect the per-language capability matrix across syntax, semantic providers, repository-declared formatter/linter/type/static/test/security providers, and advanced Property/Mutation/Fuzz/Runtime stages. Support is reported by dimension and explicit gaps rather than one boolean.", schema(json!({}), &[]), true, false),
        tool("language_quality_run", "Run one repository-declared, available, check-only language quality provider through wcode's trusted runtime authorization boundary and persist current-revision Evidence. This lane never invokes formatter fix/write modes.", schema(json!({"language":{"type":"string","enum":["bash","c","cpp","c-sharp","css","dart","elixir","go","html","java","java-script","lua","ocaml","ocaml-interface","php","python","r","ruby","rust","swift","type-script","tsx"]},"provider_id":{"type":"string","minLength":1,"maxLength":160},"timeout_seconds":{"type":"integer","minimum":1,"maximum":300,"default":120}}), &["language","provider_id"]), false, false),
        tool("semantic_provider_refresh", "Run the detected first-party LSP semantic providers for the selected workspace and persist real semantic Document Symbol / Call Hierarchy facts into the Software Graph. Requires --allow-risky-exec because language servers load repository-controlled project configuration.", schema(json!({"path":{"type":"string","default":"."},"max_files":{"type":"integer","minimum":1,"maximum":256,"default":128},"max_symbols":{"type":"integer","minimum":1,"maximum":2000,"default":1000}}), &[]), false, false),
        tool("graph_history", "List bounded persisted composite Software Graph snapshots. Identical graph content is deduplicated, so history represents meaningful graph revisions rather than read frequency.", schema(json!({"limit":{"type":"integer","minimum":1,"maximum":64,"default":20}}), &[]), true, false),
        tool("graph_query", "Query a persisted Software Graph snapshot by node id/kind/label or by incoming/outgoing relationship. Omit snapshot_id to query the latest snapshot; results remain bounded and include the snapshot/provider precision metadata.", schema(json!({"query":{"type":"object","properties":{"snapshot_id":{"type":"string","minLength":1,"maxLength":160},"node_id":{"type":"string","minLength":1,"maxLength":512},"kind":{"type":"string","enum":["product","requirement","acceptance_criterion","constraint","decision","component","package","module","file","symbol","function","struct","trait","class","interface","api","database","queue","config","test","verification","risk","evidence"]},"label_contains":{"type":"string","minLength":1,"maxLength":500},"related_to":{"type":"string","minLength":1,"maxLength":512},"edge_kind":{"type":"string","enum":["contains","defines","references","calls","imports","depends_on","implements","extends","implements_requirement","constrained_by","tested_by","verified_by","guards_against","produces_evidence","runtime_calls","conflicts_with"]},"direction":{"type":"string","enum":["incoming","outgoing","both"]},"limit":{"type":"integer","minimum":1,"maximum":500,"default":100}},"additionalProperties":false}}), &["query"]), true, false),
        tool("graph_diff", "Compare two persisted Software Graph revisions without treating provenance revision churn as delete/add noise. Node IDs and stable edge identities are aligned first; true additions/removals and changed attributes/provenance are returned separately with bounded counts. Omit IDs to compare the latest two meaningful graph snapshots.", schema(json!({"diff":{"type":"object","properties":{"from_snapshot_id":{"type":"string","minLength":1,"maxLength":160},"to_snapshot_id":{"type":"string","minLength":1,"maxLength":160},"limit":{"type":"integer","minimum":1,"maximum":200,"default":50}},"additionalProperties":false}}), &[]), true, false),
        tool("traceability_status", "Resolve Requirement → Component → implementation and Acceptance Criterion → verification chains from structured Design State. File existence is deterministic; symbol/test resolution uses Tree-sitter syntax precision; Harness check references resolve only when present in the inferred project verification profile. Returns separate coverage dimensions rather than one health score.", schema(json!({}), &[]), true, false),
        tool("drift_status", "Compare the current Git change set with Design State traceability and report bounded implementation drift and design drift findings. The result distinguishes desired-state changes that are not reflected in Actual State from design-mapped implementation changes that have no corresponding Design State change.", schema(json!({"timeout_seconds":{"type":"integer","minimum":1,"maximum":120,"default":30}}), &[]), true, false),
        tool("risk_status", "Assess the current change set, traceability gaps, and drift findings into structured Risk records and a risk-adaptive verification profile. Risk is multi-dimensional evidence for verification depth, not a single quality score.", schema(json!({"timeout_seconds":{"type":"integer","minimum":1,"maximum":120,"default":30}}), &[]), true, false),
        tool("impact_analysis", "Map the current Git change set through Design State to impacted components, requirements, acceptance criteria, declared implementation symbols, public-API signals, security boundaries, and overall risk. This is conservative impact analysis; Tree-sitter relationships remain syntax precision.", schema(json!({"timeout_seconds":{"type":"integer","minimum":1,"maximum":120,"default":30}}), &[]), true, false),
        tool("software_context", "Retrieve bounded task-oriented software intelligence: matching requirements, components, constraints, scoped confirmed semantics, syntax-level symbols, known risks, and traceability coverage. Optional scopes accept canonical wcode Product Scopes (design, graph, semantics, traceability, risk, verification, evidence, reconciliation, workspace, integrations, runtime, experience) or freeform business scopes; recognized product scopes narrow source navigation to the relevant subsystem.", schema(json!({"query":{"type":"string","minLength":1,"maxLength":1000},"intent":{"type":"string","minLength":1,"maxLength":128,"default":"inspect"},"budget":{"type":"integer","minimum":1000,"maximum":64000,"default":12000},"scopes":{"type":"array","maxItems":32,"items":{"type":"string","minLength":1,"maxLength":300}}}), &["query"]), true, false),
        tool("semantic_status", "Read the persistent workspace semantic registry. Candidate facts are non-authoritative conversation/provider/user proposals; only explicitly confirmed facts are used as authoritative query expansion, and retired facts are excluded.", schema(json!({"limit":{"type":"integer","minimum":1,"maximum":500,"default":50}}), &[]), true, false),
        tool("semantic_query", "Search the persistent semantic registry by canonical term, alias, description, scope, or relationship triple. Optional scopes now act as real filters: scoped facts must overlap a requested scope while unscoped facts remain global. Canonical wcode Product Scope aliases are normalized alongside freeform business scopes.", schema(json!({"query":{"type":"string","minLength":1,"maxLength":1000},"scopes":{"type":"array","maxItems":32,"items":{"type":"string","minLength":1,"maxLength":300}},"include_candidates":{"type":"boolean","default":true},"limit":{"type":"integer","minimum":1,"maximum":100,"default":20}}), &["query"]), true, false),
        tool("semantic_record", "Record a persistent semantic candidate without making it authoritative. Use this for user-proposed, design-derived, conversation-learned, or external-provider semantic facts; candidates never auto-promote into confirmed semantics.", schema(json!({"fact":{"type":"object","properties":{"kind":{"type":"string","enum":["concept","alias","entity","metric","dimension","relationship","rule","domain_term"]},"canonical":{"type":"string","minLength":1,"maxLength":300},"aliases":{"type":"array","maxItems":32,"items":{"type":"string","minLength":1,"maxLength":300}},"description":{"type":"string","minLength":1,"maxLength":2000},"scopes":{"type":"array","maxItems":32,"items":{"type":"string","minLength":1,"maxLength":300}},"subject":{"type":"string","minLength":1,"maxLength":512},"predicate":{"type":"string","minLength":1,"maxLength":256},"object":{"type":"string","minLength":1,"maxLength":512},"origin":{"type":"string","enum":["user","conversation","design","provider"]},"provider":{"type":"string","minLength":1,"maxLength":256},"confidence":{"type":"string","enum":["low","medium","high"]},"source":{"type":"string","minLength":1,"maxLength":1000}},"required":["kind","canonical","description","origin","confidence"],"additionalProperties":false}}), &["fact"]), false, false),
        tool("semantic_confirm", "Promote one semantic candidate to confirmed authoritative workspace semantics. Only call after explicit human confirmation; confirmed=true and an attestation identity are required. Conversation/model candidates must never self-promote.", schema(json!({"fact_id":{"type":"string","minLength":1,"maxLength":160},"attested_by":{"type":"string","minLength":1,"maxLength":256},"confirmed":{"type":"boolean","const":true}}), &["fact_id","attested_by","confirmed"]), false, false),
        tool("semantic_retire", "Retire one semantic fact through a new persistent revision. Only call after explicit human confirmation; retired facts stop affecting software_context expansion but remain auditable in history.", schema(json!({"fact_id":{"type":"string","minLength":1,"maxLength":160},"attested_by":{"type":"string","minLength":1,"maxLength":256},"confirmed":{"type":"boolean","const":true}}), &["fact_id","attested_by","confirmed"]), false, false),
        tool("verification_plan", "Create a risk-adaptive Verification Plan for the current change set. The plan selects deterministic verification depth and creates blind independent reviewer jobs without binding wcode to a model provider.", schema(json!({"timeout_seconds":{"type":"integer","minimum":1,"maximum":120,"default":30}}), &[]), false, false),
        tool("verification_claim", "Claim one queued blind Verification Job whose required capabilities match the reviewer. The job does not expose other reviewer submissions, preserving independent first-pass review, and carries bounded role-specific guidance when that role has a shared review rubric.", schema(json!({"reviewer":{"type":"string","minLength":1,"maxLength":256},"capabilities":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"string","minLength":1,"maxLength":128}},"role":{"type":"string","enum":["design_compliance","correctness","maintainability","architecture","security","performance","compatibility","adversarial","test_synthesis"]}}), &["reviewer","capabilities"]), false, false),
        tool("verification_submit", "Submit a structured verdict for a claimed Verification Job. The submission is converted into persistent provenance-bearing model-review Evidence including summary, claims, risks, and model identity.", schema(json!({"job_id":{"type":"string","minLength":1,"maxLength":160},"reviewer":{"type":"string","minLength":1,"maxLength":256},"submission":{"type":"object","properties":{"verdict":{"type":"string","enum":["pass","fail","inconclusive"]},"summary":{"type":"string","minLength":1,"maxLength":2000},"claims":{"type":"array","maxItems":32,"items":{"type":"string","minLength":1,"maxLength":1000}},"risks":{"type":"array","maxItems":32,"items":{"type":"string","minLength":1,"maxLength":1000}},"model":{"type":"string","minLength":1,"maxLength":256}},"required":["verdict","summary"],"additionalProperties":false}}), &["job_id","reviewer","submission"]), false, false),
        tool("verification_executor_status", "Inspect the cross-language Property/Mutation/Fuzz/Runtime executor registry. wcode auto-discovers common framework runners and also accepts bounded no-shell executors in .wcode/executors.yaml, so every indexed language can plug into the same Verification Mesh.", schema(json!({}), &[]), true, false),
        tool("verification_execute_stages", "Execute all currently required Property/Mutation/Fuzz/Runtime stages for one Verification Plan using the first matching configured or auto-discovered executor. Each real command result becomes persistent stage Evidence. Requires --allow-risky-exec because project tests and configured executors run repository-controlled code.", schema(json!({"plan_id":{"type":"string","minLength":1,"maxLength":160}}), &["plan_id"]), false, false),
        tool("verification_stage_submit", "Submit real Property, Mutation, Fuzz, or Runtime/Canary stage evidence for a Verification Plan. This is the provider-neutral execution adapter: external test systems or agents submit their actual result and artifact digest; verification_status keeps the latest result per producer and aggregates the stage fail-closed, so another producer's later Pass cannot mask a Fail.", schema(json!({"plan_id":{"type":"string","minLength":1,"maxLength":160},"submission":{"type":"object","properties":{"stage":{"type":"string","enum":["property","mutation","fuzz","runtime_canary"]},"producer":{"type":"string","minLength":1,"maxLength":256},"verdict":{"type":"string","enum":["pass","fail","inconclusive"]},"summary":{"type":"string","minLength":1,"maxLength":2000},"artifact_digest":{"type":"string","minLength":1,"maxLength":512},"model":{"type":"string","minLength":1,"maxLength":256}},"required":["stage","producer","verdict","summary","artifact_digest"],"additionalProperties":false}}), &["plan_id","submission"]), false, false),
        tool("verification_approve", "Record explicit human approval as persistent HumanApproval Evidence for a Verification Plan that requires it. Only call this after a human has explicitly approved the plan; confirmed=true is required and models must never self-approve.", schema(json!({"plan_id":{"type":"string","minLength":1,"maxLength":160},"approver":{"type":"string","minLength":1,"maxLength":256},"statement":{"type":"string","minLength":1,"maxLength":2000},"confirmed":{"type":"boolean","const":true}}), &["plan_id","approver","statement","confirmed"]), false, false),
        tool("verification_status", "Read one Verification Plan's durable reviewer state and readiness gate: deterministic result, stage evidence, queued/claimed/submitted jobs, reviewer failures/inconclusive/disagreement, human approval, stale-revision blockers, and final ready state. The plan must belong to the selected workspace.", schema(json!({"plan_id":{"type":"string","minLength":1,"maxLength":160}}), &["plan_id"]), true, false),
        tool("verification_history", "List recent persisted Verification Plans with their current readiness, evidence-stage results, reviewer state, human approval, and blockers. This survives wcode restarts.", schema(json!({"limit":{"type":"integer","minimum":1,"maximum":100,"default":20}}), &[]), true, false),
        tool("evidence_status", "Read bounded verification Evidence accumulated in this runtime, optionally filtered by subject. Deterministic checks and model-review evidence retain producer, revision, confidence, policy, and result provenance.", schema(json!({"subject":{"type":"string","minLength":1,"maxLength":512},"limit":{"type":"integer","minimum":1,"maximum":500,"default":50}}), &[]), true, false),
        tool("reconciliation_plan", "Create and persist a bounded Reconciliation Plan from current Design State, Git Actual State, drift, transitive syntax impact, risk, Change IR intents, implementation tasks, and a risk-adaptive Verification Plan. This plans convergence; it does not automatically apply source edits.", schema(json!({"timeout_seconds":{"type":"integer","minimum":1,"maximum":120,"default":30}}), &[]), false, false),
        tool("reconciliation_status", "Load one persisted Reconciliation Plan by ID for the selected workspace. Plans survive wcode restarts and can be handed to a different model executor.", schema(json!({"plan_id":{"type":"string","minLength":1,"maxLength":160}}), &["plan_id"]), true, false),
        tool("reconciliation_history", "List the most recent persisted Reconciliation Plans for the selected workspace.", schema(json!({"limit":{"type":"integer","minimum":1,"maximum":100,"default":10}}), &[]), true, false),
        tool("reconciliation_execution_status", "Read the durable execution state for one Reconciliation Plan. Safe implementation/design/review tasks are dependency-aware and claimable by model executors; Verification and HumanApproval tasks advance only from real verification/human evidence. converged=true means every plan task completed without a failed task.", schema(json!({"plan_id":{"type":"string","minLength":1,"maxLength":160}}), &["plan_id"]), true, false),
        tool("reconciliation_claim", "Claim one currently runnable Design, Implementation, or Review task from a persisted Reconciliation execution. Dependency order is enforced and system Verification/HumanApproval tasks cannot be claimed by models.", schema(json!({"plan_id":{"type":"string","minLength":1,"maxLength":160},"executor":{"type":"string","minLength":1,"maxLength":256},"kinds":{"type":"array","maxItems":3,"items":{"type":"string","enum":["design","implementation","review"]}}}), &["plan_id","executor"]), false, false),
        tool("reconciliation_submit", "Complete or fail one claimed Reconciliation task. The executor identity must match the claimant; the result is persisted in execution history and also emitted as provenance-bearing Reconciliation Evidence.", schema(json!({"plan_id":{"type":"string","minLength":1,"maxLength":160},"task_id":{"type":"string","minLength":1,"maxLength":160},"executor":{"type":"string","minLength":1,"maxLength":256},"submission":{"type":"object","properties":{"success":{"type":"boolean"},"summary":{"type":"string","minLength":1,"maxLength":2000},"artifact_digest":{"type":"string","minLength":1,"maxLength":512}},"required":["success","summary"],"additionalProperties":false}}), &["plan_id","task_id","executor","submission"]), false, false),
        tool("reconciliation_retry", "Requeue one failed model-executable Reconciliation task. This never bypasses dependencies or retries Verification/HumanApproval system gates; it only resets a failed Design/Implementation/Review task so another executor can claim it.", schema(json!({"plan_id":{"type":"string","minLength":1,"maxLength":160},"task_id":{"type":"string","minLength":1,"maxLength":160}}), &["plan_id","task_id"]), false, false),
        tool("project_context", "Build a bounded, cached coding context for one workspace: repository guidance excerpts, detected project types and manifests, recommended quality checks, and a preferred change workflow. Call this before substantial coding work.", schema(json!({}), &[]), true, false),
        tool(
            "review_changes",
            "Review the current Git change set before verification. Runs bounded Git status, diff-check, and numstat probes in parallel; classifies changed files; adds maintainability signals for 1k-line threshold crossings, concentrated source growth, and cross-Product-Scope churn; and recommends quick or full verification.",
            schema(json!({"timeout_seconds":{"type":"integer","minimum":1,"maximum":120,"default":30}}), &[]),
            true,
            false,
        ),
        tool(
            "parallel_tools",
            "Schedule 2-128 bounded read/discovery operations or workspace file writes. Every child uses a real global semaphore slot and appears separately in the TUI. Same-file apply_edits with the same SHA are coalesced into one atomic commit; the resource dependency graph fans out independent tasks and orders overlapping read/write, parent/child, move, delete, and directory-creation dependencies.",
            json!({
                "type": "object",
                "properties": {
                    "tasks": {
                        "type": "array",
                        "minItems": 2,
                        "maxItems": MAX_PARALLEL_FANOUT_ITEMS,
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": {"type": "string"},
                                "tool": {"type": "string", "enum": PARALLEL_READ_TOOLS.iter().chain(PARALLEL_WRITE_TOOLS.iter()).copied().collect::<Vec<_>>()},
                                "arguments": {"type": "object"}
                            },
                            "required": ["tool"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["tasks"],
                "additionalProperties": false
            }),
            false,
            true,
        ),
        tool("verify_project", "Run exact Harness-inferred quality checks with bounded, phased parallelism. This dedicated verification lane may execute approved check/test/Clippy/build shapes without --allow-risky-exec; arbitrary model-facing run_command calls remain under the stricter trust policy. Independent checks in the same phase use separate semaphore slots; tests, Clippy, and builds are sequenced to reduce compiler-cache contention.", schema(json!({"level":{"type":"string","enum":["quick","full"],"default":"quick"},"timeout_seconds":{"type":"integer","minimum":1,"maximum":300,"default":120}}), &[]), false, false),
        tool("list_files", "Fast recursive file listing inside one workspace root. All regular files are visible except protected credential, repository-control, and wcode-internal paths; symlinks are not followed.", schema(json!({"path":{"type":"string"},"max_entries":{"type":"integer","minimum":1,"maximum":10000,"default":2000}}), &[]), true, false),
        tool("search_code", "Fast exact-substring search in one workspace. File scanning runs off the async runtime and uses parallel workers.", schema(json!({"query":{"type":"string"},"path":{"type":"string"},"max_results":{"type":"integer","minimum":1,"maximum":500}}), &["query"]), true, false),
        tool("search_many", "Search up to 32 exact substrings in one filesystem traversal. Prefer this over repeated search_code calls when looking for several symbols.", schema(json!({"queries":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"string"}},"path":{"type":"string"},"max_results":{"type":"integer","minimum":1,"maximum":1000}}), &["queries"]), true, false),
        tool(
            "file_outline",
            "Parse one supported source file with Tree-sitter and return syntax-level definitions, qualified names, exact ranges, redacted signatures, total/returned symbol counts, parse status, and cache metadata. Supports Bash, C, C++, C#, CSS, Dart, Elixir, Go, HTML, Java, JavaScript, Lua, OCaml/interfaces, PHP, Python, R, Ruby, Rust, Swift, and TypeScript/TSX. HTML indexes id-bearing elements and custom components; CSS indexes selectors, custom properties, and keyframes.",
            schema(json!({
                "path": {"type": "string"},
                "max_symbols": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 500}
            }), &["path"]),
            true,
            false,
        ),
        tool(
            "find_symbol",
            "Find syntax-level symbol definitions by name or qualified name across a file or directory. Results include opaque symbol IDs for symbol_context, provider/precision metadata, exact ranges, redacted signatures, and language. IDs are tied to the current indexed revision, so query again after edits. The index is lazy and parallel.",
            schema(json!({
                "query": {"type": "string"},
                "path": {"type": "string", "default": "."},
                "kind": {"type": "string", "description": "Optional Tree-sitter tag kind such as function, method, class, interface, module, or type."},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 200, "default": 50}
            }), &["query"]),
            true,
            false,
        ),
        tool(
            "symbol_context",
            "Expand a symbol ID returned by file_outline or find_symbol into bounded source, syntax-level calls, same-file call targets, nested definitions, parse status, and in-memory AST cache metadata.",
            schema(json!({
                "symbol_id": {"type": "string"},
                "max_body_lines": {"type": "integer", "minimum": 1, "maximum": 500, "default": 200}
            }), &["symbol_id"]),
            true,
            false,
        ),
        tool("read_file", "Read one UTF-8 file with line bounds and receive its SHA-256 edit precondition.", schema(json!({"path":{"type":"string"},"start_line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":1}}), &["path"]), true, false),
        tool("read_files", "Read up to 32 UTF-8 files in one MCP round trip. Reads run in parallel and each file reports success or failure independently.", schema(json!({"paths":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"string"}},"start_line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":1}}), &["paths"]), true, false),
        tool("read_media", "Inspect one bounded workspace media file. Metadata is always safe to return. Set include_content=true only when the MCP client explicitly advertises the run.francis.wcode/media-content extension for the media kind; otherwise wcode fails closed without emitting image/audio payloads. PNG/JPEG/GIF/WebP image content and MP3/WAV/Ogg/FLAC audio content are supported; MP4/WebM are metadata-only.", schema(json!({"path":{"type":"string"},"include_content":{"type":"boolean","default":false}}), &["path"]), true, false),
        tool("path_info", "Inspect one workspace path without loading the whole file into model context. Returns type, size, SHA-256 for files, readonly state, modification time, and hard-link count when available.", schema(json!({"path":{"type":"string"}}), &["path"]), true, false),
        tool("replace_text", "Atomically replace one exact text occurrence with a SHA-256 precondition and optional 1-based original line bounds. When start_line/end_line are supplied together, old_text must match exactly once inside that original range. Protected/symlink/hard-link targets remain blocked.", schema(json!({"path":{"type":"string"},"old_text":{"type":"string"},"new_text":{"type":"string"},"expected_sha256":{"type":"string"},"start_line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":1}}), &["path","old_text","new_text","expected_sha256"]), false, true),
        tool("apply_edits", "Atomically apply up to 128 non-overlapping edits against one original SHA revision. Each edit may add 1-based start_line/end_line bounds; all edits resolve against the same original bytes before one atomic commit, so line shifts from sibling edits cannot affect targeting.", schema(json!({"path":{"type":"string"},"expected_sha256":{"type":"string"},"edits":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"object","properties":{"old_text":{"type":"string","minLength":1},"new_text":{"type":"string"},"start_line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":1}},"required":["old_text","new_text"],"additionalProperties":false}}}), &["path","expected_sha256","edits"]), false, true),
        tool("write_file", "Atomically write a complete UTF-8 file. Creating a new file requires no hash; overwriting an existing file requires expected_sha256 and preserves protected-path, symlink, hard-link, and destructive-replacement safeguards.", schema(json!({"path":{"type":"string"},"content":{"type":"string"},"expected_sha256":{"type":"string"}}), &["path","content"]), false, true),
        tool("create_directory", "Recursively create a workspace-relative directory path while rejecting protected paths, symlink components, and workspace escape.", schema(json!({"path":{"type":"string"}}), &["path"]), false, false),
        tool("create_file", "Atomically create one bounded UTF-8 file without overwrite. Protected paths, symlink components, broad path escapes, and races with an existing target are rejected.", schema(json!({"path":{"type":"string"},"content":{"type":"string"}}), &["path","content"]), false, true),
        tool("create_files", "Create up to 64 independent files concurrently. Each file is atomically created without overwrite and reports its own success or failure.", schema(json!({"files":{"type":"array","minItems":1,"maxItems":64,"items":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"],"additionalProperties":false}}}), &["files"]), false, true),
        tool("apply_file_edits", "Apply independent multi-edit transactions to up to 64 files concurrently. Every file is pinned to one SHA-256; each edit may also pin a 1-based original start_line/end_line range, and each file commits once atomically after overlap checks.", schema(json!({"files":{"type":"array","minItems":1,"maxItems":64,"items":{"type":"object","properties":{"path":{"type":"string"},"expected_sha256":{"type":"string"},"edits":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"object","properties":{"old_text":{"type":"string","minLength":1},"new_text":{"type":"string"},"start_line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":1}},"required":["old_text","new_text"],"additionalProperties":false}}},"required":["path","expected_sha256","edits"],"additionalProperties":false}}}), &["files"]), false, true),
        tool("move_path", "Move or rename one file or directory inside the workspace without overwriting the destination. File moves may include expected_source_sha256 to pin the exact source revision; directories reject that file-only precondition. Source trees containing symlinks, hard-linked files, protected paths, or workspace escapes are rejected.", schema(json!({"source":{"type":"string"},"destination":{"type":"string"},"expected_source_sha256":{"type":"string"}}), &["source","destination"]), false, true),
        tool("move_paths", "Move up to 64 independent, non-overlapping files/directories concurrently without destination overwrite. Each file move may pin expected_source_sha256; overlapping or dependent paths are rejected before execution.", schema(json!({"moves":{"type":"array","minItems":1,"maxItems":64,"items":{"type":"object","properties":{"source":{"type":"string"},"destination":{"type":"string"},"expected_source_sha256":{"type":"string"}},"required":["source","destination"],"additionalProperties":false}}}), &["moves"]), false, true),
        tool("delete_path", "Delete one regular file or empty directory only after an exact one-shot human authorization in the TUI or protected local Web UI. File deletion requires expected_sha256. Recursive deletion, workspace-root deletion, protected paths, symlinks, and hard-linked files are permanently blocked.", schema(json!({"path":{"type":"string"},"expected_sha256":{"type":"string"}}), &["path"]), false, true),
        tool("run_command", "Run a policy-checked program without a shell, with scrubbed credentials, bounded streaming output, and timeout termination. A small safe command set is pre-authorized. Other bare executable names become explicit human authorization requests and can be approved per workspace in the TUI or protected local Web UI; shell interpreters, path-bearing program names, workspace escape and protected-resource arguments remain blocked.", schema(json!({"program":{"type":"string","minLength":1,"maxLength":256,"description":"Bare executable name. Non-default programs require explicit per-workspace human authorization before execution."},"args":{"type":"array","items":{"type":"string"}},"cwd":{"type":"string"},"timeout_seconds":{"type":"integer","minimum":1,"maximum":300}}), &["program"]), false, true),
    ]
}

#[cfg(test)]
fn schema(mut properties: Value, required: &[&str]) -> Value {
    if let Some(properties) = properties.as_object_mut() {
        properties.insert(
            "workspace".to_owned(),
            json!({
                "type": "string",
                "description": "Workspace ID from workspace_info. Omit to use the default workspace."
            }),
        );
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

pub(crate) fn selected_workspace(
    state: &AppState,
    args: &Value,
) -> Result<(String, Workspace), String> {
    state
        .workspaces
        .select(string_arg(args, "workspace"))
        .map_err(|error| error.to_string())
}

#[cfg(test)]
fn tool(
    name: &str,
    description: &str,
    input_schema: Value,
    read_only: bool,
    destructive: bool,
) -> Value {
    let product_scopes = scopes::tool_scopes(name)
        .into_iter()
        .map(|scope| scope.as_str())
        .collect::<Vec<_>>();
    json!({
        "name": name,
        "title": name.replace('_', " "),
        "description": description,
        "inputSchema": input_schema,
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": destructive,
            "idempotentHint": read_only,
            "openWorldHint": false,
        },
        "_meta": {
            "dev.wcode/productScopes": product_scopes,
        }
    })
}

#[cfg(test)]
fn tool_result(value: Value, is_error: bool) -> Value {
    json!({
        "content": [{"type": "text", "text": serde_json::to_string(&value).unwrap_or_else(|_| "{}".into())}],
        "structuredContent": value,
        "isError": is_error,
    })
}

#[cfg(test)]
fn batch_validation_error(item_count: usize) -> Option<Value> {
    if item_count == 0 {
        Some(jsonrpc_error(Value::Null, -32600, "empty batch is invalid"))
    } else if item_count > MAX_BATCH_ITEMS {
        Some(jsonrpc_error(
            Value::Null,
            -32600,
            format!("batch exceeds the {MAX_BATCH_ITEMS}-item limit"),
        ))
    } else {
        None
    }
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
        let response = tool_result(
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
        assert_eq!(batch_validation_error(0).unwrap()["error"]["code"], -32600);
        assert!(batch_validation_error(1).is_none());
        assert!(batch_validation_error(MAX_BATCH_ITEMS).is_none());
        assert_eq!(
            batch_validation_error(MAX_BATCH_ITEMS + 1).unwrap()["error"]["code"],
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
