use super::*;
use crate::evidence::{Evidence, EvidenceKind, EvidenceResult};
use crate::intelligence::{
    build_project_observatory, ObservatoryInput, ProjectAcceptanceProofSummary,
    ProjectAdaptiveVerificationView, ProjectCostEvaluationView, ProjectCostFrontierEntryView,
    ProjectCostSentinelView, ProjectEngineeringJournalView, ProjectEngineeringMilestoneView,
    ProjectFocusedVerificationView, ProjectObservatory, ProjectProofSummary,
    ProjectVerificationImpactReasonView, ProjectVerificationImpactView,
    ProjectVerifiedLearningView, TraceabilityStatus,
};
use std::collections::{BTreeMap, BTreeSet};

#[path = "project/observatory.rs"]
mod observatory;

fn observatory_verification_impact(
    impact: ProjectVerificationImpact,
) -> ProjectVerificationImpactView {
    ProjectVerificationImpactView {
        selective: impact.selective,
        affected_islands: impact.affected_islands,
        reasons: impact
            .reasons
            .into_iter()
            .map(|reason| ProjectVerificationImpactReasonView {
                island: reason.island,
                kind: reason.kind.to_owned(),
                source: reason.source,
                relationship: reason.relationship,
                evidence: reason.evidence,
                provider: reason.provider.to_owned(),
                precision: reason.precision.to_owned(),
            })
            .collect(),
        truncated: impact.truncated,
        provider: impact.provider.to_owned(),
        precision: impact.precision.to_owned(),
    }
}

fn static_adaptive_verification(reason: &str) -> ProjectAdaptiveVerificationView {
    ProjectAdaptiveVerificationView {
        mode: "static".to_owned(),
        provider: "verification-planner".to_owned(),
        precision: "deterministic".to_owned(),
        base_quick_checks: 0,
        planned_quick_checks: 0,
        full_coverage_unchanged: true,
        focused_test: None,
        cost_sentinel: None,
        cost_evaluation: None,
        fallback_reason: Some(reason.to_owned()),
    }
}

