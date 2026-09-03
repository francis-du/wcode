use super::*;

pub(super) async fn build_provider_import(
    sessions: &SemanticSessionPool,
    workspace: &Workspace,
    provider: ProviderCandidate,
    executable: &Path,
    files: &[PreparedSemanticSource],
    max_symbols: usize,
    revision: String,
) -> Result<(GraphProviderImport, SemanticProviderRun, bool)> {
    let handle = sessions.handle(workspace, provider, executable)?;
    handle.ensure_started(workspace, provider).await?;
    let mut guard = handle.lock().await;
    let session = guard
        .as_mut()
        .ok_or_else(|| anyhow!("LSP session failed to initialize"))?;
    let call_hierarchy = session
        .capabilities
        .get("callHierarchyProvider")
        .is_some_and(|value| value.as_bool().unwrap_or(!value.is_null()));
    let implementation_resolution = session
        .capabilities
        .get("implementationProvider")
        .is_some_and(|value| value.as_bool().unwrap_or(!value.is_null()));
    let open_paths = files
        .iter()
        .map(|source| source.source.path.clone())
        .collect::<BTreeSet<_>>();
    session.retain_documents(workspace, &open_paths).await?;

    let mut nodes = BTreeMap::<String, GraphImportNode>::new();
    let mut edges = Vec::<GraphImportEdge>::new();
    let mut symbol_positions = Vec::new();
    let mut truncated = false;

    for prepared in files {
        if nodes.len() >= max_symbols {
            truncated = true;
            break;
        }
        let source = &prepared.source;
        let language = prepared.language;
        let (uri, _) = session.sync_document(workspace, source, language).await?;
        let result = match session
            .request(
                "textDocument/documentSymbol",
                json!({"textDocument":{"uri":uri}}),
            )
            .await
        {
            Ok(value) => value,
            Err(_) => continue,
        };
        let mut symbols = Vec::new();
        flatten_document_symbols(&result, &source.path, None, &mut symbols);
        for symbol in symbols {
            if nodes.len() >= max_symbols {
                truncated = true;
                break;
            }
            let id = semantic_node_id(
                provider.id,
                &symbol.path,
                symbol.line,
                symbol.character,
                &symbol.name,
            );
            let mut attributes = BTreeMap::new();
            attributes.insert("path".into(), json!(symbol.path));
            attributes.insert("source_sha256".into(), json!(source.sha256.as_str()));
            attributes.insert("name".into(), json!(symbol.name));
            attributes.insert("qualified_name".into(), json!(symbol.qualified_name));
            attributes.insert("lsp_kind".into(), json!(symbol.kind));
            attributes.insert("line".into(), json!(symbol.line + 1));
            attributes.insert("character".into(), json!(symbol.character + 1));
            nodes.entry(id.clone()).or_insert(GraphImportNode {
                id: id.clone(),
                kind: lsp_node_kind(symbol.kind),
                label: symbol.qualified_name.clone(),
                attributes,
            });
            symbol_positions.push((id, uri.clone(), symbol.line, symbol.character, symbol.kind));
        }
    }

    if call_hierarchy {
        for (from_id, uri, line, character, _) in symbol_positions
            .iter()
            .filter(|(_, _, _, _, kind)| call_hierarchy_candidate(*kind))
            .take(MAX_PROVIDER_RELATION_SYMBOLS.min(max_symbols))
        {
            if edges.len() >= MAX_PROVIDER_EDGES {
                truncated = true;
                break;
            }
            let prepared = match session
                .request(
                    "textDocument/prepareCallHierarchy",
                    json!({
                        "textDocument":{"uri":uri},
                        "position":{"line":line,"character":character}
                    }),
                )
                .await
            {
                Ok(value) => value,
                Err(_) => continue,
            };
            let Some(item) = prepared.as_array().and_then(|items| items.first()).cloned() else {
                continue;
            };
            let outgoing = match session
                .request("callHierarchy/outgoingCalls", json!({"item":item}))
                .await
            {
                Ok(value) => value,
                Err(_) => continue,
            };
            for call in outgoing.as_array().into_iter().flatten() {
                if edges.len() >= MAX_PROVIDER_EDGES {
                    truncated = true;
                    break;
                }
                let Some(target) = call.get("to") else {
                    continue;
                };
                let Some(target_node) = call_hierarchy_node(workspace, provider.id, target) else {
                    continue;
                };
                let to_id = target_node.id.clone();
                nodes.entry(to_id.clone()).or_insert(target_node);
                if from_id != &to_id
                    && !edges.iter().any(|edge| {
                        edge.from == *from_id && edge.to == to_id && edge.kind == EdgeKind::Calls
                    })
                {
                    edges.push(GraphImportEdge {
                        from: from_id.clone(),
                        to: to_id,
                        kind: EdgeKind::Calls,
                    });
                }
            }
        }
    }

    if implementation_resolution {
        for (interface_id, uri, line, character, _) in symbol_positions
            .iter()
            .filter(|(_, _, _, _, kind)| implementation_candidate(*kind))
            .take(MAX_PROVIDER_RELATION_SYMBOLS.min(max_symbols))
        {
            if edges.len() >= MAX_PROVIDER_EDGES {
                truncated = true;
                break;
            }
            let Some(interface) = nodes.get(interface_id).cloned() else {
                continue;
            };
            let implementations = match session
                .request(
                    "textDocument/implementation",
                    json!({
                        "textDocument":{"uri":uri},
                        "position":{"line":line,"character":character}
                    }),
                )
                .await
            {
                Ok(value) => value,
                Err(_) => continue,
            };
            for (path, target_line, target_character) in
                implementation_locations(workspace, &implementations)
            {
                if edges.len() >= MAX_PROVIDER_EDGES {
                    truncated = true;
                    break;
                }
                let existing_target = nodes
                    .values()
                    .find(|node| node_at_location(node, &path, target_line, target_character))
                    .map(|node| node.id.clone());
                let target_id = existing_target.unwrap_or_else(|| {
                    semantic_node_id(
                        provider.id,
                        &path,
                        target_line,
                        target_character,
                        &interface.label,
                    )
                });
                if !nodes.contains_key(&target_id) {
                    let source_sha256 = workspace
                        .load_source(&path)
                        .ok()
                        .map(|source| source.sha256);
                    let mut attributes = BTreeMap::new();
                    attributes.insert("path".into(), json!(path));
                    if let Some(source_sha256) = source_sha256 {
                        attributes.insert("source_sha256".into(), json!(source_sha256));
                    }
                    attributes.insert("name".into(), json!(interface.label));
                    attributes.insert("qualified_name".into(), json!(interface.label));
                    attributes.insert("line".into(), json!(target_line + 1));
                    attributes.insert("character".into(), json!(target_character + 1));
                    nodes.insert(
                        target_id.clone(),
                        GraphImportNode {
                            id: target_id.clone(),
                            kind: interface.kind,
                            label: format!("implementation of {}", interface.label),
                            attributes,
                        },
                    );
                }
                if target_id != *interface_id
                    && !edges.iter().any(|edge| {
                        edge.from == target_id
                            && edge.to == *interface_id
                            && edge.kind == EdgeKind::Implements
                    })
                {
                    edges.push(GraphImportEdge {
                        from: target_id,
                        to: interface_id.clone(),
                        kind: EdgeKind::Implements,
                    });
                }
            }
        }
    }

    if nodes.is_empty() {
        bail!("LSP server returned no semantic document symbols");
    }
    let import = GraphProviderImport {
        provider: format!("lsp:{}", provider.id),
        precision: GraphPrecision::Semantic,
        revision: revision.clone(),
        nodes: nodes.into_values().collect(),
        edges,
    };
    import.validate()?;
    let languages = files
        .iter()
        .map(|source| source.language)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let run = SemanticProviderRun {
        provider: import.provider.clone(),
        languages,
        files: files.len(),
        nodes: import.nodes.len(),
        edges: import.edges.len(),
        call_hierarchy,
        implementation_resolution,
        cached: false,
        revision,
    };
    Ok((import, run, truncated))
}
