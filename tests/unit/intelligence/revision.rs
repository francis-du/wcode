use super::*;

fn passing_report() -> VerificationReport {
    let binding = RequiredVerificationCheck::from_command(
        "rust-check",
        "cargo",
        &["check".into(), "--locked".into()],
        ".",
        "workspace",
    );
    VerificationReport {
        execution_git_binding: None,
        required_checks: Some(vec![binding.clone()]),
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
            execution: crate::evidence::VerificationCheckExecution::Executed,
            exit_code: Some(0),
            elapsed_ms: 1,
            queue_wait_ms: 0,
            execution_ms: 1,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            output_truncated: false,
            signature: Some(binding.signature),
            evidence_id: None,
        }],
    }
}

#[test]
fn process_execution_labels_without_exit_results_cannot_mint_passing_evidence() {
    use crate::evidence::VerificationCheckExecution;
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let revision = runtime.current_revision(&workspace).unwrap();
    for execution in [
        VerificationCheckExecution::Unknown,
        VerificationCheckExecution::Unavailable,
        VerificationCheckExecution::TimedOut,
        VerificationCheckExecution::Executed,
    ] {
        let retained_before = crate::evidence_store::load(&workspace).unwrap();
        let mut report = passing_report();
        report.checks[0].exit_code = None;
        report.checks[0].execution = execution;
        assert!(
            runtime
                .record_verification_report("demo", &workspace, &revision, &report)
                .is_err(),
            "a process label without execution proof cannot become pass: {execution:?}"
        );
        assert_eq!(
            crate::evidence_store::load(&workspace).unwrap(),
            retained_before
        );
        report.passed = false;
        report.checks[0].success = false;
        report.checks_failed = 1;
        let records = runtime
            .record_verification_report("demo", &workspace, &revision, &report)
            .unwrap();
        assert!(records
            .iter()
            .all(|record| record.result != EvidenceResult::Pass));
        assert!(records
            .iter()
            .filter_map(|record| record.execution_receipt.as_ref())
            .flat_map(|receipt| &receipt.checks)
            .all(|check| check.execution != VerificationCheckExecution::Executed));
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
    let mut full_report = passing_report();
    full_report.level = "full".into();
    let mut full = runtime
        .record_verification_report("demo", &workspace, &revision, &full_report)
        .unwrap()
        .into_iter()
        .find(|record| record.kind == EvidenceKind::Verification)
        .unwrap();
    let set_result = |record: &mut Evidence, result| {
        record.result = result;
        for check in &mut record.execution_receipt.as_mut().unwrap().checks {
            check.result = result;
        }
    };
    set_result(&mut full, EvidenceResult::Fail);
    full.timestamp_ms = 10;
    let mut quick = full.clone();
    quick.policy = Some("deterministic/quick/v2".into());
    quick.execution_receipt.as_mut().unwrap().level = "quick".into();
    set_result(&mut quick, EvidenceResult::Pass);
    quick.timestamp_ms = 20;
    assert_eq!(
        coverage_status(
            &runtime,
            &workspace,
            &[full.clone(), quick.clone()],
            "quick"
        )
        .deterministic_result,
        Some(EvidenceResult::Fail)
    );
    let mut recovered = full.clone();
    set_result(&mut recovered, EvidenceResult::Pass);
    recovered.timestamp_ms = 30;
    assert_eq!(
        coverage_status(
            &runtime,
            &workspace,
            &[full.clone(), quick.clone(), recovered.clone()],
            "quick"
        )
        .deterministic_result,
        Some(EvidenceResult::Pass)
    );
    set_result(&mut quick, EvidenceResult::Fail);
    assert_eq!(
        coverage_status(&runtime, &workspace, &[quick, recovered.clone()], "quick")
            .deterministic_result,
        Some(EvidenceResult::Pass)
    );
    let mut independent = full.clone();
    independent.producer = "independent-gate".into();
    assert_eq!(
        coverage_status(
            &runtime,
            &workspace,
            &[recovered.clone(), independent],
            "quick"
        )
        .deterministic_result,
        Some(EvidenceResult::Fail)
    );
    full.timestamp_ms = recovered.timestamp_ms;
    for records in [
        [full.clone(), recovered.clone()],
        [recovered.clone(), full.clone()],
    ] {
        assert_eq!(
            coverage_status(&runtime, &workspace, &records, "quick").deterministic_result,
            Some(EvidenceResult::Fail)
        );
    }
    recovered.policy = Some("deterministic/language-quality/v1".into());
    recovered.execution_receipt = None;
    assert_eq!(
        coverage_status(&runtime, &workspace, &[recovered], "quick").deterministic_result,
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
fn generated_build_state_does_not_invalidate_code_revision() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn value() -> usize { 1 }\n",
    )
    .unwrap();
    for path in [
        "build/generated.txt",
        ".dart_tool/package_config.json",
        ".build/state.json",
        ".gradle/cache.bin",
        ".swiftpm/configuration/state.json",
        "ios/Flutter/ephemeral/Packages/generated/state.json",
        "ios/Pods/generated/state.json",
        "macos/.symlinks/plugins/generated/state.json",
        "macos/.plugin_symlinks/generated/state.json",
        ".next/cache/state.json",
        ".cache/tool-state.json",
        "coverage/lcov.info",
        "dist/bundle.js",
    ] {
        let path = root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "generation=1\n").unwrap();
    }

    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let initial = runtime.current_revision(&workspace).unwrap();

    for path in [
        "build/generated.txt",
        ".dart_tool/package_config.json",
        ".build/state.json",
        ".gradle/cache.bin",
        ".swiftpm/configuration/state.json",
        "ios/Flutter/ephemeral/Packages/generated/state.json",
        "ios/Pods/generated/state.json",
        "macos/.symlinks/plugins/generated/state.json",
        "macos/.plugin_symlinks/generated/state.json",
        ".next/cache/state.json",
        ".cache/tool-state.json",
        "coverage/lcov.info",
        "dist/bundle.js",
    ] {
        fs::write(root.path().join(path), "generation=2\n").unwrap();
    }
    let generated_changed = runtime.current_revision(&workspace).unwrap();
    assert_eq!(generated_changed.code, initial.code);

    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn value() -> usize { 2 }\n",
    )
    .unwrap();
    let source_changed = runtime.current_revision(&workspace).unwrap();
    assert_ne!(source_changed.code, initial.code);
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

