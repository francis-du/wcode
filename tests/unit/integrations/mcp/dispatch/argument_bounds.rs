use super::*;

const BOUNDS: &[(&str, &str, usize, Option<usize>, u64)] = &[
    ("software_graph", "max_files", 1, Some(5_000), 500),
    ("software_graph", "max_symbols", 1, Some(5_000), 1_000),
    ("semantic_provider_refresh", "max_files", 1, Some(256), 128),
    (
        "semantic_provider_refresh",
        "max_symbols",
        1,
        Some(2_000),
        1_000,
    ),
    ("search_syntax", "max_files", 1, Some(50_000), 50_000),
    ("search_syntax", "max_results", 1, Some(2_000), 200),
    ("file_outline", "max_symbols", 1, Some(1_000), 500),
    ("find_symbol", "max_results", 1, Some(200), 50),
    ("symbol_context", "max_body_lines", 1, Some(1_000), 1_000),
    ("list_files", "max_entries", 1, Some(10_000), 2_000),
    ("semantic_navigation", "line", 1, None, 1),
    ("semantic_navigation", "character", 1, None, 1),
    ("semantic_navigation", "max_results", 1, Some(100), 50),
    ("software_context", "budget", 1_000, Some(64_000), 12_000),
    ("agent_context", "budget", 1_000, Some(12_000), 1_000),
    (
        "language_quality_run",
        "timeout_seconds",
        1,
        Some(1_800),
        120,
    ),
    ("review_changes", "timeout_seconds", 1, Some(120), 30),
    ("drift_status", "timeout_seconds", 1, Some(120), 30),
    ("risk_status", "timeout_seconds", 1, Some(120), 30),
    ("impact_analysis", "timeout_seconds", 1, Some(120), 30),
    ("verification_plan", "timeout_seconds", 1, Some(120), 30),
    ("reconciliation_plan", "timeout_seconds", 1, Some(120), 30),
    ("read_file", "start_line", 1, None, 1),
    ("read_file", "end_line", 1, None, 1),
    ("read_files", "start_line", 1, None, 1),
    ("read_files", "end_line", 1, None, 1),
    ("replace_text", "start_line", 1, None, 1),
    ("replace_text", "end_line", 1, None, 1),
    ("scan_patterns", "max_results", 1, Some(2_000), 500),
];

fn base_arguments(tool: &str) -> Value {
    match tool {
        "search_syntax" => json!({"node_kinds": ["function"]}),
        "file_outline" => json!({"path": "src/lib.rs"}),
        "find_symbol" => json!({"query": "sample"}),
        "symbol_context" => json!({"symbol_id": "symbol:rust:sample"}),
        "semantic_navigation" => json!({"path": "src/lib.rs", "line": 1, "character": 1}),
        "software_context" => json!({"query": "sample"}),
        "agent_context" => json!({"query": "sample"}),
        "language_quality_run" => json!({"language": "rust", "provider_id": "rustfmt"}),
        "read_file" => json!({"path": "sample.txt"}),
        "read_files" => json!({"paths": ["sample.txt"]}),
        "replace_text" => {
            json!({"path": "sample.txt", "old_text": "alpha", "new_text": "beta", "expected_sha256": "0000000000000000000000000000000000000000000000000000000000000000", "start_line": 1, "end_line": 1})
        }
        "scan_patterns" => json!({"preset": "go_common_bugs"}),
        _ => json!({}),
    }
}

fn case_arguments(tool: &str, key: &str, value: Value) -> Value {
    let mut args = base_arguments(tool);
    args[key] = value;
    args
}

#[tokio::test]
async fn bounded_arguments_reject_values_outside_the_declared_schema() {
    let root = tempfile::tempdir().unwrap();
    let state = batch_test_state(root.path());
    for &(tool, key, min, max, _) in BOUNDS {
        let mut invalid = vec![json!(min.saturating_sub(1))];
        if let Some(max) = max {
            invalid.push(json!(max + 1));
        }
        for value in invalid {
            let args = case_arguments(tool, key, value.clone());
            let error = match call_tool(&state, json!({"name": tool, "arguments": args})).await {
                Ok(response) => panic!("{tool} {key} {value} must be rejected, got: {response}"),
                Err(error) => error,
            };
            assert!(
                error.contains(&format!("{key} must be")),
                "unexpected error for {tool} {key} {value}: {error}"
            );
        }
    }
}

#[tokio::test]
async fn bounded_arguments_accept_schema_permitted_values() {
    let (_root, state) = permitted_fixture();
    let symbol = call_tool(
        &state,
        json!({"name": "find_symbol", "arguments": {"query": "sample"}}),
    )
    .await
    .unwrap();
    let symbol_id = symbol["structuredContent"]["results"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let sha_start = path_sha(&state, "sample_start.txt").await;
    let sha_end = path_sha(&state, "sample_end.txt").await;

    for &(tool, key, _, _, permitted) in BOUNDS {
        let mut args = case_arguments(tool, key, json!(permitted));
        if tool == "symbol_context" {
            args["symbol_id"] = json!(symbol_id);
        }
        if tool == "replace_text" {
            if key == "start_line" {
                args["path"] = json!("sample_start.txt");
                args["expected_sha256"] = json!(sha_start);
            } else {
                args["path"] = json!("sample_end.txt");
                args["expected_sha256"] = json!(sha_end);
            }
        }
        let response = call_tool(&state, json!({"name": tool, "arguments": args}))
            .await
            .unwrap_or_else(|error| panic!("{tool} {key} {permitted} must be accepted: {error}"));
        assert_eq!(
            response["isError"], false,
            "{tool} {key} {permitted}: {response}"
        );
    }
}

async fn path_sha(state: &AppState, path: &str) -> String {
    let info = call_tool(
        state,
        json!({"name": "path_info", "arguments": {"path": path}}),
    )
    .await
    .unwrap();
    info["structuredContent"]["sha256"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn permitted_state(root: &std::path::Path) -> AppState {
    let workspaces = Workspaces::new([root], true, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    }
}

fn git(root: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn permitted_fixture() -> (tempfile::TempDir, AppState) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"argument_bounds_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(root.path().join("src/lib.rs"), "pub fn sample() {}\n").unwrap();
    std::fs::write(root.path().join("sample.txt"), "alpha\n").unwrap();
    std::fs::write(root.path().join("sample_start.txt"), "alpha\n").unwrap();
    std::fs::write(root.path().join("sample_end.txt"), "alpha\n").unwrap();
    git(root.path(), &["init", "--quiet"]);
    git(root.path(), &["config", "user.name", "Fixture"]);
    git(
        root.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(root.path(), &["add", "."]);
    git(
        root.path(),
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "--no-verify",
            "-m",
            "fixture",
        ],
    );
    let state = permitted_state(root.path());
    (root, state)
}
