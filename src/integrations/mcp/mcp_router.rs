use super::*;

pub fn router(state: Arc<AppState>) -> Router {
    crate::mcp_tasks::register_monitor_bridge(&state);
    Router::new()
        .route("/", get(setup_page))
        .route("/healthz", get(health))
        .route("/healthz/probe", get(health_probe))
        .route("/setup/status", get(setup_status))
        .route("/intelligence", get(intelligence_page))
        .route("/intelligence/app.css", get(intelligence_styles))
        .route("/intelligence/app.js", get(intelligence_script))
        .route("/intelligence/logo.svg", get(intelligence_logo))
        .route("/intelligence/project", get(intelligence_web_project))
        .route("/intelligence/revision", get(intelligence_web_revision))
        .route("/intelligence/jobs", get(intelligence_web_jobs))
        .route("/intelligence/jobs/{task_id}", get(intelligence_web_job))
        .route(
            "/intelligence/jobs/{task_id}/cancel",
            post(intelligence_web_job_cancel),
        )
        .route(
            "/intelligence/verification/tasks",
            get(intelligence_web_verification_tasks),
        )
        .route(
            "/intelligence/verification/run",
            post(intelligence_web_verification_run),
        )
        .route(
            "/intelligence/verification/{task_id}",
            get(intelligence_web_verification_status),
        )
        .route(
            "/intelligence/verification/{task_id}/result",
            get(intelligence_web_verification_result),
        )
        .route(
            "/intelligence/verification/{task_id}/cancel",
            post(intelligence_web_verification_cancel),
        )
        .route("/intelligence/diagnostics", get(intelligence_diagnostics))
        .route("/intelligence/activity", get(intelligence_web_activity))
        .route("/intelligence/code-graph", get(intelligence_web_code_graph))
        .route(
            "/intelligence/code-source",
            get(intelligence_web_code_source).post(intelligence_web_edit_source),
        )
        .route(
            "/intelligence/change-detail",
            get(intelligence_web_change_detail),
        )
        .route(
            "/intelligence/historical-source",
            get(intelligence_web_historical_source),
        )
        .route("/intelligence/revisions", get(intelligence_web_revisions))
        .route(
            "/intelligence/change-impact",
            get(intelligence_web_change_impact),
        )
        .route(
            "/intelligence/semantic-refresh",
            post(intelligence_web_refresh_semantics),
        )
        .route(
            "/intelligence/workspaces",
            get(intelligence_web_workspaces).post(intelligence_web_add_workspace),
        )
        .route(
            "/intelligence/commands",
            get(intelligence_web_commands)
                .post(intelligence_web_allow_command)
                .delete(intelligence_web_revoke_command),
        )
        .route(
            "/intelligence/command-trust",
            post(intelligence_web_enable_all_commands)
                .delete(intelligence_web_disable_all_commands),
        )
        .route(
            "/intelligence/authorizations",
            get(intelligence_web_authorizations)
                .post(intelligence_web_approve_authorization)
                .delete(intelligence_web_deny_authorization),
        )
        .route(
            "/intelligence/authorizations/approve-group",
            post(intelligence_web_approve_authorization_group),
        )
        .route(
            "/intelligence/command-operations",
            post(intelligence_web_authorize_command_operation),
        )
        .route("/intelligence/status", get(intelligence_web_status))
        .route("/intelligence/scopes", get(intelligence_web_scopes))
        .route("/intelligence/graph", get(intelligence_web_graph))
        .route("/mcp", get(mcp_get).post(mcp))
        .merge(crate::mcp_legacy_sse::routes())
        .layer(axum::middleware::from_fn(
            cache_policy::intelligence_no_store,
        ))
        .with_state(state)
}
