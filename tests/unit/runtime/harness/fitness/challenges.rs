//! Counterexamples are deliberately separate from the frozen 60-case corpus.
use super::corpus::{corpus, Case, Gold, Identity};
use super::scoring::{complete_body, delivered, score};
use crate::harness::ToolHarness;
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn selection(pack: &Value) -> Value {
    let targets: Vec<_> = pack["targets"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| json!([item["path"], item["qualified_name"]]))
        .collect();
    let bodies: Vec<_> = pack["hot_source"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            json!({"path":item["path"],"symbol":item["qualified_name"],
            "selection":item["selection"],"truncated":item["body"]["truncated"]})
        })
        .collect();
    json!({"targets":targets,"bodies":bodies,"map":pack["repo_map"]["items"]})
}

#[test]
fn engineering_fitness_challenge_warm_preserves_delivered_bodies() {
    let mut losses = Vec::new();
    for case in corpus() {
        let (_root, workspace) = case.instantiate();
        let harness = ToolHarness::new(4).unwrap();
        let cold = harness
            .agent_context("fitness", &workspace, &case.query, 4_000, &[])
            .unwrap();
        let warm = harness
            .agent_context("fitness", &workspace, &case.query, 4_000, &[])
            .unwrap();
        let lost: Vec<_> = case
            .required
            .iter()
            .filter(|gold| complete_body(&cold, &case, gold) && !complete_body(&warm, &case, gold))
            .map(|gold| gold.identity.clone())
            .collect();
        let before = score(&cold, &case);
        let after = score(&warm, &case);
        if !lost.is_empty()
            || after.required_hits < before.required_hits
            || (before.all_required_edit_inputs && !after.all_required_edit_inputs)
        {
            losses.push(json!({"case":case.id,"query":case.query,"lost":lost,
                "cold":selection(&cold),"warm":selection(&warm)}));
        }
    }
    println!("FITNESS_WARM_LOSSES {}", json!(losses));
    assert!(
        losses.is_empty(),
        "unchanged warm context lost delivered Gold evidence"
    );
}

fn network_case(callers: usize) -> Case {
    let source = "pub fn decode_header() -> usize {\n    7\n}\npub fn parse_frame() -> usize {\n    decode_header()\n}\n";
    let mut files = BTreeMap::from([("src/protocol.rs".into(), source.into())]);
    let mut client_source = String::new();
    for index in 0..callers {
        client_source.push_str(&format!(
            "pub fn ingress_{index}() -> usize {{\n    parse_frame()\n}}\n"
        ));
    }
    files.insert("src/clients.rs".into(), client_source);
    for index in 0..24 {
        files.insert(
            format!("src/detached_{index:02}.rs"),
            format!("pub fn opaque_{index}() -> usize {{\n    {index}\n}}\n"),
        );
    }
    // Nonzero degree alone is not evidence of a relationship to the task.
    files.insert(
        "src/island.rs".into(),
        "pub fn island_entry() {\n    island_helper();\n}\nfn island_helper() {}\n".into(),
    );
    Case {
        id: "independent-network-counterexample".into(),
        language: "rust".into(),
        category: "counterexample".into(),
        query: "inspect parse_frame".into(),
        files,
        required: vec![Gold {
            identity: Identity::new("src/protocol.rs", "parse_frame"),
            fragment: "pub fn parse_frame() -> usize {\n    decode_header()\n}\n".into(),
        }],
        useful: vec![Identity::new("src/protocol.rs", "decode_header")],
        writable: true,
        no_answer: false,
    }
}

