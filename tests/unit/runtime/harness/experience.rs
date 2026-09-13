use super::*;

#[tokio::test]
async fn hard_repository_conventions_fail_before_project_commands_run() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"core-policy-demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("src/large_module.rs"),
        "pub fn value() {}\n".repeat(1_001),
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), false, true).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    let monitor = TaskMonitor::new(["core-policy-demo".to_owned()]);

    let report = harness
        .verify_project("core-policy-demo", &workspace, "quick", 30, &monitor)
        .await
        .unwrap();

    assert!(!report.passed);
    assert_eq!(report.execution, "core-policy");
    assert_eq!(report.checks_run, 1);
    assert_eq!(report.checks_failed, 1);
    assert_eq!(report.checks[0].id, "core-policy");
    assert!(report.checks[0]
        .stdout_tail
        .contains("oversized-source-module"));
    assert!(!report.skipped_checks.is_empty());
}

#[tokio::test]
async fn failed_phase_skips_expensive_checks_but_diagnostic_mode_runs_them() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package\ninvalid manifest\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), false, true).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    let fast = harness
        .verify_project("demo", &workspace, "full", 30, &monitor)
        .await
        .unwrap();
    assert!(!fast.passed);
    assert_eq!(fast.phases_run, 1);
    assert_eq!(fast.checks_run, 2);
    assert_eq!(
        fast.skipped_checks,
        ["rust-test", "rust-clippy", "rust-release-build"]
    );
    let evidence = crate::evidence_store::load(&workspace).unwrap();
    let recorded_checks = evidence
        .iter()
        .filter(|item| item.subject.starts_with("verification:"))
        .collect::<Vec<_>>();
    assert!(!recorded_checks.is_empty());
    assert!(recorded_checks.iter().all(|item| item
        .summary
        .as_deref()
        .is_some_and(|summary| summary.starts_with("verification-metrics-v1;elapsed_ms="))));
    for skipped in &fast.skipped_checks {
        assert!(!evidence
            .iter()
            .any(|item| item.subject == format!("verification:{skipped}")));
    }
    let diagnostic = harness
        .verify_project_mode("demo", &workspace, ("full", false), 30, &monitor)
        .await
        .unwrap();
    assert!(!diagnostic.passed);
    assert_eq!(diagnostic.phases_run, 4);
    assert_eq!(diagnostic.checks_run, 5);
    assert!(diagnostic.skipped_checks.is_empty());
    assert_eq!(diagnostic.checks_run - fast.checks_run, 3);
}

#[test]
fn failed_verification_is_not_eligible_for_retrieval_experience() {
    let snapshot = json!({
        "available": true,
        "truncated": false,
        "files": [
            {"path": "src/b.rs"},
            {"path": "src/a.rs"},
            {"path": "src/a.rs"}
        ]
    });
    let report = |passed| VerificationReport {
        workspace: "demo".into(),
        level: "full".into(),
        execution: "phased-parallel".into(),
        phases_run: 1,
        passed,
        checks_run: 1,
        checks_reused: 0,
        checks_failed: usize::from(!passed),
        skipped_checks: Vec::new(),
        elapsed_ms: 1,
        summary: "fixture".into(),
        impact: None,
        cost_model: None,
        checks: Vec::new(),
    };

    assert!(harness_quality::verified_experience_paths(&report(false), Some(&snapshot)).is_none());
    assert_eq!(
        harness_quality::verified_experience_paths(&report(true), Some(&snapshot)).unwrap(),
        ["src/a.rs", "src/b.rs"]
    );
    let truncated = json!({"available": true, "truncated": true, "files": snapshot["files"]});
    assert!(harness_quality::verified_experience_paths(&report(true), Some(&truncated)).is_none());
}

#[tokio::test]
async fn migration_audit_blocks_a_green_behavioral_suite_until_migration_is_complete() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".wcode")).unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::create_dir_all(root.path().join("tests")).unwrap();
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname='migration_fixture'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("src/lib.rs"),
        "pub fn legacy() -> usize { 1 }\npub fn modern() -> usize { legacy() }\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("tests/behavior.rs"),
        "#[test]\nfn behavior_is_green() { assert_eq!(migration_fixture::modern(), 1); }\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join(".wcode/migration.yaml"),
        r#"schema_version: 1
