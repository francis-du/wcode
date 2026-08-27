use crate::code_index::{CodeIndex, SymbolResolution};
use crate::design::{self, CodeRef, Priority, VerificationRef};
use crate::evidence::{Confidence, Evidence, EvidenceKind, EvidenceResult, Revision};
use crate::evidence_store;
use crate::graph::{EdgeKind, NodeKind, SoftwareGraphSnapshot};
use crate::graph_provider_store;
use crate::harness::{ChangeReviewReport, VerificationReport};
use crate::reconcile::{
    ChangeIntent, DesignChange, DesignChangeKind, ImpactAnalysis, ReconciliationExecution,
    ReconciliationExecutionStatus, ReconciliationPlan, ReconciliationRunStatus, ReconciliationTask,
    ReconciliationTaskKind, ReconciliationTaskRun, ReconciliationTaskSubmission,
};
use crate::reconciliation_execution_store;
use crate::reconciliation_store;
use crate::risk::{Risk, RiskCategory, RiskLevel, VerificationProfile};
use crate::scopes;
use crate::semantic::{self, SemanticCandidateInput, SemanticFact, SemanticMatch, SemanticStatus};
use crate::semantic_store;
use crate::verification::{
    ReviewSubmission, ReviewerRole, StageSubmission, VerificationJob, VerificationPlan,
    VerificationStage, VerificationState, VerificationStatus,
};
use crate::verification_store;
use crate::workspace::Workspace;
use anyhow::{anyhow, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const MAX_EVIDENCE_RECORDS: usize = 4_096;
const MAX_DRIFT_FINDINGS: usize = 256;
const MAX_CONTEXT_ITEMS: usize = 64;
const MAX_REVISION_FILES: usize = 10_000;
const MAX_IMPACT_GRAPH_FILES: usize = 1_500;
const MAX_IMPACT_GRAPH_SYMBOLS: usize = 5_000;
const MAX_TRANSITIVE_IMPACT_SYMBOLS: usize = 2_000;

#[derive(Default)]
struct IntelligenceState {
    evidence: Vec<StoredEvidence>,
    verification: VerificationState,
    verification_loaded: BTreeSet<String>,
    latest_risks: BTreeMap<String, Vec<Risk>>,
}

#[derive(Clone, Debug, Serialize)]
struct StoredEvidence {
    workspace: String,
    evidence: Evidence,
}

#[derive(Clone)]
pub struct SoftwareIntelligenceRuntime {
    state: Arc<Mutex<IntelligenceState>>,
}

impl Default for SoftwareIntelligenceRuntime {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(IntelligenceState::default())),
        }
    }
}

pub use crate::intelligence_types::{
    CodeStatBreakdown, CoverageDimension, DesignContextItem, DesignStatus, DriftFinding, DriftKind,
    DriftStatus, EvidenceStatus, FeatureAcceptanceView, FeatureComponentView,
    FeatureConstraintView, FeatureConvergenceState, FeatureDecisionView,
    FeatureDependencyAlignment, FeatureImplementationView, FeatureRequirementView, GraphContext,
    GraphContextEdge, GraphContextNode, ProjectChangeView, ProjectCodeStats,
    ProjectConvergenceSummary, ProjectGraphDeltaView, ProjectObservatory, ProjectProofSummary,
    ProjectRevisionView, RequirementTrace, RequirementTraceStatus, RiskStatus, SemanticStatusView,
    SoftwareContext, SoftwareContextRequest, TraceReference, TraceReferenceKind,
    TraceabilityStatus,
};

#[path = "analysis.rs"]
mod analysis;
#[path = "context.rs"]
mod context;
#[path = "observatory.rs"]
mod observatory;
#[path = "observatory_architecture.rs"]
mod observatory_architecture;
use analysis::*;
use context::*;
pub(crate) use observatory::{build_project_observatory, ObservatoryInput};

const MAX_TRACE_REQUIREMENTS: usize = 200;
const MAX_TRACE_DIAGNOSTICS: usize = 128;

impl SoftwareIntelligenceRuntime {
    pub(crate) fn current_revision(&self, workspace: &Workspace) -> Result<Revision> {
        workspace_revision(workspace)
    }

    pub fn design_status(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
    ) -> Result<DesignStatus> {
        let load = design::load_design(workspace)?;
        let errors = load.error_count();
        let warnings = load.warning_count();
        let state = load.state;
        Ok(DesignStatus {
            workspace: workspace_id.into(),
            initialized: load.initialized,
            valid: load.initialized && errors == 0,
            schema_version: 1,
            design_root: load.design_root,
            files_loaded: load.files_loaded,
            project: state.project.as_ref().map(|project| project.name.clone()),
            requirements: state.requirements.len(),
            components: state.components.len(),
            constraints: state.constraints.len(),
            decisions: state.decisions.len(),
            acceptance_criteria: state.acceptance.len(),
            design_nodes: state.node_count(),
            errors,
            warnings,
            diagnostics: load.diagnostics,
        })
    }

    pub(crate) fn traceability_status(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
    ) -> Result<TraceabilityStatus> {
        let load = design::load_design(workspace)?;
        self.traceability_status_from_load(
            workspace_id.into(),
            workspace,
            code_index,
            known_checks,
            &load,
        )
    }

    fn traceability_status_from_load(
        &self,
        workspace_id: String,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
        load: &design::DesignLoad,
    ) -> Result<TraceabilityStatus> {
        let errors = load.error_count();
        let initialized = load.initialized;
        let mut diagnostics = load.diagnostics.clone();
        let state = &load.state;
        let requirements_total = state.requirements.len();
        let mut requirement_components_covered = 0usize;
        let mut implementation_total = 0usize;
        let mut implementation_resolved = 0usize;
        let mut verification_total = 0usize;
        let mut verification_resolved = 0usize;
        let mut complete_requirements = 0usize;
        let mut partial_requirements = 0usize;
        let mut missing_requirements = 0usize;
        let mut requirements = Vec::new();

        for requirement in state.requirements.values() {
            let components_resolved = !requirement.implemented_by.is_empty()
                && requirement
                    .implemented_by
                    .iter()
                    .all(|id| state.components.contains_key(id));
            requirement_components_covered += usize::from(components_resolved);
            let implementation = requirement
                .implemented_by
                .iter()
                .filter_map(|id| state.components.get(id))
                .flat_map(|component| {
                    component.implementation.iter().map(|reference| {
                        resolve_code_reference(code_index, workspace, &component.id, reference)
                    })
                })
                .collect::<Vec<_>>();
            let verification = requirement
                .acceptance
                .iter()
                .filter_map(|id| state.acceptance.get(id))
                .flat_map(|criterion| {
                    criterion.verification.iter().map(|reference| {
                        resolve_verification_reference(
                            code_index,
                            workspace,
                            known_checks,
                            &criterion.id,
                            reference,
                        )
                    })
                })
                .collect::<Vec<_>>();

            implementation_total += implementation.len();
            implementation_resolved += implementation.iter().filter(|item| item.resolved).count();
            verification_total += verification.len();
            verification_resolved += verification.iter().filter(|item| item.resolved).count();
            let status =
                requirement_trace_status(components_resolved, &implementation, &verification);
            match status {
                RequirementTraceStatus::Complete => complete_requirements += 1,
                RequirementTraceStatus::Partial => partial_requirements += 1,
                RequirementTraceStatus::Missing => missing_requirements += 1,
            }
            if requirements.len() < MAX_TRACE_REQUIREMENTS {
                requirements.push(RequirementTrace {
                    id: requirement.id.clone(),
                    title: requirement.title.clone(),
                    priority: requirement.priority,
                    components: requirement.implemented_by.clone(),
                    acceptance_criteria: requirement.acceptance.clone(),
                    implementation,
                    verification,
                    status,
                });
            }
        }

        let truncated =
            requirements_total > requirements.len() || diagnostics.len() > MAX_TRACE_DIAGNOSTICS;
        diagnostics.truncate(MAX_TRACE_DIAGNOSTICS);
        Ok(TraceabilityStatus {
            workspace: workspace_id,
            initialized,
            valid_design: initialized && errors == 0,
            requirements_total,
            requirements_returned: requirements.len(),
            truncated,
            requirement_to_component: CoverageDimension::new(
                requirement_components_covered,
                requirements_total,
            ),
            design_to_implementation: CoverageDimension::new(
                implementation_resolved,
                implementation_total,
            ),
            acceptance_to_verification: CoverageDimension::new(
                verification_resolved,
                verification_total,
            ),
            complete_requirements,
            partial_requirements,
            missing_requirements,
            requirements,
            diagnostics,
        })
    }

