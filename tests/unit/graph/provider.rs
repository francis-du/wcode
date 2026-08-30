use super::*;
use crate::graph::{EdgeKind, GraphImportEdge, GraphImportNode, NodeKind};
use std::collections::BTreeMap;

#[test]
fn first_party_lsp_freshness_tracks_source_hashes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let source = workspace.load_source("a.rs").unwrap();
    let import = GraphProviderImport {
        provider: "lsp:fixture".into(),
        precision: GraphPrecision::Semantic,
        revision: "sha256:fixture".into(),
        nodes: vec![GraphImportNode {
            id: "semantic:function:a".into(),
            kind: NodeKind::Function,
            label: "a".into(),
            attributes: BTreeMap::from([
                ("path".into(), serde_json::json!("a.rs")),
                ("source_sha256".into(), serde_json::json!(source.sha256)),
            ]),
        }],
        edges: vec![],
    };
    assert_eq!(
        freshness(&workspace, &import),
        GraphProviderFreshness::Fresh
    );
    std::fs::write(dir.path().join("a.rs"), "fn changed() {}\n").unwrap();
    assert_eq!(
        freshness(&workspace, &import),
        GraphProviderFreshness::Stale
    );

    let external = GraphProviderImport {
        provider: "external-scip".into(),
        nodes: vec![GraphImportNode {
            attributes: BTreeMap::new(),
            ..import.nodes[0].clone()
        }],
        ..import
    };
    assert_eq!(
        freshness(&workspace, &external),
        GraphProviderFreshness::Unknown
    );
}

#[test]
fn latest_provider_revision_wins_without_losing_other_providers() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let first = GraphProviderImport {
        provider: "rust-analyzer".into(),
        precision: GraphPrecision::Semantic,
        revision: "one".into(),
        nodes: vec![GraphImportNode {
            id: "semantic:function:a".into(),
            kind: NodeKind::Function,
            label: "a".into(),
            attributes: BTreeMap::new(),
        }],
        edges: vec![],
    };
    let first_stored = persist(&workspace, &first).unwrap();
    let second = GraphProviderImport {
        revision: "two".into(),
        nodes: vec![GraphImportNode {
            id: "semantic:function:b".into(),
            kind: NodeKind::Function,
            label: "b".into(),
            attributes: BTreeMap::new(),
        }],
        edges: vec![GraphImportEdge {
            from: "semantic:function:b".into(),
            to: "symbol:external".into(),
            kind: EdgeKind::Calls,
        }],
        ..first
    };
    let second_stored = persist(&workspace, &second).unwrap();
    assert!(second_stored.imported_at_ms > first_stored.imported_at_ms);
    let latest = load_latest(&workspace).unwrap();
    assert_eq!(latest.len(), 1);
    assert_eq!(latest[0].import.revision, "two");
    assert_eq!(latest[0].import.nodes[0].label, "b");
}
