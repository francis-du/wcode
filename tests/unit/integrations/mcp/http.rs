use super::*;
use crate::authorization::AuthorizationKind;
use crate::workspace::WorkspaceSecurity;
use axum::body::to_bytes;
use axum::extract::State;
use std::fs;

#[tokio::test]
async fn observatory_shell_and_static_assets_are_served_without_api_authorization() {
    let page = intelligence_page().await;
    assert_eq!(page.status(), StatusCode::OK);
    assert_eq!(page.headers()[header::CACHE_CONTROL], "no-store");
    assert!(page.headers()[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap()
        .contains("script-src 'self'"));
    let page_body = to_bytes(page.into_body(), 64 * 1024).await.unwrap();
    let page_body = std::str::from_utf8(&page_body).unwrap();
    assert!(page_body.contains("/intelligence/app.js"));
    assert!(page_body.contains("/intelligence/app.css"));

    let script = intelligence_script().await;
    assert_eq!(script.status(), StatusCode::OK);
    assert_eq!(
        script.headers()[header::CONTENT_TYPE],
        "text/javascript; charset=utf-8"
    );
    let script_body = to_bytes(script.into_body(), 512 * 1024).await.unwrap();
    let script_body = std::str::from_utf8(&script_body).unwrap();
    assert!(script_body.contains("function startObservatory()"));
    assert!(script_body.contains("applyTheme();"));

    let styles = intelligence_styles().await;
    assert_eq!(styles.status(), StatusCode::OK);
    assert_eq!(
        styles.headers()[header::CONTENT_TYPE],
        "text/css; charset=utf-8"
    );
    let style_body = to_bytes(styles.into_body(), 512 * 1024).await.unwrap();
    assert!(!style_body.is_empty());

    let logo = intelligence_logo().await;
    assert_eq!(logo.status(), StatusCode::OK);
    assert_eq!(
        logo.headers()[header::CONTENT_TYPE],
        "image/svg+xml; charset=utf-8"
    );
}

#[tokio::test]
async fn observatory_activity_is_protected_scoped_bounded_and_has_no_arguments() {
    let (state, root) = origin_test_state();
    let workspace = state.workspaces.default_id().to_owned();
    let mut missing_token = ui_headers(&state, &workspace);
    missing_token.remove("x-wcode-ui-token");
    assert_eq!(
        intelligence_web_activity(State(state.clone()), missing_token)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    for (key, value) in [
        ("host", "untrusted.example"),
        ("origin", "https://untrusted.example"),
    ] {
        let mut headers = ui_headers(&state, &workspace);
        headers.insert(key, value.parse().unwrap());
        assert_eq!(
            intelligence_web_activity(State(state.clone()), headers)
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    for _ in 0..16 {
        state
            .monitor
            .queue(&workspace, "old_read", "PRIVATE-ARGUMENT", 1)
            .finish(true, 1);
    }
    let mut running = state
        .monitor
        .queue(&workspace, "read_file", "PRIVATE-ARGUMENT", 1);
    running.start();
    let queued = state
        .monitor
        .queue(&workspace, "find_symbol", "PRIVATE-ARGUMENT", 1);
    let mut foreign =
        state
            .monitor
            .queue("unrelated-project", "FOREIGN-TOOL", "PRIVATE-ARGUMENT", 1);
    foreign.start();
    let before = state.monitor.connection_status();
    let response =
        intelligence_web_activity(State(state.clone()), ui_headers(&state, &workspace)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let body = response_json(response).await;
    assert_eq!(body["workspace"], workspace);
    assert_eq!(body["resource_scope"], "whole_process");
    assert_eq!(body["activity"]["active"], 1);
    assert_eq!(body["activity"]["queued"], 1);
    assert_eq!(body["activity"]["recent"].as_array().unwrap().len(), 12);
    assert_eq!(body["activity"]["recent_truncated"], true);
    assert_eq!(body["activity"]["recent"][0]["status"], "running");
    assert_eq!(body["activity"]["recent"][1]["status"], "queued");
    assert!(!body.to_string().contains("PRIVATE-ARGUMENT"));
    assert!(!body.to_string().contains("FOREIGN-TOOL"));
    assert!(!body.to_string().contains(state.auth.ui_token()));
    assert_eq!(
        before.active_tasks,
        state.monitor.connection_status().active_tasks
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    drop((running, queued, foreign));
}

#[tokio::test]
async fn observatory_project_distinguishes_unavailable_review_from_clean() {
    let (state, _root) = origin_test_state();
    let workspace = state.workspaces.default_id().to_owned();
    let response =
        intelligence_web_project(State(state.clone()), ui_headers(&state, &workspace)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let timing = response.headers()["server-timing"].to_str().unwrap();
    assert!(timing.contains("git-review;dur="));
    assert!(timing.contains("project-snapshot;dur="));
    let value = response_json(response).await;
    assert_eq!(value["git_review"]["available"], false);
    assert_eq!(value["git_review"]["reason"], "execution_disabled");
    assert_eq!(value["activity"]["workspace"], workspace);
    assert_eq!(value["proof"]["current_evidence"], 0);
}

#[tokio::test]
async fn observatory_project_serves_cached_snapshot_before_heavy_refresh() {
    let (state, _root) = origin_test_state();
    let workspace = state.workspaces.default_id().to_owned();
    let mut cached_headers = ui_headers(&state, &workspace);
    cached_headers.insert("x-wcode-prefer-cached", "1".parse().unwrap());

    let cold = intelligence_web_project(State(state.clone()), cached_headers.clone()).await;
    assert_eq!(cold.status(), StatusCode::OK);
    let cold = response_json(cold).await;
    assert_eq!(cold["snapshot_pending"], true);

    let full = intelligence_web_project(State(state.clone()), ui_headers(&state, &workspace)).await;
    assert_eq!(full.status(), StatusCode::OK);
    let _ = response_json(full).await;

    let cached = intelligence_web_project(State(state.clone()), cached_headers).await;
    assert_eq!(cached.status(), StatusCode::OK);
    assert!(cached.headers()["server-timing"]
        .to_str()
        .unwrap()
        .contains("snapshot-cache"));
    let cached = response_json(cached).await;
    assert_eq!(cached["snapshot_cache"], "stale-while-revalidate");
    assert_eq!(cached["git_review"]["reason"], "cached_snapshot");
}

#[tokio::test]
async fn observatory_revision_exposes_proof_freshness_without_starting_commands() {
    let (state, _root) = origin_test_state();
    let workspace = state.workspaces.default_id().to_owned();
    let response =
        intelligence_web_revision(State(state.clone()), ui_headers(&state, &workspace)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let value = response_json(response).await;
    assert_eq!(value["workspace"], workspace);
    assert!(value.get("graph_signal").is_some());
    assert!(value.get("proof_revision").is_some());
    assert!(value.get("engineering_revision").is_some());
    assert!(value["fingerprint"].as_str().is_some());
    assert_eq!(value["full_refresh_required"], false);
    assert_eq!(state.monitor.connection_status().active_tasks, 0);
}

fn ui_headers(state: &AppState, workspace: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("host", "127.0.0.1:8765".parse().unwrap());
    headers.insert("origin", "http://127.0.0.1:8765".parse().unwrap());
    headers.insert("x-wcode-ui-token", state.auth.ui_token().parse().unwrap());
    headers.insert("x-wcode-workspace", workspace.parse().unwrap());
    headers
}

async fn response_json(response: Response) -> Value {
    let body = to_bytes(response.into_body(), 256 * 1024)
        .await
        .expect("handler response body");
    serde_json::from_slice(&body).expect("handler JSON response")
}

fn origin_test_state() -> (Arc<AppState>, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([root.path()], false, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let auth = Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned()));
    auth.set_public_url("https://primary.example".to_owned());
    auth.register_public_url("https://secondary.example".to_owned());
    auth.insert_test_access_token(
        "test-origin-access",
        "test-client",
        "https://primary.example/mcp",
    );
    (
        Arc::new(AppState {
            auth,
            workspaces,
            harness: ToolHarness::new(2).unwrap(),
            monitor: TaskMonitor::new([workspace_id]),
            tasks: TaskRuntime::default(),
        }),
        root,
    )
}

#[tokio::test]
async fn mcp_and_webui_accept_verified_alias_origins_without_skipping_authentication() {
    let (state, _root) = origin_test_state();
    for host in ["primary.example", "secondary.example", "127.0.0.1:8765"] {
        for origin in ["https://PRIMARY.EXAMPLE:443", "https://secondary.example"] {
            let mut headers = HeaderMap::new();
            headers.insert("host", host.parse().unwrap());
            headers.insert("origin", origin.parse().unwrap());
            headers.insert(
                "authorization",
                "Bearer test-origin-access".parse().unwrap(),
            );
            assert_eq!(
                mcp_get(State(state.clone()), headers.clone())
                    .await
                    .status(),
                StatusCode::METHOD_NOT_ALLOWED
            );
            let response = mcp(
                State(state.clone()),
                headers.clone(),
                Json(json!({"jsonrpc":"2.0","id":1,"method":"ping"})),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            assert!(response_json(response).await.get("result").is_some());
            headers.remove("authorization");
            assert_eq!(
                mcp_get(State(state.clone()), headers.clone())
                    .await
                    .status(),
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                intelligence_ui_authorized(&state, &headers)
                    .unwrap_err()
                    .status(),
                StatusCode::UNAUTHORIZED
            );
            headers.insert("x-wcode-ui-token", state.auth.ui_token().parse().unwrap());
            assert!(intelligence_ui_authorized(&state, &headers).is_ok());
        }
    }
}

#[tokio::test]
async fn transport_rejections_distinguish_host_origin_and_retired_aliases() {
    let (state, _root) = origin_test_state();
    for (host, origin, reason) in [
        (
            "attacker.example",
            "https://primary.example",
            "untrusted_host",
        ),
        (
            "primary.example",
            "https://attacker.example",
            "untrusted_origin",
        ),
    ] {
        let mut headers = HeaderMap::new();
        headers.insert("host", host.parse().unwrap());
        headers.insert("origin", origin.parse().unwrap());
        headers.insert("x-forwarded-host", "primary.example".parse().unwrap());
        headers.insert(
            "authorization",
            "Bearer test-origin-access".parse().unwrap(),
        );
        let response = mcp_get(State(state.clone()), headers.clone()).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = response_json(response).await;
        assert_eq!(body["error"]["data"]["reason"], reason);
        assert!(!body.to_string().contains("test-origin-access"));
        let response = mcp(
            State(state.clone()),
            headers.clone(),
            Json(json!({"jsonrpc":"2.0","id":1,"method":"ping"})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            response_json(response).await["error"]["data"]["reason"],
            reason
        );
        headers.insert("x-wcode-ui-token", state.auth.ui_token().parse().unwrap());
        assert_eq!(
            intelligence_ui_authorized(&state, &headers)
                .unwrap_err()
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    state
        .auth
        .unregister_public_url("https://secondary.example");
    let mut headers = HeaderMap::new();
    headers.insert("host", "primary.example".parse().unwrap());
    headers.insert("origin", "https://secondary.example".parse().unwrap());
    let response = mcp_get(State(state), headers).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response_json(response).await["error"]["data"]["reason"],
        "untrusted_origin"
    );
}

#[tokio::test]
async fn setup_hub_is_mobile_safe_and_stops_polling_while_hidden() {
    let root = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([root.path()], false, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = Arc::new(AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(2).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    });
    let response = setup_page(State(state), HeaderMap::new())
        .await
        .into_response();
    let body = to_bytes(response.into_body(), 512 * 1024)
        .await
        .expect("setup page body");
    let html = String::from_utf8(body.to_vec()).expect("setup page is UTF-8");

    for contract in [
        "viewport-fit=cover",
        "min-height:100dvh",
        "safe-area-inset-bottom",
        "aria-live=\"polite\"",
        "grid-template-columns:repeat(2,minmax(0,1fr))",
        "document.hidden",
        "setTimeout(tick,6000)",
    ] {
        assert!(html.contains(contract), "missing {contract}");
    }
    assert!(!html.contains("setInterval("));
    assert!(!html.contains("style=\"height:20px\""));
}

#[tokio::test]
async fn setup_page_has_nonce_bound_assets_and_no_operator_credential() {
    let (state, _root) = origin_test_state();
    let mut nonces = std::collections::HashSet::new();
    for _ in 0..2 {
        let response = setup_page(State(state.clone()), HeaderMap::new()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()[header::REFERRER_POLICY], "no-referrer");
        let policy = response.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .to_owned();
        let nonce = policy
            .split("'nonce-")
            .nth(1)
            .unwrap()
            .split('\'')
            .next()
            .unwrap()
            .to_owned();
        assert!(
            nonces.insert(nonce.clone()),
            "each response needs a fresh nonce"
        );
        assert!(policy.contains("frame-ancestors 'none'"));
        assert!(policy.contains("img-src 'self'"));
        let body = to_bytes(response.into_body(), 128 * 1024).await.unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(html.contains(&format!("<script nonce=\"{nonce}\">")));
        assert!(html.contains(&format!("<style nonce=\"{nonce}\">")));
        assert!(!html.contains(state.auth.ui_token()));
    }
}

#[tokio::test]
async fn setup_status_is_compact_and_preserves_connection_truth() {
    let (state, _root) = origin_test_state();
    state
        .monitor
        .register_tunnel("test", "https://secondary.example");
    state.monitor.mark_tunnel_retry(
        "retrying-provider",
        4,
        true,
        std::time::Duration::from_secs(60),
        false,
    );
    state.monitor.mark_public_url_check(true, None);
    state.monitor.mark_mcp_initialized();
    let full = health(State(state.clone()), HeaderMap::new()).await.0;
    let response = setup_status(State(state.clone()), HeaderMap::new()).await;
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let compact = response_json(response).await;
    for key in ["ok", "mcp_url", "public_endpoint", "public_url_healthy"] {
        assert_eq!(compact[key], full[key], "connection field {key}");
    }
    assert_eq!(compact["mcp_initialized"], true);
    assert!(compact["mcp_last_seen_seconds_ago"].as_u64().is_some());
    assert_eq!(compact["public_url_healthy"], true);
    assert_eq!(compact["tunnels"].as_array().unwrap().len(), 1);
    assert_eq!(full["tunnels"].as_array().unwrap().len(), 2);
    let compact_live = &compact["tunnels"][0];
    let full_live = full["tunnels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tunnel| tunnel["provider"] == "test")
        .unwrap();
    assert_eq!(compact_live["mcp_url"], full_live["mcp_url"]);
    assert_eq!(compact_live["role"], "standby");
    assert_eq!(compact_live["state"], "verified");
    let retrying = full["tunnels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tunnel| tunnel["provider"] == "retrying-provider")
        .unwrap();
    assert!(retrying["url"].is_null());
    assert_eq!(retrying["state"], "circuit-open");
    assert_eq!(retrying["death_count"], 4);
    assert_eq!(retrying["circuit_open"], true);
    for key in ["workspaces", "resources", "harness", "allowed_commands"] {
        assert!(compact.get(key).is_none());
    }
    let full_bytes = serde_json::to_vec(&full).unwrap().len();
    let compact_bytes = serde_json::to_vec(&compact).unwrap().len();
    assert!(
        compact_bytes * 5 < full_bytes,
        "setup poll must avoid full runtime discovery"
    );
    assert!(!compact.to_string().contains(state.auth.ui_token()));
    let report = json!({"full_status_bytes":full_bytes,"setup_status_bytes":compact_bytes,
        "scope":"same in-process fixture; serialized payload, not end-to-end latency"});
    let target =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/wcode-setup-status.json");
    fs::write(target, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}

fn cargo_project(root: &std::path::Path, name: &str) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn value() -> u8 { 1 }\n").unwrap();
}

#[tokio::test]
async fn webui_command_approval_gates_executable_access_without_re_gating_safe_toolchains() {
    let root = tempfile::tempdir().unwrap();
    cargo_project(root.path(), "parent_project");
    let child_root = root.path().join("child");
    cargo_project(&child_root, "child_project");

    let workspaces =
        Workspaces::new_with_security([root.path()], true, true, WorkspaceSecurity::default())
            .unwrap();
    let parent_id = workspaces.default_id().to_owned();
    let (child_id, _) = workspaces
        .add_workspace_from(Some(&parent_id), "child")
        .unwrap();
    workspaces
        .revoke_command(Some(&parent_id), "cargo")
        .unwrap();
    let (_, parent) = workspaces.select(Some(&parent_id)).unwrap();
    let (_, child) = workspaces.select(Some(&child_id)).unwrap();
    let state = Arc::new(AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(2).unwrap(),
        monitor: TaskMonitor::new([parent_id.clone(), child_id.clone()]),
        tasks: TaskRuntime::default(),
    });
    let test_args = vec!["test".to_owned()];

    let first = parent
        .run_command("cargo", &test_args, ".", 30)
        .await
        .unwrap_err();
    assert!(first.to_string().contains("authorization required"));
    let executable_request = state.workspaces.latest_pending_authorization().unwrap();
    assert_eq!(executable_request.kind, AuthorizationKind::CommandAccess);
    let approved = intelligence_web_approve_authorization(
        State(state.clone()),
        ui_headers(&state, &parent_id),
        Json(json!({"id": executable_request.id})),
    )
    .await;
    assert_eq!(approved.status(), StatusCode::OK);

    let executed = parent
        .run_command("cargo", &test_args, ".", 30)
        .await
        .expect("approved executable should run a bounded test without a second authorization");
    assert!(executed.success, "cargo test failed: {}", executed.stderr);

    let different_args = vec!["test".to_owned(), "--lib".to_owned()];
    let different = parent
        .run_command("cargo", &different_args, ".", 30)
        .await
        .expect("another bounded test shape should not need exact-operation approval");
    assert!(
        different.success,
        "cargo test --lib failed: {}",
        different.stderr
    );
    assert!(state.workspaces.latest_pending_authorization().is_none());

    let child_clean = child
        .run_command("cargo", &["clean".to_owned()], ".", 30)
        .await
        .expect("catalogued development commands should not need a separate risky-exec approval");
    assert!(
        child_clean.success,
        "cargo clean failed: {}",
        child_clean.stderr
    );
    assert!(state.workspaces.latest_pending_authorization().is_none());

    let visible = response_json(
        intelligence_web_authorizations(State(state.clone()), ui_headers(&state, &child_id)).await,
    )
    .await;
    assert!(visible["pending"].as_array().unwrap().is_empty());
}

#[test]
fn exact_operation_endpoint_does_not_implicitly_allow_an_executable() {
    let root = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([root.path()], true, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    workspaces
        .revoke_command(Some(&workspace_id), "cargo")
        .unwrap();

    let error = workspaces
        .authorize_command_operation(Some(&workspace_id), "cargo", &["test".to_owned()], ".")
        .unwrap_err()
        .to_string();
    assert!(error.contains("Executable access") || error.contains("executable access"));
    assert!(
        !workspaces.workspace_access(Some(&workspace_id)).unwrap()["allowed_commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|program| program == "cargo")
    );
}

#[tokio::test]
async fn webui_workspace_command_trust_can_be_enabled_and_revoked() {
    let root = tempfile::tempdir().unwrap();
    cargo_project(root.path(), "command_trust_project");
    let workspaces = Workspaces::new([root.path()], true, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    workspaces
        .revoke_command(Some(&workspace_id), "cargo")
        .unwrap();
    let (_, workspace) = workspaces.select(Some(&workspace_id)).unwrap();
    let state = Arc::new(AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(2).unwrap(),
        monitor: TaskMonitor::new([workspace_id.clone()]),
        tasks: TaskRuntime::default(),
    });

    let first = workspace
        .run_command("cargo", &["--version".to_owned()], ".", 30)
        .await
        .unwrap_err();
    assert!(first.to_string().contains("authorization required"));

    let enabled = response_json(
        intelligence_web_enable_all_commands(
            State(state.clone()),
            ui_headers(&state, &workspace_id),
        )
        .await,
    )
    .await;
    assert_eq!(enabled["all_commands_authorized"], true);
    assert!(state
        .workspaces
        .all_commands_authorized(Some(&workspace_id))
        .unwrap());
    assert!(state
        .workspaces
        .authorization_requests(32)
        .iter()
        .all(
            |request| request.kind == AuthorizationKind::DestructiveDelete
                || request.status != crate::authorization::AuthorizationStatus::Pending
        ));

    let result = workspace
        .run_command("cargo", &["--version".to_owned()], ".", 30)
        .await
        .expect("workspace command trust should avoid another command-access request");
    assert!(result.success);

    let disabled = response_json(
        intelligence_web_disable_all_commands(
            State(state.clone()),
            ui_headers(&state, &workspace_id),
        )
        .await,
    )
    .await;
    assert_eq!(disabled["all_commands_authorized"], false);
    assert!(!state
        .workspaces
        .all_commands_authorized(Some(&workspace_id))
        .unwrap());
}
