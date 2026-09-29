use super::client::render_error_chain as render;
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
    RenamePlan,
    OrganizeImportsPlan,
    QuickFixPlan,
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
    pub failures: Vec<&'static str>,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NavigationQueryStatus {
    Ok,
    Unsupported,
    Failed,
}

#[derive(Default)]
struct NavigationLocationCache {
    sources: std::collections::HashMap<String, SourceDocument>,
    uri_paths: std::collections::HashMap<String, String>,
}

impl NavigationLocationCache {
    fn path_for_uri(&mut self, workspace: &Workspace, uri: &str) -> Option<String> {
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

    fn source<'a>(&'a mut self, workspace: &Workspace, path: &str) -> Option<&'a SourceDocument> {
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

fn prepared_call_hierarchy_item(
    prepared: &Value,
    failures: &mut Vec<&'static str>,
) -> Option<Value> {
    let item = prepared
        .as_array()
        .filter(|items| items.len() == 1)
        .and_then(|items| items.first())
        .filter(|item| valid_call_hierarchy_item(item))
        .cloned();
    if item.is_none() {
        failures.push("calls");
    }
    item
}

fn valid_call_hierarchy_item(item: &Value) -> bool {
    item.get("name").and_then(Value::as_str).is_some()
        && item.get("kind").and_then(Value::as_u64).is_some()
        && item.get("uri").and_then(Value::as_str).is_some()
        && valid_lsp_range(item.get("range"))
        && valid_lsp_range(item.get("selectionRange"))
        && lsp_range_contains(item.get("range"), item.get("selectionRange"))
}

fn prepared_call_hierarchy_item_for_request(
    workspace: &Workspace,
    prepared: &Value,
    expected_uri: &str,
    expected_position: &Value,
    encoding: &str,
    cache: &mut NavigationLocationCache,
    failures: &mut Vec<&'static str>,
) -> Option<Value> {
    let item = prepared_call_hierarchy_item(prepared, failures)?;
    if !prepared_call_hierarchy_item_matches_request(
        workspace,
        &item,
        expected_uri,
        expected_position,
        encoding,
        cache,
    ) {
        failures.push("calls");
        return None;
    }
    Some(item)
}

fn prepared_call_hierarchy_item_matches_request(
    workspace: &Workspace,
    item: &Value,
    expected_uri: &str,
    expected_position: &Value,
    encoding: &str,
    cache: &mut NavigationLocationCache,
) -> bool {
    let Some(item_uri) = item.get("uri").and_then(Value::as_str) else {
        return false;
    };
    if item_uri != expected_uri {
        return false;
    }
    let Some(position) = lsp_position(Some(expected_position)) else {
        return false;
    };
    if !lsp_range_contains_position(item.get("selectionRange"), position) {
        return false;
    }
    let Some(path) = cache.path_for_uri(workspace, item_uri) else {
        return false;
    };
    let Some(source) = cache.source(workspace, &path) else {
        return false;
    };
    ["range", "selectionRange"]
        .iter()
        .all(|key| lsp_range_maps_to_source(item.get(*key), &source.content, encoding))
}

fn valid_lsp_range(range: Option<&Value>) -> bool {
    let Some(range) = range else {
        return false;
    };
    let Some(start) = lsp_position(range.get("start")) else {
        return false;
    };
    let Some(end) = lsp_position(range.get("end")) else {
        return false;
    };
    start <= end
}

fn lsp_position(position: Option<&Value>) -> Option<(u64, u64)> {
    let position = position?;
    Some((
        position.get("line")?.as_u64()?,
        position.get("character")?.as_u64()?,
    ))
}

fn lsp_range_contains(outer: Option<&Value>, inner: Option<&Value>) -> bool {
    let Some(outer_start) = outer.and_then(|range| lsp_position(range.get("start"))) else {
        return false;
    };
    let Some(outer_end) = outer.and_then(|range| lsp_position(range.get("end"))) else {
        return false;
    };
    let Some(inner_start) = inner.and_then(|range| lsp_position(range.get("start"))) else {
        return false;
    };
    let Some(inner_end) = inner.and_then(|range| lsp_position(range.get("end"))) else {
        return false;
    };
    outer_start <= inner_start && inner_end <= outer_end
}

fn lsp_range_contains_position(range: Option<&Value>, position: (u64, u64)) -> bool {
    let Some(start) = range.and_then(|range| lsp_position(range.get("start"))) else {
        return false;
    };
    let Some(end) = range.and_then(|range| lsp_position(range.get("end"))) else {
        return false;
    };
    start <= position && position < end
}

fn lsp_range_maps_to_source(range: Option<&Value>, content: &str, encoding: &str) -> bool {
    let Some(range) = range else {
        return false;
    };
    let Some(start) = lsp_position(range.get("start")) else {
        return false;
    };
    let Some(end) = lsp_position(range.get("end")) else {
        return false;
    };
    [start, end].into_iter().all(|(line, character)| {
        line.checked_add(1)
            .is_some_and(|line| lsp_to_byte_column(content, line, character, encoding).is_ok())
    })
}

async fn query_locations(
    workspace: &Workspace,
    session: &mut SemanticSession,
    capability: &str,
    method: &str,
    params: Value,
    max_results: usize,
    cache: &mut NavigationLocationCache,
) -> (Vec<SemanticLocation>, NavigationQueryStatus, bool) {
    if !capability_enabled(&session.capabilities, capability) {
        return (Vec::new(), NavigationQueryStatus::Unsupported, false);
    }
    let Ok(value) = session.request(method, params).await else {
        return (Vec::new(), NavigationQueryStatus::Failed, false);
    };
    let value = if value.is_object()
        && matches!(
            method,
            "textDocument/definition" | "textDocument/implementation"
        )
        && value.get("uri").and_then(Value::as_str).is_some()
        && value.get("targetUri").is_none()
    {
        Value::Array(vec![value])
    } else {
        value
    };
    let mut output = Vec::new();
    let mut truncated = false;
    append_locations(
        workspace,
        &value,
        &session.position_encoding,
        max_results,
        cache,
        &mut output,
        &mut truncated,
    );
    (output, NavigationQueryStatus::Ok, truncated)
}

fn record_query_status(
    unsupported: &mut Vec<&'static str>,
    failures: &mut Vec<&'static str>,
    label: &'static str,
    status: NavigationQueryStatus,
) {
    match status {
        NavigationQueryStatus::Ok => {}
        NavigationQueryStatus::Unsupported => unsupported.push(label),
        NavigationQueryStatus::Failed => failures.push(label),
    }
}

pub(super) fn capability_enabled(capabilities: &Value, key: &str) -> bool {
    capabilities
        .get(key)
        .is_some_and(|value| value.as_bool().unwrap_or(!value.is_null()))
}

fn append_locations(
    workspace: &Workspace,
    value: &Value,
    encoding: &str,
    max_results: usize,
    cache: &mut NavigationLocationCache,
    output: &mut Vec<SemanticLocation>,
    truncated: &mut bool,
) {
    let items = if let Some(items) = value.as_array() {
        items.iter().collect::<Vec<_>>()
    } else if value.is_null() {
        Vec::new()
    } else {
        *truncated = true;
        Vec::new()
    };
    for item in items {
        if output.len() >= max_results {
            *truncated = true;
            break;
        }
        if let Some(location) = location_from_lsp(workspace, item, encoding, None, cache) {
            if !output
                .iter()
                .any(|existing| same_location(existing, &location))
            {
                output.push(location);
            }
        } else {
            *truncated = true;
        }
    }
}

struct CallLocationContext<'a> {
    workspace: &'a Workspace,
    encoding: &'a str,
    max_results: usize,
    cache: &'a mut NavigationLocationCache,
    truncated: &'a mut bool,
}

