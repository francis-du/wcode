use super::*;

// Reserve the bounded graph budget across the whole input, not one file at a
// time. Exact targets precede their callers; unrelated definitions only use
// what remains. A large early file cannot starve a later explicit target.
pub(super) fn select_graph_definitions<'a>(
    records: &'a [Arc<FileRecord>],
    max_symbols: usize,
    priority_symbol_ids: &HashSet<String>,
    priority_symbol_names: &HashSet<String>,
) -> Vec<HashSet<&'a str>> {
    let mut selected = vec![HashSet::new(); records.len()];
    let mut remaining = max_symbols;
    for phase in 0..3 {
        if remaining == 0 {
            break;
        }
        if (phase == 0 && priority_symbol_ids.is_empty())
            || (phase == 1 && priority_symbol_names.is_empty())
        {
            continue;
        }
        for (file_index, record) in records.iter().enumerate() {
            for symbol in &record.symbols {
                if remaining == 0 {
                    break;
                }
                let candidate = match phase {
                    0 if symbol.is_definition && priority_symbol_ids.contains(&symbol.id) => {
                        Some(symbol)
                    }
                    1 if !symbol.is_definition
                        && symbol.kind == "call"
                        && priority_symbol_names.contains(&symbol.name)
                        && !has_untyped_receiver(symbol) =>
                    {
                        record
                            .symbols
                            .iter()
                            .filter(|definition| {
                                definition.is_definition
                                    && definition.start_byte <= symbol.start_byte
                                    && definition.end_byte >= symbol.end_byte
                            })
                            .min_by_key(|definition| {
                                definition.end_byte.saturating_sub(definition.start_byte)
                            })
                    }
                    2 if symbol.is_definition => Some(symbol),
                    _ => None,
                };
                if let Some(candidate) = candidate {
                    if selected[file_index].insert(candidate.id.as_str()) {
                        remaining -= 1;
                    }
                }
            }
        }
    }
    selected
}

// This private builder emits unique definition edges and explicitly deduplicated
// call edges. software_graph_from_paths validates the complete graph once;
// per-edge add_edge validation would repeatedly rescan the growing edge list.
pub(super) fn append_file_graph(
    graph: &mut SoftwareGraph,
    record: &FileRecord,
    selected: &HashSet<&str>,
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
    file_attributes.insert(
        "generated_source".to_owned(),
        json!(record.generated_source),
    );
    file_attributes.insert("parse_errors".to_owned(), json!(record.parse_errors));
    graph.add_node(GraphNode {
        id: file_id.clone(),
        kind: NodeKind::File,
        label: record.path.clone(),
        attributes: file_attributes,
        provenance: provenance.clone(),
    })?;

    // Budgeting controls emitted nodes, not the evidence used to resolve calls.
    // Omitted homonyms and nested callers still make selected targets ambiguous.
    let all_definitions = record
        .symbols
        .iter()
        .filter(|symbol| symbol.is_definition)
        .collect::<Vec<_>>();
    let definitions = all_definitions
        .iter()
        .copied()
        .filter(|symbol| selected.contains(symbol.id.as_str()))
        .collect::<Vec<_>>();
    let mut targets_by_name = HashMap::<&str, Vec<&CodeSymbol>>::new();
    for symbol in &all_definitions {
        targets_by_name
            .entry(symbol.name.as_str())
            .or_default()
            .push(symbol);
    }
    for symbol in &definitions {
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
        graph.edges.push(GraphEdge {
            from: file_id.clone(),
            to: node_id,
            kind: EdgeKind::Defines,
            provenance: provenance.clone(),
        });
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
        if has_untyped_receiver(call) {
            continue;
        }
        let Some(caller) = all_definitions
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
        graph.edges.push(GraphEdge {
            from: graph_symbol_id(caller),
            to: graph_symbol_id(target),
            kind: EdgeKind::Calls,
            provenance: provenance.clone(),
        });
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
            // A definition excluded by the output budget still participates in
            // name resolution. Endpoint membership is checked after resolution.
            targets_by_name
                .entry(symbol.name.as_str())
                .or_default()
                .push((record, symbol));
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
            if has_untyped_receiver(call) {
                continue;
            }
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
            let mut provider = "tree-sitter/global-name-resolution";
            let target = if targets.len() == 1 {
                Some(targets[0])
            } else if record.language == LanguageId::Rust {
                let mut qualified = targets.iter().copied().filter(|(target_record, _)| {
                    let path = Path::new(&target_record.path);
                    let module = if path.file_stem().and_then(|value| value.to_str()) == Some("mod")
                    {
                        path.parent()
                            .and_then(Path::file_name)
                            .and_then(|value| value.to_str())
                    } else {
                        path.file_stem().and_then(|value| value.to_str())
                    };
                    module.is_some_and(|module| {
                        call.signature.contains(&format!("{module}::{}", call.name))
                    })
                });
                let first = qualified.next();
                if first.is_some() && qualified.next().is_none() {
                    provider = "tree-sitter/rust-path-resolution";
                    first
                } else {
                    let imports = record
                        .symbols
                        .iter()
                        .filter(|symbol| {
                            !symbol.is_definition
                                && symbol.kind == "import"
                                && symbol.name == call.name
                        })
                        .collect::<Vec<_>>();
                    let mut imported = targets.iter().copied().filter(|(target_record, _)| {
                        let path = Path::new(&target_record.path);
                        let module =
                            if path.file_stem().and_then(|value| value.to_str()) == Some("mod") {
                                path.parent()
                                    .and_then(Path::file_name)
                                    .and_then(|value| value.to_str())
                            } else {
                                path.file_stem().and_then(|value| value.to_str())
                            };
                        module.is_some_and(|module| {
                            imports.iter().any(|import| {
                                import
                                    .signature
                                    .contains(&format!("{module}::{}", call.name))
                            })
                        })
                    });
                    let first = imported.next();
                    if first.is_some() && imported.next().is_none() {
                        provider = "tree-sitter/rust-import-resolution";
                        first
                    } else {
                        None
                    }
                }
            } else {
                None
            };
            let Some((target_record, target)) = target else {
                continue;
            };
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
            graph.edges.push(GraphEdge {
                from,
                to,
                kind: EdgeKind::Calls,
                provenance: GraphProvenance {
                    provider: provider.to_owned(),
                    precision: GraphPrecision::Syntax,
                    revision: format!(
                        "caller:{};target:{}",
                        &record.sha256[..record.sha256.len().min(64)],
                        &target_record.sha256[..target_record.sha256.len().min(64)]
                    ),
                },
            });
        }
    }
    Ok(())
}

fn has_untyped_receiver(call: &CodeSymbol) -> bool {
    // Tree-sitter gives us the callee name but not the receiver's resolved type.
    // Resolving `cache.flush()` to the only `Worker::flush` definition would turn
    // name uniqueness into a false semantic fact. Keep receiver-qualified calls
    // unresolved until an LSP/compiler provider supplies the missing type edge.
    let dotted = format!(".{}", call.name);
    let arrow = format!("->{}", call.name);
    call.signature.contains(&dotted) || call.signature.contains(&arrow)
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
