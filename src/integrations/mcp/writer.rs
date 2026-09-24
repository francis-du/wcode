use super::mcp_writer_recovery::writer_consistency_counts;
use super::mcp_writer_scope::{
    mutation_domain_path, mutation_domain_scopes, mutation_paths, scopes_allow_path,
};
use super::*;
use crate::execution_policy::{
    ExecutionEffect, ExecutionPolicyDecision, ExecutionPolicyInput, VerificationFloor,
};
use crate::reconcile::{
    ReconciliationClaimMode, ReconciliationRunStatus, ReconciliationTaskKind, ReconciliationTaskRun,
};
use crate::workspace::{self, Workspace, Workspaces};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const INTERNAL_OWNER: &str = "wcode-internal-tool-owner";
const FILE_MUTATION_TOOLS: &[&str] = &[
    "replace_text",
    "apply_edits",
    "write_file",
    "create_directory",
    "create_file",
    "create_files",
    "apply_file_edits",
    "move_path",
    "move_paths",
    "delete_path",
    "design_init",
];

tokio::task_local! {
    static MCP_OWNER: String;
}

pub(crate) async fn with_owner<F>(owner: String, future: F) -> F::Output
where
    F: Future,
{
    MCP_OWNER.scope(owner, future).await
}

pub(crate) fn current_owner() -> String {
    MCP_OWNER
        .try_with(Clone::clone)
        .unwrap_or_else(|_| INTERNAL_OWNER.to_owned())
}

pub(crate) fn owner_binding(owner: &str) -> String {
    digest(owner)
}

pub(crate) fn runtime() -> &'static WriterRuntime {
    static RUNTIME: OnceLock<WriterRuntime> = OnceLock::new();
    RUNTIME.get_or_init(WriterRuntime::default)
}

#[derive(Clone, Default)]
pub(crate) struct WriterRuntime {
    leases: Arc<Mutex<HashMap<PathBuf, Vec<ActiveWriterLease>>>>,
}

#[derive(Clone)]
pub(super) struct ActiveWriterLease {
    pub(super) workspace: String,
    pub(super) owner_binding: String,
    reservation_binding: String,
    pub(super) plan_id: String,
    pub(super) task_id: Option<String>,
    pub(super) executor: String,
    write_scopes: Vec<String>,
    issued_at_ms: u64,
    pub(super) durable_owner_bound: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct WriterReservation {
    domain: PathBuf,
    reservation: String,
    plan_id: String,
    task_id: Option<String>,
    executor: String,
    write_scopes: Vec<String>,
}

struct WriterClaimRequest<'a> {
    workspace_id: &'a str,
    owner: &'a str,
    plan_id: &'a str,
    executor: &'a str,
    domain: &'a Path,
    task_id: Option<String>,
    write_scopes: Vec<String>,
    recovering: bool,
}

