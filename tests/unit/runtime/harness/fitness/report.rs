use super::controls::{report_workspace, run_controls, Control};
use super::corpus::{corpus, Case};
use super::scoring::{digest, score, Score};
use crate::decision::{
    probability_calibration_summary, ProbabilityCalibrationSample,
    CONTEXT_SUFFICIENT_STOP_THRESHOLD_MILLI,
};
use crate::harness::ToolHarness;
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::Command,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[path = "breakdown.rs"]
mod breakdown;
#[path = "report_checks.rs"]
mod report_checks;

const BUDGETS: [usize; 3] = [1_000, 2_000, 4_000];
const CONTRACT_VERSION: u32 = 3;

#[derive(Debug, Serialize)]
pub(super) struct Sample {
    pub elapsed_us: u64,
    pub score: Option<Score>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct Row {
    pub case_id: String,
    pub language: String,
    pub category: String,
    pub fixture_sha256: String,
    pub fixture_files: usize,
    pub required_count: usize,
    pub writable: bool,
    pub no_answer: bool,
    pub budget: usize,
    pub phase: String,
    pub warmup_us: Option<u64>,
    pub warmup_error: Option<String>,
    pub samples: Vec<Sample>,
    pub latency: Value,
}

#[derive(Debug, Serialize)]
pub(super) struct Report {
    pub schema_version: u32,
    pub metadata: Value,
    pub summary: Vec<Value>,
    pub breakdown: Value,
    pub controls: Vec<Control>,
    pub rows: Vec<Row>,
}

fn elapsed_us(started: Instant) -> u64 {
    started.elapsed().as_micros().min(u64::MAX as u128) as u64
}

pub(super) fn distribution(values: &[u64]) -> Value {
    if values.is_empty() {
        return json!({"samples":0,"p50_us":null,"p95_us":null});
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let median = if sorted.len().is_multiple_of(2) {
        sorted[sorted.len() / 2 - 1] as f64 / 2.0 + sorted[sorted.len() / 2] as f64 / 2.0
    } else {
        sorted[sorted.len() / 2] as f64
    };
    json!({"samples":sorted.len(),"p50_us":median,
        "p95_us":sorted[(sorted.len() * 95).div_ceil(100) - 1],
        "min_us":sorted[0],"max_us":sorted[sorted.len()-1],
        "p95_is_max":(sorted.len() * 95).div_ceil(100) == sorted.len()})
}

fn score_for_request(pack: &Value, case: &Case, budget: usize) -> Score {
    let mut result = score(pack, case);
    result.within_budget = pack["budget"].as_u64() == Some(budget as u64)
        && result.response_bytes.div_ceil(4) <= budget;
    result
}

fn measure(
    harness: &ToolHarness,
    workspace: &crate::workspace::Workspace,
    case: &Case,
    budget: usize,
) -> Sample {
    let started = Instant::now();
    let result = harness.agent_context("fitness", workspace, &case.query, budget, &[]);
    let elapsed_us = elapsed_us(started);
    // Grading and report serialization are deliberately outside query timing.
    match result {
        Ok(pack) => Sample {
            elapsed_us,
            score: Some(score_for_request(&pack, case, budget)),
            error: None,
        },
        Err(error) => Sample {
            elapsed_us,
            score: None,
            error: Some(format!("{error:#}")),
        },
    }
}

fn row(case: &Case, budget: usize, phase: &str, repeats: usize) -> Result<Row> {
    let (_root, workspace) = case.instantiate();
    let warm = ToolHarness::new(4)?;
    let (warmup_us, warmup_error) = if phase == "warm" {
        let started = Instant::now();
        let result = warm.agent_context("fitness", &workspace, &case.query, budget, &[]);
        (
            Some(elapsed_us(started)),
            result.err().map(|error| format!("{error:#}")),
        )
    } else {
        (None, None)
    };
    let mut samples = Vec::new();
    for _ in 0..repeats {
        if phase == "cold" {
            let cold = ToolHarness::new(4)?;
            samples.push(measure(&cold, &workspace, case, budget));
        } else {
            samples.push(measure(&warm, &workspace, case, budget));
        }
    }
    let latency = distribution(
        &samples
            .iter()
            .map(|sample| sample.elapsed_us)
            .collect::<Vec<_>>(),
    );
    Ok(Row {
        case_id: case.id.clone(),
        language: case.language.clone(),
        category: case.category.clone(),
        fixture_sha256: digest(&serde_json::to_vec(&case.files)?),
        fixture_files: case.files.len(),
        required_count: case.required.len(),
        writable: case.writable,
        no_answer: case.no_answer,
        budget,
        phase: phase.into(),
        warmup_us,
        warmup_error,
        samples,
        latency,
    })
}

fn ratio(numerator: usize, denominator: usize) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 / denominator as f64)
}

