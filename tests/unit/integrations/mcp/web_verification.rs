//! Real protected HTTP verification lifecycle; synthetic records test isolation only.
use super::*;
#[path = "web_jobs.rs"]
mod jobs;
use crate::auth::AuthState;
use crate::harness::ToolHarness;
use crate::monitor::TaskMonitor;
use crate::workspace::Workspaces;
use axum::body::{to_bytes, Body};
use axum::http::Request;
use std::fs;
use std::path::Path as FsPath;
use std::process::Command;
use tokio::time::{sleep, timeout, Duration};
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

fn repository(root: &FsPath) {
    fs::write(root.join("README.md"), "# HTTP verification fixture\n").unwrap();
    fs::write(root.join(".gitignore"), ".wcode/\ntarget/\n").unwrap();
    git(root, &["init", "--quiet"]);
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.name=Verification Test",
            "-c",
            "user.email=verification@example.invalid",
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

fn fixture(roots: &[&FsPath], exec: bool) -> Arc<AppState> {
    let workspaces = Workspaces::new(roots, true, exec).unwrap();
    let ids = workspaces.roots().into_iter().map(|(id, _)| id);
    Arc::new(AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".into())),
        workspaces,
        harness: ToolHarness::new(1).unwrap(),
        monitor: TaskMonitor::new(ids),
        tasks: TaskRuntime::default(),
    })
}

