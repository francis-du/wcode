use super::corpus::{base_case, corpus, Case, Identity};
use super::scoring::{digest, score};
use crate::harness::ToolHarness;
use serde_json::{json, Value};
use std::collections::BTreeSet;

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
