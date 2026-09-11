use super::*;

fn selective_fixture(root: &Path) -> (Workspace, ToolHarness) {
    fs::create_dir_all(root.join("src/target")).unwrap();
    fs::create_dir_all(root.join("src/noise")).unwrap();
    fs::write(
        root.join("src/target/worker.rs"),
        "pub fn worker() {\n    let diagnostic_marker = 42;\n    let _ = diagnostic_marker;\n}\n",
    )
    .unwrap();
    for index in 0..128 {
        fs::write(
            root.join(format!("src/noise/irrelevant_{index}.rs")),
            format!("pub fn irrelevant_{index}() {{}}\n"),
        )
        .unwrap();
    }
    (
        Workspace::new(root, true, false).unwrap(),
        ToolHarness::new(4).unwrap(),
    )
}

#[test]
fn selective_context_avoids_unrelated_indexing() {
    let root = tempfile::tempdir().unwrap();
    let (workspace, harness) = selective_fixture(root.path());
    for pass in ["cold", "warm"] {
        let started = Instant::now();
        let pack = harness
            .agent_context("demo", &workspace, "src/target/worker.rs:2", 4_000, &[])
            .unwrap();
        let elapsed_us = started.elapsed().as_micros();
        let outline = harness
            .file_outline("demo", &workspace, "src/target/worker.rs", 100)
            .unwrap();
        let indexed = outline["index"]["indexed_files"].as_u64().unwrap();
        eprintln!("selective_context pass={pass} unrelated_files=128 indexed_files={indexed} elapsed_us={elapsed_us}");
        assert_eq!(pack["targets"][0]["qualified_name"], "worker");
        assert!(pack["hot_source"][0]["body"]["content"]
            .as_str()
            .unwrap()
            .contains("diagnostic_marker"));
        assert_eq!(pack["files"][0]["sha256"], pack["hot_source"][0]["sha256"]);
        assert_eq!(
            indexed, 1,
            "an exact location should not index unrelated source"
        );
        assert_eq!(pack["repo_map"]["deferred"], true);
    }
}

