use super::*;

fn passing_report() -> VerificationReport {
    VerificationReport {
        workspace: "demo".into(),
        level: "quick".into(),
        execution: "fixture".into(),
        phases_run: 1,
        passed: true,
        checks_run: 1,
        checks_reused: 0,
        checks_failed: 0,
        skipped_checks: Vec::new(),
        elapsed_ms: 1,
        summary: "fixture passed".into(),
        impact: None,
        cost_model: None,
        checks: vec![crate::harness::VerificationCheck {
            id: "rust-check".into(),
            phase: 0,
            command: "cargo check --locked".into(),
            reason: "fixture".into(),
            success: true,
            reused: false,
            exit_code: Some(0),
            elapsed_ms: 1,
            queue_wait_ms: 0,
            execution_ms: 1,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            output_truncated: false,
        }],
    }
}

#[test]
fn audit_incomplete_acceptance_is_not_a_pass_and_known_failure_is_preserved() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    fs::write(root.path().join(".wcode/design/acceptance.yaml"),
        "- schema_version: 1\n  id: AC-AUDIT-001\n  title: All required checks\n  statement: Both checks must run.\n  verification:\n    - kind: check\n      id: rust-check\n    - kind: check\n      id: rust-test\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let revision = runtime.current_revision(&workspace).unwrap();
    for success in [true, false] {
        let mut report = passing_report();
        report.passed = false;
        report.checks[0].success = success;
        report.checks_failed = usize::from(!success);
        report.skipped_checks = vec!["rust-test".into()];
        let evidence = runtime
            .record_verification_report("demo", &workspace, &revision, &report)
            .unwrap();
        let criterion = evidence
            .iter()
            .find(|item| item.subject == "AC-AUDIT-001")
            .unwrap();
        assert_eq!(
            criterion.result,
            if success {
                EvidenceResult::Inconclusive
            } else {
                EvidenceResult::Fail
            }
        );
    }
}

#[test]
fn audit_language_quality_does_not_replace_project_verification() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let revision = runtime.current_revision(&workspace).unwrap();
    let mut full = passing_report();
    full.level = "full".into();
    full.passed = false;
    full.checks_failed = 1;
    full.checks[0].success = false;
    runtime
        .record_verification_report("demo", &workspace, &revision, &full)
        .unwrap();
    let mut lint = passing_report();
    lint.level = "language-quality".into();
    lint.execution = "repository-declared-check-only-provider".into();
    lint.checks[0].id = "quality-lint-rust-clippy".into();
    let evidence = runtime
        .record_verification_report("demo", &workspace, &revision, &lint)
        .unwrap();
    assert!(evidence
        .iter()
        .any(|item| item.subject == "verification:quality-lint-rust-clippy"));
    assert!(
        evidence
            .iter()
            .all(|item| item.kind != EvidenceKind::Verification),
        "one provider check must not produce a whole-project verification pass"
    );
}

#[test]
fn audit_verification_aggregation_is_policy_scoped_and_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let revision = runtime.current_revision(&workspace).unwrap();
    let mut full = runtime
        .record_verification_report("demo", &workspace, &revision, &passing_report())
        .unwrap()
        .into_iter()
        .find(|item| item.kind == EvidenceKind::Verification)
        .unwrap();
    full.policy = Some("deterministic/full/v1".into());
    full.result = EvidenceResult::Fail;
    full.timestamp_ms = 10;
    let mut quick = full.clone();
    quick.policy = Some("deterministic/quick/v1".into());
    quick.result = EvidenceResult::Pass;
    quick.timestamp_ms = 20;
    assert_eq!(
        aggregate_verification_results([&full, &quick].into_iter()),
        Some(EvidenceResult::Fail)
    );
    let mut recovered = full.clone();
    recovered.result = EvidenceResult::Pass;
    recovered.timestamp_ms = 30;
    assert_eq!(
        aggregate_verification_results([&full, &quick, &recovered].into_iter()),
        Some(EvidenceResult::Pass)
    );
    let mut older_quick_failure = quick.clone();
    older_quick_failure.result = EvidenceResult::Fail;
    assert_eq!(
        aggregate_verification_results([&older_quick_failure, &recovered].into_iter()),
        Some(EvidenceResult::Pass)
    );
    let mut independent = recovered.clone();
    independent.producer = "independent-gate".into();
    independent.result = EvidenceResult::Fail;
    assert_eq!(
        aggregate_verification_results([&recovered, &independent].into_iter()),
        Some(EvidenceResult::Fail)
    );
    full.timestamp_ms = recovered.timestamp_ms;
    for records in [[&full, &recovered], [&recovered, &full]] {
        assert_eq!(
            aggregate_verification_results(records.into_iter()),
            Some(EvidenceResult::Fail)
        );
    }
    let mut legacy_quality = recovered.clone();
    legacy_quality.policy = Some("deterministic/language-quality/v1".into());
    assert_eq!(
        aggregate_verification_results(std::iter::once(&legacy_quality)),
        None
    );
}

#[test]
fn verification_rejects_source_changes_before_recording_any_evidence() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("lib.rs"), "pub fn before() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let before = runtime.current_revision(&workspace).unwrap();
    fs::write(root.path().join("lib.rs"), "pub fn after() {}\n").unwrap();
    let error = runtime
        .record_verification_report("demo", &workspace, &before, &passing_report())
        .unwrap_err();
    assert!(error.to_string().contains("revision changed"));
    assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
}

#[test]
fn verification_rejects_design_only_changes_before_recording_evidence() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    let path = root.path().join(".wcode/design/product.yaml");
    fs::write(
        &path,
        "schema_version: 1\nid: product:demo\nname: Demo\nvision: First\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let before = runtime.current_revision(&workspace).unwrap();
    fs::write(
        &path,
        "schema_version: 1\nid: product:demo\nname: Demo\nvision: Revised\n",
    )
    .unwrap();
    let after = runtime.current_revision(&workspace).unwrap();
    assert_eq!(before.code, after.code);
    assert_ne!(before.design, after.design);
    let error = runtime
        .record_verification_report("demo", &workspace, &before, &passing_report())
        .unwrap_err();
    assert!(error.to_string().contains("revision changed"));
    assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
}

#[test]
fn incomplete_revisions_cannot_produce_verification_evidence() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let complete = runtime.current_revision(&workspace).unwrap();
    for design_only in [false, true] {
        let mut partial = complete.clone();
        if design_only {
            partial.design = Some(format!("{}:partial", complete.code));
        } else {
            partial.code.push_str(":partial");
        }
        let error = runtime
            .record_verification_report("demo", &workspace, &partial, &passing_report())
            .unwrap_err();
        assert!(error.to_string().contains("revision is incomplete"));
        assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
    }
}

#[test]
fn stable_verification_keeps_the_captured_revision() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("lib.rs"), "pub fn unchanged() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let before = runtime.current_revision(&workspace).unwrap();
    let evidence = runtime
        .record_verification_report("demo", &workspace, &before, &passing_report())
        .unwrap();
    assert!(!evidence.is_empty());
    assert!(evidence.iter().all(|item| {
        item.revision.code == before.code && item.revision.design == before.design
    }));
    assert_eq!(
        crate::evidence_store::load(&workspace).unwrap().len(),
        evidence.len()
    );
}