fn extend_json_object(target: &mut Value, fields: Value) {
    let fields = fields
        .as_object()
        .expect("fitness summary fields must be an object");
    target
        .as_object_mut()
        .expect("fitness summary must be an object")
        .extend(fields.clone());
}

pub(super) fn summarize(rows: &[Row]) -> Vec<Value> {
    summarize_selected(rows.iter())
}

fn summarize_selected<'a>(rows: impl IntoIterator<Item = &'a Row>) -> Vec<Value> {
    let mut groups: BTreeMap<(usize, &str), Vec<&Row>> = BTreeMap::new();
    for row in rows {
        groups
            .entry((row.budget, &row.phase))
            .or_default()
            .push(row);
    }
    groups.into_iter().map(|((budget, phase), rows)| {
        let (mut required, mut hits, mut bodies, mut errors, mut count) = (0, 0, 0, 0, 0);
        let (mut editable, mut ready, mut patch_ready, mut bytes, mut received, mut over_budget) =
            (0, 0, 0, 0, 0, 0);
        let (mut no_answer_count, mut abstained) = (0, 0);
        let (mut noise_sum, mut noise_samples, mut ndcg_sum, mut ndcg_samples) = (0.0, 0, 0.0, 0);
        let (mut bug_required, mut bug_hits, mut bug_bodies) = (0, 0, 0);
        let (mut bug_noise_sum, mut bug_noise_samples) = (0.0, 0);
        let (mut sha_hits, mut body_gaps, mut write_gaps) = (0, 0, 0);
        let (mut absent_bodies, mut partial_bodies, mut unusable_bodies, mut unobserved_bodies) = (0, 0, 0, 0);
        let (mut gold_bytes, mut density_bytes, mut density_samples) = (0, 0, 0);
        let (mut raw_bound_samples, mut raw_bound_exceeds_budget) = (0, 0);
        let mut decision_samples = Vec::new();
        let mut decision_exposed = 0_usize;
        let mut times = Vec::new();
        let mut returned_tokens = Vec::new();
        for row in &rows {
            for sample in &row.samples {
                count += 1;
                required += row.required_count;
                editable += usize::from(row.writable && row.required_count > 0);
                no_answer_count += usize::from(row.no_answer);
                // Ranking is defined for every answerable attempt, including errors.
                ndcg_samples += usize::from(row.required_count > 0);
                if row.category == "bug-relevant-evidence" {
                    bug_required += row.required_count;
                }
                times.push(sample.elapsed_us);
                match &sample.score {
                    Some(score) => {
                        hits += score.required_hits;
                        bodies += score.complete_body_hits;
                        sha_hits += score.fresh_sha_hits;
                        body_gaps += score.delivery.identified_without_complete_body.len();
                        absent_bodies += score.delivery.absent_bodies.len();
                        partial_bodies += score.delivery.partial_original_bodies.len();
                        unusable_bodies += score.delivery.unusable_bodies.len();
                        write_gaps += score.delivery.unavailable_write_inputs.len();
                        if let Some(value) = score.delivery.complete_gold_source_bytes {
                            gold_bytes += value;
                            density_bytes += score.response_bytes;
                            density_samples += 1;
                        }
                        if let Some(value) = score.delivery.required_source_bytes {
                            raw_bound_samples += 1;
                            raw_bound_exceeds_budget += usize::from(value > budget.saturating_mul(4));
                        }
                        ready += usize::from(score.all_required_edit_inputs);
                        patch_ready += usize::from(score.all_required_unique_patch_preconditions);
                        bytes += score.response_bytes;
                        received += 1;
                        over_budget += usize::from(!score.within_budget);
                        abstained += usize::from(score.abstained == Some(true));
                        returned_tokens.push(score.estimated_tokens as u64);
                        decision_exposed += usize::from(score.decision_plane_exposed);
                        if let Some(probability) = score.context_sufficient_milli {
                            decision_samples.push(ProbabilityCalibrationSample::new(
                                probability,
                                score.gold_context_sufficient,
                            ));
                        }
                        if let Some(noise) = score.non_gold_symbol_fraction {
                            noise_sum += noise;
                            noise_samples += 1;
                        }
                        if let Some(ndcg) = score.ndcg_at_10 {
                            ndcg_sum += ndcg;
                        }
                        if row.category == "bug-relevant-evidence" {
                            bug_hits += score.required_hits;
                            bug_bodies += score.complete_body_hits;
                            if let Some(noise) = score.non_gold_symbol_fraction {
                                bug_noise_sum += noise;
                                bug_noise_samples += 1;
                            }
                        }
                    }
                    None => { errors += 1; unobserved_bodies += row.required_count; },
                }
            }
        }
        let decision_calibration = probability_calibration_summary(
            &decision_samples,
            CONTEXT_SUFFICIENT_STOP_THRESHOLD_MILLI,
        );
        let mut summary = json!({"budget":budget,"phase":phase,"cases":rows.len(),"attempts":count,"errors":errors,
            "warmup_errors":rows.iter().filter(|row|row.warmup_error.is_some()).count(),
            "required_gold_count":required,"required_hits":hits,"complete_body_hits":bodies,
            "required_recall":ratio(hits,required),"complete_body_recall":ratio(bodies,required)
        });
        extend_json_object(&mut summary, json!({
            "fresh_sha_recall":ratio(sha_hits,required),"missing_identity_count":required.saturating_sub(hits),
            "identified_without_complete_body_count":body_gaps,"missing_current_sha_count":required.saturating_sub(sha_hits),
            "unavailable_write_input_count":write_gaps,
            "complete_gold_source_bytes":gold_bytes,"complete_gold_density":ratio(gold_bytes,density_bytes),
            "density_response_bytes":density_bytes,"density_response_samples":density_samples,
            "complete_bodies_per_1k_budget_tokens":ratio(bodies.saturating_mul(1000),ndcg_samples.saturating_mul(budget)),
            "raw_bound_samples":raw_bound_samples,"raw_source_exceeds_budget_samples":raw_bound_exceeds_budget
        }));
        extend_json_object(&mut summary, json!({
            "mean_non_gold_symbol_fraction":(noise_samples > 0).then(|| noise_sum / noise_samples as f64),
            "noise_response_samples":noise_samples,"ranking_attempts":ndcg_samples,
            "mean_ndcg_at_10":(ndcg_samples > 0).then(|| ndcg_sum / ndcg_samples as f64),
            "bug_relevant_required_recall":ratio(bug_hits,bug_required),
            "bug_relevant_complete_body_recall":ratio(bug_bodies,bug_required),
            "bug_relevant_mean_non_gold_symbol_fraction":
                (bug_noise_samples > 0).then(|| bug_noise_sum / bug_noise_samples as f64),
            "bug_relevant_symbol_precision":
                (bug_noise_samples > 0).then(|| 1.0 - bug_noise_sum / bug_noise_samples as f64),
            "edit_input_eligible":editable,"all_required_edit_inputs_count":ready,
            "all_required_edit_inputs_rate":ratio(ready,editable),
            "all_required_unique_patch_preconditions_count":patch_ready,
            "all_required_unique_patch_preconditions_rate":ratio(patch_ready,editable)
        }));
        extend_json_object(&mut summary, json!({
            "decision_context_observations":decision_calibration.samples,
            "decision_context_coverage":ratio(decision_calibration.samples,received),
            "decision_context_exposed_count":decision_exposed,
            "decision_context_exposed_coverage":ratio(decision_exposed,received),
            "decision_context_threshold_milli":decision_calibration.threshold_milli,
            "decision_context_mean_brier":decision_calibration
                .mean_brier_million
                .map(|value| f64::from(value) / 1_000_000.0),
            "decision_context_true_stop_count":decision_calibration.correct_stop,
            "decision_context_false_stop_count":decision_calibration.false_stop,
            "decision_context_true_continue_count":decision_calibration.correct_continue,
            "decision_context_false_continue_count":decision_calibration.false_continue,
            "no_answer_attempts":no_answer_count,"abstained":abstained,
            "over_budget":over_budget,"response_samples":received,
            "mean_response_bytes":ratio(bytes,received),
            "latency_across_tasks":distribution(&times),
            "median_estimated_tokens":distribution(&returned_tokens)["p50_us"]
        }));
        summary["absent_body_count"] = json!(absent_bodies);
        summary["partial_original_body_count"] = json!(partial_bodies);
        summary["unusable_body_count"] = json!(unusable_bodies);
        summary["unobserved_body_due_to_error_count"] = json!(unobserved_bodies);
        summary
    }).collect()
}

