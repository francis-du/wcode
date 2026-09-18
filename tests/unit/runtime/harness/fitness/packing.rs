//! Body-delivery counterexamples; independent of the frozen evaluation corpus.
use super::corpus::{corpus, Case, Gold, Identity};
use super::scoring::{complete_body, score};
use crate::harness::ToolHarness;
use std::collections::BTreeMap;

fn packet_case(language: &str) -> Case {
    let (extension, decoder, caller) = match language {
        "rust" => (
            "rs",
            "pub fn decode_frame(size: usize) -> bool {\n    size > 0\n}\n",
            "pub fn serve_packet(size: usize) -> bool {\n    decode_frame(size)\n}\n",
        ),
        "go" => (
            "go",
            "func decode_frame(size int) bool {\n    return size > 0\n}\n",
            "func serve_packet(size int) bool {\n    return decode_frame(size)\n}\n",
        ),
        "typescript" => (
            "ts",
            "export function decode_frame(size: number): boolean {\n    return size > 0;\n}\n",
            "export function serve_packet(size: number): boolean {\n    return decode_frame(size);\n}\n",
        ),
        "python" => (
            "py",
            "def decode_frame(size):\n    return size > 0\n",
            "def serve_packet(size):\n    return decode_frame(size)\n",
        ),
        _ => unreachable!(),
    };
    let mut case = Case {
        id: format!("packet-body-{language}"),
        language: language.into(),
        category: "body-packing-counterexample".into(),
        query: "Find callers of decode_frame and provide their source".into(),
        files: BTreeMap::new(),
        required: Vec::new(),
        useful: Vec::new(),
        writable: true,
        no_answer: false,
    };
    for (stem, symbol, source) in [
        ("decoder", "decode_frame", decoder),
        ("server", "serve_packet", caller),
    ] {
        let path = format!("src/{stem}.{extension}");
        let prefix = if language == "go" {
            "package packet\n\n"
        } else {
            ""
        };
        case.files.insert(path.clone(), format!("{prefix}{source}"));
        case.required.push(Gold {
            identity: Identity::new(&path, symbol),
            fragment: source.into(),
        });
    }
    case
}