#[test]
fn initialized_revision_reuses_one_scan_and_keeps_code_design_boundaries() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    fs::create_dir_all(root.path().join(".wcode/evidence")).unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join(".wcode/project.yaml"),
        "schema_version: 1\nname: revision-fixture\n",
    )
    .unwrap();
    let product = root.path().join(".wcode/design/product.yaml");
    fs::write(
        &product,
        "schema_version: 1\nid: product:demo\nname: Demo\nvision: Initial\n",
    )
    .unwrap();
    let source = root.path().join("src/lib.rs");
    fs::write(&source, "pub fn value() -> usize { 1 }\n").unwrap();
    let runtime_noise = root.path().join(".wcode/evidence/runtime.json");
    fs::write(&runtime_noise, "{\"generation\":1}\n").unwrap();

    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let scan_count = || REVISION_SCAN_CALLS.with(|count| count.get());
    let reset_scans = || REVISION_SCAN_CALLS.with(|count| count.set(0));

    reset_scans();
    let initial = runtime.current_revision(&workspace).unwrap();
    assert_eq!(
        scan_count(),
        1,
        "normal initialized revisions should share one root scan"
    );
    assert!(initial.design.is_some());

    fs::write(&runtime_noise, "{\"generation\":2}\n").unwrap();
    reset_scans();
    let noise_changed = runtime.current_revision(&workspace).unwrap();
    assert_eq!(scan_count(), 1);
    assert_eq!(
        noise_changed, initial,
        "non-Design .wcode runtime state is not revision input"
    );

    fs::write(
        &product,
        "schema_version: 1\nid: product:demo\nname: Demo\nvision: Revised\n",
    )
    .unwrap();
    reset_scans();
    let design_changed = runtime.current_revision(&workspace).unwrap();
    assert_eq!(scan_count(), 1);
    assert_eq!(design_changed.code, initial.code);
    assert_ne!(design_changed.design, initial.design);

    fs::write(&source, "pub fn value() -> usize { 2 }\n").unwrap();
    reset_scans();
    let source_changed = runtime.current_revision(&workspace).unwrap();
    assert_eq!(scan_count(), 1);
    assert_ne!(source_changed.code, design_changed.code);
    assert_eq!(source_changed.design, design_changed.design);
}
#[test]
fn named_test_without_event_does_not_mint_acceptance_evidence() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    fs::write(root.path().join(".wcode/design/acceptance.yaml"),
        "- schema_version: 1\n  id: AC-NAMED\n  title: Named test\n  statement: Run the named test.\n  verification:\n    - kind: test\n      path: tests/target.rs\n      symbol: target_case\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let revision = runtime.current_revision(&workspace).unwrap();
    for id in ["rust-test", "focused-rust-test", "python-test"] {
        let mut report = passing_report();
        let binding = RequiredVerificationCheck::from_command(
            id,
            "runner",
            &["filter".into()],
            ".",
            "workspace",
        );
        report.required_checks = Some(vec![binding.clone()]);
        report.checks[0].id = id.into();
        report.checks[0].command = "runner filter".into();
        report.checks[0].signature = Some(binding.signature);
        let produced = runtime
            .record_verification_report("demo", &workspace, &revision, &report)
            .unwrap();
        assert!(produced
            .iter()
            .any(|record| record.subject == format!("verification:{id}")
                && record.execution_receipt.is_some()));
        assert!(produced.iter().all(|record| record.subject != "AC-NAMED"));
    }
}