async fn http(
    state: &Arc<AppState>,
    workspace: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
    authorized: bool,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:8765")
        .header("origin", "http://127.0.0.1:8765")
        .header("x-wcode-workspace", workspace)
        .header("content-type", "application/json");
    if authorized {
        request = request.header("x-wcode-ui-token", state.auth.ui_token());
    }
    let response = crate::mcp::router(state.clone())
        .oneshot(
            request
                .body(
                    body.map(|value| Body::from(serde_json::to_vec(&value).unwrap()))
                        .unwrap_or_else(Body::empty),
                )
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let bytes = to_bytes(response.into_body(), 256 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn request_body(state: &AppState, id: &str, workspace: &Workspace) -> Value {
    json!({"level":"quick", "revision":current_revision(state,workspace).await.unwrap(),
        "snapshot_revision":snapshot_key(state,id,workspace).await.unwrap()})
}

async fn launch(state: &Arc<AppState>, workspace_id: &str) -> Value {
    let (_, workspace) = state.workspaces.select(Some(workspace_id)).unwrap();
    let request = request_body(state, workspace_id, &workspace).await;
    let (status, value) = http(
        state,
        workspace_id,
        "POST",
        "/intelligence/verification/run",
        Some(request),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{value}");
    value
}

async fn finished(state: &Arc<AppState>, id: &str, task_id: &str) -> Value {
    timeout(Duration::from_secs(15), async {
        loop {
            let (status, value) = http(
                state,
                id,
                "GET",
                &format!("/intelligence/verification/{task_id}/result"),
                None,
                true,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{value}");
            if value["terminal"] == true {
                return value;
            }
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("native HTTP verification did not reach terminal state")
}

fn safe_projection(value: &Value) {
    for key in [
        "owner",
        "actor",
        "token",
        "runtime_instance_id",
        "params",
        "arguments",
        "result",
    ] {
        assert!(value.get(key).is_none(), "private task data leaked: {key}");
    }
    assert_eq!(value["completion_is_not_success"], true);
    assert_eq!(value["acceptance_ready"], false);
}

#[tokio::test]
async fn web_verification_run_is_authenticated_execution_guarded_and_snapshot_bound() {
    let root = tempfile::tempdir().unwrap();
    repository(root.path());
    let state = fixture(&[root.path()], true);
    let id = state.workspaces.default_id();
    let (_, workspace) = state.workspaces.select(Some(id)).unwrap();
    let request = request_body(&state, id, &workspace).await;
    let (status, _) = http(
        &state,
        id,
        "POST",
        "/intelligence/verification/run",
        Some(request.clone()),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    for (key, value) in [
        ("owner", json!("spoof")),
        ("program", json!("sh")),
        ("level", json!("unknown")),
        ("timeout_seconds", json!(0)),
        ("fail_fast", json!("true")),
    ] {
        let mut malformed = request.clone();
        malformed[key] = value;
        let (status, value) = http(
            &state,
            id,
            "POST",
            "/intelligence/verification/run",
            Some(malformed),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{value}");
    }
    fs::write(root.path().join("README.md"), "# edited after snapshot\n").unwrap();
    let (status, value) = http(
        &state,
        id,
        "POST",
        "/intelligence/verification/run",
        Some(request.clone()),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(value["code"], "stale_snapshot");
    assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
    let disabled = fixture(&[root.path()], false);
    let (status, value) = http(
        &disabled,
        disabled.workspaces.default_id(),
        "POST",
        "/intelligence/verification/run",
        Some(request),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(value["code"], "execution_disabled");
}

#[tokio::test]
async fn web_verification_runs_native_checks_and_polling_never_reexecutes() {
    let root = tempfile::tempdir().unwrap();
    repository(root.path());
    let state = fixture(&[root.path()], true);
    let id = state.workspaces.default_id();
    let created = launch(&state, id).await;
    safe_projection(&created);
    assert_eq!(created["status"], "working");
    let task_id = created["task_id"].as_str().unwrap();
    let result = finished(&state, id, task_id).await;
    safe_projection(&result);
    assert_eq!(result["status"], "completed", "{result}");
    assert_eq!(result["is_error"], false, "{result}");
    assert_eq!(result["report"]["passed"], true, "{result}");
    assert!(result["report"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["id"] == "git-diff-check" && check["execution"] == "executed"));
    let (_, workspace) = state.workspaces.select(Some(id)).unwrap();
    let evidence = crate::evidence_store::load(&workspace).unwrap();
    assert!(evidence.iter().any(|record| record.kind
        == crate::evidence::EvidenceKind::Verification
        && record.authority == crate::evidence::EvidenceAuthority::NativeVerification
        && record.producer == "verify_project"
        && record.execution_receipt.is_some()));
    let record = crate::task_store::load(&workspace, task_id)
        .unwrap()
        .unwrap();
    assert!(record.verification_git_binding.is_some());
    let retained = serde_json::to_vec(&record).unwrap();
    for _ in 0..2 {
        let (_, next) = http(
            &state,
            id,
            "GET",
            &format!("/intelligence/verification/{task_id}/result"),
            None,
            true,
        )
        .await;
        assert_eq!(next["report"], result["report"]);
    }
    assert_eq!(
        serde_json::to_vec(
            &crate::task_store::load(&workspace, task_id)
                .unwrap()
                .unwrap()
        )
        .unwrap(),
        retained
    );
    assert_eq!(
        crate::evidence_store::load(&workspace).unwrap().len(),
        evidence.len()
    );
}

#[tokio::test]
async fn web_verification_completed_failure_and_same_bytes_new_commit_are_not_current_pass() {
    let root = tempfile::tempdir().unwrap();
    repository(root.path());
    fs::write(
        root.path().join("README.md"),
        "new trailing whitespace   \n",
    )
    .unwrap();
    let state = fixture(&[root.path()], true);
    let id = state.workspaces.default_id();
    let created = launch(&state, id).await;
    let task_id = created["task_id"].as_str().unwrap();
    let result = finished(&state, id, task_id).await;
    assert_eq!(result["status"], "completed", "{result}");
    assert_eq!(result["is_error"], true);
    assert_eq!(result["report"]["passed"], false);
    assert!(result["report"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["id"] == "git-diff-check"
            && check["success"] == false
            && check["execution"] == "executed"));
    let (_, workspace) = state.workspaces.select(Some(id)).unwrap();
    let original_revision = current_revision(&state, &workspace).await.unwrap();
    git(
        root.path(),
        &[
            "-c",
            "user.name=Verification Test",
            "-c",
            "user.email=verification@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "--quiet",
            "--no-verify",
            "-m",
            "same bytes new HEAD",
        ],
    );
    assert_eq!(
        current_revision(&state, &workspace).await.unwrap(),
        original_revision
    );
    let (_, stale) = http(
        &state,
        id,
        "GET",
        &format!("/intelligence/verification/{task_id}/result"),
        None,
        true,
    )
    .await;
    assert_eq!(stale["freshness"], "stale");
    assert_eq!(stale["git_freshness"], "stale");
    assert_eq!(stale["report"]["passed"], false);
    assert_eq!(stale["acceptance_ready"], false);
}

#[tokio::test]
async fn web_verification_worker_rejects_stale_bound_revision_before_native_execution() {
    let root = tempfile::tempdir().unwrap();
    repository(root.path());
    let state = fixture(&[root.path()], true);
    let id = state.workspaces.default_id();
    let (_, workspace) = state.workspaces.select(Some(id)).unwrap();
    let record = crate::mcp_tasks::start_tool_task_bound(
        state.clone(),
        json!({"name":"verify_project","arguments":{"workspace":id,"level":"quick"}}),
        crate::mcp_tasks::monitor_ui_owner(&state),
        Some(crate::mcp_tasks::VerificationTaskBinding {
            revision: Revision {
                code: format!("sha256:{}", "a".repeat(64)),
                design: None,
            },
            git: state
                .harness
                .execution_git_binding(&workspace)
                .await
                .unwrap(),
        }),
    )
    .await
    .unwrap();
    let result = finished(&state, id, &record.task_id).await;
    assert_eq!(result["status"], "failed");
    assert!(result["error"]
        .as_str()
        .unwrap()
        .contains("snapshot changed before execution"));
    assert!(result["report"].is_null());
    assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
}

#[tokio::test]
async fn web_verification_foreign_workspace_owner_and_tool_are_indistinguishable() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    repository(root.path());
    repository(other.path());
    let state = fixture(&[root.path(), other.path()], true);
    let id = state.workspaces.default_id();
    let (_, workspace) = state.workspaces.select(Some(id)).unwrap();
    let other_id = state
        .workspaces
        .roots()
        .into_iter()
        .find(|(key, _)| key != id)
        .unwrap()
        .0;
    let mut foreign = TaskRecord::working(
        "a".repeat(64),
        id.into(),
        "verify_project".into(),
        state.auth.instance_id().into(),
    );
    let mut wrong_tool = TaskRecord::working(
        crate::mcp_tasks::monitor_ui_owner(&state),
        id.into(),
        "run_command".into(),
        state.auth.instance_id().into(),
    );
    foreign.complete(json!({"isError":false,"structuredContent":{"secret":"private MCP history"}}));
    wrong_tool.complete(json!({"isError":false,"structuredContent":{"secret":"other tool"}}));
    crate::task_store::persist(&workspace, &foreign).unwrap();
    crate::task_store::persist(&workspace, &wrong_tool).unwrap();
    for suffix in ["", "/result", "/cancel"] {
        let method = if suffix == "/cancel" { "POST" } else { "GET" };
        let (_, unknown) = http(
            &state,
            id,
            method,
            &format!("/intelligence/verification/TASK-unknown{suffix}"),
            None,
            true,
        )
        .await;
        for task_id in [&foreign.task_id, &wrong_tool.task_id] {
            let (status, value) = http(
                &state,
                id,
                method,
                &format!("/intelligence/verification/{task_id}{suffix}"),
                None,
                true,
            )
            .await;
            assert_eq!(status, StatusCode::NOT_FOUND);
            assert_eq!(value, unknown);
        }
        let (status, value) = http(
            &state,
            &other_id,
            method,
            &format!("/intelligence/verification/{}{suffix}", foreign.task_id),
            None,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(value, unknown);
    }
    assert_eq!(
        crate::task_store::load(&workspace, &foreign.task_id)
            .unwrap()
            .unwrap()
            .result,
        foreign.result
    );
}

#[tokio::test]
async fn web_verification_cancel_stops_owned_task_without_rerun_or_approval() {
    let root = tempfile::tempdir().unwrap();
    repository(root.path());
    let state = fixture(&[root.path()], true);
    let id = state.workspaces.default_id();
    let held = state.harness.acquire().await.unwrap();
    let created = launch(&state, id).await;
    let task_id = created["task_id"].as_str().unwrap();
    let (status, cancelled) = http(
        &state,
        id,
        "POST",
        &format!("/intelligence/verification/{task_id}/cancel"),
        None,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cancelled["status"], "cancelled");
    assert_eq!(cancelled["result_available"], false);
    drop(held);
    sleep(Duration::from_millis(50)).await;
    let result = finished(&state, id, task_id).await;
    assert_eq!(result["status"], "cancelled");
    assert!(result["report"].is_null());
    assert_eq!(result["acceptance_ready"], false);
    let (_, workspace) = state.workspaces.select(Some(id)).unwrap();
    assert!(!crate::evidence_store::load(&workspace)
        .unwrap()
        .iter()
        .any(|evidence| evidence.kind == crate::evidence::EvidenceKind::HumanApproval));
}

#[tokio::test]
async fn web_verification_restart_is_failed_and_legacy_binding_remains_unknown() {
    let root = tempfile::tempdir().unwrap();
    repository(root.path());
    let state = fixture(&[root.path()], true);
    let id = state.workspaces.default_id();
    let (_, workspace) = state.workspaces.select(Some(id)).unwrap();
    let record = TaskRecord::working(
        crate::mcp_tasks::monitor_ui_owner(&state),
        id.into(),
        "verify_project".into(),
        "interrupted-runtime".into(),
    );
    crate::task_store::persist(&workspace, &record).unwrap();
    let result = finished(&state, id, &record.task_id).await;
    assert_eq!(result["status"], "failed");
    assert_eq!(result["freshness"], "unknown");
    assert!(result["requested_revision"].is_null());
    assert!(result["error"]
        .as_str()
        .unwrap()
        .contains("runtime restart"));
    assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
}