fn append_call_locations(
    context: &mut CallLocationContext<'_>,
    value: &Value,
    key: &str,
    origin_item: Option<&Value>,
    output: &mut Vec<SemanticLocation>,
) {
    if !value.is_null() && !value.is_array() {
        *context.truncated = true;
        return;
    }
    for call in value.as_array().into_iter().flatten() {
        if output.len() >= context.max_results {
            *context.truncated = true;
            break;
        }
        let Some(item) = call.get(key).filter(|item| valid_call_hierarchy_item(item)) else {
            *context.truncated = true;
            continue;
        };
        if !call_hierarchy_ranges_map_to_caller(context, call, key, item, origin_item) {
            *context.truncated = true;
            continue;
        }
        let name = item.get("name").and_then(Value::as_str).map(str::to_owned);
        if let Some(location) = location_from_lsp(
            context.workspace,
            item,
            context.encoding,
            name,
            context.cache,
        ) {
            if !output
                .iter()
                .any(|existing| same_location(existing, &location))
            {
                output.push(location);
            }
        } else {
            *context.truncated = true;
        }
    }
}

fn call_hierarchy_ranges_map_to_caller(
    context: &mut CallLocationContext<'_>,
    call: &Value,
    key: &str,
    item: &Value,
    origin_item: Option<&Value>,
) -> bool {
    let Some(ranges) = call.get("fromRanges").and_then(Value::as_array) else {
        return false;
    };
    if ranges.is_empty() {
        return false;
    }
    let caller = match key {
        "from" => item,
        "to" => match origin_item {
            Some(origin_item) => origin_item,
            None => return false,
        },
        _ => return false,
    };
    let Some(uri) = caller.get("uri").and_then(Value::as_str) else {
        return false;
    };
    let Some(path) = context.cache.path_for_uri(context.workspace, uri) else {
        return false;
    };
    let Some(source) = context.cache.source(context.workspace, &path) else {
        return false;
    };
    ranges.iter().all(|range| {
        valid_lsp_range(Some(range))
            && lsp_range_contains(caller.get("range"), Some(range))
            && lsp_range_maps_to_source(Some(range), &source.content, context.encoding)
    })
}

