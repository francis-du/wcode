use super::*;
use crate::authorization::AuthorizationKind;
use crate::workspace::WorkspaceSecurity;
use axum::body::to_bytes;
use axum::extract::State;
use std::fs;

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
async fn webui_command_approval_keeps_executable_and_exact_operation_separate() {
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

    let second = parent
        .run_command("cargo", &test_args, ".", 30)
        .await
        .unwrap_err();
    assert!(second.to_string().contains("authorization required"));
    let exact_request = state.workspaces.latest_pending_authorization().unwrap();
    assert_eq!(exact_request.kind, AuthorizationKind::RiskyExecution);
    let approved = intelligence_web_approve_authorization(
        State(state.clone()),
        ui_headers(&state, &parent_id),
        Json(json!({"id": exact_request.id})),
    )
    .await;
    assert_eq!(approved.status(), StatusCode::OK);

    let executed = parent
        .run_command("cargo", &test_args, ".", 30)
        .await
        .expect("exact approved retry should execute");
    assert!(executed.success, "cargo test failed: {}", executed.stderr);

    let different_args = vec!["test".to_owned(), "--lib".to_owned()];
    let different = parent
        .run_command("cargo", &different_args, ".", 30)
        .await
        .unwrap_err();
    assert!(different.to_string().contains("authorization required"));
    let denied_request = state.workspaces.latest_pending_authorization().unwrap();
    let denied = intelligence_web_deny_authorization(
        State(state.clone()),
        ui_headers(&state, &parent_id),
        Json(json!({"id": denied_request.id})),
    )
    .await;
    assert_eq!(denied.status(), StatusCode::OK);
    assert!(parent
        .run_command("cargo", &different_args, ".", 30)
        .await
        .unwrap_err()
        .to_string()
        .contains("authorization required"));

    let child_error = child
        .run_command("cargo", &test_args, ".", 30)
        .await
        .unwrap_err();
    assert!(child_error.to_string().contains("authorization required"));
    let child_request = state.workspaces.latest_pending_authorization().unwrap();
    assert_eq!(child_request.workspace, child_id);
    let cross_workspace = intelligence_web_approve_authorization(
        State(state.clone()),
        ui_headers(&state, &parent_id),
        Json(json!({"id": child_request.id})),
    )
    .await;
    assert_eq!(cross_workspace.status(), StatusCode::BAD_REQUEST);

    let visible = response_json(
        intelligence_web_authorizations(State(state.clone()), ui_headers(&state, &child_id)).await,
    )
    .await;
    assert!(visible["pending"]
        .as_array()
        .unwrap()
        .iter()
        .all(|request| request["workspace"] == child_id));
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
