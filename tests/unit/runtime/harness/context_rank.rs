use super::*;

#[test]
fn exact_symbol_outranks_helpers_across_scope_roots() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src/app")).unwrap();
    fs::create_dir_all(root.path().join("src/runtime")).unwrap();
    let helpers = (0..24)
        .map(|index| format!("pub fn parallel_tools_helper_{index}() {{}}\n"))
        .collect::<String>();
    fs::write(root.path().join("src/app/helpers.rs"), helpers).unwrap();
    fs::write(
        root.path().join("src/runtime/target.rs"),
        "pub fn parallel_tools() -> usize { 7 }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    for _ in 0..2 {
        let pack = harness
            .agent_context("demo", &workspace, "parallel_tools", 0, &["runtime".into()])
            .unwrap();
        assert_eq!(pack["targets"][0]["qualified_name"], "parallel_tools");
        assert_eq!(pack["hot_source"][0]["qualified_name"], "parallel_tools");
        assert_eq!(pack["files"][0]["path"], "src/runtime/target.rs");
        assert!(pack["hot_source"][0]["body"]["content"]
            .as_str()
            .unwrap()
            .contains("7"));
    }
}

#[test]
fn qualified_symbol_in_prose_is_kept_whole() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn cancellation() {}\npub fn parallel_item_error() {}\npub struct Worker;\nimpl Worker { pub fn parallel_tools(&self) {} }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let pack = harness
        .agent_context(
            "demo",
            &workspace,
            "inspect Worker::parallel_tools cancellation",
            0,
            &[],
        )
        .unwrap();
    assert_eq!(
        pack["targets"][0]["qualified_name"],
        "Worker::parallel_tools"
    );
    assert_eq!(
        pack["hot_source"][0]["qualified_name"],
        "Worker::parallel_tools"
    );
}