    pub(crate) fn drift_status(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
        review: &ChangeReviewReport,
    ) -> Result<DriftStatus> {
        let workspace_id = workspace_id.into();
        let traceability =
            self.traceability_status(workspace_id.clone(), workspace, code_index, known_checks)?;
        let state = design::load_design(workspace)?.state;
        Ok(build_drift_status(
            workspace_id,
            &state,
            &traceability,
            review,
        ))
    }

    pub(crate) fn risk_status(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
        review: &ChangeReviewReport,
    ) -> Result<RiskStatus> {
        let workspace_id = workspace_id.into();
        let traceability =
            self.traceability_status(workspace_id.clone(), workspace, code_index, known_checks)?;
        let state = design::load_design(workspace)?.state;
        let drift = build_drift_status(workspace_id.clone(), &state, &traceability, review);
        let (level, risks) = assess_risk(&workspace_id, review, &traceability, &drift);
        let profile = VerificationProfile::for_risk(level);
        self.state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?
            .latest_risks
            .insert(workspace_id.clone(), risks.clone());
        Ok(RiskStatus {
            workspace: workspace_id,
            level,
            profile,
            risks,
            drift,
            traceability,
        })
    }

    pub(crate) fn impact_analysis(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
        review: &ChangeReviewReport,
    ) -> Result<ImpactAnalysis> {
        let workspace_id = workspace_id.into();
        let risk = self.risk_status(
            workspace_id.clone(),
            workspace,
            code_index,
            known_checks,
            review,
        )?;
        let state = design::load_design(workspace)?.state;
        let mut graph = code_index.software_graph(
            workspace_id.clone(),
            workspace,
            ".",
            MAX_IMPACT_GRAPH_FILES,
            MAX_IMPACT_GRAPH_SYMBOLS,
        )?;
        graph_provider_store::overlay_latest(workspace, &mut graph)?;
        Ok(build_impact_analysis(
            workspace_id,
            &state,
            review,
            risk.level,
            Some(&graph),
        ))
    }

    pub(crate) fn create_verification_plan(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
        review: &ChangeReviewReport,
    ) -> Result<VerificationPlan> {
        let workspace_id = workspace_id.into();
        let risk = self.risk_status(
            workspace_id.clone(),
            workspace,
            code_index,
            known_checks,
            review,
        )?;
        self.create_plan_for_risk(&workspace_id, workspace, risk.level)
    }