fn read_probe(program: &str, args: &[&str]) -> Option<String> {
    let result = Command::new(program)
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()?;
    result
        .status
        .success()
        .then(|| String::from_utf8_lossy(&result.stdout).trim().to_owned())
}

// Only declared build/evaluator inputs; runtime worklists and generated target reports are excluded.
fn source_snapshot() -> Result<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut paths = Vec::new();
    for directory in ["src", "tests", ".wcode/design"] {
        for entry in walkdir::WalkDir::new(root.join(directory)).follow_links(false) {
            let entry = entry?;
            if entry.file_type().is_file() {
                paths.push(entry.path().strip_prefix(root)?.to_owned());
            }
        }
    }
    for file in ["Cargo.toml", "Cargo.lock", ".wcode/project.yaml"] {
        if root.join(file).is_file() {
            paths.push(file.into());
        }
    }
    paths.sort();
    let mut manifest = BTreeMap::new();
    for path in paths {
        let bytes = fs::read(root.join(&path))?;
        manifest.insert(path, digest(&bytes));
    }
    Ok(digest(&serde_json::to_vec(&manifest)?))
}

pub(super) fn collect(repeats: usize) -> Result<Report> {
    anyhow::ensure!(repeats > 0, "at least one sample is required");
    let before = source_snapshot()?;
    let cases = corpus();
    let corpus_sha = digest(&serde_json::to_vec(&cases)?);
    let evaluator_sha = digest(
        concat!(
            include_str!("corpus.rs"),
            include_str!("scoring.rs"),
            include_str!("controls.rs"),
            include_str!("checks.rs"),
            include_str!("report.rs"),
            include_str!("challenges.rs"),
            include_str!("report_checks.rs"),
            include_str!("graph_checks.rs"),
            include_str!("packing.rs"),
            include_str!("delivery.rs"),
            include_str!("delivery_checks.rs"),
            include_str!("tight.rs"),
            include_str!("multitarget.rs"),
            include_str!("breakdown.rs")
        )
        .as_bytes(),
    );
    let mut rows = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        for budget in BUDGETS {
            // Alternate phase order to avoid always charging first-use effects to cold.
            let phases = if index % 2 == 0 {
                ["cold", "warm"]
            } else {
                ["warm", "cold"]
            };
            for phase in phases {
                rows.push(row(case, budget, phase, repeats)?);
            }
        }
    }
    let controls = run_controls();
    let after = source_snapshot()?;
    let binary_sha = std::env::current_exe()
        .ok()
        .and_then(|path| fs::read(path).ok())
        .map(|bytes| digest(&bytes));
    let mut metadata = json!({
        "name":"WCode Engineering Fitness","contract_version":CONTRACT_VERSION,
        "wcode_version":env!("CARGO_PKG_VERSION"),"model_calls":0,
        "profile":if cfg!(debug_assertions) {"debug"} else {"release"},
        "os":std::env::consts::OS,"arch":std::env::consts::ARCH,
        "available_parallelism":std::thread::available_parallelism().ok().map(|v|v.get()),
        "harness_slots":4,"rustc":read_probe("rustc", &["--version"]),
        "git_head":read_probe("git", &["rev-parse", "HEAD"]),
        "worktree_dirty":read_probe("git", &["status", "--porcelain", "--untracked-files=normal"]).map(|s|!s.is_empty()),
        "corpus_sha256":corpus_sha,"evaluator_sha256":evaluator_sha,"test_binary_sha256":binary_sha,
        "source_snapshot_before":before,"source_snapshot_after":after,"source_stable_during_run":before==after,
        "case_count":cases.len(),"samples_per_case_budget_phase":repeats,"budgets":BUDGETS,
        "completed_query_attempts":rows.iter().map(|row|row.samples.len()).sum::<usize>(),
        "query_errors":rows.iter().flat_map(|row|&row.samples).filter(|s|s.error.is_some()).count(),
        "warmup_errors":rows.iter().filter(|row|row.warmup_error.is_some()).count(),
        "completion_is_not_perfect_quality":true,
        "budget_contract":"response budget must match the requested experiment budget; serialized bytes/4 must fit it",
        "ranking_aggregation":"mean NDCG over all answerable attempts; failed queries contribute zero",
        "non_gold_scope":"mean over nonempty identity responses; unannotated useful evidence is not proven irrelevant",
        "cold_definition":"new ToolHarness; OS filesystem cache and process-global language config NOT cleared",
        "warm_definition":"same ToolHarness after one separately recorded identical-query warmup",
        "timing_scope":"agent_context only; excludes setup, Harness construction, grading, build and report writing",
        "token_measurement":"ceil(serialized JSON bytes / 4), not a model tokenizer or provider usage",
        "ranking_order":"deduplicated targets, repo_map.items, then hot_source; not raw candidate rank",
        "edit_inputs_scope":"all required identities, current SHA, full original body and writable fixture; no claim of mapped/executed verification",
        "not_measured":["live LSP accuracy","model understanding","bug detection","patch success","network MCP latency","CPU/RSS"],
        "caveats":["Synthetic development fixtures, not a hidden external holdout",
            "Four language translations share one core behavior family; not independent projects",
            "Whole-process first-use and concurrent machine load are not isolated",
            "Runtime source snapshots are not proof of compilation input identity; the test-binary hash binds the executed artifact",
            "v2 path+symbol/body scoring is not numerically comparable with the legacy v1 name-only scorer"]
    });
    metadata["delivery_metrics"] = json!({
        "version":2,
        "body_partition_scope":"required identities partition into complete, absent, verified partial-original, or unusable bodies per successful response; failed queries remain unobserved",
        "diagnostics_scope":"final delivered evidence only; a missing identity is not proof that internal retrieval missed it",
        "byte_density_scope":"union of original UTF-8 spans for complete verified required fragments / serialized response bytes; duplicates and nested overlaps counted once per path; partial fragments excluded",
        "budget_bound_scope":"raw required source byte union only: above budget*4 proves uncompressed source cannot fit, at or below is unknown, not proven feasible; failures remain in primary scores"
    });
    Ok(Report {
        schema_version: 2,
        metadata,
        summary: summarize(&rows),
        breakdown: breakdown::build(&rows),
        controls,
        rows,
    })
}

