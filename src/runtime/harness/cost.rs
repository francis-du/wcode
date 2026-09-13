use super::{
    sort_checks, verification_command_text, CheckSpec, VerificationCostDecision,
    VerificationCostFrontierEntry,
};
use crate::evidence::{Evidence, EvidenceResult};
use crate::workspace::Workspace;
use std::collections::BTreeMap;

pub(crate) const COST_MODEL: &str = "verification-cost-frontier-v3";
const COST_MODEL_PROVIDER: &str = "deterministic-evidence-history";
const COST_MODEL_PRECISION: &str = "heuristic";
pub(crate) const COST_EVALUATION_METHOD: &str = "global-temporal-revision-replay-v2";
pub(crate) const COST_BASELINE_MODEL: &str = "static-phased-fail-fast-v1";
const COST_METRICS_PREFIX: &str = "verification-metrics-v1";
const MAX_COST_EVIDENCE: usize = 512;
const MAX_FRONTIER_SENTINELS: usize = 2;
const MIN_SENTINEL_SAMPLES: usize = 4;
const MIN_SENTINEL_FAILURES: usize = 2;
const MIN_MARGINAL_SAMPLES: usize = 4;
const MIN_MARGINAL_FAILURES: usize = 2;
const MIN_COST_ESTIMATE_SAMPLES: usize = 2;
const MAX_SENTINEL_MEDIAN_MS: u128 = 10_000;
const MIN_EXPECTED_SAVINGS_MS: f64 = 100.0;
pub(crate) const MIN_BACKTEST_EVALUABLE_REVISIONS: usize = 4;

type RevisionKey = (String, Option<String>);

#[derive(Clone, Debug, Default)]
struct CheckHistory {
    observations: BTreeMap<RevisionKey, CheckObservation>,
}

impl CheckHistory {
    fn observe(
        &mut self,
        elapsed_ms: u128,
        result: EvidenceResult,
        revision: RevisionKey,
        timestamp_ms: u64,
    ) {
        let observation = CheckObservation {
            elapsed_ms,
            failed: result == EvidenceResult::Fail,
            timestamp_ms,
        };
        self.observations
            .entry(revision)
            .and_modify(|current| {
                if observation.timestamp_ms > current.timestamp_ms {
                    *current = observation;
                } else if observation.timestamp_ms == current.timestamp_ms {
                    current.failed |= observation.failed;
                    current.elapsed_ms = current.elapsed_ms.max(observation.elapsed_ms);
                }
            })
            .or_insert(observation);
    }

    fn samples(&self) -> usize {
        self.observations.len()
    }

    fn failures(&self) -> usize {
        self.observations
            .values()
            .filter(|observation| observation.failed)
            .count()
    }

    fn median_elapsed_ms(&self) -> u128 {
        let mut values = self
            .observations
            .values()
            .map(|observation| observation.elapsed_ms)
            .collect::<Vec<_>>();
        values.sort_unstable();
        values.get(values.len() / 2).copied().unwrap_or(0)
    }

