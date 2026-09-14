use super::*;
use std::fs;

#[test]
fn agent_context_keeps_direct_sha_ahead_of_alphabetical_design_paths() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join(".wcode/project.yaml"),
        "schema_version: 1\nname: demo\n",
    )
    .unwrap();
    fs::write(root.path().join(".wcode/design/requirements.yaml"), "- schema_version: 1\n  id: REQ-TARGET-001\n  title: target_feature\n  intent: target_feature\n  priority: high\n  implemented_by: [component:target]\n  acceptance: []\n  constraints: []\n  risk: {}\n").unwrap();
    let mut references = String::new();
    for index in 0..14 {
        let path = format!("src/a{index:02}.rs");
        fs::write(
            root.path().join(&path),
            format!("pub fn unrelated_{index}() {{}}\n"),
        )
        .unwrap();
        references.push_str(&format!("    - kind: file\n      path: {path}\n"));
    }
    fs::write(
        root.path().join("src/z_target.rs"),
        "pub fn target_feature() {}\n",
    )
    .unwrap();
    references.push_str(
        "    - kind: symbol\n      path: src/z_target.rs\n      symbol: target_feature\n",
    );
    fs::write(root.path().join(".wcode/design/components.yaml"), format!("- schema_version: 1\n  id: component:target\n  name: target_feature\n  responsibilities: [target_feature]\n  depends_on: []\n  constraints: []\n  implementation:\n{references}")).unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(8).unwrap();
    let pack = harness
        .agent_context("demo", &workspace, "target_feature", 2_000, &[])
        .unwrap();
    assert_eq!(pack["files"][0]["path"], "src/z_target.rs");
    assert_eq!(pack["files"][0]["sha256"].as_str().unwrap().len(), 64);
    assert!(pack["readiness"]["editable_sha_targets"].as_u64().unwrap() > 0);
}

#[test]
fn agent_context_parallel_discovery_respects_the_runtime_slot_cap() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src/runtime")).unwrap();
    fs::create_dir_all(root.path().join("src/workspace")).unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    for cap in [1, 8] {
        let harness = ToolHarness::new(cap).unwrap();
        let pack = harness
            .agent_context(
                "demo",
                &workspace,
                "unmatched_target",
                0,
                &["runtime".to_owned(), "workspace".to_owned()],
            )
            .unwrap();
        assert_eq!(pack["readiness"]["parallelism"]["candidate_lanes"], 2);
        assert_eq!(pack["readiness"]["parallelism"]["required"], true);
        assert_eq!(
            pack["readiness"]["parallelism"]["recommended_concurrency"],
            cap.min(2)
        );
    }
}