#[test]
fn engineering_fitness_packing_2k_includes_cross_file_caller_bodies() {
    let mut failures = Vec::new();
    for language in ["rust", "go", "typescript", "python"] {
        let case = packet_case(language);
        let (_root, workspace) = case.instantiate();
        let harness = ToolHarness::new(4).unwrap();
        for phase in ["cold", "warm"] {
            let pack = harness
                .agent_context("packing", &workspace, &case.query, 2_000, &[])
                .unwrap();
            let observed = score(&pack, &case);
            assert_eq!(pack["budget"], 2_000);
            assert!(observed.within_budget);
            if !observed.all_required_edit_inputs {
                failures.push(format!(
                    "{language}/{phase}: {observed:?}\n{}",
                    pack["hot_source"]
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn engineering_fitness_packing_2k_warm_preserves_each_gold_body() {
    for case in corpus() {
        let (_root, workspace) = case.instantiate();
        let harness = ToolHarness::new(4).unwrap();
        let cold = harness
            .agent_context("packing", &workspace, &case.query, 2_000, &[])
            .unwrap();
        let warm = harness
            .agent_context("packing", &workspace, &case.query, 2_000, &[])
            .unwrap();
        let before = score(&cold, &case);
        let after = score(&warm, &case);
        assert!(after.within_budget);
        assert!(after.required_hits >= before.required_hits, "{}", case.id);
        assert!(
            !before.all_required_edit_inputs || after.all_required_edit_inputs,
            "{}",
            case.id
        );
        for gold in &case.required {
            assert!(
                !complete_body(&cold, &case, gold) || complete_body(&warm, &case, gold),
                "{} lost {:?}",
                case.id,
                gold.identity
            );
        }
    }
}

#[test]
fn engineering_fitness_packing_tight_source_keeps_matching_file_sha() {
    for slots in [1, 4] {
        let mut case = packet_case("rust");
        case.query = "inspect decode_frame serve_packet".into();
        for gold in &case.required {
            case.files.insert(
                gold.identity.path.clone(),
                format!(
                    "pub fn {}() {{\r\n{}\r\n}}\r\n",
                    gold.identity.symbol,
                    "    let text = \"你好🚀\";\r\n".repeat(60)
                ),
            );
        }
        let (_root, workspace) = case.instantiate();
        let harness = ToolHarness::new(slots).unwrap();
        for budget in [1_000, 1_200, 1_600, 2_000, 3_000, 4_000] {
            let pack = harness
                .agent_context("packing", &workspace, &case.query, budget, &[])
                .unwrap();
            assert!(score(&pack, &case).within_budget);
            for source in pack["hot_source"].as_array().unwrap() {
                let path = source["path"].as_str().unwrap();
                let original = workspace.read_file(path, 1, None).unwrap();
                let content = source["body"]["content"].as_str().unwrap();
                assert!(original.content.starts_with(content));
                assert_eq!(source["sha256"], original.sha256);
                assert!(
                    pack["files"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|file| file["path"] == source["path"]
                            && file["sha256"] == source["sha256"]),
                    "budget={budget} slots={slots}: orphan body {path}"
                );
            }
        }
    }
}

#[test]
fn engineering_fitness_packing_related_expansion_keeps_ranked_bodies() {
    let mut case = packet_case("rust");
    let source = "pub fn route_packet(size: usize) -> bool {\n    decode_frame(size)\n}\n";
    case.files.insert("src/router.rs".into(), source.into());
    case.required.push(Gold {
        identity: Identity::new("src/router.rs", "route_packet"),
        fragment: source.into(),
    });
    case.query = "Find callers of decode_frame in packet flow".into();
    let (_root, workspace) = case.instantiate();
    let harness = ToolHarness::new(4).unwrap();
    for _ in 0..2 {
        let pack = harness
            .agent_context("packing", &workspace, &case.query, 4_000, &[])
            .unwrap();
        let observed = score(&pack, &case);
        assert!(
            observed.all_required_edit_inputs,
            "{observed:?}\n{}",
            pack["hot_source"]
        );
        assert!(observed.within_budget);
    }
}

#[test]
fn engineering_fitness_packing_remaining_gold_gaps_are_explicit() {
    let mut gaps = Vec::new();
    for case in corpus() {
        if case.required.is_empty() {
            continue;
        }
        let (_root, workspace) = case.instantiate();
        let harness = ToolHarness::new(4).unwrap();
        for budget in [2_000, 4_000] {
            let pack = harness
                .agent_context("packing", &workspace, &case.query, budget, &[])
                .unwrap();
            let missing = case
                .required
                .iter()
                .filter(|gold| !complete_body(&pack, &case, gold))
                .map(|gold| gold.identity.clone())
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                gaps.push(format!(
                    "{}@{}: {:?}\ntargets={}\nrepo={}\nhot={}",
                    case.id,
                    budget,
                    missing,
                    pack["targets"],
                    pack["repo_map"]["items"],
                    pack["hot_source"]
                ));
            }
        }
    }
    assert!(
        gaps.is_empty(),
        "remaining complete-body gaps:\n{}",
        gaps.join("\n")
    );
}

#[test]
fn engineering_fitness_packing_readonly_keeps_evidence_not_write_permission() {
    let mut case = packet_case("rust");
    case.writable = false;
    let (_root, workspace) = case.instantiate();
    let pack = ToolHarness::new(4)
        .unwrap()
        .agent_context("packing", &workspace, &case.query, 2_000, &[])
        .unwrap();
    let observed = score(&pack, &case);
    assert_eq!(observed.complete_body_recall, Some(1.0));
    assert!(!observed.all_required_edit_inputs);
    assert_eq!(pack["project"]["write_enabled"], false);
    assert!(observed.within_budget);
}
