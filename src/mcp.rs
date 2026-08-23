use crate::auth::AuthState;
use crate::harness::ToolHarness;
use crate::monitor::TaskMonitor;
use crate::workspace::{Workspace, Workspaces};
use crate::{
    AUTHOR_HANDLE, AUTHOR_URL, CHATGPT_CONNECTOR_SETUP_URL, CLAUDE_CONNECTOR_SETUP_URL, DOCS_URL,
    GROK_CONNECTOR_SETUP_URL, MISTRAL_CONNECTOR_SETUP_URL, PROJECT_URL,
};
use anyhow::{anyhow, Result as AnyResult};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::task::JoinSet;

const MODERN_PROTOCOL_VERSION: &str = "2026-07-28";
const LEGACY_PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
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
const PARALLEL_READ_TOOLS: &[&str] = &[
    "workspace_info",
    "project_context",
    "list_files",
    "search_code",
    "search_many",
    "file_outline",
    "find_symbol",
    "symbol_context",
    "read_file",
    "read_files",
];

#[derive(Clone)]
pub struct AppState {
    pub auth: Arc<AuthState>,
    pub workspaces: Workspaces,
    pub harness: ToolHarness,
    pub monitor: TaskMonitor,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(setup_page))
        .route("/healthz", get(health))
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
        r##"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="dark"><title>wcode MCP</title>