#[test]
fn selective_context_retains_relationship_and_scope_discovery() {
    let root = tempfile::tempdir().unwrap();
    let (workspace, harness) = selective_fixture(root.path());
    let pack = harness
        .agent_context(
            "demo",
            &workspace,
            "inspect callers of src/target/worker.rs:2",
            4_000,
            &[],
        )
        .unwrap();
    assert_ne!(pack["repo_map"]["deferred"], true);
    assert_eq!(pack["targets"][0]["qualified_name"], "worker");
    let ordinary = harness
        .agent_context("demo", &workspace, "irrelevant_42", 4_000, &[])
        .unwrap();
    assert!(ordinary["targets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|target| target["qualified_name"] == "irrelevant_42"));
}

#[test]
fn selective_context_keeps_guidance_and_design_verification() {
    let root = tempfile::tempdir().unwrap();
    let (workspace, harness) = selective_fixture(root.path());
    fs::write(
        root.path().join("AGENTS.md"),
        "Preserve the guidance_sentinel contract.\n",
    )
    .unwrap();
    fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    fs::write(
        root.path().join(".wcode/project.yaml"),
        "schema_version: 1\nname: demo\n",
    )
    .unwrap();
    fs::write(root.path().join(".wcode/design/requirements.yaml"), "- schema_version: 1\n  id: REQ-WORKER\n  title: worker\n  intent: Inspect src/target/worker.rs behavior.\n  priority: high\n  implemented_by: [component:worker]\n  acceptance: [AC-WORKER]\n  constraints: []\n  risk: {}\n").unwrap();
    fs::write(root.path().join(".wcode/design/components.yaml"), "- schema_version: 1\n  id: component:worker\n  name: worker\n  responsibilities: [worker]\n  depends_on: []\n  constraints: []\n  implementation:\n    - kind: symbol\n      path: src/target/worker.rs\n      symbol: worker\n").unwrap();
    fs::write(root.path().join(".wcode/design/acceptance.yaml"), "- schema_version: 1\n  id: AC-WORKER\n  title: worker\n  statement: worker obeys its contract.\n  verification:\n    - kind: test\n      path: src/target/worker.rs\n      symbol: worker\n").unwrap();
    let pack = harness
        .agent_context("demo", &workspace, "src/target/worker.rs:2", 8_000, &[])
        .unwrap();
    assert!(pack["guidance"].to_string().contains("guidance_sentinel"));
    assert!(pack["design"].to_string().contains("REQ-WORKER"));
    assert!(pack["tests"]
        .to_string()
        .contains("src/target/worker.rs::worker"));
    assert_eq!(pack["readiness"]["verify"], "ready");
}

#[test]
fn selective_missing_location_does_not_trigger_global_search() {
    let root = tempfile::tempdir().unwrap();
    let (workspace, harness) = selective_fixture(root.path());
    let pack = harness
        .agent_context("demo", &workspace, "src/missing.rs:2", 4_000, &[])
        .unwrap();
    assert!(pack["targets"].as_array().unwrap().is_empty());
    assert!(pack["hot_source"].as_array().unwrap().is_empty());
    assert_eq!(pack["retrieval"]["resolved"], 0);
    let outline = harness
        .file_outline("demo", &workspace, "src/target/worker.rs", 100)
        .unwrap();
    assert_eq!(outline["index"]["indexed_files"], 1);
}

#[test]
fn selective_context_preserves_multilanguage_originals_and_budgets() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    for (path, source, line) in [
        ("worker.py", "def worker():\n    return 42\n", 2),
        (
            "worker.ts",
            "export function worker() {\n  return 42;\n}\n",
            2,
        ),
        (
            "worker.go",
            "package fixture\nfunc worker() int {\n  return 42\n}\n",
            3,
        ),
        ("settings.json", "{\n  \"value\": 42\n}\n", 2),
    ] {
        fs::write(root.path().join(path), source).unwrap();
        for budget in [1_000, 4_000] {
            let pack = harness
                .agent_context("demo", &workspace, &format!("{path}:{line}"), budget, &[])
                .unwrap();
            assert_eq!(pack["files"][0]["path"], path);
            assert_eq!(pack["hot_source"][0]["body"]["start_line"], line);
            assert!(pack["hot_source"][0]["body"]["content"]
                .as_str()
                .unwrap()
                .contains("42"));
            assert_eq!(pack["hot_source"][0]["sha256"], pack["files"][0]["sha256"]);
            assert!(serde_json::to_vec(&pack).unwrap().len().div_ceil(4) <= budget);
            assert_eq!(pack["repo_map"]["deferred"], true);
        }
    }
}

#[tokio::test]
async fn oversized_verification_plan_fails_before_dispatch() {
    let root = tempfile::tempdir().unwrap();
    let workspaces = crate::workspace::Workspaces::new([root.path()], false, true).unwrap();
    workspaces.revoke_command(None, "git").unwrap();
    let (id, workspace) = workspaces.select(None).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    let (profile, _) = harness.load_project_profile(&workspace).unwrap();
    let mut oversized = (*profile).clone();
    oversized.recommended_checks = (0..=MAX_VERIFICATION_CHECKS)
        .map(|index| CheckSpec {
            id: format!("fixture-{index}"),
            level: "full".to_owned(),
            phase: 0,
            program: "git".to_owned(),
            args: vec!["diff".to_owned(), "--check".to_owned()],
            reason: "over-capacity fixture".to_owned(),
        })
        .collect();
    harness
        .project_cache
        .lock()
        .unwrap()
        .get_mut(workspace.root())
        .unwrap()
        .profile = Arc::new(oversized);
    let monitor = TaskMonitor::new([id.clone()]);
    let error = harness
        .verify_project_mode(id, &workspace, ("full", false), 1, &monitor)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no checks executed"));
    assert!(workspaces.authorization_requests(10).is_empty());
    assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
}

#[tokio::test]
async fn mixed_language_verification_accounts_for_every_inferred_check() {
    let root = tempfile::tempdir().unwrap();
    for (path, content) in [
        ("Cargo.toml", "[package]\nname='fixture'\nversion='0.1.0'\n"),
        ("pyproject.toml", "[project]\nname='fixture'\n"),
        ("go.mod", "module fixture\n"),
        ("Makefile", "check:\nlint:\ntest:\n"),
        ("package.json", "{\"scripts\":{\"lint\":\"node --version\",\"typecheck\":\"node --version\",\"check\":\"node --version\",\"format:check\":\"node --version\",\"test\":\"node --version\",\"build\":\"node --version\"}}"),
    ] {
        fs::write(root.path().join(path), content).unwrap();
    }
    let workspaces = crate::workspace::Workspaces::new([root.path()], false, true).unwrap();
    // Exercise the real scheduler without launching any external program.
    for program in ["cargo", "npm", "pytest", "go", "make"] {
        workspaces.revoke_command(None, program).unwrap();
    }
    let (id, workspace) = workspaces.select(None).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    let (profile, _) = harness.load_project_profile(&workspace).unwrap();
    let expected = profile
        .recommended_checks
        .iter()
        .map(|check| check.id.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(expected.len(), 16);
    let monitor = TaskMonitor::new([id.clone()]);
    let report = harness
        .verify_project_mode(id, &workspace, ("full", false), 1, &monitor)
        .await
        .unwrap();
    let actual = report
        .checks
        .iter()
        .map(|check| check.id.clone())
        .collect::<BTreeSet<_>>();
    eprintln!(
        "mixed_verification inferred={} accounted={}",
        expected.len(),
        actual.len()
    );
    assert_eq!(
        actual, expected,
        "full diagnostics must not silently drop inferred checks"
    );
    assert!(!report.passed);
    assert!(report.skipped_checks.is_empty());
}
