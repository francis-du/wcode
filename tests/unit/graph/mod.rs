use super::*;

fn provenance() -> GraphProvenance {
    GraphProvenance {
        provider: "tree-sitter".into(),
        precision: GraphPrecision::Syntax,
        revision: "sha256:fixture".into(),
    }
}

fn node(id: &str, kind: NodeKind) -> GraphNode {
    GraphNode {
        id: id.into(),
        kind,
        label: id.into(),
        attributes: BTreeMap::new(),
        provenance: provenance(),
    }
}

#[test]
fn graph_requires_existing_endpoints_and_unique_edges() {
    let mut graph = SoftwareGraph::default();
    graph
        .add_node(node("component:a", NodeKind::Component))
        .unwrap();
    graph
        .add_node(node("file:src/a.rs", NodeKind::File))
        .unwrap();
    let edge = GraphEdge {
        from: "component:a".into(),
        to: "file:src/a.rs".into(),
        kind: EdgeKind::Implements,
        provenance: provenance(),
    };
    graph.add_edge(edge.clone()).unwrap();
    assert_eq!(graph.add_edge(edge), Err(GraphError::DuplicateEdge));
    assert_eq!(graph.validate(), Ok(()));
}

#[test]
fn graph_precision_is_explicit_and_serializable() {
    let value = serde_json::to_value(provenance()).unwrap();
    assert_eq!(value["provider"], "tree-sitter");
    assert_eq!(value["precision"], "syntax");
    assert_eq!(value["revision"], "sha256:fixture");
}
