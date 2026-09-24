use crate::risk::RiskLevel;
use crate::verification::VerificationPlan;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignChangeKind {
    Added,
    Modified,
    Removed,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesignChange {
    pub subject: String,
    pub kind: DesignChangeKind,
    pub summary: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconciliationTaskKind {
    Design,
    Implementation,
    Verification,
    Review,
    HumanApproval,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReconciliationTask {
    pub id: String,
    pub kind: ReconciliationTaskKind,
    pub subject: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub write_scopes: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChangeIntent {
    ChangeBehavior {
        target: String,
        desired: Value,
        #[serde(default)]
        constraints: Vec<String>,
    },
    RenameSymbol {
        symbol: String,
        new_name: String,
    },
    AddVerification {
        subject: String,
        verification_kind: String,
    },
    UpdateDesign {
        subject: String,
        reason: String,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct ImpactAnalysis {
    pub workspace: String,
    pub changed_paths: Vec<String>,
    pub impacted_components: Vec<String>,
    pub impacted_requirements: Vec<String>,
    pub impacted_acceptance: Vec<String>,
    pub impacted_symbols: Vec<String>,
    pub transitive_callers: usize,
    pub graph_provider: String,
    pub graph_precision: String,
    pub graph_truncated: bool,
    pub public_api: bool,
    pub security_boundary: bool,
    pub risk_level: RiskLevel,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ReconciliationPlan {
    pub id: String,
    pub workspace: String,
    pub risk_level: RiskLevel,
    pub design_changes: Vec<DesignChange>,
    pub drift_ids: Vec<String>,
    pub impacted_components: Vec<String>,
    pub impacted_symbols: Vec<String>,
    pub impacted_tests: Vec<String>,
    #[serde(default)]
    pub impacted_acceptance: Vec<String>,
    pub implementation_tasks: Vec<ReconciliationTask>,
    pub change_intents: Vec<ChangeIntent>,
    pub verification_plan: VerificationPlan,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconciliationRunStatus {
    Pending,
    Claimed,
    Completed,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconciliationClaimMode {
    ReadOnly,
    SharedWriter,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReconciliationClaimOwnership {
    pub mode: ReconciliationClaimMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_binding: Option<String>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ReconciliationClaimSelection<'a> {
    pub task_id: Option<&'a str>,
    pub owner_binding: Option<&'a str>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReconciliationTaskRun {
    pub task: ReconciliationTask,
    pub status: ReconciliationRunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ownership: Option<ReconciliationClaimOwnership>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_digest: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReconciliationExecution {
    pub plan_id: String,
    pub workspace: String,
    pub tasks: Vec<ReconciliationTaskRun>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconciliationTaskSubmission {
    pub success: bool,
    pub summary: String,
    #[serde(default)]
    pub artifact_digest: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReconciliationExecutionStatus {
    pub execution: ReconciliationExecution,
    pub pending: usize,
    pub claimed: usize,
    pub completed: usize,
    pub failed: usize,
    pub blocked: usize,
    pub intent_checked: usize,
    pub intent_blockers: Vec<String>,
    pub converged: bool,
}

impl ReconciliationExecution {
    pub fn from_plan(plan: &ReconciliationPlan) -> Result<Self, ReconciliationError> {
        plan.validate()?;
        let now = now_ms();
        Ok(Self {
            plan_id: plan.id.clone(),
            workspace: plan.workspace.clone(),
            tasks: plan
                .implementation_tasks
                .iter()
                .cloned()
                .map(|task| ReconciliationTaskRun {
                    task,
                    status: ReconciliationRunStatus::Pending,
                    claimed_by: None,
                    ownership: None,
                    summary: None,
                    artifact_digest: None,
                })
                .collect(),
            created_at_ms: now,
            updated_at_ms: now,
        })
    }

    pub fn claim(
        &mut self,
        executor: &str,
        kinds: &[ReconciliationTaskKind],
    ) -> Result<ReconciliationTaskRun, ReconciliationError> {
        self.claim_task(executor, kinds, None)
    }

    pub fn claim_task(
        &mut self,
        executor: &str,
        kinds: &[ReconciliationTaskKind],
        task_id: Option<&str>,
    ) -> Result<ReconciliationTaskRun, ReconciliationError> {
        self.claim_task_with_owner_binding(executor, kinds, task_id, None)
    }

    pub(crate) fn claim_task_with_owner_binding(
        &mut self,
        executor: &str,
        kinds: &[ReconciliationTaskKind],
        task_id: Option<&str>,
        owner_binding: Option<&str>,
    ) -> Result<ReconciliationTaskRun, ReconciliationError> {
        if executor.trim().is_empty() || executor.len() > 256 {
            return Err(ReconciliationError::InvalidExecutor);
        }
        if let (Some(owner_binding), Some(task_id)) = (owner_binding, task_id) {
            if let Some(run) = self.tasks.iter().find(|run| {
                run.task.id == task_id
                    && run.status == ReconciliationRunStatus::Claimed
                    && run.claimed_by.as_deref() == Some(executor)
                    && (kinds.is_empty() || kinds.contains(&run.task.kind))
                    && run
                        .ownership
                        .as_ref()
                        .and_then(|ownership| ownership.owner_binding.as_deref())
                        == Some(owner_binding)
            }) {
                return Ok(run.clone());
            }
        }
        let runnable_id = self
            .claimable_task(kinds, task_id)
            .map(|run| run.task.id.clone())
            .ok_or(ReconciliationError::NoRunnableTask)?;
        let runnable = self
            .tasks
            .iter()
            .position(|run| run.task.id == runnable_id)
            .ok_or(ReconciliationError::NoRunnableTask)?;
        let mut ownership = default_claim_ownership(self.tasks[runnable].task.kind);
        ownership.owner_binding = owner_binding.map(str::to_owned);
        let run = &mut self.tasks[runnable];
        run.status = ReconciliationRunStatus::Claimed;
        run.claimed_by = Some(executor.to_owned());
        run.ownership = Some(ownership);
        let claimed = run.clone();
        self.touch();
        Ok(claimed)
    }

    pub(crate) fn claimable_task(
        &self,
        kinds: &[ReconciliationTaskKind],
        task_id: Option<&str>,
    ) -> Option<&ReconciliationTaskRun> {
        self.claimable_tasks(kinds, task_id).into_iter().next()
    }

    pub(crate) fn claimable_tasks(
        &self,
        kinds: &[ReconciliationTaskKind],
        task_id: Option<&str>,
    ) -> Vec<&ReconciliationTaskRun> {
        self.tasks
            .iter()
            .filter(|run| {
                if run.status != ReconciliationRunStatus::Pending
                    || !matches!(
                        run.task.kind,
                        ReconciliationTaskKind::Design
                            | ReconciliationTaskKind::Implementation
                            | ReconciliationTaskKind::Review
                    )
                    || (!kinds.is_empty() && !kinds.contains(&run.task.kind))
                    || task_id.is_some_and(|id| run.task.id != id)
                    || !run.task.depends_on.iter().all(|dependency| {
                        self.tasks.iter().any(|candidate| {
                            candidate.task.id == *dependency
                                && candidate.status == ReconciliationRunStatus::Completed
                        })
                    })
                {
                    return false;
                }
                if run.task.kind == ReconciliationTaskKind::Review {
                    return true;
                }
                !self.tasks.iter().any(|claimed| {
                    claimed.status == ReconciliationRunStatus::Claimed
                        && claimed.ownership.as_ref().is_some_and(|ownership| {
                            ownership.mode == ReconciliationClaimMode::SharedWriter
                        })
                        && write_scopes_conflict(&claimed.task.write_scopes, &run.task.write_scopes)
                })
            })
            .collect()
    }

    pub fn submit(
        &mut self,
        task_id: &str,
        executor: &str,
        submission: ReconciliationTaskSubmission,
    ) -> Result<ReconciliationTaskRun, ReconciliationError> {
        if submission.summary.trim().is_empty()
            || submission.summary.chars().count() > 2_000
            || submission
                .artifact_digest
                .as_ref()
                .is_some_and(|digest| digest.trim().is_empty() || digest.len() > 512)
        {
            return Err(ReconciliationError::InvalidSubmission);
        }
        let run = self
            .tasks
            .iter_mut()
            .find(|run| run.task.id == task_id)
            .ok_or(ReconciliationError::UnknownTask)?;
        if run.status != ReconciliationRunStatus::Claimed
            || run.claimed_by.as_deref() != Some(executor)
        {
            return Err(ReconciliationError::InvalidTaskState);
        }
        run.status = if submission.success {
            ReconciliationRunStatus::Completed
        } else {
            ReconciliationRunStatus::Failed
        };
        run.summary = Some(submission.summary);
        run.artifact_digest = submission.artifact_digest;
        let submitted = run.clone();
        self.touch();
        Ok(submitted)
    }

    pub fn retry(&mut self, task_id: &str) -> Result<ReconciliationTaskRun, ReconciliationError> {
        let run = self
            .tasks
            .iter_mut()
            .find(|run| run.task.id == task_id)
            .ok_or(ReconciliationError::UnknownTask)?;
        if run.status != ReconciliationRunStatus::Failed
            || !matches!(
                run.task.kind,
                ReconciliationTaskKind::Design
                    | ReconciliationTaskKind::Implementation
                    | ReconciliationTaskKind::Review
            )
        {
            return Err(ReconciliationError::InvalidTaskState);
        }
        run.status = ReconciliationRunStatus::Pending;
        run.claimed_by = None;
        run.ownership = None;
        run.summary = None;
        run.artifact_digest = None;
        let retried = run.clone();
        self.touch();
        Ok(retried)
    }

    pub fn set_system_task(
        &mut self,
        kind: ReconciliationTaskKind,
        completed: bool,
        summary: String,
    ) -> bool {
        let completed_ids = self
            .tasks
            .iter()
            .filter(|run| run.status == ReconciliationRunStatus::Completed)
            .map(|run| run.task.id.clone())
            .collect::<HashSet<_>>();
        let mut changed = false;
        for run in self.tasks.iter_mut().filter(|run| run.task.kind == kind) {
            let dependencies_completed = run
                .task
                .depends_on
                .iter()
                .all(|dependency| completed_ids.contains(dependency));
            if completed && !dependencies_completed {
                continue;
            }
            let desired = if completed {
                ReconciliationRunStatus::Completed
            } else {
                ReconciliationRunStatus::Pending
            };
            if run.status != desired || run.summary.as_deref() != Some(summary.as_str()) {
                run.status = desired;
                run.claimed_by = None;
                run.ownership = None;
                run.summary = Some(summary.clone());
                changed = true;
            }
        }
        if changed {
            self.touch();
        }
        changed
    }

    fn touch(&mut self) {
        self.updated_at_ms = now_ms().max(self.updated_at_ms.saturating_add(1));
    }

    pub fn status(&self) -> ReconciliationExecutionStatus {
        let pending = self
            .tasks
            .iter()
            .filter(|run| run.status == ReconciliationRunStatus::Pending)
            .count();
        let claimed = self
            .tasks
            .iter()
            .filter(|run| run.status == ReconciliationRunStatus::Claimed)
            .count();
        let completed = self
            .tasks
            .iter()
            .filter(|run| run.status == ReconciliationRunStatus::Completed)
            .count();
        let failed = self
            .tasks
            .iter()
            .filter(|run| run.status == ReconciliationRunStatus::Failed)
            .count();
        let blocked = self
            .tasks
            .iter()
            .filter(|run| {
                run.status == ReconciliationRunStatus::Pending
                    && run.task.depends_on.iter().any(|dependency| {
                        !self.tasks.iter().any(|candidate| {
                            candidate.task.id == *dependency
                                && candidate.status == ReconciliationRunStatus::Completed
                        })
                    })
            })
            .count();
        ReconciliationExecutionStatus {
            execution: self.clone(),
            pending,
            claimed,
            completed,
            failed,
            blocked,
            intent_checked: 0,
            intent_blockers: Vec::new(),
            converged: !self.tasks.is_empty() && completed == self.tasks.len() && failed == 0,
        }
    }

    pub fn validate(&self) -> Result<(), ReconciliationError> {
        if self.plan_id.trim().is_empty()
            || self.workspace.trim().is_empty()
            || self.tasks.len() > 256
            || self.tasks.iter().any(|run| {
                run.task.id.trim().is_empty()
                    || run
                        .claimed_by
                        .as_ref()
                        .is_some_and(|executor| executor.trim().is_empty() || executor.len() > 256)
                    || run.summary.as_ref().is_some_and(|summary| {
                        summary.trim().is_empty() || summary.chars().count() > 2_000
                    })
                    || run
                        .artifact_digest
                        .as_ref()
                        .is_some_and(|digest| digest.trim().is_empty() || digest.len() > 512)
            })
        {
            return Err(ReconciliationError::InvalidExecution);
        }
        Ok(())
    }
}

impl ReconciliationPlan {
    pub fn validate(&self) -> Result<(), ReconciliationError> {
        if self.id.trim().is_empty()
            || self.workspace.trim().is_empty()
            || self.implementation_tasks.len() > 256
            || self.change_intents.len() > 256
            || self.impacted_components.len() > 512
            || self.impacted_symbols.len() > 2_000
            || self.impacted_tests.len() > 2_000
            || self.impacted_acceptance.len() > 2_000
        {
            return Err(ReconciliationError::InvalidPlan);
        }
        let ids = self
            .implementation_tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<HashSet<_>>();
        if ids.len() != self.implementation_tasks.len() {
            return Err(ReconciliationError::InvalidPlan);
        }
        for task in &self.implementation_tasks {
            if task.id.trim().is_empty()
                || task.subject.trim().is_empty()
                || task.description.trim().is_empty()
                || task.write_scopes.len() > 32
                || task
                    .write_scopes
                    .iter()
                    .any(|scope| !valid_write_scope(scope))
                || task.write_scopes.iter().collect::<HashSet<_>>().len() != task.write_scopes.len()
                || (!matches!(
                    task.kind,
                    ReconciliationTaskKind::Design | ReconciliationTaskKind::Implementation
                ) && !task.write_scopes.is_empty())
                || task.depends_on.len() > 64
                || task
                    .depends_on
                    .iter()
                    .any(|dependency| dependency == &task.id || !ids.contains(dependency.as_str()))
            {
                return Err(ReconciliationError::InvalidPlan);
            }
        }
        if has_dependency_cycle(&self.implementation_tasks) {
            return Err(ReconciliationError::InvalidPlan);
        }
        Ok(())
    }
}

fn has_dependency_cycle(tasks: &[ReconciliationTask]) -> bool {
    let mut completed = HashSet::with_capacity(tasks.len());
    loop {
        let before = completed.len();
        for task in tasks {
            if !completed.contains(task.id.as_str())
                && task
                    .depends_on
                    .iter()
                    .all(|dependency| completed.contains(dependency.as_str()))
            {
                completed.insert(task.id.as_str());
            }
        }
        if completed.len() == tasks.len() {
            return false;
        }
        if completed.len() == before {
            return !tasks.is_empty();
        }
    }
}

pub(crate) fn write_scopes_conflict(left: &[String], right: &[String]) -> bool {
    if left.is_empty() || right.is_empty() {
        return true;
    }
    left.iter().any(|left| {
        right
            .iter()
            .any(|right| scope_contains(left, right) || scope_contains(right, left))
    })
}

pub(crate) fn scope_contains(parent: &str, child: &str) -> bool {
    scope_contains_with_case(parent, child, cfg!(any(windows, target_os = "macos")))
}

fn scope_contains_with_case(parent: &str, child: &str, case_insensitive: bool) -> bool {
    if !case_insensitive {
        return parent == child
            || child
                .strip_prefix(parent)
                .is_some_and(|suffix| suffix.starts_with('/'));
    }
    let parent = parent.as_bytes();
    let child = child.as_bytes();
    if child.len() < parent.len() || !child[..parent.len()].eq_ignore_ascii_case(parent) {
        return false;
    }
    child.len() == parent.len() || child.get(parent.len()) == Some(&b'/')
}

fn valid_write_scope(scope: &str) -> bool {
    if scope.trim().is_empty()
        || scope.len() > 300
        || scope.starts_with('/')
        || scope.contains('\\')
        || scope.contains(['\0', '\n', '\r'])
    {
        return false;
    }
    scope.split('/').all(|component| {
        !component.is_empty() && component != "." && component != ".." && !component.contains(':')
    })
}

fn default_claim_ownership(kind: ReconciliationTaskKind) -> ReconciliationClaimOwnership {
    ReconciliationClaimOwnership {
        mode: if kind == ReconciliationTaskKind::Review {
            ReconciliationClaimMode::ReadOnly
        } else {
            ReconciliationClaimMode::SharedWriter
        },
        owner_binding: None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReconciliationError {
    InvalidPlan,
    InvalidExecution,
    InvalidExecutor,
    NoRunnableTask,
    UnknownTask,
    InvalidTaskState,
    InvalidSubmission,
}

impl std::fmt::Display for ReconciliationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPlan => "reconciliation plan is invalid or exceeds its bounds",
            Self::InvalidExecution => "reconciliation execution state is invalid or unbounded",
            Self::InvalidExecutor => "reconciliation executor identity is invalid",
            Self::NoRunnableTask => {
                "no reconciliation task is currently runnable for this executor"
            }
            Self::UnknownTask => "reconciliation task does not exist",
            Self::InvalidTaskState => "reconciliation task is not in the required claimed state",
            Self::InvalidSubmission => "reconciliation task submission is invalid",
        })
    }
}

impl std::error::Error for ReconciliationError {}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "../../tests/unit/reconciliation/mod.rs"]
mod tests;
