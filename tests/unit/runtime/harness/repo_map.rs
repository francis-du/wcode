use super::*;

#[path = "repo_merge.rs"]
mod repo_merge;
#[path = "repo_rank.rs"]
mod repo_rank;
use crate::harness::harness_repo_map::repo_map_common_scope;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn repo_map_common_scope_keeps_localized_targets_bounded() {
    let local = vec![
        "src/runtime/harness/repo_map.rs".to_owned(),
        "src/runtime/harness/context_budget.rs".to_owned(),
    ];
    assert_eq!(repo_map_common_scope(&local), "src/runtime/harness");

    let split = vec![
        "src/runtime/harness/repo_map.rs".to_owned(),
        "tests/unit/runtime/harness/repo_map.rs".to_owned(),
    ];
    assert_eq!(repo_map_common_scope(&split), ".");
}

#[test]
fn repo_map_common_scope_keeps_root_level_sources() {
    for paths in [
        vec!["lib.rs".to_owned(), "src/domain/target.rs".to_owned()],
        vec!["src/domain/target.rs".to_owned(), "lib.rs".to_owned()],
    ] {
        assert_eq!(repo_map_common_scope(&paths), ".", "paths={paths:?}");
    }
}

#[test]
fn repo_map_relation_queries_preserve_cross_directory_recall() {
    let root = tempfile::tempdir().unwrap();
    for directory in ["src/domain", "src/adapter", "tests"] {
        fs::create_dir_all(root.path().join(directory)).unwrap();
    }
    fs::write(
        root.path().join("src/domain/target.rs"),
        "pub fn target_feature() -> usize { 7 }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("src/adapter/caller.rs"),
        "pub fn invoke_target() -> usize {\n    target_feature()\n}\n",
    )
    .unwrap();
    fs::write(
        root.path().join("tests/contract.rs"),
        "#[test]\nfn validates_contract() {\n    let value = target_feature();\n    assert_eq!(value, 7);\n}\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let mut missing = Vec::new();
    for (query, expected) in [
        ("show callers of target_feature", "invoke_target"),
        ("which tests verify target_feature", "validates_contract"),
        ("查找target_feature调用方", "invoke_target"),
        ("target_feature 测试", "validates_contract"),
        ("target_feature impact", "invoke_target"),
        ("find callers and tests for target_feature", "invoke_target"),
    ] {
        let harness = ToolHarness::new(4).unwrap();
        for pass in 0..2 {
            let pack = harness
                .agent_context("demo", &workspace, query, 4_000, &[])
                .unwrap();
            let items = pack["repo_map"]["items"].as_array().unwrap();
            if !items.iter().any(|item| item["qualified_name"] == expected) {
                missing.push(format!(
                    "pass={pass} query={query} missing={expected} map={}",
                    pack["repo_map"]
                ));
            }
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}

#[test]
fn repo_map_identifier_words_do_not_force_relationship_scans() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src/domain")).unwrap();
    let names = [
        "get_latest_snapshot",
        "test_factory",
        "impact_score",
        "resolve_references",
        "acceptance_gate",
    ];
    let source = names
        .iter()
        .map(|name| format!("pub fn {name}() {{}}\n"))
        .collect::<String>();
    fs::write(root.path().join("src/domain/target.rs"), source).unwrap();
    fs::write(
        root.path().join("src/unrelated.rs"),
        "pub fn unrelated() {}\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let mut failures = Vec::new();
    for name in names {
        let pack = harness
            .agent_context("demo", &workspace, name, 4_000, &[])
            .unwrap();
        if pack["repo_map"]["scope_path"] != "src/domain" {
            failures.push(format!("identifier={name} map={}", pack["repo_map"]));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn repo_map_intent_matrix_preserves_task_words_outside_identifiers() {
    use crate::harness::harness_retrieval::{
        classify_repo_map_intent, query_needs_semantic_relationships, RepoMapIntent,
    };
    let mut cases = 0;
    for prefix in [
        "get_latest_snapshot",
        "test_factory",
        "impact_score",
        "resolve_references",
        "acceptance_gate",
    ] {
        for suffix in 0..10 {
            let identifier = format!("{prefix}_{suffix}");
            for (template, intent, semantic) in [
                ("{}", RepoMapIntent::Context, false),
                ("inspect {}", RepoMapIntent::Context, false),
                ("{} callers.", RepoMapIntent::EditToRipple, true),
                ("which tests verify {}", RepoMapIntent::CodeToTest, false),
                ("{}调用方", RepoMapIntent::EditToRipple, true),
                ("{} 测试", RepoMapIntent::CodeToTest, false),
                ("requirement for {}", RepoMapIntent::TraceToCode, false),
                ("show call graph of {}", RepoMapIntent::Context, true),
                ("引用{}", RepoMapIntent::EditToRipple, true),
                ("callers and tests for {}", RepoMapIntent::Context, true),
            ] {
                let query = template.replace("{}", &identifier);
                assert_eq!(classify_repo_map_intent(&query).intent, intent, "{query}");
                assert_eq!(
                    query_needs_semantic_relationships(&query),
                    semantic,
                    "{query}"
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 500);
}

#[test]
fn repo_map_localization_and_relationship_costs_stay_separate() {
    let root = tempfile::tempdir().unwrap();
    for directory in ["src/domain", "src/adapter", "src/noise"] {
        fs::create_dir_all(root.path().join(directory)).unwrap();
    }
    fs::write(
        root.path().join("src/domain/target.rs"),
        "pub fn target_feature() -> usize { 7 }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("src/adapter/caller.rs"),
        "pub fn invoke_target() -> usize {\n    target_feature()\n}\n",
    )
    .unwrap();
    for index in 0..500 {
        fs::write(
            root.path().join(format!("src/noise/item_{index:03}.rs")),
            format!("pub fn unrelated_{index}() {{}}\n"),
        )
        .unwrap();
    }
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    for pass in 0..2 {
        let local = harness
            .agent_context("demo", &workspace, "target_feature", 4_000, &[])
            .unwrap();
        assert_eq!(local["repo_map"]["scope_path"], "src/domain");
        assert_eq!(local["repo_map"]["files_indexed"], 1);
        let related = harness
            .agent_context(
                "demo",
                &workspace,
                "show callers of target_feature",
                4_000,
                &[],
            )
            .unwrap();
        assert!(
            related["repo_map"]["files_indexed"].as_u64().unwrap_or(u64::MAX) <= 3,
            "relationship lookup should parse the localized target plus exact-match supplements, not 500 noise files: {}",
            related["repo_map"]
        );
        let caller = related["repo_map"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["qualified_name"] == "invoke_target")
            .expect("cross-directory caller must survive 500 distractors");
        assert!(caller["relationships"]
            .as_array()
            .unwrap()
            .iter()
            .any(|relation| relation["relation"] == "caller_of_direct"));
        assert_eq!(related["repo_map"]["precision"], "syntax");
        assert_eq!(local["repo_map"]["cache_hit"], pass > 0);
        assert_eq!(
            related["repo_map"]["cache_hit"], true,
            "relationship lookup should reuse the localized graph built by the direct lookup"
        );
        println!("repo-map-fixture pass={pass} local_files={} relation_files={} local_build_ms={} relation_build_ms={}", local["repo_map"]["files_indexed"], related["repo_map"]["files_indexed"], local["repo_map"]["build_ms"], related["repo_map"]["build_ms"]);
    }
    fs::write(
        root.path().join("src/adapter/caller.rs"),
        "pub fn invoke_target() -> usize { 0 }\n",
    )
    .unwrap();
    let changed = harness
        .agent_context(
            "demo",
            &workspace,
            "show callers of target_feature",
            4_000,
            &[],
        )
        .unwrap();
    assert_eq!(
        changed["repo_map"]["cache_hit"], true,
        "the unchanged localized target graph should stay reusable"
    );
    assert!(
        !changed["repo_map"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["qualified_name"] == "invoke_target"
                && item["relationships"]
                    .as_array()
                    .is_some_and(|relations| !relations.is_empty())),
        "removed caller must not retain stale edges"
    );
}

#[test]
fn relationship_queries_recover_callers_beyond_the_base_repo_map_file_cap() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src/a_noise")).unwrap();
    fs::create_dir_all(root.path().join("src/adapter")).unwrap();
    fs::create_dir_all(root.path().join("src/z_domain")).unwrap();
    for index in 0..650 {
        fs::write(
            root.path().join(format!("src/a_noise/item_{index:03}.rs")),
            format!("pub fn unrelated_{index}() {{}}\n"),
        )
        .unwrap();
    }
    fs::write(
        root.path().join("src/z_domain/target.rs"),
        "pub fn target_feature() -> usize { 7 }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("src/adapter/caller.rs"),
        "pub fn invoke_target() -> usize { target_feature() }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();

    let pack = harness
        .agent_context(
            "demo",
            &workspace,
            "show callers of target_feature",
            4_000,
            &[],
        )
        .unwrap();
    assert_eq!(pack["repo_map"]["scan_truncated"], false);
    assert!(
        pack["repo_map"]["files_indexed"].as_u64().unwrap_or(u64::MAX) <= 3,
        "relationship retrieval should avoid parsing the 650-file noise prefix while still recovering the cross-directory caller: {}",
        pack["repo_map"]
    );
    let caller = pack["repo_map"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["qualified_name"] == "invoke_target")
        .expect("caller beyond the 600-file base graph must be recovered");
    assert!(caller["relationships"]
        .as_array()
        .unwrap()
        .iter()
        .any(|relation| relation["relation"] == "caller_of_direct"));
}

#[test]
fn graph_symbol_cap_preserves_exact_target_and_caller_relationship() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    let mut source = String::new();
    for index in 0..6_200 {
        source.push_str(&format!(
            "pub fn filler_{index:04}() -> usize {{ {index} }}\n"
        ));
    }
    source.push_str("pub fn target_feature() -> usize { 7 }\n");
    source.push_str("pub fn invoke_target() -> usize {\n    target_feature()\n}\n");
    fs::write(root.path().join("src/huge.rs"), source).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();

    let pack = harness
        .agent_context(
            "demo",
            &workspace,
            "show callers of target_feature",
            4_000,
            &[],
        )
        .unwrap();
    assert_eq!(pack["repo_map"]["scan_truncated"], false);
    assert_eq!(pack["repo_map"]["graph_truncated"], true);
    assert_eq!(
        pack["repo_map"]["items"][0]["qualified_name"], "target_feature",
        "exact target beyond the bounded graph prefix must be recovered: {}",
        pack["repo_map"]
    );
    let caller = pack["repo_map"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["qualified_name"] == "invoke_target")
        .expect("caller beyond the 5000-symbol graph budget must be recovered");
    assert!(caller["relationships"]
        .as_array()
        .unwrap()
        .iter()
        .any(|relation| relation["relation"] == "caller_of_direct"));

    let exact = harness
        .agent_context("demo", &workspace, "target_feature", 4_000, &[])
        .unwrap();
    assert_eq!(exact["repo_map"]["scan_truncated"], false);
    assert_eq!(exact["repo_map"]["graph_truncated"], true);
    assert_eq!(exact["repo_map"]["files_indexed"], 1);
    assert_eq!(
        exact["repo_map"]["items"][0]["qualified_name"], "target_feature",
        "priority recovery must stay local for a non-relationship lookup"
    );
}

#[test]
fn relationship_augmentation_keeps_partial_coverage_explicit() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src/a_callers")).unwrap();
    fs::create_dir_all(root.path().join("src/z_domain")).unwrap();
    for index in 0..620 {
        fs::write(
            root.path()
                .join(format!("src/a_callers/caller_{index:03}.rs")),
            format!("pub fn caller_{index:03}() -> usize {{\n    target_feature()\n}}\n"),
        )
        .unwrap();
    }
    fs::write(
        root.path().join("src/z_domain/target.rs"),
        "pub fn target_feature() -> usize { 7 }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();

    let pack = harness
        .agent_context(
            "demo",
            &workspace,
            "show callers of target_feature",
            4_000,
            &[],
        )
        .unwrap();
    assert_eq!(pack["repo_map"]["scan_truncated"], true);
    assert_eq!(pack["repo_map"]["truncated"], true);
    assert!(pack["readiness"]["advisories"]
        .as_array()
        .unwrap()
        .iter()
        .any(|advisory| advisory == "repo_map_truncated"));
    assert!(pack["repo_map"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["relationships"]
            .as_array()
            .is_some_and(|relations| relations
                .iter()
                .any(|relation| relation["relation"] == "caller_of_direct"))));
}

#[test]
fn code_to_test_recovers_tests_beyond_the_base_file_cap() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src/a_noise")).unwrap();
    fs::create_dir_all(root.path().join("src/z_domain")).unwrap();
    fs::create_dir_all(root.path().join("tests")).unwrap();
    for index in 0..650 {
        fs::write(
            root.path().join(format!("src/a_noise/item_{index:03}.rs")),
            format!("pub fn unrelated_{index}() {{}}\n"),
        )
        .unwrap();
    }
    fs::write(
        root.path().join("src/z_domain/target.rs"),
        "pub fn target_feature() -> usize { 7 }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("tests/contract.rs"),
        "#[test]\nfn validates_contract() {\n    assert_eq!(target_feature(), 7);\n}\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();

    let pack = harness
        .agent_context(
            "demo",
            &workspace,
            "which tests verify target_feature",
            4_000,
            &[],
        )
        .unwrap();
    let repo_map = &pack["repo_map"];
    assert!(
        repo_map["scan_truncated"].as_bool().unwrap_or(false)
            || repo_map["scope_path"]
                .as_str()
                .is_some_and(|path| path != "."),
        "large-repository recovery must either report base truncation or narrow the repo-map scope: {repo_map}"
    );
    assert!(
        repo_map["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["qualified_name"] == "validates_contract"),
        "test beyond the 600-file base graph must be recovered: {}",
        pack["repo_map"]
    );
}

#[test]
fn warm_context_preserves_all_explicit_literal_targets() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src/entry")).unwrap();
    fs::create_dir_all(root.path().join("src/worker")).unwrap();
    fs::write(
        root.path().join("src/entry/mod.rs"),
        "pub fn feature_entry() -> usize { 1 }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("src/worker/mod.rs"),
        "pub fn batch_worker() -> usize { 2 }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    for query in [
        "feature_entry batch_worker",
        "feature_entry batch_worker",
        "batch_worker feature_entry",
    ] {
        let pack = harness
            .agent_context("demo", &workspace, query, 4_000, &[])
            .unwrap();
        let targets = pack["targets"].as_array().unwrap();
        for expected in ["feature_entry", "batch_worker"] {
            assert!(
                targets
                    .iter()
                    .any(|target| target["qualified_name"] == expected),
                "missing {expected} for {query}: {targets:?}"
            );
        }
        assert_eq!(pack["readiness"]["parallelism"]["candidate_lanes"], 2);
        assert_eq!(
            pack["readiness"]["parallelism"]["strategy"],
            "parallel_required"
        );
    }
}

#[test]
fn partially_warm_context_discovers_new_literal_targets() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src/entry")).unwrap();
    fs::write(
        root.path().join("src/entry/mod.rs"),
        "pub fn feature_entry() -> usize { 1 }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    harness
        .agent_context("demo", &workspace, "feature_entry", 4_000, &[])
        .unwrap();
    fs::create_dir_all(root.path().join("src/worker")).unwrap();
    fs::write(
        root.path().join("src/worker/mod.rs"),
        "pub fn batch_worker() -> usize { 2 }\n",
    )
    .unwrap();
    let pack = harness
        .agent_context("demo", &workspace, "feature_entry batch_worker", 4_000, &[])
        .unwrap();
    let targets = pack["targets"].as_array().unwrap();
    assert!(targets
        .iter()
        .any(|target| target["qualified_name"] == "feature_entry"));
    assert!(
        targets
            .iter()
            .any(|target| target["qualified_name"] == "batch_worker"),
        "partial cache coverage must not suppress discovery: {targets:?}"
    );
    assert_eq!(pack["readiness"]["parallelism"]["candidate_lanes"], 2);
}

#[test]
fn concurrent_repo_map_requests_share_one_validation_flight() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn entry() { helper(); }\nfn helper() {}\n",
    )
    .unwrap();

    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let cache_key = (workspace.root().to_path_buf(), "src".to_owned());
    let flight = harness.repo_map_flight(&cache_key).unwrap();
    let guard = flight
        .gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let generation_before = flight.generation.load(Ordering::Acquire);
    let entrants_before = flight.entrants.load(Ordering::Acquire);

    let results = std::thread::scope(|scope| {
        let (started_tx, started_rx) = mpsc::channel();
        let mut handles = Vec::new();
        for _ in 0..2 {
            let started_tx = started_tx.clone();
            let harness = &harness;
            let workspace = &workspace;
            handles.push(scope.spawn(move || {
                started_tx.send(()).unwrap();
                harness.repo_map_graph("demo", workspace, "src").unwrap().1
            }));
        }
        drop(started_tx);
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let deadline = Instant::now() + Duration::from_secs(2);
        while flight.entrants.load(Ordering::Acquire) < entrants_before + 2 {
            assert!(
                Instant::now() < deadline,
                "repo map requests did not join the shared flight"
            );
            std::thread::yield_now();
        }
        drop(guard);
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });

    assert_eq!(
        flight.generation.load(Ordering::Acquire),
        generation_before + 2,
        "overlapping requests should perform one complete freshness validation"
    );
    assert_eq!(results.iter().filter(|&&cache_hit| !cache_hit).count(), 1);
    assert_eq!(results.iter().filter(|&&cache_hit| cache_hit).count(), 1);

    let generation_before_hot_call = flight.generation.load(Ordering::Acquire);
    let (_, cache_hit) = harness.repo_map_graph("demo", &workspace, "src").unwrap();
    assert!(cache_hit);
    assert_eq!(
        flight.generation.load(Ordering::Acquire),
        generation_before_hot_call + 2,
        "a non-overlapping request must still perform its own complete freshness validation"
    );

    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn entry() { changed(); }\nfn changed() {}\n",
    )
    .unwrap();
    let (_, cache_hit_after_edit) = harness.repo_map_graph("demo", &workspace, "src").unwrap();
    assert!(
        !cache_hit_after_edit,
        "external source edits must invalidate the cached repo map without a TTL window"
    );
}

#[cfg(unix)]
#[test]
fn repo_map_cache_rejects_same_size_same_mtime_external_rewrite() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    let path = root.path().join("src/lib.rs");
    fs::write(&path, "pub fn alpha() {}\n").unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let (initial, initial_hit) = harness.repo_map_graph("demo", &workspace, "src").unwrap();
    assert!(!initial_hit);
    assert!(initial
        .graph
        .nodes
        .values()
        .any(|node| node.label == "alpha"));

    fs::write(&path, "pub fn bravo() {}\n").unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();

    let (fresh, cache_hit) = harness.repo_map_graph("demo", &workspace, "src").unwrap();
    assert!(
        !cache_hit,
        "ctime/inode-aware metadata must reject stale repo-map reuse"
    );
    assert!(fresh.graph.nodes.values().any(|node| node.label == "bravo"));
    assert!(!fresh.graph.nodes.values().any(|node| node.label == "alpha"));
}

#[test]
fn shared_repo_map_tail_validation_rejects_midflight_source_changes() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn stable() {}\n").unwrap();

    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    harness.repo_map_graph("demo", &workspace, "src").unwrap();

    let cache_key = (workspace.root().to_path_buf(), "src".to_owned());
    let expected_fingerprint = harness
        .repo_map_cache
        .lock()
        .unwrap()
        .get(&cache_key)
        .unwrap()
        .fingerprint;
    let flight = harness.repo_map_flight(&cache_key).unwrap();
    let observed_generation = flight.generation.load(Ordering::Acquire);
    let owner =
        super::harness_cache_flight::ValidationParticipant::join(&flight, observed_generation);
    let follower =
        super::harness_cache_flight::ValidationParticipant::join(&flight, observed_generation);
    assert!(owner.has_coalescible_peer());

    fs::write(root.path().join("src/new.rs"), "pub fn newly_added() {}\n").unwrap();
    let error = super::harness_repo_map::ensure_shared_repo_map_fingerprint_current(
        &owner,
        &workspace,
        "src",
        expected_fingerprint,
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("repo map changed during shared validation"));
    drop(follower);
    assert!(!owner.has_coalescible_peer());
}

#[test]
fn failed_owner_validation_is_not_reused_by_waiting_follower() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn stable() {}\n").unwrap();

    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    harness.repo_map_graph("demo", &workspace, "src").unwrap();

    let cache_key = (workspace.root().to_path_buf(), "src".to_owned());
    let flight = harness.repo_map_flight(&cache_key).unwrap();
    let guard = flight
        .gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let generation_before = flight.generation.load(Ordering::Acquire);
    assert_eq!(
        flight.successful_generation.load(Ordering::Acquire),
        generation_before
    );
    let entrants_before = flight.entrants.load(Ordering::Acquire);

    let cache_hit = std::thread::scope(|scope| {
        let handle = scope.spawn(|| harness.repo_map_graph("demo", &workspace, "src").unwrap().1);
        let deadline = Instant::now() + Duration::from_secs(2);
        while flight.entrants.load(Ordering::Acquire) < entrants_before + 1 {
            assert!(
                Instant::now() < deadline,
                "follower did not wait on the simulated owner"
            );
            std::thread::yield_now();
        }

        // Simulate an owner that started and then failed validation: generation
        // advances back to even, but successful_generation intentionally does not.
        flight.generation.fetch_add(1, Ordering::AcqRel);
        flight.generation.fetch_add(1, Ordering::Release);
        drop(guard);
        handle.join().unwrap()
    });

    assert!(
        cache_hit,
        "the follower may reuse cache only after revalidating it"
    );
    assert_eq!(
        flight.generation.load(Ordering::Acquire),
        generation_before + 4,
        "failed owner validation must force one follower validation"
    );
    assert_eq!(
        flight.successful_generation.load(Ordering::Acquire),
        generation_before + 4
    );
}

#[test]
fn invalidation_prevents_late_repo_map_validation_from_becoming_reusable() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn stable() {}\n").unwrap();

    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    harness.repo_map_graph("demo", &workspace, "src").unwrap();

    let cache_key = (workspace.root().to_path_buf(), "src".to_owned());
    let flight = harness.repo_map_flight(&cache_key).unwrap();
    let generation_before = flight.generation.load(Ordering::Acquire);
    let revision_before = flight.invalidation_revision.load(Ordering::Acquire);
    {
        let mut validation = super::harness_cache_flight::ValidationGuard::begin(&flight);
        harness.invalidate_code_file(&workspace, "src/lib.rs");
        validation.mark_success();
    }

    assert_eq!(
        flight.invalidation_revision.load(Ordering::Acquire),
        revision_before + 1
    );
    assert_eq!(
        flight.generation.load(Ordering::Acquire),
        generation_before + 2
    );
    assert_ne!(
        flight.successful_generation.load(Ordering::Acquire),
        generation_before + 2,
        "an invalidated validation must never be advertised as reusable"
    );
    assert!(harness.repo_map_cache.lock().unwrap().is_empty());

    let (_, cache_hit) = harness.repo_map_graph("demo", &workspace, "src").unwrap();
    assert!(!cache_hit, "invalidation must force a fresh repo map build");
}

#[test]
fn aggressive_memory_trim_invalidates_active_repo_map_validation() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn stable() {}\n").unwrap();

    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let cache_key = (workspace.root().to_path_buf(), "src".to_owned());
    let flight = harness.repo_map_flight(&cache_key).unwrap();
    let revision_before = flight.invalidation_revision.load(Ordering::Acquire);
    let mut validation = super::harness_cache_flight::ValidationGuard::begin(&flight);
    harness.trim_memory(true);
    validation.mark_success();
    drop(validation);

    assert_eq!(
        flight.invalidation_revision.load(Ordering::Acquire),
        revision_before + 1
    );
    assert_ne!(
        flight.successful_revision.load(Ordering::Acquire),
        revision_before + 1,
        "aggressive trim must reject late repo map publication"
    );
}

