use super::*;
use crate::graph_explorer::{GraphOverviewInput, GraphSearchInput};
use crate::graph_store::{GraphChainInput, GraphChainMode};

#[derive(serde::Deserialize)]
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
            );
        }
        "focus" => {}
        _ => return bad_request("code graph view must be overview, search, or focus"),
    }

    if query.node_id.is_none() {
        return bad_request("node_id is required for code graph focus");
    }
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
    let result =
        mcp_tools::run_blocking(
            move || match harness.graph_chain(&workspace_for_read, &input) {
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
                    harness.graph_chain(&workspace_for_read, &input)
                }
                Err(error) => Err(error),
            },
        )
        .await;
    graph_web_response(
        workspace_id,
        "graph",
        result.and_then(|value| serde_json::to_value(value).map_err(Into::into)),
        requested_snapshot,
    )
}

fn graph_web_response(
    workspace_id: String,
    key: &'static str,
    result: AnyResult<Value>,
    requested_snapshot: bool,
) -> Response {
    match result {
        Ok(value) => {
            let mut payload = serde_json::Map::new();
            payload.insert("workspace".to_owned(), Value::String(workspace_id));
            payload.insert(key.to_owned(), value);
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

fn bad_request(message: &'static str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({"error":message}))).into_response()
}

fn missing_graph_snapshot(message: &str) -> bool {
    message.contains("no stored software graph snapshot is available")
}
