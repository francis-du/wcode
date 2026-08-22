use crate::auth::AuthState;
use crate::harness::ToolHarness;
use crate::monitor::TaskMonitor;
use crate::workspace::{Workspace, Workspaces};
use anyhow::{anyhow, Result as AnyResult};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::task::JoinSet;

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
        .route("/mcp", post(mcp))
        .with_state(state)
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
    axum::response::Html(format!(
        r##"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="dark"><title>wcode MCP</title>
<style>*{{box-sizing:border-box}}body{{margin:0;min-height:100vh;display:grid;place-items:center;background:radial-gradient(800px 450px at 50% -10%,#24242b,#09090b 65%);color:#f4f4f5;font:14px/1.55 ui-sans-serif,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;padding:24px}}main{{width:min(100%,640px)}}.brand{{display:flex;align-items:center;gap:11px;margin:0 0 18px 4px}}.logo{{width:34px;height:34px;border:1px solid #3a3a42;border-radius:10px;display:grid;place-items:center;background:#151518;font:700 14px ui-monospace,monospace}}.muted{{color:#8d8d98}}.card{{border:1px solid #29292f;border-radius:18px;background:linear-gradient(180deg,#151519,#101013);padding:26px;box-shadow:0 28px 80px #0008}}h1{{margin:0 0 6px;font-size:23px}}.status{{display:inline-flex;align-items:center;gap:7px;color:#a7f3bd;font-size:12px;margin-bottom:22px}}.dot{{width:7px;height:7px;background:#5ee28a;border-radius:50%;box-shadow:0 0 12px #5ee28a88}}.endpoint{{display:flex;align-items:center;justify-content:space-between;gap:15px;padding:13px 15px;border:1px solid #29292f;border-radius:12px;background:#09090b;font:12px ui-monospace,SFMono-Regular,Menlo,monospace;overflow:auto}}.grid{{display:grid;grid-template-columns:repeat(3,1fr);gap:10px;margin-top:12px}}.stat{{padding:13px;border:1px solid #28282e;border-radius:12px;background:#111114}}.stat b{{display:block;font-size:18px}}.stat span{{font-size:11px;color:#84848f}}footer{{display:flex;justify-content:space-between;margin-top:15px;padding:0 4px;color:#72727d;font-size:12px}}a{{color:#b8b8c0;text-decoration:none}}a:hover{{color:#fff}}@media(max-width:520px){{.grid{{grid-template-columns:1fr}}}}</style></head>
<body><main><div class="brand"><div class="logo">WC</div><div><strong>wcode</strong><div class="muted">Local MCP bridge</div></div></div><section class="card"><div class="status"><i class="dot"></i>MCP server is ready</div><h1>Connected workspace bridge</h1><p class="muted">Authenticated local coding tools with bounded concurrency and multi-root routing.</p><div style="height:20px"></div><div class="endpoint">{base}/mcp</div><div class="grid"><div class="stat"><b>{workspace_count}</b><span>workspace roots</span></div><div class="stat"><b>{}</b><span>parallel tools</span></div><div class="stat"><b>{default_workspace}</b><span>default workspace</span></div></div></section><footer><span>OAuth 2.1 · PKCE</span><a href="https://github.com/francis-du/wcode" target="_blank" rel="noreferrer">github.com/francis-du/wcode ↗</a></footer></main></body></html>"##,
        state.harness.max_parallel(),
    ))
}

async fn health(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "name": "wcode",
        "version": env!("CARGO_PKG_VERSION"),
        "workspaces": state.workspaces.capabilities(),
        "max_parallel_tools": state.harness.max_parallel(),
        "mcp_url": format!("{}/mcp", state.auth.public_url()),
    }))
}

async fn mcp(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    if !state.auth.authorized(&headers) {
        return state.auth.unauthorized_response();
    }

    let protocol = headers
        .get("mcp-protocol-version")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("2025-03-26")
        .to_owned();

    let response = if let Some(items) = payload.as_array() {
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

async fn handle_message(state: Arc<AppState>, message: Value, protocol: &str) -> Option<Value> {
    let id = message.get("id").cloned()?;
    let method = message.get("method").and_then(Value::as_str)?;
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": message
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(protocol),
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name": "wcode", "version": env!("CARGO_PKG_VERSION")},
            "instructions": "Work only inside configured workspace roots. Call workspace_info to discover root IDs and pass workspace when targeting a non-default root. Read before editing and use sha256 preconditions. Independent MCP tool calls may run concurrently; keep dependent steps sequential. Prefer read_files and search_many to reduce round trips.",
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools()})),
        "tools/call" => call_tool(&state, message.get("params").cloned().unwrap_or_default()).await,
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

