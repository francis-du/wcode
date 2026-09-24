//! Descriptive Fitness views of retained milestones, never verification authority.
//! A post-operation revision identifies the observed repository state, not the
//! compilation input or a proof that the revision was stable during the tool call.
use crate::evidence::Revision;
use crate::intelligence_types::{
    ProjectEngineeringJournalView, ProjectEngineeringMilestoneView, ProjectFitnessMetricView,
    ProjectFitnessRevisionView, ProjectFitnessTrendView, ProjectFitnessView, ProjectProofSummary,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_SAMPLE: usize = 64;
const MAX_HISTORY: usize = 16;
const MIN_TREND_HALF: usize = 3;

type Milestone = ProjectEngineeringMilestoneView;
type RevisionKey = (String, Option<String>);

pub(super) fn build(
    journal: &ProjectEngineeringJournalView,
    proof: &ProjectProofSummary,
) -> ProjectFitnessView {
    let revision = Revision {
        code: proof.revision_code.clone(),
        design: proof.revision_design.clone(),
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0);
    summarize(journal, revision, now)
}

fn summarize(
    journal: &ProjectEngineeringJournalView,
    revision: Revision,
    observed_at_ms: u64,
) -> ProjectFitnessView {
    let mut view = ProjectFitnessView {
        schema_version: 1,
        available: journal.available,
        provider: "engineering-milestone-journal".into(),
        scope: "last-64-retained-milestones-not-all-tool-calls".into(),
        revision_binding: "post-operation-observation-not-verification".into(),
        revision,
        observed_at_ms,
        retained_records: journal.retained_records,
        sampled_records: 0,
        unbound_records: 0,
        stale_records: 0,
        duplicate_records: 0,
        conflicting_events: 0,
        invalid_records: 0,
        // A complete latest-64 sample is deliberately bounded, not a failed read.
        // Only missing entries inside that selected window make coverage partial.
        partial: journal.records.len() != journal.retained_records.min(MAX_SAMPLE)
            || (journal.truncated && journal.retained_records <= MAX_SAMPLE),
        window_limited: journal.retained_records > MAX_SAMPLE,
        history_truncated: false,
        window_start_ms: None,
        window_end_ms: None,
        current: Vec::new(),
        benchmark: crate::intelligence::FitnessBenchmarkView::default(),
        history: Vec::new(),
        not_measured: vec![
            "benchmark-recall-precision-ndcg-require-explicit-fitness-report".into(),
            "model-task-success-and-patch-correctness".into(),
            "flaky-retry-recovery-and-writer-conflict-rates".into(),
            "live-lsp-hit-rate-and-runtime-launch-readiness".into(),
            "cold-warm-runtime-latency".into(),
        ],
    };
    if !journal.available {
        return view;
    }
    // Check the whole bounded payload for conflicting copies before limiting it.
    // Source reads already cap the journal. A defensive overflow is unavailable,
    // not a potentially biased first page reported as a healthy population.
    if journal.records.len() > MAX_SAMPLE {
        view.available = false;
        return view;
    }
    let mut unique = BTreeMap::<String, Option<&Milestone>>::new();
    for record in &journal.records {
        if record.timestamp_ms == 0
            || record.timestamp_ms > observed_at_ms
            || !matches!(
                record.outcome.as_str(),
                "succeeded" | "partial" | "blocked" | "failed"
            )
        {
            view.invalid_records += 1;
            continue;
        }
        let encoded = serde_json::to_vec(record).unwrap_or_default();
        let key = record
            .event_id
            .as_ref()
            .map(|id| format!("event:{id}"))
            .unwrap_or_else(|| format!("legacy:{:x}", Sha256::digest(&encoded)));
        match unique.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Some(record));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                view.duplicate_records += 1;
                if let Some(previous) = *entry.get() {
                    if previous != record {
                        view.conflicting_events += 1;
                        entry.insert(None);
                    }
                }
            }
        }
    }
    view.partial |= view.invalid_records > 0 || view.conflicting_events > 0;
    let mut current = BTreeMap::<(String, Option<String>), Vec<&Milestone>>::new();
    let mut history = BTreeMap::<RevisionKey, Vec<&Milestone>>::new();
    for record in unique.values().flatten() {
        view.sampled_records += 1;
        view.window_start_ms = Some(
            view.window_start_ms
                .map_or(record.timestamp_ms, |at| at.min(record.timestamp_ms)),
        );
        view.window_end_ms = Some(
            view.window_end_ms
                .map_or(record.timestamp_ms, |at| at.max(record.timestamp_ms)),
        );
        let Some(bound) = record.observed_revision.as_ref() else {
            view.unbound_records += 1;
            continue;
        };
        history
            .entry((bound.code.clone(), bound.design.clone()))
            .or_default()
            .push(record);
        if bound == &view.revision {
            current
                .entry((record.tool.clone(), record.verification_level.clone()))
                .or_default()
                .push(record);
        } else {
            view.stale_records += 1;
        }
    }
    view.current = current
        .into_iter()
        .map(|((tool, level), records)| metric(tool, level, records))
        .collect();
    if view.partial {
        for row in &mut view.current {
            row.trend = None;
        }
    }
    view.history = history
        .into_iter()
        .map(|((code, design), records)| ProjectFitnessRevisionView {
            current: code == view.revision.code && design == view.revision.design,
            revision: Revision { code, design },
            samples: records.len(),
            first_at_ms: records
                .iter()
                .map(|record| record.timestamp_ms)
                .min()
                .unwrap_or(0),
            last_at_ms: records
                .iter()
                .map(|record| record.timestamp_ms)
                .max()
                .unwrap_or(0),
        })
        .collect();
    view.history.sort_by(|left, right| {
        right
            .last_at_ms
            .cmp(&left.last_at_ms)
            .then_with(|| left.revision.code.cmp(&right.revision.code))
            .then_with(|| left.revision.design.cmp(&right.revision.design))
    });
    view.history_truncated = view.history.len() > MAX_HISTORY;
    view.history.truncate(MAX_HISTORY);
    view
}

