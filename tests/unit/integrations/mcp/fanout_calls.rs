//! Parallel fan-out tool calls: child slots, dependency ordering, cancellation
//! and coalesced writes, all against the real MCP dispatch path.
use super::*;
use std::fs;

#[tokio::test]
async fn parallel_fanout_uses_child_slots_without_parent_deadlock() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("one.txt"), "one\n").unwrap();
    fs::write(dir.path().join("two.txt"), "two\n").unwrap();
    let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(1).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    };

    let response = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        call_tool(
            &state,
            json!({
                "name": "parallel_tools",
                "arguments": {
                    "tasks": [
                        {"id": "one", "tool": "read_file", "arguments": {"path": "one.txt"}},
                        {"id": "missing", "tool": "read_file", "arguments": {"path": "missing.txt"}},
                        {"id": "two", "tool": "read_file", "arguments": {"path": "two.txt"}}
                    ]
                }
            }),
        ),
    )
    .await
    .expect("fan-out must not deadlock with one semaphore slot")
    .unwrap();

    let result = &response["structuredContent"];
    assert_eq!(result["execution"], "parallel-fanout");
    assert_eq!(result["tasks"], 3);
    assert_eq!(result["succeeded"], 2);
    assert_eq!(result["failed"], 1);
}

#[tokio::test]
async fn parallel_fanout_skips_failed_dependencies_but_finishes_independent_work() {
    for cap in [1, 4] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("exists.txt"), "original").unwrap();
        let workspaces = Workspaces::new([dir.path()], true, false).unwrap();
        let workspace_id = workspaces.default_id().to_owned();
        let state = AppState {
            auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
            workspaces,
            harness: ToolHarness::new(cap).unwrap(),
            monitor: TaskMonitor::new([workspace_id]),
            tasks: TaskRuntime::default(),
        };
        // Ordering is covered deterministically; this outer timeout is only the integration deadlock watchdog.
        let response = tokio::time::timeout(std::time::Duration::from_secs(15), call_tool(&state, json!({
            "name":"parallel_tools","arguments":{"tasks":[
                {"id":"fail","tool":"create_file","arguments":{"path":"exists.txt","content":"replacement"}},
                {"id":"dependent","tool":"move_path","arguments":{"source":"exists.txt","destination":"moved.txt"}},
                {"id":"transitive","tool":"read_file","arguments":{"path":"moved.txt"}},
                {"id":"independent","tool":"create_file","arguments":{"path":"independent.txt","content":"ok"}}
            ]}
        }))).await.expect("fan-out dependency integration must not deadlock").unwrap();
        let result = &response["structuredContent"];
        assert_eq!(result["dispatch"], "completion-driven");
        assert_eq!(result["succeeded"], 1);
        assert_eq!(result["failed"], 3);
        assert_eq!(result["items"][3]["ok"], true);
        for index in [1, 2] {
            assert!(result["items"][index]["error"]
                .as_str()
                .unwrap()
                .contains("dependency failed"));
        }
        assert_eq!(
            fs::read_to_string(dir.path().join("exists.txt")).unwrap(),
            "original"
        );
        assert!(!dir.path().join("moved.txt").exists());
        assert_eq!(
            fs::read_to_string(dir.path().join("independent.txt")).unwrap(),
            "ok"
        );
    }
}