<style>*{{box-sizing:border-box}}body{{margin:0;min-height:100vh;display:grid;place-items:center;background:radial-gradient(800px 450px at 50% -10%,#24242b,#09090b 65%);color:#f4f4f5;font:14px/1.55 ui-sans-serif,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;padding:24px}}main{{width:min(100%,720px)}}.brand{{display:flex;align-items:center;gap:11px;margin:0 0 18px 4px}}.logo{{width:34px;height:34px;border:1px solid #3a3a42;border-radius:10px;display:grid;place-items:center;background:#151518;font:700 14px ui-monospace,monospace}}.muted{{color:#8d8d98}}.card{{border:1px solid #29292f;border-radius:18px;background:linear-gradient(180deg,#151519,#101013);padding:26px;box-shadow:0 28px 80px #0008}}h1{{margin:0 0 6px;font-size:23px}}.status{{display:inline-flex;align-items:center;gap:7px;color:#a7f3bd;font-size:12px;margin-bottom:22px}}.dot{{width:7px;height:7px;background:#5ee28a;border-radius:50%;box-shadow:0 0 12px #5ee28a88}}.endpoint{{display:flex;align-items:center;justify-content:space-between;gap:15px;padding:13px 15px;border:1px solid #29292f;border-radius:12px;background:#09090b;font:12px ui-monospace,SFMono-Regular,Menlo,monospace;overflow:auto}}.grid{{display:grid;grid-template-columns:repeat(4,1fr);gap:10px;margin-top:12px}}.stat{{padding:13px;border:1px solid #28282e;border-radius:12px;background:#111114}}.stat b{{display:block;font-size:18px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}}.stat span{{font-size:11px;color:#84848f}}.clients{{display:grid;grid-template-columns:repeat(5,1fr);gap:8px;margin-top:18px}}.client{{display:flex;align-items:center;justify-content:center;min-height:44px;padding:0 10px;border:1px solid #323239;border-radius:11px;background:#0c0c0f;color:#f4f4f5;font-weight:650;font-size:12px;text-decoration:none}}.client:hover{{border-color:#696973;background:#17171b}}.hint{{margin-top:12px;color:#73737d;font-size:11px}}footer{{display:flex;justify-content:space-between;gap:14px;flex-wrap:wrap;margin-top:15px;padding:0 4px;color:#72727d;font-size:12px}}a{{color:#b8b8c0;text-decoration:none}}a:hover{{color:#fff}}@media(max-width:720px){{.grid{{grid-template-columns:repeat(2,1fr)}}.clients{{grid-template-columns:repeat(2,1fr)}}}}@media(max-width:520px){{.grid{{grid-template-columns:1fr}}.clients{{grid-template-columns:1fr}}}}</style></head>
<body><main><div class="brand"><div class="logo">WC</div><div><strong>wcode</strong><div class="muted">Remote MCP setup</div></div></div><section class="card"><div class="status"><i class="dot"></i>Ready to connect</div><h1>Choose your AI client</h1><p class="muted">One wcode endpoint works across supported Remote MCP clients. Pick a client below, then use the shared MCP URL and OAuth pairing code.</p><div style="height:20px"></div><div class="endpoint">{mcp_url}</div><div class="clients"><a class="client" href="{grok_url}" target="_blank" rel="noreferrer">Grok ↗</a><a class="client" href="{claude_url}" target="_blank" rel="noreferrer">Claude ↗</a><a class="client" href="{chatgpt_url}" target="_blank" rel="noreferrer">ChatGPT ↗</a><a class="client" href="{mistral_url}" target="_blank" rel="noreferrer">Mistral ↗</a><a class="client" href="{docs_url}#clients" target="_blank" rel="noreferrer">Other MCP ↗</a></div><div class="hint">All clients use the same endpoint. No provider-specific wcode flags are required.</div><div class="grid"><div class="stat"><b>{workspace_count}</b><span>workspace roots</span></div><div class="stat"><b>{}</b><span>parallel slots</span></div><div class="stat"><b>{quality_tool_count}</b><span>quality harness tools</span></div><div class="stat"><b>{default_workspace}</b><span>default workspace</span></div></div></section><footer><a href="{docs_url}" target="_blank" rel="noreferrer">Docs ↗</a><a href="{project_url}" target="_blank" rel="noreferrer">Project ↗</a><a href="{author_url}" target="_blank" rel="noreferrer">{author_handle} ↗</a></footer></main></body></html>"##,
        state.harness.max_parallel(),
        quality_tool_count = state.harness.quality_tool_count(),
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
    if !state.auth.authorized(&headers) {
        return state.auth.unauthorized_response();
    }

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
        if let Some(error) = batch_validation_error(items.len()) {
            Some(error)
        } else {
            let mut tasks = JoinSet::new();
            for item in items.iter().cloned() {
                let state = state.clone();
                let protocol = protocol.clone();
                tasks.spawn(async move { handle_message(state, item, &protocol).await });
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
        handle_message(state, payload, &protocol).await
    };

    match response {
        Some(value) => (
            StatusCode::OK,
            [("mcp-protocol-version", protocol)],
            Json(value),
        )
            .into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

fn request_protocol(headers: &HeaderMap, payload: &Value) -> String {
    if let Some(value) = headers
        .get("mcp-protocol-version")
        .and_then(|value| value.to_str().ok())
    {
        return value.to_owned();
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

fn validate_modern_request(headers: &HeaderMap, payload: &Value) -> Result<(), &'static str> {
    let method = payload
        .get("method")
        .and_then(Value::as_str)
        .ok_or("missing JSON-RPC method")?;
    let header_method = headers
        .get("mcp-method")
        .and_then(|value| value.to_str().ok())
        .ok_or("missing Mcp-Method header")?;
    if header_method != method {
        return Err("Mcp-Method header does not match the JSON-RPC method");
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
    if method == "tools/call" {
        let name = payload
            .pointer("/params/name")
            .and_then(Value::as_str)
            .ok_or("tools/call is missing params.name")?;
        let header_name = headers
            .get("mcp-name")
            .and_then(|value| value.to_str().ok())
            .ok_or("tools/call is missing Mcp-Name header")?;
        if header_name != name {
            return Err("Mcp-Name header does not match tools/call params.name");
        }
    }
    Ok(())
}

fn modern_bad_request(payload: &Value, message: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        [("mcp-protocol-version", MODERN_PROTOCOL_VERSION)],
        Json(jsonrpc_error(
            payload.get("id").cloned().unwrap_or(Value::Null),
            -32600,
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

const SERVER_INSTRUCTIONS: &str = "Work only inside configured workspace roots. Call workspace_info to discover root IDs, security policy, and capabilities, then call project_context before substantial coding work. Never attempt deletion: no delete tool is exposed, protected credential/VCS paths and symlink aliases are blocked, overlapping roots are rejected by default, and destructive replacements require an explicit operator opt-in. Do not try to bypass the selected workspace through absolute paths, parent traversal, command options, interpreters, package scripts, or Git mutation. run_command permits tightly constrained Git/ripgrep inspection plus exact default Cargo verification shapes (`cargo fmt --check` and `cargo check [--locked]`) inside the selected workspace. Other direct model-facing Cargo, Go, package-manager, interpreter, compiler, build, test, and repository-aware run_command calls require --allow-risky-exec for a trusted repository because project metadata can redirect reads or execute code. verify_project uses a separate exact-shape verification lane: only Harness-inferred quality checks are eligible, so normal check/test/Clippy/build gates can run without the global risky flag while arbitrary command arguments remain blocked. Read relevant implementation and tests before editing. Prefer find_symbol, file_outline, and symbol_context when navigating supported source languages so the model receives precise syntax ranges instead of broad file dumps. Prefer read_files or search_many when one bulk operation can answer the question efficiently; when two or more independent read/discovery operations are already known, use parallel_tools so they consume separate semaphore slots and appear separately in the monitor. Keep dependent edits sequential, use sha256 preconditions, and never parallelize operations whose inputs depend on earlier results. After changes, call review_changes to inspect the bounded Git change set, test coverage signals, whitespace errors, and risk level; then run verify_project with the recommended level. Verification uses phased parallelism to avoid running compiler-heavy checks together. Report checks actually run and any failures or assumptions that remain.";

async fn handle_message(state: Arc<AppState>, message: Value, protocol: &str) -> Option<Value> {
    let id = message.get("id").cloned()?;
    let method = message.get("method").and_then(Value::as_str)?;
    let modern = protocol == MODERN_PROTOCOL_VERSION;
    let result = match method {
        "server/discover" if modern => Ok(modern_cacheable_result(json!({
            "supportedVersions": DISCOVER_PROTOCOL_VERSIONS,
            "capabilities": {"tools": {"listChanged": false}},
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
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "wcode", "version": env!("CARGO_PKG_VERSION")},
                "instructions": SERVER_INSTRUCTIONS,
            }))
        }
        "ping" => Ok(if modern {
            modern_result(json!({}))
        } else {
            json!({})
        }),
        "tools/list" => Ok(if modern {
            modern_cacheable_result(json!({"tools": tools()}))
        } else {
            json!({"tools": tools()})
        }),
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

fn modern_result(mut value: Value) -> Value {
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

async fn call_tool(state: &AppState, params: Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or("missing tool name")?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if name == "review_changes" {
        return review_changes_tool(state, &args).await;
    }
    if name == "verify_project" {
        return verify_project_tool(state, &args).await;
    }
    if name == "parallel_tools" {
        return parallel_tools(state, &args).await;
    }
    call_leaf_tool(state, name, args).await
}

async fn call_leaf_tool(state: &AppState, name: &str, args: Value) -> Result<Value, String> {
    let workspace_label = if name == "workspace_info" {
        "system".to_owned()
    } else {
        string_arg(&args, "workspace")
            .unwrap_or(state.workspaces.default_id())
            .to_owned()
    };
    let request_bytes = serde_json::to_vec(&args)
        .map(|bytes| bytes.len() as u64)
        .unwrap_or(0);
    let detail = task_detail(name, &args);
    let mut task = state
        .monitor
        .queue(workspace_label, name, detail, request_bytes);
    let _permit = state.harness.acquire().await?;
    task.start();

    let outcome: AnyResult<Value> = match name {
        "workspace_info" => {
            let mut info = state.workspaces.capabilities();
            info["harness"] = state.harness.capabilities();
            info["scheduling"] = json!({
                "max_parallel": state.harness.max_parallel(),
                "semantics": "global-cap-not-target",
                "fanout_tool": "parallel_tools",
                "bulk_tools": ["read_files", "search_many"],
                "composite_tools": ["review_changes", "verify_project"],
                "guidance": "Use bulk tools for one traversal, parallel_tools for already-known independent reads, and keep dependent edits sequential."
            });
            Ok(info)
        }
        "project_context" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .project_context(workspace_id, &workspace)
                    .and_then(|context| serde_json::to_value(context).map_err(Into::into))
            })
            .await
        }
        "list_files" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let path = string_arg(&args, "path").unwrap_or(".").to_owned();
            let limit = usize_arg(&args, "max_entries").unwrap_or(2_000);
            run_blocking(move || {
                workspace.list_files(&path, limit).map(|files| {
                    json!({"workspace": workspace_id, "files": files, "count": files.len()})
                })
            })
            .await
        }
        "search_code" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let query = required_string(&args, "query")?.to_owned();
            let path = string_arg(&args, "path").unwrap_or(".").to_owned();
            let limit = usize_arg(&args, "max_results").unwrap_or(100);
            run_blocking(move || {
                workspace.search(&query, &path, limit).map(|matches| {
                    json!({"workspace": workspace_id, "matches": matches, "count": matches.len()})
                })
            })
            .await
        }
        "search_many" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let queries = string_array_arg(&args, "queries", 32)?;
            let path = string_arg(&args, "path").unwrap_or(".").to_owned();
            let limit = usize_arg(&args, "max_results").unwrap_or(200);
            run_blocking(move || {
                workspace.search_many(&queries, &path, limit).map(|matches| {
                    json!({"workspace": workspace_id, "matches": matches, "count": matches.len()})
                })
            })
            .await
        }
        "file_outline" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let path = required_string(&args, "path")?.to_owned();
            let max_symbols = usize_arg(&args, "max_symbols").unwrap_or(500);
            let harness = state.harness.clone();
            run_blocking(move || harness.file_outline(workspace_id, &workspace, &path, max_symbols))
                .await
        }
        "find_symbol" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let query = required_string(&args, "query")?.to_owned();
            let path = string_arg(&args, "path").unwrap_or(".").to_owned();
            let kind = string_arg(&args, "kind").map(str::to_owned);
            let max_results = usize_arg(&args, "max_results").unwrap_or(50);
            let harness = state.harness.clone();
            run_blocking(move || {
                harness.find_symbol(
                    workspace_id,
                    &workspace,
                    &query,
                    &path,
                    kind.as_deref(),
                    max_results,
                )
            })
            .await
        }
        "symbol_context" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let symbol_id = required_string(&args, "symbol_id")?.to_owned();
            let max_body_lines = usize_arg(&args, "max_body_lines").unwrap_or(200);
            let harness = state.harness.clone();
            run_blocking(move || {
                harness.symbol_context(workspace_id, &workspace, &symbol_id, max_body_lines)
            })
            .await
        }
        "read_file" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let path = required_string(&args, "path")?.to_owned();
            let start = usize_arg(&args, "start_line").unwrap_or(1);
            let end = usize_arg(&args, "end_line");
            run_blocking(move || {
                workspace.read_file(&path, start, end).and_then(|view| {
                    let mut value = serde_json::to_value(view)?;
                    value["workspace"] = json!(workspace_id);
                    Ok(value)
                })
            })
            .await
        }
        "read_files" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let paths = string_array_arg(&args, "paths", 32)?;
            let start = usize_arg(&args, "start_line").unwrap_or(1);
            let end = usize_arg(&args, "end_line");
            run_blocking(move || {
                workspace.read_files(&paths, start, end).map(|files| {
                    json!({"workspace": workspace_id, "count": files.len(), "files": files})
                })
            })
            .await
        }
        "replace_text" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let path = required_string(&args, "path")?.to_owned();
            let old_text = required_string(&args, "old_text")?.to_owned();
            let new_text = required_string(&args, "new_text")?.to_owned();
            let expected = required_string(&args, "expected_sha256")?.to_owned();
            let harness = state.harness.clone();
            run_blocking(move || {
                workspace
                    .replace_text(&path, &old_text, &new_text, &expected)
                    .and_then(|result| {
                        harness.invalidate_code_file(&workspace, &result.path);
                        let mut value = serde_json::to_value(result)?;
                        value["workspace"] = json!(workspace_id);
                        Ok(value)
                    })
            })
            .await
        }
        "create_file" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let path = required_string(&args, "path")?.to_owned();
            let content = required_string(&args, "content")?.to_owned();
            let harness = state.harness.clone();
            run_blocking(move || {
                workspace.create_file(&path, &content).and_then(|result| {
                    harness.invalidate_code_file(&workspace, &result.path);
                    let mut value = serde_json::to_value(result)?;
                    value["workspace"] = json!(workspace_id);
                    Ok(value)
                })
            })
            .await
        }
        "run_command" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let program = required_string(&args, "program")?.to_owned();
            let command_args = args
                .get("args")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let cwd = string_arg(&args, "cwd").unwrap_or(".").to_owned();
            let timeout_seconds = args
                .get("timeout_seconds")
                .and_then(Value::as_u64)
                .unwrap_or(120);
            workspace
                .run_command(&program, &command_args, &cwd, timeout_seconds)
                .await
                .and_then(|result| {
                    let mut value = serde_json::to_value(result)?;
                    value["workspace"] = json!(workspace_id);
                    Ok(value)
                })
        }
        _ => return Err(format!("unknown tool: {name}")),
    };

    let success = outcome.is_ok();
    let response_bytes = match &outcome {
        Ok(value) => serde_json::to_vec(value)
            .map(|bytes| bytes.len() as u64)
            .unwrap_or(0),
        Err(error) => error.to_string().len() as u64,
    };
    let context_bytes_avoided = outcome
        .as_ref()
        .ok()
        .map(|value| estimated_context_bytes_avoided(name, value, response_bytes))
        .unwrap_or(0);
    task.finish_with_context_savings(success, response_bytes, context_bytes_avoided);
    match outcome {
        Ok(value) => Ok(tool_result(value, false)),
        Err(error) => Ok(tool_result(json!({"error": error.to_string()}), true)),
    }
}

