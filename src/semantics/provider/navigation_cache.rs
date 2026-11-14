use super::*;

#[derive(Default)]
pub(super) struct NavigationLocationCache {
    pub(super) sources: std::collections::HashMap<String, SourceDocument>,
    pub(super) uri_paths: std::collections::HashMap<String, String>,
}

impl NavigationLocationCache {
    pub(super) fn path_for_uri(&mut self, workspace: &Workspace, uri: &str) -> Option<String> {
        if let Some(path) = self.uri_paths.get(uri) {
            return Some(path.clone());
        }
        let url = Url::parse(uri).ok()?;
        let canonical = url.to_file_path().ok()?.canonicalize().ok()?;
        if !canonical.starts_with(workspace.root()) {
            return None;
        }
        let path = canonical
            .strip_prefix(workspace.root())
            .ok()?
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        self.uri_paths.insert(uri.to_owned(), path.clone());
        Some(path)
    }

    pub(super) fn source<'a>(
        &'a mut self,
        workspace: &Workspace,
        path: &str,
    ) -> Option<&'a SourceDocument> {
        if !self.sources.contains_key(path) {
            self.sources
                .insert(path.to_owned(), workspace.load_source(path).ok()?);
        }
        self.sources.get(path)
    }
}

pub(crate) async fn navigate(
    sessions: &SemanticSessionPool,
    workspace: &Workspace,
    path: &str,
    line: u64,
    character: u64,
    intent: SemanticNavigationIntent,
    max_results: usize,
) -> Result<SemanticNavigationResult> {
    if line == 0 || character == 0 {
        bail!("LSP navigation line and character are 1-based");
    }
    if !workspace.exec_enabled() {
        bail!("LSP navigation requires command execution; restart without --no-exec");
    }
    if !workspace.semantic_exec_enabled() {
        bail!("LSP navigation is disabled; restart without --no-semantic");
    }
    let source = workspace.load_source(path)?;
    let language = language_for_path(&source.path)
        .ok_or_else(|| anyhow!("LSP navigation does not support this source language"))?;
    let candidates = provider_candidates(workspace, language);
    if candidates.is_empty() {
        bail!(
            "no trusted LSP server is available for {}",
            language.as_str()
        );
    }
    let mut startup_failures = Vec::new();
    let mut selected = None;
    for (provider, executable) in candidates {
        if let Err(error) = authorize_provider_session(workspace, provider, &executable) {
            if startup_failures.is_empty() {
                return Err(error);
            }
            startup_failures.push(format!("{} authorization: {}", provider.id, render(&error)));
            break;
        }
        let handle = match sessions.handle(workspace, provider, &executable) {
            Ok(handle) => handle,
            Err(error) => {
                startup_failures.push(format!("{} session: {}", provider.id, render(&error)));
                continue;
            }
        };
        match handle.ensure_started(workspace, provider).await {
            Ok(reused) => {
                selected = Some((provider, executable, handle, reused));
                break;
            }
            Err(error) => {
                sessions.invalidate(workspace, provider, &executable);
                startup_failures.push(format!("{} initialize: {}", provider.id, render(&error)));
            }
        }
    }
    let (provider, _executable, handle, session_reused) = selected.ok_or_else(|| {
        anyhow!(
            "no installed LSP server could initialize for {}: {}",
            language.as_str(),
            startup_failures.join("; ")
        )
    })?;
    let mut guard = handle.lock().await;
    let session = guard
        .as_mut()
        .ok_or_else(|| anyhow!("LSP session failed to initialize"))?;
    let (uri, document_sync) = session.sync_document(workspace, &source, language).await?;
    let position = json!({
        "line": line - 1,
        "character": byte_column_to_lsp(&source.content, line, character, &session.position_encoding)?
    });
    let max_results = max_results.clamp(1, MAX_NAVIGATION_RESULTS);
    let mut result = SemanticNavigationResult {
        provider: format!("lsp:{}", provider.id),
        precision: "semantic",
        routing: "cross_file_semantic",
        path: source.path.clone(),
        line,
        character,
        position_encoding: session.position_encoding.clone(),
        session_reused,
        document_sync,
        queried: Vec::new(),
        definitions: Vec::new(),
        references: Vec::new(),
        implementations: Vec::new(),
        incoming_calls: Vec::new(),
        outgoing_calls: Vec::new(),
        hover: None,
        unsupported: Vec::new(),
        failures: Vec::new(),
        truncated: false,
    };
    let mut location_cache = NavigationLocationCache::default();
    location_cache.sources.insert(source.path.clone(), source);

    let want_definition = matches!(
        intent,
        SemanticNavigationIntent::Inspect | SemanticNavigationIntent::Definition
    );
    let want_hover = matches!(
        intent,
        SemanticNavigationIntent::Inspect | SemanticNavigationIntent::Hover
    );
    let want_references = matches!(
        intent,
        SemanticNavigationIntent::References | SemanticNavigationIntent::Impact
    );
    let want_incoming = matches!(
        intent,
        SemanticNavigationIntent::IncomingCalls
            | SemanticNavigationIntent::Calls
            | SemanticNavigationIntent::Impact
    );
    let want_outgoing = matches!(
        intent,
        SemanticNavigationIntent::OutgoingCalls | SemanticNavigationIntent::Calls
    );
    let want_calls = want_incoming || want_outgoing;
    let want_implementations = matches!(
        intent,
        SemanticNavigationIntent::Implementations | SemanticNavigationIntent::Impact
    );

    if want_definition {
        result.queried.push("definition");
        let (locations, status, truncated) = query_locations(
            workspace,
            session,
            "definitionProvider",
            "textDocument/definition",
            json!({"textDocument":{"uri":uri},"position":position}),
            max_results,
            &mut location_cache,
        )
        .await;
        result.definitions = locations;
        result.truncated |= truncated;
        record_query_status(
            &mut result.unsupported,
            &mut result.failures,
            "definition",
            status,
        );
    }
    if want_hover {
        result.queried.push("hover");
        if capability_enabled(&session.capabilities, "hoverProvider") {
            match session
                .request(
                    "textDocument/hover",
                    json!({"textDocument":{"uri":uri},"position":position}),
                )
                .await
            {
                Ok(value) => result.hover = hover_text(&value),
                Err(_) => result.failures.push("hover"),
            }
        } else {
            result.unsupported.push("hover");
        }
    }
    if want_references {
        result.queried.push("references");
        let (locations, status, truncated) = query_locations(
            workspace,
            session,
            "referencesProvider",
            "textDocument/references",
            json!({
                "textDocument":{"uri":uri},
                "position":position,
                "context":{"includeDeclaration":false}
            }),
            max_results,
            &mut location_cache,
        )
        .await;
        result.references = locations;
        result.truncated |= truncated;
        record_query_status(
            &mut result.unsupported,
            &mut result.failures,
            "references",
            status,
        );
    }
    if want_implementations {
        result.queried.push("implementations");
        let (locations, status, truncated) = query_locations(
            workspace,
            session,
            "implementationProvider",
            "textDocument/implementation",
            json!({"textDocument":{"uri":uri},"position":position}),
            max_results,
            &mut location_cache,
        )
        .await;
        result.implementations = locations;
        result.truncated |= truncated;
        record_query_status(
            &mut result.unsupported,
            &mut result.failures,
            "implementations",
            status,
        );
    }
    if want_calls {
        result.queried.push("calls");
        if capability_enabled(&session.capabilities, "callHierarchyProvider") {
            match session
                .request(
                    "textDocument/prepareCallHierarchy",
                    json!({"textDocument":{"uri":uri},"position":position}),
                )
                .await
            {
                Ok(prepared) => {
                    if let Some(item) = prepared_call_hierarchy_item_for_request(
                        workspace,
                        &prepared,
                        &uri,
                        &position,
                        &session.position_encoding,
                        &mut location_cache,
                        &mut result.failures,
                    ) {
                        if want_incoming {
                            match session
                                .request("callHierarchy/incomingCalls", json!({"item":item}))
                                .await
                            {
                                Ok(incoming) => {
                                    let mut context = CallLocationContext {
                                        workspace,
                                        encoding: &session.position_encoding,
                                        max_results,
                                        cache: &mut location_cache,
                                        truncated: &mut result.truncated,
                                    };
                                    append_call_locations(
                                        &mut context,
                                        &incoming,
                                        "from",
                                        None,
                                        &mut result.incoming_calls,
                                    );
                                }
                                Err(_) => result.failures.push("incoming_calls"),
                            }
                        }
                        if want_outgoing {
                            match session
                                .request("callHierarchy/outgoingCalls", json!({"item":item}))
                                .await
                            {
                                Ok(outgoing) => {
                                    let mut context = CallLocationContext {
                                        workspace,
                                        encoding: &session.position_encoding,
                                        max_results,
                                        cache: &mut location_cache,
                                        truncated: &mut result.truncated,
                                    };
                                    append_call_locations(
                                        &mut context,
                                        &outgoing,
                                        "to",
                                        Some(&item),
                                        &mut result.outgoing_calls,
                                    );
                                }
                                Err(_) => result.failures.push("outgoing_calls"),
                            }
                        }
                    }
                }
                Err(_) => result.failures.push("calls"),
            }
        } else {
            result.unsupported.push("calls");
        }
    }
    Ok(result)
}
