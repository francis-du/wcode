//! End-to-end ordinary verification receipts use native checks and the shared Task store.
use super::*;

fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
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

fn repository(failing: bool) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "--quiet"]);
    std::fs::write(root.path().join("README.md"), "before\n").unwrap();
    git(root.path(), &["add", "."]);
    git(
        root.path(),
        &[
            "-c",
            "user.name=Receipt Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=",
            "commit",
            "--quiet",
            "--no-verify",
            "-m",
            "fixture",
        ],
    );
    if failing {
        std::fs::write(root.path().join("README.md"), "after \n").unwrap();
    }
    root
}

async fn verification_terminal(
    state: &Arc<AppState>,
    protocol: &str,
    id: &str,
    workspace: &str,
) -> Value {
    timeout(Duration::from_secs(10), async {
        loop {
            let reply = call(
                state,
                protocol,
                "verify_project",
                json!({"action":"status","task_id":id,"workspace":workspace}),
                OWNER,
            )
            .await;
            assert!(reply.get("error").is_none(), "{reply}");
            let data = reply["result"]["structuredContent"].clone();
            if data["terminal"] == true {
                return data;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("verification receipt never reached terminal state")
}

#[tokio::test]
async fn ordinary_verification_start_returns_before_execution_and_poll_does_not_repeat_native_checks(
) {
    for (protocol, capable) in [(MODERN, false), (LEGACY, false), (MODERN, true)] {
        for failing in [false, true] {
            let root = repository(failing);
            let state = fixture(&[root.path()]);
            let held = state.harness.acquire().await.unwrap();
            let response = timeout(
                Duration::from_secs(1),
                crate::mcp::handle_message_isolated(
                    state.clone(),
                    request(
                        "verify_project",
                        json!({"action":"start","level":"full"}),
                        capable,
                    ),
                    protocol,
                    OWNER,
                ),
            )
            .await;
            drop(held);
            let response = response
                .expect(
                    "ordinary verification start must return a handle while execution is queued",
                )
                .unwrap();
            assert!(response.get("error").is_none(), "{response}");
            let data = &response["result"]["structuredContent"];
            assert_eq!(data["kind"], "verification_task");
            assert_eq!(data["completion_is_not_success"], true);
            assert_eq!(data["current_acceptance"], false);
            assert_eq!(data["result_scope"], "recorded_execution_only");
            assert_eq!(data["next_poll"]["tool"], "verify_project");
            for name in ["owner", "actor", "params", "runtime_instance_id"] {
                assert!(data.get(name).is_none());
            }
            let id = data["task_id"].as_str().unwrap();
            let workspace_id = data["workspace"].as_str().unwrap();
            let done = verification_terminal(&state, protocol, id, workspace_id).await;
            assert_eq!(done["status"], "completed");
            assert!(done.get("result").is_none());
            let (_, workspace) = state.workspaces.select(Some(workspace_id)).unwrap();
            let proof_count = crate::evidence_store::load(&workspace).unwrap().len();
            assert!(
                proof_count > 0,
                "native verification must retain its actual evidence"
            );
            let mut previous = None;
            for _ in 0..3 {
                let result = call(
                    &state,
                    protocol,
                    "verify_project",
                    json!({"action":"result","task_id":id,"workspace":workspace_id}),
                    OWNER,
                )
                .await;
                let result = result["result"]["structuredContent"].clone();
                assert_eq!(result["result"]["isError"], failing);
                assert_eq!(result["result"]["structuredContent"]["passed"], !failing);
                assert_eq!(result["result"]["structuredContent"]["checks_run"], 1);
                if let Some(previous) = previous {
                    assert_eq!(result, previous);
                }
                previous = Some(result);
            }
            assert_eq!(
                crate::evidence_store::load(&workspace).unwrap().len(),
                proof_count
            );
        }
    }
}

#[tokio::test]
async fn ordinary_verification_invalid_routing_never_runs_checks_or_creates_tasks() {
    let root = repository(false);
    let state = fixture(&[root.path()]);
    for args in [
        json!({"action":null}),
        json!({"action":"unknown"}),
        json!({"action":false}),
        json!({"action":"status"}),
        json!({"task_id":"TASK-not-an-execution"}),
        json!({"action":"start","task_id":"TASK-injected"}),
        json!({"action":"status","task_id":"TASK-x","level":"full"}),
        json!({"action":"start","owner":OTHER}),
        json!({"action":"start","program":"git"}),
        json!({"action":"start","level":"not-a-level"}),
        json!({"action":"start","fail_fast":"false"}),
        json!({"action":"start","timeout_seconds":1801}),
    ] {
        let response = call(&state, MODERN, "verify_project", args.clone(), OWNER).await;
        assert_eq!(
            response["error"]["code"], -32602,
            "invalid routing {args} was accepted: {response}"
        );
    }
    let (_, workspace) = state.workspaces.select(None).unwrap();
    assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
    assert!(
        !crate::evidence_store::workspace_state_directory(&workspace)
            .unwrap()
            .join("mcp-tasks")
            .exists()
    );
}

#[tokio::test]
async fn ordinary_verification_poll_cannot_read_or_cancel_a_command_task() {
    let root = repository(false);
    let state = fixture(&[root.path()]);
    let (workspace_id, workspace) = state.workspaces.select(None).unwrap();
    let mut record = TaskRecord::working(
        OWNER.into(),
        workspace_id.clone(),
        "run_command".into(),
        state.auth.instance_id().into(),
    );
    record.complete(json!({"isError":false,"structuredContent":{"stdout":"command-only-fixture"}}));
    task_store::persist(&workspace, &record).unwrap();
    for action in ["status", "result", "cancel"] {
        let denied = call(
            &state,
            MODERN,
            "verify_project",
            json!({"action":action,"workspace":workspace_id,"task_id":record.task_id}),
            OWNER,
        )
        .await;
        assert_eq!(
            denied["error"]["message"],
            "unknown task_id in selected workspace"
        );
        assert!(!denied.to_string().contains("command-only-fixture"));
    }
    assert_eq!(
        task_store::load(&workspace, &record.task_id)
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Completed
    );
    assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
}

async fn wait_queued(state: &Arc<AppState>, expected: u64) {
    timeout(Duration::from_secs(5), async {
        while state.monitor.connection_status().queued_tasks != expected {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("verification queue did not settle");
}

#[tokio::test]
async fn ordinary_verification_identity_misses_are_indistinguishable_and_do_not_mutate() {
    let first = repository(false);
    let second = repository(false);
    let state = fixture(&[first.path(), second.path()]);
    let ids = state.workspaces.roots();
    let (workspace_id, workspace) = state.workspaces.select(Some(&ids[0].0)).unwrap();
    let mut record = TaskRecord::working(
        OWNER.into(),
        workspace_id.clone(),
        "verify_project".into(),
        state.auth.instance_id().into(),
    );
    record.complete(json!({"isError":true,"structuredContent":{"passed":false,"private_fixture":"not-imported-proof"}}));
    task_store::persist(&workspace, &record).unwrap();
    let before = serde_json::to_value(&record).unwrap();
    for protocol in [MODERN, LEGACY] {
        for action in ["status", "result", "cancel"] {
            let unknown = call(
                &state,
                protocol,
                "verify_project",
                json!({"action":action,"workspace":workspace_id,"task_id":"TASK-unknown"}),
                OWNER,
            )
            .await;
            let wrong_owner = call(
                &state,
                protocol,
                "verify_project",
                json!({"action":action,"workspace":workspace_id,"task_id":record.task_id}),
                OTHER,
            )
            .await;
            let wrong_workspace = call(
                &state,
                protocol,
                "verify_project",
                json!({"action":action,"workspace":ids[1].0,"task_id":record.task_id}),
                OWNER,
            )
            .await;
            assert_eq!(wrong_owner["error"], unknown["error"]);
            assert_eq!(wrong_workspace["error"], unknown["error"]);
            assert!(!wrong_owner.to_string().contains("not-imported-proof"));
            assert!(!wrong_workspace.to_string().contains("not-imported-proof"));
        }
    }
    assert_eq!(
        serde_json::to_value(
            task_store::load(&workspace, &record.task_id)
                .unwrap()
                .unwrap()
        )
        .unwrap(),
        before
    );
    assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
}

#[tokio::test]
async fn ordinary_verification_cancel_remains_responsive_under_saturation_and_releases_waiters() {
    let root = repository(false);
    let state = fixture(&[root.path()]);
    let held = state.harness.acquire().await.unwrap();
    let created = call(
        &state,
        LEGACY,
        "verify_project",
        json!({"action":"start","level":"full"}),
        OWNER,
    )
    .await;
    let data = &created["result"]["structuredContent"];
    let id = data["task_id"].as_str().unwrap();
    let workspace_id = data["workspace"].as_str().unwrap();
    wait_queued(&state, 1).await;
    for action in ["status", "result"] {
        let result = timeout(
            Duration::from_secs(1),
            call(
                &state,
                LEGACY,
                "verify_project",
                json!({"action":action,"task_id":id,"workspace":workspace_id}),
                OWNER,
            ),
        )
        .await
        .unwrap();
        assert_eq!(result["result"]["structuredContent"]["status"], "working");
        assert_eq!(
            result["result"]["structuredContent"]["result_available"],
            false
        );
        assert!(result["result"]["structuredContent"]
            .get("result")
            .is_none());
    }
    let foreign = call(
        &state,
        LEGACY,
        "verify_project",
        json!({"action":"cancel","task_id":id,"workspace":workspace_id}),
        OTHER,
    )
    .await;
    assert!(foreign.get("error").is_some());
    assert!(state.tasks.running(id));
    let cancelled = timeout(
        Duration::from_secs(1),
        call(
            &state,
            LEGACY,
            "verify_project",
            json!({"action":"cancel","task_id":id,"workspace":workspace_id}),
            OWNER,
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        cancelled["result"]["structuredContent"]["status"],
        "cancelled"
    );
    wait_queued(&state, 0).await;
    drop(held);
    let (_, workspace) = state.workspaces.select(Some(workspace_id)).unwrap();
    assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
    let observed = call(
        &state,
        LEGACY,
        "verify_project",
        json!({"action":"result","task_id":id,"workspace":workspace_id}),
        OWNER,
    )
    .await;
    assert_eq!(
        observed["result"]["structuredContent"]["status"],
        "cancelled"
    );
    assert!(!state.tasks.running(id));
}

#[tokio::test]
async fn ordinary_verification_runtime_replacement_does_not_replay_queued_checks() {
    let root = repository(false);
    let state = fixture(&[root.path()]);
    let held = state.harness.acquire().await.unwrap();
    let created = call(
        &state,
        MODERN,
        "verify_project",
        json!({"action":"start"}),
        OWNER,
    )
    .await;
    let data = &created["result"]["structuredContent"];
    let id = data["task_id"].as_str().unwrap();
    let workspace_id = data["workspace"].as_str().unwrap();
    wait_queued(&state, 1).await;
    let replacement = fixture(&[root.path()]);
    let reply = call(
        &replacement,
        MODERN,
        "verify_project",
        json!({"action":"result","task_id":id,"workspace":workspace_id}),
        OWNER,
    )
    .await;
    let observed = &reply["result"]["structuredContent"];
    assert_eq!(observed["status"], "failed");
    assert_eq!(observed["result_available"], false);
    assert!(observed["error"]["message"]
        .as_str()
        .unwrap()
        .contains("runtime restart"));
    state.tasks.abort(id);
    wait_queued(&state, 0).await;
    drop(held);
    let (_, workspace) = replacement.workspaces.select(Some(workspace_id)).unwrap();
    assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
    assert_eq!(replacement.monitor.connection_status().active_tasks, 0);
}

#[tokio::test]
async fn ordinary_verification_can_poll_standard_tasks_without_changing_default_execution() {
    let root = repository(false);
    let state = fixture(&[root.path()]);
    let created = crate::mcp::handle_message_isolated(
        state.clone(),
        request(
            "verify_project",
            json!({"level":"full","action":"run"}),
            true,
        ),
        MODERN,
        OWNER,
    )
    .await
    .unwrap();
    assert_eq!(created["result"]["resultType"], "task");
    let id = created["result"]["taskId"].as_str().unwrap();
    let workspace_id = state.workspaces.default_id();
    verification_terminal(&state, MODERN, id, workspace_id).await;
    let standard = get_task(&state, id, OWNER).unwrap();
    let ordinary = call(
        &state,
        LEGACY,
        "verify_project",
        json!({"action":"result","task_id":id,"workspace":workspace_id}),
        OWNER,
    )
    .await;
    assert_eq!(
        ordinary["result"]["structuredContent"]["result"],
        standard["result"]
    );
    let restarted = fixture(&[root.path()]);
    let restored = call(
        &restarted,
        LEGACY,
        "verify_project",
        json!({"action":"result","task_id":id,"workspace":workspace_id}),
        OWNER,
    )
    .await;
    assert_eq!(restored, ordinary);
    for protocol in [MODERN, LEGACY] {
        let response = call(
            &state,
            protocol,
            "verify_project",
            json!({"action":"run"}),
            OWNER,
        )
        .await;
        assert!(response["result"].get("taskId").is_none());
        assert_eq!(response["result"]["structuredContent"]["passed"], true);
    }
}

#[tokio::test]
async fn ordinary_verification_nested_control_is_rejected_before_sibling_mutation() {
    let root = repository(false);
    let state = fixture(&[root.path()]);
    for action in ["start", "status", "result", "cancel"] {
        for dry_run in [false, true] {
            let response = call(
                &state,
                MODERN,
                "parallel_tools",
                json!({"dry_run":dry_run,"tasks":[
                    {"tool":"verify_project","arguments":{"action":action,"task_id":"TASK-nested"}},
                    {"tool":"create_file","arguments":{"path":"not-created.txt","content":"no"}}
                ]}),
                OWNER,
            )
            .await;
            assert!(
                response.to_string().contains("top-level tools/call"),
                "{response}"
            );
            assert!(!root.path().join("not-created.txt").exists());
        }
    }
}

#[tokio::test]
async fn ordinary_verification_lifecycle_is_discoverable_without_a_second_tool() {
    let root = repository(false);
    let state = fixture(&[root.path()]);
    let response = crate::mcp::handle_message_isolated(
        state,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
        LEGACY,
        OWNER,
    )
    .await
    .unwrap();
    let catalog = response["result"]["tools"].as_array().unwrap();
    let entry = catalog
        .iter()
        .find(|tool| tool["name"] == "verify_project")
        .unwrap();
    let properties = &entry["inputSchema"]["properties"];
    assert_eq!(
        properties["action"]["enum"],
        json!(["run", "start", "status", "result", "cancel"])
    );
    assert_eq!(properties["task_id"]["maxLength"], 96);
    assert!(properties["action"]["description"]
        .as_str()
        .unwrap()
        .contains("without Tasks"));
    assert!(properties["action"]["description"]
        .as_str()
        .unwrap()
        .contains("never rerun"));
    assert!(!catalog
        .iter()
        .any(|tool| tool["name"] == "verification_task"));
}

#[tokio::test]
async fn ordinary_verification_completed_result_survives_drift_without_claiming_current_acceptance()
{
    let root = repository(false);
    let state = fixture(&[root.path()]);
    let created = call(
        &state,
        MODERN,
        "verify_project",
        json!({"action":"start"}),
        OWNER,
    )
    .await;
    let data = &created["result"]["structuredContent"];
    let id = data["task_id"].as_str().unwrap();
    let workspace_id = data["workspace"].as_str().unwrap();
    verification_terminal(&state, MODERN, id, workspace_id).await;
    let (_, workspace) = state.workspaces.select(Some(workspace_id)).unwrap();
    let proof_count = crate::evidence_store::load(&workspace).unwrap().len();
    std::fs::write(root.path().join("README.md"), "a different revision\n").unwrap();
    let workspaces = Workspaces::new([root.path()], false, false).unwrap();
    let readonly = Arc::new(AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".into())),
        monitor: TaskMonitor::new(workspaces.roots().into_iter().map(|(id, _)| id)),
        workspaces,
        harness: ToolHarness::new(1).unwrap(),
        tasks: TaskRuntime::default(),
    });
    let reply = call(
        &readonly,
        LEGACY,
        "verify_project",
        json!({"action":"result","task_id":id,"workspace":workspace_id}),
        OWNER,
    )
    .await;
    let observed = &reply["result"]["structuredContent"];
    assert_eq!(observed["status"], "completed", "{reply}");
    assert_eq!(observed["result"]["structuredContent"]["passed"], true);
    assert_eq!(observed["current_acceptance"], false);
    assert_eq!(observed["result_scope"], "recorded_execution_only");
    assert_eq!(
        crate::evidence_store::load(&workspace).unwrap().len(),
        proof_count
    );
    assert_eq!(readonly.monitor.connection_status().active_tasks, 0);
    assert!(readonly.workspaces.authorization_requests(10).is_empty());
}

#[tokio::test]
async fn ordinary_verification_http_requires_authentication_and_preserves_owner_when_saturated() {
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    let root = repository(false);
    let state = fixture(&[root.path()]);
    state.auth.insert_test_access_token(
        "verification-primary",
        "verification-client",
        "http://127.0.0.1:8765/mcp",
    );
    state.auth.insert_test_access_token(
        "verification-other",
        "other-client",
        "http://127.0.0.1:8765/mcp",
    );
    let held = state.harness.acquire().await.unwrap();
    let app = crate::mcp::router(state.clone());
    let build = |args: Value, bearer: Option<&str>| {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("host", "127.0.0.1:8765")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", MODERN)
            .header("mcp-method", "tools/call")
            .header("mcp-name", "verify_project");
        if let Some(bearer) = bearer {
            builder = builder.header("authorization", format!("Bearer {bearer}"));
        }
        builder
            .body(Body::from(
                serde_json::to_vec(&request("verify_project", args, false)).unwrap(),
            ))
            .unwrap()
    };
    let denied = app
        .clone()
        .oneshot(build(json!({"action":"start"}), None))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    let created = timeout(
        Duration::from_secs(1),
        app.clone().oneshot(build(
            json!({"action":"start","level":"full"}),
            Some("verification-primary"),
        )),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(created.status(), StatusCode::OK);
    let created: Value =
        serde_json::from_slice(&to_bytes(created.into_body(), 128 * 1024).await.unwrap()).unwrap();
    let data = &created["result"]["structuredContent"];
    let id = data["task_id"].as_str().unwrap();
    let workspace_id = data["workspace"].as_str().unwrap();
    wait_queued(&state, 1).await;
    for action in ["status", "result", "cancel"] {
        let denied = app
            .clone()
            .oneshot(build(
                json!({"action":action,"task_id":id,"workspace":workspace_id}),
                Some("verification-other"),
            ))
            .await
            .unwrap();
        let denied: Value =
            serde_json::from_slice(&to_bytes(denied.into_body(), 128 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(
            denied["error"]["message"],
            "unknown task_id in selected workspace"
        );
        assert!(state.tasks.running(id));
    }
    let cancelled = timeout(
        Duration::from_secs(1),
        app.oneshot(build(
            json!({"action":"cancel","task_id":id,"workspace":workspace_id}),
            Some("verification-primary"),
        )),
    )
    .await
    .unwrap()
    .unwrap();
    let cancelled: Value =
        serde_json::from_slice(&to_bytes(cancelled.into_body(), 128 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(
        cancelled["result"]["structuredContent"]["status"],
        "cancelled"
    );
    wait_queued(&state, 0).await;
    drop(held);
    let (_, workspace) = state.workspaces.select(Some(workspace_id)).unwrap();
    assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
}
