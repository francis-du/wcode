use super::*;
use std::fs;

fn approval_plan(workspace_id: &str, revision: Revision) -> ReconciliationPlan {
    ReconciliationPlan {
        id: "RP-runtime-approval".into(),
        workspace: workspace_id.into(),
        risk_level: RiskLevel::Low,
        design_changes: vec![],
        drift_ids: vec![],
        impacted_components: vec!["component:runtime".into()],
        impacted_symbols: vec!["src/lib.rs::entry".into()],
        impacted_tests: vec!["tests/runtime.rs::entry".into()],
        impacted_acceptance: vec!["AC-RUNTIME-001".into()],
        implementation_tasks: vec![ReconciliationTask {
            id: "RT-runtime".into(),
            kind: ReconciliationTaskKind::Implementation,
            subject: "src/lib.rs".into(),
            description: "Apply the approved runtime change.".into(),
            write_scopes: vec![],
            depends_on: vec![],
        }],
        change_intents: vec![],
        verification_plan: VerificationPlan {
            id: "VP-runtime-approval".into(),
            workspace: workspace_id.into(),
            subject: "change:runtime-approval".into(),
            revision: Some(revision),
            risk_level: RiskLevel::Low,
            policy: "risk-adaptive/v1/low".into(),
            deterministic_level: "quick".into(),
            deterministic_checks: vec!["cargo check --locked".into()],
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
fn runtime_requires_approval_and_rejects_pre_start_revision_drift() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn entry() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let workspace_id = "demo";
    let revision = runtime.current_revision(&workspace).unwrap();
    let plan = approval_plan(workspace_id, revision.clone());
    reconciliation_store::persist(&workspace, &plan).unwrap();

    let missing = runtime
        .approved_reconciliation_snapshot(workspace_id, &workspace, &plan.id)
        .unwrap_err()
        .to_string();
    assert!(missing.contains("plan_approval_required"));

    let approval = runtime
        .reconciliation_approve(
            workspace_id,
            &workspace,
            &plan.id,
            "human:test",
            "I reviewed and approve this exact reconciliation plan.",
        )
        .unwrap();
    assert_eq!(approval["approved"], true);

    let snapshot = runtime
        .approved_reconciliation_snapshot(workspace_id, &workspace, &plan.id)
        .unwrap();
    assert_eq!(snapshot.plan, plan);
    assert_eq!(snapshot.approved_revision, revision);

    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn entry() { println!(\"changed\"); }\n",
    )
    .unwrap();
    let drift = runtime
        .approved_reconciliation_snapshot(workspace_id, &workspace, &plan.id)
        .unwrap_err()
        .to_string();
    assert!(drift.contains("replan_required"));
}

#[test]
fn runtime_claim_is_gated_by_approval_and_then_uses_the_frozen_plan() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn entry() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let workspace_id = "demo";
    let revision = runtime.current_revision(&workspace).unwrap();
    let mut verification_state = VerificationState::default();
    let verification_plan = verification_state
        .create_plan(
            "VP-runtime-approval".into(),
            workspace_id.into(),
            "change:runtime-approval".into(),
            VerificationPlanBinding {
                revision: revision.clone(),
                stage_targets: vec![],
                automation_gaps: vec![],
            },
            RiskLevel::Low,
            (0..32).map(|index| format!("VJ-runtime-{index}")),
        )
        .unwrap();
    verification_store::persist(&workspace, &verification_state).unwrap();
    let mut plan = approval_plan(workspace_id, revision);
    plan.verification_plan = verification_plan;
    reconciliation_store::persist(&workspace, &plan).unwrap();
    let execution = ReconciliationExecution::from_plan(&plan).unwrap();
    reconciliation_execution_store::persist(&workspace, &execution).unwrap();

    let denied = runtime
        .reconciliation_claim(workspace_id, &workspace, &plan.id, "writer", &[], None)
        .unwrap_err()
        .to_string();
    assert!(denied.contains("plan_approval_required"));

    runtime
        .reconciliation_approve(
            workspace_id,
            &workspace,
            &plan.id,
            "human:test",
            "Approve model execution of this exact frozen plan.",
        )
        .unwrap();
    let claimed = runtime
        .reconciliation_claim(workspace_id, &workspace, &plan.id, "writer", &[], None)
        .unwrap();
    assert_eq!(claimed.task.id, "RT-runtime");
    assert_eq!(claimed.claimed_by.as_deref(), Some("writer"));

    // The approved baseline revision is expected to advance once implementation
    // starts. Submission must continue from the frozen Plan rather than treating
    // the writer's own planned edit as automatic pre-start drift.
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn entry() { println!(\"implemented\"); }\n",
    )
    .unwrap();
    let submitted = runtime
        .reconciliation_submit(
            workspace_id,
            &workspace,
            &plan.id,
            &claimed.task.id,
            "writer",
            ReconciliationTaskSubmission {
                success: true,
                summary: "Applied the approved runtime change.".into(),
                artifact_digest: None,
            },
        )
        .unwrap();
    assert_eq!(submitted.status, ReconciliationRunStatus::Completed);
    let approval = runtime
        .reconciliation_approval_status(workspace_id, &workspace, &plan.id)
        .unwrap();
    assert_eq!(approval["state"], "approved_active");
    assert_eq!(approval["replan_required"], false);
}

#[test]
fn current_reconciliation_verification_never_downgrades_required_risk() {
    let current = Revision {
        code: "code-current".into(),
        design: Some("design-current".into()),
    };
    let mut approved = approval_plan("demo", current.clone());
    approved.risk_level = RiskLevel::Medium;

    let mut state = VerificationState::default();
    let mut create = |id: &str, risk: RiskLevel| {
        state
            .create_plan(
                id.into(),
                "demo".into(),
                format!("change:{id}"),
                VerificationPlanBinding {
                    revision: current.clone(),
                    stage_targets: vec![],
                    automation_gaps: vec![],
                },
                risk,
                (0..64).map(|index| format!("VJ-{id}-{index}")),
            )
            .unwrap()
    };
    let low = create("low", RiskLevel::Low);
    let medium = create("medium", RiskLevel::Medium);
    let high = create("high", RiskLevel::High);
    let mut low_status = state.status(&low.id).unwrap();
    low_status.ready = true;
    let mut medium_status = state.status(&medium.id).unwrap();
    medium_status.ready = true;
    let high_status = state.status(&high.id).unwrap();
    let history = vec![low_status, medium_status, high_status];

    let selected = select_current_reconciliation_verification(&approved, &current, &history)
        .expect("high-risk current verification should be selected");
    assert_eq!(selected.plan.risk_level, RiskLevel::High);
    assert!(
        !selected.ready,
        "lower-risk ready proof must not bypass a higher-risk gate"
    );
}
