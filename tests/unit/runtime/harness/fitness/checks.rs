use super::corpus::{base_case, corpus, Case, Identity};
use super::scoring::{digest, provider_context_calibration, score};
use crate::decision::{
    DecisionBatch, DecisionProvider, DecisionRequest, DecisionValue, DeterministicDecisionProvider,
};
use crate::harness::ToolHarness;
use serde_json::{json, Value};
use std::collections::BTreeSet;

struct CautiousShadowCandidate;

impl DecisionProvider for CautiousShadowCandidate {
    fn provider_id(&self) -> &'static str {
        "fixture-cautious-shadow"
    }

    fn evaluate(&self, request: &DecisionRequest) -> DecisionBatch {
        let mut batch = DeterministicDecisionProvider.evaluate(request);
        batch.provider = self.provider_id().into();
        if let Some(signal) = batch
            .signals
            .iter_mut()
            .find(|signal| signal.id == "context_sufficient")
        {
            if let DecisionValue::Probability { probability_milli } = &mut signal.value {
                *probability_milli = probability_milli.saturating_sub(300);
            }
        }
        batch
    }
}

fn complete_pack(case: &Case) -> Value {
    let mut targets = Vec::new();
    let mut bodies = Vec::new();
    let mut files = Vec::new();
    for gold in &case.required {
        let original = &case.files[&gold.identity.path];
        let offset = original.find(&gold.fragment).unwrap();
        let start = original[..offset].bytes().filter(|b| *b == b'\n').count() + 1;
        let content = gold.fragment.trim_end_matches(['\r', '\n']);
        let sha = digest(original.as_bytes());
        let target = json!({"path":gold.identity.path,"qualified_name":gold.identity.symbol});
        let mut source = target.clone();
        source["sha256"] = json!(sha);
        source["body"] = json!({"content":content,"start_line":start,
            "end_line":start + content.lines().count() - 1,"redacted":false,"truncated":false});
        targets.push(target);
        bodies.push(source);
        files.push(json!({"path":gold.identity.path,"sha256":sha,"readonly":false}));
    }
    json!({"targets":targets,"hot_source":bodies,"files":files,"repo_map":{"items":[]},
        "budget":4000,"project":{"write_enabled":true},"readiness":{"edit":"ready"}})
}

#[test]
fn provider_shadow_benchmark_scores_candidates_against_the_same_authored_gold() {
    let case = base_case("rust");
    let (_root, workspace) = case.instantiate();
    let harness = ToolHarness::new(4).unwrap();
    let pack = harness
        .agent_context("fitness", &workspace, &case.query, 4_000, &[])
        .unwrap();

    let baseline = provider_context_calibration(&pack, &case, &DeterministicDecisionProvider);
    let candidate = provider_context_calibration(&pack, &case, &CautiousShadowCandidate);

    assert!(baseline.gold_context_sufficient);
    assert_eq!(
        candidate.gold_context_sufficient,
        baseline.gold_context_sufficient
    );
    assert_eq!(baseline.predicted_stop, Some(true));
    assert_eq!(candidate.predicted_stop, Some(false));
    assert!(candidate.brier_million > baseline.brier_million);
    assert_eq!(candidate.comparison.safety_policy_violation_count, 0);
    assert_eq!(candidate.comparison.probability_abs_delta_milli_sum, 300);
}

#[test]
fn engineering_fitness_validates_frozen_gold_corpus() {
    let cases = corpus();
    assert_eq!(cases.len(), 60);
    let mut ids = BTreeSet::new();
    for case in &cases {
        assert!(ids.insert(&case.id));
        assert_eq!(case.no_answer, case.required.is_empty(), "{}", case.id);
        let mut gold_ids = BTreeSet::new();
        for gold in &case.required {
            assert!(
                gold_ids.insert(&gold.identity),
                "duplicate gold in {}",
                case.id
            );
            assert!(!gold.fragment.trim().is_empty());
            assert_eq!(
                case.files[&gold.identity.path]
                    .matches(&gold.fragment)
                    .count(),
                1
            );
        }
    }
    assert_eq!(
        digest(&serde_json::to_vec(&cases).unwrap()),
        digest(&serde_json::to_vec(&corpus()).unwrap())
    );
}