fn ratio(numerator: usize, denominator: usize) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 / denominator as f64)
}

fn median(records: &[&Milestone]) -> Option<f64> {
    let mut values = records
        .iter()
        .map(|record| record.duration_ms)
        .collect::<Vec<_>>();
    values.sort_unstable();
    let middle = values.len() / 2;
    if values.is_empty() {
        None
    } else if values.len().is_multiple_of(2) {
        Some(values[middle - 1] as f64 / 2.0 + values[middle] as f64 / 2.0)
    } else {
        Some(values[middle] as f64)
    }
}

fn success_rate(records: &[&Milestone]) -> Option<f64> {
    ratio(
        records
            .iter()
            .filter(|record| record.outcome == "succeeded")
            .count(),
        records.len(),
    )
}

fn metric(
    tool: String,
    verification_level: Option<String>,
    mut records: Vec<&Milestone>,
) -> ProjectFitnessMetricView {
    records.sort_by(|left, right| {
        left.timestamp_ms
            .cmp(&right.timestamp_ms)
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    let samples = records.len();
    let count = |outcome: &str| {
        records
            .iter()
            .filter(|record| record.outcome == outcome)
            .count()
    };
    let mut durations = records
        .iter()
        .map(|record| record.duration_ms)
        .collect::<Vec<_>>();
    durations.sort_unstable();
    let split = samples / 2;
    let trend = if split >= MIN_TREND_HALF
        && records[split - 1].timestamp_ms < records[split].timestamp_ms
    {
        let (earlier, recent) = records.split_at(split);
        Some(ProjectFitnessTrendView {
            earlier_samples: earlier.len(),
            recent_samples: recent.len(),
            success_rate_delta_pp: (success_rate(recent).unwrap_or(0.0)
                - success_rate(earlier).unwrap_or(0.0))
                * 100.0,
            p50_delta_ms: median(recent).unwrap_or(0.0) - median(earlier).unwrap_or(0.0),
        })
    } else {
        None
    };
    ProjectFitnessMetricView {
        tool,
        verification_level,
        samples,
        succeeded: count("succeeded"),
        partial: count("partial"),
        blocked: count("blocked"),
        failed: count("failed"),
        success_rate: success_rate(&records),
        p50_ms: median(&records),
        p95_ms: samples
            .checked_sub(1)
            .map(|_| durations[(samples * 95).div_ceil(100) - 1]),
        first_at_ms: records.first().map(|record| record.timestamp_ms),
        last_at_ms: records.last().map(|record| record.timestamp_ms),
        trend,
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/intelligence/fitness.rs"]
mod tests;
