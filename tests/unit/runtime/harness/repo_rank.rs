use super::*;
use crate::harness::harness_retrieval::{
    augment_relationship_graph, compare_repo_candidates, select_repo_candidates, RepoMapCandidate,
};
use std::borrow::Cow;

#[path = "repo_rank_perf.rs"]
mod perf;

fn rank_candidates(count: usize, mut seed: u64) -> Vec<RepoMapCandidate> {
    (0..count)
        .map(|index| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            RepoMapCandidate {
                id: format!("symbol:{index:06}"),
                path: format!("src/group_{:02}.rs", index % 8),
                name: format!("item_{}", index % 16),
                qualified_name: format!("item_{}", index % 16),
                kind: "function".to_owned(),
                relevance: 0.0,
                direct: index % 7 == 0,
                exact_direct: index % 97 == 0,
                design_path: false,
                query_hits: 0,
                test_target_match: false,
                experience_weight: 0,
                degree: 0,
                rank: ((seed >> 32) % 32) as f64,
            }
        })
        .collect()
}

fn ranked_ids(candidates: &[RepoMapCandidate]) -> Vec<&str> {
    candidates
        .iter()
        .map(|candidate| candidate.id.as_str())
        .collect()
}

#[test]
fn repo_rank_top_k_matches_full_sort_across_boundaries_and_ties() {
    for seed in 0..8 {
        for count in [0, 1, 15, 16, 17, 257, 6_000] {
            let input = rank_candidates(count, seed);
            let mut expected = input.clone();
            expected.sort_by(compare_repo_candidates);
            for limit in [0, 1, 8, 16, count, count + 1] {
                let mut actual = input.clone();
                select_repo_candidates(&mut actual, limit);
                assert_eq!(
                    ranked_ids(&actual),
                    ranked_ids(&expected[..limit.min(count)])
                );
                let mut reversed = input.clone();
                reversed.reverse();
                select_repo_candidates(&mut reversed, limit);
                assert_eq!(ranked_ids(&actual), ranked_ids(&reversed));
            }
        }
    }
}

#[test]
fn repo_rank_no_supplement_reuses_the_original_graph() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn target_feature() {}\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let mut context = harness
        .software_context("demo", &workspace, "target_feature", "inspect", 4_000, &[])
        .unwrap();
    let (base, _) = harness.repo_map_graph("demo", &workspace, ".").unwrap();
    for query in ["target_feature", "target_feature callers"] {
        let result =
            augment_relationship_graph(&harness, "demo", &workspace, query, &context, &base)
                .unwrap();
        assert!(matches!(&result, Cow::Borrowed(_)));
        assert!(std::ptr::eq(result.as_ref(), base.as_ref()));
    }
    let mut truncated = base.as_ref().clone();
    truncated.scan_truncated = true;
    context.symbols.clear();
    let result = augment_relationship_graph(
        &harness, "demo", &workspace, "callers", &context, &truncated,
    )
    .unwrap();
    assert!(matches!(&result, Cow::Borrowed(_)));
    assert!(std::ptr::eq(result.as_ref(), &truncated));
}

#[test]
fn direct_relationship_evidence_outranks_a_high_degree_second_hop() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    let mut source = String::from(
        "pub fn target_feature() {}\npub fn direct_caller() { target_feature(); hub(); }\npub fn hub() {}\n",
    );
    for index in 0..500 {
        source.push_str(&format!("pub fn helper_{index}() {{ hub(); }}\n"));
    }
    fs::write(root.path().join("src/lib.rs"), source).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let pack = ToolHarness::new(4)
        .unwrap()
        .agent_context(
            "demo",
            &workspace,
            "show callers of target_feature",
            4_000,
            &[],
        )
        .unwrap();
    let items = pack["repo_map"]["items"].as_array().unwrap();
    let caller = items
        .iter()
        .position(|item| item["qualified_name"] == "direct_caller")
        .expect("direct caller must be returned");
    let hub = items
        .iter()
        .position(|item| item["qualified_name"] == "hub")
        .expect("second-hop hub must be returned");
    assert!(
        caller < hub,
        "direct relationship evidence must outrank graph-central second-hop nodes: {}",
        pack["repo_map"]
    );
}

#[test]
fn exact_repo_target_stays_ahead_of_a_high_degree_helper() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    let mut source = String::from(
        "pub fn target_feature() { target_feature_hub(); }\npub fn target_feature_hub() {}\n",
    );
    for index in 0..12 {
        source.push_str(&format!(
            "pub fn target_feature_helper_{index}() {{ target_feature_hub(); }}\n"
        ));
    }
    fs::write(root.path().join("src/lib.rs"), source).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    for query in ["target_feature", "target_feature impact"] {
        for _ in 0..2 {
            let pack = harness
                .agent_context("demo", &workspace, query, 4_000, &[])
                .unwrap();
            assert_eq!(pack["targets"][0]["qualified_name"], "target_feature");
            assert_eq!(
                pack["repo_map"]["items"][0]["qualified_name"], "target_feature",
                "an explicit target must outrank central helpers: {}",
                pack["repo_map"]
            );
        }
    }
}

