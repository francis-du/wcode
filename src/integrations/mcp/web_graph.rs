use super::*;
use crate::graph_store::{GraphChainInput, GraphChainMode};

#[derive(serde::Deserialize)]
pub(crate) struct IntelligenceCodeGraphQuery {
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
    let label = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    if query.node_id.is_none() && label.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"q or node_id is required"})),
        )
            .into_response();
    }
    let input = GraphChainInput {
        snapshot_id: query.snapshot_id.filter(|value| !value.trim().is_empty()),
        node_id: query.node_id,
        label_contains: label,
        depth: query.depth.unwrap_or(2).clamp(1, 4),
        limit: query.limit.unwrap_or(120).clamp(16, 240),
        mode: query.mode.unwrap_or_default(),
    };
    let requested_snapshot = input.snapshot_id.is_some();
    let harness = state.harness.clone();
    let workspace_for_read = workspace.clone();
    let workspace_id_for_read = workspace_id.clone();
    let result =
        mcp_tools::run_blocking(
            move || match harness.graph_chain(&workspace_for_read, &input) {
                Ok(result) => Ok(result),
                Err(error)
                    if input.snapshot_id.is_none()
                        && (error
                            .to_string()
                            .contains("no stored software graph snapshot")
                            || (input.label_contains.as_deref().is_some_and(|value| {
                                value.contains('/') || value.contains('\\')
                            }) && error.to_string().contains(
                                "no code graph symbol matches the requested chain root",
                            ))) =>
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
    match result {
        Ok(result) => (
            [(header::CACHE_CONTROL, "no-store")],
            Json(json!({
                "workspace": workspace_id,
                "graph": result,
            })),
        )
            .into_response(),
        Err(error) => {
            let message = error.to_string();
            let status = if requested_snapshot
                && message.contains("no stored software graph snapshot is available")
            {
                StatusCode::CONFLICT
            } else if message.contains("code graph chain requires")
                || message.contains("code graph label query must contain")
                || message.contains("no code graph symbol matches the requested chain root")
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
