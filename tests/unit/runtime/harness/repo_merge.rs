use super::*;
use crate::harness::harness_retrieval::merge_relationship_graph;

fn fixture_graph(harness: &ToolHarness, workspace: &Workspace) -> SoftwareGraphSnapshot {
    harness
        .code_index
        .software_graph("demo", workspace, ".", 20, 200)
        .unwrap()
}

#[test]
fn repo_merge_matches_legacy_order_and_full_provenance_identity() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("main.rs"),
        "fn target() {}\nfn caller() { target(); target(); }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let base = fixture_graph(&harness, &workspace);
    let baseline = serde_json::to_vec(&base).unwrap();
    let mut supplemental = base.clone();
    let original = base.graph.edges[0].clone();
    for variant in 0..4 {
        let mut edge = original.clone();
        match variant {
            0 => edge.kind = EdgeKind::References,
            1 => edge.provenance.provider = "test-provider".to_owned(),
            2 => edge.provenance.precision = GraphPrecision::Deterministic,
            _ => edge.provenance.revision = "test-revision".to_owned(),
        }
        supplemental.graph.edges.push(edge.clone());
        supplemental.graph.edges.push(edge);
    }
    let mut expected = base.graph.edges.clone();
    for edge in &supplemental.graph.edges {
        if !expected.contains(edge) {
            expected.push(edge.clone());
        }
    }
    supplemental.scan_truncated = true;
    let merged = merge_relationship_graph(&base, supplemental).unwrap();
    assert_eq!(merged.graph.edges, expected);
    assert_eq!(merged.edge_count, expected.len());
    assert!(merged.scan_truncated);
    assert_eq!(
        serde_json::to_vec(&base).unwrap(),
        baseline,
        "merge must not mutate the shared base"
    );
    merged.graph.validate().unwrap();
}

#[test]
fn repo_merge_rejects_mixed_source_revisions() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("main.rs"),
        "fn target() {}\nfn caller() { target(); }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let base = fixture_graph(&harness, &workspace);
    let baseline = serde_json::to_vec(&base).unwrap();
    fs::write(
        root.path().join("main.rs"),
        "fn target() {}\nfn caller() {}\n",
    )
    .unwrap();
    let changed = fixture_graph(&harness, &workspace);
    let error = merge_relationship_graph(&base, changed).unwrap_err();
    assert!(error.to_string().contains("source changed"), "{error:#}");
    assert_eq!(serde_json::to_vec(&base).unwrap(), baseline);
}

#[test]
fn repo_merge_still_validates_endpoints_and_self_edges() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("main.rs"), "fn target() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let base = fixture_graph(&harness, &workspace);
    for self_edge in [false, true] {
        let mut supplemental = base.clone();
        let mut invalid = supplemental.graph.edges[0].clone();
        invalid.to = if self_edge {
            invalid.from.clone()
        } else {
            "symbol:missing".to_owned()
        };
        supplemental.graph.edges.push(invalid);
        assert!(merge_relationship_graph(&base, supplemental).is_err());
    }
}
