use super::*;
use crate::graph::{GraphPrecision, SoftwareGraph};
use std::collections::BTreeMap;

#[test]
fn graph_history_round_trips_and_queries_nodes() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let mut graph = SoftwareGraph::default();
    graph
        .add_node(GraphNode {
            id: "component:auth".into(),
            kind: NodeKind::Component,
            label: "Authentication".into(),
            attributes: BTreeMap::new(),
            provenance: crate::graph::GraphProvenance {
                provider: "design".into(),
                precision: GraphPrecision::Declared,
                revision: "design:1".into(),
            },
        })
        .unwrap();
    let snapshot = SoftwareGraphSnapshot {
        workspace: "demo".into(),
        path: ".".into(),
        provider: "wcode-composite".into(),
        precision: GraphPrecision::Mixed,
        files_considered: 0,
        files_indexed: 0,
        files_failed: 0,
        scan_truncated: false,
        truncated: false,
        node_count: 1,
        edge_count: 0,
        failures: vec![],
        graph,
    };
    let stored = persist(&workspace, &snapshot).unwrap();
    assert_eq!(history(&workspace, 10).unwrap().len(), 1);
    let result = query(
        &workspace,
        &GraphQueryInput {
            snapshot_id: Some(stored.id.clone()),
            node_id: None,
            kind: Some(NodeKind::Component),
            label_contains: Some("auth".into()),
            related_to: None,
            edge_kind: None,
            direction: None,
            limit: 10,
        },
    )
    .unwrap();
    assert_eq!(result.snapshot_id, stored.id);
    assert_eq!(result.nodes.len(), 1);
}

#[test]
fn graph_chain_traces_calls_and_keeps_precision_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let syntax = crate::graph::GraphProvenance {
        provider: "tree-sitter".into(),
        precision: GraphPrecision::Syntax,
        revision: "syntax:1".into(),
    };
    let semantic = crate::graph::GraphProvenance {
        provider: "lsp:rust-analyzer".into(),
        precision: GraphPrecision::Semantic,
        revision: "semantic:1".into(),
    };
    let declared = crate::graph::GraphProvenance {
        provider: "wcode-design".into(),
        precision: GraphPrecision::Declared,
        revision: "design:1".into(),
    };
    let mut graph = SoftwareGraph::default();
    for (id, kind, label, provenance) in [
        (
            "file:src/lib.rs",
            NodeKind::File,
            "src/lib.rs",
            syntax.clone(),
        ),
        (
            "function:caller2",
            NodeKind::Function,
            "caller2",
            syntax.clone(),
        ),
        (
            "function:caller",
            NodeKind::Function,
            "caller",
            syntax.clone(),
        ),
        (
            "function:target",
            NodeKind::Function,
            "target_feature",
            syntax.clone(),
        ),
        (
            "function:callee",
            NodeKind::Function,
            "callee",
            semantic.clone(),
        ),
        (
            "function:callee2",
            NodeKind::Function,
            "callee2",
            semantic.clone(),
        ),
        (
            "component:runtime",
            NodeKind::Component,
            "Runtime",
            declared.clone(),
        ),
        (
            "REQ-RUNTIME",
            NodeKind::Requirement,
            "Runtime requirement",
            declared.clone(),
        ),
    ] {
        graph
            .add_node(GraphNode {
                id: id.into(),
                kind,
                label: label.into(),
                attributes: BTreeMap::new(),
                provenance,
            })
            .unwrap();
    }
    for edge in [
        GraphEdge {
            from: "file:src/lib.rs".into(),
            to: "function:target".into(),
            kind: EdgeKind::Defines,
            provenance: syntax.clone(),
        },
        GraphEdge {
            from: "function:caller2".into(),
            to: "function:caller".into(),
            kind: EdgeKind::Calls,
            provenance: syntax.clone(),
        },
        GraphEdge {
            from: "function:caller".into(),
            to: "function:target".into(),
            kind: EdgeKind::Calls,
            provenance: syntax.clone(),
        },
        GraphEdge {
            from: "function:target".into(),
            to: "function:callee".into(),
            kind: EdgeKind::Calls,
            provenance: semantic.clone(),
        },
        GraphEdge {
            from: "function:callee".into(),
            to: "function:callee2".into(),
            kind: EdgeKind::Calls,
            provenance: semantic.clone(),
        },
        GraphEdge {
            from: "component:runtime".into(),
            to: "function:target".into(),
            kind: EdgeKind::Implements,
            provenance: declared.clone(),
        },
        GraphEdge {
            from: "component:runtime".into(),
            to: "REQ-RUNTIME".into(),
            kind: EdgeKind::ImplementsRequirement,
            provenance: declared.clone(),
        },
    ] {
        graph.add_edge(edge).unwrap();
    }
    let snapshot = SoftwareGraphSnapshot {
        workspace: "demo".into(),
        path: ".".into(),
        provider: "wcode-composite".into(),
        precision: GraphPrecision::Mixed,
        files_considered: 1,
        files_indexed: 1,
        files_failed: 0,
        scan_truncated: false,
        truncated: false,
        node_count: graph.nodes.len(),
        edge_count: graph.edges.len(),
        failures: vec![],
        graph,
    };
    persist(&workspace, &snapshot).unwrap();

    let calls = chain(
        &workspace,
        &GraphChainInput {
            snapshot_id: None,
            node_id: None,
            label_contains: Some("target_feature".into()),
            depth: 2,
            limit: 64,
            mode: GraphChainMode::Calls,
        },
    )
    .unwrap();
    assert_eq!(calls.root_ids, vec!["function:target"]);
    assert_eq!(calls.nodes.len(), 5);
    assert_eq!(calls.edges.len(), 4);
    assert_eq!(calls.upstream_nodes, 2);
    assert_eq!(calls.downstream_nodes, 2);
    assert_eq!(calls.precision_counts["syntax"], 2);
    assert_eq!(calls.precision_counts["semantic"], 2);
    let caller2 = calls
        .nodes
        .iter()
        .find(|node| node.node.id == "function:caller2")
        .unwrap();
    assert!(caller2.upstream && !caller2.downstream);
    let callee2 = calls
        .nodes
        .iter()
        .find(|node| node.node.id == "function:callee2")
        .unwrap();
    assert!(callee2.downstream && !callee2.upstream);

    let all = chain(
        &workspace,
        &GraphChainInput {
            snapshot_id: None,
            node_id: Some("function:target".into()),
            label_contains: None,
            depth: 2,
            limit: 64,
            mode: GraphChainMode::All,
        },
    )
    .unwrap();
    assert!(all.nodes.iter().all(|node| !matches!(
        node.node.kind,
        NodeKind::Requirement | NodeKind::Component | NodeKind::Config | NodeKind::Verification
    )));
    assert!(all.edges.iter().all(|edge| !matches!(
        edge.kind,
        EdgeKind::ImplementsRequirement | EdgeKind::VerifiedBy | EdgeKind::ConstrainedBy
    )));
    assert!(!all.truncated);

    let file_calls = chain(
        &workspace,
        &GraphChainInput {
            snapshot_id: None,
            node_id: None,
            label_contains: Some("src/lib.rs".into()),
            depth: 2,
            limit: 64,
            mode: GraphChainMode::Calls,
        },
    )
    .unwrap();
    assert_eq!(file_calls.root_ids, vec!["file:src/lib.rs"]);
    assert!(file_calls
        .nodes
        .iter()
        .any(|node| node.node.id == "function:target"));
    assert!(file_calls
        .nodes
        .iter()
        .any(|node| node.node.id == "function:caller"));
    assert!(file_calls
        .nodes
        .iter()
        .any(|node| node.node.id == "function:callee"));
    assert!(file_calls
        .edges
        .iter()
        .any(|edge| edge.kind == EdgeKind::Defines));
    assert!(file_calls
        .edges
        .iter()
        .any(|edge| edge.kind == EdgeKind::Calls));
}