#[test]
fn code_to_test_exact_target_excludes_unrelated_tests_in_the_same_file() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::create_dir_all(root.path().join("tests")).unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn target_feature() -> usize { 7 }\n",
    )
    .unwrap();
    let mut source = String::from(
        "#[test]\nfn validates_contract() {\n    let value = target_feature();\n    assert_eq!(value, 7);\n}\n#[test]\nfn checks_macro() { assert_eq!(target_feature(), 7); }\n#[test]\nfn target_feature_regression() { assert_eq!(1, 1); }\n",
    );
    for index in 0..40 {
        source.push_str(&format!(
            "#[test]\nfn unrelated_case_{index}() {{ assert_eq!(1, 1); }}\n"
        ));
    }
    fs::write(root.path().join("tests/contract.rs"), source).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    for query in ["which tests verify target_feature", "target_feature 测试"] {
        for pass in 0..2 {
            let pack = harness
                .agent_context("demo", &workspace, query, 4_000, &[])
                .unwrap();
            let items = pack["repo_map"]["items"].as_array().unwrap();
            assert!(
                items
                    .iter()
                    .any(|item| item["qualified_name"] == "validates_contract"),
                "relevant test missing: pass={pass} query={query} map={}",
                pack["repo_map"]
            );
            for name in ["checks_macro", "target_feature_regression"] {
                let item = items
                    .iter()
                    .find(|item| item["qualified_name"] == name)
                    .unwrap();
                assert_eq!(item["reason"], "test_target_text_match");
                assert!(item["relationships"].as_array().unwrap().is_empty());
            }
            println!(
                "code-to-test pass={pass} query={query} returned={} eligible={}",
                items.len(),
                pack["repo_map"]["eligible_candidates"]
            );
            let noise = items
                .iter()
                .filter(|item| {
                    item["qualified_name"]
                        .as_str()
                        .unwrap_or_default()
                        .starts_with("unrelated_case_")
                })
                .count();
            assert_eq!(
                noise, 0,
                "unrelated test noise: pass={pass} query={query} map={}",
                pack["repo_map"]
            );
        }
    }
}

#[test]
fn code_to_test_text_candidates_require_matching_source_and_symbol_range() {
    use crate::harness::harness_retrieval::mark_test_target_matches;
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("tests")).unwrap();
    fs::write(
        root.path().join("tests/contract.rs"),
        "#[test]\nfn checks_macro() {\n    assert_eq!(target_feature(), 7);\n}\n#[test]\nfn unrelated() {}\n",
    ).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let (base, _) = harness.repo_map_graph("demo", &workspace, ".").unwrap();
    let node = base
        .graph
        .nodes
        .values()
        .find(|node| node.label == "checks_macro")
        .unwrap();
    let sha = node.provenance.revision.strip_prefix("sha256:").unwrap();
    let good = json!({"path":"tests/contract.rs","sha256":sha,"line":3,"redacted":false,"text_truncated":false});
    let mut graph = base.as_ref().clone();
    let edges = graph.graph.edges.len();
    mark_test_target_matches(&mut graph, std::slice::from_ref(&good));
    let marked: Vec<_> = graph
        .graph
        .nodes
        .values()
        .filter(|node| node.attributes.get("test_target_match") == Some(&json!(true)))
        .map(|node| node.label.as_str())
        .collect();
    assert_eq!(marked, ["checks_macro"]);
    assert_eq!(
        graph.graph.edges.len(),
        edges,
        "text matches must not invent calls"
    );
    for (key, value) in [
        ("sha256", json!("0".repeat(64))),
        ("sha256", json!("invalid")),
        ("line", json!(0)),
        ("line", Value::Null),
        ("path", json!("")),
        ("redacted", json!(true)),
        ("text_truncated", json!(true)),
    ] {
        let mut row = good.clone();
        row[key] = value;
        let mut graph = base.as_ref().clone();
        mark_test_target_matches(&mut graph, &[row]);
        assert!(graph.scan_truncated, "{key}");
        assert!(
            graph
                .graph
                .nodes
                .values()
                .all(|node| !node.attributes.contains_key("test_target_match")),
            "{key}"
        );
    }
}

