use super::*;
use crate::evidence::EvidenceResult;
use crate::intelligence::{
    build_project_observatory, ObservatoryInput, ProjectObservatory, ProjectProofSummary,
};

impl ToolHarness {
    pub fn project_observatory(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        review: Option<&ChangeReviewReport>,
    ) -> Result<ProjectObservatory> {
        const MAX_OBSERVATORY_FILES: usize = 1_500;
        const MAX_OBSERVATORY_SYMBOLS: usize = 5_000;
        const MAX_OBSERVATORY_HISTORY: usize = 32;

        let workspace_id = workspace_id.into();
        let design = design::load_design(workspace)?;
        let traceability = self.traceability_status(workspace_id.clone(), workspace)?;
        let graph = self.software_graph(
            workspace_id.clone(),
            workspace,
            ".",
            MAX_OBSERVATORY_FILES,
            MAX_OBSERVATORY_SYMBOLS,
        )?;
        let impact = review
            .map(|review| self.impact_analysis(workspace_id.clone(), workspace, review))
            .transpose()?;
        let risk = review
            .map(|review| self.risk_status(workspace_id.clone(), workspace, review))
            .transpose()?;
        let language_quality = self.language_quality_status(workspace)?;
        let revision = self.intelligence.current_revision(workspace)?;
        let evidence = self.evidence_status(&workspace_id, workspace, None, 500)?;
        let current_evidence = evidence
            .evidence
            .iter()
            .filter(|item| item.revision == revision)
            .collect::<Vec<_>>();
        let current_subject = format!("change:{}", revision.code);
        let verification = self.verification_history(&workspace_id, workspace, 100)?;
        let current_verification = verification
            .iter()
            .filter(|status| status.plan.subject == current_subject)
            .collect::<Vec<_>>();
        let proof = ProjectProofSummary {
            revision_code: revision.code.clone(),
            revision_design: revision.design.clone(),
            current_evidence: current_evidence.len(),
            current_passed: current_evidence
                .iter()
                .filter(|item| item.result == EvidenceResult::Pass)
                .count(),
            current_failed: current_evidence
                .iter()
                .filter(|item| item.result == EvidenceResult::Fail)
                .count(),
            current_inconclusive: current_evidence
                .iter()
                .filter(|item| item.result == EvidenceResult::Inconclusive)
                .count(),
            current_disagreed: current_evidence
                .iter()
                .filter(|item| item.result == EvidenceResult::Disagree)
                .count(),
            current_verification_plans: current_verification.len(),
            current_verification_ready: current_verification
                .iter()
                .filter(|status| status.ready)
                .count(),
            current_verification_blocked: current_verification
                .iter()
                .filter(|status| !status.ready)
                .count(),
            latest_current_evidence_at_ms: current_evidence
                .iter()
                .map(|item| item.timestamp_ms)
                .max(),
            evidence_scan_truncated: evidence.truncated,
        };
        let reconciliation = self.reconciliation_history(workspace, 100)?;
        let latest_reconciliation_plan = reconciliation.first().map(|plan| plan.id.clone());
        let history = self.graph_history(workspace, MAX_OBSERVATORY_HISTORY)?;
        let graph_diff = if history.len() >= 2 {
            self.graph_diff(
                workspace,
                &GraphDiffInput {
                    from_snapshot_id: None,
                    to_snapshot_id: None,
                    limit: 200,
                },
            )
            .ok()
        } else {
            None
        };

        Ok(build_project_observatory(ObservatoryInput {
            workspace: workspace_id,
            root: workspace.root().display().to_string(),
            design,
            traceability,
            graph: &graph,
            review,
            impact,
            risk,
            history: &history,
            graph_diff: graph_diff.as_ref(),
            language_quality,
            proof,
            reconciliation_plans: reconciliation.len(),
            latest_reconciliation_plan,
        }))
    }
}
