//! Historical timing observations shared by OSS and Team metrics.
//! Input must come from the caller's trusted, scoped retained records. Parsing
//! this projection does not authenticate JSON, mint Evidence or authorize a gate.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceCheckTiming {
    pub source: String,
    pub check_id: String,
    pub signature: String,
    pub level: String,
    pub phase: u8,
    /// Native process execution time, not queue delay or full-run wall time.
    pub execution_ms: u64,
}

fn text(value: &Value, bound: usize) -> Option<&str> {
    value.as_str().filter(|value| {
        !value.is_empty() && value.len() <= bound && !value.chars().any(char::is_control)
    })
}

fn observed_timing(record: &Value, evidence: &Value) -> Option<AcceptanceCheckTiming> {
    if evidence["authority"] != "native_verification"
        || evidence["confidence"] != "deterministic"
        || evidence["verification_relation"] != "native_check"
        || evidence["freshness"] != "current"
        || evidence["revision"] != record["revision"]
        || !record["revision"].is_object()
    {
        return None;
    }
    let captured = record["captured_at_ms"].as_u64().filter(|at| *at > 0)?;
    evidence["timestamp_ms"]
        .as_u64()
        .filter(|at| *at > 0 && *at <= captured)?;
    let generation = record["policy"]["generation"].as_u64().filter(|g| *g > 0)?;
    let snapshot = text(&record["policy"]["snapshot_digest"], 256)?;
    let policy = format!("project-policy/v1/{generation}/{snapshot}");
    if evidence["execution_policy_binding"].as_str() != Some(policy.as_str()) {
        return None;
    }
    let raw = evidence["execution_timing"].as_object()?;
    if raw.len() != 6
        || text(&evidence["execution_timing"]["source"], 64).is_none()
        || text(&evidence["execution_timing"]["check_id"], 160).is_none()
        || text(&evidence["execution_timing"]["signature"], 256).is_none()
        || text(&evidence["execution_timing"]["level"], 8).is_none()
    {
        return None;
    }
    let timing: AcceptanceCheckTiming =
        serde_json::from_value(evidence["execution_timing"].clone()).ok()?;
    if timing.source != "native_check_execution/v1"
        || !matches!(timing.level.as_str(), "quick" | "full")
        || timing.check_id.is_empty()
        || timing.check_id.len() > 160
        || timing.signature.is_empty()
        || timing.signature.len() > 256
    {
        return None;
    }
    let checks = record["checks"].as_array()?;
    if checks.len() > 64 {
        return None;
    }
    let matching = checks
        .iter()
        .filter(|check| check["id"] == timing.check_id)
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return None;
    }
    let check = matching[0];
    let ids = check["evidence_ids"].as_array()?;
    if check["signature"] != timing.signature
        || ids.len() > 4096
        || !ids.iter().any(|id| id == &evidence["id"])
    {
        return None;
    }
    Some(timing)
}

/// Summarize unique native per-check durations across at most 256 historical
/// Records. Recapture, cache reuse and repeated registration never create a new
/// timing sample for the same workspace/repository/Evidence ID. Conflicting or
/// malformed timing copies are excluded, not resolved by last-write-wins.
/// The caller supplies ACL/time-window selection; no filesystem/network access
/// occurs here. Neither percentile values nor sample counts are Acceptance.
pub fn execution_duration_metrics<'a>(
    records: impl IntoIterator<Item = &'a Value>,
) -> Result<Value> {
    type Key = (String, String, String);
    let mut samples = BTreeMap::<Key, (String, u64)>::new();
    let mut missing = BTreeSet::<Key>::new();
    let mut excluded = BTreeSet::<Key>::new();
    let mut conflicts = BTreeSet::<Key>::new();
    let mut seen = BTreeSet::<Key>::new();
    for (index, record) in records.into_iter().enumerate() {
        ensure!(index < 256, "timing record count exceeds bound");
        ensure!(
            record["schema_version"] == 1 && record["producer"] == "wcode/native-acceptance/v1",
            "timing record schema is unsupported"
        );
        let Some(items) = record.get("evidence").filter(|value| !value.is_null()) else {
            continue;
        };
        let items = items
            .as_array()
            .context("timing Evidence inventory is invalid")?;
        ensure!(
            items.len() <= 4096,
            "timing Evidence inventory exceeds bound"
        );
        for item in items {
            let has_timing = item
                .get("execution_timing")
                .is_some_and(|value| !value.is_null());
            if !has_timing
                && (item["authority"] != "native_verification" || item["freshness"] != "current")
            {
                continue;
            }
            let key = (
                text(&record["workspace"], 256)
                    .context("timing workspace identity missing")?
                    .to_owned(),
                text(&record["git"]["binding"]["repository"], 256)
                    .context("timing repository identity missing")?
                    .to_owned(),
                text(&item["id"], 160)
                    .context("timing Evidence identity missing")?
                    .to_owned(),
            );
            seen.insert(key.clone());
            ensure!(
                seen.len() <= 16_384,
                "timing distinct Evidence count exceeds bound"
            );
            if !has_timing {
                missing.insert(key);
                continue;
            }
            let Some(timing) = observed_timing(record, item) else {
                excluded.insert(key);
                continue;
            };
            // Binding and actual measurement are part of duplicate identity;
            // observation timestamp/state of the containing Record are not.
            let signature = serde_json::to_string(&json!([
                item["revision"],
                record["git"]["binding"],
                item["execution_policy_binding"],
                item["timestamp_ms"],
                timing
            ]))?;
            if let Some((previous, _)) = samples.get(&key) {
                if previous != &signature {
                    conflicts.insert(key);
                }
            } else {
                samples.insert(key, (signature, timing.execution_ms));
            }
        }
    }
    let mut durations = samples
        .iter()
        .filter(|(key, _)| !conflicts.contains(*key) && !excluded.contains(*key))
        .map(|(_, (_, duration))| *duration)
        .collect::<Vec<_>>();
    durations.sort_unstable();
    let percentile = |percent: usize| -> Option<u64> {
        (!durations.is_empty()).then(|| durations[(durations.len() * percent).div_ceil(100) - 1])
    };
    let missing = missing
        .iter()
        .filter(|key| !samples.contains_key(*key) && !excluded.contains(*key))
        .count();
    Ok(json!({
        "available":!durations.is_empty(),"historical_only":true,
        "measurement":"per_check_execution_ms_not_run_wall_time",
        "clock":"native_monotonic_execution_excludes_process_queue",
        "sample_count":durations.len(),"samples_ms":durations,
        "min_ms":durations.first(),"p50_ms":percentile(50),"p95_ms":percentile(95),"max_ms":durations.last(),
        "percentile_method":"nearest_rank","conflicting_evidence":conflicts.len(),
        "excluded_evidence":excluded.len(),"native_evidence_without_timing":missing,
        "coverage":"unique_measured_evidence_in_selected_records_not_all_execution",
        "reason":if durations.is_empty() { Some("no_usable_native_check_timings") } else { None }
    }))
}
