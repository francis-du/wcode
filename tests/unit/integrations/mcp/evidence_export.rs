use super::*;
use crate::engineering_journal::{change_fingerprint, load_recent, persist, EngineeringMilestone};
use crate::workspace::{Workspace, Workspaces};
use std::sync::Arc;

fn fixture() -> (tempfile::TempDir, AppState) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(root.path().join("src/lib.rs"), "pub fn baseline() {}\n").unwrap();
    let workspaces = Workspaces::new([root.path()], true, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".into())),
        workspaces,
        harness: ToolHarness::new(2).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    };
    (root, state)
}

fn persist_milestone(workspace: &Workspace, timestamp_ms: u64) {
    let mut event =
        EngineeringMilestone::new("verify_project", "prove", "succeeded", 1, Vec::new()).unwrap();
    event.timestamp_ms = timestamp_ms;
    persist(workspace, &event).unwrap();
}

async fn export(state: &AppState, arguments: Value) -> Value {
    let response = call_tool(
        state,
        json!({"name": "evidence_journal_export", "arguments": arguments}),
    )
    .await
    .unwrap();
    assert_eq!(response["isError"], false);
    response["structuredContent"].clone()
}

#[tokio::test]
async fn evidence_journal_export_is_reachable_and_returns_json_lines() {
    let (_root, state) = fixture();
    let (_, workspace) = state.workspaces.select(None).unwrap();
    for index in 0..3 {
        persist_milestone(&workspace, 1_000_000 + index);
    }
    let response = call_tool(
        &state,
        json!({"name": "evidence_journal_export", "arguments": {"limit": 2}}),
    )
    .await
    .unwrap();
    assert_eq!(response["isError"], false);
    let payload = &response["structuredContent"];
    assert_eq!(payload["format"], "json-lines");
    assert_eq!(payload["requested_limit"], 2);
    assert_eq!(payload["records"], 2);
    assert_eq!(payload["retained_records"], 3);
    assert_eq!(payload["truncated"], true);
    // A truncated window's oldest link points outside the export, so the
    // window itself cannot verify as a closed chain.
    assert_eq!(payload["chain_valid"], false);
    let lines = payload["export"]
        .as_str()
        .unwrap()
        .lines()
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    for line in lines {
        let record: Value = serde_json::from_str(line).unwrap();
        assert!(record.get("record_hash").is_some());
    }
    let text = response["content"][0]["text"].as_str().unwrap();
    assert_eq!(serde_json::from_str::<Value>(text).unwrap(), *payload);
    let full = export(&state, json!({"limit": 3})).await;
    assert_eq!(full["records"], 3);
    assert_eq!(full["truncated"], false);
    assert_eq!(full["chain_valid"], true);
}

#[tokio::test]
async fn evidence_journal_export_clamps_to_the_retained_capacity() {
    let (_root, state) = fixture();
    let (_, workspace) = state.workspaces.select(None).unwrap();
    for index in 0..512 {
        persist_milestone(&workspace, 1_000_000 + index);
    }
    let payload = export(&state, json!({"limit": 100_000})).await;
    assert_eq!(payload["requested_limit"], 100_000);
    assert_eq!(payload["records"], 512);
    assert_eq!(payload["retained_records"], 512);
    assert_eq!(payload["truncated"], false);
    assert_eq!(payload["chain_valid"], true);
    assert_eq!(payload["export"].as_str().unwrap().lines().count(), 512);
}

#[tokio::test]
async fn evidence_journal_export_is_read_only_and_preserves_journal_state() {
    let (_root, state) = fixture();
    let (_, workspace) = state.workspaces.select(None).unwrap();
    for index in 0..3 {
        persist_milestone(&workspace, 1_000_000 + index);
    }
    let fingerprint_before = change_fingerprint(&workspace).unwrap();
    let history_before = load_recent(&workspace, 512).unwrap();
    let payload = export(&state, json!({"limit": 2})).await;
    assert_eq!(payload["records"], 2);
    assert_eq!(change_fingerprint(&workspace).unwrap(), fingerprint_before);
    let history_after = load_recent(&workspace, 512).unwrap();
    assert_eq!(history_after.records.len(), history_before.records.len());
    assert_eq!(
        history_after.retained_records,
        history_before.retained_records
    );
    let tool = crate::mcp::mcp_tools::tools()
        .iter()
        .find(|tool| tool["name"] == "evidence_journal_export")
        .unwrap();
    assert_eq!(tool["annotations"]["readOnlyHint"], true);
    assert_ne!(tool["annotations"]["destructiveHint"], true);
    assert_eq!(
        crate::mcp::mcp_writer::tool_effect("evidence_journal_export", &json!({})),
        crate::execution_policy::ExecutionEffect::Read
    );
}

#[tokio::test]
async fn evidence_journal_export_rejects_limit_below_the_declared_minimum() {
    let (_root, state) = fixture();
    for arguments in [
        json!({"limit": 0}),
        json!({"limit": 1.5}),
        json!({"limit": "10"}),
    ] {
        let error = call_tool(
            &state,
            json!({"name": "evidence_journal_export", "arguments": arguments}),
        )
        .await
        .expect_err("limit below the schema minimum must fail closed");
        assert!(
            error.contains("limit must be"),
            "unexpected error for {arguments}: {error}"
        );
    }
}

#[tokio::test]
async fn evidence_journal_export_rejects_unknown_and_malformed_arguments() {
    let (_root, state) = fixture();
    for arguments in [
        json!({"limit": "ten"}),
        json!({"limit": -1}),
        json!({"limit": 1.5}),
        json!({"limit": null}),
        json!({"limit": []}),
        json!({"limit": {}}),
        json!({"bogus": true}),
        json!({"limit": 2, "extra": 1}),
        json!([]),
        json!(null),
    ] {
        let result = call_tool(
            &state,
            json!({"name": "evidence_journal_export", "arguments": arguments}),
        )
        .await;
        assert!(
            result.is_err(),
            "must fail closed for {arguments}: {result:?}"
        );
    }
    assert!(call_tool(
        &state,
        json!({"name": "evidence_journal_export", "arguments": {"workspace": 7}})
    )
    .await
    .is_err());
}
