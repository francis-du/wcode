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

fn tool_request(name: &str, arguments: Value, capable: bool) -> Value {
    let capabilities = if capable {
        json!({"extensions": {(TASK_EXTENSION_ID): {}}})
    } else {
        json!({})
    };
    json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": name, "arguments": arguments,
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": crate::mcp::MODERN_PROTOCOL_VERSION,
                "io.modelcontextprotocol/clientCapabilities": capabilities
            }
        }
    })
}

fn request(arguments: Value, capable: bool) -> Value {
    tool_request("verify_project", arguments, capable)
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

async fn managed_server_port(state: &AppState, id: &str, owner: &str) -> u16 {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let task = get_task(state, id, owner).unwrap();
            assert_eq!(
                task["status"], "working",
                "managed server terminated before exposing live output: {task}"
            );
            let live = &task["_meta"]["dev.wcode/liveCommandOutput"];
            let stdout = live["stdout"].as_str().unwrap_or_default();
            let stderr = live["stderr"].as_str().unwrap_or_default();
            if stderr.contains("server-stderr") {
                if let Some(port) = stdout.lines().find_map(|line| {
                    line.strip_prefix("server-ready:")
                        .and_then(|port| port.parse::<u16>().ok())
                }) {
                    assert!(!stderr.contains("fixture-live-secret"));
                    assert_eq!(live["redacted"], true);
                    assert_eq!(live["stdoutTruncated"], false);
                    assert_eq!(live["stderrTruncated"], false);
                    break port;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("managed run task never exposed bounded live command output")
}

#[test]
fn verification_and_explicit_long_runs_are_task_eligible_without_backgrounding_mutations() {
    assert!(task_augmented_tool(&json!({"name": "verify_project"})));
    assert!(!task_augmented_tool(&json!({"name": "run_command"})));
    assert!(!task_augmented_tool(&json!({
        "name": "run_command",
        "arguments": {"program":"cargo","args":["--version"],"task_mode":false}
    })));
    assert!(task_augmented_tool(&json!({
        "name": "run_command",
        "arguments": {"program":"cargo","args":["--version"],"task_mode":true}
    })));
    assert!(requires_task_capability(&json!({
        "name": "run_command",
        "arguments": {"program":"cargo","task_mode":true}
    })));
    for name in ["create_file", "apply_file_edits", "delete_path"] {
        assert!(!task_augmented_tool(&json!({"name": name})));
    }
}

#[tokio::test]
async fn run_command_task_mode_requires_task_capability_and_completes_durably() {
    let root = tempfile::tempdir().unwrap();
    let state = fixture(root.path());
    let owner = "a".repeat(64);
    let arguments = json!({
        "program":"cargo",
        "args":["--version"],
        "timeout_seconds":30,
        "task_mode":true
    });

    let rejected = crate::mcp::handle_message_isolated(
        state.clone(),
        tool_request("run_command", arguments.clone(), false),
        crate::mcp::MODERN_PROTOCOL_VERSION,
        &owner,
    )
    .await
    .unwrap();
    assert_eq!(rejected["error"]["code"], -32021);
    assert!(rejected.get("result").is_none());

    let created = crate::mcp::handle_message_isolated(
        state.clone(),
        tool_request("run_command", arguments, true),
        crate::mcp::MODERN_PROTOCOL_VERSION,
        &owner,
    )
    .await
    .unwrap();
    assert_eq!(created["result"]["resultType"], "task");
    let id = created["result"]["taskId"].as_str().unwrap();
    let completed = terminal(&state, id, &owner).await;
    assert_eq!(completed["status"], "completed");
    assert_eq!(completed["result"]["isError"], false);
    assert_eq!(completed["result"]["structuredContent"]["success"], true);
    assert!(completed["result"]["structuredContent"]["stdout"]
        .as_str()
        .is_some_and(|stdout| stdout.contains("cargo")));
    for _ in 0..2 {
        assert_eq!(get_task(&state, id, &owner).unwrap(), completed);
    }
}

#[tokio::test]
async fn task_mode_applies_bounded_non_secret_launch_environment() {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("launch-env.js"),
        "console.log([process.env.HOST,process.env.PORT,process.env.NODE_ENV,process.env.LOG_LEVEL].join('|')); setInterval(()=>{},1000);\n",
    )
    .unwrap();
    let state = fixture(root.path());
    let owner = "a".repeat(64);
    let created = crate::mcp::handle_message_isolated(
        state.clone(),
        tool_request(
            "run_command",
            json!({
                "program":"node",
                "args":["launch-env.js"],
                "timeout_seconds":30,
                "task_mode":true,
                "env":{
                    "HOST":"127.0.0.1",
                    "PORT":"4317",
                    "NODE_ENV":"test",
                    "LOG_LEVEL":"debug"
                }
            }),
            true,
        ),
        crate::mcp::MODERN_PROTOCOL_VERSION,
        &owner,
    )
    .await
    .unwrap();
    assert_eq!(created["result"]["resultType"], "task");
    let id = created["result"]["taskId"].as_str().unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let task = get_task(&state, id, &owner).unwrap();
            assert_eq!(task["status"], "working");
            if task["_meta"]["dev.wcode/liveCommandOutput"]["stdout"]
                .as_str()
                .is_some_and(|stdout| stdout.contains("127.0.0.1|4317|test|debug"))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("managed command did not receive bounded launch environment");
    cancel_task(&state, id, &owner).unwrap();
}

#[tokio::test]
async fn run_command_without_task_mode_stays_synchronous_and_legacy_task_mode_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let state = fixture(root.path());
    let owner = "a".repeat(64);

    let synchronous = crate::mcp::handle_message_isolated(
        state.clone(),
        tool_request(
            "run_command",
            json!({"program":"cargo","args":["--version"],"task_mode":false}),
            true,
        ),
        crate::mcp::MODERN_PROTOCOL_VERSION,
        &owner,
    )
    .await
    .unwrap();
    assert!(synchronous["result"].get("taskId").is_none());
    assert_eq!(synchronous["result"]["isError"], false);
    assert_eq!(synchronous["result"]["structuredContent"]["success"], true);

    let legacy = crate::mcp::handle_message_isolated(
        state,
        tool_request(
            "run_command",
            json!({"program":"cargo","args":["--version"],"task_mode":true}),
            true,
        ),
        "2025-11-25",
        &owner,
    )
    .await
    .unwrap();
    assert_eq!(legacy["error"]["code"], -32021);
    assert!(legacy.get("result").is_none());
}

#[tokio::test]
async fn cancelling_long_run_command_task_terminates_the_supervised_server() {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("server.js"),
        "const net=require('net'); console.error('server-stderr'); console.error('api_key=fixture-live-secret'); const server=net.createServer(s=>s.end('ok')); server.listen(0,'127.0.0.1',()=>console.log('server-ready:'+server.address().port)); setInterval(()=>{},1000);\n",
    )
    .unwrap();

    let state = fixture(root.path());
    let owner = "a".repeat(64);
    let created = create_tool_task(
        state.clone(),
        json!({
            "name":"run_command",
            "arguments":{
                "program":"node",
                "args":["server.js"],
                "timeout_seconds":30,
                "task_mode":true
            }
        }),
        owner.clone(),
    )
    .await
    .unwrap();
    let id = created["taskId"].as_str().unwrap();

    let port = managed_server_port(&state, id, &owner).await;
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_ok());
    let live_observed = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let task = get_task(&state, id, &owner).unwrap();
            let live = &task["_meta"]["dev.wcode/liveCommandOutput"];
            if live["stdout"]
                .as_str()
                .is_some_and(|stdout| stdout.contains("server-ready:"))
                && live["stderr"]
                    .as_str()
                    .is_some_and(|stderr| stderr.contains("server-stderr"))
            {
                let stderr = live["stderr"].as_str().unwrap();
                assert!(stderr.contains("api_key= [REDACTED]"));
                assert!(!stderr.contains("fixture-live-secret"));
                assert_eq!(live["redacted"], true);
                assert_eq!(live["stdoutTruncated"], false);
                assert_eq!(live["stderrTruncated"], false);
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    if live_observed.is_err() {
        panic!(
            "managed run task never exposed bounded live command output; final task: {}",
            get_task(&state, id, &owner).unwrap()
        );
    }
    assert_eq!(get_task(&state, id, &owner).unwrap()["status"], "working");

    cancel_task(&state, id, &owner).unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("cancelling the task must terminate the supervised server");
    assert_eq!(get_task(&state, id, &owner).unwrap()["status"], "cancelled");

    let restarted = create_tool_task(
        state.clone(),
        json!({
            "name":"run_command",
            "arguments":{
                "program":"node",
                "args":["server.js"],
                "timeout_seconds":30,
                "task_mode":true
            }
        }),
        owner.clone(),
    )
    .await
    .unwrap();
    let restarted_id = restarted["taskId"].as_str().unwrap();
    assert_ne!(restarted_id, id);
    let restarted_port = managed_server_port(&state, restarted_id, &owner).await;
    assert!(std::net::TcpStream::connect(("127.0.0.1", restarted_port)).is_ok());
    assert_eq!(
        get_task(&state, restarted_id, &owner).unwrap()["status"],
        "working"
    );
    cancel_task(&state, restarted_id, &owner).unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if std::net::TcpStream::connect(("127.0.0.1", restarted_port)).is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("restarted managed run must still be owned by task cancellation");
}

