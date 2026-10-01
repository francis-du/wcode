use super::*;
use crate::reconcile::{
    ReconciliationClaimMode, ReconciliationClaimOwnership, ReconciliationExecution,
    ReconciliationPlan, ReconciliationRunStatus, ReconciliationTask, ReconciliationTaskKind,
    ReconciliationTaskRun, ReconciliationTaskSubmission,
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
            write_scopes: vec![],
            depends_on: vec![],
        },
        status: ReconciliationRunStatus::Claimed,
        claimed_by: Some("writer-a".into()),
        ownership: Some(ReconciliationClaimOwnership {
            mode: ReconciliationClaimMode::SharedWriter,
            owner_binding: None,
        }),
        summary: None,
        artifact_digest: None,
    }
}

fn claimed_scoped_writer(task_id: &str, scope: &str) -> ReconciliationTaskRun {
    let mut run = claimed_writer(task_id);
    run.task.write_scopes = vec![scope.to_owned()];
    run
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
            write_scopes: vec![],
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
fn restart_reclaim_is_owner_bound_and_stale_runtime_lease_fails_closed() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    let workspaces = Workspaces::new([root.path()], true, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let (_, workspace) = workspaces.select(Some(&workspace_id)).unwrap();

    let plan = writer_plan(&workspace_id);
    let mut execution = ReconciliationExecution::from_plan(&plan).unwrap();
    let binding = owner_binding("owner-a");
    let claimed = execution
        .claim_task_with_owner_binding(
            "writer-a",
            &[ReconciliationTaskKind::Implementation],
            Some("RT-writer"),
            Some(&binding),
        )
        .unwrap();
    let replayed = execution
        .claim_task_with_owner_binding(
            "writer-a",
            &[ReconciliationTaskKind::Implementation],
            Some("RT-writer"),
            Some(&binding),
        )
        .unwrap();
    assert_eq!(replayed.task.id, claimed.task.id);
    assert_eq!(
        claimed
            .ownership
            .as_ref()
            .and_then(|ownership| ownership.owner_binding.as_deref()),
        Some(binding.as_str())
    );
    crate::reconciliation_execution_store::persist(&workspace, &execution).unwrap();

    let restarted_runtime = WriterRuntime::default();
    let mut wrong_owner = json!({
        "plan_id": plan.id,
        "executor": "writer-a",
        "task_id": "RT-writer",
        "kinds": ["implementation"]
    });
    let denied = restarted_runtime
        .reserve_for_claim(
            &workspaces,
            &workspace_id,
            &workspace,
            &mut wrong_owner,
            "owner-b",
        )
        .unwrap_err();
    assert!(denied.contains("writer_lease_recovery_required"));

    let mut exact_owner = json!({
        "plan_id": plan.id,
        "executor": "writer-a",
        "task_id": "RT-writer",
        "kinds": ["implementation"]
    });
    let reservation = restarted_runtime
        .reserve_for_claim(
            &workspaces,
            &workspace_id,
            &workspace,
            &mut exact_owner,
            "owner-a",
        )
        .unwrap()
        .unwrap();
    restarted_runtime
        .finalize(&reservation, &claimed)
        .unwrap()
        .unwrap();
    restarted_runtime
        .enforce_mutation(&workspaces, &workspace, "owner-a")
        .unwrap();

    let completed = execution
        .submit(
            "RT-writer",
            "writer-a",
            ReconciliationTaskSubmission {
                success: true,
                summary: "Completed recovered writer lane.".into(),
                artifact_digest: None,
            },
        )
        .unwrap();
    crate::reconciliation_execution_store::persist(&workspace, &execution).unwrap();
    let stale = restarted_runtime
        .enforce_mutation(&workspaces, &workspace, "owner-a")
        .unwrap_err();
    assert!(stale.contains("writer_lease_recovery_required"));

    restarted_runtime
        .release_for_run(&workspace, &plan.id, &claimed, "owner-a")
        .unwrap();
    assert_eq!(completed.status, ReconciliationRunStatus::Completed);
    assert!(
        !restarted_runtime
            .status(&workspaces, &workspace)
            .unwrap()
            .recovery_required
    );
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

    let domain = primary_ws.mutation_domain_root().unwrap();
    assert_eq!(domain, subspace_ws.mutation_domain_root().unwrap());
    assert_eq!(
        mutation_domain_path(&subspace_ws, &domain, "src/lib.rs").unwrap(),
        "packages/app/src/lib.rs"
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
        tool_effect("design_init", &json!({"name":"demo"})),
        ExecutionEffect::FileMutation
    );
    assert_eq!(
        mutation_paths("design_init", &json!({"name":"demo"})).unwrap(),
        Some(vec![".wcode".to_owned()])
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

#[test]
fn scoped_writer_leases_allow_disjoint_owners_and_block_scope_escape() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::create_dir_all(root.path().join("tests")).unwrap();
    let workspaces = Workspaces::new([root.path()], true, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let (_, workspace) = workspaces.select(Some(&workspace_id)).unwrap();
    let domain = workspace.mutation_domain_root().unwrap();
    let runtime = WriterRuntime::default();

    let src_reservation = runtime
        .reserve_scoped(
            &workspaces,
            WriterClaimRequest {
                workspace_id: &workspace_id,
                owner: "owner-src",
                plan_id: "RP-scoped",
                executor: "writer-src",
                domain: &domain,
                task_id: Some("RT-src".into()),
                write_scopes: vec!["src".into()],
                recovering: false,
            },
        )
        .unwrap();
    let src_run = claimed_scoped_writer("RT-src", "src");
    runtime
        .finalize(&src_reservation, &src_run)
        .unwrap()
        .unwrap();

    let same_owner = runtime
        .reserve_scoped(
            &workspaces,
            WriterClaimRequest {
                workspace_id: &workspace_id,
                owner: "owner-src",
                plan_id: "RP-scoped",
                executor: "writer-tests-same-owner",
                domain: &domain,
                task_id: Some("RT-tests-same-owner".into()),
                write_scopes: vec!["tests".into()],
                recovering: false,
            },
        )
        .unwrap_err();
    assert!(same_owner.contains("writer_owner_conflict"));

    let tests_reservation = runtime
        .reserve_scoped(
            &workspaces,
            WriterClaimRequest {
                workspace_id: &workspace_id,
                owner: "owner-tests",
                plan_id: "RP-scoped",
                executor: "writer-tests",
                domain: &domain,
                task_id: Some("RT-tests".into()),
                write_scopes: vec!["tests".into()],
                recovering: false,
            },
        )
        .unwrap();
    let tests_run = claimed_scoped_writer("RT-tests", "tests");
    runtime
        .finalize(&tests_reservation, &tests_run)
        .unwrap()
        .unwrap();

    let status = runtime.status(&workspaces, &workspace).unwrap();
    assert_eq!(status.active_writers, 2);
    assert_eq!(status.pending_writers, 0);
    assert_eq!(status.writers.len(), 2);
    assert!(status.workspace.is_none());

    runtime
        .enforce_tool_mutation(
            &workspaces,
            &workspace,
            "owner-src",
            "write_file",
            &json!({"path":"src/lib.rs","content":"x"}),
        )
        .unwrap();
    runtime
        .enforce_tool_mutation(
            &workspaces,
            &workspace,
            "owner-tests",
            "write_file",
            &json!({"path":"tests/case.rs","content":"x"}),
        )
        .unwrap();
    let escaped = runtime
        .enforce_tool_mutation(
            &workspaces,
            &workspace,
            "owner-src",
            "write_file",
            &json!({"path":"tests/case.rs","content":"x"}),
        )
        .unwrap_err();
    assert!(escaped.contains("writer_scope_mismatch"));

    let design_escape = runtime
        .enforce_tool_mutation(
            &workspaces,
            &workspace,
            "owner-src",
            "design_init",
            &json!({"name":"demo"}),
        )
        .unwrap_err();
    assert!(design_escape.contains("writer_scope_mismatch"));

    let broad_command = runtime
        .enforce_tool_mutation(
            &workspaces,
            &workspace,
            "owner-src",
            "run_command",
            &json!({"program":"cargo","args":["fmt"]}),
        )
        .unwrap_err();
    assert!(broad_command.contains("writer_scope_required"));

    let overlap = runtime
        .reserve_scoped(
            &workspaces,
            WriterClaimRequest {
                workspace_id: &workspace_id,
                owner: "owner-overlap",
                plan_id: "RP-scoped",
                executor: "writer-overlap",
                domain: &domain,
                task_id: Some("RT-overlap".into()),
                write_scopes: vec!["src/lib.rs".into()],
                recovering: false,
            },
        )
        .unwrap_err();
    assert!(overlap.contains("writer_lease_conflict"));

    runtime
        .release_for_run(&workspace, "RP-scoped", &src_run, "owner-src")
        .unwrap();
    runtime
        .release_for_run(&workspace, "RP-scoped", &tests_run, "owner-tests")
        .unwrap();
    assert!(!runtime.status(&workspaces, &workspace).unwrap().active);
}

#[test]
fn claim_admission_routes_parallel_owners_to_disjoint_scopes() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    let workspaces = Workspaces::new([root.path()], true, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let (_, workspace) = workspaces.select(Some(&workspace_id)).unwrap();

    let mut plan = writer_plan(&workspace_id);
    plan.id = "RP-parallel-admission".into();
    plan.implementation_tasks = vec![
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
    ];
    let execution = ReconciliationExecution::from_plan(&plan).unwrap();
    crate::reconciliation_execution_store::persist(&workspace, &execution).unwrap();

    let runtime = WriterRuntime::default();
    let mut first = json!({
        "plan_id": plan.id,
        "executor": "writer-src",
        "kinds": ["implementation"]
    });
    let src = runtime
        .reserve_for_claim(
            &workspaces,
            &workspace_id,
            &workspace,
            &mut first,
            "owner-src",
        )
        .unwrap()
        .unwrap();
    assert_eq!(first["task_id"], "RT-src");

    let mut second = json!({
        "plan_id": plan.id,
        "executor": "writer-tests",
        "kinds": ["implementation"]
    });
    let tests = runtime
        .reserve_for_claim(
            &workspaces,
            &workspace_id,
            &workspace,
            &mut second,
            "owner-tests",
        )
        .unwrap()
        .unwrap();
    assert_eq!(second["task_id"], "RT-tests");

    runtime.cancel(&src);
    runtime.cancel(&tests);
}

#[tokio::test]
async fn mcp_stage_reports_cannot_satisfy_required_stages_or_replace_native_failure() {
    use crate::evidence::EvidenceResult;
    use crate::verification::{ReviewVerdict, VerificationStage};
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/a.rs"), "fn alpha() {}\n").unwrap();
    fs::write(root.path().join("src/b.rs"), "fn beta() {}\n").unwrap();
    let workspaces = Workspaces::new([root.path()], false, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".into())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    };
    let (workspace_id, workspace) = state.workspaces.select(None).unwrap();
    let plan = state
        .harness
        .verification_plan(
            workspace_id.clone(),
            &workspace,
            &crate::harness::ChangeReviewReport {
                workspace: workspace_id.clone(),
                execution: "stage-boundary-fixture".into(),
                clean: false,
                files_changed: 0,
                staged_files: 0,
                unstaged_files: 0,
                untracked_files: 0,
                additions: 0,
                deletions: 0,
                binary_files: 0,
                source_changed: false,
                tests_changed: false,
                docs_only: false,
                risk_level: "moderate".into(),
                recommended_verification: "full".into(),
                recommended_checks: vec![],
                summary: "Require property and mutation stages.".into(),
                files: vec![],
                findings: vec![],
                probes: vec![],
                truncated: false,
            },
        )
        .unwrap();
    assert!(plan.require_property && plan.require_mutation);
    assert!(!plan.stage_targets.is_empty());
    let mut report = json!({"name":"verification_stage_submit","arguments":{
        "plan_id":plan.id,"submission":{"stage":"property","producer":"executor:fixture-property",
        "verdict":"pass","summary":"Claimed runner output","artifact_digest":"sha256:unverified","targets":plan.stage_targets}}});
    let mut forged_authority = report.clone();
    forged_authority["arguments"]["submission"]["authority"] = json!("native_stage");
    let rejected = crate::mcp::call_tool_owned(&state, forged_authority, "owner-a")
        .await
        .unwrap_err();
    assert!(
        rejected.contains("unknown field") && rejected.contains("authority"),
        "{rejected}"
    );
    assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
    let mut ids = Vec::new();
    for (owner, claimed) in [
        ("owner-a", "pass"),
        ("owner-a", "fail"),
        ("owner-b", "pass"),
    ] {
        report["arguments"]["submission"]["verdict"] = json!(claimed);
        let result = crate::mcp::call_tool_owned(&state, report.clone(), owner)
            .await
            .unwrap();
        assert_eq!(result["isError"], false, "{result}");
        let record = &result["structuredContent"];
        assert_eq!(record["producer"], format!("self-reported:mcp:{owner}"));
        assert_eq!(record["result"], "inconclusive");
        assert_eq!(record["confidence"], "low");
        assert_eq!(record["authority"], "self_reported");
        let summary = record["summary"].as_str().unwrap();
        assert!(summary.contains("executor:fixture-property"));
        assert!(summary.to_lowercase().contains(claimed));
        ids.push(record["id"].as_str().unwrap().to_owned());
    }
    let missing = state
        .harness
        .verification_status(&workspace_id, &workspace, &plan.id)
        .unwrap();
    assert!(!missing.ready);
    for stage in ["property", "mutation"] {
        assert!(!missing.stage_results.contains_key(stage));
        assert!(!missing.stage_producer_results.contains_key(stage));
        assert!(missing
            .blockers
            .iter()
            .any(|reason| reason.contains(&format!("{stage}-evidence-missing"))));
    }
    let executing = crate::workspace::Workspace::new_with_security(
        root.path(),
        false,
        true,
        crate::workspace::WorkspaceSecurity {
            allow_risky_exec: true,
            ..Default::default()
        },
    )
    .unwrap();
    let execution = crate::stage_executor::execute(
        &executing,
        &crate::stage_executor::StageExecutorSpec {
            id: "fixture-property".into(),
            stage: VerificationStage::Property,
            languages: vec![crate::semantic_provider::SemanticLanguage::Rust],
            program: "rustc".into(),
            args: vec!["--wcode-invalid-stage-option".into()],
            cwd: ".".into(),
            timeout_seconds: 10,
            builtin: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(execution.verdict, ReviewVerdict::Fail);
    let native = state
        .harness
        .verification_stage_submit_native(
            &workspace_id,
            &workspace,
            &plan.id,
            crate::verification::StageSubmission {
                stage: execution.stage,
                producer: format!("executor:{}", execution.executor_id),
                verdict: execution.verdict,
                summary: execution.summary,
                artifact_digest: execution.artifact_digest,
                targets: plan.stage_targets.clone(),
                model: None,
            },
        )
        .unwrap();
    assert_eq!(native.result, EvidenceResult::Fail);
    assert_eq!(
        native.authority,
        crate::evidence::EvidenceAuthority::NativeStage
    );
    let forged = crate::mcp::call_tool_owned(&state, report, "owner-a")
        .await
        .unwrap();
    assert_eq!(forged["isError"], false, "{forged}");
    assert_eq!(forged["structuredContent"]["result"], "inconclusive");
    let failed = state
        .harness
        .verification_status(&workspace_id, &workspace, &plan.id)
        .unwrap();
    assert!(!failed.ready);
    assert_eq!(failed.stage_results["property"], EvidenceResult::Fail);
    assert_eq!(
        failed.stage_producer_results["property"]["executor:fixture-property"],
        EvidenceResult::Fail
    );
    assert_eq!(failed.stage_producer_results["property"].len(), 1);
    assert!(failed.stage_target_results["property"]
        .values()
        .all(|result| *result == EvidenceResult::Fail));
    assert!(failed
        .blockers
        .iter()
        .any(|reason| reason.contains("property") && reason.contains("failed")));
    assert!(failed
        .blockers
        .iter()
        .any(|reason| reason.contains("mutation-evidence-missing")));
    let evidence = state
        .harness
        .evidence_status(&workspace_id, &workspace, Some(&plan.subject), 50)
        .unwrap();
    for id in ids {
        assert!(evidence.evidence.iter().any(|record| record.id == id));
    }
    assert!(evidence
        .evidence
        .iter()
        .any(|record| record.id == native.id && record.result == EvidenceResult::Fail));
    let advisory = evidence
        .evidence
        .iter()
        .filter(|record| record.producer.starts_with("self-reported:mcp:"))
        .collect::<Vec<_>>();
    assert_eq!(advisory.len(), 4);
    assert!(advisory
        .iter()
        .all(|record| record.result == EvidenceResult::Inconclusive));
    assert_eq!(
        advisory
            .iter()
            .filter(|record| record.producer == "self-reported:mcp:owner-a")
            .count(),
        3
    );
    assert_eq!(
        advisory
            .iter()
            .filter(|record| record.producer == "self-reported:mcp:owner-b")
            .count(),
        1
    );
}
