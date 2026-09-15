use super::*;
use crate::reconcile::{
    ReconciliationExecution, ReconciliationPlan, ReconciliationTask, ReconciliationTaskKind,
};
use crate::risk::RiskLevel;
use crate::verification::VerificationPlan;

#[test]
fn execution_state_survives_a_fresh_load() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let plan = ReconciliationPlan {
        id: "RP-exec".into(),
        workspace: "demo".into(),
        risk_level: RiskLevel::Low,
        design_changes: vec![],
        drift_ids: vec![],
        impacted_components: vec![],
        impacted_symbols: vec![],
        impacted_tests: vec![],
        implementation_tasks: vec![ReconciliationTask {
            id: "RT-1".into(),
            kind: ReconciliationTaskKind::Implementation,
            subject: "component:a".into(),
            description: "Implement change.".into(),
            depends_on: vec![],
        }],
        change_intents: vec![],
        verification_plan: VerificationPlan {
            id: "VP-exec".into(),
            workspace: "demo".into(),
            subject: "change:fixture".into(),
            revision: None,
            risk_level: RiskLevel::Low,
            policy: "risk-adaptive/v1/low".into(),
            deterministic_level: "quick".into(),
            deterministic_checks: vec![],
            reviewer_roles: vec![],
            require_property: false,
            require_mutation: false,
            require_fuzz: false,
            require_human_approval: false,
            stage_targets: vec![],
            automation_gaps: vec![],
            job_ids: vec![],
        },
    };
    let execution = ReconciliationExecution::from_plan(&plan).unwrap();
    persist(&workspace, &execution).unwrap();
    assert_eq!(
        load(&workspace, &plan.id).unwrap().unwrap().plan_id,
        plan.id
    );
}

#[test]
fn batch_load_uses_one_exact_latest_snapshot_per_plan() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let plan = ReconciliationPlan {
        id: "RP-exec".into(),
        workspace: "demo".into(),
        risk_level: RiskLevel::Low,
        design_changes: vec![],
        drift_ids: vec![],
        impacted_components: vec![],
        impacted_symbols: vec![],
        impacted_tests: vec![],
        implementation_tasks: vec![ReconciliationTask {
            id: "RT-1".into(),
            kind: ReconciliationTaskKind::Implementation,
            subject: "component:a".into(),
            description: "Implement change.".into(),
            depends_on: vec![],
        }],
        change_intents: vec![],
        verification_plan: VerificationPlan {
            id: "VP-exec".into(),
            workspace: "demo".into(),
            subject: "change:fixture".into(),
            revision: None,
            risk_level: RiskLevel::Low,
            policy: "risk-adaptive/v1/low".into(),
            deterministic_level: "quick".into(),
            deterministic_checks: vec![],
            reviewer_roles: vec![],
            require_property: false,
            require_mutation: false,
            require_fuzz: false,
            require_human_approval: false,
            stage_targets: vec![],
            automation_gaps: vec![],
            job_ids: vec![],
        },
    };
    let first = ReconciliationExecution::from_plan(&plan).unwrap();
    persist(&workspace, &first).unwrap();
    let mut newer = first.clone();
    newer.updated_at_ms = newer.updated_at_ms.saturating_add(1);
    persist(&workspace, &newer).unwrap();

    let mut overlapping = plan.clone();
    overlapping.id = "RP-exec-long".into();
    overlapping.verification_plan.id = "VP-exec-long".into();
    let overlapping_execution = ReconciliationExecution::from_plan(&overlapping).unwrap();
    persist(&workspace, &overlapping_execution).unwrap();

    let ids = vec![plan.id.clone(), overlapping.id.clone()];
    let loaded = load_many(&workspace, &ids).unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[&plan.id].updated_at_ms, newer.updated_at_ms);
    assert_eq!(loaded[&overlapping.id].plan_id, overlapping.id);
}
