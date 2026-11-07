use super::*;

#[tokio::test]
async fn agent_context_declares_workspace_selection_and_the_reusable_handle() {
    let default_root = tempfile::tempdir().unwrap();
    let other_root = tempfile::tempdir().unwrap();
    let workspaces =
        Workspaces::new([default_root.path(), other_root.path()], false, false).unwrap();
    let default_id = workspaces.default_id().to_owned();
    let other_id = workspaces
        .roots()
        .into_iter()
        .map(|(id, _)| id)
        .find(|id| id != &default_id)
        .expect("the second root must register its own workspace id");
    let state = AppState {
        workspaces,
        ..batch_test_state(default_root.path())
    };

    let routed = call_tool(
        &state,
        json!({
            "name": "agent_context",
            "arguments": {"query": format!("inspect {other_id}"), "budget": 1000}
        }),
    )
    .await
    .unwrap();
    assert_eq!(routed["isError"], false);
    assert_eq!(routed["structuredContent"]["workspace"], other_id);
    assert_eq!(
        routed["structuredContent"]["workspace_selection"]["mode"],
        "query_routed"
    );
    assert_eq!(
        routed["structuredContent"]["workspace_selection"]["default_workspace"],
        default_id
    );
    let routed_workflow = serde_json::to_string(&routed["structuredContent"]["workflow"]).unwrap();
    assert!(
        routed_workflow.contains(&other_id),
        "the routed context must name the handle to reuse: {routed_workflow}"
    );
    assert!(routed_workflow.contains(&default_id));

    // Omitting the handle still resolves to the default, so the declared handle is
    // the only thing that keeps a multi-root session on the routed repository.
    let omitted = call_tool(&state, json!({"name": "project_context", "arguments": {}}))
        .await
        .unwrap();
    assert_eq!(omitted["structuredContent"]["workspace"], default_id);

    let explicit = call_tool(
        &state,
        json!({
            "name": "project_context",
            "arguments": {"workspace": other_id}
        }),
    )
    .await
    .unwrap();
    assert_eq!(explicit["structuredContent"]["workspace"], other_id);

    let explicit_context = call_tool(
        &state,
        json!({
            "name": "agent_context",
            "arguments": {"query": "any", "budget": 1000, "workspace": other_id}
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        explicit_context["structuredContent"]["workspace_selection"]["mode"],
        "explicit"
    );
    let explicit_workflow =
        serde_json::to_string(&explicit_context["structuredContent"]["workflow"]).unwrap();
    assert!(!explicit_workflow.contains("the default Workspace is"));
}