#[test]
fn mixed_check_test_is_inconclusive_and_known_failure_is_preserved() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    fs::write(root.path().join(".wcode/design/acceptance.yaml"),
        "- schema_version: 1\n  id: AC-MIXED\n  title: Mixed proof\n  statement: Check and exact test are required.\n  verification:\n    - kind: check\n      id: rust-check\n    - kind: test\n      path: tests/target.rs\n      symbol: target_case\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let revision = runtime.current_revision(&workspace).unwrap();
    for success in [true, false] {
        let mut report = passing_report();
        report.passed = success;
        report.checks[0].success = success;
        report.checks_failed = usize::from(!success);
        let produced = runtime
            .record_verification_report("demo", &workspace, &revision, &report)
            .unwrap();
        assert_eq!(
            produced
                .iter()
                .find(|record| record.subject == "AC-MIXED")
                .unwrap()
                .result,
            if success {
                EvidenceResult::Inconclusive
            } else {
                EvidenceResult::Fail
            }
        );
    }
}

fn coverage_status(
    runtime: &SoftwareIntelligenceRuntime,
    workspace: &Workspace,
    evidence: &[Evidence],
    level: &str,
) -> VerificationStatus {
    let plan = runtime
        .create_plan_for_risk("demo", workspace, RiskLevel::Low)
        .unwrap();
    let mut state = VerificationState::default();
    // Use the persisted plan's initial status, with reviewer completion isolated
    // from the deterministic contract under test.
    state
        .restore_workspace(verification_store::load(workspace).unwrap().unwrap())
        .unwrap();
    let mut status = state.status(&plan.id).unwrap();
    status.plan.deterministic_level = level.into();
    status.submitted = status.plan.job_ids.len();
    status.blockers.clear();
    SoftwareIntelligenceRuntime::verification_status_from_snapshot(
        status,
        &runtime.current_revision(workspace).unwrap(),
        evidence,
    )
    .unwrap()
}

#[test]
fn full_plan_rejects_quick_receipt_and_legacy_gate_claims() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let revision = runtime.current_revision(&workspace).unwrap();
    let quick = runtime
        .record_verification_report("demo", &workspace, &revision, &passing_report())
        .unwrap();
    let status = coverage_status(&runtime, &workspace, &quick, "full");
    assert_eq!(status.deterministic_result, None);
    assert!(!status.ready);
    assert!(status
        .blockers
        .iter()
        .any(|blocker| blocker == "deterministic-minimum-level-unproved:full"));
    let mut report = passing_report();
    report.level = "full".into();
    let full = runtime
        .record_verification_report("demo", &workspace, &revision, &report)
        .unwrap();
    let status = coverage_status(&runtime, &workspace, &full, "full");
    assert_eq!(status.deterministic_result, Some(EvidenceResult::Pass));
    assert!(status.ready, "{:?}", status.blockers);
    let mut legacy = full.clone();
    for record in &mut legacy {
        record.execution_receipt = None;
        record.policy = Some("deterministic/full/v1".into());
    }
    let status = coverage_status(&runtime, &workspace, &legacy, "full");
    assert_eq!(status.deterministic_result, None);
    assert!(!status.ready);
    assert!(status
        .blockers
        .iter()
        .any(|blocker| blocker == "deterministic-legacy-receipt-untrusted"));
}

