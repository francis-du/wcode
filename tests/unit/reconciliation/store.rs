use super::*;
use crate::risk::RiskLevel;
use crate::verification::VerificationPlan;

#[test]
fn reconciliation_plan_round_trips_from_persistent_store() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let plan = ReconciliationPlan {
        id: "RP-fixture".into(),
        workspace: "demo".into(),
        risk_level: RiskLevel::Low,
        design_changes: vec![],
        drift_ids: vec![],
        impacted_components: vec![],
        impacted_symbols: vec![],
        impacted_tests: vec![],
        implementation_tasks: vec![],
        change_intents: vec![],
        verification_plan: VerificationPlan {
            id: "VP-fixture".into(),
            workspace: "demo".into(),
            subject: "change:fixture".into(),
            risk_level: RiskLevel::Low,
            policy: "risk-adaptive/v1/low".into(),
            deterministic_level: "quick".into(),
            deterministic_checks: vec!["compile".into()],
            reviewer_roles: vec![],
            require_property: false,
            require_mutation: false,
            require_fuzz: false,
            require_human_approval: false,
            automation_gaps: vec![],
            job_ids: vec![],
        },
    };
    persist(&workspace, &plan).unwrap();
    assert_eq!(load(&workspace, &plan.id).unwrap().unwrap().id, plan.id);
    assert_eq!(recent(&workspace, 10).unwrap().len(), 1);
}
