use super::*;
use crate::authorization::AuthorizationStatus;

#[path = "web_status.rs"]
mod web_status;

pub(super) async fn setup_page(
    State(state): State<Arc<AppState>>,
    _headers: HeaderMap,
) -> Response {
    let capabilities = state.workspaces.capabilities();
    let roots = capabilities["workspaces"].as_array().map_or(0, Vec::len);
    let workspace = capabilities["default_workspace"]
        .as_str()
        .unwrap_or("unknown");
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let page = crate::setup_web::render(workspace, roots, state.harness.max_parallel(), &nonce);
    let mut response = (
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::REFERRER_POLICY, "no-referrer"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        axum::response::Html(page),
    )
        .into_response();
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        format!("default-src 'none'; script-src 'nonce-{nonce}'; style-src 'nonce-{nonce}'; connect-src 'self'; img-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'")
            .parse().expect("UUID nonce produces a valid CSP header"),
    );
    response
}

// The public connection guide needs no command catalog, paths, credentials or
// full Harness serialization on every poll. Keep /healthz unchanged for clients.
pub(super) async fn setup_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let connection = state.monitor.connection_status();
    let public_url = state
        .auth
        .request_public_url(&headers)
        .unwrap_or_else(|| state.auth.public_url());
    let tunnels = connection
        .tunnels
        .iter()
        .filter_map(|tunnel| {
            let url = tunnel.url.as_ref()?;
            Some(json!({
                "provider": tunnel.provider,
                "mcp_url": format!("{url}/mcp"),
                "role": tunnel.role,
                "state": tunnel.state,
                "lease_age_seconds": tunnel.lease_age_seconds,
            }))
        })
        .collect::<Vec<_>>();
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "ok": true,
            "mcp_url": format!("{public_url}/mcp"),
            "public_endpoint": connection.public_endpoint,
            "public_url_healthy": connection.public_url_healthy,
            "mcp_initialized": connection.chatgpt_initialized,
            "mcp_last_seen_seconds_ago": connection.last_mcp_seen_seconds_ago,
            "tunnels": tunnels,
        })),
    )
        .into_response()
}

