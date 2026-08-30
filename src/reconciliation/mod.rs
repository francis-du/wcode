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

#[derive(Clone, Debug, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReconciliationTask {
    pub id: String,
    pub kind: ReconciliationTaskKind,
    pub subject: String,
    pub description: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReconciliationPlan {
    pub id: String,
    pub workspace: String,
    pub risk_level: RiskLevel,
    pub design_changes: Vec<DesignChange>,
    pub drift_ids: Vec<String>,
    pub impacted_components: Vec<String>,
    pub impacted_symbols: Vec<String>,
    pub impacted_tests: Vec<String>,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReconciliationTaskRun {
    pub task: ReconciliationTask,
    pub status: ReconciliationRunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
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
        if executor.trim().is_empty() || executor.len() > 256 {
            return Err(ReconciliationError::InvalidExecutor);
        }
        let runnable = self
            .tasks
            .iter()
            .enumerate()
            .find(|(_, run)| {
                run.status == ReconciliationRunStatus::Pending
                    && matches!(
                        run.task.kind,
                        ReconciliationTaskKind::Design
                            | ReconciliationTaskKind::Implementation
                            | ReconciliationTaskKind::Review
                    )
                    && (kinds.is_empty() || kinds.contains(&run.task.kind))
                    && run.task.depends_on.iter().all(|dependency| {
                        self.tasks.iter().any(|candidate| {
                            candidate.task.id == *dependency
                                && candidate.status == ReconciliationRunStatus::Completed
                        })
                    })
            })
            .map(|(index, _)| index)
            .ok_or(ReconciliationError::NoRunnableTask)?;
        let run = &mut self.tasks[runnable];
        run.status = ReconciliationRunStatus::Claimed;
        run.claimed_by = Some(executor.to_owned());
        self.updated_at_ms = now_ms();
        Ok(run.clone())
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
        self.updated_at_ms = now_ms();
        Ok(run.clone())
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
        run.summary = None;
        run.artifact_digest = None;
        self.updated_at_ms = now_ms();
        Ok(run.clone())
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
                run.summary = Some(summary.clone());
                changed = true;
            }
        }
        if changed {
            self.updated_at_ms = now_ms();
        }
        changed
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