    fn failure_probability(&self) -> f64 {
        if self.samples() == 0 {
            0.0
        } else {
            self.failures() as f64 / self.samples() as f64
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct VerificationCostEvaluation {
    pub(crate) available: bool,
    pub(crate) revisions: usize,
    pub(crate) eligible_revisions: usize,
    pub(crate) evaluable_revisions: usize,
    pub(crate) incomplete_revisions: usize,
    pub(crate) wins: usize,
    pub(crate) ties: usize,
    pub(crate) losses: usize,
    pub(crate) outcome_mismatches: usize,
    pub(crate) static_elapsed_ms: u128,
    pub(crate) frontier_elapsed_ms: u128,
    pub(crate) gross_savings_ms: u128,
    pub(crate) regret_ms: u128,
    pub(crate) latest_revision_at_ms: Option<u64>,
}

impl VerificationCostEvaluation {
    pub(crate) fn net_savings_ms(&self) -> i128 {
        self.gross_savings_ms as i128 - self.regret_ms as i128
    }
}

pub(crate) fn cost_backtest_activation(
    evaluation: &VerificationCostEvaluation,
) -> (&'static str, &'static str) {
    if evaluation.outcome_mismatches > 0 {
        return ("blocked", "outcome_mismatch");
    }
    if evaluation.evaluable_revisions < MIN_BACKTEST_EVALUABLE_REVISIONS {
        return ("exploring", "cold_start_exploration");
    }
    if evaluation.net_savings_ms() > 0 {
        ("active", "positive_net_savings")
    } else {
        ("blocked", "non_positive_net_savings")
    }
}

pub(crate) fn cost_backtest_allows_candidate(evaluation: &VerificationCostEvaluation) -> bool {
    cost_backtest_activation(evaluation).0 != "blocked"
}

#[derive(Clone, Copy, Debug, Default)]
struct MarginalHistory {
    samples: usize,
    failures: usize,
}

impl MarginalHistory {
    fn failure_probability(self) -> f64 {
        if self.samples == 0 {
            0.0
        } else {
            self.failures as f64 / self.samples as f64
        }
    }
}

pub(crate) fn apply_historical_cost_model(
    workspace: &Workspace,
    plan: &[CheckSpec],
    fail_fast: bool,
) -> (Vec<CheckSpec>, Option<VerificationCostDecision>) {
    if !fail_fast || plan.iter().filter(|check| check.phase == 0).count() < 2 {
        return (plan.to_vec(), None);
    }
    let evidence = match crate::evidence_store::load_recent(workspace, MAX_COST_EVIDENCE) {
        Ok(evidence) => evidence,
        Err(error) => {
            tracing::warn!(%error, "verification cost history could not be loaded; using static phases");
            return (plan.to_vec(), None);
        }
    };
    let (adapted, decision) = apply_cost_model_from_evidence(plan, &evidence);
    let Some(decision) = decision else {
        return (plan.to_vec(), None);
    };
    let evaluation = evaluate_cost_model_from_evidence(plan, &evidence);
    if !cost_backtest_allows_candidate(&evaluation) {
        return (plan.to_vec(), None);
    }
    (adapted, Some(decision))
}

pub(crate) fn observatory_cost_analysis(
    workspace: &Workspace,
    plan: &[CheckSpec],
) -> (Option<VerificationCostDecision>, VerificationCostEvaluation) {
    let evidence = match crate::evidence_store::load_recent(workspace, MAX_COST_EVIDENCE) {
        Ok(evidence) => evidence,
        Err(error) => {
            tracing::warn!(%error, "verification cost history could not be loaded for observatory evaluation");
            return (None, VerificationCostEvaluation::default());
        }
    };
    let (_, decision) = apply_cost_model_from_evidence(plan, &evidence);
    let evaluation = evaluate_cost_model_from_evidence(plan, &evidence);
    let decision = decision.filter(|_| cost_backtest_allows_candidate(&evaluation));
    (decision, evaluation)
}

pub(crate) fn apply_cost_model_from_evidence(
    plan: &[CheckSpec],
    evidence: &[Evidence],
) -> (Vec<CheckSpec>, Option<VerificationCostDecision>) {
    let history = check_history(plan, evidence);
    let Some((first_index, first_savings, first_stats)) = best_first_sentinel(plan, &history)
    else {
        return (plan.to_vec(), None);
    };

    let second = if MAX_FRONTIER_SENTINELS > 1 {
        best_second_sentinel(plan, &history, first_index, &first_stats)
    } else {
        None
    };
    let frontier_len = 1 + usize::from(second.is_some());
    let second_index = second.as_ref().map(|(index, _, _, _)| *index);
    let mut adapted = plan.to_vec();
    for (index, check) in adapted.iter_mut().enumerate() {
        if index == first_index {
            check.phase = 0;
        } else if Some(index) == second_index {
            check.phase = 1;
        } else {
            check.phase = check.phase.saturating_add(frontier_len as u8);
        }
    }
    sort_checks(&mut adapted);

    let first = &plan[first_index];
    let first_entry = frontier_entry(
        1,
        first,
        &first_stats,
        MarginalHistory {
            samples: first_stats.samples(),
            failures: first_stats.failures(),
        },
        first_savings,
    );
    let mut frontier = vec![first_entry];
    let mut total_savings = first_savings;
    if let Some((index, savings, stats, marginal)) = second {
        frontier.push(frontier_entry(2, &plan[index], &stats, marginal, savings));
        total_savings += savings;
    }

    let decision = VerificationCostDecision {
        model: COST_MODEL,
        provider: COST_MODEL_PROVIDER,
        precision: COST_MODEL_PRECISION,
        sentinel_check: first.id.clone(),
        sentinel_command: verification_command_text(first),
        sentinel_island: first.island.clone(),
        samples: first_stats.samples(),
        failures: first_stats.failures(),
        failure_rate_percent: first_stats.failure_probability() * 100.0,
        median_elapsed_ms: first_stats.median_elapsed_ms(),
        estimated_savings_ms: rounded_savings(first_savings),
        estimated_total_savings_ms: rounded_savings(total_savings),
        evidence_records_scanned: evidence.len(),
        frontier,
    };
    (adapted, Some(decision))
}

pub(crate) fn evaluate_cost_model_from_evidence(
    plan: &[CheckSpec],
    evidence: &[Evidence],
) -> VerificationCostEvaluation {
    let mut batches = revision_batches(plan, evidence);
    batches.sort_by(|left, right| {
        left.timestamp_ms
            .cmp(&right.timestamp_ms)
            .then_with(|| left.revision.cmp(&right.revision))
    });
    let mut evaluation = VerificationCostEvaluation {
        available: true,
        revisions: batches.len(),
        latest_revision_at_ms: batches.last().map(|batch| batch.timestamp_ms),
        ..VerificationCostEvaluation::default()
    };
    let mut training = Vec::<Evidence>::new();
    let mut index = 0usize;
    while index < batches.len() {
        let timestamp_ms = batches[index].timestamp_ms;
        let mut end = index + 1;
        while end < batches.len() && batches[end].timestamp_ms == timestamp_ms {
            end += 1;
        }
        for batch in &batches[index..end] {
            let (adapted, decision) = apply_cost_model_from_evidence(plan, &training);
            if decision.is_none() {
                continue;
            }
            evaluation.eligible_revisions = evaluation.eligible_revisions.saturating_add(1);
            let static_run = simulate_fail_fast(plan, &batch.observations);
            let frontier_run = simulate_fail_fast(&adapted, &batch.observations);
            let (Some(static_run), Some(frontier_run)) = (static_run, frontier_run) else {
                evaluation.incomplete_revisions = evaluation.incomplete_revisions.saturating_add(1);
                continue;
            };
            if static_run.failed != frontier_run.failed {
                evaluation.outcome_mismatches = evaluation.outcome_mismatches.saturating_add(1);
                continue;
            }
            evaluation.evaluable_revisions = evaluation.evaluable_revisions.saturating_add(1);
            evaluation.static_elapsed_ms = evaluation
                .static_elapsed_ms
                .saturating_add(static_run.elapsed_ms);
            evaluation.frontier_elapsed_ms = evaluation
                .frontier_elapsed_ms
                .saturating_add(frontier_run.elapsed_ms);
            if frontier_run.elapsed_ms < static_run.elapsed_ms {
                evaluation.wins = evaluation.wins.saturating_add(1);
                evaluation.gross_savings_ms = evaluation
                    .gross_savings_ms
                    .saturating_add(static_run.elapsed_ms - frontier_run.elapsed_ms);
            } else if frontier_run.elapsed_ms > static_run.elapsed_ms {
                evaluation.losses = evaluation.losses.saturating_add(1);
                evaluation.regret_ms = evaluation
                    .regret_ms
                    .saturating_add(frontier_run.elapsed_ms - static_run.elapsed_ms);
            } else {
                evaluation.ties = evaluation.ties.saturating_add(1);
            }
        }
        for batch in &batches[index..end] {
            training.extend(batch.evidence.iter().cloned());
        }
        index = end;
    }
    evaluation
}

#[derive(Clone, Debug)]
struct RevisionBatch {
    revision: RevisionKey,
    timestamp_ms: u64,
    observations: BTreeMap<(String, String), CheckObservation>,
    evidence: Vec<Evidence>,
}

#[derive(Clone, Copy, Debug)]
struct CheckObservation {
    elapsed_ms: u128,
    failed: bool,
    timestamp_ms: u64,
}

#[derive(Clone, Copy, Debug)]
struct SimulatedRun {
    elapsed_ms: u128,
    failed: bool,
}

fn revision_batches(plan: &[CheckSpec], evidence: &[Evidence]) -> Vec<RevisionBatch> {
    let allowed = plan
        .iter()
        .map(check_key)
        .collect::<std::collections::BTreeSet<_>>();
    let mut grouped = BTreeMap::<RevisionKey, RevisionBatch>::new();
    for record in evidence {
        if !matches!(record.result, EvidenceResult::Pass | EvidenceResult::Fail) {
            continue;
        }
        let Some(id) = record.subject.strip_prefix("verification:") else {
            continue;
        };
        let key = (id.to_owned(), record.producer.clone());
        if !allowed.contains(&key) {
            continue;
        }
        let Some(elapsed_ms) = record.summary.as_deref().and_then(parse_elapsed_ms) else {
            continue;
        };
        let revision = (record.revision.code.clone(), record.revision.design.clone());
        let batch = grouped
            .entry(revision.clone())
            .or_insert_with(|| RevisionBatch {
                revision,
                timestamp_ms: record.timestamp_ms,
                observations: BTreeMap::new(),
                evidence: Vec::new(),
            });
        batch.timestamp_ms = batch.timestamp_ms.max(record.timestamp_ms);
        let observation = CheckObservation {
            elapsed_ms,
            failed: record.result == EvidenceResult::Fail,
            timestamp_ms: record.timestamp_ms,
        };
        batch
            .observations
            .entry(key)
            .and_modify(|current| {
                if observation.timestamp_ms > current.timestamp_ms {
                    *current = observation;
                } else if observation.timestamp_ms == current.timestamp_ms {
                    current.failed |= observation.failed;
                    current.elapsed_ms = current.elapsed_ms.max(observation.elapsed_ms);
                }
            })
            .or_insert(observation);
        batch.evidence.push(record.clone());
    }
    grouped.into_values().collect()
}

fn simulate_fail_fast(
    plan: &[CheckSpec],
    observations: &BTreeMap<(String, String), CheckObservation>,
) -> Option<SimulatedRun> {
    let max_phase = plan.iter().map(|check| check.phase).max().unwrap_or(0);
    let mut elapsed_ms = 0u128;
    for phase in 0..=max_phase {
        let checks = plan
            .iter()
            .filter(|check| check.phase == phase)
            .collect::<Vec<_>>();
        if checks.is_empty() {
            continue;
        }
        let mut phase_elapsed = 0u128;
        let mut phase_failed = false;
        for check in checks {
            let observation = observations.get(&check_key(check))?;
            phase_elapsed = phase_elapsed.max(observation.elapsed_ms);
            phase_failed |= observation.failed;
        }
        elapsed_ms = elapsed_ms.saturating_add(phase_elapsed);
        if phase_failed {
            return Some(SimulatedRun {
                elapsed_ms,
                failed: true,
            });
        }
    }
    Some(SimulatedRun {
        elapsed_ms,
        failed: false,
    })
}

fn best_first_sentinel(
    plan: &[CheckSpec],
    history: &BTreeMap<(String, String), CheckHistory>,
) -> Option<(usize, f64, CheckHistory)> {
    let baseline_cost = estimated_static_cost(plan, history);
    let mut best: Option<(usize, f64, CheckHistory)> = None;
    for (index, check) in plan.iter().enumerate() {
        if check.phase != 0 {
            continue;
        }
        let Some(stats) = eligible_history(history.get(&check_key(check))) else {
            continue;
        };
        let remaining_cost = estimated_remaining_cost_after(plan, history, &[index]);
        if remaining_cost == 0 {
            continue;
        }
        let adaptive_cost = stats.median_elapsed_ms() as f64
            + (1.0 - stats.failure_probability()) * remaining_cost as f64;
        let savings = baseline_cost as f64 - adaptive_cost;
        if savings < MIN_EXPECTED_SAVINGS_MS {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(_, best_savings, _)| savings > *best_savings)
        {
            best = Some((index, savings, stats.clone()));
        }
    }
    best
}

fn best_second_sentinel(
    plan: &[CheckSpec],
    history: &BTreeMap<(String, String), CheckHistory>,
    first_index: usize,
    first_stats: &CheckHistory,
) -> Option<(usize, f64, CheckHistory, MarginalHistory)> {
    let remaining_after_first = estimated_remaining_cost_after(plan, history, &[first_index]);
    if remaining_after_first == 0 {
        return None;
    }
    let first_survival_probability = 1.0 - first_stats.failure_probability();
    if first_survival_probability <= 0.0 {
        return None;
    }

    let mut best: Option<(usize, f64, CheckHistory, MarginalHistory)> = None;
    for (index, check) in plan.iter().enumerate() {
        if index == first_index || check.phase != 0 {
            continue;
        }
        let Some(stats) = eligible_history(history.get(&check_key(check))) else {
            continue;
        };
        let marginal = marginal_history(first_stats, stats);
        if marginal.samples < MIN_MARGINAL_SAMPLES || marginal.failures < MIN_MARGINAL_FAILURES {
            continue;
        }
        let remaining_after_second =
            estimated_remaining_cost_after(plan, history, &[first_index, index]);
        let second_stage_cost = stats.median_elapsed_ms() as f64
            + (1.0 - marginal.failure_probability()) * remaining_after_second as f64;
        let conditional_savings = remaining_after_first as f64 - second_stage_cost;
        let savings = first_survival_probability * conditional_savings;
        if savings < MIN_EXPECTED_SAVINGS_MS {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(_, best_savings, _, _)| savings > *best_savings)
        {
            best = Some((index, savings, stats.clone(), marginal));
        }
    }
    best
}

fn eligible_history(stats: Option<&CheckHistory>) -> Option<&CheckHistory> {
    let stats = stats?;
    if stats.samples() < MIN_SENTINEL_SAMPLES || stats.failures() < MIN_SENTINEL_FAILURES {
        return None;
    }
    let median = stats.median_elapsed_ms();
    (median > 0 && median <= MAX_SENTINEL_MEDIAN_MS).then_some(stats)
}

fn marginal_history(first: &CheckHistory, second: &CheckHistory) -> MarginalHistory {
    let mut marginal = MarginalHistory::default();
    for (revision, second_observation) in &second.observations {
        let Some(first_observation) = first.observations.get(revision) else {
            continue;
        };
        if first_observation.failed {
            continue;
        }
        marginal.samples = marginal.samples.saturating_add(1);
        marginal.failures = marginal
            .failures
            .saturating_add(usize::from(second_observation.failed));
    }
    marginal
}

fn frontier_entry(
    order: usize,
    check: &CheckSpec,
    stats: &CheckHistory,
    marginal: MarginalHistory,
    savings: f64,
) -> VerificationCostFrontierEntry {
    VerificationCostFrontierEntry {
        order,
        check_id: check.id.clone(),
        command: verification_command_text(check),
        island: check.island.clone(),
        samples: stats.samples(),
        failures: stats.failures(),
        failure_rate_percent: stats.failure_probability() * 100.0,
        median_elapsed_ms: stats.median_elapsed_ms(),
        marginal_samples: marginal.samples,
        marginal_failures: marginal.failures,
        marginal_failure_rate_percent: marginal.failure_probability() * 100.0,
        estimated_incremental_savings_ms: rounded_savings(savings),
    }
}

fn rounded_savings(value: f64) -> u128 {
    value.round().max(0.0) as u128
}

fn check_history(
    plan: &[CheckSpec],
    evidence: &[Evidence],
) -> BTreeMap<(String, String), CheckHistory> {
    let mut history = BTreeMap::new();
    for check in plan {
        history
            .entry(check_key(check))
            .or_insert_with(CheckHistory::default);
    }
    for record in evidence {
        if !matches!(record.result, EvidenceResult::Pass | EvidenceResult::Fail) {
            continue;
        }
        let Some(id) = record.subject.strip_prefix("verification:") else {
            continue;
        };
        let key = (id.to_owned(), record.producer.clone());
        let Some(stats) = history.get_mut(&key) else {
            continue;
        };
        let Some(elapsed_ms) = record.summary.as_deref().and_then(parse_elapsed_ms) else {
            continue;
        };
        stats.observe(
            elapsed_ms,
            record.result,
            (record.revision.code.clone(), record.revision.design.clone()),
            record.timestamp_ms,
        );
    }
    history
}

fn check_key(check: &CheckSpec) -> (String, String) {
    (check.id.clone(), verification_command_text(check))
}

fn estimated_static_cost(
    plan: &[CheckSpec],
    history: &BTreeMap<(String, String), CheckHistory>,
) -> u128 {
    phase_wall_cost(plan, history, 0, &[]).saturating_add(later_phase_cost(plan, history))
}

fn estimated_remaining_cost_after(
    plan: &[CheckSpec],
    history: &BTreeMap<(String, String), CheckHistory>,
    excluded_indices: &[usize],
) -> u128 {
    phase_wall_cost(plan, history, 0, excluded_indices)
        .saturating_add(later_phase_cost(plan, history))
}

fn later_phase_cost(
    plan: &[CheckSpec],
    history: &BTreeMap<(String, String), CheckHistory>,
) -> u128 {
    let max_phase = plan.iter().map(|check| check.phase).max().unwrap_or(0);
    (1..=max_phase)
        .map(|phase| phase_wall_cost(plan, history, phase, &[]))
        .fold(0u128, u128::saturating_add)
}

fn phase_wall_cost(
    plan: &[CheckSpec],
    history: &BTreeMap<(String, String), CheckHistory>,
    phase: u8,
    excluded_indices: &[usize],
) -> u128 {
    plan.iter()
        .enumerate()
        .filter(|(index, check)| check.phase == phase && !excluded_indices.contains(index))
        .filter_map(|(_, check)| history.get(&check_key(check)))
        .filter(|stats| stats.samples() >= MIN_COST_ESTIMATE_SAMPLES)
        .map(CheckHistory::median_elapsed_ms)
        .max()
        .unwrap_or(0)
}

fn parse_elapsed_ms(summary: &str) -> Option<u128> {
    let payload = summary
        .strip_prefix(COST_METRICS_PREFIX)?
        .strip_prefix(';')?;
    payload.split(';').find_map(|part| {
        let value = part.strip_prefix("elapsed_ms=")?;
        value.parse::<u128>().ok()
    })
}

pub(crate) fn metrics_summary(elapsed_ms: u128, phase: u8) -> String {
    format!("{COST_METRICS_PREFIX};elapsed_ms={elapsed_ms};phase={phase}")
}