id: remove-legacy
scopes: [src]
required_paths: [src/lib.rs]
transitions:
  - id: api
    legacy: "legacy()"
    replacement: "modern()"
"#,
    )
    .unwrap();

    let behavior = std::process::Command::new("cargo")
        .args(["test", "--quiet"])
        .current_dir(root.path())
        .status()
        .unwrap();
    assert!(behavior.success(), "fixture behavior should already pass");

    let workspace = Workspace::new(root.path(), false, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    let report = harness
        .verify_project("demo", &workspace, "full", 30, &monitor)
        .await
        .unwrap();
    assert!(!report.passed);
    assert_eq!(report.execution, "migration-audit");
    assert_eq!(report.checks_run, 1);
    assert_eq!(report.checks[0].id, "migration-audit");
    assert!(report.checks[0].stdout_tail.contains("legacy_remaining"));
    assert!(report.checks[0].stdout_tail.contains("mixed_state"));
    assert!(report.skipped_checks.iter().any(|id| id == "rust-test"));
    assert!(!report.checks.iter().any(|check| check.id == "rust-test"));

    let pack = harness
        .agent_context(
            "demo",
            &workspace,
            "complete the legacy API migration",
            4_000,
            &[],
        )
        .unwrap();
    assert_eq!(pack["migration_audit"]["configured"], true);
    assert_eq!(pack["migration_audit"]["valid"], true);
    assert_eq!(
        pack["migration_audit"]["gate_order"],
        "before_behavioral_verification"
    );
}

#[test]
fn compact_success_logs_preserve_test_totals_and_reduce_noise() {
    let mut log = "test fixture_case ... ok\n".repeat(500);
    log.push_str("test result: ok. 500 passed; 0 failed; 0 ignored\n");
    log.push_str(&"test another_case ... ok\n".repeat(200));
    log.push_str("test result: ok. 200 passed; 0 failed; 0 ignored\n");
    let (compact, cut) = harness_verification::verification_output(&log, true);
    assert!(cut);
    assert!(compact.contains("500 passed"));
    assert!(compact.contains("200 passed"));
    assert!(
        compact.len() * 5 < log.len(),
        "at least 80% less log text in this fixture"
    );
    let (failed, failed_cut) = harness_verification::verification_output(&log, false);
    assert_eq!(
        (failed, failed_cut),
        tail_chars(&log, MAX_CHECK_OUTPUT_CHARS)
    );
    for text in ["", "Finished without warnings", "中文检查完成\n"] {
        assert_eq!(
            harness_verification::verification_output(text, true),
            (text.to_owned(), false)
        );
    }
}

#[test]
fn audit_operator_context_respects_disabled_execution() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    for query in ["git status", "git commit", "全量检查"] {
        let pack = harness
            .agent_context("demo", &workspace, query, 0, &[])
            .unwrap();
        assert!(pack["readiness"]["next_actions"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(pack["readiness"]["advisories"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == "workspace_exec_disabled"));
    }
}

#[test]
fn operator_context_skips_irrelevant_source_and_preserves_coding_route() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("src/lib.rs"),
        "pub fn commit() { println!(\"source target\"); }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    for query in ["提交", "git commit", "git status", "全量检查"] {
        let pack = harness
            .agent_context("demo", &workspace, query, 0, &[])
            .unwrap();
        assert_eq!(pack["readiness"]["edit"], "not_applicable");
        assert!(pack["targets"].as_array().unwrap().is_empty());
        assert!(pack["hot_source"].as_array().unwrap().is_empty());
        assert!(serde_json::to_vec(&pack).unwrap().len() <= 4_000);
        assert_eq!(pack["context_bytes_avoided"], 0);
    }
    let code = harness
        .agent_context("demo", &workspace, "inspect commit", 4_000, &[])
        .unwrap();
    assert!(code["targets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["qualified_name"] == "commit"));
    assert_ne!(code["readiness"]["edit"], "not_applicable");
}