fn observatory_adaptive_verification(
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &Workspace,
    profile: &ProjectProfile,
    snapshot: &Value,
    impact: &ProjectVerificationImpact,
    design: &design::DesignLoad,
) -> ProjectAdaptiveVerificationView {
    let base_plan = harness_profile::verification_checks_for_impact(profile, impact, "quick");
    let gaps = harness_profile::verification_gaps_for_impact(profile, impact, "quick");
    if !gaps.is_empty() {
        let mut view = static_adaptive_verification("quick_verification_gap");
        view.base_quick_checks = base_plan.len();
        view.planned_quick_checks = base_plan.len();
        return view;
    }
    if base_plan.len() > MAX_VERIFICATION_CHECKS {
        let mut view = static_adaptive_verification("quick_plan_exceeds_bound");
        view.base_quick_checks = base_plan.len();
        view.planned_quick_checks = base_plan.len();
        return view;
    }

    let mut plan = base_plan.clone();
    let focused = if plan.len() < MAX_VERIFICATION_CHECKS {
        harness_test_focus::focused_quick_test_from_design(
            harness,
            workspace_id,
            workspace,
            profile,
            Some(snapshot),
            &design.state,
        )
        .filter(|focused| {
            !plan.iter().any(|check| {
                check.cwd == focused.cwd
                    && check.program == focused.program
                    && check.args == focused.args
            })
        })
    } else {
        None
    };
    if let Some(focused) = focused.as_ref() {
        plan.push(focused.clone());
        sort_checks(&mut plan);
    }
    let (cost, cost_evaluation) = harness_cost::observatory_cost_analysis(workspace, &plan);
    let (cost_activation_state, cost_activation_reason) =
        harness_cost::cost_backtest_activation(&cost_evaluation);
    let mode = match (focused.is_some(), cost.is_some()) {
        (true, true) => "combined",
        (true, false) => "focused_test",
        (false, true) => "cost_sentinel",
        (false, false) => "static",
    };
    let precision = match (focused.is_some(), cost.is_some()) {
        (true, true) => "mixed",
        (true, false) => harness_test_focus::FOCUSED_TEST_PRECISION,
        (false, true) => "heuristic",
        (false, false) => "deterministic",
    };
    ProjectAdaptiveVerificationView {
        mode: mode.to_owned(),
        provider: "verification-planner".to_owned(),
        precision: precision.to_owned(),
        base_quick_checks: base_plan.len(),
        planned_quick_checks: plan.len(),
        full_coverage_unchanged: true,
        focused_test: focused.map(|check| ProjectFocusedVerificationView {
            check_id: check.id.clone(),
            command: verification_command_text(&check),
            island: check.island.clone(),
            phase: check.phase,
            reason: check.reason.clone(),
            provider: harness_test_focus::FOCUSED_TEST_PROVIDER.to_owned(),
            precision: harness_test_focus::FOCUSED_TEST_PRECISION.to_owned(),
        }),
        cost_sentinel: cost.map(|decision| ProjectCostSentinelView {
            model: decision.model.to_owned(),
            provider: decision.provider.to_owned(),
            precision: decision.precision.to_owned(),
            check_id: decision.sentinel_check,
            command: decision.sentinel_command,
            island: decision.sentinel_island,
            samples: decision.samples,
            failures: decision.failures,
            failure_rate_percent: decision.failure_rate_percent,
            median_elapsed_ms: decision.median_elapsed_ms,
            estimated_savings_ms: decision.estimated_savings_ms,
            estimated_total_savings_ms: decision.estimated_total_savings_ms,
            evidence_records_scanned: decision.evidence_records_scanned,
            frontier: decision
                .frontier
                .into_iter()
                .map(|entry| ProjectCostFrontierEntryView {
                    order: entry.order,
                    check_id: entry.check_id,
                    command: entry.command,
                    island: entry.island,
                    samples: entry.samples,
                    failures: entry.failures,
                    failure_rate_percent: entry.failure_rate_percent,
                    median_elapsed_ms: entry.median_elapsed_ms,
                    marginal_samples: entry.marginal_samples,
                    marginal_failures: entry.marginal_failures,
                    marginal_failure_rate_percent: entry.marginal_failure_rate_percent,
                    estimated_incremental_savings_ms: entry.estimated_incremental_savings_ms,
                })
                .collect(),
        }),
        cost_evaluation: Some(ProjectCostEvaluationView {
            available: cost_evaluation.available,
            candidate_model: harness_cost::COST_MODEL.to_owned(),
            baseline_model: harness_cost::COST_BASELINE_MODEL.to_owned(),
            evaluation_method: harness_cost::COST_EVALUATION_METHOD.to_owned(),
            activation_state: cost_activation_state.to_owned(),
            activation_reason: cost_activation_reason.to_owned(),
            minimum_evaluable_revisions: harness_cost::MIN_BACKTEST_EVALUABLE_REVISIONS,
            revisions: cost_evaluation.revisions,
            eligible_revisions: cost_evaluation.eligible_revisions,
            evaluable_revisions: cost_evaluation.evaluable_revisions,
            incomplete_revisions: cost_evaluation.incomplete_revisions,
            wins: cost_evaluation.wins,
            ties: cost_evaluation.ties,
            losses: cost_evaluation.losses,
            outcome_mismatches: cost_evaluation.outcome_mismatches,
            static_elapsed_ms: cost_evaluation.static_elapsed_ms,
            frontier_elapsed_ms: cost_evaluation.frontier_elapsed_ms,
            gross_savings_ms: cost_evaluation.gross_savings_ms,
            regret_ms: cost_evaluation.regret_ms,
            net_savings_ms: cost_evaluation.net_savings_ms(),
            net_savings_percent: if cost_evaluation.static_elapsed_ms == 0 {
                0.0
            } else {
                cost_evaluation.net_savings_ms() as f64 * 100.0
                    / cost_evaluation.static_elapsed_ms as f64
            },
            latest_revision_at_ms: cost_evaluation.latest_revision_at_ms,
        }),
        fallback_reason: (mode == "static").then(|| match cost_activation_reason {
            "outcome_mismatch" => "cost_backtest_outcome_mismatch".to_owned(),
            "non_positive_net_savings" => "cost_backtest_non_positive_net_savings".to_owned(),
            _ => "no_strong_adaptive_evidence".to_owned(),
        }),
    }
}

fn observatory_engineering_journal(workspace: &Workspace) -> ProjectEngineeringJournalView {
    let Ok(history) = crate::engineering_journal::load_recent(workspace, 64) else {
        return ProjectEngineeringJournalView {
            available: false,
            provider: "engineering-milestone-journal".to_owned(),
            stores_prompts_or_chain_of_thought: false,
            retained_records: 0,
            truncated: false,
            records: Vec::new(),
        };
    };
    ProjectEngineeringJournalView {
        available: true,
        provider: "engineering-milestone-journal".to_owned(),
        stores_prompts_or_chain_of_thought: false,
        retained_records: history.retained_records,
        truncated: history.truncated,
        records: history
            .records
            .into_iter()
            .map(|record| ProjectEngineeringMilestoneView {
                timestamp_ms: record.timestamp_ms,
                tool: record.tool,
                stage: record.stage,
                outcome: record.outcome,
                duration_ms: record.duration_ms,
                paths: record.paths,
                verification_level: record.verification_level,
                checks_run: record.checks_run,
                checks_failed: record.checks_failed,
            })
            .collect(),
    }
}

fn learning_percent(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 * 100.0 / denominator as f64
    }
}