fn estimated_context_bytes_avoided(name: &str, value: &Value, response_bytes: u64) -> u64 {
    if !matches!(name, "file_outline" | "symbol_context") {
        return 0;
    }
    value
        .get("source_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .saturating_sub(response_bytes)
}

async fn review_changes_tool(state: &AppState, args: &Value) -> Result<Value, String> {
    let (workspace_id, workspace) = selected_workspace(state, args)?;
    let timeout_seconds = args
        .get("timeout_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(30);
    match state
        .harness
        .review_changes(workspace_id, &workspace, timeout_seconds, &state.monitor)
        .await
    {
        Ok(report) => serde_json::to_value(report)
            .map(|value| tool_result(value, false))
            .map_err(|error| error.to_string()),
        Err(error) => Ok(tool_result(json!({"error": error.to_string()}), true)),
    }
}

async fn verify_project_tool(state: &AppState, args: &Value) -> Result<Value, String> {
    let (workspace_id, workspace) = selected_workspace(state, args)?;
    let level = string_arg(args, "level").unwrap_or("quick").to_owned();
    let timeout_seconds = args
        .get("timeout_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(120);
    match state
        .harness
        .verify_project(
            workspace_id,
            &workspace,
            &level,
            timeout_seconds,
            &state.monitor,
        )
        .await
    {
        Ok(report) => serde_json::to_value(report)
            .map(|value| tool_result(value, false))
            .map_err(|error| error.to_string()),
        Err(error) => Ok(tool_result(json!({"error": error.to_string()}), true)),
    }
}

async fn parallel_tools(state: &AppState, args: &Value) -> Result<Value, String> {
    let items = args
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or("tasks must be an array")?;
    if !(2..=MAX_PARALLEL_FANOUT_ITEMS).contains(&items.len()) {
        return Err(format!(
            "tasks must contain between 2 and {MAX_PARALLEL_FANOUT_ITEMS} items"
        ));
    }

    let started = std::time::Instant::now();
    let mut results = vec![None; items.len()];
    let mut handles = Vec::new();

    for (index, item) in items.iter().enumerate() {
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("task-{}", index + 1));
        let Some(name) = item.get("tool").and_then(Value::as_str) else {
            results[index] = Some(parallel_item_error(id, "unknown", "missing tool name"));
            continue;
        };
        if !PARALLEL_READ_TOOLS.contains(&name) {
            results[index] = Some(parallel_item_error(
                id,
                name,
                "parallel_tools only accepts independent read/discovery tools",
            ));
            continue;
        }
        let arguments = item.get("arguments").cloned().unwrap_or_else(|| json!({}));
        if !arguments.is_object() {
            results[index] = Some(parallel_item_error(id, name, "arguments must be an object"));
            continue;
        }

        let child_state = state.clone();
        let child_name = name.to_owned();
        let id_for_task = id.clone();
        let name_for_task = child_name.clone();
        let handle = tokio::spawn(async move {
            match call_leaf_tool(&child_state, &name_for_task, arguments).await {
                Ok(response) => parallel_item_from_response(id_for_task, name_for_task, response),
                Err(error) => {
                    let item = parallel_item_error(id_for_task, name_for_task, error);
                    let bytes = serialized_size(&item);
                    (item, bytes)
                }
            }
        });
        handles.push((index, id, child_name, handle));
    }

    let mut fanout_response_bytes = 0usize;
    for (index, id, name, handle) in handles {
        results[index] = Some(match handle.await {
            Ok((item, item_bytes))
                if fanout_response_bytes.saturating_add(item_bytes)
                    <= MAX_PARALLEL_FANOUT_RESPONSE_BYTES =>
            {
                fanout_response_bytes = fanout_response_bytes.saturating_add(item_bytes);
                item
            }
            Ok((_item, item_bytes)) => parallel_item_error(
                id,
                name,
                format!(
                    "parallel fan-out response budget exceeded ({item_bytes}B item, {}B aggregate limit); narrow paths, line ranges, or result limits",
                    MAX_PARALLEL_FANOUT_RESPONSE_BYTES
                ),
            ),
            Err(error) => parallel_item_error(id, name, format!("task join failed: {error}")),
        });
    }

    let items = results
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            item.unwrap_or_else(|| {
                parallel_item_error(
                    format!("task-{}", index + 1),
                    "unknown",
                    "task did not produce a result",
                )
            })
        })
        .collect::<Vec<_>>();
    let succeeded = items
        .iter()
        .filter(|item| item.get("ok").and_then(Value::as_bool) == Some(true))
        .count();
    let failed = items.len().saturating_sub(succeeded);

    Ok(tool_result(
        json!({
            "execution": "parallel-fanout",
            "max_parallel": state.harness.max_parallel(),
            "tasks": items.len(),
            "succeeded": succeeded,
            "failed": failed,
            "elapsed_ms": started.elapsed().as_millis(),
            "response_bytes": fanout_response_bytes,
            "item_limit_bytes": MAX_PARALLEL_FANOUT_ITEM_BYTES,
            "response_limit_bytes": MAX_PARALLEL_FANOUT_RESPONSE_BYTES,
            "items": items,
        }),
        false,
    ))
}

