use super::*;
use crate::graph_explorer::{GraphOverviewInput, GraphSearchInput};
use crate::graph_store::{GraphChainInput, GraphChainMode};

#[path = "web_change.rs"]
pub(crate) mod web_change;
#[cfg(test)]
pub(crate) use web_change::IntelligenceChangeQuery;
pub(crate) use web_change::{changed_symbol_impact, intelligence_web_change_detail};

#[derive(serde::Deserialize)]
pub(crate) struct IntelligenceCodeSourceQuery {
    pub(crate) node_id: String,
    pub(crate) snapshot_id: String,
    #[serde(default)]
    pub(crate) context_lines: Option<usize>,
    #[serde(default)]
    pub(crate) start_line: Option<usize>,
}

pub(crate) async fn intelligence_web_code_source(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<IntelligenceCodeSourceQuery>,
) -> Response {
    let (workspace_id, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return *response,
    };
    let node_id = query.node_id.trim().to_owned();
    let snapshot_id = query.snapshot_id.trim().to_owned();
    if node_id.is_empty() || snapshot_id.is_empty() {
        return bad_request("node_id and snapshot_id are required for code source");
    }
    if query.start_line == Some(0) {
        return bad_request("start_line must be at least one");
    }
    let requested_start = query.start_line;
    let context_lines = query.context_lines.unwrap_or(12).clamp(0, 40);
    let harness = state.harness.clone();
    let workspace_for_read = workspace.clone();
    let node_id_for_read = node_id.clone();
    let snapshot_id_for_read = snapshot_id.clone();
    let result = mcp_tools::run_blocking(move || -> AnyResult<Value> {
        let (resolved_snapshot, node, path, expected_sha256) = code_source_identity(
            &harness, &workspace_for_read, &node_id_for_read, &snapshot_id_for_read,
        )?;
        let range = node.attributes.get("range");
        let focus_start = range
            .and_then(|value| value.get("start_line"))
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(1)
            .max(1);
        let line_count = node
            .attributes
            .get("line_count")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(focus_start);
        let focus_end = range
            .and_then(|value| value.get("end_line"))
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(line_count)
            .max(focus_start);
        let start_line =
            requested_start.unwrap_or_else(|| focus_start.saturating_sub(context_lines).max(1));
        let requested_end = if requested_start.is_some() {
            start_line.saturating_add(239)
        } else {
            focus_end.saturating_add(context_lines).max(start_line)
        };
        let end_line = requested_end.min(start_line.saturating_add(239));
        let file = workspace_for_read.read_file(&path, start_line, Some(end_line))?;
        if file.sha256 != expected_sha256 {
            anyhow::bail!("code source changed since graph snapshot");
        }
        if start_line > file.total_lines.max(1) {
            anyhow::bail!("code source page is outside the file");
        }
        // Focus describes only the visible window. Full symbol coordinates remain separate.
        let intersection_start = focus_start.max(file.start_line);
        let intersection_end = focus_end.min(file.end_line);
        let (visible_focus_start, visible_focus_end) =
            if file.total_lines > 0 && intersection_start <= intersection_end {
                (intersection_start, intersection_end)
            } else {
                (0, 0)
            };
        Ok(json!({
            "snapshot_id": resolved_snapshot,
            "node_id": node.id,
            "path": file.path,
            "provider": node.provenance.provider,
            "precision": node.provenance.precision,
            "source_revision": format!("sha256:{expected_sha256}"),
            "current_sha256": file.sha256,
            "focus_start_line": visible_focus_start,
            "focus_end_line": visible_focus_end,
            "symbol_start_line": focus_start,
            "symbol_end_line": focus_end,
            "start_line": file.start_line,
            "end_line": file.end_line,
            "total_lines": file.total_lines,
            "content": file.content,
            "redacted": file.redacted,
            "line_ending": file.line_ending,
            "editable": workspace_for_read.write_enabled() && !file.redacted && file.line_ending != "mixed" && file.content.len() <= 131_072,
            "truncated": file.total_lines > 0 && (end_line < requested_end || file.end_line < file.total_lines),
        }))
    })
    .await;
    let (status, body) = match result {
        Ok(source) => (
            StatusCode::OK,
            json!({"workspace": workspace_id, "source": source}),
        ),
        Err(error) => {
            let message = error.to_string();
            if message.contains("code source changed since graph snapshot") {
                (
                    StatusCode::CONFLICT,
                    json!({"error":"Source changed since the selected graph snapshot. Reload the graph before reading code.","code":"stale_source"}),
                )
            } else if message.contains("code source page is outside the file") {
                (
                    StatusCode::BAD_REQUEST,
                    json!({"error":"Requested source page is outside the file.","code":"source_page_unavailable"}),
                )
            } else if missing_graph_snapshot(&message) {
                (
                    StatusCode::CONFLICT,
                    json!({"error":"The selected graph snapshot is no longer available.","code":"stale_graph"}),
                )
            } else {
                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    json!({"error":"Source inspection unavailable for this graph node.","code":"source_unavailable"}),
                )
            }
        }
    };
    (status, [(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}

fn code_source_identity(
    harness: &ToolHarness,
    workspace: &Workspace,
    node_id: &str,
    snapshot_id: &str,
) -> AnyResult<(String, crate::graph::GraphNode, String, String)> {
    let chain = harness.graph_chain(
        workspace,
        &GraphChainInput {
            snapshot_id: Some(snapshot_id.to_owned()),
            node_id: Some(node_id.to_owned()),
            label_contains: None,
            depth: 1,
            limit: 16,
            mode: GraphChainMode::All,
        },
    )?;
    let node = chain
        .nodes
        .iter()
        .find(|entry| entry.node.id == node_id)
        .ok_or_else(|| anyhow::anyhow!("code source node is unavailable in graph snapshot"))?
        .node
        .clone();
    let path = node
        .attributes
        .get("path")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("code source path is unavailable in graph snapshot"))?
        .to_owned();
    let sha = node
        .attributes
        .get("source_sha256")
        .and_then(Value::as_str)
        .or_else(|| node.attributes.get("sha256").and_then(Value::as_str))
        .map(str::to_owned)
        .or_else(|| {
            node.provenance
                .revision
                .strip_prefix("sha256:")
                .map(str::to_owned)
        })
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| anyhow::anyhow!("code source identity is unavailable in graph snapshot"))?;
    Ok((chain.snapshot_id, node, path, sha))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct IntelligenceCodeSourceEdit {
    node_id: String,
    snapshot_id: String,
    expected_sha256: String,
    start_line: usize,
    end_line: usize,
    old_text: String,
    new_text: String,
}

pub(crate) async fn intelligence_web_edit_source(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let (workspace_id, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return *response,
    };
    if !workspace.write_enabled() {
        return source_edit_response(
            StatusCode::FORBIDDEN,
            json!({"code":"edit_forbidden","error":"Workspace is read-only."}),
        );
    }
    let query: IntelligenceCodeSourceEdit = match serde_json::from_value(body) {
        Ok(value) => value,
        Err(_) => {
            return bad_request("Source edit requires an exact snapshot, SHA and line window.")
        }
    };
    if query.node_id.is_empty()
        || query.node_id.len() > 512
        || query.snapshot_id.is_empty()
        || query.snapshot_id.len() > 256
        || query.expected_sha256.len() != 64
        || !query
            .expected_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || query.start_line == 0
        || (query.end_line < query.start_line && (query.start_line, query.end_line) != (1, 0))
        || query.end_line.saturating_sub(query.start_line) >= 240
        || query.new_text.len() > 131_072
        || query.new_text.lines().count() > 240
        || query.old_text.len() > 1_048_576
    {
        return bad_request("Source edit exceeds the bounded line window or has invalid identity.");
    }
    let harness = state.harness.clone();
    let workspace_for_read = workspace.clone();
    let prepared = mcp_tools::run_blocking(move || -> AnyResult<(&'static str, Value)> {
        let (_, _, path, sha) = code_source_identity(&harness, &workspace_for_read, &query.node_id, &query.snapshot_id)?;
        let file = workspace_for_read.read_file(&path, query.start_line, Some(query.end_line.max(1)))?;
        if sha != query.expected_sha256 || file.sha256 != sha {
            anyhow::bail!("stale_source");
        }
        if (file.start_line, file.end_line) != (query.start_line, query.end_line)
            || file.content != query.old_text {
            anyhow::bail!("stale_window");
        }
        if file.redacted || file.line_ending == "mixed" {
            anyhow::bail!("source_not_editable");
        }
        let normalized = query.new_text.replace("\r\n", "\n");
        if normalized.contains('\r') { anyhow::bail!("source_not_editable"); }
        let new_text = if file.line_ending == "crlf" { normalized.replace('\n', "\r\n") } else { normalized };
        if file.total_lines == 0 {
            Ok(("write_file", json!({"path":path,"expected_sha256":sha,"content":new_text})))
        } else {
            Ok(("apply_edits", json!({"path":path,"expected_sha256":sha,
                "edits":[{"start_line":query.start_line,"end_line":query.end_line,"old_text":query.old_text,"new_text":new_text}]})))
        }
    }).await;
    let (name, mut arguments) = match prepared {
        Ok(value) => value,
        Err(error) => return source_edit_error(&error.to_string()),
    };
    arguments["workspace"] = json!(workspace_id);
    let owner = format!("ui:{}", state.auth.instance_id());
    let result = match crate::mcp::call_tool_owned(
        &state,
        json!({"name":name,"arguments":arguments}),
        &owner,
    )
    .await
    {
        Ok(value) if value.get("isError").and_then(Value::as_bool) != Some(true) => value,
        Ok(value) => {
            return source_edit_error(
                value
                    .pointer("/structuredContent/error")
                    .and_then(Value::as_str)
                    .unwrap_or("edit rejected"),
            )
        }
        Err(error) => return source_edit_error(&error),
    };
    let data = &result["structuredContent"];
    source_edit_response(
        StatusCode::OK,
        json!({"workspace":workspace_id,"code":"source_updated",
        "edit":{"path":data["path"],"sha256_before":data["sha256_before"],
            "sha256_after":data["sha256_after"],"bytes_written":data["bytes_written"]}}),
    )
}

fn source_edit_error(message: &str) -> Response {
    let (status, code, error) = if message.contains("stale_source")
        || message.contains("stale_window")
        || message.contains("SHA-256 mismatch")
        || message.contains("sha256 mismatch")
        || message.contains("expected SHA")
    {
        (
            StatusCode::CONFLICT,
            "stale_source",
            "Source changed. Reload and compare before saving.",
        )
    } else if missing_graph_snapshot(message) {
        (
            StatusCode::CONFLICT,
            "stale_graph",
            "Graph snapshot expired. Reload and compare before saving.",
        )
    } else if message.contains("source_not_editable") {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "source_not_editable",
            "Redacted or mixed-newline source cannot be edited here.",
        )
    } else {
        (StatusCode::UNPROCESSABLE_ENTITY, "edit_rejected", "The guarded edit was rejected. Inspect workspace policy and reload source before retrying.")
    };
    source_edit_response(status, json!({"code":code,"error":error}))
}

