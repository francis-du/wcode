//! The project payload is the Observatory's single revision-bound snapshot, so
//! the identity bar, the design-consistency matrix and the navigator scope note
//! must read fields that this response actually carries.
use super::*;
use crate::auth::AuthState;
use crate::harness::ToolHarness;
use crate::monitor::TaskMonitor;
use crate::workspace::Workspaces;
use axum::body::{to_bytes, Body};
use axum::http::Request;
use std::fs;
use std::path::Path as FsPath;
use std::process::Command;
use tower::ServiceExt;

fn git(root: &FsPath, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn designed_repository(root: &FsPath) {
    fs::write(root.join("README.md"), "# HTTP project fixture\n").unwrap();
    fs::write(root.join(".gitignore"), ".wcode/\ntarget/\n").unwrap();
    fs::create_dir_all(root.join(".wcode/design")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join(".wcode/project.yaml"),
        "schema_version: 1\nname: demo\ndescription: project payload fixture\n",
    )
    .unwrap();
    fs::write(
        root.join(".wcode/design/requirements.yaml"),
        r#"- schema_version: 1
  id: REQ-FEATURE-001
  title: Feature flow
  intent: Route feature work through the helper boundary.
  priority: high
  implemented_by:
    - component:feature
  acceptance:
    - AC-FEATURE-001
  constraints: []
  risk: {}
"#,
    )
    .unwrap();
    fs::write(
        root.join(".wcode/design/components.yaml"),
        r#"- schema_version: 1
  id: component:feature
  name: Feature entry
  responsibilities:
    - expose the feature entry point
  depends_on: []
  constraints: []
  implementation:
    - kind: symbol
      path: src/lib.rs
      symbol: feature_entry
"#,
    )
    .unwrap();
    fs::write(
        root.join(".wcode/design/acceptance.yaml"),
        r#"- schema_version: 1
  id: AC-FEATURE-001
  title: Feature calls helper
  statement: The feature entry is backed by the helper implementation.
  verification:
    - kind: test
      path: src/lib.rs
      symbol: tests::feature_flow
"#,
    )
    .unwrap();
    fs::write(
        root.join(".wcode/design/constraints.yaml"),
        r#"- schema_version: 1
  id: CONSTRAINT-QUEUE-BOUND
  title: Admission stays bounded
  statement: Recent process admission stays within the designed queue-wait bound.
  applies_to:
    - REQ-FEATURE-001
"#,
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn feature_entry() {}\n").unwrap();
    git(root, &["init", "--quiet"]);
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.name=Project Test",
            "-c",
            "user.email=project@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "--no-verify",
            "-m",
            "fixture",
        ],
    );
}

fn fixture(roots: &[&FsPath]) -> Arc<AppState> {
    let workspaces = Workspaces::new(roots, true, true).unwrap();
    let ids = workspaces.roots().into_iter().map(|(id, _)| id);
    Arc::new(AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".into())),
        workspaces,
        harness: ToolHarness::new(1).unwrap(),
        monitor: TaskMonitor::new(ids),
        tasks: TaskRuntime::default(),
    })
}

async fn project(state: &Arc<AppState>, workspace: &str) -> Value {
    let request = Request::builder()
        .method("GET")
        .uri("/intelligence/project")
        .header("host", "127.0.0.1:8765")
        .header("origin", "http://127.0.0.1:8765")
        .header("x-wcode-workspace", workspace)
        .header("x-wcode-ui-token", state.auth.ui_token())
        .body(Body::empty())
        .unwrap();
    let response = crate::mcp::router(state.clone())
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 256 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn project_payload_carries_design_review_truncation_and_runtime_identity() {
    let root = tempfile::tempdir().unwrap();
    designed_repository(root.path());
    let state = fixture(&[root.path()]);
    let id = state.workspaces.default_id();
    let value = project(&state, id).await;
    let criterion = &value["design"]["acceptance"]["AC-FEATURE-001"];
    assert_eq!(criterion["title"], "Feature calls helper", "{value}");
    assert_eq!(criterion["verification"][0]["kind"], "test", "{value}");
    assert_eq!(
        criterion["verification"][0]["symbol"], "tests::feature_flow",
        "{value}"
    );
    let constraint = &value["design"]["constraints"]["CONSTRAINT-QUEUE-BOUND"];
    assert_eq!(constraint["title"], "Admission stays bounded", "{value}");
    assert_eq!(constraint["applies_to"][0], "REQ-FEATURE-001", "{value}");
    assert!(
        value["runtime"]["version"]
            .as_str()
            .is_some_and(|version| !version.is_empty()),
        "the running build identity must travel with the snapshot: {value}"
    );
    assert!(
        value["git_review"]["truncated"].is_boolean(),
        "the review scope must say whether it was cut short: {value}"
    );
    assert_eq!(value["git_review"]["reason"], "available", "{value}");
    assert!(
        value["reconciliation"].is_array(),
        "the consistency matrix must be able to tell an unclaimed drift from an unknown plan list: {value}"
    );
    assert!(
        value["reconciliation_execution"].is_array(),
        "the matrix must be able to show live plan execution per plan: {value}"
    );
}
