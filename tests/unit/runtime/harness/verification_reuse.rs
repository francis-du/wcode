use super::*;

fn static_check(id: &str, args: &[&str]) -> CheckSpec {
    CheckSpec {
        id: id.to_owned(),
        level: "quick".to_owned(),
        phase: 0,
        program: "cargo".to_owned(),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        cwd: ".".to_owned(),
        island: ".".to_owned(),
        languages: vec!["rust".to_owned()],
        reason: "verification reuse fixture".to_owned(),
    }
}

fn check_result(spec: &CheckSpec, success: bool) -> VerificationCheck {
    VerificationCheck {
        id: spec.id.clone(),
        phase: spec.phase,
        command: verification_command_text(spec),
        reason: spec.reason.clone(),
        success,
        reused: false,
        exit_code: Some(if success { 0 } else { 1 }),
        elapsed_ms: 11,
        queue_wait_ms: 1,
        execution_ms: 10,
        stdout_tail: String::new(),
        stderr_tail: String::new(),
        output_truncated: false,
    }
}

fn report(check: VerificationCheck, passed: bool) -> VerificationReport {
    VerificationReport {
        workspace: "demo".to_owned(),
        level: "quick".to_owned(),
        execution: "fixture".to_owned(),
        phases_run: 1,
        passed,
        checks_run: 1,
        checks_reused: usize::from(check.reused),
        checks_failed: usize::from(!check.success),
        skipped_checks: Vec::new(),
        elapsed_ms: 11,
        summary: "verification reuse fixture".to_owned(),
        impact: None,
        cost_model: None,
        checks: vec![check],
    }
}

fn workspace_fixture() -> (tempfile::TempDir, Workspace, ToolHarness) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("src/lib.rs"),
        "pub fn value() -> usize { 1 }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    (root, workspace, harness)
}

#[test]
fn exact_revision_static_pass_reuses_without_minting_duplicate_evidence() {
    let (_root, workspace, harness) = workspace_fixture();
    let spec = static_check("rust-check", &["check", "--locked"]);
    let revision = harness.intelligence.current_revision(&workspace).unwrap();
    let context = harness_verification_cache::VerificationReuseContext::new(
        &workspace, &revision, "quick", true, 30,
    );
    let first = report(check_result(&spec, true), true);
    let produced = harness
        .intelligence
        .record_verification_report("demo", &workspace, &revision, &first)
        .unwrap();
    assert!(!produced.is_empty());
    let evidence_before = crate::evidence_store::load(&workspace).unwrap().len();

    harness.cache_successful_verification_checks(
        &workspace,
        &context,
        std::slice::from_ref(&spec),
        &first,
    );
    let reused = harness
        .cached_verification_check(&workspace, &context, &spec)
        .expect("same-revision static pass should be reusable");
    assert!(reused.reused);
    assert!(reused.success);
    assert_eq!(reused.elapsed_ms, 0);
    assert_eq!(reused.execution_ms, 0);
    assert!(reused.reason.contains("no command executed"));

    let reused_report = report(reused, true);
    assert_eq!(reused_report.checks_reused, 1);
    let duplicate = harness
        .intelligence
        .record_verification_report("demo", &workspace, &revision, &reused_report)
        .unwrap();
    assert!(duplicate.is_empty(), "reuse must not mint fresh proof");
    assert_eq!(
        crate::evidence_store::load(&workspace).unwrap().len(),
        evidence_before,
        "reuse must cite existing proof rather than duplicating Evidence"
    );
}

#[test]
fn source_design_and_command_changes_invalidate_static_reuse() {
    let (root, workspace, harness) = workspace_fixture();
    std::fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    std::fs::write(
        root.path().join(".wcode/project.yaml"),
        "schema_version: 1\nname: reuse-fixture\n",
    )
    .unwrap();
    let product_path = root.path().join(".wcode/design/product.yaml");
    std::fs::write(
        &product_path,
        "schema_version: 1\nid: product:reuse\nname: Reuse\nvision: Initial\n",
    )
    .unwrap();
    let spec = static_check("rust-check", &["check", "--locked"]);
    let initial = harness.intelligence.current_revision(&workspace).unwrap();
    let initial_context = harness_verification_cache::VerificationReuseContext::new(
        &workspace, &initial, "quick", true, 30,
    );
    let first = report(check_result(&spec, true), true);
    harness.cache_successful_verification_checks(
        &workspace,
        &initial_context,
        std::slice::from_ref(&spec),
        &first,
    );
    assert!(harness
        .cached_verification_check(&workspace, &initial_context, &spec)
        .is_some());

    let changed_command = static_check("rust-check", &["check"]);
    assert!(harness
        .cached_verification_check(&workspace, &initial_context, &changed_command)
        .is_none());

    std::fs::write(
        root.path().join("src/lib.rs"),
        "pub fn value() -> usize { 2 }\n",
    )
    .unwrap();
    let source_revision = harness.intelligence.current_revision(&workspace).unwrap();
    assert_ne!(source_revision.code, initial.code);
    let source_context = harness_verification_cache::VerificationReuseContext::new(
        &workspace,
        &source_revision,
        "quick",
        true,
        30,
    );
    assert!(harness
        .cached_verification_check(&workspace, &source_context, &spec)
        .is_none());

    std::fs::write(
        root.path().join("src/lib.rs"),
        "pub fn value() -> usize { 1 }\n",
    )
    .unwrap();
    std::fs::write(
        &product_path,
        "schema_version: 1\nid: product:reuse\nname: Reuse\nvision: Revised\n",
    )
    .unwrap();
    let design_revision = harness.intelligence.current_revision(&workspace).unwrap();
    assert_eq!(design_revision.code, initial.code);
    assert_ne!(design_revision.design, initial.design);
    let design_context = harness_verification_cache::VerificationReuseContext::new(
        &workspace,
        &design_revision,
        "quick",
        true,
        30,
    );
    assert!(harness
        .cached_verification_check(&workspace, &design_context, &spec)
        .is_none());
}

