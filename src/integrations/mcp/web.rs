use super::*;

pub(super) async fn setup_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let capabilities = state.workspaces.capabilities();
    let workspace_count = capabilities["workspaces"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default();
    let default_workspace = capabilities["default_workspace"]
        .as_str()
        .unwrap_or("unknown");
    let base = state
        .auth
        .request_public_url(&headers)
        .unwrap_or_else(|| state.auth.public_url());
    let mcp_url = format!("{base}/mcp");
    let endpoints_html = {
        let tunnels = state.monitor.tunnel_links();
        let rows = if tunnels.is_empty() {
            format!(r#"<div class="endpoint">{mcp_url}</div>"#)
        } else {
            tunnels
                .iter()
                .map(|(provider, url)| {
                    format!(
                        r#"<div class="endpoint"><span class="p">{provider}</span><span>{url}/mcp</span></div>"#
                    )
                })
                .collect::<String>()
        };
        // The page is served once; tunnels keep joining afterwards, so the
        // client script below refreshes this container from /healthz.
        format!(r#"<div id="endpoints">{rows}</div>"#)
    };
    axum::response::Html(format!(
        r##"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="dark"><title>wcode · Software Intelligence Runtime</title>
<style>*{{box-sizing:border-box}}body{{margin:0;min-height:100vh;display:grid;place-items:center;background:radial-gradient(800px 450px at 50% -10%,#24242b,#09090b 65%);color:#f4f4f5;font:14px/1.55 ui-sans-serif,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;padding:24px}}main{{width:min(100%,720px)}}.brand{{display:flex;align-items:center;gap:11px;margin:0 0 18px 4px}}.logo{{width:34px;height:34px;border:1px solid #3a3a42;border-radius:10px;display:grid;place-items:center;background:#151518;font:700 14px ui-monospace,monospace}}.muted{{color:#8d8d98}}.card{{border:1px solid #29292f;border-radius:18px;background:linear-gradient(180deg,#151519,#101013);padding:26px;box-shadow:0 28px 80px #0008}}h1{{margin:0 0 6px;font-size:23px}}.status{{display:inline-flex;align-items:center;gap:7px;color:#a7f3bd;font-size:12px;margin-bottom:22px}}.dot{{width:7px;height:7px;background:#5ee28a;border-radius:50%;box-shadow:0 0 12px #5ee28a88}}.endpoint{{display:flex;align-items:center;justify-content:space-between;gap:15px;margin-bottom:8px;padding:13px 15px;border:1px solid #29292f;border-radius:12px;background:#09090b;font:12px ui-monospace,SFMono-Regular,Menlo,monospace;overflow:auto}}.grid{{display:grid;grid-template-columns:repeat(4,1fr);gap:10px;margin-top:12px}}.stat{{padding:13px;border:1px solid #28282e;border-radius:12px;background:#111114}}.stat b{{display:block;font-size:18px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}}.stat span{{font-size:11px;color:#84848f}}.clients{{display:grid;grid-template-columns:repeat(5,1fr);gap:8px;margin-top:18px}}.client{{display:flex;align-items:center;justify-content:center;min-height:44px;padding:0 10px;border:1px solid #323239;border-radius:11px;background:#0c0c0f;color:#f4f4f5;font-weight:650;font-size:12px;text-decoration:none}}.client:hover{{border-color:#696973;background:#17171b}}.endpoint .p{{color:#8d8d98;font-size:11px;text-transform:uppercase;letter-spacing:.04em}}.hint{{margin-top:12px;color:#73737d;font-size:11px}}footer{{display:flex;justify-content:space-between;gap:14px;flex-wrap:wrap;margin-top:15px;padding:0 4px;color:#72727d;font-size:12px}}a{{color:#b8b8c0;text-decoration:none}}a:hover{{color:#fff}}@media(max-width:720px){{.grid{{grid-template-columns:repeat(2,1fr)}}.clients{{grid-template-columns:repeat(2,1fr)}}}}@media(max-width:520px){{.grid{{grid-template-columns:1fr}}.clients{{grid-template-columns:1fr}}}}</style></head>
<body><main><div class="brand"><div class="logo">WC</div><div><strong>wcode</strong><div class="muted">Software Intelligence Runtime</div></div></div><section class="card"><div class="status"><i class="dot"></i>Runtime ready</div><h1>Connect a model executor</h1><p class="muted">wcode owns the local Design State, software context, risk, verification, and evidence layer. Connect any supported MCP model or agent below as a replaceable executor.</p><div style="height:20px"></div>{endpoints_html}<div class="clients"><a class="client" href="{grok_url}" target="_blank" rel="noreferrer">Grok ↗</a><a class="client" href="{claude_url}" target="_blank" rel="noreferrer">Claude ↗</a><a class="client" href="{chatgpt_url}" target="_blank" rel="noreferrer">ChatGPT ↗</a><a class="client" href="{mistral_url}" target="_blank" rel="noreferrer">Mistral ↗</a><a class="client" href="{docs_url}#clients" target="_blank" rel="noreferrer">Other MCP ↗</a></div><div class="hint">MCP is the model access layer. Runtime state and software intelligence remain provider-neutral inside wcode.</div><div class="grid"><div class="stat"><b>{workspace_count}</b><span>workspace roots</span></div><div class="stat"><b>{}</b><span>parallel slots</span></div><div class="stat"><b>{intelligence_capability_count}</b><span>intelligence capabilities</span></div><div class="stat"><b>{default_workspace}</b><span>default workspace</span></div></div></section><footer><a href="{docs_url}" target="_blank" rel="noreferrer">Docs ↗</a><a href="{project_url}" target="_blank" rel="noreferrer">Project ↗</a><a href="{author_url}" target="_blank" rel="noreferrer">{author_handle} ↗</a></footer></main><script>(function(){{var el=document.getElementById('endpoints');function esc(v){{return String(v==null?'':v).replace(/[&<>"']/g,function(c){{return {{'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}}[c]}})}}function render(){{fetch('/healthz').then(function(r){{return r.ok?r.json():null}}).then(function(d){{if(!d)return;var t=d.tunnels||[];el.innerHTML=t.length?t.map(function(x){{return '<div class="endpoint"><span class="p">'+esc(x.provider)+'</span><span>'+esc(x.url)+'/mcp</span></div>'}}).join(''):'<div class="endpoint">'+esc(d.mcp_url)+'</div>'}}).catch(function(){{}})}}setInterval(render,6000);render()}})();</script></body></html>"##,
        state.harness.max_parallel(),
        intelligence_capability_count = state.harness.intelligence_capability_count(),
        chatgpt_url = CHATGPT_CONNECTOR_SETUP_URL,
        grok_url = GROK_CONNECTOR_SETUP_URL,
        claude_url = CLAUDE_CONNECTOR_SETUP_URL,
        mistral_url = MISTRAL_CONNECTOR_SETUP_URL,
        docs_url = DOCS_URL,
        project_url = PROJECT_URL,
        author_url = AUTHOR_URL,
        author_handle = AUTHOR_HANDLE,
    ))
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

pub(super) fn intelligence_ui_authorized(
    state: &AppState,
    headers: &HeaderMap,
) -> std::result::Result<(), Box<Response>> {
    let Some(public_url) = state.auth.request_public_url(headers) else {
        return Err(Box::new(forbidden_origin_response()));
    };
    if !origin_allowed(&public_url, headers) {
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

pub(super) async fn intelligence_web_project(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let (workspace_id, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return *response,
    };
    let review = if workspace.exec_enabled() && workspace.root().join(".git").is_dir() {
        state
            .harness
            .review_changes(workspace_id.clone(), &workspace, 30, &state.monitor)
            .await
            .ok()
    } else {
        None
    };
    let harness = state.harness.clone();
    let workspace_for_read = workspace.clone();
    let workspace_id_for_read = workspace_id.clone();
    let project = mcp_tools::run_blocking(move || {
        let project = harness.project_observatory(
            workspace_id_for_read,
            &workspace_for_read,
            review.as_ref(),
        )?;
        serde_json::to_value(project).map_err(Into::into)
    })
    .await;
    match project {
        Ok(mut value) => {
            value["workspace_options"] = intelligence_workspace_options(&state);
            value["pending_authorizations"] = json!(state
                .workspaces
                .authorization_requests(256)
                .into_iter()
                .filter(|request| {
                    request.status == AuthorizationStatus::Pending
                        && request.workspace == workspace_id
                })
                .count());
            (StatusCode::OK, Json(value)).into_response()
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
        )
            .into_response(),
    }
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
    let graph_revision = state
        .harness
        .graph_history(&workspace, 1)
        .ok()
        .and_then(|history| history.into_iter().next())
        .map(|entry| entry.id);
    (
        StatusCode::OK,
        Json(json!({
            "fingerprint": revision.fingerprint,
            "changed_files": revision.changed_files,
            "truncated": revision.truncated,
            "full_refresh_required": revision.full_refresh_required,
            "graph_revision": graph_revision,
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
    let base = match tokio::task::spawn_blocking(move || -> AnyResult<Value> {
        let design = harness.design_status(workspace_id_for_read.clone(), &workspace_for_read)?;
        let traceability =
            harness.traceability_status(workspace_id_for_read.clone(), &workspace_for_read)?;
        let semantics =
            harness.semantic_status(&workspace_id_for_read, &workspace_for_read, 100)?;
        let scope_status = harness.product_scope_status(&workspace_for_read)?;
        let graph_history = harness.graph_history(&workspace_for_read, 20)?;
        let graph_diff = if graph_history.len() >= 2 {
            harness
                .graph_diff(
                    &workspace_for_read,
                    &GraphDiffInput {
                        from_snapshot_id: None,
                        to_snapshot_id: None,
                        limit: 20,
                    },
                )
                .ok()
        } else {
            None
        };
        let graph_providers = harness.graph_provider_status(&workspace_for_read)?;
        let semantic_providers = harness.semantic_provider_status(&workspace_for_read)?;
        let verification_executors = harness.verification_executor_status(&workspace_for_read)?;
        let evidence =
            harness.evidence_status(&workspace_id_for_read, &workspace_for_read, None, 100)?;
        let reconciliation = harness.reconciliation_history(&workspace_for_read, 20)?;
        let verification =
            harness.verification_history(&workspace_id_for_read, &workspace_for_read, 20)?;
        let mut reconciliation_execution = Vec::new();
        for plan in reconciliation.iter().take(20) {
            if let Ok(status) = harness.reconciliation_execution_status(
                &workspace_id_for_read,
                &workspace_for_read,
                &plan.id,
            ) {
                reconciliation_execution.push(status);
            }
        }
        Ok(json!({
            "design": design,
            "traceability": traceability,
            "semantics": semantics,
            "scope_status": scope_status,
            "graph_history": graph_history,
            "graph_diff": graph_diff,
            "graph_providers": graph_providers,
            "semantic_providers": semantic_providers,
            "verification_executors": verification_executors,
            "evidence": evidence,
            "reconciliation": reconciliation,
            "reconciliation_execution": reconciliation_execution,
            "verification": verification,
        }))
    })
    .await
    {
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
    let risk = if workspace.exec_enabled() && workspace.root().join(".git").is_dir() {
        match state
            .harness
            .review_changes(workspace_id.clone(), &workspace, 30, &state.monitor)
            .await
        {
            Ok(review) => state
                .harness
                .risk_status(workspace_id.clone(), &workspace, &review)
                .ok()
                .and_then(|risk| serde_json::to_value(risk).ok()),
            Err(_) => None,
        }
    } else {
        None
    };
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
