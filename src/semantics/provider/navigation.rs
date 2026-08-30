use super::*;

const MAX_NAVIGATION_RESULTS: usize = 100;
const MAX_HOVER_CHARS: usize = 4_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticNavigationIntent {
    Inspect,
    Definition,
    Hover,
    References,
    IncomingCalls,
    OutgoingCalls,
    Calls,
    Implementations,
    Impact,
}

#[derive(Clone, Debug, Serialize)]
pub struct SemanticLocation {
    pub path: String,
    pub line: u64,
    pub character: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SemanticNavigationResult {
    pub provider: String,
    pub precision: &'static str,
    pub routing: &'static str,
    pub path: String,
    pub line: u64,
    pub character: u64,
    pub position_encoding: String,
    pub session_reused: bool,
    pub document_sync: DocumentSyncState,
    pub queried: Vec<&'static str>,
    pub definitions: Vec<SemanticLocation>,
    pub references: Vec<SemanticLocation>,
    pub implementations: Vec<SemanticLocation>,
    pub incoming_calls: Vec<SemanticLocation>,
    pub outgoing_calls: Vec<SemanticLocation>,
    pub hover: Option<String>,
    pub unsupported: Vec<&'static str>,
    pub truncated: bool,
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
        bail!("semantic navigation line and character are 1-based");
    }
    if !workspace.exec_enabled() {
        bail!("semantic navigation requires command execution; restart without --no-exec");
    }
    if !workspace.semantic_exec_enabled() {
        bail!("semantic navigation is disabled; restart without --no-semantic");
    }
    let source = workspace.load_source(path)?;
    let language = language_for_path(&source.path)
        .ok_or_else(|| anyhow!("semantic navigation does not support this source language"))?;
    let (provider, executable) = select_provider(workspace, language).ok_or_else(|| {
        anyhow!(
            "no trusted semantic provider is available for {}",
            language.as_str()
        )
    })?;
    authorize_provider_session(workspace, provider)?;
    let handle = sessions.handle(workspace, provider, &executable)?;
    let session_reused = handle.ensure_started(workspace, provider).await?;
    let mut guard = handle.lock().await;
    let session = guard
        .as_mut()
        .ok_or_else(|| anyhow!("semantic session failed to initialize"))?;
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
        truncated: false,
    };

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
        let (locations, supported, truncated) = query_locations(
            workspace,
            session,
            "definitionProvider",
            "textDocument/definition",
            json!({"textDocument":{"uri":uri},"position":position}),
            max_results,
        )
        .await;
        result.definitions = locations;
        result.truncated |= truncated;
        if !supported {
            result.unsupported.push("definition");
        }
    }
    if want_hover {
        result.queried.push("hover");
        if capability_enabled(&session.capabilities, "hoverProvider") {
            if let Ok(value) = session
                .request(
                    "textDocument/hover",
                    json!({"textDocument":{"uri":uri},"position":position}),
                )
                .await
            {
                result.hover = hover_text(&value);
            }
        } else {
            result.unsupported.push("hover");
        }
    }
    if want_references {
        result.queried.push("references");
        let (locations, supported, truncated) = query_locations(
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
        )
        .await;
        result.references = locations;
        result.truncated |= truncated;
        if !supported {
            result.unsupported.push("references");
        }
    }
    if want_implementations {
        result.queried.push("implementations");
        let (locations, supported, truncated) = query_locations(
            workspace,
            session,
            "implementationProvider",
            "textDocument/implementation",
            json!({"textDocument":{"uri":uri},"position":position}),
            max_results,
        )
        .await;
        result.implementations = locations;
        result.truncated |= truncated;
        if !supported {
            result.unsupported.push("implementations");
        }
    }
    if want_calls {
        result.queried.push("calls");
        if capability_enabled(&session.capabilities, "callHierarchyProvider") {
            if let Ok(prepared) = session
                .request(
                    "textDocument/prepareCallHierarchy",
                    json!({"textDocument":{"uri":uri},"position":position}),
                )
                .await
            {
                if let Some(item) = prepared.as_array().and_then(|items| items.first()).cloned() {
                    if want_incoming {
                        if let Ok(incoming) = session
                            .request("callHierarchy/incomingCalls", json!({"item":item}))
                            .await
                        {
                            append_call_locations(
                                workspace,
                                &incoming,
                                "from",
                                &session.position_encoding,
                                max_results,
                                &mut result.incoming_calls,
                                &mut result.truncated,
                            );
                        }
                    }
                    if want_outgoing {
                        if let Ok(outgoing) = session
                            .request("callHierarchy/outgoingCalls", json!({"item":item}))
                            .await
                        {
                            append_call_locations(
                                workspace,
                                &outgoing,
                                "to",
                                &session.position_encoding,
                                max_results,
                                &mut result.outgoing_calls,
                                &mut result.truncated,
                            );
                        }
                    }
                }
            }
        } else {
            result.unsupported.push("calls");
        }
    }
    Ok(result)
}

async fn query_locations(
    workspace: &Workspace,
    session: &mut SemanticSession,
    capability: &str,
    method: &str,
    params: Value,
    max_results: usize,
) -> (Vec<SemanticLocation>, bool, bool) {
    if !capability_enabled(&session.capabilities, capability) {
        return (Vec::new(), false, false);
    }
    let Ok(value) = session.request(method, params).await else {
        return (Vec::new(), true, false);
    };
    let mut output = Vec::new();
    let mut truncated = false;
    append_locations(
        workspace,
        &value,
        &session.position_encoding,
        max_results,
        &mut output,
        &mut truncated,
    );
    (output, true, truncated)
}

