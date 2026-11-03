use super::*;
use crate::core_types::ExecutionGitBinding;
use crate::design::{PolicyLevel, PolicyRequirements, PolicySelection};
use crate::evidence::RequiredVerificationCheck;
use crate::git_provider::{
    CheckAnnotation, CheckObservation, CheckReceipt, CHECK_RECEIPT_SCHEMA_VERSION,
};
use crate::risk::RiskLevel;
use crate::verification::acceptance::{
    evaluate_acceptance, gate_evidence_ids, AcceptanceDiscovery, AcceptancePolicyBinding,
    AcceptancePolicyState, AcceptanceState,
};
use crate::verification::change::{GitChangeAuthority, GitChangeSnapshot, GitChangeTarget};
use crate::verification::{PolicyPlanRequirements, VerificationPlanBinding, VerificationState};

const WORKSPACE: &str = "receipt-fixture";
const PLAN: &str = "VP-receipt";
const JOB: &str = "VJ-receipt";

fn sha(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn git_binding() -> ExecutionGitBinding {
    ExecutionGitBinding {
        repository: sha('1'),
        head_sha: "2".repeat(40),
        tree_sha: "3".repeat(40),
        dirty: false,
        index_fingerprint: sha('4'),
    }
}

fn receipt_binding() -> ReceiptBinding {
    ReceiptBinding {
        repository: "francis-du/wcode".into(),
        repository_id: 1234,
        app_id: 7,
        check_name: "wcode/change-acceptance".into(),
    }
}

fn check_receipt(head_sha: &str) -> CheckReceipt {
    let check = CheckObservation {
        id: 9,
        head_sha: head_sha.to_owned(),
        name: "wcode/change-acceptance".into(),
        source_id: "github-app:7".into(),
        external_id: "wcode:test".into(),
        status: "completed".into(),
        conclusion: Some("success".into()),
    };
    let annotations = vec![CheckAnnotation {
        path: "src/lib.rs".into(),
        start_line: Some(1),
        end_line: Some(2),
        annotation_level: "warning".into(),
        title: "t".into(),
        message: "m".into(),
    }];
    let digest = crate::git_provider::github::check_receipt_digest(&check, &annotations);
    CheckReceipt {
        schema_version: CHECK_RECEIPT_SCHEMA_VERSION,
        provider: "github".into(),
        repository: "francis-du/wcode".into(),
        repository_id: 1234,
        app_id: 7,
        check,
        annotations,
        digest,
    }
}

fn revision() -> Revision {
    Revision {
        code: sha('a'),
        design: None,
    }
}

#[test]
fn valid_ci_receipt_import_is_observation_only() {
    let git = git_binding();
    let receipt = check_receipt(&git.head_sha);
    let evidence = import_check_receipt(&receipt, &receipt_binding(), &git, &revision()).unwrap();
    assert_eq!(evidence.kind, EvidenceKind::CiReceipt);
    assert_eq!(evidence.authority, EvidenceAuthority::ExternalReceipt);
    assert_eq!(evidence.confidence, Confidence::Medium);
    assert_eq!(evidence.execution_receipt, None);
    assert_eq!(evidence.result, EvidenceResult::Pass);
    assert!(evidence.external_receipt.is_some());
    let policy = evidence.policy.as_deref().unwrap();
    assert!(!policy.starts_with("deterministic/"));
    assert!(!policy.starts_with("acceptance/"));
    assert_eq!(evidence.subject, format!("change:{}", revision().code));
    assert_eq!(evidence.execution_git_binding, Some(git));
    assert_eq!(evidence.validate(), Ok(()));
}

#[test]
fn ci_receipt_result_mapping_is_fail_closed_for_incomplete_runs() {
    let git = git_binding();
    let binding = receipt_binding();
    let revision = revision();

    let mut failed = check_receipt(&git.head_sha);
    failed.check.conclusion = Some("failure".into());
    failed.digest =
        crate::git_provider::github::check_receipt_digest(&failed.check, &failed.annotations);
    let evidence = import_check_receipt(&failed, &binding, &git, &revision).unwrap();
    assert_eq!(evidence.result, EvidenceResult::Fail);

    let mut queued = check_receipt(&git.head_sha);
    queued.check.status = "in_progress".into();
    queued.check.conclusion = None;
    queued.digest =
        crate::git_provider::github::check_receipt_digest(&queued.check, &queued.annotations);
    let evidence = import_check_receipt(&queued, &binding, &git, &revision).unwrap();
    assert_eq!(evidence.result, EvidenceResult::Inconclusive);
}

#[test]
fn ci_receipt_import_rejects_binding_digest_and_bound_mismatches() {
    let git = git_binding();
    let binding = receipt_binding();
    let revision = revision();

    let mut digest = check_receipt(&git.head_sha);
    digest.annotations[0].message = "tampered".into();
    assert!(import_check_receipt(&digest, &binding, &git, &revision).is_err());

    let wrong_head = check_receipt(&"9".repeat(40));
    assert!(import_check_receipt(&wrong_head, &binding, &git, &revision).is_err());

    let mut repository = check_receipt(&git.head_sha);
    repository.repository = "other/repo".into();
    assert!(import_check_receipt(&repository, &binding, &git, &revision).is_err());

    let mut app = check_receipt(&git.head_sha);
    app.app_id = 8;
    assert!(import_check_receipt(&app, &binding, &git, &revision).is_err());

    let mut provider = check_receipt(&git.head_sha);
    provider.provider = "gitlab".into();
    assert!(import_check_receipt(&provider, &binding, &git, &revision).is_err());

    let mut oversized = check_receipt(&git.head_sha);
    oversized.annotations[0].path = "x".repeat(513);
    oversized.digest =
        crate::git_provider::github::check_receipt_digest(&oversized.check, &oversized.annotations);
    assert!(import_check_receipt(&oversized, &binding, &git, &revision).is_err());

    let mut reversed = check_receipt(&git.head_sha);
    reversed.annotations[0].start_line = Some(9);
    reversed.annotations[0].end_line = Some(2);
    reversed.digest =
        crate::git_provider::github::check_receipt_digest(&reversed.check, &reversed.annotations);
    assert!(import_check_receipt(&reversed, &binding, &git, &revision).is_err());
}

struct AcceptanceFixture {
    state: VerificationState,
    revision: Revision,
    git: GitChangeSnapshot,
    policy: AcceptancePolicyBinding,
    discovery: AcceptanceDiscovery,
}

impl AcceptanceFixture {
    fn new(revision: &Revision, binding: &ExecutionGitBinding) -> Self {
        let required = vec![RequiredVerificationCheck::from_command(
            "rust-check",
            "cargo",
            &["check".into()],
            ".",
            "root",
        )];
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
            root_digest: sha('c'),
            generation: 1,
            current_generation: 1,
            snapshot_digest: sha('d'),
            current_snapshot_digest: sha('d'),
            source_seal_digest: sha('e'),
            current_source_seal_digest: sha('e'),
            selection: PolicySelection {
                policy_id: "fixture-policy".into(),
                policy_version: 1,
                policy_digest: sha('f'),
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
        Self {
            state,
            revision: revision.clone(),
            git,
            policy,
            discovery: AcceptanceDiscovery {
                complete: true,
                mappings_complete: true,
                checks: required,
            },
        }
    }

    fn input<'a>(
        &'a self,
        evidence: &'a [Evidence],
    ) -> crate::verification::acceptance::AcceptanceInput<'a> {
        crate::verification::acceptance::AcceptanceInput {
            workspace: WORKSPACE,
            current_root_digest: &self.policy.root_digest,
            revision: &self.revision,
            git: &self.git,
            policy_state: AcceptancePolicyState::Active,
            policy: Some(&self.policy),
            verification: &self.state,
            plan_id: Some(PLAN),
            evidence,
            discovery: &self.discovery,
            risk_level: RiskLevel::Low,
            now_ms: 100,
        }
    }
}

