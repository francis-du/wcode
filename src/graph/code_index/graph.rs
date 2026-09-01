use super::*;

pub(super) fn append_file_graph(
    graph: &mut SoftwareGraph,
    record: &FileRecord,
    max_symbols: usize,
) -> Result<usize> {
    let _cpu = crate::resource::cpu_work(crate::resource::WorkClass::Interactive);
    let provenance = graph_provenance(record);
    let file_id = format!("file:{}", record.path);
    let mut file_attributes = BTreeMap::new();
    file_attributes.insert("path".to_owned(), json!(record.path));
    file_attributes.insert("language".to_owned(), json!(record.language.as_str()));
    file_attributes.insert("sha256".to_owned(), json!(record.sha256));
    file_attributes.insert("source_bytes".to_owned(), json!(record.source_bytes));
    file_attributes.insert("line_count".to_owned(), json!(record.line_count));
    file_attributes.insert("parse_errors".to_owned(), json!(record.parse_errors));
    graph.add_node(GraphNode {
        id: file_id.clone(),
        kind: NodeKind::File,
        label: record.path.clone(),
        attributes: file_attributes,
        provenance: provenance.clone(),
    })?;

    let definitions = record
        .symbols
        .iter()
        .filter(|symbol| symbol.is_definition)
        .take(max_symbols)
        .collect::<Vec<_>>();
    let mut targets_by_name = HashMap::<&str, Vec<&CodeSymbol>>::new();
    for symbol in &definitions {
        targets_by_name
            .entry(symbol.name.as_str())
            .or_default()
            .push(symbol);
        let node_id = graph_symbol_id(symbol);
        let mut attributes = BTreeMap::new();
        attributes.insert("path".to_owned(), json!(symbol.path));
        attributes.insert("name".to_owned(), json!(symbol.name));
        attributes.insert("qualified_name".to_owned(), json!(symbol.qualified_name));
        attributes.insert("symbol_kind".to_owned(), json!(symbol.kind));
        attributes.insert("language".to_owned(), json!(symbol.language));
        attributes.insert("range".to_owned(), serde_json::to_value(&symbol.range)?);
        graph.add_node(GraphNode {
            id: node_id.clone(),
            kind: graph_node_kind(&symbol.kind),
            label: symbol.qualified_name.clone(),
            attributes,
            provenance: provenance.clone(),
        })?;
        graph.add_edge(GraphEdge {
            from: file_id.clone(),
            to: node_id,
            kind: EdgeKind::Defines,
            provenance: provenance.clone(),
        })?;
    }

    let included = definitions
        .iter()
        .map(|symbol| symbol.id.as_str())
        .collect::<HashSet<_>>();
    let mut call_edges = HashSet::new();
    for call in record
        .symbols
        .iter()
        .filter(|symbol| !symbol.is_definition && symbol.kind == "call")
    {
        let Some(caller) = definitions
            .iter()
            .copied()
            .filter(|symbol| {
                symbol.start_byte <= call.start_byte && symbol.end_byte >= call.end_byte
            })
            .min_by_key(|symbol| symbol.end_byte.saturating_sub(symbol.start_byte))
        else {
            continue;
        };
        let Some(targets) = targets_by_name.get(call.name.as_str()) else {
            continue;
        };
        if targets.len() != 1 {
            continue;
        }
        let target = targets[0];
        if caller.id == target.id
            || !included.contains(caller.id.as_str())
            || !included.contains(target.id.as_str())
            || !call_edges.insert((caller.id.as_str(), target.id.as_str()))
        {
            continue;
        }
        graph.add_edge(GraphEdge {
            from: graph_symbol_id(caller),
            to: graph_symbol_id(target),
            kind: EdgeKind::Calls,
            provenance: provenance.clone(),
        })?;
    }

    Ok(definitions.len())
}

pub(super) fn append_cross_file_call_edges(
    graph: &mut SoftwareGraph,
    records: &[Arc<FileRecord>],
) -> Result<()> {
    let mut targets_by_name = HashMap::<&str, Vec<(&FileRecord, &CodeSymbol)>>::new();
    for record in records {
        for symbol in record.symbols.iter().filter(|symbol| symbol.is_definition) {
            if graph.nodes.contains_key(&graph_symbol_id(symbol)) {
                targets_by_name
                    .entry(symbol.name.as_str())
                    .or_default()
                    .push((record, symbol));
            }
        }
    }

    let mut existing = graph
        .edges
        .iter()
        .filter(|edge| edge.kind == EdgeKind::Calls)
        .map(|edge| (edge.from.clone(), edge.to.clone()))
        .collect::<HashSet<_>>();

    for record in records {
        let _cpu = crate::resource::cpu_work(crate::resource::WorkClass::Interactive);
        let definitions = record
            .symbols
            .iter()
            .filter(|symbol| symbol.is_definition)
            .collect::<Vec<_>>();
        for call in record
            .symbols
            .iter()
            .filter(|symbol| !symbol.is_definition && symbol.kind == "call")
        {
            let Some(caller) = definitions
                .iter()
                .copied()
                .filter(|symbol| {
                    symbol.start_byte <= call.start_byte && symbol.end_byte >= call.end_byte
                })
                .min_by_key(|symbol| symbol.end_byte.saturating_sub(symbol.start_byte))
            else {
                continue;
            };
            let Some(targets) = targets_by_name.get(call.name.as_str()) else {
                continue;
            };
            if targets.len() != 1 {
                continue;
            }
            let (target_record, target) = targets[0];
            if record.path == target_record.path {
                continue;
            }
            let from = graph_symbol_id(caller);
            let to = graph_symbol_id(target);
            if from == to
                || !graph.nodes.contains_key(&from)
                || !graph.nodes.contains_key(&to)
                || !existing.insert((from.clone(), to.clone()))
            {
                continue;
            }
            graph.add_edge(GraphEdge {
                from,
                to,
                kind: EdgeKind::Calls,
                provenance: GraphProvenance {
                    provider: "tree-sitter/global-name-resolution".to_owned(),
                    precision: GraphPrecision::Syntax,
                    revision: format!(
                        "caller:{};target:{}",
                        &record.sha256[..record.sha256.len().min(64)],
                        &target_record.sha256[..target_record.sha256.len().min(64)]
                    ),
                },
            })?;
        }
    }
    Ok(())
}

pub(super) fn graph_provenance(record: &FileRecord) -> GraphProvenance {
    GraphProvenance {
        provider: "tree-sitter".to_owned(),
        precision: GraphPrecision::Syntax,
        revision: format!("sha256:{}", record.sha256),
    }
}

pub(super) fn graph_symbol_id(symbol: &CodeSymbol) -> String {
    format!("symbol:{}", symbol.id)
}

pub(super) fn graph_node_kind(kind: &str) -> NodeKind {
    match kind {
        "function" | "method" => NodeKind::Function,
        "struct" => NodeKind::Struct,
        "trait" => NodeKind::Trait,
        "class" => NodeKind::Class,
        "interface" => NodeKind::Interface,
        _ => NodeKind::Symbol,
    }
}

pub(super) fn definition_count(record: &FileRecord) -> usize {
    record
        .symbols
        .iter()
        .filter(|symbol| symbol.is_definition)
        .count()
}
