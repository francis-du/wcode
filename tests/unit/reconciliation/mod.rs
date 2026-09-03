use super::*;

#[test]
fn change_ir_is_structured_and_not_a_text_diff() {
    let intent = ChangeIntent::ChangeBehavior {
        target: "workspace.command_policy".into(),
        desired: serde_json::json!({"allow":"cargo test --locked"}),
        constraints: vec!["must not allow arbitrary cargo arguments".into()],
    };
    let value = serde_json::to_value(intent).unwrap();
    assert_eq!(value["kind"], "change_behavior");
    assert!(value.get("diff").is_none());
}

#[test]
fn plan_validation_rejects_dependency_cycles() {
    let plan = ReconciliationPlan {
        id: "RP-cycle".into(),
        workspace: "wcode".into(),
        risk_level: RiskLevel::Medium,
        design_changes: Vec::new(),
        drift_ids: Vec::new(),
        impacted_components: Vec::new(),
        impacted_symbols: Vec::new(),
        impacted_tests: Vec::new(),
        implementation_tasks: vec![
            ReconciliationTask {
                id: "a".into(),
                kind: ReconciliationTaskKind::Design,
                subject: "REQ-a".into(),
                description: "task a".into(),
                depends_on: vec!["b".into()],
            },
            ReconciliationTask {
                id: "b".into(),
                kind: ReconciliationTaskKind::Implementation,
                subject: "component:b".into(),
                description: "task b".into(),
                depends_on: vec!["a".into()],
            },
        ],
        change_intents: Vec::new(),
        verification_plan: VerificationPlan {
            id: "VP-cycle".into(),
            workspace: "wcode".into(),
            subject: "REQ-a".into(),
            revision: None,
            risk_level: RiskLevel::Medium,
            policy: "medium".into(),
            deterministic_level: "full".into(),
            deterministic_checks: Vec::new(),
            reviewer_roles: Vec::new(),
            require_property: false,
            require_mutation: false,
            require_fuzz: false,
            require_human_approval: false,
            stage_targets: Vec::new(),
            automation_gaps: Vec::new(),
            job_ids: Vec::new(),
        },
    };

    assert_eq!(plan.validate(), Err(ReconciliationError::InvalidPlan));
}

#[test]
fn execution_enforces_dependencies_retries_and_system_evidence_gates() {
    let plan = ReconciliationPlan {
        id: "RP-test".into(),
        workspace: "wcode".into(),
        risk_level: RiskLevel::High,
        design_changes: Vec::new(),
        drift_ids: Vec::new(),
        impacted_components: Vec::new(),
        impacted_symbols: Vec::new(),
        impacted_tests: Vec::new(),
        implementation_tasks: vec![
            ReconciliationTask {
                id: "design".into(),
                kind: ReconciliationTaskKind::Design,
                subject: "REQ-test".into(),
                description: "Update desired state".into(),
                depends_on: Vec::new(),
            },
            ReconciliationTask {
                id: "implementation".into(),
                kind: ReconciliationTaskKind::Implementation,
                subject: "component:test".into(),
                description: "Implement desired state".into(),
                depends_on: vec!["design".into()],
            },
            ReconciliationTask {
                id: "verification".into(),
                kind: ReconciliationTaskKind::Verification,
                subject: "VP-test".into(),
                description: "Wait for verification evidence".into(),
                depends_on: vec!["implementation".into()],
            },
        ],
        change_intents: Vec::new(),
        verification_plan: VerificationPlan {
            id: "VP-test".into(),
            workspace: "wcode".into(),
            subject: "REQ-test".into(),
            revision: None,
            risk_level: RiskLevel::High,
            policy: "high".into(),
            deterministic_level: "full".into(),
            deterministic_checks: Vec::new(),
            reviewer_roles: Vec::new(),
            require_property: false,
            require_mutation: false,
            require_fuzz: false,
            require_human_approval: false,
            stage_targets: Vec::new(),
            automation_gaps: Vec::new(),
            job_ids: Vec::new(),
        },
    };
    let mut execution = ReconciliationExecution::from_plan(&plan).unwrap();

    assert!(!execution.set_system_task(
        ReconciliationTaskKind::Verification,
        true,
        "verification passed".into(),
    ));
    assert_eq!(execution.status().blocked, 2);

    let design = execution.claim("builder", &[]).unwrap();
    assert_eq!(design.task.id, "design");
    execution
        .submit(
            "design",
            "builder",
            ReconciliationTaskSubmission {
                success: true,
                summary: "design updated".into(),
                artifact_digest: Some("sha256:design".into()),
            },
        )
        .unwrap();

    let implementation = execution.claim("builder", &[]).unwrap();
    assert_eq!(implementation.task.id, "implementation");
    execution
        .submit(
            "implementation",
            "builder",
            ReconciliationTaskSubmission {
                success: false,
                summary: "first implementation failed".into(),
                artifact_digest: None,
            },
        )
        .unwrap();
    assert_eq!(execution.status().failed, 1);

    execution.retry("implementation").unwrap();
    let implementation = execution.claim("builder", &[]).unwrap();
    assert_eq!(implementation.task.id, "implementation");
    execution
        .submit(
            "implementation",
            "builder",
            ReconciliationTaskSubmission {
                success: true,
                summary: "implementation completed".into(),
                artifact_digest: Some("sha256:implementation".into()),
            },
        )
        .unwrap();

    assert!(execution.set_system_task(
        ReconciliationTaskKind::Verification,
        true,
        "verification passed".into(),
    ));
    let status = execution.status();
    assert_eq!(status.completed, 3);
    assert_eq!(status.blocked, 0);
    assert!(status.converged);
}
