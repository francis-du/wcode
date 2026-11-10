use super::*;
use crate::reconcile::ReconciliationPlan;
use crate::risk::RiskLevel;
use crate::verification::VerificationPlan;
use crate::worklist::{self, WorkItemPatch, WorkItemStatus, WorklistUpdate};

fn fixture() -> (tempfile::TempDir, Workspace, ToolHarness) {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("main.rs"), "fn main() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    advance_worklist(&workspace);
    let harness = ToolHarness::new(1).unwrap();
    (root, workspace, harness)
}

fn advance_worklist(workspace: &Workspace) {
    let current = worklist::snapshot(workspace).unwrap();
    worklist::update(
        workspace,
        WorklistUpdate {
            expected_revision: current.as_ref().map_or(0, |worklist| worklist.revision),
            goal: current
                .is_none()
                .then(|| "Recover current scoped work".to_owned()),
            restart: false,
            items: vec![WorkItemPatch {
                id: "work".to_owned(),
                title: Some("Continue implementation".to_owned()),
                status: Some(WorkItemStatus::InProgress),
                depends_on: None,
                write_paths: None,
                note: Some(format!(
                    "Apply scope at Worklist revision {}",
                    current.as_ref().map_or(1, |worklist| worklist.revision + 1)
                )),
            }],
        },
    )
    .unwrap();
}