#[test]
fn required_check_missing_skipped_or_changed_signature_blocks_ready() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let revision = runtime.current_revision(&workspace).unwrap();
    let produced = runtime
        .record_verification_report("demo", &workspace, &revision, &passing_report())
        .unwrap();
    let mut status = coverage_status(&runtime, &workspace, &produced, "quick");
    let test = RequiredVerificationCheck::from_command(
        "rust-test",
        "cargo",
        &["test".into(), "--locked".into()],
        ".",
        "workspace",
    );
    status
        .plan
        .required_checks
        .as_mut()
        .unwrap()
        .push(test.clone());
    status.blockers.clear();
    let missing = SoftwareIntelligenceRuntime::verification_status_from_snapshot(
        status.clone(),
        &revision,
        &produced,
    )
    .unwrap();
    assert!(!missing.ready);
    assert_eq!(missing.deterministic_result, None);
    assert!(missing
        .blockers
        .iter()
        .any(|blocker| blocker == "deterministic-required-check-missing:rust-test"));
    let mut report = passing_report();
    report.required_checks.as_mut().unwrap().push(test);
    report.skipped_checks = vec!["rust-test".into()];
    report.passed = false;
    let skipped = runtime
        .record_verification_report("demo", &workspace, &revision, &report)
        .unwrap();
    let skipped =
        SoftwareIntelligenceRuntime::verification_status_from_snapshot(status, &revision, &skipped)
            .unwrap();
    assert!(!skipped.ready);
    assert_eq!(
        skipped.deterministic_result,
        Some(EvidenceResult::Inconclusive)
    );
    assert!(skipped
        .blockers
        .iter()
        .any(|blocker| blocker == "deterministic-required-check-skipped:rust-test"));
    let mut changed = passing_report();
    let different = RequiredVerificationCheck::from_command(
        "rust-check",
        "cargo",
        &["check".into()],
        ".",
        "workspace",
    );
    changed.required_checks = Some(vec![different.clone()]);
    changed.checks[0].signature = Some(different.signature);
    let produced = runtime
        .record_verification_report("demo", &workspace, &revision, &changed)
        .unwrap();
    assert!(!coverage_status(&runtime, &workspace, &produced, "quick").ready);
}

#[test]
fn inconsistent_or_duplicate_native_report_cannot_mint_receipts() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let revision = runtime.current_revision(&workspace).unwrap();
    for case in 0..4 {
        let mut report = passing_report();
        match case {
            0 => report.checks_failed = 1,
            1 => {
                report.checks.push(report.checks[0].clone());
                report.checks_run = 2;
            }
            2 => report.required_checks.as_mut().unwrap().push(
                RequiredVerificationCheck::from_command("missing", "runner", &[], ".", "workspace"),
            ),
            _ => report.checks[0].signature = None,
        }
        assert!(runtime
            .record_verification_report("demo", &workspace, &revision, &report)
            .is_err());
        assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
    }
}

#[test]
fn reuse_without_exact_source_receipt_cannot_mint_evidence() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let revision = runtime.current_revision(&workspace).unwrap();
    for source in [None, Some("EV-invented".into())] {
        let mut report = passing_report();
        report.checks[0].reused = true;
        report.checks[0].evidence_id = source;
        report.checks_reused = 1;
        assert!(runtime
            .record_verification_report("demo", &workspace, &revision, &report)
            .is_err());
        assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
    }
}