    pub(crate) fn verification_claim(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        reviewer: &str,
        capabilities: &[String],
        role: Option<ReviewerRole>,
    ) -> Result<VerificationJob> {
        self.ensure_verification_loaded(workspace_id, workspace)?;
        let capabilities = capabilities.iter().cloned().collect::<BTreeSet<_>>();
        let (job, snapshot) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            let job = state
                .verification
                .claim(workspace_id, reviewer, &capabilities, role)?;
            let snapshot = state.verification.workspace_snapshot(workspace_id);
            (job, snapshot)
        };
        verification_store::persist(workspace, &snapshot)?;
        Ok(job)
    }

    pub(crate) fn verification_submit(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        job_id: &str,
        reviewer: &str,
        submission: ReviewSubmission,
    ) -> Result<VerificationJob> {
        self.ensure_verification_loaded(workspace_id, workspace)?;
        let revision = workspace_revision(workspace)?;
        let (job, produced, snapshot) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            let job = state
                .verification
                .submit(workspace_id, job_id, reviewer, submission)?;
            let status = state.verification.status(&job.plan_id)?;
            let evidence = VerificationState::evidence_for_submission(
                &job,
                self.next_id("EV"),
                revision.clone(),
                status.plan.policy.clone(),
            )?;
            let mut produced = vec![evidence.clone()];
            push_evidence(&mut state.evidence, workspace_id, evidence);
            if status.disagreements > 0 {
                let producer = format!("verification-mesh:{}", status.plan.id);
                let already_recorded = state.evidence.iter().any(|stored| {
                    stored.workspace == workspace_id
                        && stored.evidence.producer == producer
                        && stored.evidence.result == EvidenceResult::Disagree
                });
                if !already_recorded {
                    let mut disagreement = Evidence::new(
                        self.next_id("EV"),
                        status.plan.subject.clone(),
                        EvidenceKind::ModelReview,
                        producer,
                        revision,
                        EvidenceResult::Disagree,
                        Confidence::High,
                    )?;
                    disagreement.policy = Some(status.plan.policy.clone());
                    disagreement.artifact_digest = Some(digest_text(&format!(
                        "plan={};submitted={};disagreements={}",
                        status.plan.id, status.submitted, status.disagreements
                    )));
                    disagreement.validate()?;
                    produced.push(disagreement.clone());
                    push_evidence(&mut state.evidence, workspace_id, disagreement);
                }
            }
            let snapshot = state.verification.workspace_snapshot(workspace_id);
            (job, produced, snapshot)
        };
        verification_store::persist(workspace, &snapshot)?;
        for evidence in &produced {
            evidence_store::persist(workspace, evidence)?;
        }
        Ok(job)
    }

    pub(crate) fn verification_stage_submit(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        submission: StageSubmission,
    ) -> Result<Evidence> {
        submission.validate()?;
        self.ensure_verification_loaded(workspace_id, workspace)?;
        let plan = {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            state.verification.status(plan_id)?.plan
        };
        if plan.workspace != workspace_id {
            return Err(anyhow!(
                "verification plan does not belong to the selected workspace"
            ));
        }
        let required = match submission.stage {
            VerificationStage::Property => plan.require_property,
            VerificationStage::Mutation => plan.require_mutation,
            VerificationStage::Fuzz => plan.require_fuzz,
            VerificationStage::RuntimeCanary => plan
                .deterministic_checks
                .iter()
                .any(|check| check == "runtime-gate"),
        };
        if !required {
            return Err(anyhow!("verification stage is not required by this plan"));
        }
        let kind = match submission.stage {
            VerificationStage::Property => EvidenceKind::Property,
            VerificationStage::Mutation => EvidenceKind::Mutation,
            VerificationStage::Fuzz => EvidenceKind::Fuzz,
            VerificationStage::RuntimeCanary => EvidenceKind::Runtime,
        };
        let result = match submission.verdict {
            crate::verification::ReviewVerdict::Pass => EvidenceResult::Pass,
            crate::verification::ReviewVerdict::Fail => EvidenceResult::Fail,
            crate::verification::ReviewVerdict::Inconclusive => EvidenceResult::Inconclusive,
        };
        let mut evidence = Evidence::new(
            self.next_id("EV"),
            plan.subject.clone(),
            kind,
            submission.producer.clone(),
            workspace_revision(workspace)?,
            result,
            Confidence::High,
        )?;
        evidence.model = submission.model.clone();
        evidence.policy =
            Some(format!("{}/stage/{:?}", plan.policy, submission.stage).to_ascii_lowercase());
        evidence.artifact_digest = Some(submission.artifact_digest.clone());
        evidence.summary = Some(submission.summary.clone());
        evidence.validate()?;
        evidence_store::persist(workspace, &evidence)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?;
        push_evidence(&mut state.evidence, workspace_id, evidence.clone());
        Ok(evidence)
    }

    pub(crate) fn verification_approve(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        approver: &str,
        statement: &str,
    ) -> Result<Evidence> {
        let approver = approver.trim();
        let statement = statement.trim();
        if approver.is_empty()
            || approver.len() > 256
            || statement.is_empty()
            || statement.len() > 2_000
        {
            return Err(anyhow!("human approval identity or statement is invalid"));
        }
        self.ensure_verification_loaded(workspace_id, workspace)?;
        let plan = {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            state.verification.status(plan_id)?.plan
        };
        if plan.workspace != workspace_id {
            return Err(anyhow!(
                "verification plan does not belong to the selected workspace"
            ));
        }
        if !plan.require_human_approval {
            return Err(anyhow!("verification plan does not require human approval"));
        }
        let mut evidence = Evidence::new(
            self.next_id("EV"),
            plan.subject.clone(),
            EvidenceKind::HumanApproval,
            format!("human:{approver}"),
            workspace_revision(workspace)?,
            EvidenceResult::Pass,
            Confidence::High,
        )?;
        evidence.policy = Some(format!("{}/human-approval", plan.policy));
        evidence.artifact_digest = Some(format!("sha256:{}", digest_text(statement)));
        evidence.summary = Some(statement.to_owned());
        evidence.validate()?;
        evidence_store::persist(workspace, &evidence)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?;
        push_evidence(&mut state.evidence, workspace_id, evidence.clone());
        Ok(evidence)
    }

    pub(crate) fn verification_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<VerificationStatus> {
        self.ensure_verification_loaded(workspace_id, workspace)?;
        let (mut status, memory_evidence) = {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            let status = state.verification.status(plan_id)?;
            let memory_evidence = state
                .evidence
                .iter()
                .filter(|stored| stored.workspace == status.plan.workspace)
                .map(|stored| stored.evidence.clone())
                .collect::<Vec<_>>();
            (status, memory_evidence)
        };
        let current_subject = format!("change:{}", workspace_revision(workspace)?.code);
        if current_subject != status.plan.subject {
            status
                .blockers
                .push("workspace-revision-changed-since-plan".into());
        }
        let mut evidence = evidence_store::load(workspace)?;
        evidence.extend(memory_evidence);
        status.deterministic_result = evidence
            .iter()
            .filter(|record| {
                record.subject == status.plan.subject && record.kind == EvidenceKind::Verification
            })
            .max_by_key(|record| record.timestamp_ms)
            .map(|record| record.result);
        match status.deterministic_result {
            Some(EvidenceResult::Pass) => {}
            Some(EvidenceResult::Fail) => status
                .blockers
                .push("deterministic-verification-failed".into()),
            Some(EvidenceResult::Inconclusive | EvidenceResult::Disagree) => status
                .blockers
                .push("deterministic-verification-inconclusive".into()),
            None => status
                .blockers
                .push("deterministic-verification-missing".into()),
        }
        let require_property = status.plan.require_property;
        let require_mutation = status.plan.require_mutation;
        let require_fuzz = status.plan.require_fuzz;
        apply_stage_status(
            &mut status,
            &evidence,
            VerificationStage::Property,
            EvidenceKind::Property,
            require_property,
        );
        apply_stage_status(
            &mut status,
            &evidence,
            VerificationStage::Mutation,
            EvidenceKind::Mutation,
            require_mutation,
        );
        apply_stage_status(
            &mut status,
            &evidence,
            VerificationStage::Fuzz,
            EvidenceKind::Fuzz,
            require_fuzz,
        );
        let runtime_required = status
            .plan
            .deterministic_checks
            .iter()
            .any(|check| check == "runtime-gate");
        apply_stage_status(
            &mut status,
            &evidence,
            VerificationStage::RuntimeCanary,
            EvidenceKind::Runtime,
            runtime_required,
        );
        status.human_approval = evidence
            .iter()
            .filter(|record| {
                record.subject == status.plan.subject && record.kind == EvidenceKind::HumanApproval
            })
            .max_by_key(|record| record.timestamp_ms)
            .is_some_and(|record| record.result == EvidenceResult::Pass);
        if status.plan.require_human_approval && !status.human_approval {
            status.blockers.push("human-approval-required".into());
        }
        status.blockers.sort();
        status.blockers.dedup();
        status.ready = status.blockers.is_empty()
            && status.submitted == status.plan.job_ids.len()
            && status.deterministic_result == Some(EvidenceResult::Pass);
        Ok(status)
    }

    pub(crate) fn verification_history(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        limit: usize,
    ) -> Result<Vec<VerificationStatus>> {
        self.ensure_verification_loaded(workspace_id, workspace)?;
        let mut plans = {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            state.verification.plans_for_workspace(workspace_id)
        };
        plans.reverse();
        let mut history = Vec::new();
        for plan in plans.into_iter().take(limit.clamp(1, 100)) {
            history.push(self.verification_status(workspace_id, workspace, &plan.id)?);
        }
        Ok(history)
    }

    pub(crate) fn record_verification_report(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        report: &VerificationReport,
    ) -> Result<Vec<Evidence>> {
        let revision = workspace_revision(workspace)?;
        let design = design::load_design(workspace)?;
        let mut produced = Vec::new();
        for check in &report.checks {
            let mut evidence = Evidence::new(
                self.next_id("EV"),
                format!("verification:{}", check.id),
                evidence_kind_for_check(&check.id),
                check.command.clone(),
                revision.clone(),
                if check.success {
                    EvidenceResult::Pass
                } else {
                    EvidenceResult::Fail
                },
                Confidence::Deterministic,
            )?;
            evidence.policy = Some(format!("deterministic/{}/v1", report.level));
            evidence.artifact_digest = Some(format!(
                "sha256:{}",
                digest_text(&format!(
                    "{}\n{:?}\n{}\n{}",
                    check.command, check.exit_code, check.stdout_tail, check.stderr_tail
                ))
            ));
            produced.push(evidence);
        }
        let mut aggregate = Evidence::new(
            self.next_id("EV"),
            format!("change:{}", revision.code),
            EvidenceKind::Verification,
            "verify_project".into(),
            revision.clone(),
            if report.passed {
                EvidenceResult::Pass
            } else {
                EvidenceResult::Fail
            },
            Confidence::Deterministic,
        )?;
        aggregate.policy = Some(format!("deterministic/{}/v1", report.level));
        aggregate.artifact_digest = Some(format!(
            "sha256:{}",
            digest_text(&format!(
                "{}\n{}\n{}\n{}",
                report.level, report.checks_run, report.checks_failed, report.summary
            ))
        ));
        produced.push(aggregate);
        for criterion in design.state.acceptance.values() {
            let outcomes = criterion
                .verification
                .iter()
                .filter_map(|reference| verification_reference_outcome(reference, report))
                .collect::<Vec<_>>();
            if outcomes.is_empty() {
                continue;
            }
            let passed = outcomes.iter().all(|outcome| *outcome);
            let mut evidence = Evidence::new(
                self.next_id("EV"),
                criterion.id.clone(),
                EvidenceKind::IntegrationTest,
                "deterministic-verification-mesh".into(),
                revision.clone(),
                if passed {
                    EvidenceResult::Pass
                } else {
                    EvidenceResult::Fail
                },
                Confidence::Deterministic,
            )?;
            evidence.policy = Some(format!("acceptance/{}/v1", report.level));
            produced.push(evidence);
        }
        for evidence in &produced {
            evidence_store::persist(workspace, evidence)?;
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?;
        for evidence in &produced {
            push_evidence(&mut state.evidence, workspace_id, evidence.clone());
        }
        Ok(produced)
    }

    pub fn evidence_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        subject: Option<&str>,
        limit: usize,
    ) -> Result<EvidenceStatus> {
        let mut records = evidence_store::load(workspace)?
            .into_iter()
            .map(|evidence| (evidence.id.clone(), evidence))
            .collect::<BTreeMap<_, _>>();
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?;
        for stored in state
            .evidence
            .iter()
            .filter(|stored| stored.workspace == workspace_id)
        {
            records.insert(stored.evidence.id.clone(), stored.evidence.clone());
        }
        let mut matching = records
            .into_values()
            .filter(|evidence| subject.is_none_or(|subject| evidence.subject == subject))
            .collect::<Vec<_>>();
        matching.sort_by_key(|evidence| evidence.timestamp_ms);
        let total = matching.len();
        let passed = matching
            .iter()
            .filter(|evidence| evidence.result == EvidenceResult::Pass)
            .count();
        let failed = matching
            .iter()
            .filter(|evidence| evidence.result == EvidenceResult::Fail)
            .count();
        let inconclusive = matching
            .iter()
            .filter(|evidence| evidence.result == EvidenceResult::Inconclusive)
            .count();
        let disagreed = matching
            .iter()
            .filter(|evidence| evidence.result == EvidenceResult::Disagree)
            .count();
        let deterministic = matching
            .iter()
            .filter(|evidence| evidence.confidence == Confidence::Deterministic)
            .count();
        let limit = limit.clamp(1, 500);
        let evidence = matching.into_iter().rev().take(limit).collect::<Vec<_>>();
        Ok(EvidenceStatus {
            workspace: workspace_id.to_owned(),
            total,
            passed,
            failed,
            inconclusive,
            disagreed,
            deterministic,
            truncated: total > evidence.len(),
            evidence,
        })
    }

    pub fn semantic_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        limit: usize,
    ) -> Result<SemanticStatusView> {
        let mut facts = semantic_store::load(workspace)?;
        facts.sort_by_key(|fact| fact.timestamp_ms);
        let total = facts.len();
        let candidates = facts
            .iter()
            .filter(|fact| fact.status == SemanticStatus::Candidate)
            .count();
        let confirmed = facts
            .iter()
            .filter(|fact| fact.status == SemanticStatus::Confirmed)
            .count();
        let retired = facts
            .iter()
            .filter(|fact| fact.status == SemanticStatus::Retired)
            .count();
        let limit = limit.clamp(1, 500);
        let facts = facts.into_iter().rev().take(limit).collect::<Vec<_>>();
        Ok(SemanticStatusView {
            workspace: workspace_id.to_owned(),
            total,
            candidates,
            confirmed,
            retired,
            truncated: total > facts.len(),
            facts,
        })
    }

    pub fn semantic_query(
        &self,
        workspace: &Workspace,
        query: &str,
        requested_scopes: &[String],
        include_candidates: bool,
        limit: usize,
    ) -> Result<Vec<SemanticMatch>> {
        Ok(semantic::query_scoped(
            semantic_store::load(workspace)?,
            query,
            requested_scopes,
            include_candidates,
            limit,
        ))
    }

    pub fn semantic_record_candidate(
        &self,
        workspace: &Workspace,
        input: SemanticCandidateInput,
    ) -> Result<SemanticFact> {
        if input.origin == crate::semantic::SemanticOrigin::Provider
            && input
                .provider
                .as_ref()
                .is_none_or(|provider| provider.trim().is_empty())
        {
            return Err(anyhow!(
                "provider semantic candidates require provider provenance"
            ));
        }
        let fact = SemanticFact::candidate(self.next_id("SEM"), input)?;
        semantic_store::persist(workspace, &fact)?;
        Ok(fact)
    }

    pub fn semantic_confirm(
        &self,
        workspace: &Workspace,
        fact_id: &str,
        attested_by: &str,
    ) -> Result<SemanticFact> {
        let fact = semantic_store::load_one(workspace, fact_id)?
            .ok_or_else(|| anyhow!("semantic fact does not exist"))?;
        if fact.status == SemanticStatus::Retired {
            return Err(anyhow!(
                "retired semantic facts cannot be confirmed; record a new candidate"
            ));
        }
        if fact.status == SemanticStatus::Confirmed {
            return Ok(fact);
        }
        let confirmed = fact.confirm(attested_by.trim().to_owned())?;
        semantic_store::persist(workspace, &confirmed)?;
        Ok(confirmed)
    }

    pub fn semantic_retire(
        &self,
        workspace: &Workspace,
        fact_id: &str,
        attested_by: &str,
    ) -> Result<SemanticFact> {
        let fact = semantic_store::load_one(workspace, fact_id)?
            .ok_or_else(|| anyhow!("semantic fact does not exist"))?;
        if fact.status == SemanticStatus::Retired {
            return Ok(fact);
        }
        let retired = fact.retire(attested_by.trim().to_owned())?;
        semantic_store::persist(workspace, &retired)?;
        Ok(retired)
    }

    pub(crate) fn software_context(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
        request: &SoftwareContextRequest,
    ) -> Result<SoftwareContext> {
        let workspace_id = workspace_id.into();
        let query = request.query.trim();
        if query.is_empty() {
            return Err(anyhow!("software context query must not be empty"));
        }
        let design_load = design::load_design(workspace)?;
        let state = &design_load.state;
        let budget = request.budget.clamp(1_000, 64_000);
        let item_cap = (budget / 900).clamp(4, MAX_CONTEXT_ITEMS);
        let requested_scopes = scopes::canonicalize(&request.scopes);
        let semantic_matches = semantic::query_scoped(
            semantic_store::load(workspace)?,
            query,
            &requested_scopes,
            true,
            item_cap,
        );
        let mut semantic_expansion = query.to_owned();
        for scope in &requested_scopes {
            semantic_expansion.push(' ');
            semantic_expansion.push_str(scope);
        }
        for matched in semantic_matches
            .iter()
            .filter(|matched| matched.fact.status == SemanticStatus::Confirmed)
            .take(8)
        {
            for term in matched.fact.expansion_terms() {
                semantic_expansion.push(' ');
                semantic_expansion.push_str(&term);
            }
        }
        let tokens = context_tokens(&semantic_expansion);
        let requirements = ranked_context_ids(
            state.requirements.values().map(|requirement| {
                (
                    requirement.id.clone(),
                    format!(
                        "{} {} {}",
                        requirement.id, requirement.title, requirement.intent
                    ),
                )
            }),
            query,
            &tokens,
            item_cap,
        );
        let components = ranked_context_ids(
            state.components.values().map(|component| {
                (
                    component.id.clone(),
                    format!(
                        "{} {} {}",
                        component.id,
                        component.name,
                        component.responsibilities.join(" ")
                    ),
                )
            }),
            query,
            &tokens,
            item_cap,
        );
        let constraints = ranked_context_ids(
            state.constraints.values().map(|constraint| {
                (
                    constraint.id.clone(),
                    format!(
                        "{} {} {}",
                        constraint.id, constraint.title, constraint.statement
                    ),
                )
            }),
            query,
            &tokens,
            item_cap,
        );
        let acceptance_criteria = ranked_context_ids(
            state.acceptance.values().map(|criterion| {
                (
                    criterion.id.clone(),
                    format!(
                        "{} {} {}",
                        criterion.id, criterion.title, criterion.statement
                    ),
                )
            }),
            query,
            &tokens,
            item_cap,
        );
        let decisions = ranked_context_ids(
            state.decisions.values().map(|decision| {
                (
                    decision.id.clone(),
                    format!(
                        "{} {} {} {}",
                        decision.id, decision.title, decision.decision, decision.rationale
                    ),
                )
            }),
            query,
            &tokens,
            item_cap,
        );
        let design_items = design_context_items(
            state,
            &requirements,
            &components,
            &constraints,
            &acceptance_criteria,
            &decisions,
            item_cap,
        );

        let symbol_cap = item_cap.min(24);
        let mut symbols = Vec::new();
        let mut symbol_ids = HashSet::new();
        let mut symbol_queries = tokens
            .iter()
            .filter(|token| token.len() >= 3)
            .take(4)
            .cloned()
            .collect::<Vec<_>>();
        if symbol_queries.is_empty() {
            symbol_queries.push(query.to_owned());
        }
        let source_roots = scopes::source_roots_for(&requested_scopes);
        for source_root in &source_roots {
            let search = code_index.find_symbols_many(
                workspace_id.clone(),
                workspace,
                &symbol_queries,
                source_root,
                None,
                symbol_cap,
            )?;
            for symbol in search
                .get("results")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
            {
                let key = symbol
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| symbol.to_string());
                if symbol_ids.insert(key) {
                    symbols.push(symbol.clone());
                    if symbols.len() >= symbol_cap {
                        break;
                    }
                }
            }
            if symbols.len() >= symbol_cap {
                break;
            }
        }
        let graph_context = provider_graph_context(
            workspace,
            &semantic_expansion,
            &tokens,
            &symbols,
            item_cap.min(32),
        )?;
        let known_risks = self
            .state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?
            .latest_risks
            .get(&workspace_id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .take(item_cap)
            .collect::<Vec<_>>();
        let mut coverage = self.traceability_status_from_load(
            workspace_id.clone(),
            workspace,
            code_index,
            known_checks,
            &design_load,
        )?;
        let requirement_rank = requirements
            .iter()
            .enumerate()
            .map(|(rank, id)| (id.as_str(), rank))
            .collect::<HashMap<_, _>>();
        let relevant_components = components
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        if !requirement_rank.is_empty() || !relevant_components.is_empty() {
            coverage.requirements.retain(|requirement| {
                requirement_rank.contains_key(requirement.id.as_str())
                    || requirement
                        .components
                        .iter()
                        .any(|component| relevant_components.contains(component.as_str()))
            });
            coverage.requirements.sort_by_key(|requirement| {
                requirement_rank
                    .get(requirement.id.as_str())
                    .copied()
                    .unwrap_or(usize::MAX)
            });
        }
        if coverage.requirements.len() > item_cap {
            coverage.requirements.truncate(item_cap);
        }
        coverage.requirements_returned = coverage.requirements.len();
        coverage.truncated |= coverage.requirements_returned < coverage.requirements_total;
        if coverage.diagnostics.len() > item_cap {
            coverage.diagnostics.truncate(item_cap);
            coverage.truncated = true;
        }
        Ok(SoftwareContext {
            workspace: workspace_id,
            query: query.to_owned(),
            intent: request.intent.clone(),
            budget,
            scopes: requested_scopes,
            requirements,
            components,
            constraints,
            acceptance_criteria,
            decisions,
            design_items,
            semantic_matches,
            symbols,
            graph_context,
            known_risks,
            coverage,
        })
    }

    pub(crate) fn reconciliation_plan(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        code_index: &CodeIndex,
        known_checks: &HashSet<String>,
        review: &ChangeReviewReport,
    ) -> Result<ReconciliationPlan> {
        let workspace_id = workspace_id.into();
        let risk = self.risk_status(
            workspace_id.clone(),
            workspace,
            code_index,
            known_checks,
            review,
        )?;
        let design_state = design::load_design(workspace)?.state;
        let mut graph = code_index.software_graph(
            workspace_id.clone(),
            workspace,
            ".",
            MAX_IMPACT_GRAPH_FILES,
            MAX_IMPACT_GRAPH_SYMBOLS,
        )?;
        graph_provider_store::overlay_latest(workspace, &mut graph)?;
        let impact = build_impact_analysis(
            workspace_id.clone(),
            &design_state,
            review,
            risk.level,
            Some(&graph),
        );
        let verification_plan = self.create_plan_for_risk(&workspace_id, workspace, risk.level)?;
        let mut tasks = Vec::new();
        let mut intents = Vec::new();
        for finding in &risk.drift.findings {
            let task_id = self.next_id("RT");
            let (kind, intent) = match finding.kind {
                DriftKind::ImplementationDrift => (
                    ReconciliationTaskKind::Implementation,
                    ChangeIntent::ChangeBehavior {
                        target: finding.subject.clone(),
                        desired: serde_json::json!({"state":"conform_to_design"}),
                        constraints: Vec::new(),
                    },
                ),
                DriftKind::DesignDrift => (
                    ReconciliationTaskKind::Design,
                    ChangeIntent::UpdateDesign {
                        subject: finding.subject.clone(),
                        reason: finding.message.clone(),
                    },
                ),
            };
            tasks.push(ReconciliationTask {
                id: task_id,
                kind,
                subject: finding.subject.clone(),
                description: finding.message.clone(),
                depends_on: Vec::new(),
            });
            intents.push(intent);
        }
        let prior_tasks = tasks.iter().map(|task| task.id.clone()).collect::<Vec<_>>();
        tasks.push(ReconciliationTask {
            id: self.next_id("RT"),
            kind: ReconciliationTaskKind::Verification,
            subject: verification_plan.subject.clone(),
            description: format!(
                "Run {} deterministic verification and complete {} blind reviewer job(s).",
                verification_plan.deterministic_level,
                verification_plan.job_ids.len()
            ),
            depends_on: prior_tasks,
        });
        if verification_plan.require_human_approval {
            let verification_task = tasks
                .last()
                .map(|task| task.id.clone())
                .into_iter()
                .collect();
            tasks.push(ReconciliationTask {
                id: self.next_id("RT"),
                kind: ReconciliationTaskKind::HumanApproval,
                subject: verification_plan.subject.clone(),
                description: "Critical-risk reconciliation requires explicit human approval."
                    .into(),
                depends_on: verification_task,
            });
        }
        let impacted_tests = risk
            .traceability
            .requirements
            .iter()
            .filter(|requirement| impact.impacted_requirements.contains(&requirement.id))
            .flat_map(|requirement| requirement.verification.iter())
            .filter(|reference| reference.kind == TraceReferenceKind::Test)
            .map(|reference| reference.target.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let plan = ReconciliationPlan {
            id: self.next_id("RP"),
            workspace: workspace_id,
            risk_level: risk.level,
            design_changes: design_changes_from_review(review),
            drift_ids: risk
                .drift
                .findings
                .iter()
                .map(|finding| finding.id.clone())
                .collect(),
            impacted_components: impact.impacted_components,
            impacted_symbols: impact.impacted_symbols,
            impacted_tests,
            implementation_tasks: tasks,
            change_intents: intents,
            verification_plan,
        };
        plan.validate()?;
        reconciliation_store::persist(workspace, &plan)?;
        if reconciliation_execution_store::load(workspace, &plan.id)?.is_none() {
            let execution = ReconciliationExecution::from_plan(&plan)?;
            reconciliation_execution_store::persist(workspace, &execution)?;
        }
        Ok(plan)
    }

    pub(crate) fn reconciliation_status(
        &self,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<ReconciliationPlan> {
        reconciliation_store::load(workspace, plan_id)?
            .ok_or_else(|| anyhow!("reconciliation plan does not exist"))
    }

    pub(crate) fn reconciliation_history(
        &self,
        workspace: &Workspace,
        limit: usize,
    ) -> Result<Vec<ReconciliationPlan>> {
        reconciliation_store::recent(workspace, limit)
    }

    pub(crate) fn reconciliation_execution_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<ReconciliationExecutionStatus> {
        let plan = self.reconciliation_status(workspace, plan_id)?;
        if plan.workspace != workspace_id {
            return Err(anyhow!(
                "reconciliation plan does not belong to the selected workspace"
            ));
        }
        let mut execution = reconciliation_execution_store::load(workspace, plan_id)?
            .unwrap_or(ReconciliationExecution::from_plan(&plan)?);
        let verification =
            self.verification_status(workspace_id, workspace, &plan.verification_plan.id)?;
        let mut changed = execution.set_system_task(
            ReconciliationTaskKind::Verification,
            verification.ready,
            if verification.ready {
                "Verification Plan is ready with all required evidence.".into()
            } else {
                format!(
                    "Verification blockers: {}",
                    verification.blockers.join(", ")
                )
            },
        );
        if plan.verification_plan.require_human_approval {
            changed |= execution.set_system_task(
                ReconciliationTaskKind::HumanApproval,
                verification.human_approval,
                if verification.human_approval {
                    "Explicit HumanApproval Evidence is present.".into()
                } else {
                    "Explicit HumanApproval Evidence is still required.".into()
                },
            );
        }
        if changed || reconciliation_execution_store::load(workspace, plan_id)?.is_none() {
            reconciliation_execution_store::persist(workspace, &execution)?;
        }
        Ok(execution.status())
    }

    pub(crate) fn reconciliation_claim(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        executor: &str,
        kinds: &[ReconciliationTaskKind],
    ) -> Result<ReconciliationTaskRun> {
        self.reconciliation_execution_status(workspace_id, workspace, plan_id)?;
        let mut execution = reconciliation_execution_store::load(workspace, plan_id)?
            .ok_or_else(|| anyhow!("reconciliation execution state does not exist"))?;
        let run = execution.claim(executor, kinds)?;
        reconciliation_execution_store::persist(workspace, &execution)?;
        Ok(run)
    }

    pub(crate) fn reconciliation_submit(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        task_id: &str,
        executor: &str,
        submission: ReconciliationTaskSubmission,
    ) -> Result<ReconciliationTaskRun> {
        let plan = self.reconciliation_status(workspace, plan_id)?;
        if plan.workspace != workspace_id {
            return Err(anyhow!(
                "reconciliation plan does not belong to the selected workspace"
            ));
        }
        let mut execution = reconciliation_execution_store::load(workspace, plan_id)?
            .unwrap_or(ReconciliationExecution::from_plan(&plan)?);
        let run = execution.submit(task_id, executor, submission)?;
        reconciliation_execution_store::persist(workspace, &execution)?;
        let mut evidence = Evidence::new(
            self.next_id("EV"),
            format!("reconciliation-task:{}", run.task.id),
            EvidenceKind::Reconciliation,
            format!("executor:{executor}"),
            workspace_revision(workspace)?,
            if run.status == ReconciliationRunStatus::Completed {
                EvidenceResult::Pass
            } else {
                EvidenceResult::Fail
            },
            Confidence::High,
        )?;
        evidence.policy = Some(format!("reconciliation/{plan_id}"));
        evidence.summary = run.summary.clone();
        evidence.artifact_digest = run.artifact_digest.clone();
        evidence.validate()?;
        evidence_store::persist(workspace, &evidence)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?;
        push_evidence(&mut state.evidence, workspace_id, evidence);
        Ok(run)
    }

    pub(crate) fn reconciliation_retry(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        task_id: &str,
    ) -> Result<ReconciliationTaskRun> {
        let plan = self.reconciliation_status(workspace, plan_id)?;
        if plan.workspace != workspace_id {
            return Err(anyhow!(
                "reconciliation plan does not belong to the selected workspace"
            ));
        }
        let mut execution = reconciliation_execution_store::load(workspace, plan_id)?
            .unwrap_or(ReconciliationExecution::from_plan(&plan)?);
        let run = execution.retry(task_id)?;
        reconciliation_execution_store::persist(workspace, &execution)?;
        Ok(run)
    }

    fn create_plan_for_risk(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        risk_level: RiskLevel,
    ) -> Result<VerificationPlan> {
        self.ensure_verification_loaded(workspace_id, workspace)?;
        let revision = workspace_revision(workspace)?;
        let subject = format!("change:{}", revision.code);
        let plan_id = self.next_id("VP");
        let job_ids = std::iter::repeat_with(|| self.next_id("VJ"));
        let (plan, snapshot) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            let plan = state.verification.create_plan(
                plan_id,
                workspace_id.to_owned(),
                subject,
                risk_level,
                job_ids,
            )?;
            let snapshot = state.verification.workspace_snapshot(workspace_id);
            (plan, snapshot)
        };
        verification_store::persist(workspace, &snapshot)?;
        Ok(plan)
    }

    fn ensure_verification_loaded(&self, workspace_id: &str, workspace: &Workspace) -> Result<()> {
        {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            if state.verification_loaded.contains(workspace_id) {
                return Ok(());
            }
        }
        let persisted = verification_store::load(workspace)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?;
        if state.verification_loaded.contains(workspace_id) {
            return Ok(());
        }
        if let Some(snapshot) = persisted {
            state.verification.restore_workspace(snapshot)?;
        }
        state.verification_loaded.insert(workspace_id.to_owned());
        Ok(())
    }

    fn next_id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", Uuid::new_v4().simple())
    }
}

