use super::*;

pub(super) fn public_worklist_revision(execution: &Value) -> Value {
    let worklist = &execution["worklist"];
    match (
        worklist["available"].as_bool(),
        worklist["exists"].as_bool(),
        worklist["revision"].as_u64(),
    ) {
        (Some(true), Some(exists), Some(revision)) => {
            json!({"available":true,"exists":exists,"revision":revision})
        }
        _ => json!({"available":false}),
    }
}

pub(super) fn worklist_snapshot_key(base: &str, execution: &Value) -> String {
    let signal = public_worklist_revision(execution);
    match (signal["exists"].as_bool(), signal["revision"].as_u64()) {
        (Some(exists), Some(revision)) => {
            format!("{base}|worklist:{}:{revision}", u8::from(exists))
        }
        _ => format!("{base}|worklist:unknown"),
    }
}

pub(super) async fn intelligence_execution_snapshot(
    workspace: crate::workspace::Workspace,
) -> Value {
    match mcp_tools::run_blocking(move || {
        let mut execution = crate::execution::stored_status(&workspace)?;
        execution["worklist"] = match crate::worklist::status(&workspace) {
            Ok(mut status) => {
                status["available"] = Value::Bool(true);
                status
            }
            Err(_) => json!({"available": false}),
        };
        Ok(execution)
    })
    .await
    {
        Ok(mut value) => {
            value["available"] = Value::Bool(true);
            value
        }
        Err(_) => json!({
            "available": false,
            "exists": false,
            "active": false,
            "revision": 0,
            "checkpoint": Value::Null,
            "proposal": Value::Null,
            "reason": "execution_status_unavailable"
        }),
    }
}
pub(crate) async fn intelligence_web_revision(
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
        let worklist = crate::worklist::status(&workspace_for_read)?;
        let worklist_revision =
            json!({"available":true,"exists":worklist["exists"],"revision":worklist["revision"]});
        Ok((graph?, proof?, engineering?, worklist_revision))
    })
    .await;
    let signal_failed = signals.is_err();
    let (graph_revision, graph_signal, proof_revision, engineering_revision, worklist_revision) =
        match signals {
            Ok((graph, proof, engineering, worklist)) => {
                let (revision, signal) = graph
                    .map(|(revision, signal)| (Some(revision), Some(signal)))
                    .unwrap_or((None, None));
                (
                    revision,
                    signal,
                    Some(proof),
                    Some(engineering),
                    Some(worklist),
                )
            }
            Err(_) => (None, None, None, None, None),
        };
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "workspace": workspace_id,
            "proof_revision": proof_revision,
            "worklist_revision": worklist_revision,
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
