//! Authenticated selected-Workspace jobs and lost-receipt discovery.
use super::*;
use axum::extract::Path;

fn response(status: StatusCode, value: Value) -> Response {
    (status, [(header::CACHE_CONTROL, "no-store")], Json(value)).into_response()
}

pub(crate) async fn intelligence_web_jobs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    listing(state, headers, false).await
}

pub(crate) async fn intelligence_web_verification_tasks(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    listing(state, headers, true).await
}

fn selected(state: &AppState, headers: &HeaderMap) -> Result<(String, Workspace), Box<Response>> {
    intelligence_ui_workspace(state, headers).map_err(|mut error| {
        error
            .headers_mut()
            .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
        error
    })
}

async fn listing(state: Arc<AppState>, headers: HeaderMap, verification: bool) -> Response {
    let (id, workspace) = match selected(&state, &headers) {
        Ok(selected) => selected,
        Err(error) => return *error,
    };
    match mcp_tools::run_blocking(move || {
        crate::mcp_tasks::observation::list(&state, &id, &workspace, verification)
    })
    .await
    {
        Ok(value) => response(StatusCode::OK, value),
        Err(_) => response(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"code":"task_discovery_unavailable","error":"Task discovery is unavailable; no execution was requested"}),
        ),
    }
}

pub(crate) async fn intelligence_web_job(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    command(state, headers, id, false).await
}

pub(crate) async fn intelligence_web_job_cancel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    command(state, headers, id, true).await
}

async fn command(state: Arc<AppState>, headers: HeaderMap, id: String, cancel: bool) -> Response {
    let (workspace, _) = match selected(&state, &headers) {
        Ok(selected) => selected,
        Err(error) => return *error,
    };
    match mcp_tools::run_blocking(move || {
        crate::mcp_tasks::observation::command(&state, &workspace, &id, cancel)
    })
    .await
    {
        Ok(value) => response(StatusCode::OK, value),
        Err(error) if error.to_string() == "job cancellation denied" => response(
            StatusCode::FORBIDDEN,
            json!({"code":"cancellation_denied","error":"Only a running job owned by the current UI session can be cancelled"}),
        ),
        Err(_) => response(
            StatusCode::NOT_FOUND,
            json!({"code":"unknown_job","error":"Command job is unknown or unavailable in the selected workspace; no retry or execution was requested"}),
        ),
    }
}