fn location_from_lsp(
    workspace: &Workspace,
    item: &Value,
    encoding: &str,
    name: Option<String>,
    cache: &mut NavigationLocationCache,
) -> Option<SemanticLocation> {
    let (uri, range) = match (item.get("uri"), item.get("targetUri")) {
        (Some(uri), None) => {
            if item.get("targetRange").is_some() || item.get("targetSelectionRange").is_some() {
                return None;
            }
            if name.is_none() && item.get("selectionRange").is_some() {
                return None;
            }
            if let (Some(outer), Some(selection)) = (item.get("range"), item.get("selectionRange"))
            {
                if !lsp_range_contains(Some(outer), Some(selection)) {
                    return None;
                }
            }
            (
                uri.as_str()?,
                item.get("selectionRange").or_else(|| item.get("range"))?,
            )
        }
        (None, Some(uri)) => {
            if item.get("range").is_some() || item.get("selectionRange").is_some() {
                return None;
            }
            let target_range = item.get("targetRange")?;
            let target_selection = item.get("targetSelectionRange")?;
            if !lsp_range_contains(Some(target_range), Some(target_selection)) {
                return None;
            }
            (uri.as_str()?, target_selection)
        }
        _ => return None,
    };
    let path = cache.path_for_uri(workspace, uri)?;
    let source = cache.source(workspace, &path)?;
    for candidate in [
        "selectionRange",
        "range",
        "targetSelectionRange",
        "targetRange",
    ]
    .iter()
    .filter_map(|key| item.get(*key))
    {
        if !valid_lsp_range(Some(candidate)) {
            return None;
        }
        let start_line = candidate.pointer("/start/line")?.as_u64()?;
        let start_character = candidate.pointer("/start/character")?.as_u64()?;
        let end_line = candidate.pointer("/end/line")?.as_u64()?;
        let end_character = candidate.pointer("/end/character")?.as_u64()?;
        lsp_to_byte_column(&source.content, start_line + 1, start_character, encoding).ok()?;
        lsp_to_byte_column(&source.content, end_line + 1, end_character, encoding).ok()?;
    }
    let zero_line = range.pointer("/start/line")?.as_u64()?;
    let lsp_character = range.pointer("/start/character")?.as_u64()?;
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

pub(super) fn byte_column_to_lsp(
    content: &str,
    line: u64,
    column: u64,
    encoding: &str,
) -> Result<u64> {
    let text = content
        .split('\n')
        .nth(usize::try_from(line - 1).unwrap_or(usize::MAX))
        .ok_or_else(|| anyhow!("LSP navigation line is outside the source file"))?;
    let text = text.strip_suffix('\r').unwrap_or(text);
    let byte_offset = usize::try_from(column - 1).map_err(|_| anyhow!("column is too large"))?;
    if byte_offset > text.len() || !text.is_char_boundary(byte_offset) {
        bail!(
            "LSP navigation character must be a 1-based UTF-8 byte column on a character boundary"
        );
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
    let text = text.strip_suffix('\r').unwrap_or(text);
    let target = usize::try_from(character).map_err(|_| anyhow!("LSP character is too large"))?;
    let byte_offset = match encoding {
        "utf-8" => {
            if target > text.len() || !text.is_char_boundary(target) {
                bail!("LSP UTF-8 character is outside the source line or splits a code point");
            }
            target
        }
        "utf-32" => {
            let char_count = text.chars().count();
            if target > char_count {
                bail!("LSP UTF-32 character is outside the source line");
            }
            if target == char_count {
                text.len()
            } else {
                text.char_indices()
                    .nth(target)
                    .map(|(index, _)| index)
                    .ok_or_else(|| anyhow!("LSP UTF-32 character is outside the source line"))?
            }
        }
        _ => {
            let mut units = 0usize;
            let mut offset = None;
            for (index, ch) in text.char_indices() {
                if units == target {
                    offset = Some(index);
                    break;
                }
                let next = units.saturating_add(ch.len_utf16());
                if target < next {
                    bail!("LSP UTF-16 character splits a code point");
                }
                units = next;
            }
            if offset.is_none() && units == target {
                offset = Some(text.len());
            }
            offset.ok_or_else(|| anyhow!("LSP UTF-16 character is outside the source line"))?
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
