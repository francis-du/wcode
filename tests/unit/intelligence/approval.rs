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
            required_checks: None,
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
                required_checks: None,
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
        .reconciliation_claim_owned(
            workspace_id,
            &workspace,
            &plan.id,
            "writer",
            &[],
            crate::reconcile::ReconciliationClaimSelection::default(),
        )
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
        .reconciliation_claim_owned(
            workspace_id,
            &workspace,
            &plan.id,
            "writer",
            &[],
            crate::reconcile::ReconciliationClaimSelection::default(),
        )
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
                    required_checks: None,
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

#[test]
fn verification_approval_cannot_authorize_another_plan_or_stale_revision() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("change.rs"), "pub fn original() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let first = runtime
        .create_plan_for_risk("demo", &workspace, RiskLevel::Critical)
        .unwrap();
    let second = runtime
        .create_plan_for_risk("demo", &workspace, RiskLevel::Critical)
        .unwrap();
    assert_eq!(first.revision, second.revision);
    assert_eq!(first.subject, second.subject);
    let approval = runtime
        .verification_approve_authorized(
            "demo",
            &workspace,
            &first.id,
            "local_operator:test",
            "I approve this exact plan; this does not attest test execution.",
        )
        .unwrap();
    assert!(
        runtime
            .verification_status("demo", &workspace, &first.id)
            .unwrap()
            .human_approval
    );
    assert!(
        !runtime
            .verification_status("demo", &workspace, &second.id)
            .unwrap()
            .human_approval
    );
    assert_eq!(
        approval.policy,
        Some(verification_snapshot::human_approval_policy(&first).unwrap())
    );
    let mut changed_policy = first.clone();
    changed_policy.policy.push_str("/revised");
    assert_ne!(
        verification_snapshot::human_approval_policy(&first).unwrap(),
        verification_snapshot::human_approval_policy(&changed_policy).unwrap()
    );
    let mut changed_checks = first.clone();
    changed_checks
        .deterministic_checks
        .push("new-required-check".into());
    assert_ne!(
        verification_snapshot::human_approval_policy(&first).unwrap(),
        verification_snapshot::human_approval_policy(&changed_checks).unwrap()
    );

    fs::write(root.path().join("change.rs"), "pub fn changed() {}\n").unwrap();
    let error = runtime
        .verification_approve_authorized(
            "demo",
            &workspace,
            &second.id,
            "local_operator:test",
            "Approve the old plan.",
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("stale verification plan"));
    let current = runtime.current_revision(&workspace).unwrap();
    assert!(evidence_store::load(&workspace)
        .unwrap()
        .iter()
        .all(|record| record.kind != EvidenceKind::HumanApproval || record.revision != current));
}

#[test]
fn legacy_revision_only_human_approval_remains_advisory() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let plan = runtime
        .create_plan_for_risk("demo", &workspace, RiskLevel::Critical)
        .unwrap();
    let mut legacy = Evidence::new(
        "EV-legacy-human".into(),
        plan.subject.clone(),
        EvidenceKind::HumanApproval,
        "human:unbound".into(),
        plan.revision.clone().unwrap(),
        EvidenceResult::Pass,
        Confidence::High,
    )
    .unwrap();
    legacy.policy = Some(format!("{}/human-approval", plan.policy));
    evidence_store::persist(&workspace, &legacy).unwrap();
    let status = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert!(!status.human_approval);
    assert!(status
        .blockers
        .contains(&"human-approval-required".to_owned()));
}