fn percent(value: &Value) -> String {
    value
        .as_f64()
        .filter(|n| n.is_finite())
        .map(|n| format!("{:.1}%", n * 100.0))
        .unwrap_or_else(|| "N/A".into())
}

pub(super) fn markdown(report: &Report) -> String {
    let mut out = format!(
        "# WCode Engineering Fitness\n\nContract: {CONTRACT_VERSION} · Profile: {} · Cases: {} · Model calls: 0\n\n",
        report.metadata["profile"], report.metadata["case_count"]
    );
    out.push_str("Synthetic development fixtures, not model coding scores. Cold does not clear OS caches. Tokens are bytes/4 estimates. Timings exclude setup and grading.\n\n");
    out.push_str("| Budget | Phase | Attempts | Gold recall | Complete source | Edit inputs | Non-Gold symbols | Bug evidence | Bug source | Errors |\n| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    for group in &report.summary {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            group["budget"],
            group["phase"].as_str().unwrap_or("?"),
            group["attempts"],
            percent(&group["required_recall"]),
            percent(&group["complete_body_recall"]),
            percent(&group["all_required_edit_inputs_rate"]),
            percent(&group["mean_non_gold_symbol_fraction"]),
            percent(&group["bug_relevant_required_recall"]),
            percent(&group["bug_relevant_complete_body_recall"]),
            group["errors"]
        ));
    }
    out.push_str("\n## Decision calibration\n\n`context_sufficient` is shadow-scored out-of-band against independently authored Gold context sufficiency (required identities + current SHA + complete original bodies + fixture writability), not WCode's own runtime readiness flags. Shadow calibration does not consume Agent Context bytes; exposed coverage separately reports whether the serialized `decision_plane` survived budget compaction. A false stop means the Decision Plane would prefer editing before Gold context is sufficient; a false continue means it would keep retrieving despite sufficient Gold context.\n\n| Budget | Phase | Shadow coverage | Exposed coverage | Brier | True stop | False stop | True continue | False continue |\n| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    for group in &report.summary {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            group["budget"],
            group["phase"].as_str().unwrap_or("?"),
            percent(&group["decision_context_coverage"]),
            percent(&group["decision_context_exposed_coverage"]),
            group["decision_context_mean_brier"],
            group["decision_context_true_stop_count"],
            group["decision_context_false_stop_count"],
            group["decision_context_true_continue_count"],
            group["decision_context_false_continue_count"]
        ));
    }
    out.push_str("\n## Body delivery partition\n\nCounts use required identities per attempt. Errors are unobserved, not successful absences. Partial original source is not complete-body credit.\n\n| Budget | Phase | Complete | Absent | Partial original | Unusable | Unobserved (error) |\n| ---: | --- | ---: | ---: | ---: | ---: | ---: |\n");
    for group in &report.summary {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            group["budget"],
            group["phase"].as_str().unwrap_or("?"),
            group["complete_body_hits"],
            group["absent_body_count"],
            group["partial_original_body_count"],
            group["unusable_body_count"],
            group["unobserved_body_due_to_error_count"]
        ));
    }
    out.push_str(&breakdown::markdown(&report.breakdown));
    out.push_str("\n## Controls\n\n");
    for control in &report.controls {
        out.push_str(&format!(
            "- {}: {}\n",
            control.id,
            if control.passed {
                "PASS"
            } else {
                "FAIL (see JSON)"
            }
        ));
    }
    out.push_str("\n## Per-case results\n\n| Case | Budget | Phase | Gold recall | Source recall | NDCG@10 | Non-Gold | p50 us | p95 us |\n| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    for row in &report.rows {
        let count = row.required_count * row.samples.len();
        let hits: usize = row
            .samples
            .iter()
            .filter_map(|s| s.score.as_ref())
            .map(|s| s.required_hits)
            .sum();
        let bodies: usize = row
            .samples
            .iter()
            .filter_map(|s| s.score.as_ref())
            .map(|s| s.complete_body_hits)
            .sum();
        let percentage = |n| {
            ratio(n, count)
                .map(|v| format!("{:.1}%", v * 100.0))
                .unwrap_or_else(|| "N/A".into())
        };
        let ndcg = (row.required_count > 0).then(|| {
            row.samples
                .iter()
                .filter_map(|sample| sample.score.as_ref().and_then(|score| score.ndcg_at_10))
                .sum::<f64>()
                / row.samples.len() as f64
        });
        let noise = row
            .samples
            .iter()
            .filter_map(|sample| {
                sample
                    .score
                    .as_ref()
                    .and_then(|score| score.non_gold_symbol_fraction)
            })
            .collect::<Vec<_>>();
        let mean_noise =
            (!noise.is_empty()).then(|| noise.iter().sum::<f64>() / noise.len() as f64);
        let percent_value = |value: Option<f64>| {
            value
                .map(|value| format!("{:.1}%", value * 100.0))
                .unwrap_or_else(|| "N/A".into())
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            row.case_id,
            row.budget,
            row.phase,
            percentage(hits),
            percentage(bodies),
            percent_value(ndcg),
            percent_value(mean_noise),
            row.latency["p50_us"],
            row.latency["p95_us"]
        ));
    }
    out.push_str("\nSmall-sample p95 can equal the maximum; no timing thresholds or model-success inference. Full samples, errors, denominators and revision metadata are retained in JSON.\n");
    out
}