fn source_edit_response(status: StatusCode, body: Value) -> Response {
    (status, [(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct IntelligenceCodeGraphQuery {
    #[serde(default)]
    pub(crate) view: Option<String>,
    #[serde(default)]
    pub(crate) q: Option<String>,
    #[serde(default)]
    pub(crate) node_id: Option<String>,
    #[serde(default)]
    pub(crate) snapshot_id: Option<String>,
    #[serde(default)]
    pub(crate) depth: Option<usize>,
    #[serde(default)]
    pub(crate) limit: Option<usize>,
    #[serde(default)]
    pub(crate) mode: Option<GraphChainMode>,
    #[serde(default)]
    pub(crate) expected_code_revision: Option<String>,
    #[serde(default)]
    pub(crate) expected_design_revision: Option<String>,
}

pub(crate) async fn intelligence_web_code_graph(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<IntelligenceCodeGraphQuery>,
) -> Response {
    let (workspace_id, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return *response,
    };
    let snapshot_id = query.snapshot_id.filter(|value| !value.trim().is_empty());
    let requested_snapshot = snapshot_id.is_some();
    let view = query.view.as_deref().unwrap_or("focus");

    match view {
        "overview" => {
            let input = GraphOverviewInput {
                snapshot_id,
                limit: query.limit.unwrap_or(220).clamp(16, 320),
            };
            let harness = state.harness.clone();
            let workspace_for_read = workspace.clone();
            let workspace_id_for_read = workspace_id.clone();
            let result = mcp_tools::run_blocking(move || {
                match harness.graph_overview(&workspace_for_read, &input) {
                    Ok(result) => Ok(result),
                    Err(error)
                        if input.snapshot_id.is_none()
                            && missing_graph_snapshot(&error.to_string()) =>
                    {
                        harness.software_graph(
                            workspace_id_for_read,
                            &workspace_for_read,
                            ".",
                            1_500,
                            5_000,
                        )?;
                        harness.graph_overview(&workspace_for_read, &input)
                    }
                    Err(error) => Err(error),
                }
            })
            .await;
            return graph_web_response(
                workspace_id,
                "overview",
                result.and_then(|value| serde_json::to_value(value).map_err(Into::into)),
                requested_snapshot,
                None,
            );
        }
        "search" => {
            let label = query
                .q
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            let Some(label) = label else {
                return bad_request("q is required for code graph search");
            };
            let input = GraphSearchInput {
                snapshot_id,
                query: label,
                limit: query.limit.unwrap_or(20).clamp(1, 40),
            };
            let harness = state.harness.clone();
            let workspace_for_read = workspace.clone();
            let workspace_id_for_read = workspace_id.clone();
            let result = mcp_tools::run_blocking(move || {
                match harness.graph_search(&workspace_for_read, &input) {
                    Ok(result) => Ok(result),
                    Err(error)
                        if input.snapshot_id.is_none()
                            && missing_graph_snapshot(&error.to_string()) =>
                    {
                        harness.software_graph(
                            workspace_id_for_read,
                            &workspace_for_read,
                            ".",
                            1_500,
                            5_000,
                        )?;
                        harness.graph_search(&workspace_for_read, &input)
                    }
                    Err(error) => Err(error),
                }
            })
            .await;
            return graph_web_response(
                workspace_id,
                "search",
                result.and_then(|value| serde_json::to_value(value).map_err(Into::into)),
                requested_snapshot,
                None,
            );
        }
        "focus" => {}
        _ => return bad_request("code graph view must be overview, search, or focus"),
    }

    if query.node_id.is_none() {
        return bad_request("node_id is required for code graph focus");
    }
    let expected_code_revision = query
        .expected_code_revision
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    if query.expected_code_revision.is_some() && expected_code_revision.is_none() {
        return bad_request("expected_code_revision cannot be empty");
    }
    if expected_code_revision.is_none() && query.expected_design_revision.is_some() {
        return bad_request("expected_design_revision requires expected_code_revision");
    }
    let expected_design_revision = query.expected_design_revision.clone();
    let repository_revision_before = if let Some(expected_code_revision) = expected_code_revision {
        let harness = state.harness.clone();
        let workspace_for_revision = workspace.clone();
        match mcp_tools::run_blocking(move || harness.current_revision(&workspace_for_revision))
            .await
        {
            Ok(revision)
                if revision.code == expected_code_revision
                    && revision.design.as_deref() == expected_design_revision.as_deref() =>
            {
                Some(revision)
            }
            Ok(_) => {
                return (
                    StatusCode::CONFLICT,
                    [(header::CACHE_CONTROL, "no-store")],
                    Json(json!({"error":"Repository revision changed. Reload the change before opening its code graph.","code":"stale_change"})),
                )
                    .into_response();
            }
            Err(_) => {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    [(header::CACHE_CONTROL, "no-store")],
                    Json(json!({"error":"Repository revision is unavailable for code graph navigation.","code":"revision_unavailable"})),
                )
                    .into_response();
            }
        }
    } else {
        None
    };
    let input = GraphChainInput {
        snapshot_id,
        node_id: query.node_id,
        label_contains: None,
        depth: query.depth.unwrap_or(2).clamp(1, 4),
        limit: query.limit.unwrap_or(140).clamp(16, 240),
        mode: query.mode.unwrap_or_default(),
    };
    let harness = state.harness.clone();
    let workspace_for_read = workspace.clone();
    let workspace_id_for_read = workspace_id.clone();
    let force_current_graph = repository_revision_before.is_some();
    let result = mcp_tools::run_blocking(move || {
        if force_current_graph {
            harness.software_graph(
                workspace_id_for_read.clone(),
                &workspace_for_read,
                ".",
                1_500,
                5_000,
            )?;
            return harness.graph_chain(&workspace_for_read, &input);
        }
        match harness.graph_chain(&workspace_for_read, &input) {
            Ok(result) => Ok(result),
            Err(error)
                if input.snapshot_id.is_none()
                    && (missing_graph_snapshot(&error.to_string())
                        || error
                            .to_string()
                            .contains("no code graph symbol matches the requested chain root")) =>
            {
                harness.software_graph(
                    workspace_id_for_read,
                    &workspace_for_read,
                    ".",
                    1_500,
                    5_000,
                )?;
                harness.graph_chain(&workspace_for_read, &input)
            }
            Err(error) => Err(error),
        }
    })
    .await;
    let repository_revision = if let Some(before) = repository_revision_before {
        let harness = state.harness.clone();
        let workspace_for_revision = workspace.clone();
        match mcp_tools::run_blocking(move || harness.current_revision(&workspace_for_revision))
            .await
        {
            Ok(revision) if revision == before => serde_json::to_value(revision).ok(),
            Ok(_) => {
                return (
                    StatusCode::CONFLICT,
                    [(header::CACHE_CONTROL, "no-store")],
                    Json(json!({"error":"Repository revision changed while opening the code graph. Reload the change before navigating.","code":"stale_change"})),
                )
                    .into_response();
            }
            Err(_) => {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    [(header::CACHE_CONTROL, "no-store")],
                    Json(json!({"error":"Repository revision is unavailable for code graph navigation.","code":"revision_unavailable"})),
                )
                    .into_response();
            }
        }
    } else {
        None
    };
    graph_web_response(
        workspace_id,
        "graph",
        result.and_then(|value| serde_json::to_value(value).map_err(Into::into)),
        requested_snapshot,
        repository_revision,
    )
}

fn graph_web_response(
    workspace_id: String,
    key: &'static str,
    result: AnyResult<Value>,
    requested_snapshot: bool,
    repository_revision: Option<Value>,
) -> Response {
    match result {
        Ok(value) => {
            let mut payload = serde_json::Map::new();
            payload.insert("workspace".to_owned(), Value::String(workspace_id));
            payload.insert(key.to_owned(), value);
            if let Some(revision) = repository_revision {
                payload.insert("repository_revision".to_owned(), revision);
            }
            (
                [(header::CACHE_CONTROL, "no-store")],
                Json(Value::Object(payload)),
            )
                .into_response()
        }
        Err(error) => {
            let message = error.to_string();
            let status = if requested_snapshot && missing_graph_snapshot(&message) {
                StatusCode::CONFLICT
            } else if message.contains("code graph")
                && (message.contains("requires")
                    || message.contains("must contain")
                    || message.contains("no code graph"))
            {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (
                status,
                Json(json!({
                    "error": crate::workspace::redact_sensitive_text(&message).0
                })),
            )
                .into_response()
        }
    }
}

pub(super) fn bad_request(message: &'static str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({"error":message}))).into_response()
}

fn missing_graph_snapshot(message: &str) -> bool {
    message.contains("no stored software graph snapshot is available")
}
