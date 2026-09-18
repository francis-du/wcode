//! Independent tight-budget cases; never inserted into the frozen 60 tasks.
use super::corpus::{Case, Gold, Identity};
use super::scoring::{complete_body, score};
use crate::harness::ToolHarness;
use std::collections::BTreeMap;

fn triple(separate: bool) -> Case {
    let mut case = Case {
        id: format!("tiny-three-{separate}"),
        language: "rust".into(),
        category: "tight-independent".into(),
        query: "inspect encode_frame decode_frame finish_frame".into(),
        files: BTreeMap::new(),
        required: vec![],
        useful: vec![],
        writable: true,
        no_answer: false,
    };
    for name in ["encode_frame", "decode_frame", "finish_frame"] {
        let path = if separate {
            format!("src/{name}.rs")
        } else {
            "src/codec.rs".into()
        };
        let source = format!("pub fn {name}() -> bool {{\n    true\n}}\n");
        case.files
            .entry(path.clone())
            .or_default()
            .push_str(&source);
        case.required.push(Gold {
            identity: Identity::new(&path, name),
            fragment: source,
        });
    }
    case
}

#[test]
fn engineering_fitness_tight_three_short_bodies_keep_source_and_sha() {
    for separate in [false, true] {
        let case = triple(separate);
        let (_root, workspace) = case.instantiate();
        let harness = ToolHarness::new(4).unwrap();
        for phase in ["cold", "warm"] {
            let pack = harness
                .agent_context("fitness", &workspace, &case.query, 1_000, &[])
                .unwrap();
            let result = score(&pack, &case);
            assert!(result.within_budget);
            assert!(
                result.all_required_edit_inputs,
                "separate={separate}/{phase}: {result:?}\n{pack}"
            );
        }
    }
}

#[test]
fn engineering_fitness_tight_explicit_module_is_not_filtered_out() {
    let mut case = triple(false);
    case.files
        .insert("src/lib.rs".into(), "pub mod packet_codec;\n".into());
    case.query = "inspect packet_codec encode_frame".into();
    let (_root, workspace) = case.instantiate();
    for budget in [1_000, 2_000] {
        let pack = ToolHarness::new(4)
            .unwrap()
            .agent_context("fitness", &workspace, &case.query, budget, &[])
            .unwrap();
        assert!(
            pack["hot_source"].as_array().unwrap().iter().any(|body| {
                body["qualified_name"] == "packet_codec"
                    && body["body"]["content"] == "pub mod packet_codec;"
            }),
            "explicit module lost at {budget}: {pack}"
        );
    }
}

#[test]
fn engineering_fitness_tight_one_k_caller_pair_is_edit_ready() {
    let mut case = triple(true);
    case.files.clear();
    case.required.clear();
    case.query = "Find callers of parse_message and provide source".into();
    for (path, name, body) in [
        ("src/parser.rs", "parse_message", "true"),
        ("src/dispatch.rs", "dispatch_message", "parse_message()"),
    ] {
        let source = format!("pub fn {name}() -> bool {{\n    {body}\n}}\n");
        case.files.insert(path.into(), source.clone());
        case.required.push(Gold {
            identity: Identity::new(path, name),
            fragment: source,
        });
    }
    let (_root, workspace) = case.instantiate();
    let harness = ToolHarness::new(4).unwrap();
    for phase in ["cold", "warm"] {
        let pack = harness
            .agent_context("fitness", &workspace, &case.query, 1_000, &[])
            .unwrap();
        let observed = score(&pack, &case);
        assert!(observed.within_budget);
        assert!(
            observed.all_required_edit_inputs,
            "{phase}: {observed:?}\n{pack}"
        );
    }
}

#[test]
fn engineering_fitness_tight_escaped_query_never_buys_extra_delivered_bytes() {
    let case = triple(true);
    let (_root, workspace) = case.instantiate();
    let harness = ToolHarness::new(4).unwrap();
    for suffix in [" \" ", " \\\\ ", "\r\n\t", " 你好🚀 "] {
        for repeat in [1, 7, 19] {
            let query = format!("{} {}", case.query, suffix.repeat(repeat));
            let pack = harness
                .agent_context("fitness", &workspace, &query, 1_000, &[])
                .unwrap();
            assert_eq!(pack["budget"], 1_000);
            assert!(pack.get("query").is_none());
            assert!(serde_json::to_vec(&pack).unwrap().len() <= 4_000);
            for body in pack["hot_source"].as_array().unwrap() {
                assert!(
                    pack["files"].as_array().unwrap().iter().any(|file| {
                        file["path"] == body["path"] && file["sha256"] == body["sha256"]
                    }),
                    "orphan source: {body}"
                );
            }
        }
    }
}

#[test]
fn engineering_fitness_tight_same_named_source_and_test_remain_distinct() {
    let mut case = triple(false);
    case.files.clear();
    case.required.clear();
    case.query = "inspect decode_frame".into();
    for (path, body) in [("src/codec.rs", "true"), ("tests/codec.rs", "false")] {
        let source = format!("pub fn decode_frame() -> bool {{\n    {body}\n}}\n");
        case.files.insert(path.into(), source.clone());
        case.required.push(Gold {
            identity: Identity::new(path, "decode_frame"),
            fragment: source,
        });
    }
    let (_root, workspace) = case.instantiate();
    let harness = ToolHarness::new(4).unwrap();
    for _ in 0..2 {
        let pack = harness
            .agent_context("fitness", &workspace, &case.query, 2_000, &[])
            .unwrap();
        assert!(score(&pack, &case).all_required_edit_inputs, "{pack}");
    }
}

#[test]
fn engineering_fitness_tight_long_primary_stays_honestly_truncated() {
    let mut case = triple(false);
    let source = format!(
        "pub fn encode_frame() {{\r\n{}\r\n}}\r\n",
        "    let text = \"你好🚀\";\r\n".repeat(160)
    );
    case.files.insert("src/codec.rs".into(), source.clone());
    case.query = "inspect encode_frame".into();
    case.required.truncate(1);
    case.required[0].fragment = source;
    let (_root, workspace) = case.instantiate();
    let pack = ToolHarness::new(4)
        .unwrap()
        .agent_context("fitness", &workspace, &case.query, 1_000, &[])
        .unwrap();
    assert!(score(&pack, &case).within_budget);
    assert!(!complete_body(&pack, &case, &case.required[0]));
    let body = &pack["hot_source"][0];
    assert_eq!(body["body"]["truncated"], true);
    let text = body["body"]["content"].as_str().unwrap();
    assert!(!text.is_empty());
    assert!(case.files["src/codec.rs"].starts_with(text));
}
