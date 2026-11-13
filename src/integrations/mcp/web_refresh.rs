use super::*;

pub(super) fn request_observatory_refresh(
    state: Arc<AppState>,
    workspace_id: String,
    workspace: crate::workspace::Workspace,
) -> bool {
    let Some(refresh_guard) = state.harness.begin_observatory_refresh(&workspace) else {
        return false;
    };
    tokio::spawn(async move {
        let revision_before = web_status::revision_state(&state.harness, &workspace_id, &workspace)
            .await
            .ok();
        let review = if !workspace.exec_enabled() || !workspace.root().join(".git").exists() {
            None
        } else {
            state
                .harness
                .review_changes(workspace_id.clone(), &workspace, 30, &state.monitor)
                .await
                .ok()
        };
        let built = state
            .harness
            .project_observatory_live(&workspace_id, &workspace, review.as_ref())
            .await;
        let mut confirmed = false;
        if built.is_ok() {
            let revision_after =
                web_status::revision_state(&state.harness, &workspace_id, &workspace)
                    .await
                    .ok();
            if let (Some(before), Some(after)) = (revision_before, revision_after) {
                if before.stable_inputs_key == after.stable_inputs_key {
                    confirmed = true;
                    state
                        .harness
                        .mark_observatory_revision(&workspace, after.full_snapshot_key);
                }
            }
        }
        if !confirmed {
            state.harness.invalidate_observatory_acceptance(&workspace);
        }
        drop(refresh_guard);
    });
    true
}

pub(in crate::mcp) async fn intelligence_web_project(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let (workspace_id, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return *response,
    };
    let execution = intelligence_execution_snapshot(workspace.clone()).await;
    let worklist_revision = public_worklist_revision(&execution);
    let prefer_cached = headers
        .get("x-wcode-prefer-cached")
        .and_then(|value| value.to_str().ok())
        == Some("1");
    let background_refresh = headers
        .get("x-wcode-background-refresh")
        .and_then(|value| value.to_str().ok())
        == Some("1");
    if prefer_cached {
        let cached = state.harness.cached_project_observatory_state(&workspace);
        if background_refresh && !state.harness.observatory_refreshing(&workspace) {
            let inputs_unchanged = match cached.as_ref().and_then(|(_, key)| key.as_deref()) {
                Some(base) => web_status::revision_state(&state.harness, &workspace_id, &workspace)
                    .await
                    .is_ok_and(|current| current.full_snapshot_key == base),
                None => false,
            };
            if !inputs_unchanged {
                request_observatory_refresh(state.clone(), workspace_id.clone(), workspace.clone());
            }
        }
        let refreshing = state.harness.observatory_refreshing(&workspace);
        if let Some((snapshot, revision_key)) = cached {
            if let Ok(mut value) = serde_json::to_value(snapshot) {
                value["workspace_options"] = intelligence_workspace_options(&state);
                value["execution"] = execution.clone();
                value["worklist_revision"] = worklist_revision.clone();
                web_status::attach_project_identity(&mut value, "cached_snapshot", false);
                value["git_observation"] = json!({"available":false,"reason":if workspace.exec_enabled() {"cached_snapshot"} else {"execution_disabled"}});
                if !workspace.exec_enabled() {
                    value["acceptance"] = Value::Null;
                }
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
                value["snapshot_cache"] = json!(if refreshing {
                    "stale-while-revalidate"
                } else {
                    "cached"
                });
                value["snapshot_refreshing"] = json!(refreshing);
                value["snapshot_revision"] = revision_key.map_or(Value::Null, |base| {
                    json!(worklist_snapshot_key(&base, &execution))
                });
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
                "execution": execution.clone(),
                "worklist_revision": worklist_revision.clone(),
                "activity": state.monitor.observatory_activity(&workspace_id),
                "pending_authorizations": intelligence_pending_authorizations(&state, &workspace_id).as_array().map_or(0, Vec::len),
                "snapshot_pending": true,
                "snapshot_refreshing": refreshing
            })),
        )
            .into_response();
    }
    let revision_before = web_status::revision_state(&state.harness, &workspace_id, &workspace)
        .await
        .ok();
    let review_started = std::time::Instant::now();
    let (review, review_reason, review_truncated) = if !workspace.exec_enabled() {
        (None, "execution_disabled", false)
    } else if !workspace.root().join(".git").exists() {
        (None, "not_a_repository", false)
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
                let truncated = report.truncated;
                (Some(report), reason, truncated)
            }
            Err(_) => (None, "review_failed", false),
        }
    };
    let review_ms = review_started.elapsed().as_secs_f64() * 1_000.0;
    let project_started = std::time::Instant::now();
    let project = state
        .harness
        .project_observatory_live(&workspace_id, &workspace, review.as_ref())
        .await
        .and_then(|project| serde_json::to_value(project).map_err(Into::into));
    let project_ms = project_started.elapsed().as_secs_f64() * 1_000.0;
    let revision_after = web_status::revision_state(&state.harness, &workspace_id, &workspace)
        .await
        .ok();
    let git_observation = revision_after
        .as_ref()
        .map(|revision| revision.git_observation.clone())
        .unwrap_or_else(|| json!({"available":false,"reason":"inspection_failed"}));
    let snapshot_revision = revision_before
        .zip(revision_after)
        .and_then(|(before, after)| {
            (before.stable_inputs_key == after.stable_inputs_key).then(|| {
                state
                    .harness
                    .mark_observatory_revision(&workspace, after.full_snapshot_key.clone());
                worklist_snapshot_key(&after.full_snapshot_key, &execution)
            })
        });
    let mut response = match project {
        Ok(mut value) => {
            if snapshot_revision.is_none() {
                value["acceptance"] = Value::Null;
                state.harness.invalidate_observatory_acceptance(&workspace);
            }
            if !workspace.exec_enabled() {
                value["acceptance"] = Value::Null;
            }
            value["snapshot_revision"] = json!(snapshot_revision);
            value["git_observation"] = git_observation;
            value["workspace_options"] = intelligence_workspace_options(&state);
            value["worklist_revision"] = worklist_revision;
            value["execution"] = execution;
            web_status::attach_project_identity(&mut value, review_reason, review_truncated);
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

pub(in crate::mcp) async fn intelligence_web_refresh_semantics(
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

pub(in crate::mcp) async fn intelligence_web_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let (workspace_id, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return no_store(*response),
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
    let base =
        match base_result {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => {
                return no_store(
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": error.to_string()})),
                    )
                        .into_response(),
                )
            }
            Err(error) => return no_store(
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": format!("intelligence dashboard task failed: {error}")})),
                )
                    .into_response(),
            ),
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
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(value),
    )
        .into_response()
}

pub(in crate::mcp) async fn intelligence_web_scopes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let (workspace_id, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return no_store(*response),
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
                [(header::CACHE_CONTROL, "no-store")],
                Json(json!({"workspace": workspace_id, "scope_status": status})),
            )
                .into_response()
        }
        Err(error) => no_store(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": error.to_string()})),
            )
                .into_response(),
        ),
    }
}

pub(in crate::mcp) async fn intelligence_web_graph(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let (_, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return no_store(*response),
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
        Ok(Ok(graph)) => (
            StatusCode::OK,
            [(header::CACHE_CONTROL, "no-store")],
            Json(json!(graph)),
        )
            .into_response(),
        Ok(Err(error)) => no_store(
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": error.to_string()})),
            )
                .into_response(),
        ),
        Err(error) => no_store(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("graph dashboard task failed: {error}")})),
            )
                .into_response(),
        ),
    }
}