#[tokio::test]
async fn long_run_live_output_exposes_only_a_bounded_redacted_stream_tail() {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("logs.js"),
        "process.stdout.write('x'.repeat(40000)+'\\ntail-marker\\n'); console.error('api_key=fixture-live-tail-secret'); setInterval(()=>{},1000);\n",
    )
    .unwrap();
    let state = fixture(root.path());
    let owner = "a".repeat(64);
    let created = create_tool_task(
        state.clone(),
        json!({
            "name":"run_command",
            "arguments":{
                "program":"node",
                "args":["logs.js"],
                "timeout_seconds":30,
                "task_mode":true
            }
        }),
        owner.clone(),
    )
    .await
    .unwrap();
    let id = created["taskId"].as_str().unwrap();
    let live = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let task = get_task(&state, id, &owner).unwrap();
            let live = &task["_meta"]["dev.wcode/liveCommandOutput"];
            if live["stdout"]
                .as_str()
                .is_some_and(|stdout| stdout.contains("tail-marker"))
                && live["stderr"]
                    .as_str()
                    .is_some_and(|stderr| stderr.contains("[REDACTED]"))
                && live["redacted"] == true
            {
                break live.clone();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("managed run never exposed the bounded tail window");
    assert!(live["stdout"].as_str().unwrap().len() <= MAX_LIVE_COMMAND_STREAM_BYTES);
    assert!(live["stdoutDroppedPrefixBytes"].as_u64().unwrap() > 0);
    assert_eq!(live["stdoutCaptureTruncated"], false);
    assert_eq!(live["stdoutTruncated"], true);
    assert_eq!(live["window"], "stream_tail");
    assert_eq!(
        live["maxBytesPerStream"],
        u64::try_from(MAX_LIVE_COMMAND_STREAM_BYTES).unwrap()
    );
    assert!(live["stderr"].as_str().unwrap().contains("[REDACTED]"));
    assert!(!serde_json::to_string(&live)
        .unwrap()
        .contains("fixture-live-tail-secret"));
    assert_eq!(live["redacted"], true);
    cancel_task(&state, id, &owner).unwrap();
}