pub(super) fn persist(report: &Report) -> Result<(String, String)> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let stem = format!("target/engineering-fitness-{stamp}-{}", std::process::id());
    let json_path = format!("{stem}.json");
    let markdown_path = format!("{stem}.md");
    // Workspace creation is atomic, create-only and rejects symlink/path escapes.
    let workspace = report_workspace()?;
    workspace
        .create_file(&json_path, &serde_json::to_string(report)?)
        .context("write Fitness JSON")?;
    workspace
        .create_file(&markdown_path, &markdown(report))
        .context("write Fitness Markdown")?;
    Ok((json_path, markdown_path))
}

#[test]
fn engineering_fitness_percentiles_cover_empty_odd_and_even_samples() {
    assert_eq!(distribution(&[])["p50_us"], Value::Null);
    assert_eq!(distribution(&[8, 2, 4])["p50_us"], json!(4.0));
    assert_eq!(distribution(&[8, 2, 4, 6])["p50_us"], json!(5.0));
    assert_eq!(distribution(&[8, 2, 4, 6])["p95_us"], json!(8));
}

fn valid_observation(sample: &Sample) -> bool {
    match (&sample.score, &sample.error) {
        (Some(score), None) => score.within_budget,
        (None, Some(error)) => !error.is_empty(),
        _ => false,
    }
}

