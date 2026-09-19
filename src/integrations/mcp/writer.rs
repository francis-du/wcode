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

pub(crate) fn runtime() -> &'static WriterRuntime {
    static RUNTIME: OnceLock<WriterRuntime> = OnceLock::new();
    RUNTIME.get_or_init(WriterRuntime::default)
}

#[derive(Clone, Default)]
pub(crate) struct WriterRuntime {
    leases: Arc<Mutex<HashMap<PathBuf, ActiveWriterLease>>>,
}

#[derive(Clone)]
struct ActiveWriterLease {
    workspace: String,
    owner_binding: String,
    reservation_binding: String,
    plan_id: String,
    task_id: Option<String>,
    executor: String,
    issued_at_ms: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct WriterReservation {
    domain: PathBuf,
    reservation: String,
    plan_id: String,
    executor: String,
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
    pub issued_at_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct WriterLeaseStatus {
    pub active: bool,
    pub recovery_required: bool,
    pub linked_worktree: bool,
    pub mutation_domain: String,
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

impl WriterRuntime {
    pub(crate) fn reserve_for_claim(
        &self,
        workspaces: &Workspaces,
        workspace_id: &str,
        workspace: &Workspace,
        args: &Value,
        owner: &str,
    ) -> Result<Option<WriterReservation>, String> {
        let review_only = args
            .get("kinds")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                !items.is_empty() && items.iter().all(|item| item.as_str() == Some("review"))
            });
        if review_only {
            return Ok(None);
        }
        let plan_id = args
            .get("plan_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "reconciliation_claim is missing plan_id".to_owned())?;
        let executor = args
            .get("executor")
            .and_then(Value::as_str)
            .ok_or_else(|| "reconciliation_claim is missing executor".to_owned())?;
        self.reserve(
            workspaces,
            workspace_id,
            workspace,
            owner,
            plan_id,
            executor,
        )
        .map(Some)
    }

    pub(crate) fn reserve(
        &self,
        workspaces: &Workspaces,
        workspace_id: &str,
        workspace: &Workspace,
        owner: &str,
        plan_id: &str,
        executor: &str,
    ) -> Result<WriterReservation, String> {
        validate_identity(plan_id, 160, "plan_id")?;
        validate_identity(executor, 256, "executor")?;
        let domain = workspace
            .mutation_domain_root()
            .map_err(|error| error.to_string())?;
        let mut leases = self
            .leases
            .lock()
            .map_err(|_| "writer lease registry poisoned".to_owned())?;
        if leases.contains_key(&domain) {
            return Err(
                "writer_lease_conflict: another writer already owns this Git worktree mutation domain"
                    .to_owned(),
            );
        }
        if durable_writer_count(workspaces, &domain)? > 0 {
            return Err(
                "writer_lease_recovery_required: durable claimed writer exists but runtime ownership is absent; writes remain fail-closed after restart"
                    .to_owned(),
            );
        }

        let reservation = format!("wr_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        leases.insert(
            domain.clone(),
            ActiveWriterLease {
                workspace: workspace_id.to_owned(),
                owner_binding: digest(owner),
                reservation_binding: digest(&reservation),
                plan_id: plan_id.to_owned(),
                task_id: None,
                executor: executor.to_owned(),
                issued_at_ms: now_ms(),
            },
        );
        Ok(WriterReservation {
            domain,
            reservation,
            plan_id: plan_id.to_owned(),
            executor: executor.to_owned(),
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
        let mut leases = self
            .leases
            .lock()
            .map_err(|_| "writer lease registry poisoned".to_owned())?;
        let lease = leases
            .get_mut(&reservation.domain)
            .ok_or_else(|| "writer reservation no longer exists".to_owned())?;
        if lease.reservation_binding != digest(&reservation.reservation)
            || lease.plan_id != reservation.plan_id
            || lease.executor != reservation.executor
        {
            return Err("writer reservation changed before claim completion".to_owned());
        }
        lease.task_id = Some(run.task.id.clone());
        Ok(Some(WriterLeaseGrant {
            active: true,
            workspace: lease.workspace.clone(),
            mutation_domain: mutation_domain_id(&reservation.domain),
            plan_id: lease.plan_id.clone(),
            task_id: run.task.id.clone(),
            executor: lease.executor.clone(),
            issued_at_ms: lease.issued_at_ms,
        }))
    }

    pub(crate) fn cancel(&self, reservation: &WriterReservation) {
        let Ok(mut leases) = self.leases.lock() else {
            return;
        };
        if leases
            .get(&reservation.domain)
            .is_some_and(|lease| lease.reservation_binding == digest(&reservation.reservation))
        {
            leases.remove(&reservation.domain);
        }
    }

    pub(crate) fn enforce_mutation(
        &self,
        workspaces: &Workspaces,
        workspace: &Workspace,
        owner: &str,
    ) -> Result<(), String> {
        let domain = workspace
            .mutation_domain_root()
            .map_err(|error| error.to_string())?;
        let lease = self
            .leases
            .lock()
            .map_err(|_| "writer lease registry poisoned".to_owned())?
            .get(&domain)
            .cloned();
        if let Some(lease) = lease {
            if lease.task_id.is_none() {
                return Err(
                    "writer_lease_pending: writer claim has not finished admission yet".to_owned(),
                );
            }
            if lease.owner_binding != digest(owner) {
                return Err(
                    "writer_lease_mismatch: mutation is bound to a different MCP owner".to_owned(),
                );
            }
            return Ok(());
        }
        if durable_writer_count(workspaces, &domain)? > 0 {
            return Err(
                "writer_lease_recovery_required: durable claimed writer exists but runtime ownership is absent; writes remain fail-closed after restart"
                    .to_owned(),
            );
        }
        Ok(())
    }

    pub(crate) fn enforce_submission(
        &self,
        workspaces: &Workspaces,
        workspace: &Workspace,
        run: &ReconciliationTaskRun,
        owner: &str,
    ) -> Result<(), String> {
        if !is_writer_run(run) {
            return Ok(());
        }
        self.enforce_mutation(workspaces, workspace, owner)
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
        let lease = leases
            .get(&domain)
            .ok_or_else(|| "writer lease is not active".to_owned())?;
        if lease.owner_binding != digest(owner)
            || lease.plan_id != plan_id
            || lease.task_id.as_deref() != Some(run.task.id.as_str())
        {
            return Err("writer lease does not match the claimed task".to_owned());
        }
        leases.remove(&domain);
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
        if let Some(lease) = self
            .leases
            .lock()
            .map_err(|_| "writer lease registry poisoned".to_owned())?
            .get(&domain)
            .cloned()
        {
            return Ok(WriterLeaseStatus {
                active: lease.task_id.is_some(),
                recovery_required: false,
                linked_worktree: registered_linked_worktree(workspaces, workspace),
                mutation_domain: mutation_domain_id(&domain),
                workspace: Some(lease.workspace),
                plan_id: Some(lease.plan_id),
                task_id: lease.task_id,
                executor: Some(lease.executor),
                issued_at_ms: Some(lease.issued_at_ms),
            });
        }
        let durable = durable_writer_count(workspaces, &domain)?;
        Ok(WriterLeaseStatus {
            active: false,
            recovery_required: durable > 0,
            linked_worktree: registered_linked_worktree(workspaces, workspace),
            mutation_domain: mutation_domain_id(&domain),
            workspace: None,
            plan_id: None,
            task_id: None,
            executor: None,
            issued_at_ms: None,
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
        runtime().enforce_mutation(&state.workspaces, &context.workspace, &owner)?;
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
            runtime().enforce_submission(&state.workspaces, &workspace, &run, &owner)?;
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

fn durable_writer_count(workspaces: &Workspaces, domain: &Path) -> Result<usize, String> {
    let mut count = 0usize;
    for (workspace_id, _) in workspaces.roots() {
        let (_, workspace) = workspaces
            .select(Some(&workspace_id))
            .map_err(|error| error.to_string())?;
        if workspace
            .mutation_domain_root()
            .map_err(|error| error.to_string())?
            != domain
        {
            continue;
        }
        count = count.saturating_add(
            crate::reconciliation_execution_store::claimed_writers(&workspace)
                .map_err(|error| error.to_string())?
                .len(),
        );
    }
    Ok(count)
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