fn parallel_item_from_response(id: String, tool: String, response: Value) -> (Value, usize) {
    let is_error = response
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let result = response
        .get("structuredContent")
        .cloned()
        .unwrap_or(Value::Null);
    let result_bytes = serialized_size(&result);
    if result_bytes > MAX_PARALLEL_FANOUT_ITEM_BYTES {
        let item = parallel_item_error(
            id,
            tool,
            format!(
                "child result is {result_bytes}B, above the {}B fan-out item limit; use line bounds or a smaller result limit",
                MAX_PARALLEL_FANOUT_ITEM_BYTES
            ),
        );
        let bytes = serialized_size(&item);
        return (item, bytes);
    }
    let item = json!({
        "id": id,
        "tool": tool,
        "ok": !is_error,
        "result": result,
    });
    let bytes = serialized_size(&item);
    (item, bytes)
}

fn serialized_size(value: &Value) -> usize {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .unwrap_or(0)
}

fn parallel_item_error(
    id: impl Into<String>,
    tool: impl Into<String>,
    error: impl Into<String>,
) -> Value {
    json!({
        "id": id.into(),
        "tool": tool.into(),
        "ok": false,
        "error": error.into(),
    })
}

fn tools() -> Vec<Value> {
    vec![
        tool("workspace_info", "Show configured workspace IDs, roots, capabilities, scheduling guidance, and the active security policy. Inspect this before selecting a workspace or command strategy.", json!({"type":"object","properties":{},"additionalProperties":false}), true, false),
        tool("project_context", "Build a bounded, cached coding context for one workspace: repository guidance excerpts, detected project types and manifests, recommended quality checks, and a preferred change workflow. Call this before substantial coding work.", schema(json!({}), &[]), true, false),
        tool(
            "review_changes",
            "Review the current Git change set before verification. Runs bounded Git status, diff-check, and numstat probes in parallel, classifies changed files, highlights test/security/manifest risks, and recommends quick or full verification.",
            schema(json!({"timeout_seconds":{"type":"integer","minimum":1,"maximum":120,"default":30}}), &[]),
            true,
            false,
        ),
        tool(
            "parallel_tools",
            "Fan out 2-128 independent read or discovery operations inside one MCP invocation. Every child uses a real global semaphore slot, reports its own bounded result, and appears separately in the TUI. Prefer read_files or search_many when one bulk operation is cheaper; narrow line/result limits for large outputs, and never use this for dependent edits.",
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
                                "tool": {"type": "string", "enum": PARALLEL_READ_TOOLS},
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
            true,
            false,
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
        tool("replace_text", "Atomically replace one exact text occurrence with a SHA-256 precondition and per-file lock. Protected/symlink/hard-link targets are blocked, and emptying or removing most of a file requires an explicit operator opt-in.", schema(json!({"path":{"type":"string"},"old_text":{"type":"string"},"new_text":{"type":"string"},"expected_sha256":{"type":"string"}}), &["path","old_text","new_text","expected_sha256"]), false, true),
        tool("create_file", "Atomically create one bounded UTF-8 file without overwrite. Protected paths, symlink components, broad path escapes, and races with an existing target are rejected.", schema(json!({"path":{"type":"string"},"content":{"type":"string"}}), &["path","content"]), false, true),
        tool("run_command", "Run a policy-checked allowlisted program without a shell, with scrubbed credentials, bounded streaming output, and timeout termination. Tightly constrained Git/ripgrep inspection plus exact `cargo fmt --check` and `cargo check [--locked]` shapes are available by default; broader project execution requires --allow-risky-exec on a trusted repository.", schema(json!({"program":{"type":"string","enum":["cargo","rustc","git","rg","npm","pnpm","yarn","bun","node","python3","pytest","go","make"]},"args":{"type":"array","items":{"type":"string"}},"cwd":{"type":"string"},"timeout_seconds":{"type":"integer","minimum":1,"maximum":300}}), &["program"]), false, true),
    ]
}

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

fn selected_workspace(state: &AppState, args: &Value) -> Result<(String, Workspace), String> {
    state
        .workspaces
        .select(string_arg(args, "workspace"))
        .map_err(|error| error.to_string())
}

async fn run_blocking<F>(work: F) -> AnyResult<Value>
where
    F: FnOnce() -> AnyResult<Value> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| anyhow!("blocking task failed: {error}"))?
}