#[test]
fn code_to_test_match_storm_does_not_hide_later_test_files() {
    let root = tempfile::tempdir().unwrap();
    for directory in ["docs", "src", "tests"] {
        fs::create_dir_all(root.path().join(directory)).unwrap();
    }
    fs::write(
        root.path().join("docs/aaa_usage.md"),
        "target_feature example\n".repeat(700),
    )
    .unwrap();
    fs::write(
        root.path().join("src/target.rs"),
        "pub fn target_feature() -> usize { 7 }\npub fn secondary_feature() -> usize { 9 }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("tests/z_contract.rs"),
        "#[test]\nfn verifies_macro() {\n    assert_eq!(target_feature(), 7);\n}\n#[test]\nfn verifies_secondary() { assert_eq!(secondary_feature(), 9); }\n#[test]\nfn unrelated_case() { assert_eq!(1, 1); }\n",
    ).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    for query in [
        "which tests verify target_feature",
        "target_feature 测试",
        "which tests verify target_feature and secondary_feature",
        "target_feature 和 secondary_feature 测试",
    ] {
        for pass in 0..2 {
            let pack = harness
                .agent_context("demo", &workspace, query, 4_000, &[])
                .unwrap();
            let map = &pack["repo_map"];
            let items = map["items"].as_array().unwrap();
            let item = items
                .iter()
                .find(|item| item["qualified_name"] == "verifies_macro")
                .unwrap_or_else(|| {
                    panic!("crowded file starved test: pass={pass} query={query} map={map}")
                });
            if query.contains("secondary_feature") {
                let secondary = items
                    .iter()
                    .find(|item| item["qualified_name"] == "verifies_secondary")
                    .unwrap_or_else(|| {
                        panic!("second target missing: pass={pass} query={query} map={map}")
                    });
                assert_eq!(secondary["reason"], "test_target_text_match");
                assert!(secondary["relationships"].as_array().unwrap().is_empty());
            }
            assert_eq!(item["reason"], "test_target_text_match");
            assert!(item["relationships"].as_array().unwrap().is_empty());
            assert!(!items
                .iter()
                .any(|item| item["qualified_name"] == "unrelated_case"));
            assert!(map["files_indexed"].as_u64().unwrap() <= 3);
            assert_eq!(
                map["scan_truncated"], true,
                "bounded search must disclose omitted lines"
            );
        }
    }
}
#[test]
fn code_to_test_reports_partial_coverage_when_target_limit_is_exceeded() {
    for count in [4, 5, 8] {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("src")).unwrap();
        fs::create_dir_all(root.path().join("tests")).unwrap();
        let mut source = String::new();
        let mut checks = String::new();
        let mut names = Vec::new();
        for index in 0..count {
            let name = format!("feature_{index}");
            source.push_str(&format!("pub fn {name}() -> usize {{ {index} }}\n"));
            checks.push_str(&format!(
                "#[test]\nfn verifies_{index}() {{ assert_eq!({name}(), {index}); }}\n"
            ));
            names.push(name);
        }
        fs::write(root.path().join("src/core.rs"), source).unwrap();
        fs::write(root.path().join("tests/contract.rs"), checks).unwrap();
        let workspace = Workspace::new(root.path(), true, false).unwrap();
        let harness = ToolHarness::new(4).unwrap();
        let query = format!("which tests verify {}", names.join(" "));
        for pass in 0..2 {
            let context = harness
                .software_context("demo", &workspace, &query, "inspect", 10_000, &[])
                .unwrap();
            for name in &names {
                assert!(
                    context.symbols.iter().any(|item| item["name"] == *name),
                    "exact target missing: {name}"
                );
            }
            let (base, _) = harness.repo_map_graph("demo", &workspace, ".").unwrap();
            assert!(!base.scan_truncated);
            let graph =
                augment_relationship_graph(&harness, "demo", &workspace, &query, &context, &base)
                    .unwrap();
            let actual_tests = graph
                .graph
                .nodes
                .values()
                .filter(|node| {
                    node.label.starts_with("verifies_")
                        && node.attributes.get("test_target_match") == Some(&json!(true))
                })
                .count();
            assert_eq!(actual_tests, count.min(4), "pass={pass}");
            assert_eq!(
                graph.scan_truncated,
                count > 4,
                "omitted target search must remain visible: pass={pass} targets={count}"
            );
        }
    }
}

#[test]
fn code_to_test_recovers_jsx_and_tsx_text_candidates() {
    for suffix in ["test.jsx", "spec.jsx", "test.tsx", "spec.tsx"] {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("src")).unwrap();
        fs::write(
            root.path().join("src/core.ts"),
            "export function target_feature() { return 7; }\n",
        )
        .unwrap();
        let path = format!("src/widget.{suffix}");
        fs::write(
            root.path().join(&path),
            "export function verifies_contract() { return \"target_feature\"; }\nexport function unrelated() { return 0; }\n",
        )
        .unwrap();
        let workspace = Workspace::new(root.path(), true, false).unwrap();
        let harness = ToolHarness::new(4).unwrap();
        for pass in 0..2 {
            let pack = harness
                .agent_context(
                    "demo",
                    &workspace,
                    "which tests verify target_feature",
                    4_000,
                    &[],
                )
                .unwrap();
            let map = &pack["repo_map"];
            let items = map["items"].as_array().unwrap();
            let item = items
                .iter()
                .find(|item| item["qualified_name"] == "verifies_contract")
                .unwrap_or_else(|| {
                    panic!("test candidate missing: suffix={suffix} pass={pass} map={map}")
                });
            assert_eq!(item["path"], path);
            assert_eq!(item["reason"], "test_target_text_match");
            assert!(item["relationships"].as_array().unwrap().is_empty());
            assert!(!items
                .iter()
                .any(|item| item["qualified_name"] == "unrelated"));
        }
    }
}
