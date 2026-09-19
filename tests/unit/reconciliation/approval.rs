use super::*;
use crate::risk::RiskLevel;
use crate::verification::VerificationPlan;
use crate::workspace::Workspace;

fn revision(code: &str, design: &str) -> Revision {
    Revision {
        code: code.to_owned(),
        design: Some(design.to_owned()),
    }
}

fn plan(bound: Revision) -> ReconciliationPlan {
    ReconciliationPlan {
        id: "RP-approved".into(),
        workspace: "demo".into(),
        risk_level: RiskLevel::Medium,
        design_changes: vec![],
        drift_ids: vec![],
        impacted_components: vec!["component:core".into()],
        impacted_symbols: vec!["src/lib.rs::entry".into()],
        impacted_tests: vec!["tests/core.rs::entry_works".into()],
        impacted_acceptance: vec!["AC-CORE-001".into(), "AC-CORE-001".into()],
        implementation_tasks: vec![],
        change_intents: vec![],
        verification_plan: VerificationPlan {
            id: "VP-approved".into(),
            workspace: "demo".into(),
            subject: "change:approved".into(),
            revision: Some(bound),
            risk_level: RiskLevel::Medium,
            policy: "risk-adaptive/v1/medium".into(),
            deterministic_level: "full".into(),
            deterministic_checks: vec!["cargo test --locked".into()],
            reviewer_roles: vec![],
            require_property: false,
            require_mutation: false,
            require_fuzz: false,
            require_human_approval: false,
            stage_targets: vec![],
            automation_gaps: vec![],
            job_ids: vec![],
        },
    }
}

#[test]
fn explicit_plan_approval_freezes_digest_revision_and_references() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let approved_revision = revision("code-a", "design-a");
    let plan = plan(approved_revision.clone());

    let snapshot = approve(
        &workspace,
        &plan,
        &approved_revision,
        "human:test",
        "I reviewed and approve this exact implementation plan.",
    )
    .unwrap();

    assert_eq!(snapshot.plan_digest, digest_plan(&plan).unwrap());
    assert_eq!(snapshot.approved_revision, approved_revision);
    assert_eq!(snapshot.acceptance_refs, vec!["AC-CORE-001"]);
    assert_eq!(
        snapshot.verification_refs,
        vec!["VP-approved", "tests/core.rs::entry_works"]
    );
    assert_eq!(snapshot.plan, plan);

    let reloaded = load(&workspace, "RP-approved").unwrap().unwrap();
    assert_eq!(reloaded, snapshot);
    assert_eq!(
        reloaded.plan.verification_plan.revision,
        Some(revision("code-a", "design-a"))
    );
}

#[test]
fn claim_requires_approval_and_pre_start_revision_drift_requires_replan() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let approved_revision = revision("code-a", "design-a");
    let plan = plan(approved_revision.clone());

    let missing = require(&workspace, &plan.id).unwrap_err().to_string();
    assert!(missing.contains("plan_approval_required"));

    let snapshot = approve(
        &workspace,
        &plan,
        &approved_revision,
        "human:test",
        "Approve the frozen plan.",
    )
    .unwrap();
    let error = check_pre_start_revision(&snapshot, &revision("code-b", "design-a"), false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("replan_required"));
}

#[test]
fn started_execution_keeps_using_the_frozen_plan_after_expected_code_revision_changes() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let approved_revision = revision("code-a", "design-a");
    let original = plan(approved_revision.clone());
    let snapshot = approve(
        &workspace,
        &original,
        &approved_revision,
        "human:test",
        "Approve the frozen plan.",
    )
    .unwrap();

    check_pre_start_revision(
        &snapshot,
        &revision("code-after-plan-edit", "design-a"),
        true,
    )
    .unwrap();

    let mut live_plan = original.clone();
    live_plan
        .impacted_components
        .push("component:unexpected".into());
    assert_ne!(digest_plan(&live_plan).unwrap(), snapshot.plan_digest);

    let reloaded = require(&workspace, &original.id).unwrap();
    assert_eq!(reloaded.plan_digest, snapshot.plan_digest);
    assert_eq!(reloaded.plan, original);
    assert!(!reloaded
        .plan
        .impacted_components
        .contains(&"component:unexpected".to_owned()));
}