#[test]
fn graph_change_signal_uses_metadata_without_reading_snapshot_content() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let mut graph = SoftwareGraph::default();
    graph
        .add_node(GraphNode {
            id: "function:signal".into(),
            kind: NodeKind::Function,
            label: "signal".into(),
            attributes: BTreeMap::new(),
            provenance: crate::graph::GraphProvenance {
                provider: "tree-sitter".into(),
                precision: GraphPrecision::Syntax,
                revision: "syntax:signal".into(),
            },
        })
        .unwrap();
    let snapshot = SoftwareGraphSnapshot {
        workspace: "demo".into(),
        path: ".".into(),
        provider: "tree-sitter".into(),
        precision: GraphPrecision::Syntax,
        files_considered: 1,
        files_indexed: 1,
        files_failed: 0,
        scan_truncated: false,
        truncated: false,
        node_count: 1,
        edge_count: 0,
        failures: vec![],
        graph,
    };
    let stored = persist(&workspace, &snapshot).unwrap();

    READ_SNAPSHOT_CALLS.with(|count| count.set(0));
    let (revision, first_signal) = change_signal(&workspace).unwrap().unwrap();
    assert_eq!(revision, stored.id);
    assert_eq!(READ_SNAPSHOT_CALLS.with(|count| count.get()), 0);

    let directory = graph_directory(&workspace).unwrap();
    let path = graph_paths(&directory).unwrap().pop().unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b" ")
        .unwrap();
    let (revision_after_edit, second_signal) = change_signal(&workspace).unwrap().unwrap();
    assert_eq!(revision_after_edit, stored.id);
    assert_ne!(first_signal, second_signal);
    assert_eq!(READ_SNAPSHOT_CALLS.with(|count| count.get()), 0);
}

