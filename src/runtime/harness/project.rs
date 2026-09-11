use super::*;
use crate::evidence::{Evidence, EvidenceKind, EvidenceResult};
use crate::intelligence::{
    build_project_observatory, ObservatoryInput, ProjectAcceptanceProofSummary, ProjectObservatory,
    ProjectProofSummary, TraceabilityStatus,
};
use std::collections::{BTreeMap, BTreeSet};

fn acceptance_proof_summary(
    design: &design::DesignLoad,
    traceability: &TraceabilityStatus,
    evidence: &[Evidence],
    revision: &crate::evidence::Revision,
) -> ProjectAcceptanceProofSummary {
    let acceptance_ids = design
        .state
        .acceptance
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut references = BTreeMap::<&str, (usize, usize)>::new();
    for reference in traceability
        .requirements
        .iter()
        .flat_map(|requirement| requirement.verification.iter())
    {
        if !acceptance_ids.contains(reference.owner.as_str()) {
            continue;
        }
        let counts = references.entry(reference.owner.as_str()).or_insert((0, 0));
        counts.0 += 1;
        counts.1 += usize::from(reference.resolved);
    }
    let mapped = acceptance_ids
        .iter()
        .filter(|id| {
            references
                .get(id.as_str())
                .is_some_and(|(total, resolved)| *total > 0 && total == resolved)
        })
        .count();

    let mut groups = BTreeMap::<&str, Vec<&Evidence>>::new();
    for item in evidence {
        if !acceptance_ids.contains(item.subject.as_str())
            || matches!(
                item.kind,
                EvidenceKind::Reconciliation
                    | EvidenceKind::ModelReview
                    | EvidenceKind::HumanApproval
            )
        {
            continue;
        }
        groups.entry(item.subject.as_str()).or_default().push(item);
    }

    let outcomes = groups
        .values()
        .map(|items| {
            let newest = items
                .iter()
                .max_by_key(|item| item.timestamp_ms)
                .expect("nonempty evidence group");
            let ambiguous = items.iter().any(|item| {
                item.timestamp_ms == newest.timestamp_ms && item.revision != newest.revision
            });
            let effective =
                crate::evidence::latest_current(items.iter().copied(), &newest.revision);
            let passed = !ambiguous
                && !effective.is_empty()
                && effective
                    .iter()
                    .all(|item| item.result == EvidenceResult::Pass);
            let fresh = !ambiguous && newest.revision == *revision;
            (passed, fresh)
        })
        .collect::<Vec<_>>();
    ProjectAcceptanceProofSummary {
        total: acceptance_ids.len(),
        mapped,
        executed: outcomes.len(),
        passed: outcomes.iter().filter(|(passed, _)| *passed).count(),
        fresh: outcomes.iter().filter(|(_, fresh)| *fresh).count(),
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/runtime/harness/proof.rs"]
mod tests;

impl ToolHarness {
    pub(crate) fn observatory_proof_signal(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
    ) -> Result<String> {
        self.intelligence
            .evidence_change_signal(workspace_id, workspace)
    }

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
        let evidence = self
            .intelligence
            .evidence_records(&workspace_id, workspace)?;
        let acceptance = acceptance_proof_summary(&design, &traceability, &evidence, &revision);
        let current_evidence = evidence
            .iter()
            .filter(|item| item.revision == revision)
            .collect::<Vec<_>>();
        let current_subject = format!("change:{}", revision.code);
        let verification = self.intelligence.verification_history_from_snapshot(
            &workspace_id,
            workspace,
            100,
            &revision,
            &evidence,
        )?;
        let current_verification = verification
            .iter()
            .filter(|status| {
                status.plan.subject == current_subject
                    && status.plan.revision.as_ref() == Some(&revision)
            })
            .collect::<Vec<_>>();
        let mut effective = crate::evidence::latest_current(&evidence, &revision);
        effective.sort_by_key(|item| {
            (
                std::cmp::Reverse(crate::evidence::result_severity(item.result)),
                std::cmp::Reverse(item.timestamp_ms),
            )
        });
        let sanitize = |text: &str| crate::workspace::redact_sensitive_text(text).0;
        let effective = crate::intelligence_types::ProjectEffectiveProofSummary {
            total: effective.len(),
            passed: effective
                .iter()
                .filter(|item| item.result == EvidenceResult::Pass)
                .count(),
            failed: effective
                .iter()
                .filter(|item| item.result == EvidenceResult::Fail)
                .count(),
            inconclusive: effective
                .iter()
                .filter(|item| item.result == EvidenceResult::Inconclusive)
                .count(),
            disagreed: effective
                .iter()
                .filter(|item| item.result == EvidenceResult::Disagree)
                .count(),
            items: effective
                .iter()
                .take(24)
                .map(|item| crate::intelligence_types::ProjectEvidenceView {
                    subject: sanitize(&item.subject),
                    producer: sanitize(&item.producer),
                    policy: item.policy.as_deref().map(sanitize),
                    kind: item.kind,
                    confidence: item.confidence,
                    result: item.result,
                    timestamp_ms: item.timestamp_ms,
                    summary: item
                        .summary
                        .as_deref()
                        .map(|text| sanitize(text).chars().take(500).collect()),
                })
                .collect(),
            truncated: effective.len() > 24,
        };
        let proof = ProjectProofSummary {
            acceptance,
            effective,
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
            evidence_scan_truncated: evidence.len() >= 4_096,
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