#[test]
fn exact_receipt_failure_precedence_keeps_ties_and_independent_scope() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let revision = runtime.current_revision(&workspace).unwrap();
    let mut failed = passing_report();
    failed.passed = false;
    failed.checks_failed = 1;
    failed.checks[0].success = false;
    let mut fail = runtime
        .record_verification_report("demo", &workspace, &revision, &failed)
        .unwrap()
        .into_iter()
        .find(|record| record.kind == EvidenceKind::Verification)
        .unwrap();
    let mut full = passing_report();
    full.level = "full".into();
    let mut pass = runtime
        .record_verification_report("demo", &workspace, &revision, &full)
        .unwrap()
        .into_iter()
        .find(|record| record.kind == EvidenceKind::Verification)
        .unwrap();
    fail.timestamp_ms = 10;
    pass.timestamp_ms = 10;
    let tied = coverage_status(&runtime, &workspace, &[fail.clone(), pass.clone()], "quick");
    assert_eq!(tied.deterministic_result, Some(EvidenceResult::Fail));
    pass.timestamp_ms = 11;
    assert_eq!(
        coverage_status(&runtime, &workspace, &[fail.clone(), pass.clone()], "quick")
            .deterministic_result,
        Some(EvidenceResult::Pass)
    );
    fail.producer = "independent-native-gate".into();
    assert_eq!(
        coverage_status(&runtime, &workspace, &[fail, pass], "quick").deterministic_result,
        Some(EvidenceResult::Fail)
    );
}

#[test]
fn legacy_plan_without_required_checks_remains_readable_but_unknown() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let revision = runtime.current_revision(&workspace).unwrap();
    let proof = runtime
        .record_verification_report("demo", &workspace, &revision, &passing_report())
        .unwrap();
    let mut status = coverage_status(&runtime, &workspace, &proof, "quick");
    let mut legacy = serde_json::to_value(&status.plan).unwrap();
    legacy.as_object_mut().unwrap().remove("required_checks");
    status.plan = serde_json::from_value(legacy).unwrap();
    status.blockers.clear();
    let status =
        SoftwareIntelligenceRuntime::verification_status_from_snapshot(status, &revision, &proof)
            .unwrap();
    assert!(status.plan.required_checks.is_none());
    assert_eq!(status.deterministic_result, None);
    assert!(!status.ready);
    assert!(status
        .blockers
        .contains(&"deterministic-required-checks-unknown".into()));
    let mut legacy_evidence = serde_json::to_value(&proof[0]).unwrap();
    legacy_evidence
        .as_object_mut()
        .unwrap()
        .remove("execution_receipt");
    assert!(serde_json::from_value::<Evidence>(legacy_evidence)
        .unwrap()
        .execution_receipt
        .is_none());
}

// Isolate revision identity with every other proof dimension satisfied. These
// retained fixture records intentionally cover identities native writers reject.
fn satisfied_identity_snapshot(
    runtime: &SoftwareIntelligenceRuntime,
    workspace: &Workspace,
    plan_revision: Option<Revision>,
    current_revision: &Revision,
) -> VerificationStatus {
    let captured = runtime.current_revision(workspace).unwrap();
    let mut proof = runtime
        .record_verification_report("demo", workspace, &captured, &passing_report())
        .unwrap();
    let mut status = coverage_status(runtime, workspace, &proof, "quick");
    status.plan.revision = plan_revision;
    let proof_revision = status
        .plan
        .revision
        .as_ref()
        .unwrap_or(current_revision)
        .clone();
    status.plan.subject = format!("change:{}", proof_revision.code);
    status.plan.require_property = true;
    status.plan.require_mutation = true;
    status.plan.require_fuzz = true;
    status.plan.require_human_approval = true;
    status.plan.deterministic_checks.push("runtime-gate".into());
    status.plan.stage_targets.clear();
    status.queued = 0;
    status.claimed = 0;
    for job in &mut status.jobs {
        job.status = crate::verification::VerificationJobStatus::Submitted;
        job.submission = Some(crate::verification::ReviewSubmission {
            verdict: crate::verification::ReviewVerdict::Pass,
            summary: "Independent fixture review passed.".into(),
            claims: vec![],
            risks: vec![],
            model: Some("fixture-reviewer".into()),
        });
    }
    for record in &mut proof {
        record.subject = status.plan.subject.clone();
        record.revision = proof_revision.clone();
    }
    for kind in [
        EvidenceKind::Property,
        EvidenceKind::Mutation,
        EvidenceKind::Fuzz,
        EvidenceKind::Runtime,
        EvidenceKind::HumanApproval,
    ] {
        let mut record = Evidence::new(
            format!("EV-identity-{kind:?}"),
            status.plan.subject.clone(),
            kind,
            "fixture-native-producer".into(),
            proof_revision.clone(),
            EvidenceResult::Pass,
            Confidence::High,
        )
        .unwrap();
        record.authority = if kind == EvidenceKind::HumanApproval {
            EvidenceAuthority::LocalOperator
        } else {
            EvidenceAuthority::NativeStage
        };
        if kind == EvidenceKind::HumanApproval {
            record.policy =
                Some(verification_snapshot::human_approval_policy(&status.plan).unwrap());
        }
        proof.push(record);
    }
    status.blockers.clear();
    SoftwareIntelligenceRuntime::verification_status_from_snapshot(status, current_revision, &proof)
        .unwrap()
}

