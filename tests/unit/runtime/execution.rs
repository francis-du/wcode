use super::*;
use crate::reconcile::ReconciliationPlan;
use crate::risk::RiskLevel;
use crate::verification::VerificationPlan;
use crate::worklist::{self, WorkItemPatch, WorkItemStatus, WorklistUpdate};
use crate::workspace::Workspace;

fn signals(code: &str, ready: Option<bool>) -> ExecutionSignals {
    ExecutionSignals {
        repository_revision: Revision {
            design: None,
            code: code.to_owned(),
        },
        reconciliation_plan_id: None,
        verification_plan_id: ready.map(|_| "VP-test".to_owned()),
        verification_ready: ready,
        verification_blockers: if ready == Some(false) {
            vec!["reviewers_pending".to_owned()]
        } else {
            Vec::new()
        },
        reconciliation_converged: None,
        reconciliation_blockers: Vec::new(),
    }
}

#[test]
fn execution_checkpoint_tracks_worklist_and_requires_verification_to_complete() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    worklist::update(
        &workspace,
        WorklistUpdate {
            expected_revision: 0,
            goal: Some("ship durable execution".to_owned()),
            restart: false,
            items: vec![WorkItemPatch {
                id: "core".to_owned(),
                title: Some("Build the core".to_owned()),
                status: Some(WorkItemStatus::InProgress),
                depends_on: None,
                note: None,
            }],
        },
    )
    .unwrap();

    let worklist = worklist::snapshot(&workspace).unwrap().unwrap();
    let first = sync(&workspace, &worklist, None, false, signals("rev-a", None)).unwrap();
    assert_eq!(first["phase"], "executing");
    assert_eq!(first["checkpoint"]["open_items"], 1);
    assert_eq!(first["checkpoint"]["done_items"], 0);
    let first_id = first["execution_id"].as_str().unwrap().to_owned();

    let loaded = load(&workspace).unwrap();
    let unchanged = sync(&workspace, &worklist, loaded, false, signals("rev-a", None)).unwrap();
    assert_eq!(unchanged["revision"], first["revision"]);

    worklist::update(
        &workspace,
        WorklistUpdate {
            expected_revision: 1,
            goal: None,
            restart: false,
            items: vec![WorkItemPatch {
                id: "core".to_owned(),
                title: None,
                status: Some(WorkItemStatus::Done),
                depends_on: None,
                note: Some("implementation complete".to_owned()),
            }],
        },
    )
    .unwrap();
    let worklist = worklist::snapshot(&workspace).unwrap().unwrap();
    let verifying = sync(
        &workspace,
        &worklist,
        load(&workspace).unwrap(),
        false,
        signals("rev-b", Some(false)),
    )
    .unwrap();
    assert_eq!(verifying["execution_id"], first_id);
    assert_eq!(verifying["phase"], "verifying");
    assert_eq!(verifying["active"], true);
    assert_eq!(verifying["checkpoint"]["open_items"], 0);
    assert!(verifying["checkpoint"]["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|blocker| blocker == "reviewers_pending"));

    let completed = sync(
        &workspace,
        &worklist,
        load(&workspace).unwrap(),
        false,
        signals("rev-b", Some(true)),
    )
    .unwrap();
    assert_eq!(completed["execution_id"], first_id);
    assert_eq!(completed["phase"], "completed");
    assert_eq!(completed["active"], false);
}

#[test]
fn worklist_restart_creates_a_new_execution_generation() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    worklist::update(
        &workspace,
        WorklistUpdate {
            expected_revision: 0,
            goal: Some("first execution".to_owned()),
            restart: false,
            items: vec![WorkItemPatch {
                id: "first".to_owned(),
                title: Some("Finish first".to_owned()),
                status: Some(WorkItemStatus::InProgress),
                depends_on: None,
                note: None,
            }],
        },
    )
    .unwrap();
    let first_worklist = worklist::snapshot(&workspace).unwrap().unwrap();
    let first = sync(
        &workspace,
        &first_worklist,
        None,
        false,
        signals("rev-a", None),
    )
    .unwrap();
    let first_id = first["execution_id"].as_str().unwrap().to_owned();

    worklist::update(
        &workspace,
        WorklistUpdate {
            expected_revision: 1,
            goal: None,
            restart: false,
            items: vec![WorkItemPatch {
                id: "first".to_owned(),
                title: None,
                status: Some(WorkItemStatus::Done),
                depends_on: None,
                note: None,
            }],
        },
    )
    .unwrap();
    let completed_worklist = worklist::snapshot(&workspace).unwrap().unwrap();
    sync(
        &workspace,
        &completed_worklist,
        load(&workspace).unwrap(),
        false,
        signals("rev-a", Some(true)),
    )
    .unwrap();

    worklist::update(
        &workspace,
        WorklistUpdate {
            expected_revision: 2,
            goal: Some("second execution".to_owned()),
            restart: true,
            items: vec![WorkItemPatch {
                id: "second".to_owned(),
                title: Some("Start second".to_owned()),
                status: None,
                depends_on: None,
                note: None,
            }],
        },
    )
    .unwrap();
    let restarted_worklist = worklist::snapshot(&workspace).unwrap().unwrap();
    let second = sync(
        &workspace,
        &restarted_worklist,
        load(&workspace).unwrap(),
        true,
        signals("rev-b", None),
    )
    .unwrap();

    assert_ne!(second["execution_id"], first_id);
    assert_eq!(second["objective"], "second execution");
    assert_eq!(second["phase"], "executing");
    assert!(second["revision"].as_u64().unwrap() > first["revision"].as_u64().unwrap());
}

