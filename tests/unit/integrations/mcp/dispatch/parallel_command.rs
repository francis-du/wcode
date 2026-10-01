use super::*;

#[tokio::test]
async fn parallel_tools_accepts_synchronous_commands_but_rejects_detached_tasks() {
    let root = tempfile::tempdir().unwrap();
    let second = root.path().join("second");
    std::fs::create_dir(&second).unwrap();
    let state = AppState {
        workspaces: Workspaces::new([root.path()], true, true).unwrap(),
        ..batch_test_state(root.path())
    };
    let default_id = state.workspaces.default_id().to_owned();
    let (second_id, _) = state
        .workspaces
        .add_workspace_from(Some(&default_id), "second")
        .unwrap();

    let preview = call_tool(
        &state,
        json!({"name":"parallel_tools","arguments":{"dry_run":true,"tasks":[
            {"id":"one","tool":"run_command","arguments":{"workspace":default_id,"program":"git","args":["status","--short"]}},
            {"id":"two","tool":"run_command","arguments":{"workspace":second_id,"program":"git","args":["status","--short"]}}
        ]}}),
    )
    .await
    .unwrap();
    assert_eq!(preview["isError"], false);
    assert_eq!(preview["structuredContent"]["dependency_edges"], 0);
    assert_eq!(preview["structuredContent"]["dependency_layers"], 1);

    let detached = call_tool(
        &state,
        json!({"name":"parallel_tools","arguments":{"tasks":[
            {"tool":"run_command","arguments":{"program":"git","args":["status"],"task_mode":true}},
            {"tool":"path_info","arguments":{"path":"."}}
        ]}}),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(detached.contains("durable command tasks require a top-level tools/call"));
}
