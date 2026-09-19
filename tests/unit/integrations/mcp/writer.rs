use super::*;
use crate::reconcile::{
    ReconciliationClaimMode, ReconciliationClaimOwnership, ReconciliationExecution,
    ReconciliationPlan, ReconciliationRunStatus, ReconciliationTask, ReconciliationTaskKind,
    ReconciliationTaskRun,
};
use crate::risk::RiskLevel;
use crate::verification::VerificationPlan;
use crate::workspace::{Workspace, Workspaces};
use serde_json::json;
use std::fs;

fn claimed_writer(task_id: &str) -> ReconciliationTaskRun {
    ReconciliationTaskRun {
        task: ReconciliationTask {
            id: task_id.into(),
            kind: ReconciliationTaskKind::Implementation,
            subject: "src/lib.rs".into(),
            description: "Implement the approved change.".into(),
            depends_on: vec![],
        },
        status: ReconciliationRunStatus::Claimed,
        claimed_by: Some("writer-a".into()),
        ownership: Some(ReconciliationClaimOwnership {
            mode: ReconciliationClaimMode::SharedWriter,
        }),
        summary: None,
        artifact_digest: None,
    }
}

fn writer_plan(workspace_id: &str) -> ReconciliationPlan {
    ReconciliationPlan {
        id: "RP-writer-recovery".into(),
        workspace: workspace_id.into(),
        risk_level: RiskLevel::Low,
        design_changes: vec![],
        drift_ids: vec![],
        impacted_components: vec![],
        impacted_symbols: vec![],
        impacted_tests: vec![],
        impacted_acceptance: vec![],
        implementation_tasks: vec![ReconciliationTask {
            id: "RT-writer".into(),
            kind: ReconciliationTaskKind::Implementation,
            subject: "src/lib.rs".into(),
            description: "Implement the approved change.".into(),
            depends_on: vec![],
        }],
        change_intents: vec![],
        verification_plan: VerificationPlan {
            id: "VP-writer".into(),
            workspace: workspace_id.into(),
            subject: "change:writer".into(),
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
    }
}

#[test]
fn writer_runtime_binds_ownership_to_mcp_owner_without_model_token() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    let workspaces = Workspaces::new([root.path()], true, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let (_, workspace) = workspaces.select(Some(&workspace_id)).unwrap();
    let runtime = WriterRuntime::default();

    let reservation = runtime
        .reserve(
            &workspaces,
            &workspace_id,
            &workspace,
            "owner-a",
            "RP-a",
            "writer-a",
        )
        .unwrap();
    let run = claimed_writer("RT-a");
    let grant = runtime.finalize(&reservation, &run).unwrap().unwrap();

    assert!(grant.active);
    assert_eq!(grant.workspace, workspace_id);
    assert_eq!(grant.plan_id, "RP-a");
    assert_eq!(grant.task_id, "RT-a");

    runtime
        .enforce_mutation(&workspaces, &workspace, "owner-a")
        .unwrap();
    let wrong_owner = runtime
        .enforce_mutation(&workspaces, &workspace, "owner-b")
        .unwrap_err();
    assert!(wrong_owner.contains("writer_lease_mismatch"));

    runtime
        .release_for_run(&workspace, "RP-a", &run, "owner-a")
        .unwrap();
    runtime
        .enforce_mutation(&workspaces, &workspace, "owner-b")
        .unwrap();

    let encoded = serde_json::to_string(&grant).unwrap();
    assert!(!encoded.contains("writer_lease"));
    assert!(!encoded.contains("reservation"));
}

#[test]
fn writer_restart_with_durable_claim_fails_closed_without_runtime_lease() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    let workspaces = Workspaces::new([root.path()], true, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let (_, workspace) = workspaces.select(Some(&workspace_id)).unwrap();

    let plan = writer_plan(&workspace_id);
    let mut execution = ReconciliationExecution::from_plan(&plan).unwrap();
    let claimed = execution
        .claim("writer-a", &[ReconciliationTaskKind::Implementation])
        .unwrap();
    assert_eq!(
        claimed.ownership.as_ref().unwrap().mode,
        ReconciliationClaimMode::SharedWriter
    );
    crate::reconciliation_execution_store::persist(&workspace, &execution).unwrap();

    let restarted_runtime = WriterRuntime::default();
    let error = restarted_runtime
        .enforce_mutation(&workspaces, &workspace, "owner-a")
        .unwrap_err();
    assert!(error.contains("writer_lease_recovery_required"));

    let status = restarted_runtime.status(&workspaces, &workspace).unwrap();
    assert!(!status.active);
    assert!(status.recovery_required);
}

#[test]
fn bound_mutation_requires_explicit_plan_approval() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn value() -> u8 { 1 }\n",
    )
    .unwrap();
    let workspaces = Workspaces::new([root.path()], true, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let (_, workspace) = workspaces.select(Some(&workspace_id)).unwrap();
    let current = crate::intelligence::SoftwareIntelligenceRuntime::default()
        .current_revision(&workspace)
        .unwrap();
    let mut plan = writer_plan(&workspace_id);
    plan.id = "RP-approved-policy".into();
    plan.verification_plan.id = "VP-approved-policy".into();
    plan.verification_plan.revision = Some(current.clone());
    crate::reconciliation_store::persist(&workspace, &plan).unwrap();

    let policy = crate::execution_policy::evaluate(crate::execution_policy::ExecutionPolicyInput {
        effect: ExecutionEffect::FileMutation,
        phase: Some("executing".into()),
        risk: RiskLevel::Low,
        product_scopes: vec!["workspace".into()],
        writer_lease_active: false,
        reconciliation_bound: true,
        existing_review_required: true,
        existing_verification_floor: Some(crate::execution_policy::VerificationFloor::Quick),
        existing_human_approval_required: false,
    });
    let context = RuntimePolicyContext {
        decision: policy,
        writer: WriterRuntime::default()
            .status(&workspaces, &workspace)
            .unwrap(),
        plan_id: Some(plan.id.clone()),
        workspace: workspace.clone(),
    };
    let missing = enforce_plan_approval(&context).unwrap_err();
    assert!(missing.contains("plan_approval_required"));

    crate::reconciliation_approval::approve(
        &workspace,
        &plan,
        &current,
        "human:test",
        "I approve this exact implementation plan.",
    )
    .unwrap();
    enforce_plan_approval(&context).unwrap();
}