#[test]
fn duplicate_graph_persist_reads_only_the_matching_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let mut first = None;
    for index in 0..8 {
        let mut graph = SoftwareGraph::default();
        graph
            .add_node(GraphNode {
                id: format!("function:{index}"),
                kind: NodeKind::Function,
                label: format!("function-{index}"),
                attributes: BTreeMap::new(),
                provenance: crate::graph::GraphProvenance {
                    provider: "tree-sitter".into(),
                    precision: GraphPrecision::Syntax,
                    revision: format!("syntax:{index}"),
                },
            })
            .unwrap();
        let snapshot = SoftwareGraphSnapshot {
            workspace: "demo".into(),
            path: ".".into(),
            provider: "tree-sitter".into(),
            precision: GraphPrecision::Syntax,
            files_considered: 1,
            files_indexed: 1,
            files_failed: 0,
            scan_truncated: false,
            truncated: false,
            node_count: 1,
            edge_count: 0,
            failures: vec![],
            graph,
        };
        let stored = persist(&workspace, &snapshot).unwrap();
        if index == 0 {
            first = Some(snapshot);
            assert!(!stored.id.is_empty());
        }
    }

    READ_SNAPSHOT_CALLS.with(|count| count.set(0));
    let duplicate = persist(&workspace, first.as_ref().unwrap()).unwrap();
    assert!(!duplicate.id.is_empty());
    assert_eq!(
        READ_SNAPSHOT_CALLS.with(|count| count.get()),
        1,
        "deduplication should validate only the filename-matched graph snapshot"
    );
}

#[test]
fn graph_diff_separates_structural_changes_from_revision_churn() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let provenance = |revision: &str| crate::graph::GraphProvenance {
        provider: "lsp:fixture".into(),
        precision: GraphPrecision::Semantic,
        revision: revision.into(),
    };
    let node = |id: &str, label: &str, revision: &str| GraphNode {
        id: id.into(),
        kind: NodeKind::Function,
        label: label.into(),
        attributes: BTreeMap::new(),
        provenance: provenance(revision),
    };

    let mut before_graph = SoftwareGraph::default();
    before_graph
        .add_node(node("function:a", "a", "semantic:1"))
        .unwrap();
    before_graph
        .add_node(node("function:b", "b", "semantic:1"))
        .unwrap();
    before_graph
        .add_node(node("function:removed", "removed", "semantic:1"))
        .unwrap();
    before_graph
        .add_edge(GraphEdge {
            from: "function:a".into(),
            to: "function:b".into(),
            kind: EdgeKind::Calls,
            provenance: provenance("semantic:1"),
        })
        .unwrap();
    let before = SoftwareGraphSnapshot {
        workspace: "demo".into(),
        path: ".".into(),
        provider: "wcode-composite".into(),
        precision: GraphPrecision::Mixed,
        files_considered: 0,
        files_indexed: 0,
        files_failed: 0,
        scan_truncated: false,
        truncated: false,
        node_count: 3,
        edge_count: 1,
        failures: vec![],
        graph: before_graph,
    };
    let before = persist(&workspace, &before).unwrap();

    let mut after_graph = SoftwareGraph::default();
    after_graph
        .add_node(node("function:a", "a-renamed", "semantic:2"))
        .unwrap();
    after_graph
        .add_node(node("function:b", "b", "semantic:1"))
        .unwrap();
    after_graph
        .add_node(node("function:added", "added", "semantic:2"))
        .unwrap();
    after_graph
        .add_edge(GraphEdge {
            from: "function:a".into(),
            to: "function:b".into(),
            kind: EdgeKind::Calls,
            provenance: provenance("semantic:2"),
        })
        .unwrap();
    let after = SoftwareGraphSnapshot {
        workspace: "demo".into(),
        path: ".".into(),
        provider: "wcode-composite".into(),
        precision: GraphPrecision::Mixed,
        files_considered: 0,
        files_indexed: 0,
        files_failed: 0,
        scan_truncated: false,
        truncated: false,
        node_count: 3,
        edge_count: 1,
        failures: vec![],
        graph: after_graph,
    };
    let after = persist(&workspace, &after).unwrap();

    let result = diff(
        &workspace,
        &GraphDiffInput {
            from_snapshot_id: Some(before.id.clone()),
            to_snapshot_id: Some(after.id.clone()),
            limit: 10,
        },
    )
    .unwrap();
    assert_eq!(result.from_snapshot_id, before.id);
    assert_eq!(result.to_snapshot_id, after.id);
    assert_eq!(result.added_node_count, 1);
    assert_eq!(result.removed_node_count, 1);
    assert_eq!(result.changed_node_count, 1);
    assert_eq!(result.added_edge_count, 0);
    assert_eq!(result.removed_edge_count, 0);
    assert_eq!(result.changed_edge_count, 1);
    assert_eq!(
        result.changed_edges[0].before.provenance.revision,
        "semantic:1"
    );
    assert_eq!(
        result.changed_edges[0].after.provenance.revision,
        "semantic:2"
    );
}