#[test]
fn engineering_fitness_challenge_no_disconnected_filler() {
    let case = network_case(1);
    let (_root, workspace) = case.instantiate();
    let harness = ToolHarness::new(4).unwrap();
    let mut failures = Vec::new();
    for budget in [2_000, 4_000] {
        for phase in 0..2 {
            let pack = harness
                .agent_context("fitness", &workspace, &case.query, budget, &[])
                .unwrap();
            let identities = delivered(&pack);
            assert!(identities.contains(&case.required[0].identity));
            let detached: Vec<_> = identities
                .iter()
                .filter(|id| id.path.contains("detached_") || id.path == "src/island.rs")
                .collect();
            if !detached.is_empty() {
                failures.push(json!({"budget":budget,"phase":phase,"detached":detached}));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "disconnected fillers: {}",
        json!(failures)
    );
}

#[test]
fn engineering_fitness_challenge_4k_delivers_all_required_identities() {
    let mut misses = Vec::new();
    for case in corpus()
        .into_iter()
        .filter(|case| !case.required.is_empty())
    {
        let (_root, workspace) = case.instantiate();
        let harness = ToolHarness::new(4).unwrap();
        for phase in ["cold", "warm"] {
            if phase == "warm" {
                harness
                    .agent_context("fitness", &workspace, &case.query, 4_000, &[])
                    .unwrap();
            }
            let pack = harness
                .agent_context("fitness", &workspace, &case.query, 4_000, &[])
                .unwrap();
            let identities = delivered(&pack);
            let missing = case
                .required
                .iter()
                .filter(|gold| !identities.contains(&gold.identity))
                .map(|gold| gold.identity.clone())
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                misses.push(json!({
                    "case": case.id,
                    "phase": phase,
                    "query": case.query,
                    "missing": missing,
                    "selection": selection(&pack),
                }));
            }
        }
    }
    assert!(
        misses.is_empty(),
        "4k context missed required identities: {}",
        json!(misses)
    );
}

#[test]
fn engineering_fitness_challenge_nine_real_callers_are_not_six() {
    let mut case = network_case(9);
    case.query = "find callers of parse_frame".into();
    let (_root, workspace) = case.instantiate();
    let harness = ToolHarness::new(4).unwrap();
    for phase in 0..2 {
        let pack = harness
            .agent_context("fitness", &workspace, &case.query, 8_000, &[])
            .unwrap();
        let identities = delivered(&pack);
        let missing: Vec<_> = (0..9)
            .map(|index| Identity::new("src/clients.rs", &format!("ingress_{index}")))
            .filter(|id| !identities.contains(id))
            .collect();
        assert!(
            missing.is_empty(),
            "phase={phase}: real callers lost to a focused count cap: {missing:?}"
        );
    }
}

#[test]
fn engineering_fitness_rust_module_shell_stays_out_of_delivery() {
    let case = super::corpus::base_case("rust");
    let (_root, workspace) = case.instantiate();
    let pack = ToolHarness::new(4)
        .unwrap()
        .agent_context("fitness", &workspace, &case.query, 4_000, &[])
        .unwrap();
    let shell = pack["repo_map"]["items"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|item| item["path"] == "src/lib.rs" && item["qualified_name"] == "session");
    assert!(
        shell.is_none(),
        "generic module/re-export filler must remain Non-Gold and stay out of delivery: {}",
        selection(&pack)
    );
    let delivered = delivered(&pack);
    assert!(delivered.contains(&case.required[0].identity));
    assert!(delivered.contains(&case.required[1].identity));
}

#[test]
fn engineering_fitness_challenge_exact_name_prefers_production_over_test_duplicate() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::create_dir_all(root.path().join("tests")).unwrap();
    std::fs::write(
        root.path().join("src/owner.rs"),
        "pub fn cleanup_if_owner(old: u64, current: u64) -> bool { old == current }\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("tests/owner.rs"),
        "#[test]\nfn cleanup_if_owner() { assert!(true); }\n",
    )
    .unwrap();
    let workspace = crate::workspace::Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();

    for budget in [1_000, 2_000] {
        let pack = harness
            .agent_context(
                "fitness",
                &workspace,
                "inspect cleanup_if_owner implementation",
                budget,
                &[],
            )
            .unwrap();
        assert_eq!(
            pack["hot_source"][0]["path"],
            "src/owner.rs",
            "generic implementation lookup must not let a test duplicate outrank production: {}",
            selection(&pack)
        );
    }
}

