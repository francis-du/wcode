use super::*;
use crate::design::{
    AcceptanceCriterion, CodeRef, ComponentDesign, DesignState, Requirement, VerificationRef,
};
use std::collections::BTreeSet;

#[test]
fn focused_test_mapping_requires_direct_design_ownership_and_same_rust_island() {
    let mut state = DesignState::default();
    state.components.insert(
        "component:engine".into(),
        ComponentDesign {
            schema_version: 1,
            id: "component:engine".into(),
            name: "Engine".into(),
            responsibilities: vec![],
            depends_on: vec![],
            constraints: vec![],
            implementation: vec![CodeRef::Symbol {
                path: "src/engine.rs".into(),
                symbol: "run".into(),
            }],
        },
    );
    state.requirements.insert(
        "REQ-ENGINE-001".into(),
        Requirement {
            schema_version: 1,
            id: "REQ-ENGINE-001".into(),
            title: "Engine".into(),
            intent: "Run engine".into(),
            priority: crate::design::Priority::Critical,
            implemented_by: vec!["component:engine".into()],
            acceptance: vec!["AC-ENGINE-001".into()],
            constraints: vec![],
            risk: Default::default(),
        },
    );
    state.acceptance.insert(
        "AC-ENGINE-001".into(),
        AcceptanceCriterion {
            schema_version: 1,
            id: "AC-ENGINE-001".into(),
            title: "Engine test".into(),
            statement: "Engine behavior is tested".into(),
            verification: vec![VerificationRef::Test {
                path: "tests/engine.rs".into(),
                symbol: "engine_runs".into(),
            }],
        },
    );
    let profile = focus_profile();

    let direct = BTreeSet::from(["src/engine.rs".to_owned()]);
    let candidates = harness_test_focus::declared_test_candidates(&state, &profile, &direct);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].test_symbol, "engine_runs");
    assert_eq!(candidates[0].island, ".");

    let unrelated = BTreeSet::from(["src/other.rs".to_owned()]);
    assert!(harness_test_focus::declared_test_candidates(&state, &profile, &unrelated).is_empty());

    state
        .acceptance
        .get_mut("AC-ENGINE-001")
        .unwrap()
        .verification = vec![VerificationRef::Test {
        path: "tests/engine.rs".into(),
        symbol: "engine_runs::not_a_simple_filter".into(),
    }];
    assert!(harness_test_focus::declared_test_candidates(&state, &profile, &direct).is_empty());
}

#[test]
fn focused_quick_test_requires_a_resolved_declared_test_symbol() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::create_dir_all(root.path().join("tests")).unwrap();
    fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(root.path().join("Cargo.lock"), "# fixture lock\n").unwrap();
    fs::write(
        root.path().join("src/engine.rs"),
        "pub fn run() -> u8 { 1 }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("tests/engine.rs"),
        "#[test]\nfn engine_runs() { assert_eq!(1, 1); }\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".wcode/project.yaml"),
        "schema_version: 1\nname: demo\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".wcode/design/requirements.yaml"),
        "- schema_version: 1\n  id: REQ-ENGINE-001\n  title: Engine\n  intent: Run engine\n  priority: critical\n  implemented_by: [component:engine]\n  acceptance: [AC-ENGINE-001]\n  constraints: []\n  risk: {}\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".wcode/design/components.yaml"),
        "- schema_version: 1\n  id: component:engine\n  name: Engine\n  responsibilities: [run engine]\n  depends_on: []\n  constraints: []\n  implementation:\n    - kind: symbol\n      path: src/engine.rs\n      symbol: run\n",
    )
    .unwrap();
    let acceptance = root.path().join(".wcode/design/acceptance.yaml");
    fs::write(
        &acceptance,
        "- schema_version: 1\n  id: AC-ENGINE-001\n  title: Engine test\n  statement: Engine runs\n  verification:\n    - kind: test\n      path: tests/engine.rs\n      symbol: engine_runs\n",
    )
    .unwrap();

    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let (profile, _) = harness.load_project_profile(&workspace).unwrap();
    let snapshot = json!({
        "available": true,
        "truncated": false,
        "files": [{"path": "src/engine.rs", "status": "modified"}],
    });
    let focused = harness_test_focus::focused_quick_test(
        &harness,
        "demo",
        &workspace,
        &profile,
        Some(&snapshot),
    )
    .expect("resolved Design-declared Rust test should be prioritized");
    assert_eq!(focused.phase, 1);
    assert_eq!(focused.args, vec!["test", "--locked", "engine_runs"]);
    assert!(focused.reason.contains("REQ-ENGINE-001"));
    assert!(focused.reason.contains("AC-ENGINE-001"));
    assert!(focused.reason.contains("declared+syntax"));

    fs::write(
        &acceptance,
        "- schema_version: 1\n  id: AC-ENGINE-001\n  title: Engine test\n  statement: Engine runs\n  verification:\n    - kind: test\n      path: tests/engine.rs\n      symbol: stale_test_name\n",
    )
    .unwrap();
    assert!(harness_test_focus::focused_quick_test(
        &harness,
        "demo",
        &workspace,
        &profile,
        Some(&snapshot),
    )
    .is_none());
}

