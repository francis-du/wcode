use super::*;

impl ToolHarness {
    pub fn graph_provider_import(
        &self,
        workspace: &Workspace,
        import: GraphProviderImport,
    ) -> Result<StoredGraphProvider> {
        graph_provider_store::persist(workspace, &import)
    }

    pub fn graph_provider_status(
        &self,
        workspace: &Workspace,
    ) -> Result<Vec<GraphProviderSummary>> {
        graph_provider_store::summaries(workspace)
    }

    pub fn graph_history(
        &self,
        workspace: &Workspace,
        limit: usize,
    ) -> Result<Vec<GraphHistoryEntry>> {
        graph_store::history(workspace, limit)
    }

    pub(crate) fn observatory_graph_signal(
        &self,
        workspace: &Workspace,
    ) -> Result<Option<(String, String)>> {
        graph_store::change_signal(workspace)
    }

    pub fn graph_query(
        &self,
        workspace: &Workspace,
        input: &GraphQueryInput,
    ) -> Result<GraphQueryResult> {
        graph_store::query(workspace, input)
    }

    pub fn graph_chain(
        &self,
        workspace: &Workspace,
        input: &GraphChainInput,
    ) -> Result<GraphChainResult> {
        graph_store::chain(workspace, input)
    }

    pub(crate) fn graph_search(
        &self,
        workspace: &Workspace,
        input: &GraphSearchInput,
    ) -> Result<GraphSearchResult> {
        crate::graph_explorer::search(workspace, input)
    }

    pub(crate) fn graph_overview(
        &self,
        workspace: &Workspace,
        input: &GraphOverviewInput,
    ) -> Result<GraphOverviewResult> {
        crate::graph_explorer::overview(workspace, input)
    }

    pub fn graph_diff(
        &self,
        workspace: &Workspace,
        input: &GraphDiffInput,
    ) -> Result<GraphDiffResult> {
        graph_store::diff(workspace, input)
    }

    pub(super) fn software_graph_from_design(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        path: &str,
        max_files: usize,
        max_symbols: usize,
        load: &design::DesignLoad,
    ) -> Result<SoftwareGraphSnapshot> {
        let mut snapshot = self.code_index.software_graph(
            workspace_id,
            workspace,
            path,
            max_files,
            max_symbols,
        )?;
        let mut composite = false;
        if load.initialized {
            overlay_design_graph(&mut snapshot, &load.state, &self.code_index, workspace)?;
            composite = true;
        }
        if graph_provider_store::overlay_latest(workspace, &mut snapshot)? > 0 {
            composite = true;
        }
        if composite {
            snapshot.provider = "wcode-composite".to_owned();
            snapshot.precision = GraphPrecision::Mixed;
        }
        snapshot.node_count = snapshot.graph.nodes.len();
        snapshot.edge_count = snapshot.graph.edges.len();
        snapshot.graph.validate()?;
        graph_store::persist(workspace, &snapshot)?;
        Ok(snapshot)
    }
}

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

    overlay_design_documents(snapshot, workspace, &provenance)?;

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

fn overlay_design_documents(
    snapshot: &mut SoftwareGraphSnapshot,
    workspace: &Workspace,
    provenance: &GraphProvenance,
) -> Result<()> {
    let (mut paths, _) = workspace.source_files(design::DESIGN_ROOT, design::MAX_DESIGN_FILES)?;
    if workspace.root().join(design::PROJECT_FILE).is_file() {
        paths.push(design::PROJECT_FILE.to_owned());
    }
    paths.sort();
    paths.dedup();
    for path in paths {
        let Some(collection) = design_document_collection(&path) else {
            continue;
        };
        let source = workspace.load_source(&path)?;
        let target_ids = design_document_ids(&source.content);
        let node_id = format!("config:{path}");
        add_graph_node_if_absent(
            snapshot,
            GraphNode {
                id: node_id.clone(),
                kind: NodeKind::Config,
                label: path.clone(),
                attributes: BTreeMap::from([
                    ("path".to_owned(), json!(path)),
                    ("design_collection".to_owned(), json!(collection)),
                    ("entries".to_owned(), json!(target_ids.len())),
                ]),
                provenance: provenance.clone(),
            },
        )?;
        for target in target_ids {
            add_graph_edge_if_possible(
                snapshot,
                &node_id,
                &target,
                EdgeKind::Contains,
                provenance,
            )?;
        }
    }
    Ok(())
}

fn design_document_collection(path: &str) -> Option<&'static str> {
    if path == design::PROJECT_FILE {
        return Some("project");
    }
    let relative = path.strip_prefix(design::DESIGN_ROOT)?.strip_prefix('/')?;
    if !(relative.ends_with(".yaml") || relative.ends_with(".yml")) {
        return None;
    }
    for collection in [
        "product",
        "requirements",
        "components",
        "constraints",
        "decisions",
        "acceptance",
    ] {
        if relative == format!("{collection}.yaml") || relative == format!("{collection}.yml") {
            return Some(collection);
        }
        if collection != "product" && relative.starts_with(&format!("{collection}/")) {
            return Some(collection);
        }
    }
    None
}

fn design_document_ids(content: &str) -> Vec<String> {
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(content) else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    let mut push_mapping_id = |mapping: &serde_yaml::Mapping| {
        if let Some(id) = mapping.iter().find_map(|(key, value)| {
            (key.as_str() == Some("id"))
                .then(|| value.as_str())
                .flatten()
        }) {
            ids.push(id.to_owned());
        }
    };
    match value {
        serde_yaml::Value::Mapping(mapping) => push_mapping_id(&mapping),
        serde_yaml::Value::Sequence(items) => {
            for item in items {
                if let serde_yaml::Value::Mapping(mapping) = item {
                    push_mapping_id(&mapping);
                }
            }
        }
        _ => {}
    }
    ids.sort();
    ids.dedup();
    ids
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
