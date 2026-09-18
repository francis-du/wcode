use super::corpus::{Case, Gold, Identity};
use crate::decision::{
    agent_context_decisions, calibration_brier_million, compare_decision_batches,
    evaluate_agent_context_provider, probability_milli, DecisionBatch, DecisionProvider,
    DecisionShadowComparison, CONTEXT_SUFFICIENT_STOP_THRESHOLD_MILLI,
};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize)]
pub(super) struct Score {
    pub delivery: super::delivery::Delivery,
    pub required_count: usize,
    pub required_hits: usize,
    pub required_recall: Option<f64>,
    pub recall_at_5: Option<f64>,
    pub recall_at_10: Option<f64>,
    pub first_required_rank: Option<usize>,
    pub ndcg_at_10: Option<f64>,
    pub delivered_identities: usize,
    pub non_gold_symbol_fraction: Option<f64>,
    pub non_gold_identities: Vec<Identity>,
    pub fresh_sha_hits: usize,
    pub complete_body_hits: usize,
    pub complete_body_recall: Option<f64>,
    pub all_required_edit_inputs: bool,
    pub gold_context_sufficient: bool,
    pub unique_patch_precondition_hits: usize,
    pub all_required_unique_patch_preconditions: bool,
    pub reported_edit_ready: bool,
    pub decision_plane_exposed: bool,
    pub context_sufficient_milli: Option<u16>,
    pub context_sufficient_brier_million: Option<u32>,
    pub abstained: Option<bool>,
    pub response_bytes: usize,
    pub estimated_tokens: usize,
    pub budget_tokens: Option<usize>,
    pub within_budget: bool,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct ProviderContextCalibration {
    pub provider: String,
    pub probability_milli: Option<u16>,
    pub brier_million: Option<u32>,
    pub predicted_stop: Option<bool>,
    pub gold_context_sufficient: bool,
    pub comparison: DecisionShadowComparison,
}

pub(super) fn provider_context_calibration(
    pack: &Value,
    case: &Case,
    provider: &dyn DecisionProvider,
) -> ProviderContextCalibration {
    let gold_context_sufficient = score(pack, case).gold_context_sufficient;
    let baseline = agent_context_decisions(pack, &case.query);
    let candidate = evaluate_agent_context_provider(provider, pack, &case.query);
    let probability_milli = probability_milli(&candidate, "context_sufficient");
    ProviderContextCalibration {
        provider: candidate.provider.clone(),
        probability_milli,
        brier_million: probability_milli
            .map(|probability| calibration_brier_million(probability, gold_context_sufficient)),
        predicted_stop: probability_milli
            .map(|probability| probability >= CONTEXT_SUFFICIENT_STOP_THRESHOLD_MILLI),
        gold_context_sufficient,
        comparison: compare_decision_batches(&baseline, &candidate),
    }
}

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn identity(item: &Value) -> Option<Identity> {
    let path = item["path"].as_str()?;
    let symbol = item["qualified_name"].as_str()?;
    if path.is_empty() || symbol.is_empty() {
        return None;
    }
    Some(Identity::new(path, symbol))
}

// Stable deduplicated delivery order, NOT the internal retrieval candidate rank.
pub(super) fn delivered(pack: &Value) -> Vec<Identity> {
    let mut seen = BTreeSet::new();
    let mut ordered = Vec::new();
    for values in [
        &pack["targets"],
        &pack["repo_map"]["items"],
        &pack["hot_source"],
    ] {
        for item in values.as_array().into_iter().flatten() {
            if let Some(id) = identity(item) {
                if seen.insert(id.clone()) {
                    ordered.push(id);
                }
            }
        }
    }
    ordered
}

fn ratio(hits: usize, total: usize) -> Option<f64> {
    (total > 0).then(|| hits as f64 / total as f64)
}

fn current_file<'a>(pack: &'a Value, case: &Case, gold: &Gold) -> Option<&'a Value> {
    let source = case.files.get(&gold.identity.path)?;
    let sha = digest(source.as_bytes());
    pack["files"].as_array()?.iter().find(|file| {
        file["path"] == gold.identity.path && file["sha256"].as_str() == Some(sha.as_str())
    })
}

pub(super) fn complete_body(pack: &Value, case: &Case, gold: &Gold) -> bool {
    let Some(original) = case.files.get(&gold.identity.path) else {
        return false;
    };
    let sha = digest(original.as_bytes());
    pack["hot_source"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|item| {
            if identity(item).as_ref() != Some(&gold.identity)
                || item["sha256"].as_str() != Some(sha.as_str())
                || item["body"]["redacted"].as_bool() != Some(false)
            {
                return false;
            }
            let body = &item["body"];
            let (Some(start), Some(end), Some(content)) = (
                body["start_line"].as_u64(),
                body["end_line"].as_u64(),
                body["content"].as_str(),
            ) else {
                return false;
            };
            if start == 0 || end < start || content.is_empty() {
                return false;
            }
            let Some(offset) = line_offset(original, start as usize) else {
                return false;
            };
            original[offset..].starts_with(content)
                && end == start + content.lines().count() as u64 - 1
                && content.contains(gold.fragment.trim_end_matches(['\r', '\n']))
        })
}

