use super::*;

async fn preview_call(state: &AppState, arguments: Value) -> Result<Value, String> {
    call_tool(
        state,
        json!({"name": "parallel_tools", "arguments": arguments}),
    )
    .await
}

#[tokio::test]
async fn audit_verification_options_reject_invalid_explicit_values_before_execution() {
    let root = tempfile::tempdir().unwrap();
    let state = batch_test_state(root.path());
    for (key, value) in [
        ("level", Value::Null),
        ("level", json!(0)),
        ("level", json!([])),
        ("level", json!("")),
        ("level", json!("unknown")),
        ("timeout_seconds", Value::Null),
        ("timeout_seconds", json!("120")),
        ("timeout_seconds", json!(0)),
        ("timeout_seconds", json!(-1)),
        ("timeout_seconds", json!(301)),
        ("timeout_seconds", json!(1.5)),
        ("fail_fast", Value::Null),
    ] {
        let mut arguments = json!({});
        arguments[key] = value;
        let error = call_tool(
            &state,
            json!({"name": "verify_project", "arguments": arguments}),
        )
        .await
        .unwrap_err();
        assert!(
            error.contains(key),
            "invalid {key} must be rejected directly: {error}"
        );
    }
    assert!(state.workspaces.authorization_requests(10).is_empty());
}

#[tokio::test]
async fn dependency_preview_never_executes_or_waits_for_tool_slots() {
    let root = tempfile::tempdir().unwrap();
    let mut state = batch_test_state(root.path());
    state.harness = ToolHarness::new(1).unwrap();
    let _permit = state.harness.acquire().await.unwrap();
    let args = json!({"dry_run": true, "tasks": [
        {"tool": "create_file", "arguments": {"path": "a.txt", "content": "private_payload"}},
        {"tool": "read_file", "arguments": {"path": "a.txt"}},
        {"tool": "create_file", "arguments": {"path": "b.txt", "content": "private_payload"}}
    ]});
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        preview_call(&state, args),
    )
    .await
    .unwrap()
    .unwrap();
    let plan = &response["structuredContent"];
    assert_eq!(response["isError"], false);
    assert_eq!(plan["execution"], "dependency-preview");
    assert_eq!(plan["tasks_executed"], 0);
    assert_eq!(plan["waves"], json!([[0, 2], [1]]));
    assert_eq!(plan["tasks"][1]["depends_on"], json!([0]));
    assert_eq!(plan["initial_concurrency"], 1);
    assert_eq!(plan["authorization_checked"], false);
    assert_eq!(plan["file_preconditions_checked"], false);
    assert!(!root.path().join("a.txt").exists());
    assert!(!root.path().join("b.txt").exists());
    assert!(!response.to_string().contains("private_payload"));
    assert!(state.workspaces.authorization_requests(10).is_empty());
}

#[tokio::test]
async fn dependency_preview_reports_coalescing_without_applying_edits() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.txt"), "first second").unwrap();
    let state = batch_test_state(root.path());
    let args = json!({"dry_run": true, "tasks": [
        {"tool": "apply_edits", "arguments": {"path": "a.txt", "expected_sha256": "unverified",
            "edits": [{"old_text": "first", "new_text": "changed"}]}},
        {"tool": "apply_edits", "arguments": {"path": "a.txt", "expected_sha256": "unverified",
            "edits": [{"old_text": "second", "new_text": "also changed"}]}},
        {"tool": "read_file", "arguments": {"path": "a.txt"}}
    ]});
    let response = preview_call(&state, args).await.unwrap();
    let plan = &response["structuredContent"];
    assert_eq!(plan["coalesced_same_file_edits"], 1);
    assert_eq!(plan["tasks"][1]["coalesced_into"], 0);
    assert_eq!(plan["waves"], json!([[0], [2]]));
    assert_eq!(plan["tasks"][2]["depends_on"], json!([0]));
    assert_eq!(
        std::fs::read_to_string(root.path().join("a.txt")).unwrap(),
        "first second"
    );
}

#[tokio::test]
async fn dependency_preview_rejects_invalid_flags_and_resource_models() {
    let root = tempfile::tempdir().unwrap();
    let state = batch_test_state(root.path());
    for value in [Value::Null, json!("true"), json!(1), json!([])] {
        let args = json!({"dry_run": value, "tasks": [
            {"tool": "create_file", "arguments": {"path": "never.txt", "content": "x"}},
            {"tool": "read_file", "arguments": {"path": "never.txt"}}
        ]});
        let response = preview_call(&state, args).await;
        assert!(response.unwrap_err().contains("dry_run must be a boolean"));
    }
    let args = json!({"dry_run": true, "tasks": [
        {"tool": "create_file", "arguments": {"path": "never.txt", "content": "x"}},
        {"tool": "read_file", "arguments": {"path": "../outside.txt"}}
    ]});
    assert!(preview_call(&state, args).await.is_err());
    assert!(!root.path().join("never.txt").exists());
}

#[tokio::test]
async fn dependency_preview_is_not_an_authorization_grant() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("keep.txt"), "keep").unwrap();
    let state = AppState {
        workspaces: Workspaces::new([root.path()], false, false).unwrap(),
        ..batch_test_state(root.path())
    };
    let args = json!({"dry_run": true, "tasks": [
        {"tool": "delete_path", "arguments": {"path": "keep.txt"}},
        {"tool": "create_file", "arguments": {"path": "new.txt", "content": "new"}}
    ]});
    let response = preview_call(&state, args).await.unwrap();
    assert_eq!(
        response["structuredContent"]["authorization_checked"],
        false
    );
    assert!(root.path().join("keep.txt").exists());
    assert!(!root.path().join("new.txt").exists());
    assert!(state.workspaces.authorization_requests(10).is_empty());
}

#[tokio::test]
async fn explicit_false_preserves_normal_parallel_execution() {
    let root = tempfile::tempdir().unwrap();
    let state = batch_test_state(root.path());
    let args = json!({"dry_run": false, "tasks": [
        {"tool": "create_file", "arguments": {"path": "a.txt", "content": "a"}},
        {"tool": "read_file", "arguments": {"path": "a.txt"}}
    ]});
    let response = preview_call(&state, args).await.unwrap();
    assert_eq!(
        response["structuredContent"]["execution"],
        "parallel-fanout"
    );
    assert_eq!(response["structuredContent"]["failed"], 0);
    assert_eq!(
        response["structuredContent"]["items"][1]["result"]["content"],
        "a"
    );
}