fn apply_stage_status(
    status: &mut VerificationStatus,
    evidence: &[Evidence],
    stage: VerificationStage,
    kind: EvidenceKind,
    required: bool,
) {
    if !required {
        return;
    }
    let key = match stage {
        VerificationStage::Property => "property",
        VerificationStage::Mutation => "mutation",
        VerificationStage::Fuzz => "fuzz",
        VerificationStage::RuntimeCanary => "runtime_canary",
    };
    let mut latest_by_producer = BTreeMap::<String, &Evidence>::new();
    for record in evidence
        .iter()
        .filter(|record| record.subject == status.plan.subject && record.kind == kind)
    {
        let replace = latest_by_producer
            .get(&record.producer)
            .is_none_or(|current| {
                (current.timestamp_ms, current.id.as_str())
                    < (record.timestamp_ms, record.id.as_str())
            });
        if replace {
            latest_by_producer.insert(record.producer.clone(), record);
        }
    }
    let producer_results = latest_by_producer
        .into_iter()
        .map(|(producer, record)| (producer, record.result))
        .collect::<BTreeMap<_, _>>();
    let result = if producer_results
        .values()
        .any(|result| *result == EvidenceResult::Fail)
    {
        Some(EvidenceResult::Fail)
    } else if producer_results
        .values()
        .any(|result| *result == EvidenceResult::Disagree)
    {
        Some(EvidenceResult::Disagree)
    } else if producer_results
        .values()
        .any(|result| *result == EvidenceResult::Inconclusive)
    {
        Some(EvidenceResult::Inconclusive)
    } else if !producer_results.is_empty() {
        Some(EvidenceResult::Pass)
    } else {
        None
    };
    if let Some(result) = result {
        status.stage_results.insert(key.to_owned(), result);
    }
    if !producer_results.is_empty() {
        status
            .stage_producer_results
            .insert(key.to_owned(), producer_results);
    }
    match result {
        Some(EvidenceResult::Pass) => {}
        Some(EvidenceResult::Fail) => status.blockers.push(format!("{key}-evidence-failed")),
        Some(EvidenceResult::Inconclusive | EvidenceResult::Disagree) => {
            status.blockers.push(format!("{key}-evidence-inconclusive"))
        }
        None => status.blockers.push(format!("{key}-evidence-missing")),
    }
}

