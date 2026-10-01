//! Trusted pure-snapshot fixtures, not Evidence of repository check execution.
use super::*;
use crate::design::PolicyRequirements;
use crate::evidence::{
    Confidence, EvidenceAuthority, EvidenceKind, VerificationCheckReceipt,
    VerificationExecutionReceipt,
};
use crate::verification::{
    change::{ExecutionGitBinding, GitChangeAuthority, GitChangeTarget},
    PolicyPlanRequirements, ReviewSubmission, ReviewVerdict, ReviewerRole, VerificationJobStatus,
    VerificationPlanBinding, VerificationStage,
};

fn timing_fixture() -> Fixture {
    let mut fixture = Fixture::new();
    let mut single = fixture.evidence[0].clone();
    single.id = "EV-timing".into();
    single.kind = EvidenceKind::Compiler;
    single.timestamp_ms = 2;
    let receipt = single.execution_receipt.as_mut().unwrap();
    receipt.checks.truncate(1);
    receipt.required_checks.truncate(1);
    single.subject = format!("verification:{}", receipt.checks[0].check.id);
    single.summary = Some(crate::report_types::verification_metrics_summary(42, 0));
    fixture.evidence.push(single);
    fixture
}

#[test]
fn acceptance_timing_projects_only_bound_native_single_checks_and_preserves_gate_semantics() {
    let mut fixture = timing_fixture();
    // Identical text on an aggregate receipt is not a second measured check.
    fixture.evidence[0].summary = fixture.evidence[1].summary.clone();
    let measured = fixture.evaluate();
    assert_eq!(measured.state, AcceptanceState::Ready);
    let timed = measured
        .evidence
        .iter()
        .filter_map(|e| e.execution_timing.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(timed.len(), 1);
    assert_eq!(timed[0].execution_ms, 42);
    assert_eq!(timed[0].check_id, "rust-check");
    let mut input = fixture.input();
    input.now_ms = 101;
    assert_eq!(
        evaluate_acceptance(input).unwrap().digest(),
        measured.digest()
    );
    fixture.evidence[1].summary = None;
    let legacy = fixture.evaluate();
    assert_eq!(legacy.state, measured.state);
    assert_eq!(legacy.summary.executed, measured.summary.executed);
    assert_eq!(legacy.summary.passed, measured.summary.passed);
    assert!(legacy.evidence.iter().all(|e| e.execution_timing.is_none()));
    let value = serde_json::to_value(&legacy).unwrap();
    assert!(value["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e.get("execution_timing").is_none()));
}

#[test]
fn acceptance_timing_rejects_malformed_or_noncanonical_native_summaries() {
    for summary in [
        "",
        "PRIVATE-UNTRUSTED-OUTPUT",
        "verification-metrics-v1;elapsed_ms=-1;phase=0",
        "verification-metrics-v1;elapsed_ms=+1;phase=0",
        "verification-metrics-v1;elapsed_ms=01;phase=0",
        "verification-metrics-v1;elapsed_ms=18446744073709551616;phase=0",
        "verification-metrics-v1;elapsed_ms=1;phase=256",
        "verification-metrics-v1;elapsed_ms=1;phase=0;phase=1",
        "verification-metrics-v1;elapsed_ms=1;phase=0\n",
        "verification-metrics-v1;phase=0;elapsed_ms=1",
    ] {
        let mut fixture = timing_fixture();
        fixture.evidence[1].summary = Some(summary.into());
        // Empty/control-containing metadata may be rejected earlier by the
        // canonical Evidence validator. Otherwise no timing may be projected.
        match evaluate_acceptance(fixture.input()) {
            Ok(record) => {
                assert!(
                    record.evidence.iter().all(|e| e.execution_timing.is_none()),
                    "{summary}"
                );
                assert!(!serde_json::to_string(&record)
                    .unwrap()
                    .contains("PRIVATE-UNTRUSTED-OUTPUT"));
            }
            Err(_) => assert!(summary.is_empty() || summary.chars().any(char::is_control)),
        }
    }
    let mut fixture = timing_fixture();
    fixture.evidence[1].summary = Some(crate::report_types::verification_metrics_summary(0, 0));
    assert_eq!(
        fixture
            .evaluate()
            .evidence
            .iter()
            .find_map(|e| e.execution_timing.as_ref())
            .unwrap()
            .execution_ms,
        0
    );
}

#[test]
fn acceptance_timing_excludes_reused_stale_advisory_and_nonexecuted_receipts() {
    for variant in [
        "reused",
        "revision",
        "git",
        "policy",
        "advisory",
        "future",
        "unavailable",
    ] {
        let mut fixture = timing_fixture();
        let evidence = &mut fixture.evidence[1];
        match variant {
            "reused" => {
                evidence.execution_receipt.as_mut().unwrap().checks[0].reused_from =
                    Some("EV-previous".into())
            }
            "revision" => evidence.revision.code = hash('9'),
            "git" => {
                evidence.execution_git_binding = None;
                evidence
                    .execution_receipt
                    .as_mut()
                    .unwrap()
                    .execution_git_binding = None;
            }
            "policy" => {
                evidence.execution_policy_binding = Some("project-policy/v1/99/other".into())
            }
            "advisory" => evidence.authority = EvidenceAuthority::SelfReported,
            "future" => evidence.timestamp_ms = 101,
            _ => {
                evidence.result = EvidenceResult::Inconclusive;
                let item = &mut evidence.execution_receipt.as_mut().unwrap().checks[0];
                item.execution = crate::evidence::VerificationCheckExecution::Unavailable;
                item.result = EvidenceResult::Inconclusive;
            }
        }
        let record = fixture.evaluate();
        assert!(
            record.evidence.iter().all(|e| e.execution_timing.is_none()),
            "{variant}"
        );
    }
}

const WORKSPACE: &str = "acceptance-fixture";
const PLAN: &str = "VP-acceptance";
const JOB: &str = "VJ-acceptance";

fn hash(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

struct Fixture {
    state: VerificationState,
    revision: Revision,
    git: GitChangeSnapshot,
    policy: AcceptancePolicyBinding,
    discovery: AcceptanceDiscovery,
    evidence: Vec<Evidence>,
    risk: RiskLevel,
}

impl Fixture {
    fn new() -> Self {
        let revision = Revision {
            code: hash('a'),
            design: Some(hash('b')),
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
        let requirements = PolicyRequirements {
            minimum_level: PolicyLevel::Quick,
            checks: required.iter().map(|check| check.id.clone()).collect(),
            stages: vec![],
            reviewers: vec![],
            human_approval: false,
            human_approval_min_risk: None,
        };
        let policy = AcceptancePolicyBinding {
            workspace: WORKSPACE.into(),
            root_digest: hash('c'),
            generation: 1,
            current_generation: 1,
            snapshot_digest: hash('d'),
            current_snapshot_digest: hash('d'),
            source_seal_digest: hash('e'),
            current_source_seal_digest: hash('e'),
            selection: PolicySelection {
                policy_id: "fixture-policy".into(),
                policy_version: 1,
                policy_digest: hash('f'),
                docs_only: false,
                matched_rules: vec![],
                requirements: requirements.clone(),
            },
            required_checks: required.clone(),
            expires_at_ms: None,
        };
        let mut state = VerificationState::default();
        state
            .create_plan_with_policy(
                PLAN.into(),
                WORKSPACE.into(),
                format!("change:{}", revision.code),
                VerificationPlanBinding {
                    revision: revision.clone(),
                    stage_targets: vec![],
                    automation_gaps: vec![],
                    required_checks: Some(required.clone()),
                },
                PolicyPlanRequirements {
                    risk_level: RiskLevel::Low,
                    requirements,
                    policy_binding: policy.plan_policy(),
                },
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
        let binding = ExecutionGitBinding {
            repository: hash('1'),
            head_sha: "2".repeat(40),
            tree_sha: "3".repeat(40),
            dirty: false,
            index_fingerprint: hash('4'),
        };
        let git = GitChangeSnapshot {
            requested_base: "5".repeat(40),
            target: GitChangeTarget::Commit {
                revision: binding.head_sha.clone(),
            },
            base_sha: Some("5".repeat(40)),
            target_sha: Some(binding.head_sha.clone()),
            binding: Some(binding.clone()),
            changes: vec![],
            complete: true,
            executable_changes: Some(false),
            regular_file_changes: Some(false),
            unknown_reasons: vec![],
            authority: GitChangeAuthority::MetadataOnly,
        };
        let mut evidence = Evidence::new(
            "EV-aggregate".into(),
            format!("change:{}", revision.code),
            EvidenceKind::Verification,
            "fixture-native-runner".into(),
            revision.clone(),
            EvidenceResult::Pass,
            Confidence::Deterministic,
        )
        .unwrap();
        evidence.authority = EvidenceAuthority::NativeVerification;
        evidence.policy = Some("deterministic/quick/v2".into());
        evidence.timestamp_ms = 1;
        evidence.execution_policy_binding = Some(policy.plan_policy());
        evidence.execution_git_binding = Some(binding.clone());
        evidence.execution_receipt = Some(VerificationExecutionReceipt {
            schema_version: 1,
            level: "quick".into(),
            required_checks: required.clone(),
            checks: required
                .iter()
                .cloned()
                .map(|check| VerificationCheckReceipt {
                    check,
                    result: EvidenceResult::Pass,
                    execution: crate::evidence::VerificationCheckExecution::Executed,
                    reused_from: None,
                })
                .collect(),
            skipped_checks: vec![],
            execution_git_binding: Some(binding),
        });
        Self {
            state,
            revision,
            git,
            policy,
            discovery: AcceptanceDiscovery {
                complete: true,
                mappings_complete: true,
                checks: required,
            },
            evidence: vec![evidence],
            risk: RiskLevel::Low,
        }
    }

    fn input(&self) -> AcceptanceInput<'_> {
        AcceptanceInput {
            workspace: WORKSPACE,
            current_root_digest: &self.policy.root_digest,
            revision: &self.revision,
            git: &self.git,
            policy_state: AcceptancePolicyState::Active,
            policy: Some(&self.policy),
            verification: &self.state,
            plan_id: Some(PLAN),
            evidence: &self.evidence,
            discovery: &self.discovery,
            risk_level: self.risk,
            now_ms: 100,
        }
    }

    fn evaluate(&self) -> ChangeAcceptanceRecord {
        evaluate_acceptance(self.input()).unwrap()
    }
    fn receipt(&mut self) -> &mut VerificationExecutionReceipt {
        self.evidence[0].execution_receipt.as_mut().unwrap()
    }
    fn human(&self, authority: EvidenceAuthority, result: EvidenceResult) -> Evidence {
        let plan = &self.state.plans[PLAN];
        let mut record = Evidence::new(
            "EV-human".into(),
            plan.subject.clone(),
            EvidenceKind::HumanApproval,
            "fixture-operator".into(),
            self.revision.clone(),
            result,
            Confidence::High,
        )
        .unwrap();
        record.authority = authority;
        record.policy = Some(format!(
            "human-approval/v1/sha256:{:x}",
            Sha256::digest(serde_json::to_vec(plan).unwrap())
        ));
        record.execution_git_binding = self.git.binding.clone();
        record.execution_policy_binding = Some(self.policy.plan_policy());
        record.timestamp_ms = 2;
        record
    }
}

fn has(record: &ChangeAcceptanceRecord, code: &str) -> bool {
    record.reasons.iter().any(|reason| reason.code == code)
}

#[test]
fn acceptance_ready_reuses_native_engine_and_has_stable_fact_digest() {
    let fixture = Fixture::new();
    let before = serde_json::to_value(&fixture.state).unwrap();
    let first = fixture.evaluate();
    assert_eq!(first.state, AcceptanceState::Ready);
    assert_eq!(first.summary.required, 2);
    assert_eq!(first.summary.passed, 2);
    assert_eq!(
        first.evidence[0].authority,
        EvidenceAuthority::NativeVerification
    );
    let mut later = fixture.input();
    later.now_ms = 101;
    let second = evaluate_acceptance(later).unwrap();
    assert_ne!(first.captured_at_ms, second.captured_at_ms);
    assert_eq!(first.digest(), second.digest());
    assert_eq!(first.id, second.id);
    assert_eq!(serde_json::to_value(&fixture.state).unwrap(), before);
}

#[test]
fn acceptance_skipped_required_check_cannot_pass() {
    let mut fixture = Fixture::new();
    fixture.receipt().checks.pop();
    fixture.receipt().skipped_checks.push("rust-test".into());
    fixture.evidence[0].result = EvidenceResult::Inconclusive;
    let record = fixture.evaluate();
    assert_eq!(record.state, AcceptanceState::Incomplete);
    assert!(has(&record, "required_check_skipped"));
    let row = record
        .checks
        .iter()
        .find(|row| row.id == "rust-test")
        .unwrap();
    assert_eq!(row.execution, AcceptanceExecution::Skipped);
    assert_ne!(row.outcome, AcceptanceOutcome::Pass);
}

#[test]
fn acceptance_old_revision_cannot_approve_new_commit() {
    let mut fixture = Fixture::new();
    fixture
        .state
        .plans
        .get_mut(PLAN)
        .unwrap()
        .require_human_approval = true;
    fixture
        .evidence
        .push(fixture.human(EvidenceAuthority::LocalOperator, EvidenceResult::Pass));
    fixture.revision.code = hash('6');
    fixture.git.binding.as_mut().unwrap().head_sha = "7".repeat(40);
    fixture.git.target_sha = Some("7".repeat(40));
    fixture.git.target = GitChangeTarget::Commit {
        revision: "7".repeat(40),
    };
    let record = fixture.evaluate();
    assert_eq!(record.state, AcceptanceState::Stale);
    assert!(has(&record, "plan_revision_changed"));
    assert!(has(&record, "human_approval_binding_missing"));
    assert_eq!(record.verification.unwrap().human_approval, None);
}

#[test]
fn acceptance_same_code_new_git_identity_does_not_reuse_old_receipts() {
    let mut fixture = Fixture::new();
    fixture.git.binding.as_mut().unwrap().index_fingerprint = hash('6');
    let record = fixture.evaluate();
    assert_eq!(record.state, AcceptanceState::Stale);
    assert_eq!(record.summary.passed, 0);
    assert!(record
        .checks
        .iter()
        .all(|row| row.freshness == AcceptanceFreshness::Stale));
    assert_eq!(record.verification.unwrap().deterministic_result, None);
}

#[test]
fn acceptance_native_failure_cannot_be_overridden_by_model_or_human() {
    let mut fixture = Fixture::new();
    fixture.receipt().checks[0].result = EvidenceResult::Fail;
    fixture.evidence[0].result = EvidenceResult::Fail;
    fixture
        .evidence
        .push(fixture.human(EvidenceAuthority::LocalOperator, EvidenceResult::Pass));
    let mut reported = fixture.evidence[0].clone();
    reported.id = "EV-self-report".into();
    reported.authority = EvidenceAuthority::SelfReported;
    reported.result = EvidenceResult::Pass;
    reported.execution_receipt.as_mut().unwrap().checks[0].result = EvidenceResult::Pass;
    reported.timestamp_ms = 99;
    fixture.evidence.push(reported);
    let record = fixture.evaluate();
    assert_eq!(record.state, AcceptanceState::Blocked);
    assert!(has(&record, "native_check_failed"));
    assert_eq!(record.summary.failed, 1);
}

#[test]
fn acceptance_policy_generation_definition_expiry_and_workspace_fail_closed() {
    let mut fixture = Fixture::new();
    fixture.policy.current_generation = 2;
    assert_eq!(fixture.evaluate().state, AcceptanceState::Stale);
    fixture.policy.current_generation = 1;
    fixture.policy.current_source_seal_digest = hash('6');
    assert!(has(&fixture.evaluate(), "policy_definition_changed"));
    fixture.policy.current_source_seal_digest = fixture.policy.source_seal_digest.clone();
    fixture.policy.expires_at_ms = Some(100);
    assert!(has(&fixture.evaluate(), "policy_expired"));
    fixture.policy.expires_at_ms = None;
    fixture.policy.workspace = "different-workspace".into();
    assert!(has(&fixture.evaluate(), "policy_workspace_mismatch"));
}

#[test]
fn acceptance_inactive_policy_and_missing_plan_are_inspectable_incomplete() {
    let fixture = Fixture::new();
    let mut input = fixture.input();
    input.policy_state = AcceptancePolicyState::Inactive;
    input.policy = None;
    input.plan_id = None;
    let record = evaluate_acceptance(input).unwrap();
    assert_eq!(record.state, AcceptanceState::Incomplete);
    assert!(has(&record, "policy_inactive"));
    assert!(has(&record, "verification_plan_missing"));
    assert!(record.policy.is_none() && record.plan.is_none());
}

#[test]
fn acceptance_discovered_mapped_and_executed_are_independent_axes() {
    let mut fixture = Fixture::new();
    fixture.discovery.checks.pop();
    fixture
        .policy
        .selection
        .requirements
        .checks
        .push("unknown-required".into());
    let record = fixture.evaluate();
    let unknown = record
        .checks
        .iter()
        .find(|row| row.id == "unknown-required")
        .unwrap();
    assert!(unknown.required);
    assert!(unknown.signature.is_none());
    assert!(!unknown.discovered && !unknown.mapped);
    assert_eq!(unknown.execution, AcceptanceExecution::Unavailable);
    let existing = record
        .checks
        .iter()
        .find(|row| row.id == "rust-test")
        .unwrap();
    assert!(!existing.discovered && existing.mapped);
    assert_eq!(existing.execution, AcceptanceExecution::Executed);
    assert_ne!(record.state, AcceptanceState::Ready);
}

#[test]
fn acceptance_partial_discovery_mapping_git_and_revision_never_ready() {
    let mut fixture = Fixture::new();
    fixture.discovery.complete = false;
    fixture.discovery.mappings_complete = false;
    fixture.git.complete = false;
    fixture.revision.code.push_str(":partial");
    let record = fixture.evaluate();
    assert!(record.partial);
    assert!(has(&record, "revision_incomplete"));
    assert!(has(&record, "git_capture_incomplete"));
    assert!(has(&record, "discovery_incomplete"));
    assert!(has(&record, "mapping_incomplete"));
    assert_ne!(record.state, AcceptanceState::Ready);
}

#[test]
fn acceptance_quick_execution_does_not_prove_full_required_strength() {
    let mut fixture = Fixture::new();
    fixture.policy.selection.requirements.minimum_level = PolicyLevel::Full;
    fixture
        .state
        .plans
        .get_mut(PLAN)
        .unwrap()
        .deterministic_level = "full".into();
    let record = fixture.evaluate();
    assert_eq!(record.state, AcceptanceState::Incomplete);
    assert!(has(&record, "minimum_level_unproved"));
    assert!(record
        .checks
        .iter()
        .all(|row| row.level.as_deref() == Some("quick") && row.required_level == "full"));
}

#[test]
fn acceptance_pending_reviewers_are_needs_review_not_completed() {
    let mut fixture = Fixture::new();
    let job = fixture.state.jobs.get_mut(JOB).unwrap();
    job.status = VerificationJobStatus::Queued;
    job.claimed_by = None;
    job.submission = None;
    let record = fixture.evaluate();
    assert_eq!(record.state, AcceptanceState::NeedsReview);
    assert_eq!(record.verification.unwrap().queued, 1);
}

#[test]
fn acceptance_human_self_report_and_unbound_legacy_are_not_approval() {
    let mut fixture = Fixture::new();
    fixture
        .state
        .plans
        .get_mut(PLAN)
        .unwrap()
        .require_human_approval = true;
    fixture
        .evidence
        .push(fixture.human(EvidenceAuthority::SelfReported, EvidenceResult::Pass));
    let record = fixture.evaluate();
    assert_eq!(record.state, AcceptanceState::NeedsReview);
    assert_eq!(record.verification.unwrap().human_approval, None);
    let mut native = fixture.human(EvidenceAuthority::LocalOperator, EvidenceResult::Pass);
    native.id = "EV-unbound".into();
    native.execution_git_binding = None;
    fixture.evidence.push(native);
    let record = fixture.evaluate();
    assert!(has(&record, "human_approval_binding_missing"));
    assert_eq!(record.verification.unwrap().human_approval, None);
}

#[test]
fn acceptance_native_human_rejection_is_distinct_from_missing_review() {
    let mut fixture = Fixture::new();
    fixture
        .state
        .plans
        .get_mut(PLAN)
        .unwrap()
        .require_human_approval = true;
    fixture
        .evidence
        .push(fixture.human(EvidenceAuthority::LocalOperator, EvidenceResult::Fail));
    let record = fixture.evaluate();
    assert_eq!(
        record.verification.unwrap().human_approval,
        Some(EvidenceResult::Fail)
    );
    assert_ne!(record.state, AcceptanceState::Ready);
}

#[test]
fn acceptance_required_stage_needs_native_exact_git_policy_and_target() {
    let mut fixture = Fixture::new();
    fixture.state.plans.get_mut(PLAN).unwrap().require_property = true;
    fixture.state.plans.get_mut(PLAN).unwrap().stage_targets = vec!["component:fixture".into()];
    let mut stage = Evidence::new(
        "EV-stage".into(),
        format!("change:{}", fixture.revision.code),
        EvidenceKind::Property,
        "fixture-stage-runner".into(),
        fixture.revision.clone(),
        EvidenceResult::Pass,
        Confidence::High,
    )
    .unwrap();
    stage.authority = EvidenceAuthority::NativeStage;
    stage.policy =
        Some(format!("{}/stage/property", fixture.policy.plan_policy()).to_ascii_lowercase());
    stage.targets = vec!["component:fixture".into()];
    stage.execution_git_binding = fixture.git.binding.clone();
    stage.execution_policy_binding = Some(fixture.policy.plan_policy());
    fixture.evidence.push(stage);
    assert_eq!(fixture.evaluate().state, AcceptanceState::Ready);
    fixture.evidence.last_mut().unwrap().execution_git_binding = None;
    let unbound = fixture.evaluate();
    assert!(has(&unbound, "native_stage_binding_missing"));
    assert!(unbound.verification.unwrap().stage_results.is_empty());
}

#[test]
fn acceptance_cached_receipt_requires_real_matching_source_evidence() {
    let mut fixture = Fixture::new();
    fixture.receipt().checks[0].reused_from = Some("EV-missing".into());
    let record = fixture.evaluate();
    assert_eq!(record.state, AcceptanceState::Incomplete);
    assert!(has(&record, "cached_proof_source_missing"));
    assert_ne!(record.checks[0].outcome, AcceptanceOutcome::Pass);
}

#[test]
fn acceptance_legacy_plan_and_legacy_receipts_remain_readable_not_ready() {
    let mut fixture = Fixture::new();
    fixture.state.plans.get_mut(PLAN).unwrap().revision = None;
    fixture.evidence[0].authority = EvidenceAuthority::LegacyUnknown;
    let record = fixture.evaluate();
    assert!(has(&record, "plan_revision_unbound"));
    assert_ne!(record.state, AcceptanceState::Ready);
    assert_eq!(record.summary.passed, 0);
}

#[test]
fn acceptance_invalid_duplicate_and_oversized_inputs_return_error() {
    let mut fixture = Fixture::new();
    let mut conflicting = fixture.evidence[0].clone();
    conflicting.producer = "different-content".into();
    fixture.evidence.push(conflicting);
    assert!(evaluate_acceptance(fixture.input())
        .unwrap_err()
        .to_string()
        .contains("conflicting"));
    fixture.evidence.pop();
    fixture.state.jobs.get_mut(JOB).unwrap().id = "different-map-key".into();
    assert!(evaluate_acceptance(fixture.input()).is_err());
    let mut fixture = Fixture::new();
    fixture.evidence = vec![fixture.evidence[0].clone(); 4097];
    assert!(evaluate_acceptance(fixture.input())
        .unwrap_err()
        .to_string()
        .contains("4096"));
}

#[test]
fn acceptance_plan_policy_union_preserves_risk_floor_and_adds_review_jobs() {
    let fixture = Fixture::new();
    let mut state = VerificationState::default();
    let mut requirements = fixture.policy.selection.requirements.clone();
    requirements.reviewers = vec![ReviewerRole::Performance];
    requirements.stages = vec![VerificationStage::RuntimeCanary];
    requirements.human_approval = true;
    let plan = state
        .create_plan_with_policy(
            "VP-high".into(),
            WORKSPACE.into(),
            format!("change:{}", fixture.revision.code),
            VerificationPlanBinding {
                revision: fixture.revision.clone(),
                stage_targets: vec![],
                automation_gaps: vec![],
                required_checks: Some(fixture.policy.required_checks.clone()),
            },
            PolicyPlanRequirements {
                risk_level: RiskLevel::High,
                requirements,
                policy_binding: fixture.policy.plan_policy(),
            },
            (0..9).map(|index| format!("VJ-high-{index}")),
        )
        .unwrap();
    assert_eq!(plan.deterministic_level, "full");
    assert!(
        plan.require_property
            && plan.require_mutation
            && plan.require_fuzz
            && plan.require_human_approval
    );
    assert!(plan.deterministic_checks.contains(&"runtime-gate".into()));
    assert!(plan.reviewer_roles.contains(&ReviewerRole::Performance));
    assert!(plan.reviewer_roles.contains(&ReviewerRole::Security));
    assert_eq!(plan.job_ids.len(), plan.reviewer_roles.len());
    assert_eq!(plan.policy, fixture.policy.plan_policy());
}

#[test]
fn acceptance_plan_policy_invalid_or_insufficient_jobs_does_not_mutate_state() {
    let fixture = Fixture::new();
    for unknown in [true, false] {
        let mut state = VerificationState::default();
        let before = serde_json::to_value(&state).unwrap();
        let mut requirements = fixture.policy.selection.requirements.clone();
        if unknown {
            requirements.checks.push("not-discovered".into());
        } else {
            requirements.reviewers.push(ReviewerRole::Performance);
        }
        let result = state.create_plan_with_policy(
            "VP-invalid".into(),
            WORKSPACE.into(),
            format!("change:{}", fixture.revision.code),
            VerificationPlanBinding {
                revision: fixture.revision.clone(),
                stage_targets: vec![],
                automation_gaps: vec![],
                required_checks: Some(fixture.policy.required_checks.clone()),
            },
            PolicyPlanRequirements {
                risk_level: RiskLevel::Low,
                requirements,
                policy_binding: fixture.policy.plan_policy(),
            },
            ["VJ-only-one".into()].into_iter(),
        );
        assert!(result.is_err());
        assert_eq!(serde_json::to_value(&state).unwrap(), before);
    }
}

#[test]
fn acceptance_plan_generation_overflow_is_atomic() {
    let fixture = Fixture::new();
    let mut state = VerificationState {
        generation: u64::MAX,
        ..VerificationState::default()
    };
    let before = serde_json::to_value(&state).unwrap();
    assert!(state
        .create_plan_with_policy(
            "VP-overflow".into(),
            WORKSPACE.into(),
            format!("change:{}", fixture.revision.code),
            VerificationPlanBinding {
                revision: fixture.revision.clone(),
                stage_targets: vec![],
                automation_gaps: vec![],
                required_checks: Some(fixture.policy.required_checks.clone())
            },
            PolicyPlanRequirements {
                risk_level: RiskLevel::Low,
                requirements: fixture.policy.selection.requirements.clone(),
                policy_binding: fixture.policy.plan_policy()
            },
            ["VJ-overflow".into()].into_iter(),
        )
        .is_err());
    assert_eq!(serde_json::to_value(&state).unwrap(), before);
}
#[test]
fn acceptance_unknown_unavailable_and_timeout_receipts_do_not_prove_execution() {
    use crate::evidence::VerificationCheckExecution;
    for execution in [
        VerificationCheckExecution::Unknown,
        VerificationCheckExecution::Unavailable,
        VerificationCheckExecution::TimedOut,
    ] {
        let mut fixture = Fixture::new();
        fixture.receipt().checks[0].execution = execution;
        fixture.evidence[0].result = EvidenceResult::Inconclusive;
        let record = fixture.evaluate();
        let row = record
            .checks
            .iter()
            .find(|row| row.id == "rust-check")
            .unwrap();
        assert_ne!(row.execution, AcceptanceExecution::Executed);
        assert_ne!(row.outcome, AcceptanceOutcome::Pass);
        assert_eq!(record.state, AcceptanceState::Incomplete);
    }
}

#[test]
fn acceptance_receipt_from_old_policy_generation_is_stale_not_current_pass() {
    let mut fixture = Fixture::new();
    fixture.evidence[0].execution_policy_binding =
        Some(format!("project-policy/v1/2/{}", hash('d')));
    let record = fixture.evaluate();
    assert_eq!(record.state, AcceptanceState::Stale);
    assert_eq!(record.summary.passed, 0);
    assert!(record
        .checks
        .iter()
        .all(|row| row.freshness == AcceptanceFreshness::Stale));
    fixture.evidence[0].execution_policy_binding = None;
    let record = fixture.evaluate();
    assert_eq!(record.state, AcceptanceState::Incomplete);
    assert!(record
        .checks
        .iter()
        .all(|row| row.freshness == AcceptanceFreshness::Unbound));
}

#[test]
fn acceptance_stage_summary_does_not_report_pass_for_missing_required_target() {
    let mut fixture = Fixture::new();
    let plan = fixture.state.plans.get_mut(PLAN).unwrap();
    plan.require_property = true;
    plan.stage_targets = vec!["component:a".into(), "component:b".into()];
    let mut stage = Evidence::new(
        "EV-one-target".into(),
        format!("change:{}", fixture.revision.code),
        EvidenceKind::Property,
        "fixture-runner".into(),
        fixture.revision.clone(),
        EvidenceResult::Pass,
        Confidence::High,
    )
    .unwrap();
    stage.authority = EvidenceAuthority::NativeStage;
    stage.execution_git_binding = fixture.git.binding.clone();
    stage.execution_policy_binding = Some(fixture.policy.plan_policy());
    stage.policy =
        Some(format!("{}/stage/property", fixture.policy.plan_policy()).to_ascii_lowercase());
    stage.targets = vec!["component:a".into()];
    fixture.evidence.push(stage);
    let record = fixture.evaluate();
    assert_ne!(record.state, AcceptanceState::Ready);
    assert!(!record
        .verification
        .unwrap()
        .stage_results
        .contains_key("property"));
}

#[test]
fn acceptance_legacy_receipt_execution_defaults_unknown_without_upgrading_pass() {
    let fixture = Fixture::new();
    let mut serialized = serde_json::to_value(&fixture.evidence[0]).unwrap();
    let receipt = serialized["execution_receipt"].as_object_mut().unwrap();
    for item in receipt["checks"].as_array_mut().unwrap() {
        item.as_object_mut().unwrap().remove("execution");
    }
    let legacy: Evidence = serde_json::from_value(serialized).unwrap();
    assert_eq!(
        legacy.execution_receipt.as_ref().unwrap().checks[0].execution,
        crate::evidence::VerificationCheckExecution::Unknown
    );
    assert_eq!(
        legacy.execution_receipt.as_ref().unwrap().result(),
        EvidenceResult::Inconclusive
    );
    let mut fixture = fixture;
    fixture.evidence[0] = legacy;
    assert_ne!(fixture.evaluate().state, AcceptanceState::Ready);
}
