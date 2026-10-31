use super::*;
use crate::graph::{GraphEdge, GraphNode, GraphProvenance, SoftwareGraph};
use crate::intelligence_types::{CoverageDimension, ProjectArchitectureView, TraceReferenceKind};

fn component(
    id: &str,
    implementation: Vec<design::CodeRef>,
    depends_on: &[&str],
) -> design::ComponentDesign {
    design::ComponentDesign {
        schema_version: 1,
        id: id.to_owned(),
        name: id.to_owned(),
        responsibilities: Vec::new(),
        depends_on: depends_on.iter().map(|id| (*id).to_owned()).collect(),
        constraints: Vec::new(),
        implementation,
    }
}

fn symbol(path: &str, symbol: &str) -> design::CodeRef {
    design::CodeRef::Symbol {
        path: path.to_owned(),
        symbol: symbol.to_owned(),
    }
}

fn node(name: &str, path: &str) -> GraphNode {
    GraphNode {
        id: format!("symbol:{name}"),
        kind: crate::graph::NodeKind::Function,
        label: name.to_owned(),
        attributes: BTreeMap::from([
            ("path".to_owned(), serde_json::json!(path)),
            ("qualified_name".to_owned(), serde_json::json!(name)),
            ("name".to_owned(), serde_json::json!(name)),
        ]),
        provenance: GraphProvenance {
            provider: "tree-sitter".to_owned(),
            precision: GraphPrecision::Syntax,
            revision: "fixture".to_owned(),
        },
    }
}

fn edge(from: &str, to: &str, precision: GraphPrecision, provider: &str) -> GraphEdge {
    GraphEdge {
        from: format!("symbol:{from}"),
        to: format!("symbol:{to}"),
        kind: crate::graph::EdgeKind::Calls,
        provenance: GraphProvenance {
            provider: provider.to_owned(),
            precision,
            revision: "fixture".to_owned(),
        },
    }
}

fn snapshot(nodes: Vec<GraphNode>, edges: Vec<GraphEdge>) -> SoftwareGraphSnapshot {
    SoftwareGraphSnapshot {
        workspace: "fixture".to_owned(),
        path: ".".to_owned(),
        provider: "fixture".to_owned(),
        precision: GraphPrecision::Mixed,
        files_considered: 0,
        files_indexed: 0,
        files_failed: 0,
        scan_truncated: false,
        truncated: false,
        node_count: nodes.len(),
        edge_count: edges.len(),
        failures: Vec::new(),
        graph: SoftwareGraph {
            nodes: nodes
                .into_iter()
                .map(|node| (node.id.clone(), node))
                .collect(),
            edges,
        },
    }
}

