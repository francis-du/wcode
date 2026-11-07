use super::*;
use std::hint::black_box;
use std::time::Instant;

fn graph_fixture(source: &str, symbols: &[&str], budget: usize) -> SoftwareGraphSnapshot {
    let references = symbols
        .iter()
        .map(|symbol| ("src/lib.rs", *symbol))
        .collect::<Vec<_>>();
    graph_fixture_with_references(source, &references, budget)
}

fn graph_fixture_with_references(
    source: &str,
    references: &[(&str, &str)],
    budget: usize,
) -> SoftwareGraphSnapshot {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    fs::write(root.path().join("src/lib.rs"), source).unwrap();
    fs::write(
        root.path().join(".wcode/project.yaml"),
        "schema_version: 1\nname: budget-fixture\ndescription: bounded architecture graph\n",
    )
    .unwrap();
    let mut components = String::new();
    for (position, (path, symbol)) in references.iter().enumerate() {
        components.push_str(&format!(
            "- schema_version: 1\n  id: component:part-{position}\n  name: Part {position}\n  responsibilities: [fixture]\n  constraints: []\n  depends_on: []\n  implementation:\n    - kind: symbol\n      path: {path}\n      symbol: {symbol}\n"
        ));
    }
    fs::write(
        root.path().join(".wcode/design/components.yaml"),
        components,
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let load = crate::design::load_design(&workspace).unwrap();
    assert!(load.initialized);
    assert_eq!(load.error_count(), 0);
    ToolHarness::new(2)
        .unwrap()
        .software_graph_from_design("fixture", &workspace, "src", 10, budget, &load)
        .unwrap()
}

fn module_noise() -> String {
    (0..16)
        .map(|index| format!("mod declaration_{index};\n"))
        .collect()
}

fn syntax_definitions(graph: &SoftwareGraphSnapshot) -> Vec<&crate::graph::GraphNode> {
    graph
        .graph
        .nodes
        .values()
        .filter(|node| {
            node.provenance.precision == GraphPrecision::Syntax && node.kind != NodeKind::File
        })
        .collect()
}

#[test]
fn observatory_graph_budget_keeps_design_entries_ahead_of_module_declarations() {
    let mut source = module_noise();
    source.push_str("pub fn entry() { helper(); }\npub fn helper() {}\n");
    let graph = graph_fixture(&source, &["entry", "helper"], 2);
    let definitions = syntax_definitions(&graph);
    assert_eq!(
        definitions.len(),
        2,
        "Design priorities cannot grow the symbol budget"
    );
    assert!(
        definitions.iter().any(|node| node.label == "entry"),
        "module declarations must not displace the exact Design entry"
    );
    assert!(definitions.iter().any(|node| node.label == "helper"));
    assert!(
        graph.truncated,
        "omitted modules must remain visible as truncation"
    );
    let calls = graph
        .graph
        .edges
        .iter()
        .filter(|edge| edge.kind == EdgeKind::Calls)
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].provenance.precision, GraphPrecision::Syntax);
    assert_eq!(graph.graph.nodes[&calls[0].from].label, "entry");
    assert_eq!(graph.graph.nodes[&calls[0].to].label, "helper");
}

#[test]
fn observatory_graph_budget_keeps_direct_call_endpoint_after_exact_design_entry() {
    let mut source = module_noise();
    source.push_str("pub fn entry() { helper(); }\nfn helper() {}\n");
    let graph = graph_fixture(&source, &["entry"], 2);
    let definitions = syntax_definitions(&graph);
    assert_eq!(definitions.len(), 2);
    assert!(definitions.iter().any(|node| node.label == "entry"));
    assert!(
        definitions.iter().any(|node| node.label == "helper"),
        "the observed direct call endpoint must precede unrelated declarations"
    );
    assert_eq!(
        graph
            .graph
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Calls)
            .count(),
        1
    );
    assert!(graph.truncated);
}

#[test]
fn observatory_graph_budget_does_not_expand_for_more_design_entries_or_invent_calls() {
    let source = format!(
        "{}pub fn entry() {{}}\npub fn helper() {{}}\n",
        module_noise()
    );
    for budget in [1, 2] {
        let graph = graph_fixture(&source, &["entry", "helper", "missing"], budget);
        let definitions = syntax_definitions(&graph);
        assert_eq!(definitions.len(), budget);
        assert!(
            definitions
                .iter()
                .all(|node| matches!(node.label.as_str(), "entry" | "helper")),
            "only source-resolved exact entries may consume the priority slots"
        );
        assert!(!graph
            .graph
            .nodes
            .values()
            .any(|node| node.label == "missing"));
        assert!(!graph
            .graph
            .edges
            .iter()
            .any(|edge| edge.kind == EdgeKind::Calls));
        assert!(graph.truncated);
    }
}