#[test]
fn focused_python_and_go_tests_compile_exact_island_scoped_commands() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("py/tests")).unwrap();
    fs::create_dir_all(root.path().join("go/pkg")).unwrap();
    fs::write(root.path().join("py/app.py"), "def run():\n    return 1\n").unwrap();
    fs::write(
        root.path().join("py/tests/test_app.py"),
        "def test_run():\n    assert True\n",
    )
    .unwrap();
    fs::write(
        root.path().join("go/pkg/app.go"),
        "package pkg\n\nfunc Run() int { return 1 }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("go/pkg/app_test.go"),
        "package pkg\n\nimport \"testing\"\n\nfunc TestRun(t *testing.T) {}\n",
    )
    .unwrap();

    let mut state = DesignState::default();
    state.components.insert(
        "component:python-engine".into(),
        ComponentDesign {
            schema_version: 1,
            id: "component:python-engine".into(),
            name: "Python engine".into(),
            responsibilities: vec![],
            depends_on: vec![],
            constraints: vec![],
            implementation: vec![CodeRef::Symbol {
                path: "py/app.py".into(),
                symbol: "run".into(),
            }],
        },
    );
    state.requirements.insert(
        "REQ-PY-001".into(),
        Requirement {
            schema_version: 1,
            id: "REQ-PY-001".into(),
            title: "Python engine".into(),
            intent: "Run Python engine".into(),
            priority: crate::design::Priority::Critical,
            implemented_by: vec!["component:python-engine".into()],
            acceptance: vec!["AC-PY-001".into()],
            constraints: vec![],
            risk: Default::default(),
        },
    );
    state.acceptance.insert(
        "AC-PY-001".into(),
        AcceptanceCriterion {
            schema_version: 1,
            id: "AC-PY-001".into(),
            title: "Python test".into(),
            statement: "Python behavior is tested".into(),
            verification: vec![VerificationRef::Test {
                path: "py/tests/test_app.py".into(),
                symbol: "test_run".into(),
            }],
        },
    );
    state.components.insert(
        "component:go-engine".into(),
        ComponentDesign {
            schema_version: 1,
            id: "component:go-engine".into(),
            name: "Go engine".into(),
            responsibilities: vec![],
            depends_on: vec![],
            constraints: vec![],
            implementation: vec![CodeRef::Symbol {
                path: "go/pkg/app.go".into(),
                symbol: "Run".into(),
            }],
        },
    );
    state.requirements.insert(
        "REQ-GO-001".into(),
        Requirement {
            schema_version: 1,
            id: "REQ-GO-001".into(),
            title: "Go engine".into(),
            intent: "Run Go engine".into(),
            priority: crate::design::Priority::Critical,
            implemented_by: vec!["component:go-engine".into()],
            acceptance: vec!["AC-GO-001".into()],
            constraints: vec![],
            risk: Default::default(),
        },
    );
    state.acceptance.insert(
        "AC-GO-001".into(),
        AcceptanceCriterion {
            schema_version: 1,
            id: "AC-GO-001".into(),
            title: "Go test".into(),
            statement: "Go behavior is tested".into(),
            verification: vec![VerificationRef::Test {
                path: "go/pkg/app_test.go".into(),
                symbol: "TestRun".into(),
            }],
        },
    );

    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let profile = polyglot_focus_profile();
    let python_snapshot = json!({
        "available": true,
        "truncated": false,
        "files": [{"path": "py/app.py", "status": "modified"}],
    });
    let python = harness_test_focus::focused_quick_test_from_design(
        &harness,
        "polyglot-focus",
        &workspace,
        &profile,
        Some(&python_snapshot),
        &state,
    )
    .expect("resolved Python Design test should compile to one focused check");
    assert_eq!(python.id, "focused-python-tests");
    assert_eq!(python.program, "pytest");
    assert_eq!(python.cwd, "py");
    assert_eq!(python.args, vec!["-q", "tests/test_app.py::test_run"]);

    let go_snapshot = json!({
        "available": true,
        "truncated": false,
        "files": [{"path": "go/pkg/app.go", "status": "modified"}],
    });
    let go = harness_test_focus::focused_quick_test_from_design(
        &harness,
        "polyglot-focus",
        &workspace,
        &profile,
        Some(&go_snapshot),
        &state,
    )
    .expect("resolved Go Design test should compile to one focused check");
    assert_eq!(go.id, "focused-go-tests");
    assert_eq!(go.program, "go");
    assert_eq!(go.cwd, "go");
    assert_eq!(go.args, vec!["test", "./pkg", "-run", "^TestRun$"]);
}

