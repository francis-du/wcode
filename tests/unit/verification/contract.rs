//! Pure trusted-input fixtures; these records do not represent executed checks.
use super::*;
use crate::evidence::{
    Confidence, EvidenceAuthority, EvidenceKind, EvidenceResult, RequiredVerificationCheck,
    VerificationCheckReceipt, VerificationExecutionReceipt,
};
use crate::risk::RiskLevel;
use crate::verification::{ReviewSubmission, ReviewVerdict, ReviewerRole, VerificationPlanBinding};
use sha2::{Digest, Sha256};

const WORKSPACE: &str = "fixture";
const PLAN: &str = "VP-fixture";
const JOB: &str = "VJ-fixture";

fn fixture() -> (VerificationState, Revision, Vec<Evidence>) {
    let revision = Revision {
        code: "sha256:fixture".into(),
        design: Some("sha256:design".into()),
    };
    let required = vec![
        RequiredVerificationCheck::from_command(
            "rust-check",
            "cargo",
            &["check".into()],
            ".",
            "root",
        ),
        RequiredVerificationCheck::from_command(
            "rust-test",
            "cargo",
            &["test".into()],
            ".",
            "root",
        ),
    ];
    let mut state = VerificationState::default();
    state
        .create_plan(
            PLAN.into(),
            WORKSPACE.into(),
            format!("change:{}", revision.code),
            VerificationPlanBinding {
                revision: revision.clone(),
                stage_targets: vec![],
                automation_gaps: vec![],
                required_checks: Some(required.clone()),
            },
            RiskLevel::Low,
            [JOB.into()].into_iter(),
        )
        .unwrap();
    state
        .claim(
            WORKSPACE,
            "fixture-reviewer",
            &BTreeSet::from(["correctness_review".into()]),
            Some(ReviewerRole::Correctness),
        )
        .unwrap();
    state
        .submit(
            WORKSPACE,
            JOB,
            "fixture-reviewer",
            ReviewSubmission {
                verdict: ReviewVerdict::Pass,
                summary: "Pure fixture review.".into(),
                claims: vec![],
                risks: vec![],
                model: None,
            },
        )
        .unwrap();
    let mut record = Evidence::new(
        "EV-fixture".into(),
        format!("change:{}", revision.code),
        EvidenceKind::Verification,
        "fixture-native-runner".into(),
        revision.clone(),
        EvidenceResult::Pass,
        Confidence::Deterministic,
    )
    .unwrap();
    record.authority = EvidenceAuthority::NativeVerification;
    record.policy = Some("deterministic/quick/v2".into());
    record.timestamp_ms = 1;
    record.execution_receipt = Some(VerificationExecutionReceipt {
        schema_version: 1,
        level: "quick".into(),
        checks: required
            .iter()
            .cloned()
            .map(|check| VerificationCheckReceipt {
                execution: crate::evidence::VerificationCheckExecution::Executed,
                check,
                result: EvidenceResult::Pass,
                reused_from: None,
            })
            .collect(),
        required_checks: required,
        skipped_checks: vec![],
        execution_git_binding: None,
    });
    (state, revision, vec![record])
}

fn evaluate(
    state: &VerificationState,
    revision: &Revision,
    evidence: &[Evidence],
) -> VerificationStatus {
    evaluate_snapshot(state, WORKSPACE, PLAN, revision, evidence).unwrap()
}

#[test]
fn public_snapshot_contract_matches_native_engine_without_mutation() {
    let (state, revision, evidence) = fixture();
    let before_state = serde_json::to_value(&state).unwrap();
    let before_evidence = serde_json::to_value(&evidence).unwrap();
    let public = evaluate(&state, &revision, &evidence);
    let native = SoftwareIntelligenceRuntime::verification_status_from_snapshot(
        state.status(PLAN).unwrap(),
        &revision,
        &evidence,
    )
    .unwrap();
    assert!(public.ready);
    assert_eq!(
        serde_json::to_value(public).unwrap(),
        serde_json::to_value(native).unwrap()
    );
    assert_eq!(serde_json::to_value(&state).unwrap(), before_state);
    assert_eq!(serde_json::to_value(&evidence).unwrap(), before_evidence);
}

#[test]
fn public_snapshot_contract_does_not_complete_pending_reviewers() {
    let (mut state, revision, evidence) = fixture();
    let job = state.jobs.get_mut(JOB).unwrap();
    job.status = VerificationJobStatus::Queued;
    job.claimed_by = None;
    job.submission = None;
    let status = evaluate(&state, &revision, &evidence);
    assert!(!status.ready);
    assert_eq!(status.queued, 1);
    assert!(status.blockers.contains(&"reviewer-jobs-incomplete".into()));
}

