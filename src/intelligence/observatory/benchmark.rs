//! Read-only projection of explicit, repository-local benchmark summaries.
//! These reports are descriptive artifacts, never verification Evidence or trust grants.
use crate::evidence::Revision;
use crate::workspace::Workspace;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

const DIRECTORY: &str = "target/engineering-fitness";
const MAX_REPORTS: usize = 32;
const MAX_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Serialize)]
pub struct FitnessBenchmarkView {
    pub available: bool,
    pub status: String,
    pub partial: bool,
    pub invalid_reports: usize,
    pub duplicate_reports: usize,
    pub report_count: usize,
    pub latest: Option<FitnessBenchmarkSnapshot>,
    pub history: Vec<FitnessBenchmarkHistory>,
}

impl Default for FitnessBenchmarkView {
    fn default() -> Self {
        Self {
            available: false,
            status: "not_loaded".into(),
            partial: false,
            invalid_reports: 0,
            duplicate_reports: 0,
            report_count: 0,
            latest: None,
            history: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct FitnessBenchmarkHistory {
    pub captured_at_ms: u64,
    pub revision: Revision,
    pub current: bool,
    pub comparable_to_latest: bool,
    pub artifact_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FitnessBenchmarkSnapshot {
    pub schema_version: u8,
    pub contract_version: u32,
    pub captured_at_ms: u64,
    pub wcode_version: String,
    pub profile: String,
    pub os: String,
    pub arch: String,
    pub case_count: usize,
    pub samples_per_case: usize,
    pub model_calls: usize,
    pub harness_slots: usize,
    pub corpus_sha256: String,
    pub evaluator_sha256: String,
    pub test_binary_sha256: String,
    pub revision_before: Revision,
    pub revision_after: Revision,
    pub source_snapshot_before: String,
    pub source_snapshot_after: String,
    pub source_stable_during_run: bool,
    pub controls_passed: usize,
    pub controls_total: usize,
    pub rows: Vec<FitnessBenchmarkRow>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FitnessBenchmarkRow {
    pub budget: usize,
    pub phase: String,
    pub attempts: usize,
    pub query_errors: usize,
    pub warmup_errors: usize,
    pub over_budget: usize,
    pub required_count: usize,
    pub required_hits: usize,
    pub complete_body_hits: usize,
    pub fresh_sha_hits: usize,
    pub edit_input_eligible: usize,
    pub edit_input_ready: usize,
    pub ranking_attempts: usize,
    pub mean_ndcg_at_10: Option<f64>,
    pub noise_samples: usize,
    pub mean_non_gold_fraction: Option<f64>,
    pub complete_gold_bytes: usize,
    pub density_response_bytes: usize,
    pub latency_samples: usize,
    pub p50_us: Option<f64>,
    pub p95_us: Option<u64>,
}

fn hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn revision_valid(value: &Revision) -> bool {
    value.code.strip_prefix("sha256:").is_some_and(hash)
        && value
            .design
            .as_ref()
            .is_none_or(|design| design.strip_prefix("sha256:").is_some_and(hash))
}

fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn optional_ratio(value: Option<f64>, denominator: usize) -> bool {
    match (value, denominator) {
        (None, 0) => true,
        (Some(value), n) if n > 0 => value.is_finite() && (0.0..=1.0).contains(&value),
        _ => false,
    }
}

impl FitnessBenchmarkSnapshot {
    pub(crate) fn validate(&self, now: u64) -> Result<()> {
        ensure!(
            self.schema_version == 1 && self.contract_version == 3,
            "unsupported Fitness report contract"
        );
        ensure!(
            self.captured_at_ms > 0 && self.captured_at_ms <= now,
            "invalid Fitness report time"
        );
        ensure!(
            self.model_calls == 0 && self.harness_slots > 0 && self.harness_slots <= 64,
            "invalid Fitness execution shape"
        );
        ensure!(
            self.case_count > 0
                && self.case_count <= 10_000
                && self.samples_per_case > 0
                && self.samples_per_case <= 100,
            "invalid Fitness sample count"
        );
        ensure!(
            matches!(self.profile.as_str(), "debug" | "release")
                && [&self.wcode_version, &self.os, &self.arch]
                    .into_iter()
                    .all(|value| token(value)),
            "invalid Fitness environment"
        );
        ensure!(
            [
                &self.corpus_sha256,
                &self.evaluator_sha256,
                &self.test_binary_sha256,
                &self.source_snapshot_before,
                &self.source_snapshot_after
            ]
            .into_iter()
            .all(|value| hash(value)),
            "invalid Fitness fingerprint"
        );
        ensure!(
            revision_valid(&self.revision_before) && revision_valid(&self.revision_after),
            "invalid Fitness revision"
        );
        ensure!(
            !self.source_stable_during_run
                || self.source_snapshot_before == self.source_snapshot_after,
            "contradictory Fitness stability"
        );
        ensure!(
            self.controls_total > 0
                && self.controls_total <= 1_000
                && self.controls_passed <= self.controls_total,
            "invalid Fitness controls"
        );
        ensure!(
            self.rows.len() == 6,
            "incomplete Fitness budget/phase matrix"
        );
        let mut groups = BTreeSet::new();
        for row in &self.rows {
            ensure!(
                matches!(row.budget, 1000 | 2000 | 4000)
                    && matches!(row.phase.as_str(), "cold" | "warm")
                    && groups.insert((row.budget, row.phase.as_str())),
                "duplicate or invalid Fitness matrix group"
            );
            ensure!(
                row.attempts == self.case_count * self.samples_per_case,
                "inconsistent Fitness attempts"
            );
            ensure!(
                row.query_errors <= row.attempts
                    && row.warmup_errors <= self.case_count
                    && row.over_budget <= row.attempts,
                "invalid Fitness failures"
            );
            ensure!(
                row.required_count <= row.attempts.saturating_mul(1000)
                    && row.required_hits <= row.required_count
                    && row.complete_body_hits <= row.required_hits
                    && row.fresh_sha_hits <= row.required_count,
                "invalid Fitness delivery counts"
            );
            ensure!(
                row.edit_input_eligible <= row.attempts
                    && row.edit_input_ready <= row.edit_input_eligible,
                "invalid Fitness edit counts"
            );
            ensure!(
                row.ranking_attempts <= row.attempts
                    && row.noise_samples <= row.attempts.saturating_sub(row.query_errors),
                "invalid Fitness ranking counts"
            );
            ensure!(
                optional_ratio(row.mean_ndcg_at_10, row.ranking_attempts)
                    && optional_ratio(row.mean_non_gold_fraction, row.noise_samples),
                "invalid Fitness ratio"
            );
            ensure!(
                row.complete_gold_bytes <= row.density_response_bytes
                    && row.latency_samples == row.attempts,
                "invalid Fitness byte/latency counts"
            );
            ensure!(
                row.p50_us.is_some_and(|value| value.is_finite()
                    && value >= 0.0
                    && value <= u64::MAX as f64)
                    && row
                        .p95_us
                        .is_some_and(|value| value as f64 >= row.p50_us.unwrap_or(0.0)),
                "invalid Fitness latency distribution"
            );
        }
        Ok(())
    }

    fn current(&self, revision: &Revision) -> bool {
        self.source_stable_during_run
            && self.revision_before == self.revision_after
            && &self.revision_after == revision
    }

    fn comparable(&self, other: &Self) -> bool {
        self.contract_version == other.contract_version
            && self.corpus_sha256 == other.corpus_sha256
            && self.evaluator_sha256 == other.evaluator_sha256
            && self.profile == other.profile
            && self.os == other.os
            && self.arch == other.arch
            && self.samples_per_case == other.samples_per_case
            && self.harness_slots == other.harness_slots
    }
}

fn report_paths(workspace: &Workspace) -> Result<Vec<String>> {
    match workspace.bounded_directory_entries(DIRECTORY, MAX_REPORTS + 1) {
        Ok(paths) => Ok(paths),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(Vec::new())
        }
        Err(error) => Err(error),
    }
}

fn read_summary(workspace: &Workspace, path: &str) -> Result<crate::workspace::FileView> {
    let name = path
        .strip_prefix("target/engineering-fitness/")
        .unwrap_or_default();
    ensure!(
        name.len() <= 80
            && name
                .strip_suffix(".json")
                .is_some_and(|stem| !stem.is_empty()
                    && stem
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || byte == b'-')),
        "invalid Fitness summary name"
    );
    // Reuse the canonical bounded, redacting Workspace reader and path authority.
    let file = workspace.read_file(path, 1, Some(1000))?;
    ensure!(
        !file.redacted && file.total_lines <= 1000 && file.content.len() <= MAX_BYTES,
        "partial, redacted or oversized Fitness summary"
    );
    Ok(file)
}

pub(crate) fn benchmark_signal(workspace: &Workspace) -> String {
    let Ok(paths) = report_paths(workspace) else {
        return "fitness:unavailable".into();
    };
    if paths.len() > MAX_REPORTS {
        return "fitness:over_capacity".into();
    }
    let mut hasher = Sha256::new();
    for path in paths {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        // Content hashes detect a same-size/same-mtime rewrite without trusting filenames.
        match read_summary(workspace, &path) {
            Ok(file) => hasher.update(file.sha256.as_bytes()),
            Err(_) => hasher.update(b"unreadable"),
        }
    }
    format!("fitness:{:x}", hasher.finalize())
}

pub(crate) fn load_benchmarks(workspace: &Workspace, revision: &Revision) -> FitnessBenchmarkView {
    let mut view = FitnessBenchmarkView::default();
    let Ok(paths) = report_paths(workspace) else {
        view.status = "unavailable".into();
        return view;
    };
    if paths.len() > MAX_REPORTS {
        view.partial = true;
        view.status = "over_capacity".into();
        return view;
    }
    view.available = true;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0);
    let mut reports = Vec::new();
    let mut seen = BTreeSet::new();
    for path in paths {
        let report = read_summary(workspace, &path).and_then(|file| {
            let snapshot: FitnessBenchmarkSnapshot = serde_json::from_str(&file.content)?;
            snapshot.validate(now)?;
            Ok((file.sha256, snapshot))
        });
        match report {
            Ok((digest, snapshot)) if seen.insert(digest.clone()) => {
                reports.push((digest, snapshot))
            }
            Ok(_) => view.duplicate_reports += 1,
            Err(_) => view.invalid_reports += 1,
        }
    }
    reports.sort_by(|left, right| {
        right
            .1
            .captured_at_ms
            .cmp(&left.1.captured_at_ms)
            .then_with(|| left.0.cmp(&right.0))
    });
    view.report_count = reports.len();
    view.partial = view.invalid_reports > 0;
    let Some((_, latest)) = reports.first() else {
        view.status = if view.partial {
            "invalid_reports"
        } else {
            "not_measured"
        }
        .into();
        return view;
    };
    // A partial scan may omit a newer report; never call an older valid one current.
    view.status = if view.partial {
        "partial"
    } else if latest.current(revision) {
        "current"
    } else if !latest.source_stable_during_run || latest.revision_before != latest.revision_after {
        "unstable"
    } else {
        "historical"
    }
    .into();
    view.latest = Some(latest.clone());
    view.history = reports
        .iter()
        .map(|(digest, report)| FitnessBenchmarkHistory {
            captured_at_ms: report.captured_at_ms,
            revision: report.revision_after.clone(),
            current: !view.partial && report.current(revision),
            comparable_to_latest: report.comparable(latest),
            artifact_sha256: digest.clone(),
        })
        .collect();
    view
}

#[cfg(test)]
#[path = "../../../tests/unit/intelligence/benchmark.rs"]
mod tests;