#[test]
fn blocked_worklist_projects_to_blocked_execution_without_chat_state() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    worklist::update(
        &workspace,
        WorklistUpdate {
            expected_revision: 0,
            goal: Some("wait for an external decision".to_owned()),
            restart: false,
            items: vec![WorkItemPatch {
                id: "decision".to_owned(),
                title: Some("Need decision".to_owned()),
                status: Some(WorkItemStatus::Blocked),
                depends_on: None,
                note: Some("waiting".to_owned()),
            }],
        },
    )
    .unwrap();
    let worklist = worklist::snapshot(&workspace).unwrap().unwrap();
    let status = sync(&workspace, &worklist, None, false, signals("rev-a", None)).unwrap();

    assert_eq!(status["phase"], "blocked");
    assert_eq!(status["checkpoint"]["runnable"], serde_json::json!([]));
    assert_eq!(
        status["checkpoint"]["blockers"],
        serde_json::json!(["worklist:decision"])
    );
    assert!(serde_json::to_string(&status).unwrap().len() < 8_000);
}

#[test]
fn model_terminal_proposal_is_advisory_and_cannot_self_complete() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    worklist::update(
        &workspace,
        WorklistUpdate {
            expected_revision: 0,
            goal: Some("prove completion authority".to_owned()),
            restart: false,
            items: vec![WorkItemPatch {
                id: "work".to_owned(),
                title: Some("Still working".to_owned()),
                status: Some(WorkItemStatus::InProgress),
                depends_on: None,
                note: None,
            }],
        },
    )
    .unwrap();
    let worklist = worklist::snapshot(&workspace).unwrap().unwrap();
    let initial = sync(&workspace, &worklist, None, false, signals("rev-a", None)).unwrap();
    let proposed = propose(
        &workspace,
        ExecutionProposalInput {
            expected_revision: initial["revision"].as_u64().unwrap(),
            status: ExecutionProposalStatus::Complete,
            summary: "I believe the task is complete".to_owned(),
            proposer: "model:test".to_owned(),
        },
    )
    .unwrap();

    assert_eq!(proposed["phase"], "executing");
    assert_eq!(proposed["active"], true);
    assert_eq!(proposed["proposal"]["status"], "complete");
    assert_eq!(
        proposed["settlement_authority"],
        "worklist+reconciliation+verification_evidence"
    );
}

