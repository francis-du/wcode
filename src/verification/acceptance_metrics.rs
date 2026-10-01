//! Local pilot metrics over retained Records; never telemetry or current gate authority.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn summarize(history: &Value) -> Result<Value> {
    ensure!(
        history["authority"] == "historical_only" && history["current_acceptance"] == false,
        "pilot metrics require historical-only native history"
    );
    let records = history["records"]
        .as_array()
        .context("Acceptance history has no records")?;
    ensure!(
        records.len() <= 256,
        "Acceptance metrics exceed retained history bound"
    );
    let mut states = BTreeMap::<String, usize>::new();
    let mut revisions = BTreeSet::new();
    let mut missing = 0usize;
    let mut stale = 0usize;
    let mut reviews = 0usize;
    let mut approved = 0usize;
    let mut first = None::<u64>;
    let mut last = None::<u64>;
    let mut transitions = BTreeMap::<String, Vec<(u64, bool)>>::new();
    for record in records {
        ensure!(
            record["schema_version"] == 1 && record["producer"] == "wcode/native-acceptance/v1",
            "unsupported Acceptance metric record"
        );
        let at = record["captured_at_ms"]
            .as_u64()
            .filter(|time| *time > 0)
            .context("Acceptance record timestamp is unavailable")?;
        let state = record["state"]
            .as_str()
            .filter(|state| {
                ["ready", "blocked", "needs_review", "incomplete", "stale"].contains(state)
            })
            .context("invalid Acceptance state")?;
        *states.entry(state.to_owned()).or_default() += 1;
        first = Some(first.map_or(at, |previous| previous.min(at)));
        last = Some(last.map_or(at, |previous| previous.max(at)));
        let identity = serde_json::to_string(&json!([
            record["workspace"],
            record["revision"],
            record["git"]["base_sha"],
            record["git"]["target_sha"],
            record["git"]["binding"],
            record["policy"]
        ]))?;
        revisions.insert(identity.clone());
        transitions
            .entry(identity)
            .or_default()
            .push((at, state == "ready"));
        missing += usize::from(record["checks"].as_array().is_some_and(|checks| {
            checks
                .iter()
                .any(|check| check["required"] == true && check["execution"] != "executed")
        }));
        stale += usize::from(
            record["evidence"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["freshness"] == "stale"))
                || record["summary"]["stale"].as_u64().unwrap_or(0) > 0,
        );
        reviews += usize::from(record["actions"].as_array().is_some_and(|actions| {
            actions
                .iter()
                .any(|action| action == "request_human_approval")
        }));
        approved += usize::from(record["verification"]["human_approval"] == "pass");
    }
    let mut observed_transition_ms = Vec::new();
    for values in transitions.values_mut() {
        values.sort_unstable();
        if let Some((start, false)) = values.first().copied() {
            if let Some((end, true)) = values.iter().copied().find(|(_, ready)| *ready) {
                observed_transition_ms.push(end - start);
            }
        }
    }
    let verification_duration = super::acceptance::execution_duration_metrics(records.iter())?;
    Ok(json!({
        "schema_version":1,"authority":"historical_metrics_only","current_acceptance":false,
        "source":"retained_native_acceptance_records","uploads":false,
        "retained_records":records.len(),"capacity":history["capacity"],"distinct_revision_policy_bindings":revisions.len(),
        "window":{"first_capture_ms":first,"last_capture_ms":last,"bounded":true,"may_be_left_censored":true},
        "states":states,"records_with_missing_required_execution":missing,"records_with_stale_evidence":stale,
        "records_requesting_human_review":reviews,"records_with_human_approval":approved,
        "observed_not_ready_to_ready_ms":observed_transition_ms,
        "verification_duration":verification_duration,
        "exceptions":{"available":false,"reason":"exception_product_not_implemented"},
        "counting":"Record observations, not distinct changes or causal savings. Transition samples start at the first retained non-ready observation of the same revision and Policy; not full acceptance lead time."
    }))
}

#[cfg(test)]
#[path = "../../tests/unit/verification/acceptance_metrics.rs"]
mod tests;
