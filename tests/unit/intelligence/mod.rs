use super::*;
use crate::graph::GraphPrecision;
use crate::stage_executor::{execute as execute_stage, StageExecutorSpec};
use crate::workspace::{Workspace, WorkspaceSecurity};
use std::fs;

mod verification;

#[test]
fn design_status_distinguishes_uninitialized_from_invalid() {
    let empty = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(empty.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let status = runtime.design_status("demo", &workspace).unwrap();
    assert!(!status.initialized);
    assert!(!status.valid);
    assert_eq!(status.errors, 0);

    fs::create_dir_all(empty.path().join(".wcode/design/requirements")).unwrap();
    fs::write(
        empty.path().join(".wcode/design/requirements/bad.yaml"),
        "id: REQ-1\ntitle: Missing intent\n",
    )
    .unwrap();
    let status = runtime.design_status("demo", &workspace).unwrap();
    assert!(status.initialized);
    assert!(!status.valid);
    assert!(status.errors > 0);
}

#[test]
fn traces_requirement_to_real_implementation_and_test_symbols() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::create_dir_all(dir.path().join(".wcode/design/requirements")).unwrap();
    fs::create_dir_all(dir.path().join(".wcode/design/components")).unwrap();
    fs::create_dir_all(dir.path().join(".wcode/design/acceptance")).unwrap();
    fs::write(
            dir.path().join("src/lib.rs"),
            "fn secure(path: &str) -> bool { !path.contains(\"..\") }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn blocks_escape() { assert!(!super::secure(\"../secret\")); }\n}\n",
        )
        .unwrap();
    fs::write(
            dir.path().join(".wcode/design/requirements/REQ-SEC-001.yaml"),
            "id: REQ-SEC-001\ntitle: Workspace isolation\nintent: Paths must stay inside the workspace.\npriority: critical\nimplemented_by:\n  - component:workspace-security\nacceptance:\n  - AC-SEC-001\n",
        )
        .unwrap();
    fs::write(
            dir.path().join(".wcode/design/components/workspace-security.yaml"),
            "id: component:workspace-security\nname: Workspace Security\nimplementation:\n  - kind: symbol\n    path: src/lib.rs\n    symbol: secure\n",
        )
        .unwrap();
    fs::write(
            dir.path().join(".wcode/design/acceptance/AC-SEC-001.yaml"),
            "id: AC-SEC-001\ntitle: Escape is blocked\nstatement: Parent traversal is rejected.\nverification:\n  - kind: test\n    path: src/lib.rs\n    symbol: blocks_escape\n",
        )
        .unwrap();

    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let status = runtime
        .traceability_status("demo", &workspace, &index, &HashSet::new())
        .unwrap();

    assert!(status.valid_design, "{:?}", status.diagnostics);
    assert_eq!(status.requirement_to_component.percent, 100);
    assert_eq!(status.design_to_implementation.percent, 100);
    assert_eq!(status.acceptance_to_verification.percent, 100);
    assert_eq!(status.complete_requirements, 1);
    assert_eq!(
        status.requirements[0].status,
        RequirementTraceStatus::Complete
    );
    assert!(status.requirements[0]
        .implementation
        .iter()
        .all(|reference| reference.resolved && reference.precision == "syntax"));
    assert!(status.requirements[0]
        .verification
        .iter()
        .all(|reference| reference.resolved && reference.kind == TraceReferenceKind::Test));
}