async fn call_tool(state: &AppState, params: Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or("missing tool name")?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
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
    let mut task = state.monitor.queue(workspace_label, name, request_bytes);
    let _permit = state.harness.acquire().await?;
    task.start();

    let outcome: AnyResult<Value> = match name {
        "workspace_info" => Ok(state.workspaces.capabilities()),
        "list_files" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let path = string_arg(&args, "path").unwrap_or(".").to_owned();
            let limit = usize_arg(&args, "max_entries").unwrap_or(500);
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
            run_blocking(move || {
                workspace
                    .replace_text(&path, &old_text, &new_text, &expected)
                    .and_then(|result| {
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
            run_blocking(move || {
                workspace.create_file(&path, &content).and_then(|result| {
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
    task.finish(success, response_bytes);
    match outcome {
        Ok(value) => Ok(tool_result(value, false)),
        Err(error) => Ok(tool_result(json!({"error": error.to_string()}), true)),
    }
}

fn tools() -> Vec<Value> {
    vec![
        tool("workspace_info", "Show all configured workspace roots, their IDs, and enabled capabilities.", json!({"type":"object","properties":{},"additionalProperties":false}), true, false),
        tool("list_files", "Fast recursive file listing inside one workspace root. Build, VCS, IDE, dependency, log, and local secret files are skipped.", schema(json!({"path":{"type":"string"},"max_entries":{"type":"integer","minimum":1,"maximum":2000}}), &[]), true, false),
        tool("search_code", "Fast exact-substring search in one workspace. File scanning runs off the async runtime and uses parallel workers.", schema(json!({"query":{"type":"string"},"path":{"type":"string"},"max_results":{"type":"integer","minimum":1,"maximum":500}}), &["query"]), true, false),
        tool("search_many", "Search up to 32 exact substrings in one filesystem traversal. Prefer this over repeated search_code calls when looking for several symbols.", schema(json!({"queries":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"string"}},"path":{"type":"string"},"max_results":{"type":"integer","minimum":1,"maximum":1000}}), &["queries"]), true, false),
        tool("read_file", "Read one UTF-8 file with line bounds and receive its SHA-256 edit precondition.", schema(json!({"path":{"type":"string"},"start_line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":1}}), &["path"]), true, false),
        tool("read_files", "Read up to 32 UTF-8 files in one MCP round trip. Reads run in parallel and each file reports success or failure independently.", schema(json!({"paths":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"string"}},"start_line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":1}}), &["paths"]), true, false),
        tool("replace_text", "Atomically replace one exact text occurrence. SHA-256 is rechecked under a per-file lock, so different files can be edited concurrently.", schema(json!({"path":{"type":"string"},"old_text":{"type":"string"},"new_text":{"type":"string"},"expected_sha256":{"type":"string"}}), &["path","old_text","new_text","expected_sha256"]), false, true),
        tool("create_file", "Atomically create a new UTF-8 file. Existing files are never overwritten and different files can be created concurrently.", schema(json!({"path":{"type":"string"},"content":{"type":"string"}}), &["path","content"]), false, true),
        tool("run_command", "Run an allowlisted program without a shell in a selected workspace, with timeout and bounded output.", schema(json!({"program":{"type":"string","enum":["cargo","rustc","git","rg","npm","pnpm","yarn","bun","node","python3","pytest","go","make"]},"args":{"type":"array","items":{"type":"string"}},"cwd":{"type":"string"},"timeout_seconds":{"type":"integer","minimum":1,"maximum":300}}), &["program"]), false, true),
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

fn jsonrpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
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

    #[test]
    fn tool_list_exposes_bulk_tools_without_parallel_meta_tool() {
        let names = tools()
            .into_iter()
            .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        assert!(names.contains(&"read_files".to_owned()));
        assert!(names.contains(&"search_many".to_owned()));
        assert!(!names.contains(&"parallel_tools".to_owned()));
    }
}
