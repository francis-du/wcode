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