#[test]
fn partial_revision_identity_never_ready_even_with_all_required_proof() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let complete = runtime.current_revision(&workspace).unwrap();
    let control =
        satisfied_identity_snapshot(&runtime, &workspace, Some(complete.clone()), &complete);
    assert!(control.ready, "{:?}", control.blockers);
    assert!(
        complete.design.is_none(),
        "captured absence of Design is complete"
    );
    for dimension in ["code", "design", "both"] {
        let mut partial = complete.clone();
        if dimension != "design" {
            partial.code.push_str(":partial");
        }
        if dimension != "code" {
            partial.design = Some(format!("{}:partial", complete.code));
        }
        for binding in ["current", "plan", "both"] {
            let current = if binding == "plan" {
                &complete
            } else {
                &partial
            };
            let planned = if binding == "current" {
                &complete
            } else {
                &partial
            };
            let status =
                satisfied_identity_snapshot(&runtime, &workspace, Some(planned.clone()), current);
            assert!(!status.ready, "{dimension}/{binding} passed");
            if binding != "plan" {
                assert!(status
                    .blockers
                    .contains(&"workspace-revision-incomplete".into()));
            }
            if binding != "current" {
                assert!(status
                    .blockers
                    .contains(&"verification-plan-revision-incomplete".into()));
            }
            if binding == "both" {
                assert_eq!(status.deterministic_result, Some(EvidenceResult::Pass));
                assert!(status.human_approval);
                assert_eq!(status.submitted, status.plan.job_ids.len());
                assert!(status.jobs.iter().all(|job| job.submission.is_some()));
                assert_eq!(status.stage_results.len(), 4);
                assert!(status
                    .stage_results
                    .values()
                    .all(|result| *result == EvidenceResult::Pass));
                assert!(!status
                    .blockers
                    .iter()
                    .any(|reason| reason.contains("changed-since-plan")));
            }
        }
    }
}

#[test]
fn unbound_legacy_plan_is_readable_but_never_revision_ready() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".wcode")).unwrap();
    fs::write(
        root.path().join(".wcode/project.yaml"),
        "schema_version: 1\nname: identity\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let revision = runtime.current_revision(&workspace).unwrap();
    assert!(revision.design.is_some());
    let control =
        satisfied_identity_snapshot(&runtime, &workspace, Some(revision.clone()), &revision);
    assert!(control.ready, "{:?}", control.blockers);
    let legacy = satisfied_identity_snapshot(&runtime, &workspace, None, &revision);
    let decoded: VerificationPlan =
        serde_json::from_value(serde_json::to_value(&legacy.plan).unwrap()).unwrap();
    assert!(
        decoded.revision.is_none(),
        "legacy serialization remains readable"
    );
    assert_eq!(legacy.deterministic_result, Some(EvidenceResult::Pass));
    assert!(legacy.human_approval);
    assert_eq!(legacy.stage_results.len(), 4);
    assert!(!legacy.ready);
    assert!(legacy
        .blockers
        .contains(&"verification-plan-revision-unbound".into()));
    assert!(!legacy
        .blockers
        .iter()
        .any(|reason| reason.contains("changed-since-plan")));
}
