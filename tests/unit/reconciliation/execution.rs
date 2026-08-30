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
            risk_level: RiskLevel::Low,
            policy: "risk-adaptive/v1/low".into(),
            deterministic_level: "quick".into(),
            deterministic_checks: vec![],
            reviewer_roles: vec![],
            require_property: false,
            require_mutation: false,
            require_fuzz: false,
            require_human_approval: false,
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