fn observatory_verified_learning(workspace: &Workspace) -> ProjectVerifiedLearningView {
    let retrieval_model = crate::experience_store::retrieval_model().to_owned();
    let baseline_model = crate::experience_store::baseline_model().to_owned();
    let evaluation_method = crate::experience_store::evaluation_method().to_owned();
    let Ok(evaluation) = crate::experience_store::evaluate_history(workspace) else {
        return ProjectVerifiedLearningView {
            available: false,
            provider: "verified-change-history".to_owned(),
            retrieval_precision: "heuristic".to_owned(),
            retrieval_model,
            baseline_model,
            evaluation_method,
            stores_prompts_or_chain_of_thought: false,
            top_k: 0,
            records: 0,
            full_records: 0,
            quick_records: 0,
            unique_paths: 0,
            live_paths: 0,
            stale_path_references: 0,
            evaluable_records: 0,
            records_with_prediction: 0,
            baseline_records_with_prediction: 0,
            evaluation_cases: 0,
            prediction_cases: 0,
            baseline_prediction_cases: 0,
            hit_cases: 0,
            baseline_hit_cases: 0,
            predictions: 0,
            baseline_predictions: 0,
            true_positives: 0,
            baseline_true_positives: 0,
            expected_targets: 0,
            coverage_percent: 0.0,
            baseline_coverage_percent: 0.0,
            coverage_delta_percent_points: 0.0,
            hit_rate_percent: 0.0,
            baseline_hit_rate_percent: 0.0,
            hit_rate_delta_percent_points: 0.0,
            precision_at_k_percent: 0.0,
            baseline_precision_at_k_percent: 0.0,
            precision_at_k_delta_percent_points: 0.0,
            recall_at_k_percent: 0.0,
            baseline_recall_at_k_percent: 0.0,
            recall_at_k_delta_percent_points: 0.0,
            latest_record_at_ms: None,
        };
    };
    let coverage_percent =
        learning_percent(evaluation.prediction_cases, evaluation.evaluation_cases);
    let baseline_coverage_percent = learning_percent(
        evaluation.baseline_prediction_cases,
        evaluation.evaluation_cases,
    );
    let hit_rate_percent = learning_percent(evaluation.hit_cases, evaluation.prediction_cases);
    let baseline_hit_rate_percent = learning_percent(
        evaluation.baseline_hit_cases,
        evaluation.baseline_prediction_cases,
    );
    let precision_at_k_percent =
        learning_percent(evaluation.true_positives, evaluation.predictions);
    let baseline_precision_at_k_percent = learning_percent(
        evaluation.baseline_true_positives,
        evaluation.baseline_predictions,
    );
    let recall_at_k_percent =
        learning_percent(evaluation.true_positives, evaluation.expected_targets);
    let baseline_recall_at_k_percent = learning_percent(
        evaluation.baseline_true_positives,
        evaluation.expected_targets,
    );
    ProjectVerifiedLearningView {
        available: evaluation.available,
        provider: "verified-change-history".to_owned(),
        retrieval_precision: "heuristic".to_owned(),
        retrieval_model,
        baseline_model,
        evaluation_method,
        stores_prompts_or_chain_of_thought: false,
        top_k: evaluation.top_k,
        records: evaluation.records,
        full_records: evaluation.full_records,
        quick_records: evaluation.quick_records,
        unique_paths: evaluation.unique_paths,
        live_paths: evaluation.live_paths,
        stale_path_references: evaluation.stale_path_references,
        evaluable_records: evaluation.evaluable_records,
        records_with_prediction: evaluation.records_with_prediction,
        baseline_records_with_prediction: evaluation.baseline_records_with_prediction,
        evaluation_cases: evaluation.evaluation_cases,
        prediction_cases: evaluation.prediction_cases,
        baseline_prediction_cases: evaluation.baseline_prediction_cases,
        hit_cases: evaluation.hit_cases,
        baseline_hit_cases: evaluation.baseline_hit_cases,
        predictions: evaluation.predictions,
        baseline_predictions: evaluation.baseline_predictions,
        true_positives: evaluation.true_positives,
        baseline_true_positives: evaluation.baseline_true_positives,
        expected_targets: evaluation.expected_targets,
        coverage_percent,
        baseline_coverage_percent,
        coverage_delta_percent_points: coverage_percent - baseline_coverage_percent,
        hit_rate_percent,
        baseline_hit_rate_percent,
        hit_rate_delta_percent_points: hit_rate_percent - baseline_hit_rate_percent,
        precision_at_k_percent,
        baseline_precision_at_k_percent,
        precision_at_k_delta_percent_points: precision_at_k_percent
            - baseline_precision_at_k_percent,
        recall_at_k_percent,
        baseline_recall_at_k_percent,
        recall_at_k_delta_percent_points: recall_at_k_percent - baseline_recall_at_k_percent,
        latest_record_at_ms: evaluation.latest_record_at_ms,
    }
}

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

    pub(crate) fn observatory_engineering_signal(&self, workspace: &Workspace) -> Result<String> {
        crate::engineering_journal::change_fingerprint(workspace)
    }
}
