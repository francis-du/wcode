use super::*;
use crate::graph::NodeKind;
use crate::harness::ToolHarness;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

// Run explicitly against a frozen candidate. This reports dependency evidence,
// not Acceptance completion, and never persists a graph or starts a service.
#[test]
#[ignore = "manual read-only current-repository architecture diagnostic"]
fn architecture_current_repository_dependency_diagnostic() -> Result<()> {
    const MAX_FILES: usize = 1_500;
    const MAX_SYMBOLS: usize = 5_000;
    let workspace = Workspace::new(env!("CARGO_MANIFEST_DIR"), false, false)?;
    let harness = ToolHarness::new(2)?;
    let before = harness.current_revision(&workspace)?;
    let load = design::load_design(&workspace)?;
    anyhow::ensure!(
        load.initialized && load.error_count() == 0,
        "valid Design is required"
    );
    let traceability = harness.traceability_status("architecture-diagnostic", &workspace)?;
    let index = CodeIndex::new()?;

    // Match the production Design-entry selection, but use the pure source
    // graph API: the harness graph wrapper also persists and merges providers.
    let mut symbols_by_path = BTreeMap::<&str, Vec<String>>::new();
    for component in load.state.components.values() {
        for reference in &component.implementation {
            if let design::CodeRef::Symbol { path, symbol } = reference {
                symbols_by_path
                    .entry(path)
                    .or_default()
                    .push(symbol.clone());
            }
        }
    }
    let mut priority_symbol_ids = HashSet::new();
    let mut missing_priority_files = Vec::new();
    for (path, symbols) in symbols_by_path {
        let resolved = match index.resolve_symbols(&workspace, path, &symbols) {
            Ok(resolved) => resolved,
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                missing_priority_files.push(path);
                continue;
            }
            Err(error) => return Err(error),
        };
        priority_symbol_ids.extend(resolved.into_values().flatten().map(|symbol| symbol.id));
    }
    let graph = index.software_graph_with_priorities(
        "architecture-diagnostic",
        &workspace,
        ".",
        MAX_FILES,
        MAX_SYMBOLS,
        &priority_symbol_ids,
    )?;
    let actual = actual_component_dependencies(&load.state, &traceability, &graph);
    let desired = desired_component_dependencies(&load.state);
    let alignment = project_dependency_alignment(&load.state, &desired, &actual);
    let syntax_definitions = graph
        .graph
        .nodes
        .values()
        .filter(|node| {
            node.provenance.precision == GraphPrecision::Syntax && node.kind != NodeKind::File
        })
        .count();
    anyhow::ensure!(
        syntax_definitions <= MAX_SYMBOLS,
        "source symbol budget exceeded"
    );

    let mut entries = Vec::new();
    for (path, symbol) in [
        (
            "src/runtime/harness/agent_context.rs",
            "ToolHarness::agent_context",
        ),
        (
            "src/runtime/harness/repo_map.rs",
            "ToolHarness::ranked_repo_map",
        ),
        (
            "src/runtime/harness/graph.rs",
            "ToolHarness::software_graph_from_design",
        ),
        ("src/graph/code_index/api.rs", "CodeIndex::software_graph"),
        (
            "src/graph/code_index/api.rs",
            "CodeIndex::software_graph_with_priorities",
        ),
        ("src/workspace/edits.rs", "Workspace::apply_edits"),
        ("src/workspace/roots.rs", "Workspace::existing_path"),
        (
            "src/runtime/harness/quality/verification_run.rs",
            "ToolHarness::verify_project",
        ),
    ] {
        let node_id = index
            .resolve_symbol(&workspace, path, symbol)?
            .map(|resolved| format!("symbol:{}", resolved.id));
        entries.push(json!({
            "path": path, "symbol": symbol,
            "resolved_node_id": node_id,
            "emitted": node_id.as_ref().is_some_and(|id| graph.graph.nodes.contains_key(id)),
        }));
    }

    let representatives = [
        ("component:agent-context", "component:workspace-operations"),
        ("component:agent-context", "component:verification-harness"),
        ("component:agent-context", "component:software-graph"),
        ("component:experience-graph", "component:agent-context"),
        (
            "component:workspace-security",
            "component:workspace-operations",
        ),
    ]
    .into_iter()
    .map(|(from, to)| {
        let key = (from.to_owned(), to.to_owned());
        let row = alignment
            .iter()
            .find(|row| row.from == from && row.to == to);
        let observation = actual.confirmed.get(&key);
        json!({
            "from": from, "to": to,
            "desired": desired.contains(&key), "confirmed": observation.is_some(),
            "ledger_status": row.map(|row| row.status.as_str()),
            "evidence_total": observation.map_or(0, |value| value.evidence_total),
            "evidence": observation.map(|value| value.evidence.iter().take(3).collect::<Vec<_>>()),
        })
    })
    .collect::<Vec<_>>();

    // These probes retain only existing source edges and run the same mapper.
    // They expose representative ownership without relying on the global
    // bounded candidate sample retaining either example.
    let source_edge_probes = [
        diagnostic_source_edge(
            &index,
            &workspace,
            &load.state,
            &traceability,
            &graph,
            (
                "src/runtime/harness/repo_map.rs",
                "ToolHarness::ranked_repo_map",
            ),
            (
                "src/runtime/harness/retrieval.rs",
                "classify_repo_map_intent",
            ),
        )?,
        diagnostic_source_edge(
            &index,
            &workspace,
            &load.state,
            &traceability,
            &graph,
            ("src/workspace/edits.rs", "Workspace::replace_text"),
            ("src/workspace/mod.rs", "portable_relative_path"),
        )?,
    ];
    let after = harness.current_revision(&workspace)?;
    anyhow::ensure!(
        before.code == after.code && before.design == after.design,
        "repository source or Design changed during diagnostic"
    );
    let snapshot_sha256 = format!("sha256:{:x}", Sha256::digest(serde_json::to_vec(&graph)?));
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema_version": 1,
            "scope": "native_syntax_source_graph_with_real_traceability",
            "workspace_root": workspace.root().display().to_string(),
            "excluded_graph_layers": ["persisted_design_overlay", "imported_semantic_or_runtime_providers"],
            "functional_acceptance": false,
            "persistent_graph_written": false,
            "workspace_write_enabled": workspace.write_enabled(),
            "workspace_exec_enabled": workspace.exec_enabled(),
            "source_and_design_stable": true,
            "revision_before": before, "revision_after": after,
            "in_memory_snapshot_sha256": snapshot_sha256,
            "bounds": {"files": MAX_FILES, "source_symbols": MAX_SYMBOLS},
            "graph": {
                "provider": graph.provider, "precision": graph.precision,
                "files_considered": graph.files_considered, "files_indexed": graph.files_indexed,
                "files_failed": graph.files_failed, "failures": graph.failures,
                "scan_truncated": graph.scan_truncated, "graph_truncated": graph.truncated,
                "nodes": graph.node_count, "edges": graph.edge_count,
                "source_definitions": syntax_definitions,
                "resolved_design_priority_ids": priority_symbol_ids.len(),
                "missing_design_priority_files": missing_priority_files,
            },
            "traceability": {
                "valid_design": traceability.valid_design,
                "requirements_total": traceability.requirements_total,
                "requirements_returned": traceability.requirements_returned,
                "truncated": traceability.truncated,
                "design_to_implementation": traceability.design_to_implementation,
                "acceptance_to_verification_mapping": traceability.acceptance_to_verification,
            },
            "dependency_pairs": {
                "declared": desired.len(), "confirmed_observed": actual.confirmed.len(),
                "ledger_union": alignment.len(),
                "declared_and_observed": alignment.iter().filter(|row| row.desired && row.actual).count(),
                "declared_not_observed": alignment.iter().filter(|row| row.desired && !row.actual).count(),
                "observed_not_declared": alignment.iter().filter(|row| !row.desired && row.actual).count(),
            },
            "candidate_evidence": {
                "total": actual.candidates_total, "retained": actual.candidates.len(),
                "truncated": actual.candidates_total > actual.candidates.len(),
                "unit": "source-edge observations, not component pairs",
            },
            "key_entries": entries, "representative_pairs": representatives,
            "source_edge_probes": source_edge_probes,
        }))?
    );
    Ok(())
}

