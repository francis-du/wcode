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
        impacted_acceptance: vec![],
        implementation_tasks: vec![ReconciliationTask {
            id: "RT-1".into(),
            kind: ReconciliationTaskKind::Implementation,
            subject: "component:a".into(),
            description: "Implement change.".into(),
            write_scopes: vec![],
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
        impacted_acceptance: vec![],
        implementation_tasks: vec![ReconciliationTask {
            id: "RT-1".into(),
            kind: ReconciliationTaskKind::Implementation,
            subject: "component:a".into(),
            description: "Implement change.".into(),
            write_scopes: vec![],
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

#[test]
fn concurrent_scoped_claim_transactions_preserve_both_writers_and_monotonic_revision() {
    use crate::reconcile::ReconciliationRunStatus;
    use std::sync::{Arc, Barrier};
    use std::thread;

    let dir = tempfile::tempdir().unwrap();
    let workspace = Arc::new(Workspace::new(dir.path(), false, false).unwrap());
    let plan = ReconciliationPlan {
        id: "RP-concurrent-writers".into(),
        workspace: "demo".into(),
        risk_level: RiskLevel::Low,
        design_changes: vec![],
        drift_ids: vec![],
        impacted_components: vec![],
        impacted_symbols: vec![],
        impacted_tests: vec![],
        impacted_acceptance: vec![],
        implementation_tasks: vec![
            ReconciliationTask {
                id: "RT-src".into(),
                kind: ReconciliationTaskKind::Implementation,
                subject: "src".into(),
                description: "Edit source lane.".into(),
                write_scopes: vec!["src".into()],
                depends_on: vec![],
            },
            ReconciliationTask {
                id: "RT-tests".into(),
                kind: ReconciliationTaskKind::Implementation,
                subject: "tests".into(),
                description: "Edit test lane.".into(),
                write_scopes: vec!["tests".into()],
                depends_on: vec![],
            },
        ],
        change_intents: vec![],
        verification_plan: VerificationPlan {
            id: "VP-concurrent-writers".into(),
            workspace: "demo".into(),
            subject: "change:concurrent-writers".into(),
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
    let initial = ReconciliationExecution::from_plan(&plan).unwrap();
    let initial_revision = initial.updated_at_ms;
    persist(&workspace, &initial).unwrap();

    let barrier = Arc::new(Barrier::new(3));
    let mut joins = Vec::new();
    for (task_id, executor) in [("RT-src", "writer-src"), ("RT-tests", "writer-tests")] {
        let workspace = workspace.clone();
        let barrier = barrier.clone();
        let plan_id = plan.id.clone();
        joins.push(thread::spawn(move || {
            barrier.wait();
            update_existing(&workspace, &plan_id, |execution| {
                Ok(execution.claim_task(
                    executor,
                    &[ReconciliationTaskKind::Implementation],
                    Some(task_id),
                )?)
            })
            .unwrap();
        }));
    }
    barrier.wait();
    for join in joins {
        join.join().unwrap();
    }

    let loaded = load(&workspace, &plan.id).unwrap().unwrap();
    assert!(loaded.updated_at_ms >= initial_revision.saturating_add(2));
    for (task_id, executor) in [("RT-src", "writer-src"), ("RT-tests", "writer-tests")] {
        let run = loaded
            .tasks
            .iter()
            .find(|run| run.task.id == task_id)
            .unwrap();
        assert_eq!(run.status, ReconciliationRunStatus::Claimed);
        assert_eq!(run.claimed_by.as_deref(), Some(executor));
    }
}