#[test]
fn engineering_fitness_useful_annotations_keep_true_noise_unlabelled() {
    let cases = corpus();
    for language in ["rust", "go", "typescript", "python"] {
        let extension = match language {
            "rust" => "rs",
            "go" => "go",
            "typescript" => "ts",
            "python" => "py",
            _ => unreachable!(),
        };
        let path = format!("src/session.{extension}");
        let cleanup = cases
            .iter()
            .find(|case| case.id == format!("{language}-cleanup-only"))
            .unwrap();
        assert!(cleanup
            .useful
            .contains(&Identity::new(&path, "refresh_session")));

        let refresh = cases
            .iter()
            .find(|case| case.id == format!("{language}-refresh-only"))
            .unwrap();
        assert!(refresh
            .useful
            .contains(&Identity::new(&path, "cleanup_if_owner")));
    }

    let mutation = cases
        .iter()
        .find(|case| case.id == "rust-mutation-self-owner")
        .unwrap();
    assert!(mutation.useful.contains(&Identity::new(
        "tests/session.rs",
        "replacement_keeps_new_owner",
    )));
    assert!(
        !mutation
            .useful
            .contains(&Identity::new("src/lib.rs", "session")),
        "module re-export filler must remain measurable as true Non-Gold noise"
    );
}

#[test]
fn engineering_fitness_rejects_wrong_path_same_symbol() {
    let case = base_case("rust");
    let mut pack = complete_pack(&case);
    for field in ["targets", "hot_source"] {
        for item in pack[field].as_array_mut().unwrap() {
            item["path"] = json!("src/wrong.rs");
        }
    }
    let result = score(&pack, &case);
    assert_eq!(result.required_recall, Some(0.0));
    assert_eq!(result.complete_body_recall, Some(0.0));
    assert!(!result.all_required_edit_inputs);
    assert!(
        result.reported_edit_ready,
        "test must challenge an optimistic self-report"
    );
}

#[test]
fn decision_calibration_truth_does_not_trust_system_readiness_flags() {
    let case = base_case("rust");
    let mut pack = complete_pack(&case);
    pack["project"]["write_enabled"] = json!(false);
    pack["readiness"]["edit"] = json!("blocked");
    for file in pack["files"].as_array_mut().unwrap() {
        file["readonly"] = json!(true);
    }
    let observed = score(&pack, &case);
    assert!(
        observed.gold_context_sufficient,
        "authored Gold delivery must be graded independently from WCode self-reported readiness"
    );
    assert!(
        !observed.all_required_edit_inputs,
        "operational edit readiness may still reflect the system-provided write contract"
    );

    let mut readonly = case;
    readonly.writable = false;
    let pack = complete_pack(&readonly);
    let observed = score(&pack, &readonly);
    assert!(
        !observed.gold_context_sufficient,
        "authored readonly truth must not be overridden by an optimistic pack"
    );
}

#[test]
fn decision_shadow_uses_final_ready_context_inputs() {
    let case = corpus()
        .into_iter()
        .find(|case| case.id == "typescript-call-chain")
        .unwrap();
    let (_root, workspace) = case.instantiate();
    let pack = ToolHarness::new(4)
        .unwrap()
        .agent_context("fitness", &workspace, &case.query, 4_000, &[])
        .unwrap();
    let probability = crate::decision::probability_milli(
        &crate::decision::agent_context_decisions(&pack, &case.query),
        "context_sufficient",
    )
    .unwrap();
    let state = json!({
        "edit": pack.pointer("/readiness/edit"),
        "editable_sha_targets": pack.pointer("/readiness/editable_sha_targets"),
        "targets": pack["targets"].as_array().map_or(0, Vec::len),
        "hot_source": pack["hot_source"].as_array().map_or(0, Vec::len),
        "tests": pack["tests"].as_array().map_or(0, Vec::len),
        "repo_map_truncated": pack.pointer("/repo_map/truncated"),
        "probability_milli": probability,
    });
    assert_eq!(pack["readiness"]["edit"], "ready", "{state}");
    assert!(
        pack["hot_source"]
            .as_array()
            .is_some_and(|items| !items.is_empty()),
        "{state}"
    );
    assert!(
        pack.pointer("/readiness/editable_sha_targets")
            .and_then(Value::as_u64)
            .is_some_and(|count| count > 0),
        "{state}"
    );
    assert!(
        probability >= crate::decision::CONTEXT_SUFFICIENT_STOP_THRESHOLD_MILLI,
        "final ready context must not recommend unnecessary retrieval: {state}"
    );
}

