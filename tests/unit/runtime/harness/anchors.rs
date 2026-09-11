use super::*;

fn anchor_fixture(root: &std::path::Path) -> (Workspace, ToolHarness) {
    fs::create_dir_all(root.join("src/runtime")).unwrap();
    let mut lines = vec!["pub fn anchor_worker() {".to_owned()];
    lines.extend((2..120).map(|_| "    let _ = 0;".to_owned()));
    lines.push("    let diagnostic_marker = 42;".to_owned());
    lines.push("    let _ = diagnostic_marker;".to_owned());
    lines.push("}".to_owned());
    fs::write(root.join("src/runtime/worker.rs"), lines.join("\n")).unwrap();
    (
        Workspace::new(root, true, false).unwrap(),
        ToolHarness::new(4).unwrap(),
    )
}

fn anchor_pack(harness: &ToolHarness, workspace: &Workspace, query: &str) -> Value {
    harness
        .agent_context("demo", workspace, query, 0, &[])
        .unwrap()
}

#[test]
fn diagnostic_anchor_selects_containing_symbol_and_exact_source_line() {
    let root = tempfile::tempdir().unwrap();
    let (workspace, harness) = anchor_fixture(root.path());
    for budget in [1_000, 1_400, 4_000] {
        let pack = harness
            .agent_context(
                "demo",
                &workspace,
                "error[E0308] at src/runtime/worker.rs:120:9",
                budget,
                &["runtime".into()],
            )
            .unwrap();
        assert_eq!(pack["targets"][0]["qualified_name"], "anchor_worker");
        assert_eq!(pack["files"][0]["path"], "src/runtime/worker.rs");
        assert_eq!(pack["hot_source"][0]["body"]["start_line"], 120);
        assert!(pack["hot_source"][0]["body"]["content"]
            .as_str()
            .unwrap()
            .contains("diagnostic_marker"));
        assert_eq!(pack["hot_source"][0]["sha256"], pack["files"][0]["sha256"]);
        assert_eq!(pack["retrieval"]["resolved"], 1);
        assert!(serde_json::to_vec(&pack).unwrap().len().div_ceil(4) <= budget);
    }
}

#[test]
fn explicit_file_context_supports_non_code_files() {
    let root = tempfile::tempdir().unwrap();
    let (workspace, harness) = anchor_fixture(root.path());
    fs::write(
        root.path().join("settings.json"),
        "{\n  \"answer\": 42\n}\n",
    )
    .unwrap();
    let pack = anchor_pack(&harness, &workspace, "inspect settings.json:2");
    assert_eq!(pack["targets"][0]["kind"], "file");
    assert_eq!(pack["targets"][0]["precision"], "deterministic");
    assert_eq!(pack["hot_source"][0]["body"]["start_line"], 2);
    assert!(pack["hot_source"][0]["body"]["content"]
        .as_str()
        .unwrap()
        .contains("answer"));
    assert_eq!(pack["readiness"]["edit"], "ready");
}

#[test]
fn missing_anchors_do_not_promote_unrelated_symbols() {
    let root = tempfile::tempdir().unwrap();
    let (workspace, harness) = anchor_fixture(root.path());
    let pack = anchor_pack(
        &harness,
        &workspace,
        "anchor_worker fails at src/missing.rs:4",
    );
    assert!(pack["targets"].as_array().unwrap().is_empty());
    assert!(pack["hot_source"].as_array().unwrap().is_empty());
    assert_eq!(pack["retrieval"]["resolved"], 0);
    assert_eq!(pack["readiness"]["edit"], "needs_target");
}