#[test]
fn public_snapshot_contract_preserves_stale_partial_and_legacy_blockers() {
    let (state, revision, evidence) = fixture();
    let mut stale = revision.clone();
    stale.code = "sha256:changed".into();
    let status = evaluate(&state, &stale, &evidence);
    assert!(!status.ready);
    assert!(status
        .blockers
        .contains(&"workspace-revision-changed-since-plan".into()));
    let mut changed_design = revision.clone();
    changed_design.design = None;
    assert!(evaluate(&state, &changed_design, &evidence)
        .blockers
        .contains(&"design-revision-changed-since-plan".into()));

    let mut partial_state = state.clone();
    let mut partial_revision = revision.clone();
    partial_revision.code.push_str(":partial");
    let partial_subject = format!("change:{}", partial_revision.code);
    let plan = partial_state.plans.get_mut(PLAN).unwrap();
    plan.revision = Some(partial_revision.clone());
    plan.subject = partial_subject.clone();
    partial_state.jobs.get_mut(JOB).unwrap().subject = partial_subject.clone();
    let mut partial_evidence = evidence.clone();
    partial_evidence[0].revision = partial_revision.clone();
    partial_evidence[0].subject = partial_subject;
    let partial = evaluate(&partial_state, &partial_revision, &partial_evidence);
    assert_eq!(partial.deterministic_result, Some(EvidenceResult::Pass));
    assert!(!partial.ready);
    assert!(partial
        .blockers
        .contains(&"workspace-revision-incomplete".into()));
    assert!(partial
        .blockers
        .contains(&"verification-plan-revision-incomplete".into()));

    let mut legacy = state;
    legacy.plans.get_mut(PLAN).unwrap().revision = None;
    let status = evaluate(&legacy, &revision, &evidence);
    assert_eq!(status.deterministic_result, Some(EvidenceResult::Pass));
    assert!(!status.ready);
    assert!(status
        .blockers
        .contains(&"verification-plan-revision-unbound".into()));
}

#[test]
fn public_snapshot_contract_missing_or_skipped_check_is_not_pass() {
    let (state, revision, evidence) = fixture();
    let missing = evaluate(&state, &revision, &[]);
    assert!(!missing.ready);
    assert_eq!(missing.deterministic_result, None);
    assert!(missing
        .blockers
        .contains(&"deterministic-required-check-missing:rust-test".into()));

    let mut narrow = evidence.clone();
    let receipt = narrow[0].execution_receipt.as_mut().unwrap();
    receipt.required_checks.pop();
    receipt.checks.pop();
    let mapped = evaluate(&state, &revision, &narrow);
    assert!(!mapped.ready);
    assert_eq!(mapped.deterministic_result, None);
    assert!(mapped
        .blockers
        .contains(&"deterministic-required-check-missing:rust-test".into()));

    let mut skipped = evidence;
    let receipt = skipped[0].execution_receipt.as_mut().unwrap();
    receipt.checks.pop();
    receipt.skipped_checks.push("rust-test".into());
    skipped[0].result = EvidenceResult::Inconclusive;
    let status = evaluate(&state, &revision, &skipped);
    assert!(!status.ready);
    assert_eq!(
        status.deterministic_result,
        Some(EvidenceResult::Inconclusive)
    );
    assert!(status
        .blockers
        .contains(&"deterministic-required-check-skipped:rust-test".into()));
}

#[test]
fn public_snapshot_contract_enforces_plan_level_and_check_signatures() {
    let (mut state, revision, mut evidence) = fixture();
    state.plans.get_mut(PLAN).unwrap().deterministic_level = "full".into();
    let quick = evaluate(&state, &revision, &evidence);
    assert!(!quick.ready);
    assert!(quick
        .blockers
        .contains(&"deterministic-minimum-level-unproved:full".into()));
    state.plans.get_mut(PLAN).unwrap().deterministic_level = "quick".into();
    let receipt = evidence[0].execution_receipt.as_mut().unwrap();
    receipt.required_checks[1].signature = format!("sha256:{}", "a".repeat(64));
    receipt.checks[1].check = receipt.required_checks[1].clone();
    let changed = evaluate(&state, &revision, &evidence);
    assert!(!changed.ready);
    assert!(changed
        .blockers
        .contains(&"deterministic-required-check-missing:rust-test".into()));
}

