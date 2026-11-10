// Verification reuse is only as current as the evidence it came from.
// The status path re-checks the chain against what this snapshot can see,
// because retention may have pruned the source since the receipt was written.

use super::*;

#[test]
fn a_reused_check_whose_source_is_no_longer_observable_is_not_a_current_pass() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let revision = runtime.current_revision(&workspace).unwrap();
    let binding = RequiredVerificationCheck::from_command(
        "rust-check",
        "cargo",
        &["check".into(), "--locked".into()],
        ".",
        "workspace",
    );
    let subject = format!("change:{}", revision.code);
    let mut source = Evidence::new(
        "EV-SOURCE".into(),
        subject.clone(),
        EvidenceKind::Verification,
        "verify_project".into(),
        revision.clone(),
        EvidenceResult::Pass,
        Confidence::Deterministic,
    )
    .unwrap();
    source.policy = Some("deterministic/quick/v2".into());
    source.execution_receipt = Some(crate::evidence::VerificationExecutionReceipt {
        execution_git_binding: None,
        schema_version: 1,
        level: "quick".into(),
        required_checks: vec![binding.clone()],
        checks: vec![crate::evidence::VerificationCheckReceipt {
            execution: crate::evidence::VerificationCheckExecution::Executed,
            check: binding.clone(),
            result: EvidenceResult::Pass,
            reused_from: None,
        }],
        skipped_checks: vec![],
    });
    let mut aggregate = Evidence::new(
        "EV-REUSED".into(),
        subject,
        EvidenceKind::Verification,
        "verify_project".into(),
        revision.clone(),
        EvidenceResult::Pass,
        Confidence::Deterministic,
    )
    .unwrap();
    aggregate.policy = Some("deterministic/quick/v2".into());
    aggregate.execution_receipt = Some(crate::evidence::VerificationExecutionReceipt {
        execution_git_binding: None,
        schema_version: 1,
        level: "quick".into(),
        required_checks: vec![binding.clone()],
        checks: vec![crate::evidence::VerificationCheckReceipt {
            execution: crate::evidence::VerificationCheckExecution::Executed,
            check: binding,
            result: EvidenceResult::Pass,
            reused_from: Some("EV-SOURCE".into()),
        }],
        skipped_checks: vec![],
    });
    let with_source = super::revision::coverage_status(
        &runtime,
        &workspace,
        &[source.clone(), aggregate.clone()],
        "quick",
    );
    assert_eq!(
        with_source.deterministic_result,
        Some(EvidenceResult::Pass),
        "an observable reuse source keeps the aggregate current"
    );
    let pruned = super::revision::coverage_status(
        &runtime,
        &workspace,
        std::slice::from_ref(&aggregate),
        "quick",
    );
    assert_ne!(
        pruned.deterministic_result,
        Some(EvidenceResult::Pass),
        "a reuse whose source this snapshot cannot see must not read as a current pass"
    );
    assert!(!pruned.ready);
}