#[test]
fn unsafe_and_invalid_anchors_never_return_source() {
    let root = tempfile::tempdir().unwrap();
    let (workspace, harness) = anchor_fixture(root.path());
    fs::write(root.path().join(".env"), "private_sentinel=secret\n").unwrap();
    for query in [
        "../outside.rs:1",
        ".env:1",
        "src/runtime/worker.rs:0",
        "src/runtime/worker.rs:999999999999999999999999999999999999",
        "src/runtime/worker.rs:900",
        "C:/outside/worker.rs:1",
    ] {
        let pack = anchor_pack(&harness, &workspace, query);
        assert_eq!(pack["retrieval"]["resolved"], 0, "{query}");
        assert!(pack["hot_source"].as_array().unwrap().is_empty(), "{query}");
        assert!(!serde_json::to_string(&pack)
            .unwrap()
            .contains("private_sentinel"));
    }
}

#[test]
fn portable_and_markdown_anchor_forms_deduplicate() {
    let root = tempfile::tempdir().unwrap();
    let (workspace, harness) = anchor_fixture(root.path());
    let pack = anchor_pack(
        &harness,
        &workspace,
        "inspect `src\\runtime\\worker.rs:120:9` (src/runtime/worker.rs#L120)",
    );
    assert_eq!(pack["retrieval"]["resolved"], 1);
    assert_eq!(pack["hot_source"].as_array().unwrap().len(), 1);
    assert_eq!(pack["files"][0]["path"], "src/runtime/worker.rs");
}

#[test]
fn anchored_source_uses_fresh_sha_after_file_changes() {
    let root = tempfile::tempdir().unwrap();
    let (workspace, harness) = anchor_fixture(root.path());
    let first = anchor_pack(&harness, &workspace, "src/runtime/worker.rs:120");
    let path = root.path().join("src/runtime/worker.rs");
    let source = fs::read_to_string(&path)
        .unwrap()
        .replace("diagnostic_marker", "new_diagnostic_marker");
    fs::write(path, source).unwrap();
    let second = anchor_pack(&harness, &workspace, "src/runtime/worker.rs:120");
    assert_ne!(first["files"][0]["sha256"], second["files"][0]["sha256"]);
    assert_eq!(
        second["files"][0]["sha256"],
        second["hot_source"][0]["sha256"]
    );
    assert!(second["hot_source"][0]["body"]["content"]
        .as_str()
        .unwrap()
        .contains("new_diagnostic_marker"));
}

#[test]
fn prose_and_version_numbers_do_not_trigger_anchor_mode() {
    let root = tempfile::tempdir().unwrap();
    let (workspace, harness) = anchor_fixture(root.path());
    let pack = anchor_pack(
        &harness,
        &workspace,
        "inspect anchor_worker for version 0.6.1.",
    );
    assert!(pack.get("retrieval").is_none());
    assert_eq!(pack["targets"][0]["qualified_name"], "anchor_worker");
}

#[test]
fn absolute_anchors_must_stay_inside_the_selected_workspace() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let (workspace, harness) = anchor_fixture(root.path());
    let inside = workspace.root().join("src/runtime/worker.rs");
    let pack = anchor_pack(&harness, &workspace, &format!("{}:120", inside.display()));
    assert_eq!(pack["retrieval"]["resolved"], 1);
    fs::write(
        outside.path().join("external.rs"),
        "pub fn never_expose_sentinel() {}\n",
    )
    .unwrap();
    let query = format!("{}:1", outside.path().join("external.rs").display());
    let pack = anchor_pack(&harness, &workspace, &query);
    assert_eq!(pack["retrieval"]["resolved"], 0);
    assert!(!serde_json::to_string(&pack)
        .unwrap()
        .contains("never_expose_sentinel"));
}

#[cfg(unix)]
#[test]
fn anchor_symlinks_remain_blocked() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let (workspace, harness) = anchor_fixture(root.path());
    fs::write(
        outside.path().join("outside.rs"),
        "pub fn no_leak_sentinel() {}\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("outside.rs"),
        root.path().join("linked.rs"),
    )
    .unwrap();
    let pack = anchor_pack(&harness, &workspace, "linked.rs:1");
    assert_eq!(pack["retrieval"]["resolved"], 0);
    assert!(!serde_json::to_string(&pack)
        .unwrap()
        .contains("no_leak_sentinel"));
}