fn requirement_trace_status(
    components_resolved: bool,
    implementation: &[TraceReference],
    verification: &[TraceReference],
) -> RequirementTraceStatus {
    let implementation_complete =
        !implementation.is_empty() && implementation.iter().all(|reference| reference.resolved);
    let verification_complete =
        !verification.is_empty() && verification.iter().all(|reference| reference.resolved);
    if components_resolved && implementation_complete && verification_complete {
        RequirementTraceStatus::Complete
    } else if components_resolved
        || implementation.iter().any(|reference| reference.resolved)
        || verification.iter().any(|reference| reference.resolved)
    {
        RequirementTraceStatus::Partial
    } else {
        RequirementTraceStatus::Missing
    }
}

fn resolve_code_reference(
    code_index: &CodeIndex,
    workspace: &Workspace,
    owner: &str,
    reference: &CodeRef,
) -> TraceReference {
    match reference {
        CodeRef::File { path } => match workspace.source_stamp(path) {
            Ok(_) => TraceReference {
                owner: owner.to_owned(),
                kind: TraceReferenceKind::File,
                target: path.clone(),
                resolved: true,
                provider: "filesystem".into(),
                precision: "deterministic".into(),
                node_id: Some(format!("file:{path}")),
                revision: None,
                message: None,
            },
            Err(error) => unresolved_reference(
                owner,
                TraceReferenceKind::File,
                path,
                "filesystem",
                "deterministic",
                error.to_string(),
            ),
        },
        CodeRef::Symbol { path, symbol } => resolve_symbol_reference(
            code_index,
            workspace,
            owner,
            TraceReferenceKind::Symbol,
            path,
            symbol,
        ),
    }
}

