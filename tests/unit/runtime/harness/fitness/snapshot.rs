//! Export only typed counters and provenance; full source-bearing reports stay separate.
use super::Report;
use crate::intelligence::{FitnessBenchmarkRow, FitnessBenchmarkSnapshot};
use crate::workspace::Workspace;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};

fn project_row(row: &Value) -> Result<FitnessBenchmarkRow> {
    let required = row["required_gold_count"]
        .as_u64()
        .context("missing required count")?;
    let missing_sha = row["missing_current_sha_count"]
        .as_u64()
        .context("missing SHA count")?;
    let fresh_sha = required
        .checked_sub(missing_sha)
        .context("invalid missing SHA count")?;
    Ok(serde_json::from_value(json!({
        "budget": row["budget"], "phase": row["phase"], "attempts": row["attempts"],
        "query_errors": row["errors"], "warmup_errors": row["warmup_errors"], "over_budget": row["over_budget"],
        "required_count": required, "required_hits": row["required_hits"],
        "complete_body_hits": row["complete_body_hits"], "fresh_sha_hits": fresh_sha,
        "edit_input_eligible": row["edit_input_eligible"], "edit_input_ready": row["all_required_edit_inputs_count"],
        "ranking_attempts": row["ranking_attempts"], "mean_ndcg_at_10": row["mean_ndcg_at_10"],
        "noise_samples": row["noise_response_samples"], "mean_non_gold_fraction": row["mean_non_gold_symbol_fraction"],
        "complete_gold_bytes": row["complete_gold_source_bytes"], "density_response_bytes": row["density_response_bytes"],
        "latency_samples": row["latency_across_tasks"]["samples"],
        "p50_us": row["latency_across_tasks"]["p50_us"], "p95_us": row["latency_across_tasks"]["p95_us"]
    }))?)
}

pub(super) fn persist_summary(report: &Report, workspace: &Workspace, stamp: u128) -> Result<()> {
    let metadata = &report.metadata;
    let rows = report
        .summary
        .iter()
        .map(project_row)
        .collect::<Result<Vec<_>>>()?;
    let captured_at_ms = u64::try_from(stamp / 1_000_000).context("invalid report timestamp")?;
    let snapshot: FitnessBenchmarkSnapshot = serde_json::from_value(json!({
        "schema_version": 1, "contract_version": metadata["contract_version"], "captured_at_ms": captured_at_ms,
        "wcode_version": metadata["wcode_version"], "profile": metadata["profile"], "os": metadata["os"], "arch": metadata["arch"],
        "case_count": metadata["case_count"], "samples_per_case": metadata["samples_per_case_budget_phase"],
        "model_calls": metadata["model_calls"], "harness_slots": metadata["harness_slots"],
        "corpus_sha256": metadata["corpus_sha256"], "evaluator_sha256": metadata["evaluator_sha256"],
        "test_binary_sha256": metadata["test_binary_sha256"],
        "revision_before": metadata["repository_revision_before"], "revision_after": metadata["repository_revision_after"],
        "source_snapshot_before": metadata["source_snapshot_before"], "source_snapshot_after": metadata["source_snapshot_after"],
        "source_stable_during_run": metadata["source_stable_during_run"],
        "controls_passed": report.controls.iter().filter(|control| control.passed).count(),
        "controls_total": report.controls.len(), "rows": rows,
    }))?;
    snapshot.validate(captured_at_ms)?;
    let content = serde_json::to_string(&snapshot)?;
    ensure!(
        content.len() <= 64 * 1024,
        "Fitness summary exceeds the reader bound"
    );
    workspace.create_directory("target/engineering-fitness")?;
    let path = format!(
        "target/engineering-fitness/{stamp}-{}.json",
        std::process::id()
    );
    workspace.create_file(&path, &content)?;
    println!(
        "FITNESS_OBSERVATORY_SUMMARY {}",
        json!({"path":path,"schema_version":1,"bytes":content.len()})
    );
    Ok(())
}

#[test]
fn fitness_summary_projection_preserves_failures_denominators_and_excludes_unmeasured_source() {
    let mut input = json!({
        "budget":1000,"phase":"cold","attempts":60,"errors":2,"warmup_errors":1,"over_budget":1,
        "required_gold_count":98,"required_hits":92,"complete_body_hits":90,"missing_current_sha_count":8,
        "edit_input_eligible":58,"all_required_edit_inputs_count":50,"ranking_attempts":59,"mean_ndcg_at_10":0.8,
        "noise_response_samples":58,"mean_non_gold_symbol_fraction":0.03,
        "complete_gold_source_bytes":100,"density_response_bytes":1000,
        "latency_across_tasks":{"samples":60,"p50_us":12.5,"p95_us":30},
        "source_body":"must not be copied","query":"must not be copied","response":"must not be copied"
    });
    let row = project_row(&input).unwrap();
    assert_eq!(
        (
            row.attempts,
            row.query_errors,
            row.warmup_errors,
            row.over_budget
        ),
        (60, 2, 1, 1)
    );
    assert_eq!(row.fresh_sha_hits, 90);
    assert_eq!(row.edit_input_eligible, 58);
    assert_eq!(row.latency_samples, 60);
    let encoded = serde_json::to_string(&row).unwrap();
    assert!(!encoded.contains("must not be copied"));
    assert!(!encoded.contains("source_body"));
    input["missing_current_sha_count"] = json!(99);
    assert!(project_row(&input).is_err());
}
