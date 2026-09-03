use crate::auth::AuthState;
use crate::authorization::AuthorizationRequired;
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
    "semantic_navigation",
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

#[path = "authorization.rs"]
mod mcp_authorization;
#[cfg(test)]
pub(crate) use mcp_authorization::AUTHORIZATION_INPUT_KEY;
pub(crate) use mcp_authorization::{
    apply_authorization_response, authorization_elicitation_params,
    authorization_request_from_tool_result, authorization_request_state, supports_form_elicitation,
};
use mcp_authorization::{
    apply_authorization_retry, authorization_input_required, client_supports_elicitation,
};

#[path = "web.rs"]
mod web;
use web::*;

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
        .merge(crate::mcp_legacy_sse::routes())
        .with_state(state)
}

async fn mcp_get(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let Some(public_url) = state.auth.request_public_url(&headers) else {
        return forbidden_origin_response();
    };
    if !origin_allowed(&public_url, &headers) {
        return forbidden_origin_response();
    }
    if state
        .auth
        .authorized_client_fingerprint_for(&headers, &public_url)
        .is_none()
    {
        return state.auth.unauthorized_response_for(&public_url);
    }
    state.monitor.mark_mcp_seen();
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [("allow", "POST")],
        "Use Streamable HTTP POST at /mcp; legacy clients can connect to /sse",
    )
        .into_response()
}

async fn health(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Json<Value> {
    let connection = state.monitor.connection_status();
    let public_url = state
        .auth
        .request_public_url(&headers)
        .unwrap_or_else(|| state.auth.public_url());
    Json(json!({
        "ok": true,
        "name": "wcode",
        "instance_id": state.auth.instance_id(),
        "version": env!("CARGO_PKG_VERSION"),
        "workspaces": state.workspaces.capabilities(),
        "max_parallel_tools": state.harness.max_parallel(),
        "resources": crate::resource::snapshot(),
        "harness": state.harness.capabilities(),
        "mcp_url": format!("{public_url}/mcp"),
        "legacy_sse_url": format!("{public_url}/sse"),
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
        "tunnels": state
            .monitor
            .tunnel_links()
            .into_iter()
            .map(|(provider, url)| {
                json!({"provider": provider, "url": url, "mcp_url": format!("{url}/mcp")})
            })
            .collect::<Vec<_>>(),
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
    let Some(public_url) = state.auth.request_public_url(&headers) else {
        return forbidden_origin_response();
    };
    if !origin_allowed(&public_url, &headers) {
        return forbidden_origin_response();
    }
    let Some(owner) = state
        .auth
        .authorized_client_fingerprint_for(&headers, &public_url)
    else {
        return state.auth.unauthorized_response_for(&public_url);
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

    let response = dispatch_mcp_payload(state, payload, &protocol, &owner).await;

    match response {
        Some(value) => {
            let status = mcp_response_status(&value, modern);
            (status, [("mcp-protocol-version", protocol)], Json(value)).into_response()
        }
        None => StatusCode::ACCEPTED.into_response(),
    }
}

pub(crate) async fn dispatch_mcp_payload(
    state: Arc<AppState>,
    payload: Value,
    protocol: &str,
    owner: &str,
) -> Option<Value> {
    if let Some(items) = payload.as_array() {
        if let Some(error) = mcp_tools::batch_validation_error(items.len()) {
            return Some(error);
        }
        let mut tasks = JoinSet::new();
        for item in items.iter().cloned() {
            let state = state.clone();
            let protocol = protocol.to_owned();
            let owner = owner.to_owned();
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
        (!responses.is_empty()).then_some(Value::Array(responses))
    } else {
        handle_message_isolated(state, payload, protocol, owner).await
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

pub(crate) fn origin_allowed(public_url: &str, headers: &HeaderMap) -> bool {
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

pub(crate) fn forbidden_origin_response() -> Response {
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

const SERVER_INSTRUCTIONS: &str = "Stay inside configured Workspaces; never bypass authorization or path protections. Send only required arguments; omit the default Workspace and server-default path/limit/timeout/budget values. For coding call agent_context first and follow readiness/next_actions/parallelism; resume its active Worklist and update status without dropping unfinished items. Run independent dependency lanes as concurrent top-level calls when supported; use bulk tools for known inputs and parallel_tools only for compact fanout. Use find_symbol/search_code for localization, semantic_navigation only for needed cross-file relations, and symbol_context/read_file only for missing source while preserving original formatting. Use guarded edits, then review_changes and verify_project. Tree-sitter is syntax precision unless fresh stronger evidence exists. Never fabricate Evidence, stage success, semantic precision, HumanApproval, authorization, or Worklist completion.";

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
    if modern && method == "tools/call" {
        match apply_authorization_retry(&state, owner, &message) {
            Ok(Some(value)) => {
                return Some(json!({
                    "jsonrpc":"2.0",
                    "id":id,
                    "result":modern_result(value)
                }));
            }
            Ok(None) => {}
            Err(error) => return Some(jsonrpc_error(id, -32602, error)),
        }
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
        "tools/call" => {
            match call_tool(&state, message.get("params").cloned().unwrap_or_default()).await {
                Ok(value) if modern => {
                    if let Some(request) = authorization_request_from_tool_result(&state, &value) {
                        if !client_supports_elicitation(&message) {
                            return Some(json!({
                                "jsonrpc":"2.0",
                                "id":id,
                                "error":{
                                    "code":-32021,
                                    "message":"Client does not advertise elicitation required for human authorization",
                                    "data":{"requiredCapabilities":{"elicitation":{}}}
                                }
                            }));
                        }
                        authorization_input_required(&state, &request, owner)
                    } else {
                        Ok(modern_result(value))
                    }
                }
                Ok(value) => Ok(value),
                Err(error) => Err(error),
            }
        }
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

#[path = "dispatch/mod.rs"]
mod mcp_dispatch;
#[path = "tools.rs"]
mod mcp_tools;
pub(crate) use mcp_dispatch::call_tool;
#[cfg(test)]
use mcp_dispatch::{estimated_context_bytes_avoided, parallel_item_from_response};
#[cfg(test)]
#[path = "../../../tests/unit/integrations/mcp/http.rs"]
mod web_tests;

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
#[path = "../../../tests/unit/integrations/mcp/protocol.rs"]
mod protocol_tests;
#[cfg(test)]
#[path = "../../../tests/unit/integrations/mcp/support.rs"]
mod test_support;
#[cfg(test)]
#[path = "../../../tests/unit/integrations/mcp/calls.rs"]
mod tool_tests;