fn resolve_verification_reference(
    code_index: &CodeIndex,
    workspace: &Workspace,
    known_checks: &HashSet<String>,
    owner: &str,
    reference: &VerificationRef,
) -> TraceReference {
    match reference {
        VerificationRef::Test { path, symbol } => resolve_symbol_reference(
            code_index,
            workspace,
            owner,
            TraceReferenceKind::Test,
            path,
            symbol,
        ),
        VerificationRef::Check { id } if known_checks.contains(id) => TraceReference {
            owner: owner.to_owned(),
            kind: TraceReferenceKind::Check,
            target: id.clone(),
            resolved: true,
            provider: "harness".into(),
            precision: "deterministic".into(),
            node_id: Some(format!("verification:{id}")),
            revision: None,
            message: None,
        },
        VerificationRef::Check { id } => unresolved_reference(
            owner,
            TraceReferenceKind::Check,
            id,
            "harness",
            "deterministic",
            "verification check is not present in the inferred project profile",
        ),
    }
}

fn resolve_symbol_reference(
    code_index: &CodeIndex,
    workspace: &Workspace,
    owner: &str,
    kind: TraceReferenceKind,
    path: &str,
    symbol: &str,
) -> TraceReference {
    let target = format!("{path}::{symbol}");
    match code_index.resolve_symbol(workspace, path, symbol) {
        Ok(Some(resolution)) => resolved_symbol_reference(owner, kind, target, resolution),
        Ok(None) => unresolved_reference(
            owner,
            kind,
            &target,
            "tree-sitter",
            "syntax",
            "no unique symbol definition matched the declared reference",
        ),
        Err(error) => unresolved_reference(
            owner,
            kind,
            &target,
            "tree-sitter",
            "syntax",
            error.to_string(),
        ),
    }
}

