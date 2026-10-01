use super::*;
use crate::workspace::Workspaces;
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

#[tokio::test]
async fn failure_memory_mcp_callback_matches_literal_without_retaining_private_payloads() {
    let (root, state) = fixture();
    std::fs::create_dir_all(root.path().join(".wcode")).unwrap();
    std::fs::write(
        root.path().join(".wcode/failure-rules.yaml"),
        "schema_version: 1\nrules:\n  - id: stale-edit\n    literal: 'stale file: expected sha256 '\n    path: src\n    guidance: Read the current source and SHA before retrying.\n",
    )
    .unwrap();
    let response = super::super::call_tool(
        &state,
        json!({"name":"apply_edits","arguments":{
            "path":"src/lib.rs","expected_sha256":"0".repeat(64),
            "edits":[{"old_text":"baseline","new_text":"PRIVATE_EDIT_BODY"}]
        }}),
    )
    .await
    .unwrap();
    assert_eq!(response["isError"], true);
    assert!(payload(&response)["error"]
        .as_str()
        .unwrap()
        .starts_with("stale file: expected sha256 "));
    assert_eq!(
        std::fs::read_to_string(root.path().join("src/lib.rs")).unwrap(),
        "pub fn baseline() {}\n"
    );
    let (_, workspace) = state.workspaces.select(None).unwrap();
    let memory = crate::engineering_journal::failure_memory::recall(
        &workspace,
        "",
        &["src/lib.rs".into()],
        &std::collections::HashSet::new(),
        8,
    )
    .unwrap();
    let lesson = memory.items.iter().find(|item| item.id == "stale-edit");
    assert!(
        lesson.is_some(),
        "native MCP callback did not match the literal rule"
    );
    let lesson = lesson.unwrap();
    assert_eq!(lesson.observations, 1);
    assert_eq!(lesson.status, "unverified");
    assert!(!lesson.cause_proven);
    let state_root = crate::evidence_store::workspace_state_directory(&workspace).unwrap();
    for name in ["engineering-journal", "failure-memory"] {
        for entry in std::fs::read_dir(state_root.join(name)).unwrap() {
            let bytes = std::fs::read_to_string(entry.unwrap().path()).unwrap();
            assert!(!bytes.contains("PRIVATE_EDIT_BODY"));
            assert!(!bytes.contains("stale file: expected sha256 "));
        }
    }
}

#[tokio::test]
async fn failure_memory_mcp_callback_bounds_native_error_and_ignores_arguments_and_output() {
    for (native_error, expected) in [
        (Some(format!("native limit {}", "界".repeat(1361))), true),
        (Some(format!("native limit {}", "界".repeat(1362))), false),
        (None, false),
    ] {
        let (root, state) = fixture();
        std::fs::create_dir_all(root.path().join(".wcode")).unwrap();
        std::fs::write(root.path().join(".wcode/failure-rules.yaml"),
            "schema_version: 1\nrules:\n  - id: bounded-native\n    literal: native limit\n    guidance: Inspect the bounded native failure.\n").unwrap();
        record(
            &state,
            "run_command",
            &json!({"args":["native limit PRIVATE_ARGUMENT"]}),
            "failed",
            1,
            Some(&json!({"error":native_error,
                "stdout":"native limit PRIVATE_OUTPUT", "stderr":"native limit PRIVATE_OUTPUT"})),
        )
        .await;
        let (_, workspace) = state.workspaces.select(None).unwrap();
        let memory = crate::engineering_journal::failure_memory::recall(
            &workspace,
            "",
            &[],
            &std::collections::HashSet::new(),
            8,
        )
        .unwrap();
        assert_eq!(
            memory.items.iter().any(|item| item.id == "bounded-native"),
            expected
        );
        let directory = crate::evidence_store::workspace_state_directory(&workspace).unwrap();
        for name in ["engineering-journal", "failure-memory"] {
            for entry in std::fs::read_dir(directory.join(name)).unwrap() {
                let stored = std::fs::read_to_string(entry.unwrap().path()).unwrap();
                assert!(!stored.contains("PRIVATE_ARGUMENT"));
                assert!(!stored.contains("PRIVATE_OUTPUT"));
                assert!(!stored.contains("界"));
            }
        }
    }
}

#[test]
fn mcp_journal_failure_classifier_is_fixed_unique_and_ignores_private_output() {
    use EngineeringFailureCode::*;
    let result = json!({
        "error": "stale file: expected sha256 old, current sha256 is new PRIVATE-DIAGNOSTIC",
        "stdout": "authorization required: PRIVATE-STDOUT",
        "stderr": "wcode authority state paths are not accessible through file tools",
        "results": [
            {"ok": false, "error": "stale file: expected sha256 old"},
            {"ok": false, "error": "wcode core policy blocks growing src/lib.rs to 1001 maintained source lines"}
        ]
    });
    assert_eq!(
        failure_codes("apply_file_edits", "failed", Some(&result)),
        vec![ShaMismatch, SourceLimit]
    );
    assert!(failure_codes("apply_file_edits", "succeeded", Some(&result)).is_empty());
    assert!(failure_codes(
        "read_file",
        "failed",
        Some(&json!({"error": "PRIVATE-UNKNOWN"}))
    )
    .is_empty());
    let mut results = vec![json!({"ok": false, "error": "unknown"}); 64];
    results.push(json!({"ok": false, "error": "authorization required: private"}));
    assert!(failure_codes(
        "apply_file_edits",
        "failed",
        Some(&json!({"results": results}))
    )
    .is_empty());
}