#[test]
fn engineering_fitness_deduplicates_gold_and_delivery() {
    let mut case = base_case("rust");
    let mut pack = complete_pack(&case);
    case.required.push(case.required[0].clone());
    let duplicate = pack["targets"][0].clone();
    pack["targets"].as_array_mut().unwrap().push(duplicate);
    let result = score(&pack, &case);
    assert_eq!(result.required_count, 2);
    assert_eq!(result.required_hits, 2);
    assert_eq!(result.delivered_identities, 2);
    assert_eq!(result.required_recall, Some(1.0));
    assert!(result.all_required_edit_inputs);
}

#[test]
fn engineering_fitness_empty_results_are_not_perfect_precision() {
    let case = base_case("rust");
    let result = score(&json!({}), &case);
    assert_eq!(result.required_recall, Some(0.0));
    assert_eq!(result.non_gold_symbol_fraction, None);
    assert!(!result.within_budget);
    assert!(!result.all_required_edit_inputs);
    let mut no_answer = case;
    no_answer.required.clear();
    no_answer.no_answer = true;
    let empty = score(&json!({}), &no_answer);
    assert_eq!(empty.required_recall, None);
    assert_eq!(empty.ndcg_at_10, None);
    assert_eq!(empty.abstained, Some(true));
    assert!(!empty.all_required_edit_inputs);
}

#[test]
fn engineering_fitness_penalizes_noise_and_ranking() {
    let mut case = base_case("rust");
    case.required.truncate(1);
    case.useful.clear();
    let mut pack = complete_pack(&case);
    assert_eq!(score(&pack, &case).ndcg_at_10, Some(1.0));
    pack["targets"].as_array_mut().unwrap().insert(
        0,
        json!({"path":"src/other.rs","qualified_name":"unrelated"}),
    );
    let result = score(&pack, &case);
    assert_eq!(result.non_gold_symbol_fraction, Some(0.5));
    assert_eq!(result.first_required_rank, Some(2));
    assert!((result.ndcg_at_10.unwrap() - 1.0 / 3.0_f64.log2()).abs() < 1e-12);
}

#[test]
fn engineering_fitness_requires_each_current_sha_and_complete_source() {
    let case = base_case("rust");
    let good = complete_pack(&case);
    for tampering in [
        "stale_sha",
        "missing_sha",
        "redacted",
        "wrong_line",
        "invented_source",
        "partial_body",
        "readonly",
    ] {
        let mut pack = good.clone();
        match tampering {
            "stale_sha" => pack["hot_source"][1]["sha256"] = json!("0".repeat(64)),
            "missing_sha" => pack["files"] = json!([]),
            "redacted" => pack["hot_source"][1]["body"]["redacted"] = json!(true),
            "wrong_line" => pack["hot_source"][1]["body"]["start_line"] = json!(1),
            "invented_source" => pack["hot_source"][1]["body"]["content"] = json!("fabricated()"),
            "partial_body" => pack["hot_source"].as_array_mut().unwrap().truncate(1),
            "readonly" => pack["files"][0]["readonly"] = json!(true),
            _ => unreachable!(),
        }
        assert!(
            !score(&pack, &case).all_required_edit_inputs,
            "accepted {tampering}"
        );
    }
}

#[test]
fn engineering_fitness_source_grading_preserves_crlf_unicode() {
    let case = corpus()
        .into_iter()
        .find(|case| case.id == "rust-unicode-crlf-anchor")
        .unwrap();
    let mut pack = complete_pack(&case);
    assert_eq!(score(&pack, &case).complete_body_recall, Some(1.0));
    let body = pack["hot_source"][0]["body"]["content"]
        .as_str()
        .unwrap()
        .replace("\r\n", "\n");
    pack["hot_source"][0]["body"]["content"] = json!(body);
    assert_eq!(score(&pack, &case).complete_body_recall, Some(0.0));
}

#[test]
fn engineering_fitness_mutation_corpus_is_distinct_and_gold_is_authored() {
    let cases = corpus();
    let mutations: Vec<_> = cases
        .iter()
        .filter(|case| case.category == "bug-relevant-evidence")
        .collect();
    assert_eq!(mutations.len(), 10);
    let source_digests: BTreeSet<_> = mutations
        .iter()
        .map(|case| digest(case.files["src/session.rs"].as_bytes()))
        .collect();
    assert_eq!(source_digests.len(), mutations.len());
    assert!(mutations.iter().all(|case| case.required.len() == 2));
}