fn resolved_symbol_reference(
    owner: &str,
    kind: TraceReferenceKind,
    target: String,
    resolution: SymbolResolution,
) -> TraceReference {
    TraceReference {
        owner: owner.to_owned(),
        kind,
        target,
        resolved: true,
        provider: "tree-sitter".into(),
        precision: "syntax".into(),
        node_id: Some(format!("symbol:{}", resolution.id)),
        revision: Some(resolution.revision),
        message: Some(format!(
            "resolved `{}` as `{}` ({}) in {}",
            resolution.name, resolution.qualified_name, resolution.kind, resolution.path
        )),
    }
}

fn unresolved_reference(
    owner: &str,
    kind: TraceReferenceKind,
    target: &str,
    provider: &str,
    precision: &str,
    message: impl AsRef<str>,
) -> TraceReference {
    TraceReference {
        owner: owner.to_owned(),
        kind,
        target: target.to_owned(),
        resolved: false,
        provider: provider.to_owned(),
        precision: precision.to_owned(),
        node_id: None,
        revision: None,
        message: Some(bounded_message(message.as_ref())),
    }
}

fn bounded_message(message: &str) -> String {
    message.chars().take(300).collect()
}

fn provider_graph_context(
    workspace: &Workspace,
    query: &str,
    tokens: &[String],
    symbols: &[serde_json::Value],
    limit: usize,
) -> Result<GraphContext> {
    let providers = graph_provider_store::load_latest(workspace)?;
    if providers.is_empty() {
        return Ok(GraphContext::default());
    }
    let limit = limit.clamp(1, MAX_CONTEXT_ITEMS);
    let needle = query.to_ascii_lowercase();
    let symbol_paths = symbols
        .iter()
        .filter_map(|symbol| symbol.get("path").and_then(serde_json::Value::as_str))
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    let fresh_providers = providers
        .iter()
        .filter(|stored| {
            graph_provider_store::freshness(workspace, &stored.import)
                != graph_provider_store::GraphProviderFreshness::Stale
        })
        .collect::<Vec<_>>();
    let mut nodes_by_id = BTreeMap::<String, GraphContextNode>::new();
    let mut ranked = Vec::<(usize, String)>::new();

    for stored in &fresh_providers {
        let provenance = stored.import.provenance();
        for node in &stored.import.nodes {
            let path = node
                .attributes
                .get("path")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let mut haystack = node.label.to_ascii_lowercase();
            for key in ["name", "qualified_name", "path"] {
                if let Some(value) = node.attributes.get(key).and_then(serde_json::Value::as_str) {
                    haystack.push(' ');
                    haystack.push_str(&value.to_ascii_lowercase());
                }
            }
            let exact = usize::from(!needle.is_empty() && haystack.contains(&needle));
            let token_hits = tokens
                .iter()
                .filter(|token| haystack.contains(token.as_str()))
                .count();
            let path_hit = usize::from(
                path.as_ref()
                    .is_some_and(|path| symbol_paths.contains(path)),
            );
            let score = exact
                .saturating_mul(100)
                .saturating_add(token_hits.saturating_mul(10))
                .saturating_add(path_hit.saturating_mul(40));
            let id = node.id.clone();
            nodes_by_id.entry(id.clone()).or_insert(GraphContextNode {
                id: id.clone(),
                kind: node.kind,
                label: node.label.clone(),
                path,
                provider: provenance.provider.clone(),
                precision: provenance.precision,
                score,
            });
            if score > 0 {
                ranked.push((score, id));
            }
        }
    }

    ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    ranked.dedup_by(|left, right| left.1 == right.1);
    let candidate_count = ranked.len();
    let seeds = ranked
        .into_iter()
        .take(limit)
        .map(|(_, id)| id)
        .collect::<BTreeSet<_>>();
    if seeds.is_empty() {
        return Ok(GraphContext::default());
    }
    let mut selected = seeds.clone();
    let edge_limit = limit.saturating_mul(4).min(256);
    let mut edges = Vec::new();
    let mut edge_matches = 0usize;
    for stored in fresh_providers {
        let provenance = stored.import.provenance();
        for edge in &stored.import.edges {
            if !seeds.contains(&edge.from) && !seeds.contains(&edge.to) {
                continue;
            }
            if !nodes_by_id.contains_key(&edge.from) || !nodes_by_id.contains_key(&edge.to) {
                continue;
            }
            edge_matches = edge_matches.saturating_add(1);
            if selected.len() < limit {
                selected.insert(edge.from.clone());
                if selected.len() < limit {
                    selected.insert(edge.to.clone());
                }
            }
            if selected.contains(&edge.from)
                && selected.contains(&edge.to)
                && edges.len() < edge_limit
            {
                edges.push(GraphContextEdge {
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                    kind: edge.kind,
                    provider: provenance.provider.clone(),
                    precision: provenance.precision,
                });
            }
        }
    }
    let mut nodes = selected
        .into_iter()
        .filter_map(|id| nodes_by_id.remove(&id))
        .collect::<Vec<_>>();
    nodes.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.label.cmp(&right.label))
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(GraphContext {
        truncated: candidate_count > nodes.len() || edge_matches > edges.len(),
        nodes,
        edges,
    })
}