#[test]
fn engineering_fitness_failures_stay_in_quality_denominators() {
    let case = super::corpus::base_case("rust");
    let observed = score(
        &json!({"budget":4000,"targets":[
            {"path":"src/session.rs","qualified_name":"cleanup_if_owner"},
            {"path":"src/session.rs","qualified_name":"refresh_session"}
        ]}),
        &case,
    );
    let row = Row {
        case_id: "scorer-control".into(),
        language: "rust".into(),
        category: "control".into(),
        fixture_sha256: "fixture".into(),
        fixture_files: 1,
        required_count: 2,
        writable: true,
        no_answer: false,
        budget: 4000,
        phase: "cold".into(),
        warmup_us: None,
        warmup_error: None,
        samples: vec![
            Sample {
                elapsed_us: 1,
                score: Some(observed),
                error: None,
            },
            Sample {
                elapsed_us: 2,
                score: None,
                error: Some("budget rejected".into()),
            },
        ],
        latency: distribution(&[1, 2]),
    };
    let summary = summarize(&[row]);
    assert_eq!(summary[0]["attempts"], 2);
    assert_eq!(summary[0]["errors"], 1);
    assert_eq!(summary[0]["required_gold_count"], 4);
    assert_eq!(summary[0]["required_recall"], json!(0.5));
    assert_eq!(summary[0]["absent_body_count"], 2);
    assert_eq!(summary[0]["partial_original_body_count"], 0);
    assert_eq!(summary[0]["unusable_body_count"], 0);
    assert_eq!(summary[0]["unobserved_body_due_to_error_count"], 2);
    assert!(!valid_observation(&Sample {
        elapsed_us: 0,
        score: None,
        error: None
    }));
}