#[test]
fn engineering_fitness_actual_four_language_explicit_retrieval() {
    for language in ["rust", "go", "typescript", "python"] {
        let case = base_case(language);
        let (_root, workspace) = case.instantiate();
        let pack = ToolHarness::new(4)
            .unwrap()
            .agent_context("fitness", &workspace, &case.query, 4_000, &[])
            .unwrap();
        let result = score(&pack, &case);
        assert_eq!(result.required_recall, Some(1.0), "{language}: {result:?}");
        assert_eq!(
            result.complete_body_recall,
            Some(1.0),
            "{language}: {result:?}\n{pack}"
        );
        assert!(result.within_budget, "{language}: {result:?}");
        println!(
            "FITNESS_EXPLICIT {}",
            json!({"language":language,"score":result})
        );
    }
}

#[test]
fn engineering_fitness_warm_cache_never_loses_cold_edit_evidence() {
    let mut regressions = Vec::new();
    for case in corpus()
        .into_iter()
        .filter(|case| !case.required.is_empty())
    {
        let (_root, workspace) = case.instantiate();
        let cold = ToolHarness::new(4)
            .unwrap()
            .agent_context("fitness", &workspace, &case.query, 4_000, &[])
            .unwrap();
        let warm_harness = ToolHarness::new(4).unwrap();
        warm_harness
            .agent_context("fitness", &workspace, &case.query, 4_000, &[])
            .unwrap();
        let warm = warm_harness
            .agent_context("fitness", &workspace, &case.query, 4_000, &[])
            .unwrap();
        let cold_score = score(&cold, &case);
        let warm_score = score(&warm, &case);
        if warm_score.complete_body_hits < cold_score.complete_body_hits
            || (cold_score.all_required_edit_inputs && !warm_score.all_required_edit_inputs)
        {
            regressions.push(json!({
                "case": case.id,
                "cold": cold_score,
                "warm": warm_score,
                "cold_targets": cold["targets"],
                "warm_targets": warm["targets"],
                "cold_repo": cold["repo_map"]["items"],
                "warm_repo": warm["repo_map"]["items"],
                "cold_source": cold["hot_source"],
                "warm_source": warm["hot_source"],
            }));
        }
    }
    assert!(
        regressions.is_empty(),
        "warm cache lost cold edit evidence: {}",
        serde_json::to_string_pretty(&regressions).unwrap()
    );
}

#[test]
fn engineering_fitness_explicit_context_omits_unrelated_central_filler() {
    let case = base_case("rust");
    let (_root, workspace) = case.instantiate();
    let pack = ToolHarness::new(4)
        .unwrap()
        .agent_context("fitness", &workspace, &case.query, 4_000, &[])
        .unwrap();
    let central = pack["repo_map"]["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["reason"] == "central")
        .collect::<Vec<_>>();
    assert!(
        central.is_empty(),
        "task-unrelated central filler should not consume explicit context budget: {central:#?}"
    );
}

#[test]
fn engineering_fitness_report_workspace_keeps_repository_boundary() {
    let workspace = super::controls::report_workspace().unwrap();
    assert_eq!(
        workspace.root(),
        std::fs::canonicalize(env!("CARGO_MANIFEST_DIR")).unwrap()
    );
    assert!(
        !workspace.exec_enabled(),
        "reporting must not enable command execution"
    );
    let outside = tempfile::tempdir().unwrap();
    let external = outside.path().join("report.json");
    std::fs::write(&external, "unchanged").unwrap();
    assert!(workspace
        .read_file(external.to_str().unwrap(), 1, None)
        .is_err());
    assert!(workspace
        .create_file(external.to_str().unwrap(), "changed")
        .is_err());
    assert!(workspace.create_file("../report.json", "changed").is_err());
    assert_eq!(std::fs::read_to_string(external).unwrap(), "unchanged");
}

#[test]
fn engineering_fitness_live_controls_preserve_safety_and_precision() {
    let controls = super::controls::run_controls();
    println!(
        "FITNESS_CONTROLS {}",
        serde_json::to_string(&controls).unwrap()
    );
    assert_eq!(controls.len(), 9);
    assert!(
        controls.iter().all(|control| control.passed),
        "{controls:#?}"
    );
}

#[test]
fn engineering_fitness_useful_context_is_not_required_context() {
    let mut case = base_case("rust");
    let pack = complete_pack(&case);
    case.required[1].identity = Identity::new("src/unknown.rs", "unknown");
    case.useful
        .push(Identity::new("src/session.rs", "refresh_session"));
    let result = score(&pack, &case);
    assert_eq!(result.required_recall, Some(0.5));
    assert_eq!(result.non_gold_symbol_fraction, Some(0.0));
    assert!(!result.all_required_edit_inputs);
}