fn context_tokens(query: &str) -> Vec<String> {
    let mut tokens = query
        .split(|character: char| !character.is_alphanumeric())
        .map(str::trim)
        .filter(|token| token.chars().count() >= 2)
        .map(str::to_ascii_lowercase)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    tokens.sort_by_key(|token| std::cmp::Reverse(token.len()));
    tokens
}

fn ranked_context_ids(
    items: impl IntoIterator<Item = (String, String)>,
    query: &str,
    tokens: &[String],
    limit: usize,
) -> Vec<String> {
    let needle = query.to_ascii_lowercase();
    let mut ranked = items
        .into_iter()
        .filter_map(|(id, text)| {
            let haystack = text.to_ascii_lowercase();
            let exact = usize::from(haystack.contains(&needle));
            let token_hits = tokens
                .iter()
                .filter(|token| haystack.contains(token.as_str()))
                .count();
            let id_match = usize::from(
                tokens
                    .iter()
                    .any(|token| id.to_ascii_lowercase().contains(token)),
            );
            let score = exact
                .saturating_mul(100)
                .saturating_add(token_hits.saturating_mul(10))
                .saturating_add(id_match.saturating_mul(5));
            (score > 0).then_some((score, id))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    ranked.into_iter().take(limit).map(|(_, id)| id).collect()
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