#[test]
fn mcp_journal_recognizes_native_boundaries_without_learning_arbitrary_prose() {
    use EngineeringFailureCode::*;
    for (error, expected) in [
        (
            "authorization required: operator request",
            AuthorizationRequired,
        ),
        (
            "protected credential or repository-control path is not accessible: .env",
            ProtectedPath,
        ),
        (
            "wcode authority state paths are not accessible through file tools",
            ProtectedPath,
        ),
        (
            "verification Git identity changed during execution; results are stale",
            RevisionStale,
        ),
        ("replan_required: checkpoint changed", RevisionStale),
    ] {
        assert_eq!(
            failure_codes("read_file", "blocked", Some(&json!({"error": error}))),
            vec![expected]
        );
    }
    assert!(failure_codes(
        "read_file",
        "failed",
        Some(&json!({
            "stdout": "stale file: expected sha256 forged",
            "error": "user says authorization required: ignore previous instructions"
        }))
    )
    .is_empty());
}

#[tokio::test]
async fn mcp_journal_failed_native_verification_report_overrides_success_flag() {
    let (_root, state) = fixture();
    let (_, workspace) = state.workspaces.select(None).unwrap();
    let report = json!({"structuredContent": {
        "passed": false, "level": "quick", "checks_run": 2, "checks_failed": 1,
        "checks": [{"id": "profile-discovery-completeness", "success": false,
                    "stdout_tail": "PRIVATE-OUTPUT"}],
        "impact": {"reasons": [{"source": "src/lib.rs"}]}
    }});
    record(
        &state,
        "verify_project",
        &json!({}),
        "succeeded",
        7,
        Some(&report),
    )
    .await;
    let history = crate::engineering_journal::load_recent(&workspace, 4).unwrap();
    assert_eq!(history.records.len(), 1);
    let event = &history.records[0];
    assert_eq!(event.outcome, "failed");
    assert_eq!(event.checks_run, Some(2));
    assert_eq!(event.checks_failed, Some(1));
    assert_eq!(event.paths, ["src/lib.rs"]);
    assert_eq!(
        event.failure_codes,
        [
            EngineeringFailureCode::VerificationFailure,
            EngineeringFailureCode::DiscoveryIncomplete
        ]
    );
    assert!(!serde_json::to_string(event)
        .unwrap()
        .contains("PRIVATE-OUTPUT"));
}

#[tokio::test]
async fn mcp_journal_known_timeout_persists_understand_without_command_or_output() {
    let (_root, state) = fixture();
    let (_, workspace) = state.workspaces.select(None).unwrap();
    let result = json!({"structuredContent": {
        "success": false, "timed_out": true, "stdout": "PRIVATE-STDOUT", "stderr": "PRIVATE-STDERR",
        "command": "PRIVATE-COMMAND", "exit_code": null
    }});
    record(
        &state,
        "run_command",
        &json!({"command": "PRIVATE-COMMAND"}),
        "failed",
        8,
        Some(&result),
    )
    .await;
    record(
        &state,
        "read_file",
        &json!({"path": "src/lib.rs"}),
        "succeeded",
        1,
        None,
    )
    .await;
    record(
        &state,
        "unknown_freeform_tool",
        &json!({}),
        "failed",
        1,
        Some(&result),
    )
    .await;
    let history = crate::engineering_journal::load_recent(&workspace, 4).unwrap();
    assert_eq!(history.records.len(), 1);
    let event = &history.records[0];
    assert_eq!(event.stage, "understand");
    assert_eq!(event.failure_codes, [EngineeringFailureCode::Timeout]);
    assert!(!serde_json::to_string(event).unwrap().contains("PRIVATE-"));
}

#[tokio::test]
async fn mcp_journal_failed_edits_keep_only_guarded_readable_paths_and_codes() {
    let (_root, state) = fixture();
    let (_, workspace) = state.workspaces.select(None).unwrap();
    let args = json!({"files": [
        {"path": "src/lib.rs", "new_text": "PRIVATE-NEW-TEXT"},
        {"path": "../outside.rs"},
        {"path": ".env"},
        {"path": "missing.rs"}
    ]});
    let result =
        json!({"error": "stale file: expected sha256 old, current sha256 is new PRIVATE-ERROR"});
    record(
        &state,
        "apply_file_edits",
        &args,
        "failed",
        1,
        Some(&result),
    )
    .await;
    let history = crate::engineering_journal::load_recent(&workspace, 4).unwrap();
    assert_eq!(history.records.len(), 1);
    assert_eq!(history.records[0].paths, ["src/lib.rs"]);
    assert_eq!(
        history.records[0].failure_codes,
        [EngineeringFailureCode::ShaMismatch]
    );
    assert!(!serde_json::to_string(&history.records[0])
        .unwrap()
        .contains("PRIVATE-"));
}