#[test]
fn software_context_uses_token_scoring_and_real_budget_caps() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".wcode/design")).unwrap();
    fs::write(
        dir.path().join(".wcode/project.yaml"),
        "schema_version: 1\nname: demo\n",
    )
    .unwrap();
    fs::write(
            dir.path().join(".wcode/design/requirements.yaml"),
            "- schema_version: 1\n  id: REQ-AAA-1\n  title: Unrelated analytics\n  intent: Render an unrelated metrics dashboard.\n  implemented_by:\n    - component:noise\n- schema_version: 1\n  id: REQ-SEC-1\n  title: Workspace isolation\n  intent: Keep command execution inside the workspace security boundary.\n  implemented_by:\n    - component:security\n",
        )
        .unwrap();
    fs::write(
            dir.path().join(".wcode/design/components.yaml"),
            "- schema_version: 1\n  id: component:noise\n  name: Analytics Noise\n  responsibilities:\n    - render unrelated metrics\n- schema_version: 1\n  id: component:security\n  name: Command Security\n  responsibilities:\n    - enforce workspace command boundaries\n",
        )
        .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let index = CodeIndex::new().unwrap();
    graph_provider_store::persist(
        &workspace,
        &crate::graph::GraphProviderImport {
            provider: "fixture-lsp".into(),
            precision: GraphPrecision::Semantic,
            revision: "sha256:fixture-graph-context".into(),
            nodes: vec![
                crate::graph::GraphImportNode {
                    id: "semantic:workspace-command-guard".into(),
                    kind: NodeKind::Function,
                    label: "workspace_command_guard".into(),
                    attributes: BTreeMap::from([(
                        "path".into(),
                        serde_json::json!("src/security.rs"),
                    )]),
                },
                crate::graph::GraphImportNode {
                    id: "semantic:audit-command".into(),
                    kind: NodeKind::Function,
                    label: "audit_command".into(),
                    attributes: BTreeMap::from([(
                        "path".into(),
                        serde_json::json!("src/audit.rs"),
                    )]),
                },
            ],
            edges: vec![crate::graph::GraphImportEdge {
                from: "semantic:workspace-command-guard".into(),
                to: "semantic:audit-command".into(),
                kind: EdgeKind::Calls,
            }],
        },
    )
    .unwrap();
    let context = runtime
        .software_context(
            "demo",
            &workspace,
            &index,
            &HashSet::new(),
            &SoftwareContextRequest {
                query: "workspace command security".into(),
                intent: "inspect".into(),
                budget: 1_000,
                scopes: vec![],
            },
        )
        .unwrap();
    assert_eq!(context.budget, 1_000);
    assert_eq!(context.requirements, vec!["REQ-SEC-1"]);
    assert_eq!(context.components, vec!["component:security"]);
    assert!(context.requirements.len() <= 4);
    assert_eq!(context.coverage.requirements_returned, 1);
    assert_eq!(context.coverage.requirements[0].id, "REQ-SEC-1");
    assert!(context.coverage.truncated);
    assert!(context
        .graph_context
        .nodes
        .iter()
        .any(|node| node.id == "semantic:workspace-command-guard"));
    assert!(context.graph_context.edges.iter().any(|edge| {
        edge.from == "semantic:workspace-command-guard"
            && edge.to == "semantic:audit-command"
            && edge.kind == EdgeKind::Calls
    }));
}

#[test]
fn software_context_scopes_narrow_source_navigation() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/graph")).unwrap();
    fs::create_dir_all(dir.path().join("src/verification")).unwrap();
    fs::write(
        dir.path().join("src/graph/scoped.rs"),
        "pub fn scope_marker_graph() {}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/verification/scoped.rs"),
        "pub fn scope_marker_verification() {}\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let index = CodeIndex::new().unwrap();
    let context = runtime
        .software_context(
            "demo",
            &workspace,
            &index,
            &HashSet::new(),
            &SoftwareContextRequest {
                query: "scope marker".into(),
                intent: "inspect".into(),
                budget: 4_000,
                scopes: vec!["software graph".into()],
            },
        )
        .unwrap();

    assert_eq!(context.scopes, vec!["graph"]);
    assert!(!context.symbols.is_empty());
    assert!(context.symbols.iter().all(|symbol| {
        symbol
            .get("path")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|path| path.starts_with("src/graph/"))
    }));
    assert!(context.symbols.iter().any(|symbol| {
        symbol
            .get("qualified_name")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|name| name.contains("scope_marker_graph"))
    }));
}

#[test]
fn transitive_call_impact_walks_reverse_callers() {
    use crate::graph::{GraphEdge, GraphNode, GraphPrecision, GraphProvenance, SoftwareGraph};
    let provenance = GraphProvenance {
        provider: "fixture".into(),
        precision: GraphPrecision::Syntax,
        revision: "sha256:fixture".into(),
    };
    let mut graph = SoftwareGraph::default();
    for (id, path, name) in [
        ("symbol:callee", "src/callee.rs", "callee"),
        ("symbol:caller", "src/caller.rs", "caller"),
    ] {
        graph
            .add_node(GraphNode {
                id: id.into(),
                kind: NodeKind::Function,
                label: name.into(),
                attributes: BTreeMap::from([
                    ("path".into(), serde_json::json!(path)),
                    ("qualified_name".into(), serde_json::json!(name)),
                ]),
                provenance: provenance.clone(),
            })
            .unwrap();
    }
    graph
        .add_edge(GraphEdge {
            from: "symbol:caller".into(),
            to: "symbol:callee".into(),
            kind: EdgeKind::Calls,
            provenance,
        })
        .unwrap();
    let snapshot = SoftwareGraphSnapshot {
        workspace: "demo".into(),
        path: ".".into(),
        provider: "tree-sitter".into(),
        precision: crate::graph::GraphPrecision::Syntax,
        files_considered: 2,
        files_indexed: 2,
        files_failed: 0,
        scan_truncated: false,
        truncated: false,
        node_count: 2,
        edge_count: 1,
        failures: vec![],
        graph,
    };
    let changed = HashSet::from(["src/callee.rs".to_owned()]);
    let (paths, symbols, callers) = transitive_call_impact(&snapshot, &changed);
    assert!(paths.contains("src/callee.rs"));
    assert!(paths.contains("src/caller.rs"));
    assert!(symbols.contains("src/caller.rs::caller"));
    assert_eq!(callers, 1);
}