#[test]
fn observatory_graph_budget_does_not_pin_unknown_receiver_targets() {
    let source = format!(
        "{}struct Worker;\nimpl Worker {{ fn flush(&self) {{}} }}\npub fn entry(cache: &Unknown) {{ cache.flush(); }}\n",
        module_noise()
    );
    let graph = graph_fixture(&source, &["entry"], 2);
    assert!(syntax_definitions(&graph)
        .iter()
        .any(|node| node.label == "entry"));
    assert!(!graph
        .graph
        .edges
        .iter()
        .any(|edge| edge.kind == EdgeKind::Calls));
    assert!(graph.truncated);
}
#[test]
fn observatory_graph_budget_keeps_missing_design_file_unresolved() {
    let source = format!("{}pub fn entry() {{}}\n", module_noise());
    let graph = graph_fixture_with_references(
        &source,
        &[("src/lib.rs", "entry"), ("src/missing.rs", "planned")],
        1,
    );
    let definitions = syntax_definitions(&graph);
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].label, "entry");
    assert!(!graph.graph.nodes.contains_key("file:src/missing.rs"));
    assert!(!graph
        .graph
        .nodes
        .values()
        .any(|node| node.label == "planned"));
    assert_eq!(
        graph.graph.nodes["component:part-1"].provenance.precision,
        GraphPrecision::Declared
    );
    assert!(!graph.graph.edges.iter().any(|edge| {
        edge.kind == EdgeKind::Calls
            || (edge.from == "component:part-1" && edge.kind == EdgeKind::Implements)
    }));
    assert!(graph.truncated);
}

#[test]
fn observatory_graph_leaves_design_symbols_outside_the_file_bound_unresolved() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn entry() {}\n").unwrap();
    fs::write(root.path().join("src/other.rs"), "pub fn planned() {}\n").unwrap();
    fs::write(
        root.path().join(".wcode/project.yaml"),
        "schema_version: 1\nname: bound-fixture\ndescription: bounded architecture graph\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".wcode/design/components.yaml"),
        "- schema_version: 1\n  id: component:part-0\n  name: Part 0\n  responsibilities: [fixture]\n  constraints: []\n  depends_on: []\n  implementation:\n    - kind: symbol\n      path: src/other.rs\n      symbol: planned\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let load = crate::design::load_design(&workspace).unwrap();
    assert!(load.initialized);
    assert_eq!(load.error_count(), 0);
    let graph = ToolHarness::new(2)
        .unwrap()
        .software_graph_from_design("fixture", &workspace, "src", 1, 10, &load)
        .unwrap();
    assert_eq!(
        graph
            .graph
            .nodes
            .values()
            .filter(|node| node.kind == NodeKind::File)
            .count(),
        1,
        "the file bound must still exclude the second source file"
    );
    assert!(!graph.graph.nodes.contains_key("file:src/other.rs"));
    assert!(!graph
        .graph
        .nodes
        .values()
        .any(|node| node.label == "planned"));
    assert!(!graph
        .graph
        .edges
        .iter()
        .any(|edge| edge.from == "component:part-0"));
    assert!(graph.scan_truncated);
}

#[test]
#[ignore = "explicit release benchmark; elapsed time is not a CI threshold"]
fn design_overlay_release_cost() {
    let referenced = 96;
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn entry() {}\n").unwrap();
    fs::write(
        root.path().join(".wcode/project.yaml"),
        "schema_version: 1\nname: overlay-fixture\ndescription: bounded design overlay\n",
    )
    .unwrap();
    let mut acceptance = String::new();
    for index in 0..referenced {
        let path = format!("src/unreached_{index:03}.rs");
        fs::write(
            root.path().join(&path),
            format!("pub fn probe_{index:03}() {{}}\n"),
        )
        .unwrap();
        acceptance.push_str(&format!(
            "- schema_version: 1\n  id: acceptance:probe-{index:03}\n  title: Probe {index:03}\n  statement: fixture\n  verification:\n    - kind: test\n      path: {path}\n      symbol: probe_{index:03}\n"
        ));
    }
    fs::write(
        root.path().join(".wcode/design/acceptance.yaml"),
        acceptance,
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let load = crate::design::load_design(&workspace).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    let mut timings = Vec::new();
    let mut result_bytes = 0;
    let mut reference = None;
    for iteration in 0..9 {
        let start = Instant::now();
        let graph = harness
            .software_graph_from_design("fixture", &workspace, "src", 1, 10, &load)
            .unwrap();
        black_box(&graph);
        let elapsed = start.elapsed().as_nanos();
        let serialized = serde_json::to_vec(&graph.graph).unwrap();
        result_bytes = serialized.len();
        if let Some(expected) = &reference {
            assert_eq!(&serialized, expected);
        } else {
            reference = Some(serialized);
        }
        assert_eq!(
            graph
                .graph
                .nodes
                .values()
                .filter(|node| node.kind == NodeKind::File)
                .count(),
            1,
            "the referenced files stay outside the bounded source scan"
        );
        if iteration >= 2 {
            timings.push(elapsed);
        }
    }
    timings.sort_unstable();
    println!(
        "{}",
        serde_json::json!({
            "stage": "design-overlay-unbounded-references",
            "referenced_files": referenced,
            "samples": timings.len(),
            "p50_us": timings[timings.len() / 2] as f64 / 1000.0,
            "p95_us": timings[(timings.len() * 95).div_ceil(100) - 1] as f64 / 1000.0,
            "graph_bytes": result_bytes,
            "identical_outputs_across_samples": true
        })
    );
}