#[test]
fn bound_reconciliation_plan_survives_expected_repository_revision_change() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    worklist::update(
        &workspace,
        WorklistUpdate {
            expected_revision: 0,
            goal: Some("keep approved convergence bound".to_owned()),
            restart: false,
            items: vec![WorkItemPatch {
                id: "work".to_owned(),
                title: Some("Implementation".to_owned()),
                status: Some(WorkItemStatus::InProgress),
                depends_on: None,
                note: None,
            }],
        },
    )
    .unwrap();
    let worklist = worklist::snapshot(&workspace).unwrap().unwrap();
    let mut bound = signals("rev-a", None);
    bound.reconciliation_plan_id = Some("RP-approved".to_owned());
    let initial = sync(&workspace, &worklist, None, false, bound).unwrap();
    let current = load(&workspace).unwrap().unwrap();

    assert_eq!(
        initial["checkpoint"]["reconciliation_plan_id"],
        "RP-approved"
    );
    assert_eq!(
        select_reconciliation_plan_id(Some(&current), false, false, None),
        Some("RP-approved".to_owned())
    );
    assert_eq!(
        select_reconciliation_plan_id(
            Some(&current),
            false,
            false,
            Some("RP-new-revision".to_owned())
        ),
        Some("RP-approved".to_owned())
    );
    assert_eq!(
        select_reconciliation_plan_id(
            Some(&current),
            false,
            true,
            Some("RP-steering-replan".to_owned())
        ),
        Some("RP-steering-replan".to_owned())
    );
    assert_eq!(
        select_reconciliation_plan_id(
            Some(&current),
            true,
            false,
            Some("RP-new-generation".to_owned())
        ),
        Some("RP-new-generation".to_owned())
    );
}

#[test]
fn verification_ready_cannot_complete_until_current_reconciliation_converges() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    worklist::update(
        &workspace,
        WorklistUpdate {
            expected_revision: 0,
            goal: Some("settle only from proof".to_owned()),
            restart: false,
            items: vec![WorkItemPatch {
                id: "work".to_owned(),
                title: Some("Implementation".to_owned()),
                status: Some(WorkItemStatus::Done),
                depends_on: None,
                note: None,
            }],
        },
    )
    .unwrap();
    let worklist = worklist::snapshot(&workspace).unwrap().unwrap();
    let mut pending = signals("rev-a", Some(true));
    pending.reconciliation_plan_id = Some("RP-test".to_owned());
    pending.reconciliation_converged = Some(false);
    pending.reconciliation_blockers = vec!["reconciliation_pending".to_owned()];
    let verifying = sync(&workspace, &worklist, None, false, pending).unwrap();
    assert_eq!(verifying["phase"], "verifying");
    assert!(verifying["checkpoint"]["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|blocker| blocker == "reconciliation_pending"));

    let mut converged = signals("rev-a", Some(true));
    converged.reconciliation_plan_id = Some("RP-test".to_owned());
    converged.reconciliation_converged = Some(true);
    let completed = sync(
        &workspace,
        &worklist,
        load(&workspace).unwrap(),
        false,
        converged,
    )
    .unwrap();
    assert_eq!(completed["phase"], "completed");
}

fn active_execution(workspace: &Workspace) -> Value {
    worklist::update(
        workspace,
        WorklistUpdate {
            expected_revision: 0,
            goal: Some("durable steering target".to_owned()),
            restart: false,
            items: vec![WorkItemPatch {
                id: "work".to_owned(),
                title: Some("Continue work".to_owned()),
                status: Some(WorkItemStatus::InProgress),
                depends_on: None,
                note: None,
            }],
        },
    )
    .unwrap();
    let worklist = worklist::snapshot(workspace).unwrap().unwrap();
    sync(workspace, &worklist, None, false, signals("rev-a", None)).unwrap()
}

#[test]
fn execution_steering_is_revision_guarded_and_survives_reload() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let initial = active_execution(&workspace);
    let steered = steer(
        &workspace,
        ExecutionDirectiveInput {
            expected_revision: initial["revision"].as_u64().unwrap(),
            kind: ExecutionDirectiveKind::RefineObjective,
            summary: "Prioritize the recovery path before polish".to_owned(),
            requested_by: "user:test".to_owned(),
            objective: Some("Finish recovery semantics before UI polish".to_owned()),
            scopes: Vec::new(),
            verification_strength: None,
        },
    )
    .unwrap();
    assert_eq!(steered["pending_directive"]["kind"], "refine_objective");
    assert_eq!(
        steered["pending_directive"]["objective"],
        "Finish recovery semantics before UI polish"
    );
    let reloaded = stored_status(&workspace).unwrap();
    assert_eq!(reloaded["pending_directive"], steered["pending_directive"]);

    let stale = steer(
        &workspace,
        ExecutionDirectiveInput {
            expected_revision: initial["revision"].as_u64().unwrap(),
            kind: ExecutionDirectiveKind::StrengthenVerification,
            summary: "Require adversarial verification".to_owned(),
            requested_by: "user:test".to_owned(),
            objective: None,
            scopes: Vec::new(),
            verification_strength: Some("adversarial".to_owned()),
        },
    )
    .unwrap_err()
    .to_string();
    assert!(stale.contains("execution revision changed"));

    let second = steer(
        &workspace,
        ExecutionDirectiveInput {
            expected_revision: steered["revision"].as_u64().unwrap(),
            kind: ExecutionDirectiveKind::StrengthenVerification,
            summary: "Require adversarial verification".to_owned(),
            requested_by: "user:test".to_owned(),
            objective: None,
            scopes: Vec::new(),
            verification_strength: Some("adversarial".to_owned()),
        },
    )
    .unwrap_err()
    .to_string();
    assert!(second.contains("already has pending steering"));
    assert_eq!(
        stored_status(&workspace).unwrap()["pending_directive"],
        steered["pending_directive"]
    );
}