#[tokio::test]
async fn quick_verification_prioritizes_one_resolved_design_test_after_static_checks() {
    use std::process::Command;

    let root = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(root.path())
            .status()
            .expect("git must be available for focused verification fixture")
    };
    assert!(git(&["init", "-q"]).success());
    assert!(git(&["config", "user.email", "wcode@example.test"]).success());
    assert!(git(&["config", "user.name", "wcode test"]).success());
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::create_dir_all(root.path().join("tests")).unwrap();
    fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub mod engine;\n").unwrap();
    fs::write(
        root.path().join("src/engine.rs"),
        "pub fn run() -> u8 {\n    1\n}\n",
    )
    .unwrap();
    fs::write(
        root.path().join("tests/engine.rs"),
        "#[test]\nfn engine_runs() {\n    assert_eq!(demo::engine::run(), 1);\n}\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".wcode/project.yaml"),
        "schema_version: 1\nname: demo\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".wcode/design/requirements.yaml"),
        "- schema_version: 1\n  id: REQ-ENGINE-001\n  title: Engine\n  intent: Run engine\n  priority: critical\n  implemented_by: [component:engine]\n  acceptance: [AC-ENGINE-001]\n  constraints: []\n  risk: {}\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".wcode/design/components.yaml"),
        "- schema_version: 1\n  id: component:engine\n  name: Engine\n  responsibilities: [run engine]\n  depends_on: []\n  constraints: []\n  implementation:\n    - kind: symbol\n      path: src/engine.rs\n      symbol: run\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".wcode/design/acceptance.yaml"),
        "- schema_version: 1\n  id: AC-ENGINE-001\n  title: Engine test\n  statement: Engine runs\n  verification:\n    - kind: test\n      path: tests/engine.rs\n      symbol: engine_runs\n",
    )
    .unwrap();
    let cargo_status = Command::new("cargo")
        .arg("generate-lockfile")
        .current_dir(root.path())
        .status()
        .unwrap();
    assert!(cargo_status.success());
    assert!(git(&["add", "."]).success());
    assert!(git(&["-c", "commit.gpgsign=false", "commit", "-qm", "initial"]).success());
    fs::write(
        root.path().join("src/engine.rs"),
        "pub fn run() -> u8 {\n    2 - 1\n}\n",
    )
    .unwrap();

    let workspace = Workspace::new(root.path(), false, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let workspace_id = "focused-quick".to_owned();
    let monitor = TaskMonitor::new([workspace_id.clone()]);
    let report = harness
        .verify_project(workspace_id.clone(), &workspace, "quick", 120, &monitor)
        .await
        .unwrap();
    assert!(
        report.passed,
        "focused quick verification failed: {}",
        report.summary
    );
    let focused = report
        .checks
        .iter()
        .find(|check| check.id == "focused-rust-test")
        .expect("quick verification should include the mapped focused test");
    assert_eq!(focused.phase, 1);
    assert!(focused.command.ends_with("cargo test --locked engine_runs"));
    assert!(focused.reason.contains("REQ-ENGINE-001"));
    assert!(focused.reason.contains("declared+syntax"));
    assert!(report
        .checks
        .iter()
        .any(|check| check.id == "rust-check" && check.phase == 0));

    let review = harness
        .review_changes(workspace_id.clone(), &workspace, 30, &monitor)
        .await
        .unwrap();
    let project = harness
        .project_observatory(workspace_id, &workspace, Some(&review))
        .unwrap();
    assert_eq!(project.adaptive_verification.mode, "focused_test");
    assert!(project.adaptive_verification.full_coverage_unchanged);
    let preview = project
        .adaptive_verification
        .focused_test
        .as_ref()
        .expect("Observatory should preview the same focused test without executing it");
    assert_eq!(preview.check_id, "focused-rust-test");
    assert!(preview.command.ends_with("cargo test --locked engine_runs"));
    assert_eq!(preview.provider, "design-traceability");
    assert_eq!(preview.precision, "declared+syntax");
}

