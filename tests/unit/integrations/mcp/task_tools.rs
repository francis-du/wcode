//! Ordinary MCP clients use the same durable Tasks, never imported execution proof.
use super::*;
use crate::auth::AuthState;
use crate::harness::ToolHarness;
use crate::monitor::TaskMonitor;
use crate::workspace::Workspaces;
use std::path::Path;
use tokio::time::{sleep, timeout};

const OWNER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const MODERN: &str = crate::mcp::MODERN_PROTOCOL_VERSION;
const LEGACY: &str = "2025-11-25";

#[path = "verification_task.rs"]
mod verification;

fn fixture(roots: &[&Path]) -> Arc<AppState> {
    let workspaces = Workspaces::new(roots, true, true).unwrap();
    let ids = workspaces.roots().into_iter().map(|(id, _)| id);
    Arc::new(AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".into())),
        monitor: TaskMonitor::new(ids),
        workspaces,
        harness: ToolHarness::new(1).unwrap(),
        tasks: TaskRuntime::default(),
    })
}

fn request(tool: &str, args: Value, tasks: bool) -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
        "name":tool,"arguments":args,"_meta":{
            "io.modelcontextprotocol/protocolVersion":MODERN,
            "io.modelcontextprotocol/clientCapabilities":if tasks {
                json!({"extensions":{(TASK_EXTENSION_ID):{}}})
            } else { json!({}) }
        }
    }})
}

async fn call(
    state: &Arc<AppState>,
    protocol: &str,
    tool: &str,
    args: Value,
    owner: &str,
) -> Value {
    crate::mcp::handle_message_isolated(state.clone(), request(tool, args, false), protocol, owner)
        .await
        .unwrap()
}

