use crate::evidence::Revision;
#[path = "execution_support.rs"]
mod support;
#[path = "execution_view.rs"]
mod view;
use crate::harness::ToolHarness;
use crate::workspace::Workspace;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub(crate) use support::{handoff, propose, steer};
pub(crate) use view::active_summary;
use view::{empty_status, status_value};

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

impl ExecutionDirective {
    fn normalize_replan_requirement(&mut self) {
        self.requires_replan |= matches!(
            self.kind,
            ExecutionDirectiveKind::RefineObjective | ExecutionDirectiveKind::ChangeScope
        ) && self.reconciliation_plan_id.is_some();
    }
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
    let current = support::load(workspace)?;
    if current.is_none() && crate::worklist::is_complete(&worklist) && !restart {
        return Ok(empty_status());
    }

    let repository_revision = harness.current_revision(workspace)?;
    let current_revision_plan_id = harness
        .reconciliation_history(workspace, 20)?
        .into_iter()
        .find(|plan| {
            plan.workspace == workspace_id
                && plan.verification_plan.revision.as_ref() == Some(&repository_revision)
        })
        .map(|plan| plan.id);
    let steering_replan_ready = current.as_ref().is_some_and(|execution| {
        execution
            .pending_directive
            .as_ref()
            .is_some_and(|directive| {
                directive.requires_replan && worklist.revision > directive.worklist_revision
            })
    });
    let reconciliation_plan_id = support::select_reconciliation_plan_id(
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
    let verification_floor = current
        .as_ref()
        .and_then(|execution| execution.verification_floor.as_deref());
    let bound_verification = match current
        .as_ref()
        .and_then(|execution| execution.checkpoint.verification_plan_id.as_deref())
    {
        Some(id) => harness.verification_status_if_present(workspace_id, workspace, id)?,
        None => None,
    }
    .filter(|status| status.plan.revision.as_ref() == Some(&repository_revision));
    let verification = match bound_verification {
        Some(status)
            if support::verification_plan_satisfies_floor(&status.plan, verification_floor) =>
        {
            Some(status)
        }
        _ => harness.verification_status_for_revision(
            workspace_id,
            workspace,
            &repository_revision,
        )?,
    };
    let verification_floor_satisfied = verification.as_ref().is_some_and(|status| {
        support::verification_plan_satisfies_floor(&status.plan, verification_floor)
    });
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
    support::sync(workspace, &worklist, current, restart, signals)
}

pub(crate) fn stored_status(workspace: &Workspace) -> Result<Value> {
    Ok(match support::load(workspace)? {
        Some(execution) => status_value(&execution),
        None => empty_status(),
    })
}

pub(crate) fn verification_risk_floor(
    workspace: &Workspace,
    risk: crate::risk::RiskLevel,
) -> Result<crate::risk::RiskLevel> {
    let floor = support::load(workspace)?
        .and_then(|execution| execution.verification_floor)
        .unwrap_or_default();
    Ok(match floor.as_str() {
        "adversarial" => risk.max(crate::risk::RiskLevel::High),
        "full" => risk.max(crate::risk::RiskLevel::Medium),
        _ => risk,
    })
}

#[cfg(test)]
#[path = "../../tests/unit/runtime/execution.rs"]
mod tests;

#[cfg(test)]
#[path = "../../tests/unit/runtime/execution_scope.rs"]
mod scope_tests;

#[cfg(test)]
#[path = "../../tests/unit/runtime/execution_replan.rs"]
mod replan_tests;