#[test]
fn failures_behavioral_tests_and_unknown_checks_are_never_reused() {
    let (_root, workspace, harness) = workspace_fixture();
    let revision = harness.intelligence.current_revision(&workspace).unwrap();
    let context = harness_verification_cache::VerificationReuseContext::new(
        &workspace, &revision, "quick", true, 30,
    );
    let static_spec = static_check("rust-check", &["check", "--locked"]);
    let failed = report(check_result(&static_spec, false), false);
    harness.cache_successful_verification_checks(
        &workspace,
        &context,
        std::slice::from_ref(&static_spec),
        &failed,
    );
    assert!(harness
        .cached_verification_check(&workspace, &context, &static_spec)
        .is_none());

    for spec in [
        static_check("rust-test", &["test", "--locked"]),
        static_check("focused-rust-test", &["test", "fixture"]),
        static_check("repository-script", &["run", "custom-check"]),
    ] {
        assert!(!harness_verification_cache::reusable_static_check(&spec));
        let successful = report(check_result(&spec, true), true);
        harness.cache_successful_verification_checks(
            &workspace,
            &context,
            std::slice::from_ref(&spec),
            &successful,
        );
        assert!(harness
            .cached_verification_check(&workspace, &context, &spec)
            .is_none());
    }
    assert!(harness_verification_cache::reusable_static_check(
        &static_spec
    ));
}

#[test]
fn mixed_run_does_not_remint_acceptance_for_only_reused_check() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    std::fs::write(
        root.path().join("src/lib.rs"),
        "pub fn value() -> usize { 1 }\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join(".wcode/project.yaml"),
        "schema_version: 1\nname: reuse-acceptance\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join(".wcode/design/product.yaml"),
        "schema_version: 1\nid: product:reuse\nname: Reuse\nvision: Exact proof\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join(".wcode/design/acceptance.yaml"),
        "- schema_version: 1\n  id: AC-STATIC\n  title: Static proof\n  statement: Static check remains valid.\n  verification:\n    - kind: check\n      id: rust-check\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let revision = harness.intelligence.current_revision(&workspace).unwrap();
    let context = harness_verification_cache::VerificationReuseContext::new(
        &workspace, &revision, "quick", true, 30,
    );
    let static_spec = static_check("rust-check", &["check", "--locked"]);
    let initial = report(check_result(&static_spec, true), true);
    let initial_evidence = harness
        .intelligence
        .record_verification_report("demo", &workspace, &revision, &initial)
        .unwrap();
    assert!(initial_evidence
        .iter()
        .any(|item| item.subject == "AC-STATIC"));
    harness.cache_successful_verification_checks(
        &workspace,
        &context,
        std::slice::from_ref(&static_spec),
        &initial,
    );
    let reused = harness
        .cached_verification_check(&workspace, &context, &static_spec)
        .unwrap();
    let test_spec = static_check("rust-test", &["test", "--locked"]);
    let executed_test = check_result(&test_spec, true);
    let mixed = VerificationReport {
        workspace: "demo".to_owned(),
        level: "quick".to_owned(),
        execution: "fixture+exact-revision-static-reuse".to_owned(),
        phases_run: 1,
        passed: true,
        checks_run: 2,
        checks_reused: 1,
        checks_failed: 0,
        skipped_checks: Vec::new(),
        elapsed_ms: 11,
        summary: "one reused static check and one executed behavioral test".to_owned(),
        impact: None,
        cost_model: None,
        checks: vec![reused, executed_test],
    };
    let produced = harness
        .intelligence
        .record_verification_report("demo", &workspace, &revision, &mixed)
        .unwrap();
    assert!(produced.iter().all(|item| item.subject != "AC-STATIC"));
    assert!(produced
        .iter()
        .any(|item| item.subject == "verification:rust-test"));
}