fn tool(
    name: &str,
    description: &str,
    input_schema: Value,
    read_only: bool,
    destructive: bool,
) -> Value {
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
        }
    })
}

fn tool_result(value: Value, is_error: bool) -> Value {
    json!({
        "content": [{"type": "text", "text": serde_json::to_string(&value).unwrap_or_else(|_| "{}".into())}],
        "structuredContent": value,
        "isError": is_error,
    })
}

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

fn jsonrpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

fn task_detail(name: &str, args: &Value) -> String {
    let path = || string_arg(args, "path").unwrap_or(".");
    match name {
        "workspace_info" => "inspect configured roots and capabilities".to_owned(),
        "project_context" => "collect repository guidance and inferred quality checks".to_owned(),
        "verify_project" => format!(
            "{} quality gate · timeout {}s",
            string_arg(args, "level").unwrap_or("quick"),
            args.get("timeout_seconds")
                .and_then(Value::as_u64)
                .unwrap_or(120)
        ),
        "list_files" => format!(
            "{} · limit {}",
            path(),
            usize_arg(args, "max_entries").unwrap_or(2_000)
        ),
        "search_code" => format!(
            "{} · query {} chars · limit {}",
            path(),
            string_arg(args, "query").map(str::len).unwrap_or(0),
            usize_arg(args, "max_results").unwrap_or(100)
        ),
        "search_many" => format!(
            "{} · {} queries · limit {}",
            path(),
            array_len(args, "queries"),
            usize_arg(args, "max_results").unwrap_or(200)
        ),
        "file_outline" => format!(
            "{} · syntax outline · limit {}",
            path(),
            usize_arg(args, "max_symbols").unwrap_or(500)
        ),
        "find_symbol" => format!(
            "{} · symbol query {} chars · limit {}",
            path(),
            string_arg(args, "query").map(str::len).unwrap_or(0),
            usize_arg(args, "max_results").unwrap_or(50)
        ),
        "symbol_context" => format!(
            "symbol id {} chars · body limit {} lines",
            string_arg(args, "symbol_id").map(str::len).unwrap_or(0),
            usize_arg(args, "max_body_lines").unwrap_or(200)
        ),
        "read_file" => format!(
            "{} · lines {}-{}",
            path(),
            usize_arg(args, "start_line").unwrap_or(1),
            usize_arg(args, "end_line")
                .map(|line| line.to_string())
                .unwrap_or_else(|| "auto".to_owned())
        ),
        "read_files" => format!(
            "{} files · lines {}-{}{}",
            array_len(args, "paths"),
            usize_arg(args, "start_line").unwrap_or(1),
            usize_arg(args, "end_line")
                .map(|line| line.to_string())
                .unwrap_or_else(|| "auto".to_owned()),
            first_array_item(args, "paths")
                .map(|path| format!(" · first {path}"))
                .unwrap_or_default()
        ),
        "replace_text" => format!(
            "{} · replace {}B with {}B",
            path(),
            string_arg(args, "old_text").map(str::len).unwrap_or(0),
            string_arg(args, "new_text").map(str::len).unwrap_or(0)
        ),
        "create_file" => format!(
            "{} · create {}B",
            path(),
            string_arg(args, "content").map(str::len).unwrap_or(0)
        ),
        "run_command" => command_preview(args),
        _ => "unknown tool request".to_owned(),
    }
}