pub(super) async fn intelligence_page() -> Response {
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

pub(super) async fn intelligence_styles() -> Response {
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

pub(super) async fn intelligence_script() -> Response {
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

pub(super) async fn intelligence_logo() -> Response {
    (
        [
            (header::CONTENT_TYPE, "image/svg+xml; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        crate::intelligence_web::INTELLIGENCE_LOGO_SVG,
    )
        .into_response()
}

pub(super) fn intelligence_ui_authorized(
    state: &AppState,
    headers: &HeaderMap,
) -> std::result::Result<(), Box<Response>> {
    if state.auth.request_public_url(headers).is_none() {
        return Err(Box::new(forbidden_host_response()));
    }
    if !origin_allowed(&state.auth, headers) {
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

pub(super) fn requested_intelligence_workspace(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("x-wcode-workspace")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
}

pub(super) fn intelligence_ui_workspace(
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

pub(super) fn intelligence_workspace_options(state: &AppState) -> Value {
    json!(state
        .workspaces
        .roots()
        .into_iter()
        .map(|(id, root)| json!({"id":id,"root":root}))
        .collect::<Vec<_>>())
}

pub(super) fn intelligence_bad_request(error: impl ToString) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error": error.to_string()})),
    )
        .into_response()
}

pub(super) async fn intelligence_web_workspaces(
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

pub(super) async fn intelligence_web_add_workspace(
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
    match state
        .workspaces
        .add_workspace_from(requested_intelligence_workspace(&headers), root)
    {
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

pub(super) async fn intelligence_web_commands(
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

pub(super) async fn intelligence_web_allow_command(
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

pub(super) async fn intelligence_web_enable_all_commands(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = intelligence_ui_authorized(&state, &headers) {
        return *response;
    }
    match state
        .workspaces
        .set_all_commands_authorized(requested_intelligence_workspace(&headers), true)
    {
        Ok(workspace) => (StatusCode::OK, Json(workspace)).into_response(),
        Err(error) => intelligence_bad_request(error),
    }
}

pub(super) async fn intelligence_web_disable_all_commands(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = intelligence_ui_authorized(&state, &headers) {
        return *response;
    }
    match state
        .workspaces
        .set_all_commands_authorized(requested_intelligence_workspace(&headers), false)
    {
        Ok(workspace) => (StatusCode::OK, Json(workspace)).into_response(),
        Err(error) => intelligence_bad_request(error),
    }
}

pub(super) async fn intelligence_web_revoke_command(
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

pub(super) fn intelligence_workspace_id(
    state: &AppState,
    headers: &HeaderMap,
) -> AnyResult<String> {
    state
        .workspaces
        .select(requested_intelligence_workspace(headers))
        .map(|(id, _)| id)
}

pub(super) fn intelligence_pending_authorizations(state: &AppState, workspace_id: &str) -> Value {
    json!(state
        .workspaces
        .authorization_requests(256)
        .into_iter()
        .filter(|request| {
            request.status == AuthorizationStatus::Pending && request.workspace == workspace_id
        })
        .collect::<Vec<_>>())
}

pub(super) async fn intelligence_web_authorizations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = intelligence_ui_authorized(&state, &headers) {
        return *response;
    }
    let workspace_id = match intelligence_workspace_id(&state, &headers) {
        Ok(id) => id,
        Err(error) => return intelligence_bad_request(error),
    };
    (
        StatusCode::OK,
        Json(json!({"pending": intelligence_pending_authorizations(&state, &workspace_id)})),
    )
        .into_response()
}

pub(super) async fn intelligence_web_approve_authorization(
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
    let workspace_id = match intelligence_workspace_id(&state, &headers) {
        Ok(id) => id,
        Err(error) => return intelligence_bad_request(error),
    };
    if state
        .workspaces
        .authorization_request(id)
        .is_none_or(|request| request.workspace != workspace_id)
    {
        return intelligence_bad_request(
            "authorization request does not belong to the selected workspace",
        );
    }
    match state.workspaces.approve_authorization_session_result(id) {
        Ok(request) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "request": request,
                "pending": intelligence_pending_authorizations(&state, &workspace_id)
            })),
        )
            .into_response(),
        Err(error) => intelligence_bad_request(error),
    }
}

pub(super) async fn intelligence_web_authorize_command_operation(
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
    let workspace_id = match intelligence_workspace_id(&state, &headers) {
        Ok(id) => id,
        Err(error) => return intelligence_bad_request(error),
    };
    match state
        .workspaces
        .authorize_command_operation(Some(&workspace_id), program, &args, cwd)
    {
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
                    "pending": intelligence_pending_authorizations(&state, &workspace_id)
                })),
            )
                .into_response(),
            Err(error) => intelligence_bad_request(error),
        },
        Err(error) => intelligence_bad_request(error),
    }
}

pub(super) async fn intelligence_web_deny_authorization(
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
    let workspace_id = match intelligence_workspace_id(&state, &headers) {
        Ok(id) => id,
        Err(error) => return intelligence_bad_request(error),
    };
    if state
        .workspaces
        .authorization_request(id)
        .is_none_or(|request| request.workspace != workspace_id)
    {
        return intelligence_bad_request(
            "authorization request does not belong to the selected workspace",
        );
    }
    if !state.workspaces.deny_authorization(id) {
        return intelligence_bad_request("authorization request is missing or no longer pending");
    }
    (
        StatusCode::OK,
        Json(
            json!({"ok":true,"pending":intelligence_pending_authorizations(&state, &workspace_id)}),
        ),
    )
        .into_response()
}

pub(super) async fn intelligence_web_activity(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let (workspace_id, _) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return *response,
    };
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "workspace": workspace_id,
            "activity": state.monitor.observatory_activity(&workspace_id),
            "resources": crate::resource::capabilities(),
            "resource_scope": "whole_process",
            "pending_authorizations": intelligence_pending_authorizations(&state, &workspace_id).as_array().map_or(0, Vec::len),
        })),
    ).into_response()
}

