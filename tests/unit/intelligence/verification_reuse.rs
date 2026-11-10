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