async fn terminal(state: &Arc<AppState>, id: &str, workspace: &str) -> Value {
    timeout(Duration::from_secs(10), async {
        loop {
            let reply = call(
                state,
                MODERN,
                "command_task",
                json!({"action":"status","task_id":id,"workspace":workspace}),
                OWNER,
            )
            .await;
            let data = reply["result"]["structuredContent"].clone();
            assert!(reply.get("error").is_none(), "{reply}");
            if data["terminal"] == true {
                break data;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("ordinary command Task did not reach terminal status")
}

fn assert_safe_projection(value: &Value) {
    assert_eq!(value["kind"], "command_task");
    assert_eq!(value["completion_is_not_success"], true);
    for field in [
        "owner",
        "actor",
        "runtime_instance_id",
        "token",
        "arguments",
        "params",
    ] {
        assert!(
            value.get(field).is_none(),
            "private TaskRecord field leaked: {field}"
        );
    }
}

#[tokio::test]
async fn ordinary_command_task_receipt_works_without_task_extension_and_legacy() {
    let root = tempfile::tempdir().unwrap();
    let state = fixture(&[root.path()]);
    for protocol in [MODERN, LEGACY] {
        let created = call(
            &state,
            protocol,
            "run_command",
            json!({"program":"git","args":["--version"],"task_mode":true,"timeout_seconds":1800}),
            OWNER,
        )
        .await;
        assert!(created.get("error").is_none(), "{created}");
        assert_ne!(created["result"]["resultType"], "task");
        assert!(created["result"].get("taskId").is_none());
        let data = &created["result"]["structuredContent"];
        assert_safe_projection(data);
        let id = data["task_id"].as_str().unwrap();
        let workspace = data["workspace"].as_str().unwrap();
        assert_eq!(data["next_poll"]["arguments"]["workspace"], workspace);
        let completed = terminal(&state, id, workspace).await;
        assert_eq!(completed["status"], "completed");
        assert_eq!(completed["result_available"], true);
        assert!(
            completed.get("result").is_none(),
            "status omits bulky delivered output"
        );
        let result = call(
            &state,
            protocol,
            "command_task",
            json!({"action":"result","task_id":id,"workspace":workspace}),
            OWNER,
        )
        .await;
        assert_eq!(
            result["result"]["structuredContent"]["result"]["isError"],
            false
        );
        assert!(
            result["result"]["structuredContent"]["result"]["structuredContent"]["stdout"]
                .as_str()
                .unwrap()
                .contains("git version")
        );
    }
    let created = call(
        &state,
        MODERN,
        "command_task",
        json!({"action":"create","program":"git","args":["--version"]}),
        OWNER,
    )
    .await;
    assert!(created.get("error").is_none(), "{created}");
    let data = &created["result"]["structuredContent"];
    terminal(
        &state,
        data["task_id"].as_str().unwrap(),
        data["workspace"].as_str().unwrap(),
    )
    .await;
}

#[tokio::test]
async fn ordinary_command_task_exact_owner_workspace_and_tool_are_required() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let state = fixture(&[first.path(), second.path()]);
    let ids = state.workspaces.roots();
    let workspace_id = &ids[0].0;
    let foreign_workspace = &ids[1].0;
    let (_, workspace) = state.workspaces.select(Some(workspace_id)).unwrap();
    let mut record = TaskRecord::working(
        OWNER.into(),
        workspace_id.clone(),
        "run_command".into(),
        state.auth.instance_id().into(),
    );
    record.complete(
        json!({"content":[],"structuredContent":{"success":false,"exit_code":7},"isError":true}),
    );
    task_store::persist(&workspace, &record).unwrap();
    let original = serde_json::to_vec(
        &task_store::load(&workspace, &record.task_id)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    for action in ["status", "result", "cancel"] {
        let owner_miss = call(
            &state,
            MODERN,
            "command_task",
            json!({"action":action,"task_id":record.task_id,"workspace":workspace_id}),
            OTHER,
        )
        .await;
        let workspace_miss = call(
            &state,
            MODERN,
            "command_task",
            json!({"action":action,"task_id":record.task_id,"workspace":foreign_workspace}),
            OWNER,
        )
        .await;
        let unknown = call(
            &state,
            MODERN,
            "command_task",
            json!({"action":action,"task_id":"TASK-unknown","workspace":workspace_id}),
            OWNER,
        )
        .await;
        assert_eq!(owner_miss["error"], unknown["error"]);
        assert_eq!(workspace_miss["error"], unknown["error"]);
    }
    assert_eq!(
        serde_json::to_vec(
            &task_store::load(&workspace, &record.task_id)
                .unwrap()
                .unwrap()
        )
        .unwrap(),
        original
    );
    let mut record = TaskRecord::working(
        OWNER.into(),
        workspace_id.clone(),
        "verify_project".into(),
        state.auth.instance_id().into(),
    );
    record.complete(json!({"isError":false,"structuredContent":{"fixture":"not-command-proof"}}));
    task_store::persist(&workspace, &record).unwrap();
    let denied = call(
        &state,
        MODERN,
        "command_task",
        json!({"action":"result","task_id":record.task_id,"workspace":workspace_id}),
        OWNER,
    )
    .await;
    assert_eq!(
        denied["error"]["message"],
        "unknown task_id in selected workspace"
    );
}

#[tokio::test]
async fn ordinary_command_task_failed_command_keeps_inner_error_and_never_mints_evidence() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("before.txt"), "before\n").unwrap();
    std::fs::write(root.path().join("after.txt"), "after\n").unwrap();
    let state = fixture(&[root.path()]);
    let created = call(&state, MODERN, "command_task",
        json!({"action":"create","program":"git","args":["diff","--exit-code","--no-index","before.txt","after.txt"]}), OWNER).await;
    let data = &created["result"]["structuredContent"];
    let id = data["task_id"].as_str().unwrap();
    let workspace = data["workspace"].as_str().unwrap();
    let completed = terminal(&state, id, workspace).await;
    assert_eq!(
        completed["status"], "completed",
        "delivery is completed even when the command failed"
    );
    let reply = call(
        &state,
        MODERN,
        "command_task",
        json!({"action":"result","task_id":id,"workspace":workspace}),
        OWNER,
    )
    .await;
    let delivered = &reply["result"]["structuredContent"]["result"];
    assert_eq!(delivered["isError"], true);
    assert_eq!(delivered["structuredContent"]["success"], false);
    assert_eq!(delivered["structuredContent"]["exit_code"], 1);
    let (_, selected) = state.workspaces.select(Some(workspace)).unwrap();
    assert!(crate::evidence_store::load(&selected).unwrap().is_empty());
}

#[tokio::test]
async fn ordinary_command_task_restart_is_failed_without_rerunning() {
    let root = tempfile::tempdir().unwrap();
    let state = fixture(&[root.path()]);
    let held = state.harness.acquire().await;
    let created = call(
        &state,
        MODERN,
        "command_task",
        json!({"action":"create","program":"git","args":["--version"]}),
        OWNER,
    )
    .await;
    let data = &created["result"]["structuredContent"];
    let id = data["task_id"].as_str().unwrap();
    let workspace = data["workspace"].as_str().unwrap();
    let restarted = fixture(&[root.path()]);
    let reply = call(
        &restarted,
        MODERN,
        "command_task",
        json!({"action":"result","task_id":id,"workspace":workspace}),
        OWNER,
    )
    .await;
    let result = &reply["result"]["structuredContent"];
    assert_eq!(result["status"], "failed");
    assert_eq!(result["terminal"], true);
    assert_eq!(result["result_available"], false);
    assert!(result["error"]["message"]
        .as_str()
        .unwrap()
        .contains("runtime restart"));
    assert!(result.get("result").is_none());
    state.tasks.abort(id);
    drop(held);
}

#[tokio::test]
async fn ordinary_command_task_cancel_terminates_actual_process() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("server.js"),
        "const net=require('net');const s=net.createServer();s.listen(0,'127.0.0.1',()=>console.log('ready:'+s.address().port));console.error('api_key=ordinary-fixture-secret');setInterval(()=>{},1000);\n").unwrap();
    let state = fixture(&[root.path()]);
    let created = call(
        &state,
        LEGACY,
        "command_task",
        json!({"action":"create","program":"node","args":["server.js"],"timeout_seconds":1800}),
        OWNER,
    )
    .await;
    let data = &created["result"]["structuredContent"];
    let id = data["task_id"].as_str().unwrap();
    let workspace = data["workspace"].as_str().unwrap();
    let port = timeout(Duration::from_secs(10), async {
        loop {
            let reply = call(
                &state,
                LEGACY,
                "command_task",
                json!({"action":"status","task_id":id,"workspace":workspace}),
                OWNER,
            )
            .await;
            let snapshot = &reply["result"]["structuredContent"];
            assert_eq!(snapshot["status"], "working", "{reply}");
            let output = &snapshot["live_output"];
            assert!(!serde_json::to_string(output)
                .unwrap()
                .contains("ordinary-fixture-secret"));
            if let Some(port) = output["stdout"].as_str().and_then(|stdout| {
                stdout.lines().find_map(|line| {
                    line.strip_prefix("ready:")
                        .and_then(|port| port.parse::<u16>().ok())
                })
            }) {
                break port;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("actual ordinary command Task did not produce live output");
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_ok());
    let denied = call(
        &state,
        LEGACY,
        "command_task",
        json!({"action":"cancel","task_id":id,"workspace":workspace}),
        OTHER,
    )
    .await;
    assert!(denied.get("error").is_some());
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_ok());
    let cancelled = call(
        &state,
        LEGACY,
        "command_task",
        json!({"action":"cancel","task_id":id,"workspace":workspace}),
        OWNER,
    )
    .await;
    assert_eq!(
        cancelled["result"]["structuredContent"]["status"],
        "cancelled"
    );
    timeout(Duration::from_secs(10), async {
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("cancellation did not close the actual command process");
}

#[tokio::test]
async fn ordinary_command_task_rejects_identity_fields_and_invalid_create_args() {
    let root = tempfile::tempdir().unwrap();
    let state = fixture(&[root.path()]);
    for (key, value) in [
        ("owner", json!(OWNER)),
        ("actor", json!("human")),
        ("runtime_instance_id", json!("foreign")),
        ("task_id", json!("TASK-injected")),
        ("task_mode", json!(false)),
        ("task_mode", Value::Null),
        ("task_mode", json!("yes")),
        ("timeout_seconds", json!(1801)),
        ("args", json!("git --version")),
        ("env", json!({"SECRET":"x"})),
        ("cwd", json!("")),
        ("program", json!("")),
    ] {
        let mut args = json!({"action":"create","program":"git","args":["--version"]});
        args[key] = value;
        let denied = call(&state, MODERN, "command_task", args, OWNER).await;
        assert!(
            denied.get("error").is_some(),
            "invalid {key} was accepted: {denied}"
        );
    }
    let denied = call(
        &state,
        MODERN,
        "command_task",
        json!({"action":"list","owner":OWNER}),
        OWNER,
    )
    .await;
    assert!(denied.get("error").is_some());
    let (_, workspace) = state.workspaces.select(None).unwrap();
    let task_root = crate::evidence_store::workspace_state_directory(&workspace)
        .unwrap()
        .join("mcp-tasks");
    assert!(
        !task_root.exists(),
        "invalid input must not create a worker or durable receipt"
    );
}

#[tokio::test]
async fn ordinary_command_task_nested_fanout_is_rejected_before_execution() {
    let root = tempfile::tempdir().unwrap();
    let state = fixture(&[root.path()]);
    for nested in [
        json!({"tool":"command_task","arguments":{"action":"create","program":"git","args":["--version"]}}),
        json!({"tool":"run_command","arguments":{"program":"git","args":["--version"],"task_mode":true}}),
    ] {
        let reply = call(&state, MODERN, "parallel_tools", json!({"tasks":[
            nested, {"tool":"create_file","arguments":{"path":"should-not-exist.txt","content":"unexpected"}}
        ]}), OWNER).await;
        let text = serde_json::to_string(&reply).unwrap();
        assert!(text.contains("top-level tools/call"), "{reply}");
        assert!(
            !root.path().join("should-not-exist.txt").exists(),
            "preflight must reject all fanout before mutation"
        );
    }
}

#[tokio::test]
async fn ordinary_command_task_http_authentication_and_owner_isolation() {
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    let root = tempfile::tempdir().unwrap();
    let state = fixture(&[root.path()]);
    state.auth.insert_test_access_token(
        "primary-token",
        "primary-client",
        "http://127.0.0.1:8765/mcp",
    );
    state
        .auth
        .insert_test_access_token("other-token", "other-client", "http://127.0.0.1:8765/mcp");
    let app = crate::mcp::router(state.clone());
    let build = |args: Value, token: Option<&str>| {
        let mut request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("host", "127.0.0.1:8765")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", MODERN)
            .header("mcp-method", "tools/call")
            .header("mcp-name", "command_task");
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        request
            .body(Body::from(
                serde_json::to_vec(&request_payload(args)).unwrap(),
            ))
            .unwrap()
    };
    let denied = app
        .clone()
        .oneshot(build(
            json!({"action":"create","program":"git","args":["--version"]}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    let created = app
        .clone()
        .oneshot(build(
            json!({"action":"create","program":"git","args":["--version"]}),
            Some("primary-token"),
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::OK);
    let created: Value =
        serde_json::from_slice(&to_bytes(created.into_body(), 128 * 1024).await.unwrap()).unwrap();
    let data = &created["result"]["structuredContent"];
    let id = data["task_id"].as_str().unwrap();
    let workspace = data["workspace"].as_str().unwrap();
    let foreign = app
        .clone()
        .oneshot(build(
            json!({"action":"status","task_id":id,"workspace":workspace}),
            Some("other-token"),
        ))
        .await
        .unwrap();
    let foreign: Value =
        serde_json::from_slice(&to_bytes(foreign.into_body(), 128 * 1024).await.unwrap()).unwrap();
    assert_eq!(
        foreign["error"]["message"],
        "unknown task_id in selected workspace"
    );
    let own = app
        .oneshot(build(
            json!({"action":"status","task_id":id,"workspace":workspace}),
            Some("primary-token"),
        ))
        .await
        .unwrap();
    let own: Value =
        serde_json::from_slice(&to_bytes(own.into_body(), 128 * 1024).await.unwrap()).unwrap();
    assert!(own.get("error").is_none(), "{own}");
    assert_safe_projection(&own["result"]["structuredContent"]);
}

fn request_payload(args: Value) -> Value {
    request("command_task", args, false)
}

#[tokio::test]
async fn ordinary_command_task_standard_extension_response_is_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let state = fixture(&[root.path()]);
    let created = crate::mcp::handle_message_isolated(
        state.clone(),
        request(
            "run_command",
            json!({"program":"git","args":["--version"],"task_mode":true}),
            true,
        ),
        MODERN,
        OWNER,
    )
    .await
    .unwrap();
    assert_eq!(created["result"]["resultType"], "task");
    let id = created["result"]["taskId"].as_str().unwrap();
    assert!(created["result"].get("structuredContent").is_none());
    let workspace = state.workspaces.default_id();
    terminal(&state, id, workspace).await;
    let standard = get_task(&state, id, OWNER).unwrap();
    assert_eq!(standard["resultType"], "complete");
    assert_eq!(standard["status"], "completed");
    let ordinary = call(
        &state,
        MODERN,
        "command_task",
        json!({"action":"result","task_id":id,"workspace":workspace}),
        OWNER,
    )
    .await;
    assert_eq!(
        ordinary["result"]["structuredContent"]["result"],
        standard["result"]
    );
}
#[tokio::test]
#[ignore = "manual: actual Node command runs for 61 seconds beyond the former synchronous ceiling"]
async fn command_task_runs_past_old_sixty_second_ceiling() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("long.js"),
        "console.log('started');setTimeout(()=>{console.log('finished');process.exit(0)},61000);\n",
    )
    .unwrap();
    let state = fixture(&[root.path()]);
    let started = Instant::now();
    // No timeout override: the compiled server's native default must suffice.
    let created = call(
        &state,
        LEGACY,
        "command_task",
        json!({"action":"create","program":"node","args":["long.js"]}),
        OWNER,
    )
    .await;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "creation must return a pollable receipt"
    );
    assert!(created.get("error").is_none(), "{created}");
    let data = &created["result"]["structuredContent"];
    let id = data["task_id"].as_str().unwrap();
    let workspace = data["workspace"].as_str().unwrap();
    timeout(Duration::from_secs(90), async {
        loop {
            let reply = call(
                &state,
                LEGACY,
                "command_task",
                json!({"action":"status","task_id":id,"workspace":workspace}),
                OWNER,
            )
            .await;
            assert!(reply.get("error").is_none(), "{reply}");
            let snapshot = &reply["result"]["structuredContent"];
            if snapshot["terminal"] == true {
                assert_eq!(snapshot["status"], "completed");
                break;
            }
            sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .expect("61-second durable command did not complete");
    assert!(
        started.elapsed() >= Duration::from_secs(61),
        "real execution must cross 60 seconds"
    );
    let result = call(
        &state,
        LEGACY,
        "command_task",
        json!({"action":"result","task_id":id,"workspace":workspace}),
        OWNER,
    )
    .await;
    let delivered = &result["result"]["structuredContent"]["result"];
    assert_eq!(delivered["isError"], false);
    assert_eq!(delivered["structuredContent"]["success"], true);
    assert_eq!(delivered["structuredContent"]["exit_code"], 0);
    assert!(delivered["structuredContent"]["stdout"]
        .as_str()
        .unwrap()
        .contains("finished"));
}
