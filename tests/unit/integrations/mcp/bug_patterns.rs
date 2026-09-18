use super::*;
use std::fs;

#[tokio::test]
async fn text_pattern_scan_skips_pure_comments_by_default() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("comments.go"),
        "package demo\n// helper()\n/* helper() */\nfunc run() { helper() }\nfunc inline() { helper() } // helper()\n",
    )
    .unwrap();
    let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    };

    let filtered = call_tool(
        &state,
        json!({"name":"scan_patterns","arguments":{"patterns":["helper\\(\\)"]}}),
    )
    .await
    .unwrap();
    assert_eq!(filtered["isError"], false);
    assert_eq!(filtered["structuredContent"]["count"], 2);
    assert_eq!(filtered["structuredContent"]["total_matches"], 2);
    assert_eq!(filtered["structuredContent"]["include_comments"], false);

    let included = call_tool(
        &state,
        json!({"name":"scan_patterns","arguments":{
            "patterns":["helper\\(\\)"],"include_comments":true
        }}),
    )
    .await
    .unwrap();
    assert_eq!(included["isError"], false);
    assert_eq!(included["structuredContent"]["count"], 4);
    assert_eq!(included["structuredContent"]["total_matches"], 4);
    assert_eq!(included["structuredContent"]["include_comments"], true);
}

#[tokio::test]
async fn go_common_bug_preset_returns_only_ast_validated_candidates() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("bugs.go"),
        r#"package demo
func inspect(xs []int, rsp *Response, t *testing.T) {
    if len(xs) == 0 { return }
    _ = xs[1]
    _ = rsp.BatchList[0]
    _ = mayFail()
    t.Run("empty", func(t *testing.T) {})
}
type Response struct { BatchList []int }
"#,
    )
    .unwrap();

    let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    };

    let found = call_tool(
        &state,
        json!({"name":"scan_patterns","arguments":{"preset":"go_common_bugs","auto_page":true}}),
    )
    .await
    .unwrap();
    assert_eq!(found["isError"], false);
    let data = &found["structuredContent"];
    assert_eq!(data["provider"], "tree-sitter-bug-patterns");
    assert_eq!(data["preset"], "go_common_bugs");
    assert_eq!(data["coverage_complete"], true);

    for pattern in [
        "index_mismatch",
        "unguarded_subscript",
        "err_swallowed",
        "empty_test",
    ] {
        assert!(
            data["pattern_counts"][pattern].as_u64().unwrap_or_default() >= 1,
            "{pattern}"
        );
    }
    assert!(data["matches"].as_array().unwrap().iter().all(|row| {
        row["bug_patterns"]
            .as_array()
            .is_some_and(|patterns| !patterns.is_empty())
    }));

    let mismatch_only = call_tool(
        &state,
        json!({"name":"scan_patterns","arguments":{"pattern":"index_mismatch"}}),
    )
    .await
    .unwrap();
    let data = &mismatch_only["structuredContent"];
    assert_eq!(data["count"], 1);
    assert_eq!(data["matches"][0]["text"], "xs[1]");
}

#[tokio::test]
async fn go_common_bug_preset_recovers_all_three_empty_subtests_without_assertion_noise() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("empty_tests.go"),
        r#"package demo
func TestCases(t *testing.T) {
    t.Run("empty-inline", func(t *testing.T) {})
    t.Run("empty-work", func(t *testing.T) { setup(); exercise() })
    t.Run("empty-helper", func(t *testing.T) { value := compute(); _ = value })
    t.Run("asserted", func(t *testing.T) { assert.Equal(t, 2, compute()) })
}
"#,
    )
    .unwrap();

    let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    };

    let found = call_tool(
        &state,
        json!({"name":"scan_patterns","arguments":{
            "preset":"go_common_bugs","auto_page":true
        }}),
    )
    .await
    .unwrap();
    assert_eq!(found["isError"], false);
    let data = &found["structuredContent"];
    assert_eq!(data["pattern_counts"]["empty_test"], 3);
    let empty_tests = data["matches"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            row["bug_patterns"]
                .as_array()
                .is_some_and(|patterns| patterns.iter().any(|pattern| pattern == "empty_test"))
        })
        .collect::<Vec<_>>();
    assert_eq!(empty_tests.len(), 3);
    assert!(empty_tests.iter().all(|row| !row["text"]
        .as_str()
        .unwrap_or_default()
        .contains("assert.Equal")));
}
