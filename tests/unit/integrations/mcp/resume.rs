use super::*;
use crate::auth::AuthState;
use crate::harness::ToolHarness;
use crate::monitor::TaskMonitor;
use crate::workspace::Workspaces;
use std::time::Duration;

fn fixture(root: &std::path::Path) -> Arc<AppState> {
    let workspaces = Workspaces::new([root], true, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    Arc::new(AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".into())),
        workspaces,
        harness: ToolHarness::new(1).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    })
}

fn git(root: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn request(arguments: Value, capable: bool) -> Value {
    let capabilities = if capable {
        json!({"extensions": {(TASK_EXTENSION_ID): {}}})
    } else {
        json!({})
    };
    json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": "verify_project", "arguments": arguments,
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": crate::mcp::MODERN_PROTOCOL_VERSION,
                "io.modelcontextprotocol/clientCapabilities": capabilities
            }
        }
    })
}

async fn queued(state: &AppState, expected: u64) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while state.monitor.connection_status().queued_tasks != expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("verification queue did not reach the expected state");
}

async fn terminal(state: &AppState, id: &str, owner: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let value = get_task(state, id, owner).unwrap();
            if value["status"] != "working" {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("verification did not finish")
}

#[test]
fn verification_is_task_eligible_without_making_mutations_task_eligible() {
    assert!(task_augmented_tool(&json!({"name": "verify_project"})));
    for name in [
        "run_command",
        "create_file",
        "apply_file_edits",
        "delete_path",
    ] {
        assert!(!task_augmented_tool(&json!({"name": name})));
    }
}

#[tokio::test]
async fn malformed_task_routing_is_rejected_before_durable_creation() {
    let root = tempfile::tempdir().unwrap();
    let state = fixture(root.path());
    let (_, workspace) = state.workspaces.select(None).unwrap();
    for arguments in [
        Value::Null,
        json!([]),
        json!({"workspace": 1}),
        json!({"workspace": " "}),
    ] {
        assert!(crate::mcp::selected_workspace(&state, &arguments).is_err());
        let result = create_tool_task(
            state.clone(),
            json!({"name": "semantic_provider_refresh", "arguments": arguments}),
            "a".repeat(64),
        )
        .await;
        assert!(
            result.is_err(),
            "invalid routing must not create a durable task"
        );
    }
    assert!(state.tasks.workers.lock().unwrap().is_empty());
    assert!(
        !crate::evidence_store::workspace_state_directory(&workspace)
            .unwrap()
            .join("mcp-tasks")
            .exists()
    );
}

#[tokio::test]
async fn malformed_verification_options_never_create_tasks() {
    let root = tempfile::tempdir().unwrap();
    let state = fixture(root.path());
    for arguments in [
        json!({"level": "unknown"}),
        json!({"level": null}),
        json!({"timeout_seconds": 0}),
        json!({"timeout_seconds": 1801}),
        json!({"timeout_seconds": "120"}),
        json!({"fail_fast": "false"}),
    ] {
        let response = crate::mcp::handle_message_isolated(
            state.clone(),
            request(arguments, true),
            crate::mcp::MODERN_PROTOCOL_VERSION,
            &"a".repeat(64),
        )
        .await
        .unwrap();
        assert_eq!(response["error"]["code"], -32602);
        assert!(response.get("result").is_none());
    }
    let (_, workspace) = state.workspaces.select(None).unwrap();
    assert!(
        !crate::evidence_store::workspace_state_directory(&workspace)
            .unwrap()
            .join("mcp-tasks")
            .exists()
    );
    assert!(state.tasks.workers.lock().unwrap().is_empty());
    assert!(state.workspaces.authorization_requests(10).is_empty());
}

#[tokio::test]
async fn task_deadlines_cancel_without_polling_or_starting_expired_work() {
    for already_expired in [false, true] {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-q"]);
        let state = fixture(root.path());
        let held = state.harness.acquire().await.unwrap();
        let (workspace_id, workspace) = state.workspaces.select(None).unwrap();
        let owner = "a".repeat(64);
        let mut record = TaskRecord::working(
            owner,
            workspace_id,
            "verify_project".into(),
            state.auth.instance_id().into(),
        );
        record.ttl_ms = 1_000;
        task_store::persist(&workspace, &record).unwrap();
        let deadline = if already_expired {
            Instant::now()
        } else {
            Instant::now() + Duration::from_millis(record.ttl_ms)
        };
        let mut workers = JoinSet::new();
        let handle = workers.spawn(run_task_worker(
            state.clone(),
            workspace.clone(),
            record.task_id.clone(),
            json!({"name": "verify_project"}),
            "resume-test-owner".to_owned(),
            deadline,
        ));
        state
            .tasks
            .register(record.task_id.clone(), record.workspace.clone(), handle);
        if !already_expired {
            queued(&state, 1).await;
        }
        tokio::time::timeout(Duration::from_secs(10), workers.join_next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        queued(&state, 0).await;
        let persisted = task_store::load(&workspace, &record.task_id)
            .unwrap()
            .unwrap();
        assert_eq!(persisted.status, TaskStatus::Failed);
        assert_eq!(persisted.error.as_ref().unwrap()["code"], -32603);
        assert!(persisted.status_message.contains("TTL"));
        assert!(state.tasks.workers.lock().unwrap().is_empty());
        assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
        drop(held);
    }
}

#[tokio::test]
async fn verification_task_reconnects_without_rerunning_checks_and_preserves_failures() {
    for failing in [false, true] {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-q"]);
        if failing {
            std::fs::write(root.path().join("file.txt"), "before\n").unwrap();
            git(root.path(), &["add", "--", "file.txt"]);
            std::fs::write(root.path().join("file.txt"), "after \n").unwrap();
        }
        let state = fixture(root.path());
        let owner = "a".repeat(64);
        let held = state.harness.acquire().await.unwrap();
        let response = tokio::time::timeout(
            Duration::from_secs(3),
            crate::mcp::handle_message_isolated(
                state.clone(),
                request(json!({"level": "full", "timeout_seconds": 1800}), true),
                crate::mcp::MODERN_PROTOCOL_VERSION,
                &owner,
            ),
        )
        .await
        .expect("task handle must not wait for execution slots")
        .unwrap();
        assert_eq!(response["result"]["resultType"], "task");
        let id = response["result"]["taskId"].as_str().unwrap();
        let (_, workspace, record) = task_store::find(&state.workspaces, id).unwrap().unwrap();
        assert_eq!(record.owner, owner);
        queued(&state, 1).await;
        assert!(get_task(&state, id, &"b".repeat(64)).is_err());
        assert!(cancel_task(&state, id, &"b".repeat(64)).is_err());
        drop(held);
        let completed = terminal(&state, id, &owner).await;
        assert_eq!(completed["status"], "completed");
        assert_eq!(completed["result"]["isError"], failing);
        assert_eq!(completed["result"]["structuredContent"]["passed"], !failing);
        assert_eq!(completed["result"]["structuredContent"]["checks_run"], 1);
        let evidence = crate::evidence_store::load(&workspace).unwrap();
        assert!(!evidence.is_empty());
        for _ in 0..3 {
            assert_eq!(get_task(&state, id, &owner).unwrap(), completed);
        }
        let restarted = fixture(root.path());
        assert_eq!(get_task(&restarted, id, &owner).unwrap(), completed);
        assert_eq!(
            crate::evidence_store::load(&workspace).unwrap().len(),
            evidence.len()
        );
        assert_eq!(restarted.monitor.connection_status().queued_tasks, 0);
        assert_eq!(restarted.monitor.connection_status().active_tasks, 0);
    }
}

#[tokio::test]
async fn verification_task_cancellation_releases_queued_checks_without_evidence() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-q"]);
    let state = fixture(root.path());
    let held = state.harness.acquire().await.unwrap();
    let owner = "a".repeat(64);
    let created = create_tool_task(
        state.clone(),
        json!({"name": "verify_project"}),
        owner.clone(),
    )
    .await
    .unwrap();
    let id = created["taskId"].as_str().unwrap();
    queued(&state, 1).await;
    cancel_task(&state, id, &owner).unwrap();
    queued(&state, 0).await;
    drop(held);
    assert_eq!(get_task(&state, id, &owner).unwrap()["status"], "cancelled");
    let (_, workspace) = state.workspaces.select(None).unwrap();
    assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
}

#[tokio::test]
async fn verification_keeps_synchronous_fallback_without_modern_tasks() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-q"]);
    let state = fixture(root.path());
    for (protocol, capable) in [
        (crate::mcp::MODERN_PROTOCOL_VERSION, false),
        ("2025-11-25", true),
    ] {
        let response = crate::mcp::handle_message_isolated(
            state.clone(),
            request(json!({}), capable),
            protocol,
            &"a".repeat(64),
        )
        .await
        .unwrap();
        assert!(response["result"].get("taskId").is_none());
        assert_eq!(response["result"]["isError"], false);
        assert_eq!(response["result"]["structuredContent"]["passed"], true);
    }
}