fn diagnostic_source_edge(
    index: &CodeIndex,
    workspace: &Workspace,
    state: &design::DesignState,
    traceability: &TraceabilityStatus,
    graph: &SoftwareGraphSnapshot,
    from: (&str, &str),
    to: (&str, &str),
) -> Result<Value> {
    let from_node = index
        .resolve_symbol(workspace, from.0, from.1)?
        .map(|symbol| format!("symbol:{}", symbol.id));
    let to_node = index
        .resolve_symbol(workspace, to.0, to.1)?
        .map(|symbol| format!("symbol:{}", symbol.id));
    let mut scoped = graph.clone();
    scoped.graph.edges.retain(|edge| {
        from_node.as_ref() == Some(&edge.from) && to_node.as_ref() == Some(&edge.to)
    });
    scoped.edge_count = scoped.graph.edges.len();
    let observations = actual_component_dependencies(state, traceability, &scoped);
    let confirmed = observations
        .confirmed
        .iter()
        .map(|((from, to), value)| {
            json!({
                "from": from, "to": to, "precision": value.precision,
                "evidence_total": value.evidence_total, "evidence": value.evidence,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "from_path": from.0, "from_symbol": from.1,
        "to_path": to.0, "to_symbol": to.1,
        "existing_source_edges": scoped.edge_count,
        "confirmed_pairs": confirmed,
        "candidate_evidence_total": observations.candidates_total,
        "candidate_evidence": observations.candidates,
    }))
}