fn array_len(args: &Value, key: &str) -> usize {
    args.get(key)
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

fn first_array_item<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(Value::as_str)
}

fn command_preview(args: &Value) -> String {
    let program = string_arg(args, "program").unwrap_or("command");
    let command_args = args
        .get("args")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut parts = vec![program.to_owned()];
    let mut redact_next = false;

    for value in command_args.iter().take(6) {
        let Some(argument) = value.as_str() else {
            continue;
        };
        if redact_next {
            parts.push("[REDACTED]".to_owned());
            redact_next = false;
            continue;
        }

        let lower = argument.to_ascii_lowercase();
        let sensitive = ["token", "secret", "password", "passwd", "api-key", "apikey"]
            .iter()
            .any(|needle| lower.contains(needle));
        if sensitive {
            if let Some((key, _)) = argument.split_once('=') {
                parts.push(format!("{key}=[REDACTED]"));
            } else {
                parts.push(argument.to_owned());
                redact_next = argument.starts_with('-');
            }
        } else {
            parts.push(argument.to_owned());
        }
    }
    if command_args.len() > 6 {
        parts.push(format!("…+{}", command_args.len() - 6));
    }
    format!(
        "{} · cwd {}",
        parts.join(" "),
        string_arg(args, "cwd").unwrap_or(".")
    )
}