fn persist_plan(
    harness: &ToolHarness,
    workspace: &Workspace,
    workspace_id: &str,
    id: &str,
) -> ReconciliationPlan {
    let plan = ReconciliationPlan {
        id: id.to_owned(),
        workspace: workspace_id.to_owned(),
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
            id: format!("VP-{id}"),
            workspace: workspace_id.to_owned(),
            subject: format!("change:{id}"),
            revision: Some(harness.current_revision(workspace).unwrap()),
            risk_level: RiskLevel::Low,
            policy: "risk-adaptive/v1/low".into(),
            deterministic_level: "quick".into(),
            required_checks: None,
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
    crate::reconciliation_store::persist(workspace, &plan).unwrap();
    plan
}

fn approve_fixture_plan(workspace: &Workspace, plan: &ReconciliationPlan) {
    crate::reconciliation_approval::approve(
        workspace,
        plan,
        plan.verification_plan.revision.as_ref().unwrap(),
        "human:test",
        "Approve this exact isolated test plan.",
    )
    .unwrap();
}

fn steer_bound(workspace: &Workspace, kind: ExecutionDirectiveKind) -> Value {
    let current = stored_status(workspace).unwrap();
    steer(
        workspace,
        ExecutionDirectiveInput {
            expected_revision: current["revision"].as_u64().unwrap(),
            kind,
            summary: "Continue the scoped work under a current plan".to_owned(),
            requested_by: "user:test".to_owned(),
            objective: (kind == ExecutionDirectiveKind::RefineObjective)
                .then(|| "Recover the scoped implementation".to_owned()),
            scopes: if kind == ExecutionDirectiveKind::ChangeScope {
                vec!["component:runtime".to_owned()]
            } else {
                vec![]
            },
            verification_strength: (kind == ExecutionDirectiveKind::StrengthenVerification)
                .then(|| "full".to_owned()),
        },
    )
    .unwrap()
}

#[test]
fn execution_refresh_recovers_unapproved_binding_after_explicit_replan() {
    for kind in [
        ExecutionDirectiveKind::RefineObjective,
        ExecutionDirectiveKind::ChangeScope,
    ] {
        let (root, workspace, harness) = fixture();
        let old = persist_plan(&harness, &workspace, "legacy-workspace", "RP-old");
        let initial = refresh(&harness, "legacy-workspace", &workspace, false).unwrap();
        assert_eq!(initial["checkpoint"]["reconciliation_plan_id"], old.id);
        let steered = steer_bound(&workspace, kind);

        fs::write(root.path().join("main.rs"), "fn main() { let _ = 1; }\n").unwrap();
        let current = persist_plan(&harness, &workspace, "current-workspace", "RP-new");
        approve_fixture_plan(&workspace, &current);
        let approved = serde_json::to_value(
            crate::reconciliation_approval::load(&workspace, &current.id).unwrap(),
        )
        .unwrap();
        let before_advance = refresh(&harness, "current-workspace", &workspace, false).unwrap();
        assert_eq!(
            before_advance["checkpoint"]["reconciliation_plan_id"],
            old.id
        );
        assert!(!before_advance["pending_directive"].is_null());

        advance_worklist(&workspace);
        let reloaded = ToolHarness::new(1).unwrap();
        let recovered = refresh(&reloaded, "current-workspace", &workspace, false).unwrap();
        assert_eq!(
            recovered["checkpoint"]["reconciliation_plan_id"], current.id,
            "explicit steering plus Worklist advance must replace the stale unapproved binding"
        );
        assert_eq!(steered["pending_directive"]["requires_replan"], true);
        assert!(recovered["pending_directive"].is_null());
        assert_eq!(recovered["phase"], "executing");
        assert_eq!(recovered["checkpoint"]["open_items"], 1);
        assert!(crate::reconciliation_approval::load(&workspace, &old.id)
            .unwrap()
            .is_none());
        assert_eq!(
            serde_json::to_value(
                crate::reconciliation_approval::load(&workspace, &current.id).unwrap()
            )
            .unwrap(),
            approved,
            "rebinding must not edit the approved snapshot"
        );
    }
}

#[test]
fn execution_refresh_preserves_bound_plan_without_explicit_steering() {
    for old_approved in [false, true] {
        let (root, workspace, harness) = fixture();
        let old = persist_plan(&harness, &workspace, "replan-recovery", "RP-old");
        if old_approved {
            approve_fixture_plan(&workspace, &old);
        }
        refresh(&harness, "replan-recovery", &workspace, false).unwrap();
        fs::write(root.path().join("main.rs"), "fn main() { let _ = 2; }\n").unwrap();
        let current = persist_plan(&harness, &workspace, "replan-recovery", "RP-new");
        approve_fixture_plan(&workspace, &current);
        advance_worklist(&workspace);

        let reloaded = ToolHarness::new(1).unwrap();
        let preserved = refresh(&reloaded, "replan-recovery", &workspace, false).unwrap();
        assert_eq!(preserved["checkpoint"]["reconciliation_plan_id"], old.id);
        assert!(preserved["pending_directive"].is_null());
        assert_ne!(preserved["phase"], "completed");
    }
}

#[test]
fn execution_refresh_keeps_scope_steering_pending_until_replacement_exists() {
    let (_root, workspace, harness) = fixture();
    let old = persist_plan(&harness, &workspace, "replan-recovery", "RP-old");
    refresh(&harness, "replan-recovery", &workspace, false).unwrap();
    steer_bound(&workspace, ExecutionDirectiveKind::ChangeScope);
    advance_worklist(&workspace);

    let pending = refresh(&harness, "replan-recovery", &workspace, false).unwrap();
    assert_eq!(pending["checkpoint"]["reconciliation_plan_id"], old.id);
    assert!(
        !pending["pending_directive"].is_null(),
        "advancing Worklist alone cannot apply scope steering against a bound plan"
    );
    assert_eq!(pending["pending_directive"]["requires_replan"], true);
    assert_eq!(pending["phase"], "blocked");
}

#[test]
fn execution_refresh_ignores_same_revision_plan_from_another_workspace() {
    let (_root, workspace, harness) = fixture();
    let foreign = persist_plan(&harness, &workspace, "other-workspace", "RP-foreign");
    let current_revision = harness.current_revision(&workspace).unwrap();
    assert_eq!(foreign.verification_plan.revision, Some(current_revision));
    let isolated = refresh(&harness, "current-workspace", &workspace, false).unwrap();
    assert!(
        isolated["checkpoint"]["reconciliation_plan_id"].is_null(),
        "matching repository content must not import another workspace's plan"
    );

    let local = persist_plan(&harness, &workspace, "current-workspace", "RP-local");
    let selected = refresh(&harness, "current-workspace", &workspace, false).unwrap();
    assert_eq!(selected["checkpoint"]["reconciliation_plan_id"], local.id);
    assert_ne!(selected["phase"], "completed");
}

#[test]
fn execution_refresh_reopening_work_preserves_pending_steering_and_verification_floor() {
    for kind in [
        ExecutionDirectiveKind::ChangeScope,
        ExecutionDirectiveKind::StrengthenVerification,
    ] {
        let (_root, workspace, harness) = fixture();
        let old = persist_plan(&harness, &workspace, "replan-recovery", "RP-old");
        approve_fixture_plan(&workspace, &old);
        let initial = refresh(&harness, "replan-recovery", &workspace, false).unwrap();
        let current_worklist = worklist::snapshot(&workspace).unwrap().unwrap();
        worklist::update(
            &workspace,
            WorklistUpdate {
                expected_revision: current_worklist.revision,
                goal: None,
                restart: false,
                items: vec![WorkItemPatch {
                    id: "work".to_owned(),
                    title: None,
                    status: Some(WorkItemStatus::Done),
                    depends_on: None,
                    write_paths: None,
                    note: Some(
                        "Implementation fixture complete; verification still pending".to_owned(),
                    ),
                }],
            },
        )
        .unwrap();
        let verifying = refresh(&harness, "replan-recovery", &workspace, false).unwrap();
        assert_eq!(verifying["checkpoint"]["open_items"], 0);
        assert_eq!(verifying["phase"], "verifying");
        let directive = steer_bound(&workspace, kind);
        advance_worklist(&workspace);

        let reloaded = ToolHarness::new(1).unwrap();
        let reopened = refresh(&reloaded, "replan-recovery", &workspace, false).unwrap();
        assert_eq!(reopened["checkpoint"]["reconciliation_plan_id"], old.id);
        assert_eq!(reopened["checkpoint"]["open_items"], 1);
        if kind == ExecutionDirectiveKind::ChangeScope {
            assert_eq!(
                reopened["pending_directive"], directive["pending_directive"],
                "reopening unfinished work must retain unsatisfied scope steering"
            );
            assert_eq!(reopened["phase"], "blocked");
        } else {
            assert_eq!(
                reopened["verification_floor"], "full",
                "reopening work must not erase the explicitly strengthened proof floor"
            );
            assert!(reopened["pending_directive"].is_null());
            assert_eq!(reopened["phase"], "executing");
        }
        assert_eq!(reopened["execution_id"], initial["execution_id"]);
        assert_ne!(reopened["phase"], "completed");
    }
}

fn persist_legacy_directive(
    workspace: &Workspace,
    kind: ExecutionDirectiveKind,
    recorded_requirement: Option<bool>,
) -> (PathBuf, Vec<u8>) {
    let mut execution = load(workspace).unwrap().unwrap();
    let mut directive = build_directive(
        &execution,
        ExecutionDirectiveInput {
            expected_revision: execution.revision,
            kind,
            summary: "Continue the scope from a schema-v1 checkpoint".to_owned(),
            requested_by: "user:test".to_owned(),
            objective: (kind == ExecutionDirectiveKind::RefineObjective)
                .then(|| "Resume the legacy scoped work".to_owned()),
            scopes: if kind == ExecutionDirectiveKind::ChangeScope {
                vec!["component:runtime".to_owned()]
            } else {
                vec![]
            },
            verification_strength: (kind == ExecutionDirectiveKind::StrengthenVerification)
                .then(|| "full".to_owned()),
        },
    )
    .unwrap();
    // Reproduce old serialized state without calling the repaired steer entrypoint.
    directive.requires_replan = recorded_requirement.unwrap_or(false);
    execution.revision += 1;
    execution.updated_at_ms = now_ms();
    execution.phase = ExecutionPhase::Blocked;
    execution.verification_floor = directive.verification_strength.clone();
    execution.pending_directive = Some(directive);
    append_execution_blocker(
        &mut execution.checkpoint,
        if recorded_requirement == Some(true) {
            "steering_replan_required"
        } else {
            "steering_pending"
        },
    );
    validate(&execution).unwrap();
    persist(workspace, &execution).unwrap();
    let directory = execution_directory(workspace).unwrap();
    let mut path = directory.join(format!("{:020}.json", execution.revision));
    let mut bytes = fs::read(&path).unwrap();
    if recorded_requirement.is_none() {
        let mut legacy: Value = serde_json::from_slice(&bytes).unwrap();
        legacy["revision"] = json!(execution.revision + 1);
        legacy["pending_directive"]
            .as_object_mut()
            .unwrap()
            .remove("requires_replan");
        bytes = serde_json::to_vec(&legacy).unwrap();
        // Publish another immutable fixture snapshot with the older omitted field.
        path = directory.join(format!("{:020}.json", execution.revision + 1));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.write_all(&bytes).unwrap();
    }
    let raw: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(raw["schema_version"], 1);
    assert_eq!(
        raw["pending_directive"]["requires_replan"].as_bool(),
        recorded_requirement
    );
    (path, bytes)
}

#[test]
fn execution_legacy_scope_directive_restores_replan_after_reload() {
    for kind in [
        ExecutionDirectiveKind::RefineObjective,
        ExecutionDirectiveKind::ChangeScope,
    ] {
        for recorded_requirement in [Some(false), None] {
            let (_root, workspace, harness) = fixture();
            let old = persist_plan(&harness, &workspace, "legacy-recovery", "RP-a-legacy-old");
            let initial = refresh(&harness, "legacy-recovery", &workspace, false).unwrap();
            let (snapshot, original_bytes) =
                persist_legacy_directive(&workspace, kind, recorded_requirement);

            let restored = stored_status(&workspace).unwrap();
            assert_eq!(restored["replan_required"], true);
            assert_eq!(restored["pending_directive"]["requires_replan"], true);
            assert_eq!(
                fs::read(&snapshot).unwrap(),
                original_bytes,
                "loading legacy state must not rewrite its published snapshot"
            );

            advance_worklist(&workspace);
            let reloaded = ToolHarness::new(1).unwrap();
            let pending = refresh(&reloaded, "legacy-recovery", &workspace, false).unwrap();
            assert_eq!(pending["checkpoint"]["reconciliation_plan_id"], old.id);
            assert_eq!(pending["pending_directive"]["requires_replan"], true);
            assert_eq!(pending["phase"], "blocked");
            assert_eq!(pending["execution_id"], initial["execution_id"]);

            let replacement =
                persist_plan(&reloaded, &workspace, "legacy-recovery", "RP-z-legacy-new");
            let recovered = refresh(&reloaded, "legacy-recovery", &workspace, false).unwrap();
            assert_eq!(
                recovered["checkpoint"]["reconciliation_plan_id"],
                replacement.id
            );
            assert!(recovered["pending_directive"].is_null());
            assert_eq!(recovered["phase"], "executing");
            assert_eq!(recovered["checkpoint"]["open_items"], 1);
            assert_eq!(recovered["execution_id"], initial["execution_id"]);
            for plan_id in [&old.id, &replacement.id] {
                assert!(crate::reconciliation_approval::load(&workspace, plan_id)
                    .unwrap()
                    .is_none());
                assert!(
                    crate::reconciliation_approval::require(&workspace, plan_id)
                        .unwrap_err()
                        .to_string()
                        .contains("plan_approval_required"),
                    "recovering the binding cannot approve either plan"
                );
            }
            assert_eq!(fs::read(&snapshot).unwrap(), original_bytes);
        }
    }
}

#[test]
fn execution_legacy_explicit_replan_requirement_survives_reload() {
    let (_root, workspace, harness) = fixture();
    refresh(&harness, "legacy-recovery", &workspace, false).unwrap();
    let (snapshot, original_bytes) = persist_legacy_directive(
        &workspace,
        ExecutionDirectiveKind::StrengthenVerification,
        Some(true),
    );
    let restored = stored_status(&workspace).unwrap();
    assert_eq!(restored["replan_required"], true);
    assert_eq!(restored["pending_directive"]["requires_replan"], true);
    assert!(restored["pending_directive"]["reconciliation_plan_id"].is_null());
    assert_eq!(fs::read(&snapshot).unwrap(), original_bytes);

    advance_worklist(&workspace);
    let pending = refresh(&harness, "legacy-recovery", &workspace, false).unwrap();
    assert_eq!(pending["pending_directive"]["requires_replan"], true);
    assert_eq!(pending["verification_floor"], "full");
    assert_eq!(pending["phase"], "blocked");
}