fn shared_unit_report() -> &'static Report {
    static REPORT: std::sync::OnceLock<Report> = std::sync::OnceLock::new();
    REPORT.get_or_init(|| {
        collect(1).expect("model-free Engineering Fitness unit snapshot must collect")
    })
}

#[test]
fn engineering_fitness_matrix_records_every_case_budget_and_phase() {
    let report = shared_unit_report();
    assert_eq!(report.rows.len(), 60 * BUDGETS.len() * 2);
    assert_eq!(report.summary.len(), 6);
    assert!(report.rows.iter().all(|row| row.samples.len() == 1));
    assert!(report.summary.iter().all(|group| {
        group.get("decision_context_observations").is_some()
            && group.get("decision_context_exposed_coverage").is_some()
            && group.get("decision_context_mean_brier").is_some()
            && group.get("decision_context_false_stop_count").is_some()
            && group.get("decision_context_false_continue_count").is_some()
    }));
    println!(
        "FITNESS_MATRIX {}",
        serde_json::to_string(&report.summary).unwrap()
    );
    assert!(markdown(report)
        .contains("| Case | Budget | Phase | Gold recall | Source recall | NDCG@10 | Non-Gold |"));
    let query_failures: Vec<_> = report.rows.iter().filter(|row| row.samples.iter().any(|s| s.error.is_some()))
        .map(|row| json!({"case":row.case_id,"budget":row.budget,"phase":row.phase,"samples":row.samples})).collect();
    println!(
        "FITNESS_QUERY_FAILURES {}",
        serde_json::to_string(&query_failures).unwrap()
    );
    let mut decision_mistakes = BTreeMap::new();
    for row in &report.rows {
        for sample in &row.samples {
            let Some(score) = sample.score.as_ref() else {
                continue;
            };
            let Some(probability) = score.context_sufficient_milli else {
                continue;
            };
            let predicted_stop = probability >= CONTEXT_SUFFICIENT_STOP_THRESHOLD_MILLI;
            if predicted_stop != score.gold_context_sufficient {
                decision_mistakes
                    .entry(row.case_id.clone())
                    .or_insert_with(|| {
                        json!({
                            "category": row.category,
                            "first_budget": row.budget,
                            "first_phase": row.phase,
                            "probability_milli": probability,
                            "gold_context_sufficient": score.gold_context_sufficient,
                            "reported_edit_ready": score.reported_edit_ready,
                            "required_hits": score.required_hits,
                            "complete_body_hits": score.complete_body_hits,
                            "fresh_sha_hits": score.fresh_sha_hits
                        })
                    });
            }
        }
    }
    println!(
        "FITNESS_DECISION_MISTAKES {}",
        serde_json::to_string(&decision_mistakes).unwrap()
    );
    // Measurement correctness is not a requirement that the measured tool score 100%.
    // Rejected requests remain errors and contribute zero hits in the denominator.
    assert!(report
        .rows
        .iter()
        .flat_map(|row| &row.samples)
        .all(valid_observation));
}