pub(super) async fn intelligence_web_project(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let (workspace_id, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return *response,
    };
    let prefer_cached = headers
        .get("x-wcode-prefer-cached")
        .and_then(|value| value.to_str().ok())
        == Some("1");
    if prefer_cached {
        if let Some(snapshot) = state.harness.cached_project_observatory(&workspace) {
            if let Ok(mut value) = serde_json::to_value(snapshot) {
                value["workspace_options"] = intelligence_workspace_options(&state);
                value["git_review"] = json!({"available":false,"reason":"cached_snapshot"});
                value["activity"] = state.monitor.observatory_activity(&workspace_id);
                value["pending_authorizations"] = json!(state
                    .workspaces
                    .authorization_requests(256)
                    .into_iter()
                    .filter(|request| {
                        request.status == AuthorizationStatus::Pending
                            && request.workspace == workspace_id
                    })
                    .count());
                value["snapshot_cache"] = json!("stale-while-revalidate");
                let mut response =
                    ([(header::CACHE_CONTROL, "no-store")], Json(value)).into_response();
                response.headers_mut().insert(
                    axum::http::HeaderName::from_static("server-timing"),
                    "snapshot-cache;dur=0.1".parse().unwrap(),
                );
                return response;
            }
        }
        return (
            [(header::CACHE_CONTROL, "no-store")],
            Json(json!({
                "workspace": workspace_id,
                "workspace_options": intelligence_workspace_options(&state),
                "activity": state.monitor.observatory_activity(&workspace_id),
                "pending_authorizations": intelligence_pending_authorizations(&state, &workspace_id).as_array().map_or(0, Vec::len),
                "snapshot_pending": true
            })),
        )
            .into_response();
    }
    let review_started = std::time::Instant::now();
    let (review, review_reason) = if !workspace.exec_enabled() {
        (None, "execution_disabled")
    } else if !workspace.root().join(".git").exists() {
        (None, "not_a_repository")
    } else {
        match state
            .harness
            .review_changes(workspace_id.clone(), &workspace, 30, &state.monitor)
            .await
        {
            Ok(report) => {
                let reason = if report.probes.iter().all(|probe| probe.success) {
                    "available"
                } else {
                    "partial_review"
                };
                (Some(report), reason)
            }
            Err(_) => (None, "review_failed"),
        }
    };
    let review_ms = review_started.elapsed().as_secs_f64() * 1_000.0;
    let git_review = json!({"available": review_reason == "available", "reason": review_reason});
    let harness = state.harness.clone();
    let workspace_for_read = workspace.clone();
    let workspace_id_for_read = workspace_id.clone();
    let project_started = std::time::Instant::now();
    let project = mcp_tools::run_blocking(move || {
        let project = harness.project_observatory(
            workspace_id_for_read,
            &workspace_for_read,
            review.as_ref(),
        )?;
        serde_json::to_value(project).map_err(Into::into)
    })
    .await;
    let project_ms = project_started.elapsed().as_secs_f64() * 1_000.0;
    let mut response = match project {
        Ok(mut value) => {
            value["workspace_options"] = intelligence_workspace_options(&state);
            value["git_review"] = git_review;
            value["activity"] = state.monitor.observatory_activity(&workspace_id);
            value["pending_authorizations"] = json!(state
                .workspaces
                .authorization_requests(256)
                .into_iter()
                .filter(|request| {
                    request.status == AuthorizationStatus::Pending
                        && request.workspace == workspace_id
                })
                .count());
            ([(header::CACHE_CONTROL, "no-store")], Json(value)).into_response()
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
        )
            .into_response(),
    };
    response.headers_mut().insert(
        axum::http::HeaderName::from_static("server-timing"),
        format!("git-review;dur={review_ms:.1}, project-snapshot;dur={project_ms:.1}")
            .parse()
            .expect("bounded server timing values form a valid header"),
    );
    response
}

pub(super) async fn intelligence_web_revision(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let (workspace_id, workspace) = match intelligence_ui_workspace(&state, &headers) {
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
    let harness = state.harness.clone();
    let workspace_for_read = workspace.clone();
    let id_for_read = workspace_id.clone();
    // File I/O stays off the async worker. The proof signal reads record
    // metadata only; it is invalidation information, never verification proof.
    let signals = mcp_tools::run_blocking(move || -> AnyResult<_> {
        let (graph, (proof, engineering)) = rayon::join(
            || harness.observatory_graph_signal(&workspace_for_read),
            || {
                rayon::join(
                    || harness.observatory_proof_signal(&id_for_read, &workspace_for_read),
                    || harness.observatory_engineering_signal(&workspace_for_read),
                )
            },
        );
        Ok((graph?, proof?, engineering?))
    })
    .await;
    let signal_failed = signals.is_err();
    let (graph_revision, graph_signal, proof_revision, engineering_revision) = match signals {
        Ok((graph, proof, engineering)) => {
            let (revision, signal) = graph
                .map(|(revision, signal)| (Some(revision), Some(signal)))
                .unwrap_or((None, None));
            (revision, signal, Some(proof), Some(engineering))
        }
        Err(_) => (None, None, None, None),
    };
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "workspace": workspace_id,
            "proof_revision": proof_revision,
            "engineering_revision": engineering_revision,
            "fingerprint": revision.fingerprint,
            "changed_files": revision.changed_files,
            "truncated": revision.truncated,
            "full_refresh_required": revision.full_refresh_required || signal_failed,
            "graph_revision": graph_revision,
            "graph_signal": graph_signal,
            "pending_authorizations": state
                .workspaces
                .authorization_requests(256)
                .into_iter()
                .filter(|request| {
                    request.status == AuthorizationStatus::Pending
                        && request.workspace == workspace_id
                })
                .count()
        })),
    )
        .into_response()
}

pub(super) async fn intelligence_web_refresh_semantics(
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

pub(super) async fn intelligence_web_status(
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
    let base_task = tokio::task::spawn_blocking(move || {
        web_status::snapshot(&harness, &workspace_id_for_read, &workspace_for_read)
    });
    let review_task = async {
        if workspace.exec_enabled() && workspace.root().join(".git").is_dir() {
            state
                .harness
                .review_changes(workspace_id.clone(), &workspace, 30, &state.monitor)
                .await
                .ok()
        } else {
            None
        }
    };
    let (base_result, review) = tokio::join!(base_task, review_task);
    let base = match base_result {
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
    let risk = review.and_then(|review| {
        state
            .harness
            .risk_status(workspace_id.clone(), &workspace, &review)
            .ok()
            .and_then(|risk| serde_json::to_value(risk).ok())
    });
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

pub(super) async fn intelligence_web_scopes(
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

pub(super) async fn intelligence_web_graph(
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