fn capability_enabled(capabilities: &Value, key: &str) -> bool {
    capabilities
        .get(key)
        .is_some_and(|value| value.as_bool().unwrap_or(!value.is_null()))
}

fn append_locations(
    workspace: &Workspace,
    value: &Value,
    encoding: &str,
    max_results: usize,
    output: &mut Vec<SemanticLocation>,
    truncated: &mut bool,
) {
    let items = if let Some(items) = value.as_array() {
        items.iter().collect::<Vec<_>>()
    } else if value.is_object() {
        vec![value]
    } else {
        Vec::new()
    };
    for item in items {
        if output.len() >= max_results {
            *truncated = true;
            break;
        }
        if let Some(location) = location_from_lsp(workspace, item, encoding, None) {
            if !output
                .iter()
                .any(|existing| same_location(existing, &location))
            {
                output.push(location);
            }
        }
    }
}

fn append_call_locations(
    workspace: &Workspace,
    value: &Value,
    key: &str,
    encoding: &str,
    max_results: usize,
    output: &mut Vec<SemanticLocation>,
    truncated: &mut bool,
) {
    for call in value.as_array().into_iter().flatten() {
        if output.len() >= max_results {
            *truncated = true;
            break;
        }
        let Some(item) = call.get(key) else {
            continue;
        };
        let name = item.get("name").and_then(Value::as_str).map(str::to_owned);
        if let Some(location) = location_from_lsp(workspace, item, encoding, name) {
            if !output
                .iter()
                .any(|existing| same_location(existing, &location))
            {
                output.push(location);
            }
        }
    }
}

fn location_from_lsp(
    workspace: &Workspace,
    item: &Value,
    encoding: &str,
    name: Option<String>,
) -> Option<SemanticLocation> {
    let uri = item
        .get("uri")
        .or_else(|| item.get("targetUri"))?
        .as_str()?;
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
    let range = item
        .get("selectionRange")
        .or_else(|| item.get("range"))
        .or_else(|| item.get("targetSelectionRange"))
        .or_else(|| item.get("targetRange"))?;
    let zero_line = range.pointer("/start/line")?.as_u64()?;
    let lsp_character = range.pointer("/start/character")?.as_u64()?;
    let source = workspace.load_source(&path).ok()?;
    let character =
        lsp_to_byte_column(&source.content, zero_line + 1, lsp_character, encoding).ok()?;
    Some(SemanticLocation {
        path,
        line: zero_line + 1,
        character,
        name,
    })
}

fn same_location(left: &SemanticLocation, right: &SemanticLocation) -> bool {
    left.path == right.path && left.line == right.line && left.character == right.character
}

fn byte_column_to_lsp(content: &str, line: u64, column: u64, encoding: &str) -> Result<u64> {
    let text = content
        .split('\n')
        .nth(usize::try_from(line - 1).unwrap_or(usize::MAX))
        .ok_or_else(|| anyhow!("semantic navigation line is outside the source file"))?;
    let byte_offset = usize::try_from(column - 1).map_err(|_| anyhow!("column is too large"))?;
    if byte_offset > text.len() || !text.is_char_boundary(byte_offset) {
        bail!("semantic navigation character must be a 1-based UTF-8 byte column on a character boundary");
    }
    let prefix = &text[..byte_offset];
    Ok(match encoding {
        "utf-8" => prefix.len() as u64,
        "utf-32" => prefix.chars().count() as u64,
        _ => prefix.encode_utf16().count() as u64,
    })
}

fn lsp_to_byte_column(content: &str, line: u64, character: u64, encoding: &str) -> Result<u64> {
    let text = content
        .split('\n')
        .nth(usize::try_from(line - 1).unwrap_or(usize::MAX))
        .ok_or_else(|| anyhow!("LSP location line is outside the source file"))?;
    let target = usize::try_from(character).map_err(|_| anyhow!("LSP character is too large"))?;
    let byte_offset = match encoding {
        "utf-8" => target.min(text.len()),
        "utf-32" => text
            .char_indices()
            .nth(target)
            .map(|(index, _)| index)
            .unwrap_or(text.len()),
        _ => {
            let mut units = 0usize;
            let mut offset = text.len();
            for (index, ch) in text.char_indices() {
                if units >= target {
                    offset = index;
                    break;
                }
                units = units.saturating_add(ch.len_utf16());
            }
            offset
        }
    };
    Ok(byte_offset as u64 + 1)
}

fn hover_text(value: &Value) -> Option<String> {
    let contents = value.get("contents")?;
    let mut text = String::new();
    append_hover_value(contents, &mut text);
    let compact = text.trim();
    (!compact.is_empty()).then(|| compact.chars().take(MAX_HOVER_CHARS).collect())
}

fn append_hover_value(value: &Value, output: &mut String) {
    match value {
        Value::String(text) => {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(text);
        }
        Value::Array(items) => {
            for item in items {
                append_hover_value(item, output);
            }
        }
        Value::Object(object) => {
            if let Some(text) = object.get("value").and_then(Value::as_str) {
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(text);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/semantics/navigation.rs"]
mod tests;