pub(crate) enum WriterToolGuard {
    None,
    Claim(Option<WriterReservation>),
    Submission {
        workspace: Workspace,
        plan_id: String,
        run: Box<ReconciliationTaskRun>,
        owner: String,
    },
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct WriterLeaseGrant {
    pub active: bool,
    pub workspace: String,
    pub mutation_domain: String,
    pub plan_id: String,
    pub task_id: String,
    pub executor: String,
    pub write_scopes: Vec<String>,
    pub issued_at_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct WriterLeaseStatus {
    pub active: bool,
    pub recovery_required: bool,
    pub linked_worktree: bool,
    pub mutation_domain: String,
    pub active_writers: usize,
    pub pending_writers: usize,
    pub writers: Vec<WriterLeaseView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issued_at_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct WriterLeaseView {
    pub workspace: String,
    pub plan_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub executor: String,
    pub write_scopes: Vec<String>,
    pub issued_at_ms: u64,
}

impl WriterRuntime {
    pub(crate) fn reserve_for_claim(
        &self,
        workspaces: &Workspaces,
        workspace_id: &str,
        workspace: &Workspace,
        args: &mut Value,
        owner: &str,
    ) -> Result<Option<WriterReservation>, String> {
        let plan_id = args
            .get("plan_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "reconciliation_claim is missing plan_id".to_owned())?
            .to_owned();
        let executor = args
            .get("executor")
            .and_then(Value::as_str)
            .ok_or_else(|| "reconciliation_claim is missing executor".to_owned())?
            .to_owned();
        let requested_task_id = args
            .get("task_id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let kinds = args
            .get("kinds")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .cloned()
                    .map(|value| {
                        serde_json::from_value::<ReconciliationTaskKind>(value)
                            .map_err(|error| format!("invalid reconciliation task kind: {error}"))
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();
        let execution = crate::reconciliation_execution_store::load(workspace, &plan_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "reconciliation execution state does not exist".to_owned())?;
        let domain = workspace
            .mutation_domain_root()
            .map_err(|error| error.to_string())?;
        let binding = owner_binding(owner);
        if let Some(task_id) = requested_task_id.as_deref() {
            if let Some(run) = execution.tasks.iter().find(|run| {
                run.task.id == task_id
                    && run.status == ReconciliationRunStatus::Claimed
                    && is_writer_run(run)
            }) {
                let durable_owner = run
                    .ownership
                    .as_ref()
                    .and_then(|ownership| ownership.owner_binding.as_deref());
                if run.claimed_by.as_deref() != Some(executor.as_str())
                    || durable_owner != Some(binding.as_str())
                {
                    return Err(
                        "writer_lease_recovery_required: durable claimed writer can only be reclaimed by its bound MCP owner and executor"
                            .to_owned(),
                    );
                }
                if kinds.is_empty() || kinds.contains(&run.task.kind) {
                    args["task_id"] = Value::String(run.task.id.clone());
                    let scopes =
                        mutation_domain_scopes(workspace, &domain, &run.task.write_scopes)?;
                    return self
                        .reserve_scoped(
                            workspaces,
                            WriterClaimRequest {
                                workspace_id,
                                owner,
                                plan_id: &plan_id,
                                executor: &executor,
                                domain: &domain,
                                task_id: Some(run.task.id.clone()),
                                write_scopes: scopes,
                                recovering: true,
                            },
                        )
                        .map(Some);
                }
            }
        }
        let candidates = execution.claimable_tasks(&kinds, requested_task_id.as_deref());
        if candidates.is_empty() {
            return Ok(None);
        }
        let explicit = requested_task_id.is_some();
        for candidate in candidates {
            args["task_id"] = Value::String(candidate.task.id.clone());
            if candidate.task.kind == ReconciliationTaskKind::Review {
                return Ok(None);
            }
            let scopes = mutation_domain_scopes(workspace, &domain, &candidate.task.write_scopes)?;
            match self.reserve_scoped(
                workspaces,
                WriterClaimRequest {
                    workspace_id,
                    owner,
                    plan_id: &plan_id,
                    executor: &executor,
                    domain: &domain,
                    task_id: Some(candidate.task.id.clone()),
                    write_scopes: scopes,
                    recovering: false,
                },
            ) {
                Ok(reservation) => return Ok(Some(reservation)),
                Err(error) if !explicit && error.starts_with("writer_lease_conflict:") => continue,
                Err(error) => return Err(error),
            }
        }
        Err(
            "writer_lease_conflict: all runnable writer tasks overlap active writer scopes"
                .to_owned(),
        )
    }

    #[cfg(test)]
    pub(crate) fn reserve(
        &self,
        workspaces: &Workspaces,
        workspace_id: &str,
        workspace: &Workspace,
        owner: &str,
        plan_id: &str,
        executor: &str,
    ) -> Result<WriterReservation, String> {
        let domain = workspace
            .mutation_domain_root()
            .map_err(|error| error.to_string())?;
        self.reserve_scoped(
            workspaces,
            WriterClaimRequest {
                workspace_id,
                owner,
                plan_id,
                executor,
                domain: &domain,
                task_id: None,
                write_scopes: Vec::new(),
                recovering: false,
            },
        )
    }

    fn reserve_scoped(
        &self,
        workspaces: &Workspaces,
        request: WriterClaimRequest<'_>,
    ) -> Result<WriterReservation, String> {
        validate_identity(request.plan_id, 160, "plan_id")?;
        validate_identity(request.executor, 256, "executor")?;
        let mut leases = self
            .leases
            .lock()
            .map_err(|_| "writer lease registry poisoned".to_owned())?;
        let active = leases.entry(request.domain.to_path_buf()).or_default();
        let owner_binding = owner_binding(request.owner);
        let reservation = format!("wr_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let active_task_id = if request.recovering {
            Some(
                request
                    .task_id
                    .clone()
                    .ok_or_else(|| "writer recovery requires an explicit task_id".to_owned())?,
            )
        } else {
            None
        };
        let candidate = ActiveWriterLease {
            workspace: request.workspace_id.to_owned(),
            owner_binding: owner_binding.clone(),
            reservation_binding: digest(&reservation),
            plan_id: request.plan_id.to_owned(),
            task_id: active_task_id,
            executor: request.executor.to_owned(),
            write_scopes: request.write_scopes.clone(),
            issued_at_ms: now_ms(),
            durable_owner_bound: request.recovering,
        };
        let consistency_active = if request.recovering {
            let mut prospective = active.clone();
            prospective.push(candidate.clone());
            prospective
        } else {
            active.clone()
        };
        let (orphaned, stale) =
            writer_consistency_counts(workspaces, request.domain, &consistency_active)?;
        if orphaned > 0 || stale > 0 {
            return Err(
                "writer_lease_recovery_required: durable and runtime writer ownership disagree; writes remain fail-closed"
                    .to_owned(),
            );
        }
        if active
            .iter()
            .any(|lease| lease.owner_binding == owner_binding)
        {
            return Err(
                "writer_owner_conflict: one MCP owner may hold only one active or pending writer lease; use a distinct authenticated MCP owner for parallel writer lanes"
                    .to_owned(),
            );
        }
        if active.iter().any(|lease| {
            crate::reconcile::write_scopes_conflict(&lease.write_scopes, &request.write_scopes)
        }) {
            return Err(
                "writer_lease_conflict: requested write scope overlaps another writer in this Git worktree mutation domain"
                    .to_owned(),
            );
        }
        active.push(candidate);
        Ok(WriterReservation {
            domain: request.domain.to_path_buf(),
            reservation,
            plan_id: request.plan_id.to_owned(),
            task_id: request.task_id,
            executor: request.executor.to_owned(),
            write_scopes: request.write_scopes,
        })
    }

    pub(crate) fn finalize(
        &self,
        reservation: &WriterReservation,
        run: &ReconciliationTaskRun,
    ) -> Result<Option<WriterLeaseGrant>, String> {
        if !is_writer_run(run) {
            self.cancel(reservation);
            return Ok(None);
        }
        if reservation
            .task_id
            .as_deref()
            .is_some_and(|task_id| task_id != run.task.id)
        {
            self.cancel(reservation);
            return Err("writer reservation was admitted for a different task".to_owned());
        }
        let mut leases = self
            .leases
            .lock()
            .map_err(|_| "writer lease registry poisoned".to_owned())?;
        let active = leases
            .get_mut(&reservation.domain)
            .ok_or_else(|| "writer reservation no longer exists".to_owned())?;
        let lease = active
            .iter_mut()
            .find(|lease| lease.reservation_binding == digest(&reservation.reservation))
            .ok_or_else(|| "writer reservation no longer exists".to_owned())?;
        if lease.plan_id != reservation.plan_id
            || lease.executor != reservation.executor
            || lease.write_scopes != reservation.write_scopes
        {
            return Err("writer reservation changed before claim completion".to_owned());
        }
        if let Some(durable_owner) = run
            .ownership
            .as_ref()
            .and_then(|ownership| ownership.owner_binding.as_deref())
        {
            if durable_owner != lease.owner_binding {
                return Err("durable writer owner does not match runtime ownership".to_owned());
            }
            lease.durable_owner_bound = true;
        }
        lease.task_id = Some(run.task.id.clone());
        Ok(Some(WriterLeaseGrant {
            active: true,
            workspace: lease.workspace.clone(),
            mutation_domain: mutation_domain_id(&reservation.domain),
            plan_id: lease.plan_id.clone(),
            task_id: run.task.id.clone(),
            executor: lease.executor.clone(),
            write_scopes: lease.write_scopes.clone(),
            issued_at_ms: lease.issued_at_ms,
        }))
    }

    pub(crate) fn cancel(&self, reservation: &WriterReservation) {
        let Ok(mut leases) = self.leases.lock() else {
            return;
        };
        let mut empty = false;
        if let Some(active) = leases.get_mut(&reservation.domain) {
            let binding = digest(&reservation.reservation);
            active.retain(|lease| lease.reservation_binding != binding);
            empty = active.is_empty();
        }
        if empty {
            leases.remove(&reservation.domain);
        }
    }

    #[cfg(test)]
    pub(crate) fn enforce_mutation(
        &self,
        workspaces: &Workspaces,
        workspace: &Workspace,
        owner: &str,
    ) -> Result<(), String> {
        self.enforce_mutation_paths(workspaces, workspace, owner, None)
    }

    pub(crate) fn enforce_tool_mutation(
        &self,
        workspaces: &Workspaces,
        workspace: &Workspace,
        owner: &str,
        tool_name: &str,
        args: &Value,
    ) -> Result<(), String> {
        let paths = mutation_paths(tool_name, args)?;
        self.enforce_mutation_paths(workspaces, workspace, owner, paths)
    }

    fn enforce_mutation_paths(
        &self,
        workspaces: &Workspaces,
        workspace: &Workspace,
        owner: &str,
        paths: Option<Vec<String>>,
    ) -> Result<(), String> {
        let domain = workspace
            .mutation_domain_root()
            .map_err(|error| error.to_string())?;
        let active = self
            .leases
            .lock()
            .map_err(|_| "writer lease registry poisoned".to_owned())?
            .get(&domain)
            .cloned()
            .unwrap_or_default();
        let (orphaned, stale) = writer_consistency_counts(workspaces, &domain, &active)?;
        if orphaned > 0 || stale > 0 {
            return Err(
                "writer_lease_recovery_required: durable and runtime writer ownership disagree; writes remain fail-closed"
                    .to_owned(),
            );
        }
        if active.is_empty() {
            return Ok(());
        }
        let owner_binding = digest(owner);
        let owned = active
            .iter()
            .filter(|lease| lease.owner_binding == owner_binding && lease.task_id.is_some())
            .collect::<Vec<_>>();
        if owned.is_empty() {
            if active
                .iter()
                .any(|lease| lease.owner_binding == owner_binding && lease.task_id.is_none())
            {
                return Err(
                    "writer_lease_pending: writer claim has not finished admission yet".to_owned(),
                );
            }
            return Err(
                "writer_lease_mismatch: mutation is bound to a different MCP owner".to_owned(),
            );
        }
        let Some(paths) = paths else {
            if owned.iter().any(|lease| lease.write_scopes.is_empty()) {
                return Ok(());
            }
            return Err(
                "writer_scope_required: workspace-mutating commands require whole-domain writer ownership"
                    .to_owned(),
            );
        };
        for path in paths {
            let path = mutation_domain_path(workspace, &domain, &path)?;
            if !owned
                .iter()
                .any(|lease| scopes_allow_path(&lease.write_scopes, &path))
            {
                return Err(format!(
                    "writer_scope_mismatch: mutation path '{path}' is outside this MCP owner's approved write scopes"
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn enforce_submission(
        &self,
        workspaces: &Workspaces,
        workspace: &Workspace,
        plan_id: &str,
        run: &ReconciliationTaskRun,
        owner: &str,
    ) -> Result<(), String> {
        if !is_writer_run(run) {
            return Ok(());
        }
        let domain = workspace
            .mutation_domain_root()
            .map_err(|error| error.to_string())?;
        let active = self
            .leases
            .lock()
            .map_err(|_| "writer lease registry poisoned".to_owned())?
            .get(&domain)
            .cloned()
            .unwrap_or_default();
        let (orphaned, stale) = writer_consistency_counts(workspaces, &domain, &active)?;
        if orphaned > 0 || stale > 0 {
            return Err(
                "writer_lease_recovery_required: durable and runtime writer ownership disagree"
                    .to_owned(),
            );
        }
        let owner_binding = digest(owner);
        if active.iter().any(|lease| {
            lease.owner_binding == owner_binding
                && lease.plan_id == plan_id
                && lease.task_id.as_deref() == Some(run.task.id.as_str())
        }) {
            return Ok(());
        }
        Err("writer lease does not match the claimed task".to_owned())
    }

    pub(crate) fn release_for_run(
        &self,
        workspace: &Workspace,
        plan_id: &str,
        run: &ReconciliationTaskRun,
        owner: &str,
    ) -> Result<(), String> {
        if !is_writer_run(run) {
            return Ok(());
        }
        let domain = workspace
            .mutation_domain_root()
            .map_err(|error| error.to_string())?;
        let mut leases = self
            .leases
            .lock()
            .map_err(|_| "writer lease registry poisoned".to_owned())?;
        let Some(active) = leases.get_mut(&domain) else {
            return Err("writer lease is not active".to_owned());
        };
        let owner_binding = digest(owner);
        let before = active.len();
        active.retain(|lease| {
            !(lease.owner_binding == owner_binding
                && lease.plan_id == plan_id
                && lease.task_id.as_deref() == Some(run.task.id.as_str()))
        });
        if active.len() == before {
            return Err("writer lease does not match the claimed task".to_owned());
        }
        if active.is_empty() {
            leases.remove(&domain);
        }
        Ok(())
    }

    pub(crate) fn status(
        &self,
        workspaces: &Workspaces,
        workspace: &Workspace,
    ) -> Result<WriterLeaseStatus, String> {
        let domain = workspace
            .mutation_domain_root()
            .map_err(|error| error.to_string())?;
        let active = self
            .leases
            .lock()
            .map_err(|_| "writer lease registry poisoned".to_owned())?
            .get(&domain)
            .cloned()
            .unwrap_or_default();
        let (orphaned, stale) = writer_consistency_counts(workspaces, &domain, &active)?;
        let recovery_required = orphaned > 0 || stale > 0;
        let active_writers = active
            .iter()
            .filter(|lease| lease.task_id.is_some())
            .count();
        let pending_writers = active.len().saturating_sub(active_writers);
        let writers = active
            .iter()
            .map(|lease| WriterLeaseView {
                workspace: lease.workspace.clone(),
                plan_id: lease.plan_id.clone(),
                task_id: lease.task_id.clone(),
                executor: lease.executor.clone(),
                write_scopes: lease.write_scopes.clone(),
                issued_at_ms: lease.issued_at_ms,
            })
            .collect::<Vec<_>>();
        let single = (active.len() == 1).then(|| active[0].clone());
        Ok(WriterLeaseStatus {
            active: active_writers > 0,
            recovery_required,
            linked_worktree: registered_linked_worktree(workspaces, workspace),
            mutation_domain: mutation_domain_id(&domain),
            active_writers,
            pending_writers,
            writers,
            workspace: single.as_ref().map(|lease| lease.workspace.clone()),
            plan_id: single.as_ref().map(|lease| lease.plan_id.clone()),
            task_id: single.as_ref().and_then(|lease| lease.task_id.clone()),
            executor: single.as_ref().map(|lease| lease.executor.clone()),
            issued_at_ms: single.as_ref().map(|lease| lease.issued_at_ms),
        })
    }
}

struct RuntimePolicyContext {
    decision: ExecutionPolicyDecision,
    writer: WriterLeaseStatus,
    plan_id: Option<String>,
    workspace: Workspace,
}

fn runtime_policy_context(
    state: &AppState,
    tool_name: &str,
    tool_args: &Value,
    selected_args: &Value,
) -> Result<RuntimePolicyContext, String> {
    let (workspace_id, workspace) = super::mcp_tools::selected_workspace(state, selected_args)?;
    let execution = crate::execution::refresh(&state.harness, &workspace_id, &workspace, false)
        .map_err(|error| error.to_string())?;
    let phase = execution
        .get("phase")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let plan_id = execution
        .pointer("/checkpoint/reconciliation_plan_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let plan = match plan_id.as_deref() {
        Some(plan_id) => crate::reconciliation_store::load(&workspace, plan_id)
            .map_err(|error| error.to_string())?,
        None => None,
    };
    let risk = plan
        .as_ref()
        .map(|plan| plan.risk_level)
        .unwrap_or(crate::risk::RiskLevel::Low);
    let writer = runtime().status(&state.workspaces, &workspace)?;
    let product_scopes = crate::scopes::tool_scopes(tool_name)
        .into_iter()
        .map(|scope| scope.as_str().to_owned())
        .collect::<Vec<_>>();
    let existing_floor = plan.as_ref().map(|plan| {
        if plan.verification_plan.deterministic_level == "full" {
            VerificationFloor::Full
        } else {
            VerificationFloor::Quick
        }
    });
    let decision = crate::execution_policy::evaluate(ExecutionPolicyInput {
        effect: tool_effect(tool_name, tool_args),
        phase,
        risk,
        product_scopes,
        writer_lease_active: writer.active || writer.recovery_required,
        reconciliation_bound: plan.is_some(),
        existing_review_required: plan.is_some(),
        existing_verification_floor: existing_floor,
        existing_human_approval_required: plan
            .as_ref()
            .is_some_and(|plan| plan.verification_plan.require_human_approval),
    });
    Ok(RuntimePolicyContext {
        decision,
        writer,
        plan_id,
        workspace,
    })
}

pub(crate) fn policy_status(state: &AppState, args: &Value) -> Result<Value, String> {
    let tool_name = args
        .get("tool_name")
        .and_then(Value::as_str)
        .ok_or_else(|| "execution_policy_status requires tool_name".to_owned())?;
    let tool_args = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
    if !tool_args.is_object() {
        return Err("execution_policy_status arguments must be an object".to_owned());
    }
    let context = runtime_policy_context(state, tool_name, &tool_args, args)?;
    Ok(json!({
        "policy": context.decision,
        "writer": context.writer,
        "sandbox": crate::workspace::execution_sandbox_status(),
        "reconciliation_plan_id": context.plan_id,
        "source": "runtime_state"
    }))
}

fn enforce_plan_approval(context: &RuntimePolicyContext) -> Result<(), String> {
    if !context.decision.require_plan_approval {
        return Ok(());
    }
    let plan_id = context
        .plan_id
        .as_deref()
        .ok_or_else(|| "plan_approval_required: no bound reconciliation plan".to_owned())?;
    let live_plan = crate::reconciliation_store::load(&context.workspace, plan_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("reconciliation plan does not exist: {plan_id}"))?;
    crate::reconciliation_approval::require(&context.workspace, plan_id)
        .map_err(|error| error.to_string())?;
    let current_revision = crate::intelligence::SoftwareIntelligenceRuntime::default()
        .current_revision(&context.workspace)
        .map_err(|error| error.to_string())?;
    let execution = crate::reconciliation_execution_store::load(&context.workspace, plan_id)
        .map_err(|error| error.to_string())?;
    let status = crate::reconciliation_approval::status(
        &context.workspace,
        &live_plan,
        &current_revision,
        execution.as_ref(),
    )
    .map_err(|error| error.to_string())?;
    if status["replan_required"].as_bool() == Some(true) {
        return Err(format!(
            "replan_required: approved plan no longer matches runtime premises: {}",
            status["reasons"]
        ));
    }
    Ok(())
}

pub(crate) fn before_tool(
    state: &AppState,
    name: &str,
    args: &mut Value,
) -> Result<WriterToolGuard, String> {
    let owner = current_owner();
    let effect = tool_effect(name, args);
    if effect.mutates_workspace() {
        let context = runtime_policy_context(state, name, args, args)?;
        enforce_plan_approval(&context)?;
        runtime().enforce_tool_mutation(
            &state.workspaces,
            &context.workspace,
            &owner,
            name,
            args,
        )?;
    }

    if name == "reconciliation_claim" {
        let (workspace_id, workspace) = super::mcp_tools::selected_workspace(state, args)?;
        return Ok(WriterToolGuard::Claim(runtime().reserve_for_claim(
            &state.workspaces,
            &workspace_id,
            &workspace,
            args,
            &owner,
        )?));
    }
    if name == "reconciliation_submit" {
        let (_, workspace) = super::mcp_tools::selected_workspace(state, args)?;
        let plan_id = args
            .get("plan_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "reconciliation_submit is missing plan_id".to_owned())?
            .to_owned();
        let task_id = args
            .get("task_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "reconciliation_submit is missing task_id".to_owned())?;
        let execution = crate::reconciliation_execution_store::load(&workspace, &plan_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "reconciliation execution state does not exist".to_owned())?;
        let run = execution
            .tasks
            .iter()
            .find(|run| run.task.id == task_id)
            .cloned()
            .ok_or_else(|| "reconciliation task does not exist".to_owned())?;
        if run.status == ReconciliationRunStatus::Claimed {
            runtime().enforce_submission(&state.workspaces, &workspace, &plan_id, &run, &owner)?;
        }
        return Ok(WriterToolGuard::Submission {
            workspace,
            plan_id,
            run: Box::new(run),
            owner,
        });
    }
    Ok(WriterToolGuard::None)
}

pub(crate) fn after_tool(
    _state: &AppState,
    guard: WriterToolGuard,
    outcome: &mut AnyResult<Value>,
) {
    match guard {
        WriterToolGuard::None => {}
        WriterToolGuard::Claim(reservation) => {
            let Some(reservation) = reservation else {
                return;
            };
            if outcome.is_err() {
                runtime().cancel(&reservation);
                return;
            }
            let finalize = outcome
                .as_ref()
                .map_err(|error| error.to_string())
                .and_then(|value| {
                    serde_json::from_value::<ReconciliationTaskRun>(value.clone())
                        .map_err(|error| format!("invalid claimed reconciliation task: {error}"))
                })
                .and_then(|run| runtime().finalize(&reservation, &run));
            match finalize {
                Ok(Some(grant)) => {
                    if let Ok(value) = outcome.as_mut() {
                        value["writer_ownership"] =
                            serde_json::to_value(grant).unwrap_or_else(|_| json!({"active": true}));
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    runtime().cancel(&reservation);
                    *outcome = Err(anyhow!(error));
                }
            }
        }
        WriterToolGuard::Submission {
            workspace,
            plan_id,
            run,
            owner,
        } => {
            if outcome.is_ok() {
                if let Err(error) =
                    runtime().release_for_run(&workspace, &plan_id, run.as_ref(), &owner)
                {
                    *outcome = Err(anyhow!(error));
                }
            }
        }
    }
}

pub(crate) fn tool_effect(name: &str, args: &Value) -> ExecutionEffect {
    if FILE_MUTATION_TOOLS.contains(&name) {
        return ExecutionEffect::FileMutation;
    }
    if name == "run_command" {
        let Some(program) = args.get("program").and_then(Value::as_str) else {
            return ExecutionEffect::CommandMutation;
        };
        let command_args = match args.get("args") {
            None => Vec::new(),
            Some(Value::Array(values)) => {
                let Some(values) = values
                    .iter()
                    .map(|value| value.as_str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()
                else {
                    return ExecutionEffect::CommandMutation;
                };
                values
            }
            Some(_) => return ExecutionEffect::CommandMutation,
        };
        return if workspace::command_writes_workspace(program, &command_args) {
            ExecutionEffect::CommandMutation
        } else {
            ExecutionEffect::Read
        };
    }
    ExecutionEffect::Read
}

pub(crate) fn sanitized_args(value: &Value) -> Value {
    value.clone()
}

fn registered_linked_worktree(workspaces: &Workspaces, workspace: &Workspace) -> bool {
    workspaces.roots().into_iter().any(|(id, _)| {
        workspaces
            .select(Some(&id))
            .ok()
            .is_some_and(|(_, candidate)| {
                workspace.is_linked_worktree_of(&candidate).unwrap_or(false)
            })
    })
}

fn is_writer_run(run: &ReconciliationTaskRun) -> bool {
    matches!(
        run.task.kind,
        ReconciliationTaskKind::Design | ReconciliationTaskKind::Implementation
    ) && run
        .ownership
        .as_ref()
        .is_some_and(|ownership| ownership.mode == ReconciliationClaimMode::SharedWriter)
}

fn validate_identity(value: &str, max: usize, label: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max {
        return Err(format!("{label} is invalid"));
    }
    Ok(())
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn mutation_domain_id(domain: &Path) -> String {
    let digest = digest(&domain.to_string_lossy());
    format!("md-{}", &digest[..16])
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/mcp/writer.rs"]
mod tests;
