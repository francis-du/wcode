use super::*;

pub(super) fn design_product_id(name: &str) -> String {
    let mut slug = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "project" } else { slug };
    format!("product:{}", slug.chars().take(120).collect::<String>())
}

pub(super) fn overlay_design_graph(
    snapshot: &mut SoftwareGraphSnapshot,
    state: &design::DesignState,
    code_index: &CodeIndex,
    workspace: &Workspace,
) -> Result<()> {
    let revision = design_state_revision(state)?;
    let provenance = GraphProvenance {
        provider: "wcode-design".to_owned(),
        precision: GraphPrecision::Declared,
        revision,
    };

    if let Some(product) = &state.product {
        let mut attributes = BTreeMap::new();
        attributes.insert("name".to_owned(), json!(product.name));
        attributes.insert("vision".to_owned(), json!(product.vision));
        add_graph_node_if_absent(
            snapshot,
            GraphNode {
                id: product.id.clone(),
                kind: NodeKind::Product,
                label: product.name.clone(),
                attributes,
                provenance: provenance.clone(),
            },
        )?;
    }

    for requirement in state.requirements.values() {
        let mut attributes = BTreeMap::new();
        attributes.insert("title".to_owned(), json!(requirement.title));
        attributes.insert("intent".to_owned(), json!(requirement.intent));
        attributes.insert(
            "priority".to_owned(),
            serde_json::to_value(requirement.priority)?,
        );
        add_graph_node_if_absent(
            snapshot,
            GraphNode {
                id: requirement.id.clone(),
                kind: NodeKind::Requirement,
                label: requirement.title.clone(),
                attributes,
                provenance: provenance.clone(),
            },
        )?;
    }
    for component in state.components.values() {
        let mut attributes = BTreeMap::new();
        attributes.insert("name".to_owned(), json!(component.name));
        attributes.insert(
            "responsibilities".to_owned(),
            json!(component.responsibilities),
        );
        add_graph_node_if_absent(
            snapshot,
            GraphNode {
                id: component.id.clone(),
                kind: NodeKind::Component,
                label: component.name.clone(),
                attributes,
                provenance: provenance.clone(),
            },
        )?;
    }
    for constraint in state.constraints.values() {
        let mut attributes = BTreeMap::new();
        attributes.insert("statement".to_owned(), json!(constraint.statement));
        add_graph_node_if_absent(
            snapshot,
            GraphNode {
                id: constraint.id.clone(),
                kind: NodeKind::Constraint,
                label: constraint.title.clone(),
                attributes,
                provenance: provenance.clone(),
            },
        )?;
    }
    for criterion in state.acceptance.values() {
        let mut attributes = BTreeMap::new();
        attributes.insert("statement".to_owned(), json!(criterion.statement));
        add_graph_node_if_absent(
            snapshot,
            GraphNode {
                id: criterion.id.clone(),
                kind: NodeKind::AcceptanceCriterion,
                label: criterion.title.clone(),
                attributes,
                provenance: provenance.clone(),
            },
        )?;
    }
    for decision in state.decisions.values() {
        let mut attributes = BTreeMap::new();
        attributes.insert("decision".to_owned(), json!(decision.decision));
        attributes.insert("status".to_owned(), serde_json::to_value(decision.status)?);
        add_graph_node_if_absent(
            snapshot,
            GraphNode {
                id: decision.id.clone(),
                kind: NodeKind::Decision,
                label: decision.title.clone(),
                attributes,
                provenance: provenance.clone(),
            },
        )?;
    }

    for requirement in state.requirements.values() {
        for component in &requirement.implemented_by {
            add_graph_edge_if_possible(
                snapshot,
                component,
                &requirement.id,
                EdgeKind::ImplementsRequirement,
                &provenance,
            )?;
        }
        for constraint in &requirement.constraints {
            add_graph_edge_if_possible(
                snapshot,
                &requirement.id,
                constraint,
                EdgeKind::ConstrainedBy,
                &provenance,
            )?;
        }
        for criterion in &requirement.acceptance {
            add_graph_edge_if_possible(
                snapshot,
                &requirement.id,
                criterion,
                EdgeKind::VerifiedBy,
                &provenance,
            )?;
        }
    }

    for component in state.components.values() {
        for dependency in &component.depends_on {
            add_graph_edge_if_possible(
                snapshot,
                &component.id,
                dependency,
                EdgeKind::DependsOn,
                &provenance,
            )?;
        }
        for constraint in &component.constraints {
            add_graph_edge_if_possible(
                snapshot,
                &component.id,
                constraint,
                EdgeKind::ConstrainedBy,
                &provenance,
            )?;
        }
        for reference in &component.implementation {
            let target = match reference {
                CodeRef::File { path } => Some(format!("file:{path}")),
                CodeRef::Symbol { path, symbol } => code_index
                    .resolve_symbol(workspace, path, symbol)?
                    .map(|resolution| format!("symbol:{}", resolution.id)),
            };
            if let Some(target) = target {
                add_graph_edge_if_possible(
                    snapshot,
                    &component.id,
                    &target,
                    EdgeKind::Implements,
                    &provenance,
                )?;
            }
        }
    }

    for constraint in state.constraints.values() {
        for target in &constraint.applies_to {
            add_graph_edge_if_possible(
                snapshot,
                target,
                &constraint.id,
                EdgeKind::ConstrainedBy,
                &provenance,
            )?;
        }
    }
    for decision in state.decisions.values() {
        for target in &decision.affects {
            add_graph_edge_if_possible(
                snapshot,
                &decision.id,
                target,
                EdgeKind::References,
                &provenance,
            )?;
        }
    }
    for criterion in state.acceptance.values() {
        for verification in &criterion.verification {
            match verification {
                VerificationRef::Test { path, symbol } => {
                    if let Some(resolution) = code_index.resolve_symbol(workspace, path, symbol)? {
                        add_graph_edge_if_possible(
                            snapshot,
                            &criterion.id,
                            &format!("symbol:{}", resolution.id),
                            EdgeKind::TestedBy,
                            &provenance,
                        )?;
                    }
                }
                VerificationRef::Check { id } => {
                    let node_id = format!("verification:{id}");
                    add_graph_node_if_absent(
                        snapshot,
                        GraphNode {
                            id: node_id.clone(),
                            kind: NodeKind::Verification,
                            label: id.clone(),
                            attributes: BTreeMap::from([(
                                "declared_check".to_owned(),
                                json!(true),
                            )]),
                            provenance: provenance.clone(),
                        },
                    )?;
                    add_graph_edge_if_possible(
                        snapshot,
                        &criterion.id,
                        &node_id,
                        EdgeKind::VerifiedBy,
                        &provenance,
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn design_state_revision(state: &design::DesignState) -> Result<String> {
    let encoded = serde_json::to_vec(state)?;
    let mut hasher = Sha256::new();
    hasher.update(&encoded);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn add_graph_node_if_absent(snapshot: &mut SoftwareGraphSnapshot, node: GraphNode) -> Result<()> {
    if !snapshot.graph.nodes.contains_key(&node.id) {
        snapshot.graph.add_node(node)?;
    }
    Ok(())
}

fn add_graph_edge_if_possible(
    snapshot: &mut SoftwareGraphSnapshot,
    from: &str,
    to: &str,
    kind: EdgeKind,
    provenance: &GraphProvenance,
) -> Result<()> {
    if from == to
        || !snapshot.graph.nodes.contains_key(from)
        || !snapshot.graph.nodes.contains_key(to)
        || snapshot
            .graph
            .edges
            .iter()
            .any(|edge| edge.from == from && edge.to == to && edge.kind == kind)
    {
        return Ok(());
    }
    snapshot.graph.add_edge(GraphEdge {
        from: from.to_owned(),
        to: to.to_owned(),
        kind,
        provenance: provenance.clone(),
    })?;
    Ok(())
}