fn unique_patch_precondition(pack: &Value, case: &Case, gold: &Gold) -> bool {
    let Some(original) = case.files.get(&gold.identity.path) else {
        return false;
    };
    let sha = digest(original.as_bytes());
    let fragment = gold.fragment.trim_end_matches(['\r', '\n']);
    pack["hot_source"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| {
            identity(item).as_ref() == Some(&gold.identity)
                && item["sha256"].as_str() == Some(sha.as_str())
                && item["body"]["redacted"].as_bool() == Some(false)
        })
        .any(|item| {
            let body = &item["body"];
            let (Some(start), Some(end), Some(content)) = (
                body["start_line"].as_u64(),
                body["end_line"].as_u64(),
                body["content"].as_str(),
            ) else {
                return false;
            };
            if start == 0 || end < start || content.is_empty() || !content.contains(fragment) {
                return false;
            }
            let Some(offset) = line_offset(original, start as usize) else {
                return false;
            };
            original[offset..].starts_with(content)
                && end == start + content.lines().count() as u64 - 1
                && original.match_indices(content).nth(1).is_none()
        })
}

fn line_offset(source: &str, line: usize) -> Option<usize> {
    if line == 1 {
        return Some(0);
    }
    source
        .match_indices('\n')
        .nth(line.checked_sub(2)?)
        .map(|(offset, _)| offset + 1)
}

fn ndcg(
    ordered: &[Identity],
    required: &BTreeSet<Identity>,
    useful: &BTreeSet<Identity>,
) -> Option<f64> {
    if required.is_empty() {
        return None;
    }
    let gain = |id: &Identity| {
        if required.contains(id) {
            3.0
        } else if useful.contains(id) {
            1.0
        } else {
            0.0
        }
    };
    let dcg: f64 = ordered
        .iter()
        .take(10)
        .enumerate()
        .map(|(rank, id)| gain(id) / ((rank + 2) as f64).log2())
        .sum();
    let ideal: f64 = (0..required.len() + useful.difference(required).count())
        .take(10)
        .map(|rank| (if rank < required.len() { 3.0 } else { 1.0 }) / ((rank + 2) as f64).log2())
        .sum();
    Some(dcg / ideal)
}

pub(super) fn score(pack: &Value, case: &Case) -> Score {
    let ordered = delivered(pack);
    let observed: BTreeSet<_> = ordered.iter().cloned().collect();
    let required: BTreeSet<_> = case
        .required
        .iter()
        .map(|gold| gold.identity.clone())
        .collect();
    let useful: BTreeSet<_> = case.useful.iter().cloned().collect();
    let relevant: BTreeSet<_> = required.union(&useful).cloned().collect();
    let hits = observed.intersection(&required).count();
    let unique_gold: Vec<_> = required
        .iter()
        .map(|id| case.required.iter().find(|g| &g.identity == id).unwrap())
        .collect();
    let sha_hits = unique_gold
        .iter()
        .filter(|gold| current_file(pack, case, gold).is_some())
        .count();
    let body_hits = unique_gold
        .iter()
        .filter(|gold| complete_body(pack, case, gold))
        .count();
    let patch_hits = unique_gold
        .iter()
        .filter(|gold| unique_patch_precondition(pack, case, gold))
        .count();
    let bytes = serde_json::to_vec(pack).unwrap().len();
    let budget = pack["budget"].as_u64().map(|n| n as usize);
    let gold_context_sufficient = !required.is_empty()
        && hits == required.len()
        && sha_hits == required.len()
        && body_hits == required.len()
        && case.writable;
    let all_edit_inputs = gold_context_sufficient
        && pack["project"]["write_enabled"].as_bool() == Some(true)
        && unique_gold.iter().all(|gold| {
            current_file(pack, case, gold)
                .is_some_and(|file| file["readonly"].as_bool() == Some(false))
        });
    let exposed_batch = serde_json::from_value::<DecisionBatch>(
        pack.get("decision_plane").cloned().unwrap_or(Value::Null),
    )
    .ok();
    let decision_plane_exposed = exposed_batch.is_some();
    let shadow_batch = agent_context_decisions(pack, &case.query);
    let context_sufficient_milli = probability_milli(&shadow_batch, "context_sufficient");
    let context_sufficient_brier_million = context_sufficient_milli
        .map(|probability| calibration_brier_million(probability, gold_context_sufficient));
    Score {
        delivery: super::delivery::inspect(pack, case),
        required_count: required.len(),
        required_hits: hits,
        required_recall: ratio(hits, required.len()),
        recall_at_5: ratio(
            ordered
                .iter()
                .take(5)
                .filter(|id| required.contains(*id))
                .count(),
            required.len(),
        ),
        recall_at_10: ratio(
            ordered
                .iter()
                .take(10)
                .filter(|id| required.contains(*id))
                .count(),
            required.len(),
        ),
        first_required_rank: ordered
            .iter()
            .position(|id| required.contains(id))
            .map(|rank| rank + 1),
        ndcg_at_10: ndcg(&ordered, &required, &useful),
        delivered_identities: ordered.len(),
        non_gold_symbol_fraction: ratio(observed.difference(&relevant).count(), observed.len()),
        non_gold_identities: observed.difference(&relevant).cloned().collect(),
        fresh_sha_hits: sha_hits,
        complete_body_hits: body_hits,
        complete_body_recall: ratio(body_hits, required.len()),
        all_required_edit_inputs: all_edit_inputs,
        gold_context_sufficient,
        unique_patch_precondition_hits: patch_hits,
        all_required_unique_patch_preconditions: all_edit_inputs && patch_hits == required.len(),
        reported_edit_ready: pack["readiness"]["edit"].as_str() == Some("ready"),
        decision_plane_exposed,
        context_sufficient_milli,
        context_sufficient_brier_million,
        abstained: case.no_answer.then_some(observed.is_empty()),
        response_bytes: bytes,
        estimated_tokens: bytes.div_ceil(4),
        budget_tokens: budget,
        within_budget: budget.is_some_and(|limit| bytes.div_ceil(4) <= limit),
        truncated: pack["truncated"].as_bool().unwrap_or(false),
    }
}
