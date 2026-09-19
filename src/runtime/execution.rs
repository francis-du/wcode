use crate::evidence::Revision;
use crate::harness::ToolHarness;
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const EXECUTION_SCHEMA_VERSION: u32 = 1;
const MAX_EXECUTION_SNAPSHOTS: usize = 128;
const MAX_EXECUTION_BYTES: u64 = 128 * 1024;
const MAX_EXECUTION_BLOCKERS: usize = 24;
const MAX_PROPOSAL_SUMMARY_CHARS: usize = 1_000;
const MAX_STEERING_SUMMARY_CHARS: usize = 1_000;
const MAX_STEERING_SCOPES: usize = 32;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionPhase {
    Executing,
    Blocked,
    Verifying,
    Completed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionProposalStatus {
    Complete,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ExecutionProposal {
    pub status: ExecutionProposalStatus,
    pub summary: String,
    pub proposer: String,
    pub worklist_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_revision: Option<Revision>,
    pub proposed_at_ms: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ExecutionProposalInput {
    pub expected_revision: u64,
    pub status: ExecutionProposalStatus,
    pub summary: String,
    pub proposer: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionDirectiveKind {
    RefineObjective,
    ChangeScope,
    StrengthenVerification,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ExecutionDirective {
    pub kind: ExecutionDirectiveKind,
    pub summary: String,
    pub requested_by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_strength: Option<String>,
    pub worklist_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_revision: Option<Revision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconciliation_plan_id: Option<String>,
    #[serde(default)]
    pub requires_replan: bool,
    pub requested_at_ms: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ExecutionDirectiveInput {
    pub expected_revision: u64,
    pub kind: ExecutionDirectiveKind,
    pub summary: String,
    pub requested_by: String,
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub verification_strength: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ExecutionHandoffInput {
    pub expected_revision: u64,
    pub requested_by: String,
    pub summary: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ExecutionLineage {
    pub parent_execution_id: String,
    pub handoff_count: u32,
    pub requested_by: String,
    pub summary: String,
    pub handed_off_at_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ExecutionCheckpoint {
    pub worklist_revision: u64,
    pub worklist_updated_at_ms: u64,
    pub open_items: usize,
    pub done_items: usize,
    pub blocked_items: usize,
    pub runnable: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_revision: Option<Revision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconciliation_plan_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_plan_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconciliation_converged: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Execution {
    pub schema_version: u32,
    pub execution_id: String,
    pub revision: u64,
    pub objective: String,
    pub phase: ExecutionPhase,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub checkpoint: ExecutionCheckpoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal: Option<ExecutionProposal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_directive: Option<ExecutionDirective>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage: Option<ExecutionLineage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_floor: Option<String>,
}

#[derive(Clone, Debug)]
struct ExecutionSignals {
    repository_revision: Revision,
    reconciliation_plan_id: Option<String>,
    verification_plan_id: Option<String>,
    verification_ready: Option<bool>,
    verification_blockers: Vec<String>,
    reconciliation_converged: Option<bool>,
    reconciliation_blockers: Vec<String>,
}

pub(crate) fn refresh(
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &Workspace,
    restart: bool,
) -> Result<Value> {
    let Some(worklist) = crate::worklist::snapshot(workspace)? else {
        return Ok(empty_status());
    };
    let current = load(workspace)?;
    if current.is_none() && crate::worklist::is_complete(&worklist) && !restart {
        return Ok(empty_status());
    }

    let repository_revision = harness.current_revision(workspace)?;
    let current_revision_plan_id = harness
        .reconciliation_history(workspace, 20)?
        .into_iter()
        .find(|plan| plan.verification_plan.revision.as_ref() == Some(&repository_revision))
        .map(|plan| plan.id);
    let steering_replan_ready = current.as_ref().is_some_and(|execution| {
        execution
            .pending_directive
            .as_ref()
            .is_some_and(|directive| {
                directive.requires_replan && worklist.revision > directive.worklist_revision
            })
    });
    let reconciliation_plan_id = select_reconciliation_plan_id(
        current.as_ref(),
        restart,
        steering_replan_ready,
        current_revision_plan_id,
    );
    let (reconciliation_converged, reconciliation_blockers) =
        if crate::worklist::is_complete(&worklist) {
            match reconciliation_plan_id.as_deref() {
                Some(plan_id) => {
                    match harness.reconciliation_execution_status(workspace_id, workspace, plan_id)
                    {
                        Ok(status) => {
                            let mut blockers = status.intent_blockers;
                            if status.failed > 0 {
                                blockers.push("reconciliation_failed".to_owned());
                            }
                            if !status.converged {
                                blockers.push("reconciliation_pending".to_owned());
                            }
                            (Some(status.converged), blockers)
                        }
                        Err(_) => (
                            Some(false),
                            vec!["reconciliation_status_unavailable".to_owned()],
                        ),
                    }
                }
                None => (None, Vec::new()),
            }
        } else {
            (None, Vec::new())
        };
    let verification = harness
        .verification_history(workspace_id, workspace, 20)?
        .into_iter()
        .find(|status| status.plan.revision.as_ref() == Some(&repository_revision));
    let verification_floor = current
        .as_ref()
        .and_then(|execution| execution.verification_floor.as_deref());
    let verification_floor_satisfied = verification
        .as_ref()
        .is_some_and(|status| verification_plan_satisfies_floor(&status.plan, verification_floor));
    let mut verification_blockers = verification
        .as_ref()
        .map(|status| status.blockers.clone())
        .unwrap_or_default();
    if verification.is_some() && !verification_floor_satisfied {
        verification_blockers.push("verification_floor_unsatisfied".to_owned());
    }
    let signals = ExecutionSignals {
        repository_revision,
        reconciliation_plan_id,
        verification_plan_id: verification.as_ref().map(|status| status.plan.id.clone()),
        verification_ready: verification
            .as_ref()
            .map(|status| status.ready && verification_floor_satisfied),
        verification_blockers,
        reconciliation_converged,
        reconciliation_blockers,
    };
    sync(workspace, &worklist, current, restart, signals)
}

pub(crate) fn active_summary(
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &Workspace,
) -> Result<Option<Value>> {
    let status = refresh(harness, workspace_id, workspace, false)?;
    if !status["exists"].as_bool().unwrap_or(false) || !status["active"].as_bool().unwrap_or(false)
    {
        return Ok(None);
    }
    Ok(Some(json!({
        "id": status["execution_id"],
        "revision": status["revision"],
        "objective": status["objective"],
        "phase": status["phase"],
        "checkpoint": status["checkpoint"],
        "proposal": status["proposal"],
        "pending_directive": status["pending_directive"],
        "replan_required": status["replan_required"],
        "verification_floor": status["verification_floor"],
        "lineage": status["lineage"],
        "guidance": "Resume from this checkpoint before reconstructing completed work. Apply pending structured steering through a newer Worklist revision and, when replan_required=true, a new Reconciliation plan before unrelated work. A model proposal is advisory only; current-revision Worklist/Reconciliation/Verification authority settles terminal state."
    })))
}

pub(crate) fn stored_status(workspace: &Workspace) -> Result<Value> {
    Ok(match load(workspace)? {
        Some(execution) => status_value(&execution),
        None => empty_status(),
    })
}

pub(crate) fn verification_risk_floor(
    workspace: &Workspace,
    risk: crate::risk::RiskLevel,
) -> Result<crate::risk::RiskLevel> {
    let floor = load(workspace)?
        .and_then(|execution| execution.verification_floor)
        .unwrap_or_default();
    Ok(match floor.as_str() {
        "adversarial" => risk.max(crate::risk::RiskLevel::High),
        "full" => risk.max(crate::risk::RiskLevel::Medium),
        _ => risk,
    })
}

fn verification_plan_satisfies_floor(
    plan: &crate::verification::VerificationPlan,
    floor: Option<&str>,
) -> bool {
    match floor {
        None => true,
        Some("full") => plan.deterministic_level == "full",
        Some("adversarial") => {
            plan.deterministic_level == "full"
                && plan
                    .reviewer_roles
                    .contains(&crate::verification::ReviewerRole::Adversarial)
        }
        Some(_) => false,
    }
}

fn sync(
    workspace: &Workspace,
    worklist: &crate::worklist::Worklist,
    current: Option<Execution>,
    restart: bool,
    signals: ExecutionSignals,
) -> Result<Value> {
    let mut checkpoint = checkpoint(worklist, &signals);
    let new_generation = restart
        || current.is_none()
        || current.as_ref().is_some_and(|execution| {
            execution.checkpoint.open_items == 0
                && !crate::worklist::is_complete(worklist)
                && worklist.revision > execution.checkpoint.worklist_revision
        });
    let mut pending_directive = if new_generation {
        None
    } else {
        current
            .as_ref()
            .and_then(|execution| execution.pending_directive.clone())
    };
    if pending_directive.as_ref().is_some_and(|directive| {
        let worklist_advanced = worklist.revision > directive.worklist_revision;
        let replan_satisfied = !directive.requires_replan
            || (checkpoint.reconciliation_plan_id.is_some()
                && checkpoint.reconciliation_plan_id != directive.reconciliation_plan_id);
        worklist_advanced && replan_satisfied
    }) {
        pending_directive = None;
    }
    if let Some(directive) = pending_directive.as_ref() {
        append_execution_blocker(
            &mut checkpoint,
            if directive.requires_replan {
                "steering_replan_required"
            } else {
                "steering_pending"
            },
        );
    }
    let phase = phase(worklist, &checkpoint, pending_directive.is_some());

    if let Some(existing) = current.as_ref() {
        if !new_generation
            && existing.objective == worklist.goal
            && existing.phase == phase
            && existing.checkpoint == checkpoint
            && existing.pending_directive == pending_directive
        {
            return Ok(status_value(existing));
        }
    }

    let proposal = current.as_ref().and_then(|execution| {
        execution.proposal.clone().filter(|proposal| {
            proposal.worklist_revision == checkpoint.worklist_revision
                && proposal.repository_revision == checkpoint.repository_revision
        })
    });
    let now = now_ms();
    let revision = current
        .as_ref()
        .map_or(1, |execution| execution.revision.saturating_add(1));
    let execution = Execution {
        schema_version: EXECUTION_SCHEMA_VERSION,
        execution_id: if new_generation {
            format!("EX-{}", Uuid::new_v4().simple())
        } else {
            current
                .as_ref()
                .map(|execution| execution.execution_id.clone())
                .unwrap_or_else(|| format!("EX-{}", Uuid::new_v4().simple()))
        },
        revision,
        objective: worklist.goal.clone(),
        phase,
        created_at_ms: if new_generation {
            now
        } else {
            current
                .as_ref()
                .map_or(now, |execution| execution.created_at_ms)
        },
        updated_at_ms: now,
        checkpoint,
        proposal: if new_generation || pending_directive.is_some() {
            None
        } else {
            proposal
        },
        pending_directive,
        lineage: if new_generation {
            None
        } else {
            current
                .as_ref()
                .and_then(|execution| execution.lineage.clone())
        },
        verification_floor: if new_generation {
            None
        } else {
            current
                .as_ref()
                .and_then(|execution| execution.verification_floor.clone())
        },
    };
    validate(&execution)?;
    persist(workspace, &execution)?;
    Ok(status_value(&execution))
}

fn checkpoint(
    worklist: &crate::worklist::Worklist,
    signals: &ExecutionSignals,
) -> ExecutionCheckpoint {
    let done_items = worklist
        .items
        .iter()
        .filter(|item| item.status == crate::worklist::WorkItemStatus::Done)
        .count();
    let blocked_ids = worklist
        .items
        .iter()
        .filter(|item| item.status == crate::worklist::WorkItemStatus::Blocked)
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    let mut blockers = blocked_ids
        .iter()
        .map(|id| format!("worklist:{id}"))
        .collect::<Vec<_>>();
    blockers.extend(signals.verification_blockers.iter().cloned());
    blockers.extend(signals.reconciliation_blockers.iter().cloned());
    if crate::worklist::is_complete(worklist)
        && signals.verification_plan_id.is_none()
        && signals.verification_ready != Some(true)
    {
        blockers.push("verification_pending".to_owned());
    }
    blockers.sort();
    blockers.dedup();
    blockers.truncate(MAX_EXECUTION_BLOCKERS);

    ExecutionCheckpoint {
        worklist_revision: worklist.revision,
        worklist_updated_at_ms: worklist.updated_at_ms,
        open_items: worklist.items.len().saturating_sub(done_items),
        done_items,
        blocked_items: blocked_ids.len(),
        runnable: crate::worklist::runnable_ids(worklist),
        repository_revision: Some(signals.repository_revision.clone()),
        reconciliation_plan_id: signals.reconciliation_plan_id.clone(),
        verification_plan_id: signals.verification_plan_id.clone(),
        verification_ready: signals.verification_ready,
        reconciliation_converged: signals.reconciliation_converged,
        blockers,
    }
}

fn select_reconciliation_plan_id(
    current: Option<&Execution>,
    restart: bool,
    allow_steering_replan: bool,
    current_revision_plan_id: Option<String>,
) -> Option<String> {
    if restart {
        return current_revision_plan_id;
    }
    let bound = current.and_then(|execution| execution.checkpoint.reconciliation_plan_id.clone());
    if allow_steering_replan {
        if let Some(candidate) = current_revision_plan_id.as_ref() {
            if bound.as_ref() != Some(candidate) {
                return Some(candidate.clone());
            }
        }
    }
    bound.or(current_revision_plan_id)
}

fn phase(
    worklist: &crate::worklist::Worklist,
    checkpoint: &ExecutionCheckpoint,
    steering_pending: bool,
) -> ExecutionPhase {
    if steering_pending {
        return ExecutionPhase::Blocked;
    }
    if crate::worklist::is_complete(worklist) {
        let reconciliation_ready = checkpoint.reconciliation_plan_id.is_none()
            || checkpoint.reconciliation_converged == Some(true);
        if reconciliation_ready && checkpoint.verification_ready == Some(true) {
            ExecutionPhase::Completed
        } else {
            ExecutionPhase::Verifying
        }
    } else if checkpoint.open_items > 0
        && checkpoint.runnable.is_empty()
        && checkpoint.blocked_items > 0
    {
        ExecutionPhase::Blocked
    } else {
        ExecutionPhase::Executing
    }
}

fn status_value(execution: &Execution) -> Value {
    json!({
        "exists": true,
        "active": execution.phase != ExecutionPhase::Completed,
        "execution_id": execution.execution_id,
        "revision": execution.revision,
        "objective": execution.objective,
        "phase": execution.phase,
        "created_at_ms": execution.created_at_ms,
        "updated_at_ms": execution.updated_at_ms,
        "checkpoint": execution.checkpoint,
        "proposal": execution.proposal,
        "pending_directive": execution.pending_directive,
        "replan_required": execution.pending_directive.as_ref().is_some_and(|directive| directive.requires_replan),
        "lineage": execution.lineage,
        "verification_floor": execution.verification_floor,
        "settlement_authority": "worklist+reconciliation+verification_evidence",
    })
}

fn empty_status() -> Value {
    json!({
        "exists": false,
        "active": false,
        "revision": 0,
        "checkpoint": Value::Null,
        "proposal": Value::Null,
        "pending_directive": Value::Null,
        "replan_required": false,
        "lineage": Value::Null,
        "verification_floor": Value::Null,
    })
}

pub(crate) fn steer(workspace: &Workspace, input: ExecutionDirectiveInput) -> Result<Value> {
    let mut execution =
        load(workspace)?.ok_or_else(|| anyhow::anyhow!("active execution does not exist"))?;
    ensure_active_revision(&execution, input.expected_revision)?;
    if execution.pending_directive.is_some() {
        bail!("execution already has pending steering; apply it through Worklist/replan state before recording another directive");
    }
    let mut directive = build_directive(&execution, input)?;
    directive.reconciliation_plan_id = execution.checkpoint.reconciliation_plan_id.clone();
    directive.requires_replan = matches!(
        directive.kind,
        ExecutionDirectiveKind::RefineObjective | ExecutionDirectiveKind::ChangeScope
    ) && match directive.reconciliation_plan_id.as_deref() {
        Some(plan_id) => crate::reconciliation_approval::load(workspace, plan_id)?.is_some(),
        None => false,
    };
    if directive.kind == ExecutionDirectiveKind::StrengthenVerification {
        execution.verification_floor = directive.verification_strength.clone();
    }
    execution.revision = execution.revision.saturating_add(1);
    execution.updated_at_ms = now_ms();
    execution.phase = ExecutionPhase::Blocked;
    append_execution_blocker(
        &mut execution.checkpoint,
        if directive.requires_replan {
            "steering_replan_required"
        } else {
            "steering_pending"
        },
    );
    execution.proposal = None;
    execution.pending_directive = Some(directive);
    validate(&execution)?;
    persist(workspace, &execution)?;
    Ok(status_value(&execution))
}

pub(crate) fn handoff(workspace: &Workspace, input: ExecutionHandoffInput) -> Result<Value> {
    let mut execution =
        load(workspace)?.ok_or_else(|| anyhow::anyhow!("active execution does not exist"))?;
    ensure_active_revision(&execution, input.expected_revision)?;
    let requested_by = input.requested_by.trim();
    let summary = input.summary.trim();
    if requested_by.is_empty()
        || requested_by.len() > 256
        || summary.is_empty()
        || summary.chars().count() > MAX_STEERING_SUMMARY_CHARS
    {
        bail!("execution handoff requester or summary is invalid");
    }
    let parent_execution_id = execution.execution_id.clone();
    let handoff_count = execution
        .lineage
        .as_ref()
        .map_or(1, |lineage| lineage.handoff_count.saturating_add(1));
    execution.execution_id = format!("EX-{}", Uuid::new_v4().simple());
    execution.revision = execution.revision.saturating_add(1);
    execution.updated_at_ms = now_ms();
    execution.created_at_ms = execution.updated_at_ms;
    execution.proposal = None;
    execution.lineage = Some(ExecutionLineage {
        parent_execution_id,
        handoff_count,
        requested_by: requested_by.to_owned(),
        summary: summary.to_owned(),
        handed_off_at_ms: execution.updated_at_ms,
    });
    validate(&execution)?;
    persist(workspace, &execution)?;
    Ok(status_value(&execution))
}

fn ensure_active_revision(execution: &Execution, expected_revision: u64) -> Result<()> {
    if execution.phase == ExecutionPhase::Completed {
        bail!("execution is already settled as completed");
    }
    if expected_revision != execution.revision {
        bail!(
            "execution revision changed: expected {expected_revision}, current {}; reread execution_status and retry",
            execution.revision
        );
    }
    Ok(())
}

fn build_directive(
    execution: &Execution,
    input: ExecutionDirectiveInput,
) -> Result<ExecutionDirective> {
    let summary = input.summary.trim();
    let requested_by = input.requested_by.trim();
    if summary.is_empty()
        || summary.chars().count() > MAX_STEERING_SUMMARY_CHARS
        || requested_by.is_empty()
        || requested_by.len() > 256
        || input.scopes.len() > MAX_STEERING_SCOPES
        || input
            .scopes
            .iter()
            .any(|scope| scope.trim().is_empty() || scope.len() > 300)
    {
        bail!("execution steering directive is invalid");
    }
    let objective = input.objective.map(|value| value.trim().to_owned());
    let verification_strength = input
        .verification_strength
        .map(|value| value.trim().to_ascii_lowercase());
    match input.kind {
        ExecutionDirectiveKind::RefineObjective => {
            if objective.as_deref().is_none_or(str::is_empty)
                || objective
                    .as_ref()
                    .is_some_and(|value| value.chars().count() > 1_000)
                || !input.scopes.is_empty()
                || verification_strength.is_some()
            {
                bail!("refine_objective requires objective only");
            }
        }
        ExecutionDirectiveKind::ChangeScope => {
            if input.scopes.is_empty() || objective.is_some() || verification_strength.is_some() {
                bail!("change_scope requires one or more scopes only");
            }
        }
        ExecutionDirectiveKind::StrengthenVerification => {
            if objective.is_some() || !input.scopes.is_empty() {
                bail!("strengthen_verification accepts verification_strength only");
            }
            if !matches!(
                verification_strength.as_deref(),
                Some("full" | "adversarial")
            ) {
                bail!("verification_strength must be full or adversarial");
            }
            if verification_strength.as_deref() == Some("full")
                && execution.verification_floor.as_deref() == Some("adversarial")
            {
                bail!("verification steering cannot lower strength from adversarial to full");
            }
        }
    }
    let scopes = input
        .scopes
        .into_iter()
        .map(|scope| scope.trim().to_owned())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(ExecutionDirective {
        kind: input.kind,
        summary: summary.to_owned(),
        requested_by: requested_by.to_owned(),
        objective,
        scopes,
        verification_strength,
        worklist_revision: execution.checkpoint.worklist_revision,
        repository_revision: execution.checkpoint.repository_revision.clone(),
        reconciliation_plan_id: execution.checkpoint.reconciliation_plan_id.clone(),
        requires_replan: false,
        requested_at_ms: now_ms(),
    })
}

pub(crate) fn propose(workspace: &Workspace, input: ExecutionProposalInput) -> Result<Value> {
    let mut execution =
        load(workspace)?.ok_or_else(|| anyhow::anyhow!("active execution does not exist"))?;
    ensure_active_revision(&execution, input.expected_revision)?;
    let summary = input.summary.trim();
    let proposer = input.proposer.trim();
    if summary.is_empty()
        || summary.chars().count() > MAX_PROPOSAL_SUMMARY_CHARS
        || proposer.is_empty()
        || proposer.len() > 256
    {
        bail!("execution proposal summary or proposer is invalid");
    }
    execution.revision = execution.revision.saturating_add(1);
    execution.updated_at_ms = now_ms();
    execution.proposal = Some(ExecutionProposal {
        status: input.status,
        summary: summary.to_owned(),
        proposer: proposer.to_owned(),
        worklist_revision: execution.checkpoint.worklist_revision,
        repository_revision: execution.checkpoint.repository_revision.clone(),
        proposed_at_ms: execution.updated_at_ms,
    });
    validate(&execution)?;
    persist(workspace, &execution)?;
    Ok(status_value(&execution))
}

fn load(workspace: &Workspace) -> Result<Option<Execution>> {
    let directory = execution_directory(workspace)?;
    if !directory.exists() {
        return Ok(None);
    }
    ensure_regular_directory(&directory)?;
    for path in snapshot_paths(&directory)?.into_iter().rev() {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_EXECUTION_BYTES
        {
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let execution: Execution = match serde_json::from_slice(&bytes) {
            Ok(execution) => execution,
            Err(_) => continue,
        };
        if validate(&execution).is_ok() {
            return Ok(Some(execution));
        }
    }
    Ok(None)
}

fn persist(workspace: &Workspace, execution: &Execution) -> Result<()> {
    let _guard = update_lock()
        .lock()
        .map_err(|_| anyhow::anyhow!("execution update lock poisoned"))?;
    let directory = execution_directory(workspace)?;
    fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create execution store {}", directory.display()))?;
    ensure_regular_directory(&directory)?;
    let bytes = serde_json::to_vec(execution).context("cannot encode execution checkpoint")?;
    if bytes.len() as u64 > MAX_EXECUTION_BYTES {
        bail!("execution checkpoint exceeds persistent store size bound");
    }
    let target = directory.join(format!("{:020}.json", execution.revision));
    let temp = directory.join(format!(".execution-{}.tmp", Uuid::new_v4().simple()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temp)
        .with_context(|| format!("cannot create execution temp {}", temp.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("cannot write execution temp {}", temp.display()))?;
    file.sync_all()
        .with_context(|| format!("cannot sync execution temp {}", temp.display()))?;
    match fs::hard_link(&temp, &target) {
        Ok(()) => {
            let _ = fs::remove_file(&temp);
        }
        Err(error) => {
            let _ = fs::remove_file(&temp);
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                bail!(
                    "execution revision changed concurrently; refresh execution_status and retry"
                );
            }
            return Err(error).context("cannot atomically publish execution checkpoint");
        }
    }
    prune(&directory)?;
    Ok(())
}

fn validate(execution: &Execution) -> Result<()> {
    let checkpoint = &execution.checkpoint;
    if execution.schema_version != EXECUTION_SCHEMA_VERSION
        || execution.execution_id.trim().is_empty()
        || execution.execution_id.len() > 96
        || execution.revision == 0
        || execution.objective.trim().is_empty()
        || execution.objective.len() > 1_000
        || checkpoint.worklist_revision == 0
        || checkpoint.blocked_items > checkpoint.open_items
        || checkpoint.runnable.len() > 64
        || checkpoint.blockers.len() > MAX_EXECUTION_BLOCKERS
        || execution.proposal.as_ref().is_some_and(|proposal| {
            proposal.summary.trim().is_empty()
                || proposal.summary.chars().count() > MAX_PROPOSAL_SUMMARY_CHARS
                || proposal.proposer.trim().is_empty()
                || proposal.proposer.len() > 256
                || proposal.worklist_revision == 0
                || proposal.proposed_at_ms == 0
        })
        || execution
            .pending_directive
            .as_ref()
            .is_some_and(|directive| {
                directive.summary.trim().is_empty()
                    || directive.summary.chars().count() > MAX_STEERING_SUMMARY_CHARS
                    || directive.requested_by.trim().is_empty()
                    || directive.requested_by.len() > 256
                    || directive.scopes.len() > MAX_STEERING_SCOPES
                    || directive.worklist_revision == 0
                    || directive.requested_at_ms == 0
                    || directive
                        .reconciliation_plan_id
                        .as_deref()
                        .is_some_and(|id| id.trim().is_empty() || id.len() > 160)
                    || directive.objective.as_ref().is_some_and(|value| {
                        value.trim().is_empty() || value.chars().count() > 1_000
                    })
                    || directive
                        .scopes
                        .iter()
                        .any(|scope| scope.trim().is_empty() || scope.len() > 300)
                    || directive
                        .verification_strength
                        .as_deref()
                        .is_some_and(|value| !matches!(value, "full" | "adversarial"))
            })
        || execution
            .verification_floor
            .as_deref()
            .is_some_and(|value| !matches!(value, "full" | "adversarial"))
        || execution.lineage.as_ref().is_some_and(|lineage| {
            lineage.parent_execution_id.trim().is_empty()
                || lineage.parent_execution_id.len() > 96
                || lineage.handoff_count == 0
                || lineage.requested_by.trim().is_empty()
                || lineage.requested_by.len() > 256
                || lineage.summary.trim().is_empty()
                || lineage.summary.chars().count() > MAX_STEERING_SUMMARY_CHARS
                || lineage.handed_off_at_ms == 0
        })
    {
        bail!("invalid execution checkpoint");
    }
    for id in &checkpoint.runnable {
        if id.is_empty() || id.len() > 64 {
            bail!("execution checkpoint contains invalid runnable item");
        }
    }
    for blocker in &checkpoint.blockers {
        if blocker.trim().is_empty() || blocker.len() > 1_000 {
            bail!("execution checkpoint contains invalid blocker");
        }
    }
    for id in [
        checkpoint.reconciliation_plan_id.as_deref(),
        checkpoint.verification_plan_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if id.trim().is_empty() || id.len() > 160 {
            bail!("execution checkpoint contains invalid plan id");
        }
    }
    Ok(())
}

fn append_execution_blocker(checkpoint: &mut ExecutionCheckpoint, blocker: &str) {
    if checkpoint.blockers.iter().any(|value| value == blocker) {
        return;
    }
    if checkpoint.blockers.len() >= MAX_EXECUTION_BLOCKERS {
        checkpoint.blockers.pop();
    }
    checkpoint.blockers.push(blocker.to_owned());
    checkpoint.blockers.sort();
    checkpoint.blockers.dedup();
}

fn execution_directory(workspace: &Workspace) -> Result<PathBuf> {
    Ok(crate::evidence_store::workspace_state_directory(workspace)?.join("execution"))
}

fn snapshot_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(directory)
        .with_context(|| format!("cannot list execution store {}", directory.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            (name.ends_with(".json")
                && name[..name.len().saturating_sub(5)]
                    .bytes()
                    .all(|byte| byte.is_ascii_digit()))
            .then(|| entry.path())
        })
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn prune(directory: &Path) -> Result<()> {
    let paths = snapshot_paths(directory)?;
    let excess = paths.len().saturating_sub(MAX_EXECUTION_SNAPSHOTS);
    for path in paths.into_iter().take(excess) {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

fn ensure_regular_directory(directory: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(directory)
        .with_context(|| format!("cannot inspect execution store {}", directory.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("execution store path is not a regular directory");
    }
    Ok(())
}

fn update_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "../../tests/unit/runtime/execution.rs"]
mod tests;