#[tokio::test]
async fn cancelling_fanout_does_not_detach_queued_writes() {
    let dir = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([dir.path()], true, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(1).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    };
    let permit = state.harness.acquire().await.unwrap();
    let child_state = state.clone();
    let parent = tokio::spawn(async move {
        call_tool(
            &child_state,
            json!({"name":"parallel_tools","arguments":{"tasks":[
                {"tool":"create_file","arguments":{"path":"one.txt","content":"one"}},
                {"tool":"create_file","arguments":{"path":"two.txt","content":"two"}}
            ]}}),
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while state.monitor.connection_status().queued_tasks != 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    parent.abort();
    assert!(parent.await.unwrap_err().is_cancelled());
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while state.monitor.connection_status().queued_tasks != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cancelled parent must drop its queued children before slots reopen");
    drop(permit);
    assert!(!dir.path().join("one.txt").exists());
    assert!(!dir.path().join("two.txt").exists());
}

#[tokio::test]
async fn parallel_fanout_inherits_the_parent_workspace() {
    let root = tempfile::tempdir().unwrap();
    let api = root.path().join("api");
    let web = root.path().join("web");
    fs::create_dir_all(&api).unwrap();
    fs::create_dir_all(&web).unwrap();
    fs::write(api.join("name.txt"), "api\n").unwrap();
    fs::write(web.join("name.txt"), "web\n").unwrap();
    let workspaces = Workspaces::new([&api, &web], false, false).unwrap();
    let default_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([default_id]),
        tasks: TaskRuntime::default(),
    };

    let response = call_tool(
        &state,
        json!({
            "name":"parallel_tools",
            "arguments":{
                "workspace":"web",
                "tasks":[
                    {"id":"a","tool":"read_file","arguments":{"path":"name.txt"}},
                    {"id":"b","tool":"path_info","arguments":{"path":"name.txt"}},
                    {"id":"api","tool":"read_file","arguments":{"workspace":"api","path":"name.txt"}}
                ]
            }
        }),
    )
    .await
    .unwrap();
    let items = response["structuredContent"]["items"].as_array().unwrap();
    assert_eq!(items[0]["result"]["content"], "web");
    assert_eq!(items[1]["result"]["path"], "name.txt");
    assert_eq!(items[2]["result"]["content"], "api");
}

#[tokio::test]
async fn parallel_fanout_runs_writes_and_coalesces_same_file_edits() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("shared.txt"), "same\nmiddle\nsame\n").unwrap();
    let workspaces = Workspaces::new([dir.path()], true, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    };
    let read = call_tool(
        &state,
        json!({"name":"read_file","arguments":{"path":"shared.txt"}}),
    )
    .await
    .unwrap();
    let sha = read["structuredContent"]["sha256"].as_str().unwrap();

    let response = call_tool(
        &state,
        json!({
            "name":"parallel_tools",
            "arguments":{"tasks":[
                {"id":"create-a","tool":"create_file","arguments":{"path":"a.txt","content":"a\n"}},
                {"id":"create-b","tool":"create_file","arguments":{"path":"b.txt","content":"b\n"}},
                {"id":"edit-first","tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":sha,"edits":[{"old_text":"same","new_text":"FIRST","start_line":1,"end_line":1}]}},
                {"id":"edit-last","tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":sha,"edits":[{"old_text":"same","new_text":"LAST","start_line":3,"end_line":3}]}}
            ]}
        }),
    )
    .await
    .unwrap();

    let result = &response["structuredContent"];
    assert_eq!(result["succeeded"], 4, "{result:#}");
    assert_eq!(result["failed"], 0);
    assert_eq!(result["coalesced_same_file_edits"], 1);
    assert_eq!(
        fs::read_to_string(dir.path().join("shared.txt")).unwrap(),
        "FIRST\nmiddle\nLAST\n"
    );
    assert_eq!(fs::read_to_string(dir.path().join("a.txt")).unwrap(), "a\n");
    assert_eq!(fs::read_to_string(dir.path().join("b.txt")).unwrap(), "b\n");
}

#[tokio::test]
async fn parallel_fanout_orders_real_directory_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([dir.path()], true, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    };

    let response = call_tool(
        &state,
        json!({
            "name":"parallel_tools",
            "arguments":{"tasks":[
                {"id":"child","tool":"create_file","arguments":{"path":"src/domain/model.rs","content":"pub struct Model;\n"}},
                {"id":"parent","tool":"create_directory","arguments":{"path":"src/domain"}}
            ]}
        }),
    )
    .await
    .unwrap();

    let result = &response["structuredContent"];
    assert_eq!(result["scheduler"], "dependency-graph");
    assert_eq!(result["dependency_edges"], 1);
    assert_eq!(result["dependency_layers"], 2);
    assert_eq!(result["succeeded"], 2);
    assert!(dir.path().join("src/domain/model.rs").is_file());
}