#[tokio::test]
async fn legacy_stage_authority_cannot_satisfy_an_otherwise_ready_plan() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new_with_security(
        root.path(),
        false,
        true,
        crate::workspace::WorkspaceSecurity {
            allow_risky_exec: true,
            ..Default::default()
        },
    )
    .unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let revision = runtime.current_revision(&workspace).unwrap();
    let binding = RequiredVerificationCheck::from_command(
        "rust-check",
        "cargo",
        &["check".into(), "--locked".into()],
        ".",
        "workspace",
    );
    let mut state = VerificationState::default();
    let plan = state
        .create_plan(
            "VP-authority".into(),
            "demo".into(),
            format!("change:{}", revision.code),
            VerificationPlanBinding {
                revision: revision.clone(),
                required_checks: Some(vec![binding.clone()]),
                stage_targets: vec!["language:rust".into(), "language:javascript".into()],
                automation_gaps: vec![],
            },
            RiskLevel::Medium,
            (0..32).map(|index| format!("VJ-authority-{index}")),
        )
        .unwrap();
    for (index, queued) in state.status(&plan.id).unwrap().jobs.iter().enumerate() {
        let reviewer = format!("reviewer-{index}");
        let capabilities = queued.required_capabilities.iter().cloned().collect();
        let claimed = state
            .claim("demo", &reviewer, &capabilities, Some(queued.role))
            .unwrap();
        state
            .submit(
                "demo",
                &claimed.id,
                &reviewer,
                ReviewSubmission {
                    verdict: crate::verification::ReviewVerdict::Pass,
                    summary: "Review passed.".into(),
                    claims: vec![],
                    risks: vec![],
                    model: None,
                },
            )
            .unwrap();
    }
    verification_store::persist(&workspace, &state).unwrap();
    let report = VerificationReport {
        execution_git_binding: None,
        required_checks: Some(vec![binding.clone()]),
        workspace: "demo".into(),
        level: "full".into(),
        execution: "native-receipt-fixture".into(),
        phases_run: 1,
        passed: true,
        checks_run: 1,
        checks_reused: 0,
        checks_failed: 0,
        skipped_checks: vec![],
        elapsed_ms: 1,
        summary: "Full required command passed.".into(),
        impact: None,
        cost_model: None,
        checks: vec![crate::harness::VerificationCheck {
            id: binding.id.clone(),
            phase: 0,
            command: "cargo check --locked".into(),
            reason: "fixture".into(),
            success: true,
            reused: false,
            execution: crate::evidence::VerificationCheckExecution::Executed,
            exit_code: Some(0),
            elapsed_ms: 1,
            queue_wait_ms: 0,
            execution_ms: 1,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            output_truncated: false,
            signature: Some(binding.signature),
            evidence_id: None,
        }],
    };
    runtime
        .record_verification_report("demo", &workspace, &revision, &report)
        .unwrap();
    for stage in [VerificationStage::Property, VerificationStage::Mutation] {
        let execution = crate::stage_executor::execute(
            &workspace,
            &crate::stage_executor::StageExecutorSpec {
                id: format!("{stage:?}"),
                stage,
                languages: vec![],
                program: "rustc".into(),
                args: vec!["--version".into()],
                cwd: ".".into(),
                timeout_seconds: 10,
                builtin: false,
            },
        )
        .await
        .unwrap();
        assert_eq!(execution.verdict, crate::verification::ReviewVerdict::Pass);
        let record = runtime
            .verification_stage_submit_native(
                "demo",
                &workspace,
                &plan.id,
                StageSubmission {
                    stage,
                    producer: format!("executor:{}", execution.executor_id),
                    verdict: execution.verdict,
                    summary: execution.summary,
                    artifact_digest: execution.artifact_digest,
                    targets: plan.stage_targets.clone(),
                    model: None,
                },
            )
            .unwrap();
        assert_eq!(record.authority, EvidenceAuthority::NativeStage);
    }
    let proof = evidence_store::load(&workspace).unwrap();
    let base = state.status(&plan.id).unwrap();
    let evaluate = |records: &[Evidence]| {
        SoftwareIntelligenceRuntime::verification_status_from_snapshot(
            base.clone(),
            &revision,
            records,
        )
        .unwrap()
    };
    let control = evaluate(&proof);
    assert!(control.ready, "{:?}", control.blockers);
    assert_eq!(control.submitted, control.plan.job_ids.len());
    assert_eq!(control.deterministic_result, Some(EvidenceResult::Pass));
    assert!(
        SoftwareIntelligenceRuntime::default()
            .verification_status("demo", &workspace, &plan.id)
            .unwrap()
            .ready
    );

    let receipt = proof
        .iter()
        .find(|record| record.kind == EvidenceKind::Verification)
        .unwrap()
        .execution_receipt
        .clone();
    let mut legacy = proof.clone();
    for record in &mut legacy {
        if record.authority == EvidenceAuthority::NativeStage {
            // Even a guessed native label and a strong full receipt cannot turn
            // an old Stage claim into a native execution.
            record.producer = "verify_project".into();
            record.confidence = Confidence::Deterministic;
            record.policy = Some("deterministic/full/v2".into());
            record.execution_receipt = receipt.clone();
            let mut json = serde_json::to_value(&record).unwrap();
            json.as_object_mut().unwrap().remove("authority");
            *record = serde_json::from_value(json).unwrap();
            assert_eq!(
                record.effective_authority(),
                EvidenceAuthority::LegacyUnknown
            );
        }
    }
    let unknown = evaluate(&legacy);
    assert_eq!(unknown.deterministic_result, Some(EvidenceResult::Pass));
    assert_eq!(unknown.submitted, unknown.plan.job_ids.len());
    assert!(!unknown.ready);
    assert!(unknown.stage_results.is_empty());
    assert!(unknown
        .blockers
        .contains(&"property-evidence-missing".into()));
    assert!(unknown
        .blockers
        .contains(&"mutation-evidence-missing".into()));
    assert!(unknown
        .blockers
        .iter()
        .all(|reason| reason.starts_with("property-") || reason.starts_with("mutation-")));

    let property = proof
        .iter()
        .find(|record| record.kind == EvidenceKind::Property)
        .unwrap();
    let generic = runtime
        .verification_stage_submit(
            "demo",
            &workspace,
            &plan.id,
            StageSubmission {
                stage: VerificationStage::Property,
                producer: property.producer.clone(),
                verdict: crate::verification::ReviewVerdict::Pass,
                summary: "Claimed native output".into(),
                artifact_digest: "sha256:unverified".into(),
                targets: plan.stage_targets.clone(),
                model: None,
            },
        )
        .unwrap();
    assert_eq!(generic.authority, EvidenceAuthority::SelfReported);
    assert_eq!(generic.confidence, Confidence::Low);
    let mut negative = proof
        .iter()
        .filter(|record| record.kind != EvidenceKind::Property)
        .cloned()
        .collect::<Vec<_>>();
    let mut old_failure = property.clone();
    old_failure.id = "EV-legacy-fail".into();
    old_failure.authority = EvidenceAuthority::LegacyUnknown;
    old_failure.result = EvidenceResult::Fail;
    old_failure.timestamp_ms = 1;
    old_failure.targets.clear();
    negative.push(old_failure.clone());
    let mut unknown_pass = old_failure.clone();
    unknown_pass.id = "EV-unbound-pass".into();
    unknown_pass.result = EvidenceResult::Pass;
    unknown_pass.timestamp_ms = 2;
    negative.extend([unknown_pass, generic]);
    let blocked = evaluate(&negative);
    assert!(!blocked.ready);
    assert_eq!(blocked.stage_results["property"], EvidenceResult::Fail);
    assert!(blocked
        .blockers
        .contains(&"property-evidence-failed".into()));
    let mut retry = property.clone();
    retry.timestamp_ms = 3;
    negative.push(retry);
    let resolved = evaluate(&negative);
    assert!(resolved.ready, "{:?}", resolved.blockers);
}

#[test]
fn generic_approval_labels_and_full_plan_digest_are_not_operator_authority() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let plan = runtime
        .create_plan_for_risk("demo", &workspace, RiskLevel::Critical)
        .unwrap();
    let claimed = runtime
        .verification_approve(
            "demo",
            &workspace,
            &plan.id,
            "local_operator:guessed",
            "I approve this exact full plan.",
        )
        .unwrap();
    assert_eq!(claimed.authority, EvidenceAuthority::LegacyUnknown);
    assert_eq!(
        claimed.policy,
        Some(verification_snapshot::human_approval_policy(&plan).unwrap())
    );
    let unknown = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert!(!unknown.human_approval);
    assert!(unknown.blockers.contains(&"human-approval-required".into()));
    let authorized = runtime
        .verification_approve_authorized(
            "demo",
            &workspace,
            &plan.id,
            "local_operator:consumed-fixture",
            "Exact native decision fixture.",
        )
        .unwrap();
    assert_eq!(authorized.authority, EvidenceAuthority::LocalOperator);
    let current = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert!(current.human_approval);
    assert!(
        !current.ready,
        "operator approval is not verification execution"
    );
}