#[test]
fn imported_ci_receipt_cannot_satisfy_a_stage_or_reach_acceptance() {
    let git = git_binding();
    let receipt = check_receipt(&git.head_sha);
    let revision = revision();
    let evidence = import_check_receipt(&receipt, &receipt_binding(), &git, &revision).unwrap();

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
                required_checks: None,
            },
            RiskLevel::Medium,
            [JOB.into(), "VJ-receipt-2".into()].into_iter(),
        )
        .unwrap();
    let status = crate::verification::evaluate_snapshot(
        &state,
        WORKSPACE,
        PLAN,
        &revision,
        std::slice::from_ref(&evidence),
    )
    .unwrap();
    assert!(!status.stage_results.contains_key("property"));
    assert!(status
        .blockers
        .iter()
        .any(|blocker| blocker == "property-evidence-missing"));
    assert!(!status.ready);

    let fixture = AcceptanceFixture::new(&revision, &git);
    let input = fixture.input(std::slice::from_ref(&evidence));
    let gate_status = fixture.state.status(PLAN).unwrap();
    assert!(!gate_evidence_ids(&input, Some(&gate_status)).contains(&evidence.id));
    let record = evaluate_acceptance(input).unwrap();
    assert_ne!(record.state, AcceptanceState::Ready);
    assert!(!record
        .evidence
        .iter()
        .any(|summary| summary.id == evidence.id));
}