#[test]
fn evidence_bound_to_another_policy_cannot_satisfy_the_plan() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let revision = runtime.current_revision(&workspace).unwrap();
    let plan = runtime
        .create_plan_for_risk("policy", &workspace, RiskLevel::Low)
        .unwrap();
    let binding = RequiredVerificationCheck::from_command(
        "rust-check",
        "cargo",
        &["check".into(), "--locked".into()],
        ".",
        "workspace",
    );
    let mut state = VerificationState::default();
    state
        .restore_workspace(verification_store::load(&workspace).unwrap().unwrap())
        .unwrap();
    let mut status = state.status(&plan.id).unwrap();
    status.plan.required_checks = Some(vec![binding.clone()]);
    status.plan.deterministic_level = "quick".into();
    status.blockers.clear();
    let mut foreign = Evidence::new(
        "EV-FOREIGN-POLICY".into(),
        format!("change:{}", revision.code),
        EvidenceKind::Verification,
        "verify_project".into(),
        revision.clone(),
        EvidenceResult::Pass,
        Confidence::Deterministic,
    )
    .unwrap();
    foreign.policy = Some("deterministic/quick/v2".into());
    foreign.execution_policy_binding = Some("project-policy/v1/999/foreign".into());
    foreign.execution_receipt = Some(crate::evidence::VerificationExecutionReceipt {
        execution_git_binding: None,
        schema_version: 1,
        level: "quick".into(),
        required_checks: vec![binding.clone()],
        checks: vec![crate::evidence::VerificationCheckReceipt {
            execution: crate::evidence::VerificationCheckExecution::Executed,
            check: binding,
            result: EvidenceResult::Pass,
            reused_from: None,
        }],
        skipped_checks: vec![],
    });
    let result = SoftwareIntelligenceRuntime::verification_status_from_snapshot(
        status,
        &revision,
        std::slice::from_ref(&foreign),
        None,
    )
    .unwrap();
    assert_ne!(
        result.deterministic_result,
        Some(EvidenceResult::Pass),
        "evidence produced under another policy must not satisfy this plan"
    );
    assert!(!result.ready);
}

#[test]
fn evidence_from_another_git_identity_cannot_satisfy_the_plan() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let revision = runtime.current_revision(&workspace).unwrap();
    let plan = runtime
        .create_plan_for_risk("git", &workspace, RiskLevel::Low)
        .unwrap();
    let binding = RequiredVerificationCheck::from_command(
        "rust-check",
        "cargo",
        &["check".into(), "--locked".into()],
        ".",
        "workspace",
    );
    let mut state = VerificationState::default();
    state
        .restore_workspace(verification_store::load(&workspace).unwrap().unwrap())
        .unwrap();
    let mut status = state.status(&plan.id).unwrap();
    status.plan.required_checks = Some(vec![binding.clone()]);
    status.plan.deterministic_level = "quick".into();
    status.blockers.clear();

    let make = |head: &str| {
        let mut record = Evidence::new(
            "EV-GIT".into(),
            format!("change:{}", revision.code),
            EvidenceKind::Verification,
            "verify_project".into(),
            revision.clone(),
            EvidenceResult::Pass,
            Confidence::Deterministic,
        )
        .unwrap();
        record.policy = Some("deterministic/quick/v2".into());
        record.execution_git_binding = Some(crate::verification::change::ExecutionGitBinding {
            repository: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .into(),
            head_sha: head.into(),
            tree_sha: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
            dirty: false,
            index_fingerprint:
                "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".into(),
        });
        record.execution_receipt = Some(crate::evidence::VerificationExecutionReceipt {
            execution_git_binding: record.execution_git_binding.clone(),
            schema_version: 1,
            level: "quick".into(),
            required_checks: vec![binding.clone()],
            checks: vec![crate::evidence::VerificationCheckReceipt {
                execution: crate::evidence::VerificationCheckExecution::Executed,
                check: binding.clone(),
                result: EvidenceResult::Pass,
                reused_from: None,
            }],
            skipped_checks: vec![],
        });
        record
    };
    let current = make(&"a".repeat(40));
    let other = make(&"d".repeat(40));
    let with_current = SoftwareIntelligenceRuntime::verification_status_from_snapshot(
        status.clone(),
        &revision,
        std::slice::from_ref(&current),
        current.execution_git_binding.as_ref(),
    )
    .unwrap();
    assert_eq!(
        with_current.deterministic_result,
        Some(EvidenceResult::Pass),
        "evidence bound to the workspace's current Git identity stays current"
    );
    let with_other = SoftwareIntelligenceRuntime::verification_status_from_snapshot(
        status,
        &revision,
        std::slice::from_ref(&other),
        current.execution_git_binding.as_ref(),
    )
    .unwrap();
    assert_ne!(
        with_other.deterministic_result,
        Some(EvidenceResult::Pass),
        "evidence from another Git identity must not satisfy the plan"
    );
    assert!(!with_other.ready);
}
