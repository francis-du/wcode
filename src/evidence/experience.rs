use crate::evidence::Revision;
use crate::evidence_store::workspace_state_directory;
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "experience_io.rs"]
mod experience_io;

const EXPERIENCE_SCHEMA_VERSION: u8 = 2;
const MAX_STORED_EXPERIENCES: usize = 512;
// Very broad release/migration sweeps are deliberately not learned as one
// all-to-all relationship because they create noisy retrieval shortcuts.
const MAX_EXPERIENCE_PATHS: usize = 24;
const MAX_CONTEXT_PATHS: usize = 12;
const MAX_CONTEXT_TRAJECTORY_AGE_MS: u64 = 6 * 60 * 60 * 1_000;
const CONTEXT_TRAJECTORY_WEIGHT: f64 = 0.45;
const MAX_EXPERIENCE_BYTES: u64 = 32 * 1024;
const MAX_PATH_LENGTH: usize = 512;
const EXPERIENCE_EVALUATION_TOP_K: usize = 5;
const EXPERIENCE_RETRIEVAL_MODEL: &str = "verified-context-cochange-v3";
const EXPERIENCE_BASELINE_MODEL: &str = "raw-count-v1";
const EXPERIENCE_EVALUATION_METHOD: &str = "global-temporal-ab-v2";
const EXPERIENCE_SCORE_SCALE: f64 = 1_000.0;
const EXPERIENCE_DECAY_HALF_LIFE_MS: u64 = 30 * 24 * 60 * 60 * 1_000;
const EXPERIENCE_CONFIDENCE_PRIOR: f64 = 3.0;
const EXPERIENCE_ACTIVATION_POLICY: &str = "temporal-regression-gate-v1";
const EXPERIENCE_ACTIVATION_MIN_RECORDS: usize = 6;
const EXPERIENCE_ACTIVATION_MARGIN: f64 = 0.05;
const EXPERIENCE_DEGRADED_FACTOR: f64 = 0.25;
const EXPERIENCE_ACTIVATION_CACHE_LIMIT: usize = 64;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct VerifiedChangeExperience {
    schema_version: u8,
    revision: Revision,
    level: String,
    paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    context_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retrieval_intent: Option<String>,
    timestamp_ms: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct ExperienceActivation {
    pub(crate) state: &'static str,
    pub(crate) reason: &'static str,
    pub(crate) factor: f64,
    pub(crate) evaluable_records: usize,
}

impl Default for ExperienceActivation {
    fn default() -> Self {
        Self {
            state: "cold_start",
            reason: "insufficient_temporal_evidence",
            factor: 1.0,
            evaluable_records: 0,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ExperienceMatches {
    pub(crate) available: bool,
    pub(crate) weights: BTreeMap<String, u16>,
    pub(crate) matched_records: usize,
    pub(crate) scanned_records: usize,
    pub(crate) truncated: bool,
    pub(crate) activation: ExperienceActivation,
}

impl ExperienceMatches {
    pub(crate) fn unavailable() -> Self {
        Self::default()
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ExperienceEvaluation {
    pub(crate) available: bool,
    pub(crate) records: usize,
    pub(crate) full_records: usize,
    pub(crate) quick_records: usize,
    pub(crate) unique_paths: usize,
    pub(crate) live_paths: usize,
    pub(crate) stale_path_references: usize,
    pub(crate) evaluable_records: usize,
    pub(crate) records_with_prediction: usize,
    pub(crate) baseline_records_with_prediction: usize,
    pub(crate) evaluation_cases: usize,
    pub(crate) prediction_cases: usize,
    pub(crate) baseline_prediction_cases: usize,
    pub(crate) hit_cases: usize,
    pub(crate) baseline_hit_cases: usize,
    pub(crate) predictions: usize,
    pub(crate) baseline_predictions: usize,
    pub(crate) true_positives: usize,
    pub(crate) baseline_true_positives: usize,
    pub(crate) expected_targets: usize,
    pub(crate) top_k: usize,
    pub(crate) latest_record_at_ms: Option<u64>,
}

pub(crate) fn persist_verified_change(
    workspace: &Workspace,
    revision: &Revision,
    level: &str,
    paths: &[String],
) -> Result<bool> {
    if !matches!(level, "quick" | "full") {
        bail!("verified experience requires quick or full verification");
    }
    let paths = normalize_paths(paths);
    // A single changed file has no edit-ripple relationship to learn. Large
    // change sets are intentionally ignored rather than teaching noisy global
    // co-change edges from release sweeps or generated migrations.
    if paths.len() < 2 || paths.len() > MAX_EXPERIENCE_PATHS {
        return Ok(false);
    }
    if revision.code.trim().is_empty() || revision.code.ends_with(":partial") {
        return Ok(false);
    }

    let directory = experience_directory(workspace)?;
    ensure_regular_directory(&directory)?;
    let stable = serde_json::to_vec(&(EXPERIENCE_SCHEMA_VERSION, revision, level, &paths))?;
    let digest = digest_bytes(&stable);
    let path = directory.join(format!("{}.json", &digest[..32]));
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                bail!("verified experience record path is not a regular file");
            }
            return Ok(false);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    let (context_paths, retrieval_intent) = recent_context_trajectory(workspace, &paths);
    let record = VerifiedChangeExperience {
        schema_version: EXPERIENCE_SCHEMA_VERSION,
        revision: revision.clone(),
        level: level.to_owned(),
        paths,
        context_paths,
        retrieval_intent,
        timestamp_ms: now_ms(),
    };
    let bytes = serde_json::to_vec(&record)?;
    if bytes.len() as u64 > MAX_EXPERIENCE_BYTES {
        bail!("verified experience record exceeds the persistent store size bound");
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .with_context(|| format!("cannot create verified experience {}", path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("cannot write verified experience {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("cannot sync verified experience {}", path.display()))?;
    prune_directory(&directory)?;
    Ok(true)
}

#[cfg(test)]
pub(crate) fn related_paths(
    workspace: &Workspace,
    anchors: &BTreeSet<String>,
    max_results: usize,
) -> Result<ExperienceMatches> {
    related_paths_for_intent(workspace, anchors, None, max_results)
}

pub(crate) fn related_paths_for_intent(
    workspace: &Workspace,
    anchors: &BTreeSet<String>,
    retrieval_intent: Option<&str>,
    max_results: usize,
) -> Result<ExperienceMatches> {
    if anchors.is_empty() {
        return Ok(ExperienceMatches {
            available: true,
            ..ExperienceMatches::default()
        });
    }
    let records = load(workspace)?;
    let activation = activation_for_records(workspace, &records);
    let matched_records = records
        .iter()
        .filter(|record| {
            record.paths.iter().any(|path| anchors.contains(path))
                || record
                    .context_paths
                    .iter()
                    .any(|path| anchors.contains(path))
        })
        .count();
    let mut model = NormalizedCochangeModel::default();
    for record in &records {
        model.decay_to(record.timestamp_ms);
        model.learn(&record.paths, &record.level, record.paths.len());
        model.learn_trajectory(
            &record.context_paths,
            &record.paths,
            &record.level,
            trajectory_intent_factor(record.retrieval_intent.as_deref(), retrieval_intent),
        );
    }
    model.decay_to(now_ms());

    let limit = max_results.max(1);
    let ranked = model.ranked_scores(anchors);
    let mut live_ranked = Vec::with_capacity(ranked.len().min(limit.saturating_add(1)));
    for (path, score) in ranked {
        if workspace.source_metadata_stamp(&path).is_err() {
            continue;
        }
        live_ranked.push((path, score));
        if live_ranked.len() > limit {
            break;
        }
    }
    let truncated = live_ranked.len() > limit;
    live_ranked.truncate(limit);
    let mut weights = live_ranked.into_iter().collect::<BTreeMap<_, _>>();
    if activation.factor < 1.0 {
        for weight in weights.values_mut() {
            *weight = ((*weight as f64) * activation.factor).round() as u16;
        }
        weights.retain(|_, weight| *weight > 0);
    }
    Ok(ExperienceMatches {
        available: true,
        weights,
        matched_records,
        scanned_records: records.len(),
        truncated,
        activation,
    })
}

fn recent_context_trajectory(
    workspace: &Workspace,
    verified_paths: &[String],
) -> (Vec<String>, Option<String>) {
    let Ok(history) = crate::engineering_journal::load_recent(workspace, 64) else {
        return (Vec::new(), None);
    };
    let now = now_ms();
    for milestone in history.records {
        if milestone.tool != "agent_context"
            || milestone.stage != "understand"
            || milestone.outcome != "succeeded"
            || milestone.paths.is_empty()
            || now.saturating_sub(milestone.timestamp_ms) > MAX_CONTEXT_TRAJECTORY_AGE_MS
        {
            continue;
        }
        let context_paths = normalize_paths(&milestone.paths)
            .into_iter()
            .take(MAX_CONTEXT_PATHS)
            .collect::<Vec<_>>();
        if context_paths
            .iter()
            .any(|path| verified_paths.iter().any(|verified| verified == path))
        {
            return (context_paths, milestone.retrieval_intent);
        }
    }
    (Vec::new(), None)
}

pub(crate) fn evaluate_history(workspace: &Workspace) -> Result<ExperienceEvaluation> {
    let records = load(workspace)?;
    evaluate_history_from_records(workspace, &records)
}

fn evaluate_history_from_records(
    workspace: &Workspace,
    records: &[VerifiedChangeExperience],
) -> Result<ExperienceEvaluation> {
    let paths = experience_io::prepare_evaluation_paths(workspace, records)?;
    Ok(evaluate_prepared_history(records, paths))
}

fn evaluate_prepared_history(
    records: &[VerifiedChangeExperience],
    paths: experience_io::EvaluationPaths,
) -> ExperienceEvaluation {
    #[cfg(test)]
    tests::record_temporal_evaluation();
    let prepared = paths.prepared;
    let prepared_context = paths.prepared_context;

    let mut evaluation = ExperienceEvaluation {
        available: true,
        records: records.len(),
        full_records: records
            .iter()
            .filter(|record| record.level == "full")
            .count(),
        quick_records: records
            .iter()
            .filter(|record| record.level == "quick")
            .count(),
        unique_paths: paths.unique_paths,
        live_paths: paths.live_paths,
        stale_path_references: paths.stale_path_references,
        top_k: EXPERIENCE_EVALUATION_TOP_K,
        latest_record_at_ms: records.last().map(|record| record.timestamp_ms),
        ..ExperienceEvaluation::default()
    };
    let mut candidate_model = NormalizedCochangeModel::default();
    let mut baseline_model = RawCountModel::default();
    let mut candidate_metrics = EvaluationCounters::default();
    let mut baseline_metrics = EvaluationCounters::default();
    let mut index = 0usize;
    while index < records.len() {
        let timestamp_ms = records[index].timestamp_ms;
        let mut end = index + 1;
        while end < records.len() && records[end].timestamp_ms == timestamp_ms {
            end += 1;
        }
        candidate_model.decay_to(timestamp_ms);

        // Evaluate the whole timestamp batch before teaching any record from it.
        // This preserves a global historical boundary even when multiple
        // verifications land in the same millisecond.
        for record_paths in &prepared[index..end] {
            if record_paths.len() < 2 {
                continue;
            }
            evaluation.evaluable_records = evaluation.evaluable_records.saturating_add(1);
            let mut candidate_record_has_prediction = false;
            let mut baseline_record_has_prediction = false;
            for anchor in record_paths {
                evaluation.evaluation_cases = evaluation.evaluation_cases.saturating_add(1);
                evaluation.expected_targets = evaluation
                    .expected_targets
                    .saturating_add(record_paths.len().saturating_sub(1));
                let anchors = BTreeSet::from([anchor.clone()]);
                let candidate_ranked = candidate_model
                    .ranked_scores(&anchors)
                    .into_iter()
                    .take(EXPERIENCE_EVALUATION_TOP_K)
                    .map(|(path, _)| path)
                    .collect::<Vec<_>>();
                candidate_record_has_prediction |= observe_ranked_case(
                    &mut candidate_metrics,
                    &candidate_ranked,
                    anchor,
                    record_paths,
                );
                let baseline_ranked =
                    baseline_model.ranked_paths(anchor, EXPERIENCE_EVALUATION_TOP_K);
                baseline_record_has_prediction |= observe_ranked_case(
                    &mut baseline_metrics,
                    &baseline_ranked,
                    anchor,
                    record_paths,
                );
            }
            candidate_metrics.records_with_prediction = candidate_metrics
                .records_with_prediction
                .saturating_add(usize::from(candidate_record_has_prediction));
            baseline_metrics.records_with_prediction = baseline_metrics
                .records_with_prediction
                .saturating_add(usize::from(baseline_record_has_prediction));
        }

        for offset in index..end {
            let record = &records[offset];
            let record_paths = &prepared[offset];
            candidate_model.learn(record_paths, &record.level, record.paths.len());
            candidate_model.learn_trajectory(
                &prepared_context[offset],
                record_paths,
                &record.level,
                trajectory_intent_factor(record.retrieval_intent.as_deref(), None),
            );
            baseline_model.learn(record_paths, &record.level);
        }
        index = end;
    }
    evaluation.records_with_prediction = candidate_metrics.records_with_prediction;
    evaluation.baseline_records_with_prediction = baseline_metrics.records_with_prediction;
    evaluation.prediction_cases = candidate_metrics.prediction_cases;
    evaluation.baseline_prediction_cases = baseline_metrics.prediction_cases;
    evaluation.hit_cases = candidate_metrics.hit_cases;
    evaluation.baseline_hit_cases = baseline_metrics.hit_cases;
    evaluation.predictions = candidate_metrics.predictions;
    evaluation.baseline_predictions = baseline_metrics.predictions;
    evaluation.true_positives = candidate_metrics.true_positives;
    evaluation.baseline_true_positives = baseline_metrics.true_positives;
    evaluation
}

#[derive(Default)]
struct EvaluationCounters {
    records_with_prediction: usize,
    prediction_cases: usize,
    hit_cases: usize,
    predictions: usize,
    true_positives: usize,
}

#[derive(Default)]
struct NormalizedCochangeModel {
    last_timestamp_ms: Option<u64>,
    path_support: BTreeMap<String, f64>,
    cochange: BTreeMap<String, BTreeMap<String, f64>>,
}

impl NormalizedCochangeModel {
    fn decay_to(&mut self, timestamp_ms: u64) {
        let Some(previous) = self.last_timestamp_ms else {
            self.last_timestamp_ms = Some(timestamp_ms);
            return;
        };
        if timestamp_ms <= previous {
            return;
        }
        let age = timestamp_ms.saturating_sub(previous) as f64;
        let factor = (-(age / EXPERIENCE_DECAY_HALF_LIFE_MS as f64)).exp2();
        for support in self.path_support.values_mut() {
            *support *= factor;
        }
        for candidates in self.cochange.values_mut() {
            for weight in candidates.values_mut() {
                *weight *= factor;
            }
        }
        self.last_timestamp_ms = Some(timestamp_ms);
    }

    fn learn(&mut self, paths: &[String], level: &str, original_path_count: usize) {
        if paths.is_empty() {
            return;
        }
        let support_weight = level_weight(level);
        for path in paths {
            *self.path_support.entry(path.clone()).or_default() += support_weight;
        }
        if paths.len() < 2 {
            return;
        }
        let breadth = original_path_count.saturating_sub(1).max(1) as f64;
        let pair_weight = support_weight / breadth;
        for anchor in paths {
            let candidates = self.cochange.entry(anchor.clone()).or_default();
            for candidate in paths {
                if candidate != anchor {
                    *candidates.entry(candidate.clone()).or_default() += pair_weight;
                }
            }
        }
    }

    fn learn_trajectory(
        &mut self,
        context_paths: &[String],
        changed_paths: &[String],
        level: &str,
        intent_factor: f64,
    ) {
        if context_paths.is_empty() || changed_paths.is_empty() || intent_factor <= 0.0 {
            return;
        }
        let support_weight = level_weight(level) * CONTEXT_TRAJECTORY_WEIGHT * intent_factor;
        for path in context_paths {
            *self.path_support.entry(path.clone()).or_default() += support_weight;
        }
        let breadth = changed_paths.len().max(1) as f64;
        let edge_weight = support_weight / breadth;
        for anchor in context_paths {
            let candidates = self.cochange.entry(anchor.clone()).or_default();
            for candidate in changed_paths {
                if candidate != anchor {
                    *candidates.entry(candidate.clone()).or_default() += edge_weight;
                }
            }
        }
    }

    fn ranked_scores(&self, anchors: &BTreeSet<String>) -> Vec<(String, u16)> {
        let mut candidates = BTreeSet::<String>::new();
        for anchor in anchors {
            if let Some(related) = self.cochange.get(anchor) {
                candidates.extend(
                    related
                        .keys()
                        .filter(|candidate| !anchors.contains(*candidate))
                        .cloned(),
                );
            }
        }
        let mut ranked = candidates
            .into_iter()
            .filter_map(|candidate| {
                let combined = anchors
                    .iter()
                    .map(|anchor| self.pair_score(anchor, &candidate) as u32)
                    .sum::<u32>()
                    .min(EXPERIENCE_SCORE_SCALE as u32) as u16;
                (combined > 0).then_some((candidate, combined))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|(left_path, left_score), (right_path, right_score)| {
            right_score
                .cmp(left_score)
                .then_with(|| left_path.cmp(right_path))
        });
        ranked
    }

    fn pair_score(&self, anchor: &str, candidate: &str) -> u16 {
        let Some(cochange) = self
            .cochange
            .get(anchor)
            .and_then(|candidates| candidates.get(candidate))
            .copied()
        else {
            return 0;
        };
        let Some(anchor_support) = self.path_support.get(anchor).copied() else {
            return 0;
        };
        let Some(candidate_support) = self.path_support.get(candidate).copied() else {
            return 0;
        };
        let denominator = anchor_support + candidate_support;
        if cochange <= 0.0 || denominator <= 0.0 {
            return 0;
        }
        // Weighted Dice penalizes globally popular files, while confidence
        // shrinkage prevents one accidental co-change from looking certain.
        // Pair weight is already discounted by the original change-set breadth.
        let association = (2.0 * cochange / denominator).clamp(0.0, 1.0);
        let confidence = cochange / (cochange + EXPERIENCE_CONFIDENCE_PRIOR);
        (association * confidence * EXPERIENCE_SCORE_SCALE)
            .round()
            .clamp(0.0, EXPERIENCE_SCORE_SCALE) as u16
    }
}

#[derive(Default)]
struct RawCountModel {
    cochange: BTreeMap<String, BTreeMap<String, u32>>,
}

impl RawCountModel {
    fn learn(&mut self, paths: &[String], level: &str) {
        if paths.len() < 2 {
            return;
        }
        let weight = if level == "full" { 3 } else { 1 };
        for anchor in paths {
            let candidates = self.cochange.entry(anchor.clone()).or_default();
            for candidate in paths {
                if candidate != anchor {
                    let entry = candidates.entry(candidate.clone()).or_default();
                    *entry = entry.saturating_add(weight);
                }
            }
        }
    }

    fn ranked_paths(&self, anchor: &str, limit: usize) -> Vec<String> {
        let Some(candidates) = self.cochange.get(anchor) else {
            return Vec::new();
        };
        let mut ranked = candidates.iter().collect::<Vec<_>>();
        ranked.sort_by(|(left_path, left_weight), (right_path, right_weight)| {
            right_weight
                .cmp(left_weight)
                .then_with(|| left_path.cmp(right_path))
        });
        ranked
            .into_iter()
            .take(limit)
            .map(|(path, _)| path.clone())
            .collect()
    }
}

fn level_weight(level: &str) -> f64 {
    if level == "full" {
        3.0
    } else {
        1.0
    }
}

fn trajectory_intent_factor(recorded: Option<&str>, requested: Option<&str>) -> f64 {
    match (recorded, requested) {
        (Some(recorded), Some(requested)) if recorded == requested => 1.35,
        (Some(_), Some(_)) => 0.35,
        (None, Some(_)) => 0.55,
        (_, None) => 0.60,
    }
}

fn observe_ranked_case(
    metrics: &mut EvaluationCounters,
    ranked: &[String],
    anchor: &str,
    expected_paths: &[String],
) -> bool {
    if ranked.is_empty() {
        return false;
    }
    metrics.prediction_cases = metrics.prediction_cases.saturating_add(1);
    metrics.predictions = metrics.predictions.saturating_add(ranked.len());
    let hits = ranked
        .iter()
        .filter(|candidate| {
            expected_paths
                .iter()
                .any(|expected| expected != anchor && expected == *candidate)
        })
        .count();
    metrics.true_positives = metrics.true_positives.saturating_add(hits);
    metrics.hit_cases = metrics.hit_cases.saturating_add(usize::from(hits > 0));
    true
}

pub(crate) fn activation_policy() -> &'static str {
    EXPERIENCE_ACTIVATION_POLICY
}

pub(crate) fn retrieval_model() -> &'static str {
    EXPERIENCE_RETRIEVAL_MODEL
}

pub(crate) fn baseline_model() -> &'static str {
    EXPERIENCE_BASELINE_MODEL
}

pub(crate) fn evaluation_method() -> &'static str {
    EXPERIENCE_EVALUATION_METHOD
}

pub(crate) fn capabilities() -> serde_json::Value {
    serde_json::json!({
        "persistent": true,
        "format": "verified-context-change-v2",
        "scope": "per-workspace",
        "learning_source": "successful deterministic verification",
        "retrieval_precision": "heuristic",
        "retrieval_model": EXPERIENCE_RETRIEVAL_MODEL,
        "baseline_model": EXPERIENCE_BASELINE_MODEL,
        "activation_policy": EXPERIENCE_ACTIVATION_POLICY,
        "activation_min_evaluable_records": EXPERIENCE_ACTIVATION_MIN_RECORDS,
        "degraded_weight_factor": EXPERIENCE_DEGRADED_FACTOR,
        "stores_prompts_or_chain_of_thought": false,
        "evaluation": EXPERIENCE_EVALUATION_METHOD,
        "evaluation_top_k": EXPERIENCE_EVALUATION_TOP_K,
        "recency_half_life_days": EXPERIENCE_DECAY_HALF_LIFE_MS / (24 * 60 * 60 * 1_000),
        "change_set_discount": "inverse-other-path-count",
        "trajectory_learning": "recent-selected-context-to-verified-change",
        "trajectory_intent_conditioning": "same-intent-stronger-cross-intent-weaker",
        "trajectory_weight": CONTEXT_TRAJECTORY_WEIGHT,
        "trajectory_max_age_hours": MAX_CONTEXT_TRAJECTORY_AGE_MS / (60 * 60 * 1_000),
        "popularity_normalization": "weighted-dice-with-confidence-shrinkage",
        "max_records": MAX_STORED_EXPERIENCES,
        "max_paths_per_record": MAX_EXPERIENCE_PATHS,
        "max_record_bytes": MAX_EXPERIENCE_BYTES,
    })
}

#[derive(Clone)]
struct CachedActivation {
    fingerprint: [u8; 32],
    activation: ExperienceActivation,
}

static ACTIVATION_CACHE: OnceLock<Mutex<BTreeMap<PathBuf, CachedActivation>>> = OnceLock::new();

fn activation_for_records(
    workspace: &Workspace,
    records: &[VerifiedChangeExperience],
) -> ExperienceActivation {
    // One request-local liveness snapshot drives both the cache key and replay.
    // Historical revisions and present file membership are independent inputs.
    let prepared = experience_io::prepare_evaluation_paths(workspace, records).and_then(|paths| {
        experience_io::activation_fingerprint(records, &paths).map(|key| (paths, key))
    });
    let Ok((paths, fingerprint)) = prepared else {
        return ExperienceActivation {
            state: "blocked",
            reason: "temporal_evaluation_unavailable",
            factor: 0.0,
            evaluable_records: 0,
        };
    };
    let cache = ACTIVATION_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Ok(cache) = cache.lock() {
        if let Some(cached) = cache.get(workspace.root()) {
            if cached.fingerprint == fingerprint {
                return cached.activation.clone();
            }
        }
    }

    let flight = experience_io::activation_flight(workspace.root());
    let _flight = flight
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Ok(cache) = cache.lock() {
        if let Some(cached) = cache.get(workspace.root()) {
            if cached.fingerprint == fingerprint {
                return cached.activation.clone();
            }
        }
    }

    let activation = activation_from_evaluation(&evaluate_prepared_history(records, paths));
    if let Ok(mut cache) = cache.lock() {
        if cache.len() >= EXPERIENCE_ACTIVATION_CACHE_LIMIT && !cache.contains_key(workspace.root())
        {
            if let Some(oldest) = cache.keys().next().cloned() {
                cache.remove(&oldest);
            }
        }
        cache.insert(
            workspace.root().to_path_buf(),
            CachedActivation {
                fingerprint,
                activation: activation.clone(),
            },
        );
    }
    activation
}

fn activation_from_evaluation(evaluation: &ExperienceEvaluation) -> ExperienceActivation {
    if evaluation.evaluable_records < EXPERIENCE_ACTIVATION_MIN_RECORDS {
        return ExperienceActivation {
            evaluable_records: evaluation.evaluable_records,
            ..ExperienceActivation::default()
        };
    }
    let candidate = [
        ratio(evaluation.prediction_cases, evaluation.evaluation_cases),
        ratio(evaluation.hit_cases, evaluation.prediction_cases),
        ratio(evaluation.true_positives, evaluation.predictions),
        ratio(evaluation.true_positives, evaluation.expected_targets),
    ];
    let baseline = [
        ratio(
            evaluation.baseline_prediction_cases,
            evaluation.evaluation_cases,
        ),
        ratio(
            evaluation.baseline_hit_cases,
            evaluation.baseline_prediction_cases,
        ),
        ratio(
            evaluation.baseline_true_positives,
            evaluation.baseline_predictions,
        ),
        ratio(
            evaluation.baseline_true_positives,
            evaluation.expected_targets,
        ),
    ];
    let regressions = candidate
        .iter()
        .zip(baseline)
        .filter(|(candidate, baseline)| **candidate + EXPERIENCE_ACTIVATION_MARGIN < *baseline)
        .count();
    let improvements = candidate
        .iter()
        .zip(baseline)
        .filter(|(candidate, baseline)| **candidate > *baseline + EXPERIENCE_ACTIVATION_MARGIN)
        .count();
    if regressions >= 3 || (regressions >= 2 && improvements == 0) {
        ExperienceActivation {
            state: "degraded",
            reason: "temporal_holdout_regressed_vs_baseline",
            factor: EXPERIENCE_DEGRADED_FACTOR,
            evaluable_records: evaluation.evaluable_records,
        }
    } else {
        ExperienceActivation {
            state: "active",
            reason: "temporal_holdout_non_regressing",
            factor: 1.0,
            evaluable_records: evaluation.evaluable_records,
        }
    }
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn load(workspace: &Workspace) -> Result<Vec<VerifiedChangeExperience>> {
    let directory = experience_directory(workspace)?;
    let metadata = match fs::symlink_metadata(&directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("verified experience store path is not a regular directory");
    }
    let paths = experience_paths(&directory)?
        .into_iter()
        .rev()
        .take(MAX_STORED_EXPERIENCES)
        .collect::<Vec<_>>();
    let loaded = crate::resource::parallel_io(&paths, |path| experience_io::read_record(path))?;
    let mut records = Vec::with_capacity(paths.len());
    for record in loaded {
        if let Some(record) = record? {
            records.push(record);
        }
    }
    records.sort_by(|left, right| {
        left.timestamp_ms
            .cmp(&right.timestamp_ms)
            .then_with(|| left.revision.code.cmp(&right.revision.code))
            .then_with(|| left.level.cmp(&right.level))
            .then_with(|| left.paths.cmp(&right.paths))
    });
    Ok(records)
}

fn valid_record(record: &VerifiedChangeExperience) -> bool {
    matches!(record.schema_version, 1 | EXPERIENCE_SCHEMA_VERSION)
        && matches!(record.level.as_str(), "quick" | "full")
        && !record.revision.code.trim().is_empty()
        && !record.revision.code.ends_with(":partial")
        && (2..=MAX_EXPERIENCE_PATHS).contains(&record.paths.len())
        && record
            .paths
            .iter()
            .all(|path| normalize_path(path).is_some())
        && normalize_paths(&record.paths) == record.paths
        && record.context_paths.len() <= MAX_CONTEXT_PATHS
        && record
            .context_paths
            .iter()
            .all(|path| normalize_path(path).is_some())
        && normalize_paths(&record.context_paths) == record.context_paths
        && record.retrieval_intent.as_ref().is_none_or(|intent| {
            matches!(
                intent.as_str(),
                "balanced_context"
                    | "trace_to_code"
                    | "code_to_test"
                    | "comment_to_context"
                    | "failure_trace_to_code"
                    | "edit_to_ripple"
            )
        })
}

fn normalize_paths(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter_map(|path| normalize_path(path))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn normalize_path(raw: &str) -> Option<String> {
    let replaced = raw.trim().replace('\\', "/");
    if replaced.is_empty() || replaced.len() > MAX_PATH_LENGTH {
        return None;
    }
    let path = Path::new(&replaced);
    if path.is_absolute() {
        return None;
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?.to_owned()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn experience_directory(workspace: &Workspace) -> Result<PathBuf> {
    Ok(workspace_state_directory(workspace)?.join("experience"))
}

fn ensure_regular_directory(directory: &Path) -> Result<()> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                bail!("verified experience store path is not a regular directory");
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(directory)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn experience_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(directory)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(".json"))
                .then(|| entry.path())
        })
        .collect::<Vec<_>>();
    paths.sort_by(|left, right| {
        let left_modified = fs::symlink_metadata(left)
            .and_then(|metadata| metadata.modified())
            .ok();
        let right_modified = fs::symlink_metadata(right)
            .and_then(|metadata| metadata.modified())
            .ok();
        left_modified
            .cmp(&right_modified)
            .then_with(|| left.cmp(right))
    });
    Ok(paths)
}

fn prune_directory(directory: &Path) -> Result<()> {
    let paths = experience_paths(directory)?;
    let excess = paths.len().saturating_sub(MAX_STORED_EXPERIENCES);
    for path in paths.into_iter().take(excess) {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
#[path = "../../tests/unit/evidence/experience.rs"]
mod tests;