#[test]
fn mutation_domain_groups_subspaces_and_separates_linked_worktrees() {
    let root = tempfile::tempdir().unwrap();
    let primary = root.path().join("repo");
    let subspace = primary.join("packages/app");
    let linked = root.path().join("repo-linked");
    fs::create_dir_all(primary.join(".git/worktrees/repo-linked")).unwrap();
    fs::create_dir_all(&subspace).unwrap();
    fs::create_dir_all(&linked).unwrap();
    fs::write(
        linked.join(".git"),
        format!(
            "gitdir: {}\n",
            primary.join(".git/worktrees/repo-linked").to_string_lossy()
        ),
    )
    .unwrap();
    fs::write(
        primary.join(".git/worktrees/repo-linked/commondir"),
        "../..\n",
    )
    .unwrap();

    let primary_ws = Workspace::new(&primary, true, false).unwrap();
    let subspace_ws = Workspace::new(&subspace, true, false).unwrap();
    let linked_ws = Workspace::new(&linked, true, false).unwrap();

    assert_eq!(
        primary_ws.mutation_domain_root().unwrap(),
        subspace_ws.mutation_domain_root().unwrap()
    );
    assert_ne!(
        primary_ws.mutation_domain_root().unwrap(),
        linked_ws.mutation_domain_root().unwrap()
    );
    assert!(linked_ws.is_linked_worktree_of(&primary_ws).unwrap());
}

#[test]
fn active_writer_lease_gates_file_and_mutating_command_tools() {
    assert_eq!(
        tool_effect("write_file", &json!({"path":"src/lib.rs","content":"x"})),
        ExecutionEffect::FileMutation
    );
    assert_eq!(
        tool_effect(
            "run_command",
            &json!({"program":"git","args":["status","--short"]})
        ),
        ExecutionEffect::Read
    );
    assert_eq!(
        tool_effect("run_command", &json!({"program":"cargo","args":["fmt"]})),
        ExecutionEffect::CommandMutation
    );
    assert_eq!(
        tool_effect(
            "run_command",
            &json!({"program":"cargo","args":["fmt","--check"]})
        ),
        ExecutionEffect::Read
    );

    let args = json!({
        "tasks": [{"tool": "write_file", "arguments": {"path":"src/lib.rs"}}]
    });
    assert_eq!(sanitized_args(&args), args);
}
