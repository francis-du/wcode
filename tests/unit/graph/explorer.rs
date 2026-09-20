use super::*;
use crate::graph::{GraphEdge, GraphProvenance, SoftwareGraph, SoftwareGraphSnapshot};

fn fixture_workspace() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn alpha() { beta(); }\npub fn beta() {}\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/other.rs"), "pub fn gamma() {}\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    (dir, workspace)
}

fn persist_fixture(workspace: &Workspace) {
    let syntax = GraphProvenance {
        provider: "tree-sitter".into(),
        precision: GraphPrecision::Syntax,
        revision: "syntax:1".into(),
    };
    let semantic = GraphProvenance {
        provider: "lsp:rust-analyzer".into(),
        precision: GraphPrecision::Semantic,
        revision: "semantic:1".into(),
    };
    let mut graph = SoftwareGraph::default();
    for (id, kind, label, path, language, provenance) in [
        (
            "file:src/lib.rs",
            NodeKind::File,
            "src/lib.rs",
            "src/lib.rs",
            "rust",
            syntax.clone(),
        ),
        (
            "file:src/other.rs",
            NodeKind::File,
            "src/other.rs",
            "src/other.rs",
            "rust",
            syntax.clone(),
        ),
        (
            "function:alpha",
            NodeKind::Function,
            "alpha",
            "src/lib.rs",
            "rust",
            syntax.clone(),
        ),
        (
            "function:beta",
            NodeKind::Function,
            "beta",
            "src/lib.rs",
            "rust",
            syntax.clone(),
        ),
        (
            "function:gamma",
            NodeKind::Function,
            "gamma",
            "src/other.rs",
            "rust",
            semantic.clone(),
        ),
    ] {
        let mut attributes = BTreeMap::new();
        attributes.insert("path".to_owned(), serde_json::json!(path));
        attributes.insert("language".to_owned(), serde_json::json!(language));
        graph
            .add_node(GraphNode {
                id: id.into(),
                kind,
                label: label.into(),
                attributes,
                provenance,
            })
            .unwrap();
    }
    for edge in [
        GraphEdge {
            from: "file:src/lib.rs".into(),
            to: "function:alpha".into(),
            kind: EdgeKind::Defines,
            provenance: syntax.clone(),
        },
        GraphEdge {
            from: "file:src/lib.rs".into(),
            to: "function:beta".into(),
            kind: EdgeKind::Defines,
            provenance: syntax.clone(),
        },
        GraphEdge {
            from: "file:src/other.rs".into(),
            to: "function:gamma".into(),
            kind: EdgeKind::Defines,
            provenance: semantic.clone(),
        },
        GraphEdge {
            from: "function:alpha".into(),
            to: "function:gamma".into(),
            kind: EdgeKind::Calls,
            provenance: semantic,
        },
    ] {
        graph.add_edge(edge).unwrap();
    }
    let snapshot = SoftwareGraphSnapshot {
        workspace: "demo".into(),
        path: ".".into(),
        provider: "wcode-composite".into(),
        precision: GraphPrecision::Mixed,
        files_considered: 2,
        files_indexed: 2,
        files_failed: 0,
        scan_truncated: false,
        truncated: false,
        node_count: graph.nodes.len(),
        edge_count: graph.edges.len(),
        failures: vec![],
        graph,
    };
    crate::graph_store::persist(workspace, &snapshot).unwrap();
}

#[test]
fn graph_search_candidate_can_be_used_as_exact_focus_root() {
    let (_dir, workspace) = fixture_workspace();
    persist_fixture(&workspace);

    let search = search(
        &workspace,
        &GraphSearchInput {
            snapshot_id: None,
            query: "alpha".into(),
            limit: 20,
        },
    )
    .unwrap();
    assert_eq!(search.results.len(), 1);
    assert_eq!(search.results[0].node.id, "function:alpha");
    assert_eq!(search.results[0].match_kind, "exact_label");

    let focus = crate::graph_store::chain(
        &workspace,
        &crate::graph_store::GraphChainInput {
            snapshot_id: Some(search.snapshot_id),
            node_id: Some(search.results[0].node.id.clone()),
            label_contains: None,
            depth: 2,
            limit: 64,
            mode: crate::graph_store::GraphChainMode::All,
        },
    )
    .unwrap();
    assert_eq!(focus.root_ids, vec!["function:alpha"]);
    assert!(focus
        .nodes
        .iter()
        .any(|node| node.node.id == "function:gamma"));
}

#[test]
fn repository_overview_aggregates_cross_file_relations_from_same_snapshot() {
    let (_dir, workspace) = fixture_workspace();
    persist_fixture(&workspace);

    let overview = overview(
        &workspace,
        &GraphOverviewInput {
            snapshot_id: None,
            limit: 64,
        },
    )
    .unwrap();
    assert_eq!(overview.files_considered, 2);
    assert_eq!(overview.files_indexed, 2);
    assert_eq!(overview.total_files, 2);
    assert_eq!(overview.languages.get("rust"), Some(&2));
    assert!(overview
        .nodes
        .iter()
        .any(|node| node.id == "file:src/lib.rs"));
    assert!(overview
        .nodes
        .iter()
        .any(|node| node.id == "file:src/other.rs"));
    let edge = overview
        .edges
        .iter()
        .find(|edge| edge.from == "file:src/lib.rs" && edge.to == "file:src/other.rs")
        .expect("cross-file semantic call should aggregate to file overview");
    assert_eq!(edge.count, 1);
    assert_eq!(edge.kinds.get("calls"), Some(&1));
    assert_eq!(edge.precision.get("semantic"), Some(&1));
    assert!(!overview.truncated);
}