#[test]
fn request_joining_active_validation_rechecks_external_edits() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn old() {}\n").unwrap();

    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let (_, initial_hit) = harness.repo_map_graph("demo", &workspace, "src").unwrap();
    assert!(!initial_hit);

    let cache_key = (workspace.root().to_path_buf(), "src".to_owned());
    let flight = harness.repo_map_flight(&cache_key).unwrap();
    let guard = flight
        .gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let generation_before = flight.generation.load(Ordering::Acquire);
    assert_eq!(generation_before % 2, 0);

    // Simulate an owner whose fingerprint scan has already started. The source
    // then changes before a follower arrives. That follower observes the odd
    // generation and must run a new validation after the owner completes.
    flight.generation.fetch_add(1, Ordering::AcqRel);
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn externally_changed_and_longer() {}\n",
    )
    .unwrap();

    let entrants_before = flight.entrants.load(Ordering::Acquire);
    let cache_hit = std::thread::scope(|scope| {
        let handle = scope.spawn(|| harness.repo_map_graph("demo", &workspace, "src").unwrap().1);
        let deadline = Instant::now() + Duration::from_secs(2);
        while flight.entrants.load(Ordering::Acquire) < entrants_before + 1 {
            assert!(
                Instant::now() < deadline,
                "follower did not join active validation"
            );
            std::thread::yield_now();
        }
        flight.generation.fetch_add(1, Ordering::Release);
        drop(guard);
        handle.join().unwrap()
    });

    assert!(
        !cache_hit,
        "a caller that arrives after validation starts must not reuse a pre-edit fingerprint"
    );
    assert_eq!(
        flight.generation.load(Ordering::Acquire),
        generation_before + 4
    );
}
