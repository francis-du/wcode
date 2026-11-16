use super::*;

#[test]
fn acceptance_arguments_reject_forged_proof_and_approval() {
    for value in [
        json!({"evidence":[]}),
        json!({"ready":true}),
        json!({"approved":true}),
        json!({"action":"approve"}),
        json!({"actor":"owner"}),
        json!({"base_revision":"main"}),
        json!({"target_revision":"HEAD~1"}),
        json!({"base_revision":"worktree"}),
        json!({"action":"inspect","timeout_seconds":120}),
        json!({"action":"history","base_revision":"HEAD"}),
        json!({"action":"verify","timeout_seconds":1801}),
        json!({"workspace":null}),
    ] {
        assert!(Args::parse(&value).is_err(), "{value}");
    }
    for action in ["inspect", "plan", "record", "verify", "history"] {
        assert!(Args::parse(&json!({"action":action})).is_ok());
    }
}

fn app(root: &std::path::Path) -> AppState {
    let workspaces = crate::workspace::Workspaces::new_with_security(
        [root],
        true,
        true,
        crate::workspace::WorkspaceSecurity {
            allow_risky_exec: true,
            allow_unrestricted_commands: true,
            ..Default::default()
        },
    )
    .unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".into())),
        workspaces,
        harness: ToolHarness::new(2).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    }
}

#[tokio::test]
async fn change_acceptance_reaches_the_dispatcher_for_read_only_actions() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"acceptance_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(root.path().join("src/lib.rs"), "pub fn baseline() {}\n").unwrap();
    let state = app(root.path());

    for action in ["inspect", "history"] {
        let result = crate::mcp::call_tool_owned(
            &state,
            json!({"name":"change_acceptance","arguments":{"action":action}}),
            &"a".repeat(64),
        )
        .await
        .unwrap_or_else(|error| panic!("{action} was not dispatched: {error}"));
        assert_eq!(result["isError"], false, "{action}: {result}");
    }
}