fn string_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn required_string<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    string_arg(args, key).ok_or_else(|| format!("missing string argument: {key}"))
}

fn usize_arg(args: &Value, key: &str) -> Option<usize> {
    args.get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn string_array_arg(args: &Value, key: &str, max_items: usize) -> Result<Vec<String>, String> {
    let values = args
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("missing array argument: {key}"))?;
    if values.is_empty() || values.len() > max_items {
        return Err(format!(
            "{key} must contain between 1 and {max_items} items"
        ));
    }
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| format!("{key} must contain non-empty strings"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
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
            Err("Mcp-Name header does not match tools/call params.name")
        );

        let missing_meta = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}});
        assert!(
            validate_modern_request(&modern_headers("tools/list", None), &missing_meta).is_err()
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

    #[test]
    fn task_details_explain_work_without_exposing_payloads() {
        let search = json!({"path": "src", "query": "private implementation text"});
        let detail = task_detail("search_code", &search);
        assert!(detail.contains("src"));
        assert!(detail.contains("27 chars"));
        assert!(!detail.contains("private implementation text"));

        let command = json!({
            "program": "cargo",
            "args": ["test", "--token", "very-secret", "--locked"],
            "cwd": "crates/core"
        });
        let detail = task_detail("run_command", &command);
        assert!(detail.contains("cargo test"));
        assert!(detail.contains("[REDACTED]"));
        assert!(!detail.contains("very-secret"));
        assert!(detail.contains("crates/core"));
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
        };

        let result = call_tool(&state, json!({"name": "workspace_info", "arguments": {}}))
            .await
            .unwrap();
        let info = &result["structuredContent"];
        assert_eq!(info["scheduling"]["max_parallel"], 16);
        assert_eq!(info["scheduling"]["semantics"], "global-cap-not-target");
        assert_eq!(info["scheduling"]["fanout_tool"], "parallel_tools");
        assert_eq!(info["harness"]["phased_parallel_verification"], true);
        assert_eq!(
            info["harness"]["verification_exec_without_risky_flag"],
            true
        );
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
                            {"id": "two", "tool": "read_file", "arguments": {"path": "two.txt"}},
                            {"id": "write", "tool": "replace_text", "arguments": {}}
                        ]
                    }
                }),
            ),
        )
        .await
        .expect("fan-out must not deadlock with one semaphore slot")
        .unwrap();

        assert_eq!(response["isError"], false);
        let result = &response["structuredContent"];
        assert_eq!(result["execution"], "parallel-fanout");
        assert_eq!(result["max_parallel"], 1);
        assert_eq!(result["tasks"], 4);
        assert_eq!(result["succeeded"], 2);
        assert_eq!(result["failed"], 2);
        let ids = result["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["one", "missing", "two", "write"]);
        assert_eq!(result["items"][0]["result"]["content"], "one");
        assert_eq!(result["items"][1]["ok"], false);
        assert_eq!(result["items"][2]["result"]["content"], "two");
        assert!(result["items"][3]["error"]
            .as_str()
            .unwrap()
            .contains("only accepts independent read"));
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
        let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
        let workspace_id = workspaces.default_id().to_owned();
        let state = AppState {
            auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
            workspaces,
            harness: ToolHarness::new(4).unwrap(),
            monitor: TaskMonitor::new([workspace_id.clone()]),
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
        let names = tools()
            .into_iter()
            .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        for expected in [
            "read_files",
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