fn traceability() -> TraceabilityStatus {
    TraceabilityStatus {
        workspace: "fixture".to_owned(),
        initialized: true,
        valid_design: true,
        requirements_total: 0,
        requirements_returned: 0,
        truncated: false,
        requirement_to_component: CoverageDimension::new(0, 0),
        design_to_implementation: CoverageDimension::new(0, 0),
        acceptance_to_verification: CoverageDimension::new(0, 0),
        complete_requirements: 0,
        partial_requirements: 0,
        missing_requirements: 0,
        requirements: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn projection(
    state: &design::DesignState,
    traceability: &TraceabilityStatus,
    graph: &SoftwareGraphSnapshot,
) -> ProjectArchitectureView {
    let observations = actual_component_dependencies(state, traceability, graph);
    let dependencies =
        project_dependency_alignment(state, &desired_component_dependencies(state), &observations);
    super::super::observatory_architecture::build_project_architecture(
        state,
        &BTreeMap::new(),
        &HashSet::new(),
        dependencies,
        &observations,
    )
}

#[test]
fn dependency_ownership_shared_symbols_remain_candidates() {
    let mut state = design::DesignState::default();
    for component in [
        component(
            "component:agent-context",
            vec![
                symbol("src/context/entry.rs", "assemble"),
                symbol("src/experience/memory.rs", "recall"),
            ],
            &["component:experience"],
        ),
        component(
            "component:experience",
            vec![symbol("src/experience/memory.rs", "recall")],
            &[],
        ),
    ] {
        state.components.insert(component.id.clone(), component);
    }
    let graph = snapshot(
        vec![
            node("assemble", "src/context/entry.rs"),
            node("recall", "src/experience/memory.rs"),
        ],
        vec![edge(
            "recall",
            "assemble",
            GraphPrecision::Semantic,
            "rust-analyzer",
        )],
    );
    let mut trace = traceability();
    trace.requirements.push(RequirementTrace {
        id: "REQ-SHARED".to_owned(),
        title: "Shared feature mapping".to_owned(),
        priority: design::Priority::Medium,
        components: vec!["component:agent-context".to_owned()],
        acceptance_criteria: Vec::new(),
        implementation: vec![TraceReference {
            owner: "component:agent-context".to_owned(),
            kind: TraceReferenceKind::Symbol,
            target: "src/experience/memory.rs::recall".to_owned(),
            resolved: true,
            provider: "rust-analyzer".to_owned(),
            precision: "semantic".to_owned(),
            node_id: Some("symbol:recall".to_owned()),
            revision: Some("fixture".to_owned()),
            message: None,
        }],
        verification: Vec::new(),
        status: RequirementTraceStatus::Complete,
    });
    let architecture = projection(&state, &trace, &graph);
    assert_eq!(architecture.observed_edges, 0);
    assert_eq!(architecture.aligned_edges, 0);
    assert_eq!(architecture.blocking_drift_edges, 0);
    assert_eq!(architecture.dependencies.len(), 1);
    assert_eq!(architecture.dependencies[0].status, "unverified_actual");
    assert!(!architecture.dependencies[0].actual);
    assert!(architecture
        .subsystems
        .iter()
        .all(|subsystem| subsystem.observed_depends_on.is_empty()));
    assert_eq!(architecture.dependency_candidates_total, 1);
    assert!(!architecture.dependency_candidates_truncated);
    let candidate = &architecture.dependency_candidates[0];
    assert_eq!(candidate.ownership, "ambiguous");
    assert_eq!(candidate.from_node, "symbol:recall");
    assert_eq!(candidate.to_node, "symbol:assemble");
    assert_eq!(
        candidate.from_path.as_deref(),
        Some("src/experience/memory.rs")
    );
    assert_eq!(candidate.to_path.as_deref(), Some("src/context/entry.rs"));
    assert_eq!(candidate.from_symbol.as_deref(), Some("recall"));
    assert_eq!(candidate.to_symbol.as_deref(), Some("assemble"));
    assert_eq!(candidate.provider, "rust-analyzer");
    assert_eq!(candidate.precision, "semantic");
    assert_eq!(candidate.revision, "fixture");
    assert_eq!(candidate.from_owners_total, 2);
    assert_eq!(
        candidate.from_owners[0].sources,
        ["symbol_reference", "trace_symbol"]
    );
    for index in 0..20 {
        let component = component(
            &format!("component:extra-{index:02}"),
            vec![symbol("src/experience/memory.rs", "recall")],
            &[],
        );
        state.components.insert(component.id.clone(), component);
    }
    let bounded = projection(&state, &trace, &graph);
    let candidate = &bounded.dependency_candidates[0];
    assert_eq!(candidate.from_owners_total, 22);
    assert_eq!(candidate.from_owners.len(), 16);
    assert_eq!(bounded.observed_edges, 0);
}

#[test]
fn dependency_ownership_symbol_reference_does_not_claim_sibling_functions() {
    let mut state = design::DesignState::default();
    for component in [
        component(
            "component:workspace-operations",
            vec![symbol("src/workspace/paths.rs", "named_operation")],
            &["component:security"],
        ),
        component(
            "component:security",
            vec![symbol("src/security/guard.rs", "check_access")],
            &[],
        ),
    ] {
        state.components.insert(component.id.clone(), component);
    }
    let mut edges = (0..200)
        .map(|index| {
            edge(
                "unmapped_a",
                "unmapped_b",
                GraphPrecision::Semantic,
                &format!("unmapped-{index:03}"),
            )
        })
        .collect::<Vec<_>>();
    edges.push(edge(
        "check_access",
        "portable_relative_path",
        GraphPrecision::Semantic,
        "rust-analyzer",
    ));
    let graph = snapshot(
        vec![
            node("named_operation", "src/workspace/paths.rs"),
            node("portable_relative_path", "src/workspace/paths.rs"),
            node("check_access", "src/security/guard.rs"),
            node("unmapped_a", "src/unmapped/a.rs"),
            node("unmapped_b", "src/unmapped/b.rs"),
        ],
        edges,
    );
    let architecture = projection(&state, &traceability(), &graph);
    assert_eq!(architecture.observed_edges, 0);
    assert_eq!(architecture.blocking_drift_edges, 0);
    assert!(!architecture.dependencies.iter().any(|edge| {
        edge.from == "component:security" && edge.to == "component:workspace-operations"
    }));
    assert_eq!(architecture.dependency_candidates_total, 1);
    assert_eq!(architecture.dependency_candidates.len(), 1);
    assert!(!architecture.dependency_candidates_truncated);
    let candidate = &architecture.dependency_candidates[0];
    assert_eq!(candidate.ownership, "unowned");
    assert_eq!(candidate.to_path.as_deref(), Some("src/workspace/paths.rs"));
    assert_eq!(
        candidate.to_symbol.as_deref(),
        Some("portable_relative_path")
    );
    assert_eq!(candidate.from_owners_total, 1);
    assert_eq!(candidate.to_owners_total, 0);
    assert!(candidate.to_owners.is_empty());
}

#[test]
fn dependency_ownership_explicit_files_preserve_bounded_precision_evidence() {
    let mut state = design::DesignState::default();
    for component in [
        component(
            "component:caller",
            vec![symbol("src/caller/entry.rs", "entry")],
            &["component:backend"],
        ),
        component(
            "component:backend",
            vec![design::CodeRef::File {
                path: "src/backend/lib.rs".to_owned(),
            }],
            &[],
        ),
    ] {
        state.components.insert(component.id.clone(), component);
    }
    let mut edges = (0..12)
        .map(|index| {
            edge(
                "entry",
                "helper",
                if index == 11 {
                    GraphPrecision::Semantic
                } else {
                    GraphPrecision::Syntax
                },
                &format!("observed-{index:03}"),
            )
        })
        .collect::<Vec<_>>();
    edges.extend((0..130).map(|index| {
        edge(
            "entry",
            "helper",
            GraphPrecision::Heuristic,
            &format!("heuristic-{index:03}"),
        )
    }));
    edges.push(edge("entry", "helper", GraphPrecision::Declared, "design"));
    edges.push(edge("entry", "helper", GraphPrecision::Mixed, "mixed"));
    let mut graph = snapshot(
        vec![
            node("entry", "src/caller/entry.rs"),
            node("helper", "src/backend/lib.rs"),
        ],
        edges,
    );
    let trace = traceability();
    let architecture = projection(&state, &trace, &graph);
    assert_eq!(architecture.observed_edges, 1);
    assert_eq!(architecture.aligned_edges, 1);
    assert_eq!(architecture.evidence_coverage_percent, 100.0);
    let observed = &architecture.dependencies[0];
    assert_eq!(observed.precision, "semantic");
    assert_eq!(observed.evidence_total, 12);
    assert_eq!(observed.evidence.len(), 8);
    assert!(observed.evidence_truncated);
    assert_eq!(observed.evidence[0].precision, "semantic");
    assert!(observed
        .evidence
        .iter()
        .any(|edge| edge.precision == "syntax"));
    assert_eq!(observed.evidence[0].from_symbol.as_deref(), Some("entry"));
    assert_eq!(observed.evidence[0].to_symbol.as_deref(), Some("helper"));
    assert_eq!(
        observed.evidence[0].from_owners[0].sources,
        ["symbol_reference"]
    );
    assert_eq!(
        observed.evidence[0].to_owners[0].sources,
        ["file_reference"]
    );
    assert_eq!(architecture.dependency_candidates_total, 132);
    assert_eq!(architecture.dependency_candidates.len(), 128);
    assert!(architecture.dependency_candidates_truncated);
    assert!(architecture
        .dependency_candidates
        .iter()
        .all(|edge| edge.ownership == "unique"));
    graph.graph.edges.reverse();
    let reordered = projection(&state, &trace, &graph);
    assert_eq!(
        serde_json::to_value(&architecture).unwrap(),
        serde_json::to_value(reordered).unwrap()
    );

    graph.graph.edges.retain(|edge| {
        matches!(
            edge.provenance.precision,
            GraphPrecision::Declared | GraphPrecision::Heuristic | GraphPrecision::Mixed
        )
    });
    let unconfirmed = projection(&state, &trace, &graph);
    assert_eq!(unconfirmed.observed_edges, 0);
    assert_eq!(unconfirmed.aligned_edges, 0);
    assert_eq!(unconfirmed.blocking_drift_edges, 0);
    assert_eq!(unconfirmed.dependencies[0].status, "unverified_actual");
    assert_eq!(unconfirmed.dependencies[0].evidence_total, 0);
}

#[test]
fn dependency_ownership_unqualified_names_need_unique_resolution() {
    let mut state = design::DesignState::default();
    for component in [
        component(
            "component:caller",
            vec![symbol("src/caller.rs", "entry")],
            &["component:backend"],
        ),
        component(
            "component:backend",
            vec![symbol("src/backend.rs", "run")],
            &[],
        ),
    ] {
        state.components.insert(component.id.clone(), component);
    }
    let mut left = node("Left::run", "src/backend.rs");
    left.attributes
        .insert("name".to_owned(), serde_json::json!("run"));
    let mut right = node("Right::run", "src/backend.rs");
    right
        .attributes
        .insert("name".to_owned(), serde_json::json!("run"));
    let mut graph = snapshot(
        vec![node("entry", "src/caller.rs"), left, right],
        vec![edge(
            "entry",
            "Left::run",
            GraphPrecision::Semantic,
            "rust-analyzer",
        )],
    );
    let ambiguous = projection(&state, &traceability(), &graph);
    assert_eq!(ambiguous.observed_edges, 0);
    assert_eq!(ambiguous.dependency_candidates[0].ownership, "ambiguous");
    assert_eq!(ambiguous.dependency_candidates[0].to_owners_total, 1);
    assert_eq!(
        ambiguous.dependency_candidates[0].to_owners[0].sources,
        ["ambiguous_symbol_reference"]
    );

    graph.graph.nodes.remove("symbol:Right::run");
    graph.node_count = graph.graph.nodes.len();
    graph.truncated = true;
    let bounded = projection(&state, &traceability(), &graph);
    assert_eq!(
        bounded.observed_edges, 0,
        "bounded absence cannot prove unique ownership"
    );

    let reference = |owner: &str, target: &str, node: Option<&str>| TraceReference {
        owner: owner.to_owned(),
        kind: TraceReferenceKind::Symbol,
        target: target.to_owned(),
        resolved: node.is_some(),
        provider: "tree-sitter".to_owned(),
        precision: "syntax".to_owned(),
        node_id: node.map(str::to_owned),
        revision: Some("fixture".to_owned()),
        message: None,
    };
    let mut trace = traceability();
    trace.requirements.push(RequirementTrace {
        id: "REQ-RESOLUTION".to_owned(),
        title: "Resolution provenance".to_owned(),
        priority: design::Priority::Medium,
        components: vec![
            "component:caller".to_owned(),
            "component:backend".to_owned(),
        ],
        acceptance_criteria: Vec::new(),
        implementation: vec![reference("component:backend", "src/backend.rs::run", None)],
        verification: Vec::new(),
        status: RequirementTraceStatus::Partial,
    });
    graph.truncated = false;
    let unresolved = projection(&state, &trace, &graph);
    assert_eq!(
        unresolved.observed_edges, 0,
        "unresolved canonical trace must not be promoted"
    );
    assert_eq!(unresolved.dependency_candidates[0].ownership, "ambiguous");

    state
        .components
        .get_mut("component:backend")
        .unwrap()
        .implementation = vec![symbol("src/backend.rs", "Left::run")];
    let qualified = projection(&state, &traceability(), &graph);
    assert_eq!(qualified.observed_edges, 1);
    assert_eq!(
        qualified.dependencies[0].evidence[0].to_symbol.as_deref(),
        Some("Left::run")
    );

    graph.truncated = true;
    trace.requirements[0].implementation = vec![
        reference(
            "component:caller",
            "src/caller.rs::entry",
            Some("symbol:entry"),
        ),
        reference(
            "component:backend",
            "src/backend.rs::Left::run",
            Some("symbol:Left::run"),
        ),
    ];
    let resolved = projection(&state, &trace, &graph);
    assert_eq!(
        resolved.observed_edges, 1,
        "resolved canonical trace remains usable in a bounded graph"
    );
    assert!(resolved.dependencies[0].evidence[0].to_owners[0]
        .sources
        .iter()
        .any(|source| source == "trace_symbol"));
}