#[tokio::test]
async fn long_run_live_output_keeps_advancing_after_final_capture_saturates() {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("continuous-logs.js"),
        "for(let i=0;i<40;i++) process.stdout.write('x'.repeat(8192)+'\\n'); setTimeout(()=>{console.log('post-capture-marker'); console.error('api_key=post-capture-secret');},100); setInterval(()=>{},1000);\n",
    )
    .unwrap();
    let state = fixture(root.path());
    let owner = "a".repeat(64);
    let created = create_tool_task(
        state.clone(),
        json!({
            "name":"run_command",
            "arguments":{
                "program":"node",
                "args":["continuous-logs.js"],
                "timeout_seconds":30,
                "task_mode":true
            }
        }),
        owner.clone(),
    )
    .await
    .unwrap();
    let id = created["taskId"].as_str().unwrap();
    let live = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let task = get_task(&state, id, &owner).unwrap();
            assert_eq!(task["status"], "working");
            let live = &task["_meta"]["dev.wcode/liveCommandOutput"];
            if live["stdoutCaptureTruncated"] == true
                && live["stdout"]
                    .as_str()
                    .is_some_and(|stdout| stdout.contains("post-capture-marker"))
                && live["stderr"]
                    .as_str()
                    .is_some_and(|stderr| stderr.contains("[REDACTED]"))
            {
                break live.clone();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("live stream tail stopped advancing after final command capture saturated");
    assert_eq!(live["window"], "stream_tail");
    assert_eq!(live["stdoutCaptureTruncated"], true);
    assert_eq!(live["stdoutTruncated"], true);
    assert!(live["stdoutDroppedPrefixBytes"].as_u64().unwrap() > 0);
    assert!(live["stdout"]
        .as_str()
        .unwrap()
        .contains("post-capture-marker"));
    assert!(!serde_json::to_string(&live)
        .unwrap()
        .contains("post-capture-secret"));
    assert_eq!(live["redacted"], true);
    cancel_task(&state, id, &owner).unwrap();
}

#[tokio::test]
async fn task_mode_never_bypasses_command_policy() {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let state = fixture(root.path());
    let owner = "a".repeat(64);
    let created = create_tool_task(
        state.clone(),
        json!({
            "name":"run_command",
            "arguments":{
                "program":"node",
                "args":["--eval","process.exit(0)"],
                "timeout_seconds":30,
                "task_mode":true
            }
        }),
        owner.clone(),
    )
    .await
    .unwrap();
    let id = created["taskId"].as_str().unwrap();
    let completed = terminal(&state, id, &owner).await;
    assert_eq!(completed["status"], "completed");
    assert_eq!(completed["result"]["isError"], true);
    let serialized = serde_json::to_string(&completed).unwrap();
    assert!(serialized.contains("inline or interactive Node execution is blocked"));
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
