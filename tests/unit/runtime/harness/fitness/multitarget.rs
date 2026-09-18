//! Independent small multi-target regressions, outside the frozen corpus.
use super::corpus::{Case, Gold, Identity};
use super::scoring::score;
use crate::harness::ToolHarness;
use std::collections::BTreeMap;

fn check(case: Case) {
    let (_root, workspace) = case.instantiate();
    let mut failures = Vec::new();
    for budget in [1_000, 1_200, 2_000, 4_000] {
        let harness = ToolHarness::new(4).unwrap();
        for phase in ["cold", "warm"] {
            let pack = harness
                .agent_context("fitness", &workspace, &case.query, budget, &[])
                .unwrap();
            let result = score(&pack, &case);
            assert!(result.within_budget, "{} {budget}/{phase}", case.id);
            if !result.all_required_edit_inputs {
                failures.push(format!("{} {budget}/{phase}: {result:?}\n{pack}", case.id));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn engineering_fitness_multitarget_three_stage_chain_keeps_every_requested_body() {
    for language in ["rust", "go", "typescript", "python"] {
        let (extension, prefix, functions) = match language {
            "rust" => (
                "rs",
                "",
                vec![
                    "pub fn check_packet() -> bool {\n    true\n}\n",
                    "pub fn route_packet() -> bool {\n    check_packet()\n}\n",
                    "pub fn start_packet() -> bool {\n    route_packet()\n}\n",
                ],
            ),
            "go" => (
                "go",
                "package packet\n",
                vec![
                    "func check_packet() bool {\n    return true\n}\n",
                    "func route_packet() bool {\n    return check_packet()\n}\n",
                    "func start_packet() bool {\n    return route_packet()\n}\n",
                ],
            ),
            "typescript" => (
                "ts",
                "",
                vec![
                    "export function check_packet(): boolean {\n    return true;\n}\n",
                    "export function route_packet(): boolean {\n    return check_packet();\n}\n",
                    "export function start_packet(): boolean {\n    return route_packet();\n}\n",
                ],
            ),
            _ => (
                "py",
                "",
                vec![
                    "def check_packet():\n    return True\n",
                    "def route_packet():\n    return check_packet()\n",
                    "def start_packet():\n    return route_packet()\n",
                ],
            ),
        };
        let path = format!("src/packet.{extension}");
        let mut files =
            BTreeMap::from([(path.clone(), format!("{prefix}{}", functions.join("\n")))]);
        if language == "rust" {
            files.insert(
                "Cargo.toml".into(),
                "[package]\nname='packet_fixture'\nversion='0.1.0'\nedition='2021'\n".into(),
            );
            files.insert(
                "src/lib.rs".into(),
                "mod packet;\npub use packet::*;\n".into(),
            );
        }
        let required = ["check_packet", "route_packet", "start_packet"]
            .into_iter()
            .zip(functions)
            .map(|(name, source)| Gold {
                identity: Identity::new(&path, name),
                fragment: source.into(),
            })
            .collect();
        check(Case {
            id: format!("packet-three-stage-{language}"),
            language: language.into(),
            category: "multi-stage-independent".into(),
            query: "Trace start_packet through route_packet to check_packet".into(),
            files,
            required,
            useful: vec![],
            writable: true,
            no_answer: false,
        });
    }
}

#[test]
fn engineering_fitness_multitarget_explicit_module_wins_its_name_collision() {
    let files = BTreeMap::from([
        ("src/lib.rs".into(), "pub mod packet_codec;\n".into()),
        (
            "src/packet_codec.rs".into(),
            "pub fn packet_codec() {}\n".into(),
        ),
    ]);
    let case = Case {
        id: "explicit-module-collision".into(),
        language: "rust".into(),
        category: "module-independent".into(),
        query: "inspect module packet_codec".into(),
        files,
        required: vec![Gold {
            identity: Identity::new("src/lib.rs", "packet_codec"),
            fragment: "pub mod packet_codec;\n".into(),
        }],
        useful: vec![],
        writable: true,
        no_answer: false,
    };
    let (_root, workspace) = case.instantiate();
    let harness = ToolHarness::new(4).unwrap();
    for _ in 0..2 {
        let pack = harness
            .agent_context("fitness", &workspace, &case.query, 1_000, &[])
            .unwrap();
        assert_eq!(pack["hot_source"][0]["path"], "src/lib.rs");
        assert!(score(&pack, &case).all_required_edit_inputs);
    }
}

#[test]
fn engineering_fitness_multitarget_four_files_keep_each_body_and_sha() {
    let names = [
        "decode_packet",
        "encode_packet",
        "route_packet",
        "verify_packet",
    ];
    let mut files = BTreeMap::from([(
        "Cargo.toml".into(),
        "[package]\nname='packet_fixture'\nversion='0.1.0'\nedition='2021'\n".into(),
    )]);
    let mut required = Vec::new();
    let mut modules = String::new();
    for name in names {
        let path = format!("src/{name}.rs");
        let source = format!("pub fn {name}() -> usize {{\n    7\n}}\n");
        modules.push_str(&format!("pub mod {name};\n"));
        files.insert(path.clone(), source.clone());
        required.push(Gold {
            identity: Identity::new(&path, name),
            fragment: source,
        });
    }
    files.insert("src/lib.rs".into(), modules);
    check(Case {
        id: "packet-four-files".into(),
        language: "rust".into(),
        category: "multi-file-independent".into(),
        query: format!("inspect {}", names.join(" ")),
        files,
        required,
        useful: vec![],
        writable: true,
        no_answer: false,
    });
}