#[test]
fn public_snapshot_contract_keeps_failure_over_narrower_or_equal_time_pass() {
    let (state, revision, evidence) = fixture();
    let mut failed = evidence[0].clone();
    failed.id = "EV-failed".into();
    failed.timestamp_ms = 7;
    failed.result = EvidenceResult::Fail;
    failed.execution_receipt.as_mut().unwrap().checks[1].result = EvidenceResult::Fail;
    let mut passed = evidence[0].clone();
    passed.id = "EV-narrow-pass".into();
    passed.timestamp_ms = 8;
    let receipt = passed.execution_receipt.as_mut().unwrap();
    receipt.required_checks.pop();
    receipt.checks.pop();
    let status = evaluate(&state, &revision, &[failed.clone(), passed]);
    assert!(!status.ready);
    assert_eq!(status.deterministic_result, Some(EvidenceResult::Fail));
    assert!(status
        .blockers
        .contains(&"deterministic-verification-failed".into()));

    let mut equal_pass = evidence[0].clone();
    equal_pass.id = "EV-equal-pass".into();
    equal_pass.timestamp_ms = 7;
    for records in [
        vec![failed.clone(), equal_pass.clone()],
        vec![equal_pass, failed],
    ] {
        assert_eq!(
            evaluate(&state, &revision, &records).deterministic_result,
            Some(EvidenceResult::Fail)
        );
    }
}

#[test]
fn public_snapshot_contract_self_reported_human_decision_is_not_authority() {
    let (mut state, revision, mut evidence) = fixture();
    let plan = state.plans.get_mut(PLAN).unwrap();
    plan.require_human_approval = true;
    let policy = format!(
        "human-approval/v1/sha256:{:x}",
        Sha256::digest(serde_json::to_vec(plan).unwrap())
    );
    let mut advisory = Evidence::new(
        "EV-advisory-approval".into(),
        plan.subject.clone(),
        EvidenceKind::HumanApproval,
        "self-reported:mcp:fixture".into(),
        revision.clone(),
        EvidenceResult::Pass,
        Confidence::High,
    )
    .unwrap();
    advisory.authority = EvidenceAuthority::SelfReported;
    advisory.policy = Some(policy);
    evidence.push(advisory);
    let status = evaluate(&state, &revision, &evidence);
    assert!(!status.human_approval);
    assert!(!status.ready);
    assert!(status.blockers.contains(&"human-approval-required".into()));
}

#[test]
fn public_snapshot_contract_advisory_or_wrong_target_stage_cannot_satisfy_plan() {
    let (mut state, revision, mut evidence) = fixture();
    let plan = state.plans.get_mut(PLAN).unwrap();
    plan.require_property = true;
    plan.stage_targets = vec!["language:rust".into(), "language:python".into()];
    let mut stage = Evidence::new(
        "EV-advisory-stage".into(),
        plan.subject.clone(),
        EvidenceKind::Property,
        "self-reported:mcp:fixture".into(),
        revision.clone(),
        EvidenceResult::Pass,
        Confidence::High,
    )
    .unwrap();
    stage.authority = EvidenceAuthority::SelfReported;
    stage.targets = plan.stage_targets.clone();
    evidence.push(stage);
    let advisory = evaluate(&state, &revision, &evidence);
    assert!(!advisory.ready);
    assert!(advisory
        .blockers
        .contains(&"property-target-missing:language:rust".into()));
    evidence[1].authority = EvidenceAuthority::NativeStage;
    evidence[1].targets = vec!["language:rust".into()];
    let partial = evaluate(&state, &revision, &evidence);
    assert!(!partial.ready);
    assert_eq!(
        partial.stage_target_results["property"]["language:rust"],
        EvidenceResult::Pass
    );
    assert!(partial
        .blockers
        .contains(&"property-target-missing:language:python".into()));
}

