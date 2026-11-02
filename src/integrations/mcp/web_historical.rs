use super::*;
use crate::workspace::{historical_source_error_code, MAX_HISTORICAL_REVISIONS};

#[derive(serde::Deserialize)]
pub(crate) struct IntelligenceHistoricalSourceQuery {
    pub(crate) path: String,
    pub(crate) revision: String,
    #[serde(default)]
    pub(crate) start_line: Option<usize>,
}

#[derive(serde::Deserialize)]
pub(crate) struct IntelligenceRevisionListQuery {
    #[serde(default)]
    pub(crate) limit: Option<usize>,
}

/// Read one protected path as of one exact recorded commit. The revision must be
/// a complete object ID; the path must already be the inspected file. No
/// revision expression, shell command or write crosses this endpoint.
pub(crate) async fn intelligence_web_historical_source(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<IntelligenceHistoricalSourceQuery>,
) -> Response {
    let (workspace_id, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return *response,
    };
    if query.path.trim().is_empty() {
        return historical_bad_request("path is required for historical source");
    }
    if query.revision.trim().is_empty() {
        return historical_bad_request("revision is required for historical source");
    }
    if query.start_line == Some(0) {
        return historical_bad_request("start_line must be at least one");
    }
    let start_line = query.start_line.unwrap_or(1);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(25),
        workspace.historical_source(&query.revision, &query.path, start_line, None),
    )
    .await;
    match result {
        Ok(Ok(source)) => (
            StatusCode::OK,
            [(header::CACHE_CONTROL, "no-store")],
            Json(json!({"workspace": workspace_id, "source": source})),
        )
            .into_response(),
        Ok(Err(error)) => historical_source_error(&workspace_id, &error.to_string()),
        Err(_) => (
            StatusCode::GATEWAY_TIMEOUT,
            [(header::CACHE_CONTROL, "no-store")],
            Json(json!({"error":"Historical source read timed out.","code":"historical_source_timeout"})),
        )
            .into_response(),
    }
}

/// Report the bounded set of recent commits the selector may offer. The list is
/// derived from the repository itself, never from client-supplied revisions.
pub(crate) async fn intelligence_web_revisions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<IntelligenceRevisionListQuery>,
) -> Response {
    let (workspace_id, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return *response,
    };
    let limit = query.limit.unwrap_or(24).clamp(1, MAX_HISTORICAL_REVISIONS);
    match workspace.historical_revisions(limit).await {
        Ok((revisions, truncated)) => (
            StatusCode::OK,
            [(header::CACHE_CONTROL, "no-store")],
            Json(
                json!({"workspace": workspace_id, "revisions": revisions, "truncated": truncated}),
            ),
        )
            .into_response(),
        Err(_) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            [(header::CACHE_CONTROL, "no-store")],
            Json(json!({
                "workspace": workspace_id,
                "error": "Historical revision list is unavailable for this repository state.",
                "code": "revision_list_unavailable",
            })),
        )
            .into_response(),
    }
}

fn historical_source_error(workspace_id: &str, message: &str) -> Response {
    let (status, code, error) = match historical_source_error_code(message) {
        Some("invalid_revision") => (
            StatusCode::BAD_REQUEST,
            "invalid_revision",
            "Historical source requires a complete commit object ID.",
        ),
        Some("unknown_revision") => (
            StatusCode::NOT_FOUND,
            "unknown_revision",
            "This revision is not available in this repository.",
        ),
        Some("source_missing") => (
            StatusCode::NOT_FOUND,
            "source_missing",
            "This path does not exist at the selected revision.",
        ),
        Some("invalid_path") => (
            StatusCode::BAD_REQUEST,
            "invalid_path",
            "Historical source requires one explicit repository-relative file path.",
        ),
        Some("path_restricted") => (
            StatusCode::FORBIDDEN,
            "path_restricted",
            "This path is restricted by the workspace protected-path policy.",
        ),
        _ => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "historical_source_unavailable",
            "Historical source is unavailable for this path or repository state.",
        ),
    };
    historical_response(
        status,
        json!({"workspace": workspace_id, "error": error, "code": code}),
    )
}

fn historical_bad_request(message: &'static str) -> Response {
    historical_response(StatusCode::BAD_REQUEST, json!({"error": message}))
}

fn historical_response(status: StatusCode, body: Value) -> Response {
    (status, [(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}