#[test]
fn engineering_fitness_challenge_large_fixture_never_displaces_required_body() {
    let target =
        "pub fn cleanup_if_owner(old: u64, current: u64) -> bool {\n    old == current\n}\n";
    let mut fixture =
        String::from("#[test]\nfn cleanup_if_owner_fixture() {\n    let mut total = 0usize;\n");
    for index in 0..320 {
        fixture.push_str(&format!(
            "    total += {index}; // stale ownership replacement cleanup guard fixture\n"
        ));
    }
    fixture.push_str("    assert!(total > 0);\n}\n");
    let case = Case {
        id: "large-fixture-counterexample".into(),
        language: "rust".into(),
        category: "counterexample".into(),
        query: "Find the ownership guard that prevents stale replacement cleanup".into(),
        files: BTreeMap::from([
            ("src/owner.rs".into(), target.into()),
            ("tests/fixtures/owner.rs".into(), fixture),
        ]),
        required: vec![Gold {
            identity: Identity::new("src/owner.rs", "cleanup_if_owner"),
            fragment: target.into(),
        }],
        useful: vec![],
        writable: true,
        no_answer: false,
    };
    let (_root, workspace) = case.instantiate();
    let harness = ToolHarness::new(4).unwrap();

    for budget in [1_000, 2_000] {
        let pack = harness
            .agent_context("fitness", &workspace, &case.query, budget, &[])
            .unwrap();
        assert!(
            complete_body(&pack, &case, &case.required[0]),
            "budget={budget}: large lexical fixture displaced required production body: {}",
            selection(&pack)
        );
        let hot = pack["hot_source"].as_array().unwrap();
        let source = hot
            .iter()
            .position(|item| item["path"] == "src/owner.rs")
            .expect("required source must be delivered");
        if let Some(fixture) = hot
            .iter()
            .position(|item| item["path"] == "tests/fixtures/owner.rs")
        {
            assert!(
                source < fixture,
                "budget={budget}: fixture body outranked production body: {}",
                selection(&pack)
            );
        }
    }
}

#[test]
fn engineering_fitness_challenge_relationship_context_stops_after_two_hops() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("src/chain.rs"),
        "pub fn target_feature() -> usize { 7 }\n\
         pub fn direct_caller() -> usize { target_feature() }\n\
         pub fn second_hop() -> usize { direct_caller() }\n\
         pub fn third_hop_noise() -> usize { second_hop() }\n",
    )
    .unwrap();
    let workspace = crate::workspace::Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();

    let pack = harness
        .agent_context(
            "fitness",
            &workspace,
            "show callers of target_feature",
            4_000,
            &[],
        )
        .unwrap();
    let items = pack["repo_map"]["items"].as_array().unwrap();
    let names = items
        .iter()
        .filter_map(|item| item["qualified_name"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(
        names.contains("target_feature"),
        "repo_map={}",
        pack["repo_map"]
    );
    assert!(
        names.contains("direct_caller"),
        "repo_map={}",
        pack["repo_map"]
    );
    assert!(
        names.contains("second_hop"),
        "repo_map={}",
        pack["repo_map"]
    );
    assert!(
        !names.contains("third_hop_noise"),
        "three-hop transitive noise must not consume relationship context: {}",
        pack["repo_map"]
    );
}

#[test]
fn engineering_fitness_challenge_natural_refresh_prefers_requested_refresh_target() {
    let case = corpus()
        .into_iter()
        .find(|case| case.id == "rust-refresh-natural")
        .expect("frozen corpus must include rust-refresh-natural");
    let expected = case.required[0].identity.clone();
    let (_root, workspace) = case.instantiate();
    let harness = ToolHarness::new(4).unwrap();

    for budget in [1_000, 4_000] {
        let pack = harness
            .agent_context("fitness", &workspace, &case.query, budget, &[])
            .unwrap();
        let identities = delivered(&pack);
        assert_eq!(
            identities.first(),
            Some(&expected),
            "budget={budget}: natural-language refresh target must outrank related/test noise: {}",
            selection(&pack)
        );
    }
}