#[test]
fn scope_steering_against_bound_plan_requires_replan() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    active_execution(&workspace);
    let revision = Revision {
        design: None,
        code: "rev-a".to_owned(),
    };
    let plan = ReconciliationPlan {
        id: "RP-steering".into(),
        workspace: "demo".into(),
        risk_level: RiskLevel::Low,
        design_changes: vec![],
        drift_ids: vec![],
        impacted_components: vec![],
        impacted_symbols: vec![],
        impacted_tests: vec![],
        impacted_acceptance: vec![],
        implementation_tasks: vec![],
        change_intents: vec![],
        verification_plan: VerificationPlan {
            id: "VP-steering".into(),
            workspace: "demo".into(),
            subject: "change:steering".into(),
            revision: Some(revision.clone()),
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
    let approved = crate::reconciliation_approval::approve(
        &workspace,
        &plan,
        &revision,
        "human:test",
        "Approve this exact plan.",
    )
    .unwrap();
    let worklist = worklist::snapshot(&workspace).unwrap().unwrap();
    let mut bound = signals("rev-a", None);
    bound.reconciliation_plan_id = Some(plan.id.clone());
    let status = sync(
        &workspace,
        &worklist,
        load(&workspace).unwrap(),
        false,
        bound,
    )
    .unwrap();

    let steered = steer(
        &workspace,
        ExecutionDirectiveInput {
            expected_revision: status["revision"].as_u64().unwrap(),
            kind: ExecutionDirectiveKind::ChangeScope,
            summary: "Expand the approved plan to another subsystem".to_owned(),
            requested_by: "user:test".to_owned(),
            objective: None,
            scopes: vec!["component:other".to_owned()],
            verification_strength: None,
        },
    )
    .unwrap();
    assert_eq!(steered["replan_required"], true);
    assert_eq!(steered["phase"], "blocked");
    assert_eq!(
        steered["pending_directive"]["reconciliation_plan_id"],
        "RP-steering"
    );
    assert!(steered["checkpoint"]["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|blocker| blocker == "steering_replan_required"));
    let reloaded = crate::reconciliation_approval::load(&workspace, &plan.id)
        .unwrap()
        .unwrap();
    assert_eq!(reloaded.plan_digest, approved.plan_digest);
    assert_eq!(reloaded.plan, approved.plan);

    worklist::update(
        &workspace,
        WorklistUpdate {
            expected_revision: 1,
            goal: None,
            restart: false,
            items: vec![WorkItemPatch {
                id: "work".to_owned(),
                title: None,
                status: Some(WorkItemStatus::InProgress),
                depends_on: None,
                note: Some("Applied scope steering and requested a new plan".to_owned()),
            }],
        },
    )
    .unwrap();
    let worklist = worklist::snapshot(&workspace).unwrap().unwrap();
    let mut replanned = signals("rev-b", None);
    replanned.reconciliation_plan_id = Some("RP-steering-v2".to_owned());
    let resumed = sync(
        &workspace,
        &worklist,
        load(&workspace).unwrap(),
        false,
        replanned,
    )
    .unwrap();
    assert!(resumed["pending_directive"].is_null());
    assert_eq!(resumed["replan_required"], false);
    assert_eq!(
        resumed["checkpoint"]["reconciliation_plan_id"],
        "RP-steering-v2"
    );
    assert_eq!(resumed["phase"], "executing");
}

#[test]
fn verification_steering_floor_is_monotonic_across_applied_directives() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let initial = active_execution(&workspace);
    let full = steer(
        &workspace,
        ExecutionDirectiveInput {
            expected_revision: initial["revision"].as_u64().unwrap(),
            kind: ExecutionDirectiveKind::StrengthenVerification,
            summary: "Require full verification".to_owned(),
            requested_by: "user:test".to_owned(),
            objective: None,
            scopes: Vec::new(),
            verification_strength: Some("full".to_owned()),
        },
    )
    .unwrap();
    assert_eq!(full["verification_floor"], "full");

    worklist::update(
        &workspace,
        WorklistUpdate {
            expected_revision: 1,
            goal: None,
            restart: false,
            items: vec![WorkItemPatch {
                id: "work".to_owned(),
                title: None,
                status: Some(WorkItemStatus::InProgress),
                depends_on: None,
                note: Some("Applied full verification steering".to_owned()),
            }],
        },
    )
    .unwrap();
    let worklist = worklist::snapshot(&workspace).unwrap().unwrap();
    let cleared = sync(
        &workspace,
        &worklist,
        load(&workspace).unwrap(),
        false,
        signals("rev-a", None),
    )
    .unwrap();
    assert!(cleared["pending_directive"].is_null());
    assert_eq!(cleared["verification_floor"], "full");

    let adversarial = steer(
        &workspace,
        ExecutionDirectiveInput {
            expected_revision: cleared["revision"].as_u64().unwrap(),
            kind: ExecutionDirectiveKind::StrengthenVerification,
            summary: "Require adversarial verification".to_owned(),
            requested_by: "user:test".to_owned(),
            objective: None,
            scopes: Vec::new(),
            verification_strength: Some("adversarial".to_owned()),
        },
    )
    .unwrap();
    assert_eq!(adversarial["verification_floor"], "adversarial");

    worklist::update(
        &workspace,
        WorklistUpdate {
            expected_revision: 2,
            goal: None,
            restart: false,
            items: vec![WorkItemPatch {
                id: "work".to_owned(),
                title: None,
                status: Some(WorkItemStatus::InProgress),
                depends_on: None,
                note: Some("Applied adversarial verification steering".to_owned()),
            }],
        },
    )
    .unwrap();
    let worklist = worklist::snapshot(&workspace).unwrap().unwrap();
    let cleared = sync(
        &workspace,
        &worklist,
        load(&workspace).unwrap(),
        false,
        signals("rev-a", None),
    )
    .unwrap();
    let error = steer(
        &workspace,
        ExecutionDirectiveInput {
            expected_revision: cleared["revision"].as_u64().unwrap(),
            kind: ExecutionDirectiveKind::StrengthenVerification,
            summary: "Attempt to lower verification".to_owned(),
            requested_by: "user:test".to_owned(),
            objective: None,
            scopes: Vec::new(),
            verification_strength: Some("full".to_owned()),
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("cannot lower strength"));
    assert_eq!(
        stored_status(&workspace).unwrap()["verification_floor"],
        "adversarial"
    );
}

#[test]
fn verification_steering_floor_raises_policy_and_rejects_weaker_proof() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let initial = active_execution(&workspace);
    let steered = steer(
        &workspace,
        ExecutionDirectiveInput {
            expected_revision: initial["revision"].as_u64().unwrap(),
            kind: ExecutionDirectiveKind::StrengthenVerification,
            summary: "Require adversarial proof before settlement".to_owned(),
            requested_by: "user:test".to_owned(),
            objective: None,
            scopes: Vec::new(),
            verification_strength: Some("adversarial".to_owned()),
        },
    )
    .unwrap();
    assert_eq!(steered["verification_floor"], "adversarial");
    assert_eq!(
        verification_risk_floor(&workspace, RiskLevel::Low).unwrap(),
        RiskLevel::High,
        "new verification plans must be raised to the adversarial floor"
    );

    let revision = Revision {
        code: "rev-proof".to_owned(),
        design: None,
    };
    let weak = VerificationPlan {
        id: "VP-weak".into(),
        workspace: "demo".into(),
        subject: "change:weak".into(),
        revision: Some(revision.clone()),
        risk_level: RiskLevel::High,
        policy: "risk-adaptive/v2/high".into(),
        deterministic_level: "full".into(),
        deterministic_checks: vec!["cargo test --locked".into()],
        reviewer_roles: vec![],
        require_property: true,
        require_mutation: true,
        require_fuzz: true,
        require_human_approval: false,
        stage_targets: vec![],
        automation_gaps: vec![],
        job_ids: vec![],
    };
    assert!(
        !verification_plan_satisfies_floor(&weak, Some("adversarial")),
        "full deterministic proof without an adversarial reviewer must not settle the floor"
    );

    let mut strong = weak.clone();
    strong.id = "VP-strong".into();
    strong.reviewer_roles = vec![crate::verification::ReviewerRole::Adversarial];
    assert!(verification_plan_satisfies_floor(
        &strong,
        Some("adversarial")
    ));
}

#[test]
fn execution_handoff_creates_clean_lineage_without_transcript_state() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let initial = active_execution(&workspace);
    let steered = steer(
        &workspace,
        ExecutionDirectiveInput {
            expected_revision: initial["revision"].as_u64().unwrap(),
            kind: ExecutionDirectiveKind::StrengthenVerification,
            summary: "Use adversarial verification before settlement".to_owned(),
            requested_by: "user:test".to_owned(),
            objective: None,
            scopes: Vec::new(),
            verification_strength: Some("adversarial".to_owned()),
        },
    )
    .unwrap();
    let parent = steered["execution_id"].as_str().unwrap().to_owned();
    let handed = handoff(
        &workspace,
        ExecutionHandoffInput {
            expected_revision: steered["revision"].as_u64().unwrap(),
            requested_by: "user:test".to_owned(),
            summary: "Continue in a clean model context".to_owned(),
        },
    )
    .unwrap();
    assert_ne!(handed["execution_id"], parent);
    assert_eq!(handed["lineage"]["parent_execution_id"], parent);
    assert_eq!(handed["lineage"]["handoff_count"], 1);
    assert_eq!(handed["pending_directive"], steered["pending_directive"]);
    assert!(handed["proposal"].is_null());
    let encoded = serde_json::to_string(&handed).unwrap().to_ascii_lowercase();
    for forbidden in [
        "transcript",
        "chain_of_thought",
        "messages",
        "provider_response",
    ] {
        assert!(!encoded.contains(forbidden));
    }
}
