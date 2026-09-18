//! Additive diagnostics; never changes the Contract v3 identity/body scorer.
use super::corpus::{Case, Gold, Identity};
use super::scoring::{complete_body, delivered, digest, identity};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize)]
pub(super) struct Delivery {
    pub missing_identities: Vec<Identity>,
    pub identified_without_complete_body: Vec<Identity>,
    pub absent_bodies: Vec<Identity>,
    pub partial_original_bodies: Vec<Identity>,
    pub unusable_bodies: Vec<Identity>,
    pub missing_current_sha: Vec<Identity>,
    pub unavailable_write_inputs: Vec<Identity>,
    pub required_source_bytes: Option<usize>,
    pub complete_gold_source_bytes: Option<usize>,
    pub complete_gold_density: Option<f64>,
}

fn span(case: &Case, gold: &Gold) -> Option<(usize, usize)> {
    let text = case.files.get(&gold.identity.path)?;
    let fragment = gold.fragment.trim_end_matches(['\r', '\n']);
    if fragment.is_empty() {
        return None;
    }
    let mut matches = text.match_indices(fragment);
    let start = matches.next()?.0;
    if matches.next().is_some() {
        return None;
    }
    Some((start, start + fragment.len()))
}

// Count physical UTF-8 bytes once per path, including overlapping/nested Gold.
fn union_bytes(mut ranges: BTreeMap<&str, Vec<(usize, usize)>>) -> usize {
    let mut total = 0;
    for spans in ranges.values_mut() {
        spans.sort_unstable();
        let mut end = 0;
        for &(start, stop) in spans.iter() {
            total += stop.saturating_sub(start.max(end));
            end = end.max(stop);
        }
    }
    total
}

// Partial credit is diagnostic only. It requires original bytes overlapping
// the authored fragment, current SHA, exact line bounds and no redaction.
fn original_overlap(item: &Value, case: &Case, gold: &Gold) -> bool {
    let Some(original) = case.files.get(&gold.identity.path) else {
        return false;
    };
    let Some((gold_start, gold_end)) = span(case, gold) else {
        return false;
    };
    if item["sha256"].as_str() != Some(digest(original.as_bytes()).as_str())
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
    let Ok(start_index) = usize::try_from(start) else {
        return false;
    };
    let offset = if start_index == 1 {
        Some(0)
    } else {
        original
            .match_indices('\n')
            .nth(start_index - 2)
            .map(|(offset, _)| offset + 1)
    };
    let Some(offset) = offset else { return false };
    original[offset..].starts_with(content)
        && start.checked_add(content.lines().count() as u64 - 1) == Some(end)
        && offset < gold_end
        && offset.saturating_add(content.len()) > gold_start
}

pub(super) fn inspect(pack: &Value, case: &Case) -> Delivery {
    let observed: BTreeSet<_> = delivered(pack).into_iter().collect();
    let unique: BTreeMap<_, _> = case.required.iter().map(|g| (&g.identity, g)).collect();
    let mut missing_identities = Vec::new();
    let mut identified_without_complete_body = Vec::new();
    let mut absent_bodies = Vec::new();
    let mut partial_original_bodies = Vec::new();
    let mut unusable_bodies = Vec::new();
    let mut missing_current_sha = Vec::new();
    let mut unavailable_write_inputs = Vec::new();
    let mut required_ranges = BTreeMap::<&str, Vec<(usize, usize)>>::new();
    let mut complete_ranges = BTreeMap::<&str, Vec<(usize, usize)>>::new();
    let mut valid_gold = !unique.is_empty();
    for (&identity, &gold) in &unique {
        let complete = complete_body(pack, case, gold);
        if !observed.contains(identity) {
            missing_identities.push(identity.clone());
        } else if !complete {
            identified_without_complete_body.push(identity.clone());
        }
        if !complete {
            let matching: Vec<_> = pack["hot_source"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|item| self::identity(item).as_ref() == Some(identity))
                .collect();
            if matching.is_empty() {
                absent_bodies.push(identity.clone());
            } else if matching
                .iter()
                .any(|item| original_overlap(item, case, gold))
            {
                partial_original_bodies.push(identity.clone());
            } else {
                unusable_bodies.push(identity.clone());
            }
        }
        let sha = case
            .files
            .get(&identity.path)
            .map(|text| digest(text.as_bytes()));
        let current = pack["files"].as_array().into_iter().flatten().find(|file| {
            sha.as_deref().is_some_and(|sha| {
                file["path"] == identity.path && file["sha256"].as_str() == Some(sha)
            })
        });
        if current.is_none() {
            missing_current_sha.push(identity.clone());
        }
        if !case.writable
            || pack["project"]["write_enabled"].as_bool() != Some(true)
            || !current.is_some_and(|file| file["readonly"].as_bool() == Some(false))
        {
            unavailable_write_inputs.push(identity.clone());
        }
        if let Some(range) = span(case, gold) {
            required_ranges
                .entry(&identity.path)
                .or_default()
                .push(range);
            if complete {
                complete_ranges
                    .entry(&identity.path)
                    .or_default()
                    .push(range);
            }
        } else {
            valid_gold = false;
        }
    }
    // Conflicting duplicate annotations must not silently produce a byte bound.
    valid_gold &= case.required.iter().all(|gold| {
        unique
            .get(&gold.identity)
            .is_some_and(|other| other.fragment == gold.fragment)
    });
    let required_source_bytes = valid_gold.then(|| union_bytes(required_ranges));
    let complete_gold_source_bytes = valid_gold.then(|| union_bytes(complete_ranges));
    let bytes = serde_json::to_vec(pack)
        .expect("JSON Value is serializable")
        .len();
    Delivery {
        missing_identities,
        identified_without_complete_body,
        absent_bodies,
        partial_original_bodies,
        unusable_bodies,
        missing_current_sha,
        unavailable_write_inputs,
        required_source_bytes,
        complete_gold_source_bytes,
        complete_gold_density: complete_gold_source_bytes.map(|gold| gold as f64 / bytes as f64),
    }
}