#[test]
fn public_snapshot_contract_rejects_corrupt_state_relations_and_statuses() {
    let (state, revision, evidence) = fixture();
    type StateMutation = fn(&mut VerificationState);
    let mutations: &[(&str, StateMutation)] = &[
        ("plan key", |s| {
            s.plans.get_mut(PLAN).unwrap().id = "VP-wrong".into();
        }),
        ("job key", |s| {
            s.jobs.get_mut(JOB).unwrap().id = "VJ-wrong".into();
        }),
        ("missing job", |s| {
            s.jobs.remove(JOB);
        }),
        ("duplicate job", |s| {
            let p = s.plans.get_mut(PLAN).unwrap();
            p.job_ids.push(JOB.into());
            p.reviewer_roles.push(ReviewerRole::Maintainability);
        }),
        ("duplicate role", |s| {
            let p = s.plans.get_mut(PLAN).unwrap();
            p.job_ids.push("VJ-other".into());
            p.reviewer_roles.push(ReviewerRole::Correctness);
        }),
        ("empty reviewer contract", |s| {
            let p = s.plans.get_mut(PLAN).unwrap();
            p.job_ids.clear();
            p.reviewer_roles.clear();
            s.jobs.clear();
        }),
        ("wrong plan", |s| {
            s.jobs.get_mut(JOB).unwrap().plan_id = "VP-other".into();
        }),
        ("wrong workspace", |s| {
            s.jobs.get_mut(JOB).unwrap().workspace = "other".into();
        }),
        ("wrong subject", |s| {
            s.jobs.get_mut(JOB).unwrap().subject = "change:other".into();
        }),
        ("wrong role", |s| {
            s.jobs.get_mut(JOB).unwrap().role = ReviewerRole::Security;
        }),
        ("not blind", |s| {
            s.jobs.get_mut(JOB).unwrap().blind = false;
        }),
        ("missing capability", |s| {
            s.jobs.get_mut(JOB).unwrap().required_capabilities.clear();
        }),
        ("submitted without submission", |s| {
            s.jobs.get_mut(JOB).unwrap().submission = None;
        }),
        ("submitted without actor", |s| {
            s.jobs.get_mut(JOB).unwrap().claimed_by = None;
        }),
        ("invalid submission", |s| {
            s.jobs
                .get_mut(JOB)
                .unwrap()
                .submission
                .as_mut()
                .unwrap()
                .summary
                .clear();
        }),
        ("queued with claim", |s| {
            s.jobs.get_mut(JOB).unwrap().status = VerificationJobStatus::Queued;
        }),
        ("claimed with submission", |s| {
            s.jobs.get_mut(JOB).unwrap().status = VerificationJobStatus::Claimed;
        }),
        ("orphan job", |s| {
            let mut job = s.jobs[JOB].clone();
            job.id = "VJ-orphan".into();
            s.jobs.insert(job.id.clone(), job);
        }),
    ];
    for (label, mutate) in mutations {
        let mut invalid = state.clone();
        mutate(&mut invalid);
        assert!(
            evaluate_snapshot(&invalid, WORKSPACE, PLAN, &revision, &evidence).is_err(),
            "{label}"
        );
    }
}

#[test]
fn public_snapshot_contract_rejects_workspace_and_plan_mismatch() {
    let (state, revision, evidence) = fixture();
    assert!(evaluate_snapshot(&state, "other", PLAN, &revision, &evidence).is_err());
    assert!(evaluate_snapshot(&state, WORKSPACE, "VP-missing", &revision, &evidence).is_err());
}

#[test]
fn public_snapshot_contract_rejects_conflicting_evidence_identity() {
    let (state, revision, evidence) = fixture();
    let mut conflict = evidence[0].clone();
    conflict.summary = Some("Different contents under one immutable id.".into());
    assert!(evaluate_snapshot(
        &state,
        WORKSPACE,
        PLAN,
        &revision,
        &[evidence[0].clone(), conflict]
    )
    .is_err());
    assert!(
        evaluate(
            &state,
            &revision,
            &[evidence[0].clone(), evidence[0].clone()]
        )
        .ready
    );
    let mut invalid = evidence[0].clone();
    invalid.execution_receipt.as_mut().unwrap().required_checks[0].signature = "invalid".into();
    assert!(evaluate_snapshot(&state, WORKSPACE, PLAN, &revision, &[invalid]).is_err());
}

#[test]
fn public_snapshot_contract_rejects_bounded_count_record_and_state_overflow() {
    let (state, revision, evidence) = fixture();
    let count = vec![evidence[0].clone(); MAX_EVIDENCE_RECORDS + 1];
    assert!(
        evaluate_snapshot(&state, WORKSPACE, PLAN, &revision, &count)
            .unwrap_err()
            .to_string()
            .contains("count")
    );
    let mut record = evidence[0].clone();
    record.claims = vec!["字".repeat(1_000); 32];
    record.risks = record.claims.clone();
    record.validate().unwrap();
    assert!(
        evaluate_snapshot(&state, WORKSPACE, PLAN, &revision, &[record])
            .unwrap_err()
            .to_string()
            .contains("record")
    );
    let mut huge = state;
    huge.plans.get_mut(PLAN).unwrap().policy = "x".repeat(MAX_STATE_BYTES);
    assert!(
        evaluate_snapshot(&huge, WORKSPACE, PLAN, &revision, &evidence)
            .unwrap_err()
            .to_string()
            .contains("state")
    );
}

#[test]
fn public_snapshot_contract_rejects_aggregate_evidence_budget() {
    let (state, revision, evidence) = fixture();
    let mut record = evidence[0].clone();
    record.claims = vec!["x".repeat(1_000); 32];
    record.risks = record.claims.clone();
    record.validate().unwrap();
    let bytes = serialized_size(&record, MAX_EVIDENCE_BYTES).unwrap();
    let count = MAX_EVIDENCE_TOTAL_BYTES / bytes + 1;
    assert!(count < MAX_EVIDENCE_RECORDS);
    let records = (0..count)
        .map(|index| {
            let mut item = record.clone();
            item.id = format!("EV-budget-{index}");
            item
        })
        .collect::<Vec<_>>();
    assert!(
        evaluate_snapshot(&state, WORKSPACE, PLAN, &revision, &records)
            .unwrap_err()
            .to_string()
            .contains("total")
    );
}