#[test]
fn engineering_fitness_noise_snapshot() {
    let report = shared_unit_report();
    let summary = report
        .summary
        .iter()
        .map(|group| {
            json!({
                "budget": group["budget"],
                "phase": group["phase"],
                "required_recall": group["required_recall"],
                "complete_body_recall": group["complete_body_recall"],
                "mean_ndcg_at_10": group["mean_ndcg_at_10"],
                "mean_non_gold_symbol_fraction": group["mean_non_gold_symbol_fraction"],
                "query_errors": group["errors"],
            })
        })
        .collect::<Vec<_>>();
    let mut counts = BTreeMap::<String, usize>::new();
    for row in &report.rows {
        for sample in &row.samples {
            let Some(score) = sample.score.as_ref() else {
                continue;
            };
            for identity in &score.non_gold_identities {
                let key = format!("{}::{}", identity.path, identity.symbol);
                *counts.entry(key).or_default() += 1;
            }
        }
    }
    let mut dominant = counts.into_iter().collect::<Vec<_>>();
    dominant.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    dominant.truncate(12);
    println!(
        "FITNESS_NOISE_BASELINE {}",
        json!({"summary": summary, "dominant_non_gold": dominant})
    );
    assert!(report.summary.iter().all(|group| group["errors"] == 0));
    assert!(report
        .summary
        .iter()
        .all(|group| group["required_recall"] == 1.0));
}

#[test]
fn engineering_fitness_decision_calibration_snapshot() {
    let report = shared_unit_report();
    let compact = report
        .summary
        .iter()
        .map(|group| {
            json!({
                "budget": group["budget"],
                "phase": group["phase"],
                "shadow_coverage": group["decision_context_coverage"],
                "exposed_coverage": group["decision_context_exposed_coverage"],
                "brier": group["decision_context_mean_brier"],
                "false_stop": group["decision_context_false_stop_count"],
                "false_continue": group["decision_context_false_continue_count"],
                "query_errors": group["errors"],
            })
        })
        .collect::<Vec<_>>();
    println!(
        "FITNESS_DECISION_CALIBRATION {}",
        serde_json::to_string(&compact).unwrap()
    );
    assert!(compact.iter().all(|group| group["shadow_coverage"] == 1.0));
    assert!(compact.iter().all(|group| group["query_errors"] == 0));
}

#[test]
#[ignore = "explicit model-free Fitness trial; emits reports; timing is descriptive only"]
fn engineering_fitness_trial() {
    let report = collect(7).unwrap();
    let paths = persist(&report).unwrap();
    println!(
        "FITNESS_REPORT {}",
        json!({"json":paths.0,"markdown":paths.1,
        "metadata":report.metadata,"summary":report.summary,"controls":report.controls})
    );
    assert!(
        report.controls.iter().all(|control| control.passed),
        "controls failed; inspect saved report"
    );
    assert!(
        report
            .rows
            .iter()
            .flat_map(|row| &row.samples)
            .all(valid_observation),
        "invalid observation or oversized successful response; inspect saved report"
    );
}
