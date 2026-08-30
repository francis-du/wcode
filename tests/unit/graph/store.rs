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
