use super::*;

#[tokio::test]
async fn admission_regression_saturated_workspace_info_still_diagnoses_capacity() {
    let root = tempfile::tempdir().unwrap();
    let state = AppState {
        harness: ToolHarness::new(2).unwrap(),
        ..batch_test_state(root.path())
    };
    let held = [
        state.harness.acquire().await.unwrap(),
        state.harness.acquire().await.unwrap(),
    ];
    let response = tokio::time::timeout(
        std::time::Duration::from_millis(300),
        call_tool(&state, json!({"name":"workspace_info","arguments":{}})),
    )
    .await
    .expect("overload diagnostics must not queue behind the work they diagnose")
    .unwrap();
    let diagnostic = &response["structuredContent"];
    assert_eq!(response["isError"], false);
    assert_eq!(diagnostic["status"], "partial");
    assert_eq!(diagnostic["diagnostic_only"], true);
    assert_eq!(diagnostic["admission"]["slots_in_use"], 2);
    assert_eq!(diagnostic["admission"]["total_limit"], 2);
    assert!(diagnostic.get("workspaces").is_none());
    assert!(diagnostic["runtime"]["process_id"].as_u64().is_some());
    assert!(serde_json::to_vec(diagnostic).unwrap().len() < 4096);
    let read = tokio::time::timeout(
        std::time::Duration::from_millis(25),
        call_tool(
            &state,
            json!({"name":"read_file","arguments":{"path":"missing"}}),
        ),
    )
    .await;
    assert!(
        read.is_err(),
        "diagnostics must not grant a general admission bypass"
    );
    assert_eq!(state.monitor.connection_status().queued_tasks, 0);
    drop(held);
    let recovered = call_tool(&state, json!({"name":"workspace_info","arguments":{}}))
        .await
        .unwrap();
    assert_eq!(recovered["isError"], false);
    assert!(recovered["structuredContent"].get("workspaces").is_some());
}

#[tokio::test]
async fn admission_regression_status_race_falls_back_without_extending_capacity() {
    let root = tempfile::tempdir().unwrap();
    let state = AppState {
        harness: ToolHarness::new(1).unwrap(),
        ..batch_test_state(root.path())
    };
    assert!(state.harness.overload_diagnostic().is_none());
    // Capacity changes after the initial overload observation, before admission.
    let held = state.harness.acquire_tool(false).await.unwrap();
    let diagnostic = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        crate::mcp::mcp_tools::acquire_status_permit(&state),
    )
    .await
    .unwrap()
    .err()
    .unwrap();
    assert_eq!(diagnostic["diagnostic_only"], true);
    assert_eq!(diagnostic["admission"]["slots_in_use"], 1);
    assert_eq!(state.harness.admission_snapshot().waiting_for_slot, 0);
    let invalid = call_tool(
        &state,
        json!({"name":"workspace_info","arguments":{"workspace":123}}),
    )
    .await;
    assert!(
        invalid.is_err(),
        "diagnostics must not bypass argument validation"
    );
    drop(held);
    assert!(crate::mcp::mcp_tools::acquire_status_permit(&state)
        .await
        .is_ok());
}

#[tokio::test]
async fn queued_commands_leave_capacity_for_real_file_reads() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("note.txt"),
        "readable while commands wait\n",
    )
    .unwrap();
    let state = AppState {
        workspaces: Workspaces::new([root.path()], true, true).unwrap(),
        harness: ToolHarness::new(32).unwrap(),
        ..batch_test_state(root.path())
    };
    let mut held = Vec::new();
    for _ in 0..crate::resource::limits().child_processes {
        held.push(crate::resource::global().acquire_child().await.unwrap());
    }
    let mut requests = (0..32)
        .map(|_| {
            Box::pin(call_tool(
                &state,
                json!({"name":"run_command","arguments":{"program":"rustc","args":["--version"]}}),
            ))
        })
        .collect::<Vec<_>>();
    // Poll each request into its resource wait without launching any command.
    for request in &mut requests {
        assert!(futures_util::poll!(request.as_mut()).is_pending());
    }
    let started = std::time::Instant::now();
    let read = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        call_tool(
            &state,
            json!({"name":"read_file","arguments":{"path":"note.txt"}}),
        ),
    )
    .await;
    let read_elapsed_us = started.elapsed().as_micros();
    // Cancel before freeing process capacity so no queued program can start.
    drop(requests);
    drop(held);
    let mut recovered = Vec::new();
    for _ in 0..32 {
        recovered.push(
            tokio::time::timeout(std::time::Duration::from_secs(1), state.harness.acquire())
                .await
                .unwrap()
                .unwrap(),
        );
    }
    drop(recovered);
    assert!(state.workspaces.authorization_requests(100).is_empty());
    let read = read
        .expect("waiting commands must not occupy every tool slot")
        .unwrap();
    assert_eq!(read["isError"], false);
    assert!(read["structuredContent"]["content"]
        .as_str()
        .unwrap()
        .contains("readable"));
    let report = json!({"queued_command_requests":32,"tool_limit":32,
        "read_elapsed_us":read_elapsed_us,"all_queued_commands_cancelled_before_launch":true,
        "scope":"in-process MCP saturation fixture; no network or model latency"});
    let output =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/wcode-admission.json");
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    std::fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