fn polyglot_focus_profile() -> ProjectProfile {
    let python = CheckSpec {
        id: "python-tests".into(),
        level: "full".into(),
        phase: 1,
        program: "pytest".into(),
        args: vec!["-q".into()],
        cwd: "py".into(),
        island: "py".into(),
        languages: vec!["python".into()],
        reason: "Run Python tests".into(),
    };
    let go = CheckSpec {
        id: "go-tests".into(),
        level: "full".into(),
        phase: 1,
        program: "go".into(),
        args: vec!["test".into(), "./...".into()],
        cwd: "go".into(),
        island: "go".into(),
        languages: vec!["go".into()],
        reason: "Run Go tests".into(),
    };
    let island = |id: &str, project_type: &str, manifest: &str, check_id: &str| ProjectIsland {
        id: id.into(),
        root: id.into(),
        project_types: vec![project_type.into()],
        languages: vec![project_type.into()],
        manifests: vec![manifest.into()],
        check_ids: vec![check_id.into()],
        dependencies: vec![],
        verification_status: "native",
        verification_gaps: vec![],
        provider: "manifest-discovery",
        precision: "structural",
    };
    ProjectProfile {
        root: ".".into(),
        project_types: vec!["python".into(), "go".into()],
        manifests: vec!["py/pyproject.toml".into(), "go/go.mod".into()],
        islands: vec![
            island("py", "python", "pyproject.toml", "python-tests"),
            island("go", "go", "go.mod", "go-tests"),
        ],
        contracts: ProjectContractTopology {
            bridges: vec![],
            diagnostics: vec![],
            truncated: false,
            provider: "contract-config",
            precision: "structural",
        },
        guidance: vec![],
        recommended_checks: vec![python, go],
        workflow: vec![],
        write_enabled: true,
        exec_enabled: true,
    }
}

fn focus_profile() -> ProjectProfile {
    let test = CheckSpec {
        id: "rust-test".into(),
        level: "full".into(),
        phase: 1,
        program: "cargo".into(),
        args: vec!["test".into(), "--locked".into()],
        cwd: ".".into(),
        island: ".".into(),
        languages: vec!["rust".into()],
        reason: "Run Rust tests".into(),
    };
    ProjectProfile {
        root: ".".into(),
        project_types: vec!["rust".into()],
        manifests: vec!["Cargo.toml".into()],
        islands: vec![ProjectIsland {
            id: ".".into(),
            root: ".".into(),
            project_types: vec!["rust".into()],
            languages: vec!["rust".into()],
            manifests: vec!["Cargo.toml".into()],
            check_ids: vec!["rust-test".into()],
            dependencies: vec![],
            verification_status: "native",
            verification_gaps: vec![],
            provider: "manifest-discovery",
            precision: "structural",
        }],
        contracts: ProjectContractTopology {
            bridges: vec![],
            diagnostics: vec![],
            truncated: false,
            provider: "contract-config",
            precision: "structural",
        },
        guidance: vec![],
        recommended_checks: vec![test],
        workflow: vec![],
        write_enabled: true,
        exec_enabled: true,
    }
}
