use super::web_graph::{bad_request, changed_symbol_impact};
use super::*;

#[derive(serde::Deserialize)]
pub(crate) struct IntelligenceChangeImpactQuery {
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) layer: crate::workspace::ChangeLayer,
    pub(crate) expected_snapshot: String,
    pub(crate) expected_code_revision: String,
    #[serde(default)]
    pub(crate) expected_design_revision: Option<String>,
    pub(crate) node_id: String,
}

const MAX_CHANGE_RELATIONS: usize = 24;

fn partial_or_malformed_flag(value: Option<&Value>) -> bool {
    match value {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => true,
    }
}

fn bounded_change_locations(
    value: Option<&Value>,
    allow_syntax_node_id: bool,
) -> (Vec<Value>, bool) {
    let Some(value) = value else {
        return (Vec::new(), false);
    };
    let Some(items) = value.as_array() else {
        return (Vec::new(), true);
    };
    let mut locations = Vec::new();
    let mut partial = items.len() >= MAX_CHANGE_RELATIONS;
    for item in items {
        let Some(path) = item.get("path").and_then(Value::as_str).map(str::trim) else {
            partial = true;
            continue;
        };
        let Some(line) = item.get("line").and_then(Value::as_u64) else {
            partial = true;
            continue;
        };
        let character = match item.get("character") {
            None => 1,
            Some(value) => match value.as_u64() {
                Some(character) if character > 0 => character,
                _ => {
                    partial = true;
                    continue;
                }
            },
        };
        if path.is_empty() || line == 0 {
            partial = true;
            continue;
        }
        if locations.len() >= MAX_CHANGE_RELATIONS {
            partial = true;
            break;
        }
        let node_id = if allow_syntax_node_id {
            match item.get("node_id") {
                None => None,
                Some(value) if value.is_null() => None,
                Some(value) => match value
                    .as_str()
                    .filter(|value| value.starts_with("symbol:ts:"))
                {
                    Some(node_id) => Some(node_id),
                    None => {
                        partial = true;
                        None
                    }
                },
            }
        } else {
            None
        };
        locations.push(json!({
            "path": path,
            "line": line,
            "character": character,
            "name": item.get("name").and_then(Value::as_str),
            "node_id": node_id,
        }));
    }
    (locations, partial)
}

fn bounded_change_search_matches(
    selected_path: &str,
    selected_line: usize,
    value: Option<&Value>,
) -> (Vec<Value>, bool) {
    let Some(value) = value else {
        return (Vec::new(), false);
    };
    let Some(items) = value.as_array() else {
        return (Vec::new(), true);
    };
    let mut matches = Vec::new();
    let mut partial = false;
    for item in items {
        match item.get("redacted") {
            Some(Value::Bool(true)) => {
                partial = true;
                continue;
            }
            Some(Value::Bool(false)) | None => {}
            Some(_) => {
                partial = true;
                continue;
            }
        }
        let Some(path) = item.get("path").and_then(Value::as_str).map(str::trim) else {
            partial = true;
            continue;
        };
        let Some(line) = item.get("line").and_then(Value::as_u64) else {
            partial = true;
            continue;
        };
        let Some(source_sha256) = item.get("sha256").and_then(Value::as_str) else {
            partial = true;
            continue;
        };
        if path.is_empty()
            || line == 0
            || source_sha256.len() != 64
            || !source_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            partial = true;
            continue;
        }
        if path == selected_path && line == selected_line as u64 {
            continue;
        }
        if matches.len() >= MAX_CHANGE_RELATIONS {
            partial = true;
            break;
        }
        let mut text = item
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        if text.chars().count() > 240 {
            text = text.chars().take(240).collect();
            partial = true;
        }
        if partial_or_malformed_flag(item.get("text_truncated")) {
            partial = true;
        }
        matches.push(json!({
            "path": path,
            "line": line,
            "source_sha256": source_sha256,
            "text": text,
        }));
    }
    if items.len() >= MAX_CHANGE_RELATIONS {
        partial = true;
    }
    (matches, partial)
}

pub(crate) fn normalize_change_relation_impact(
    path: &str,
    snapshot_id: &str,
    node_id: &str,
    source_sha256: &str,
    selected_line: usize,
    navigation: Value,
) -> AnyResult<Value> {
    let expected_revision = format!("sha256:{source_sha256}");
    let navigation_path = navigation
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("change impact path provenance is unavailable"))?;
    if navigation_path != path {
        anyhow::bail!("change impact path provenance is contradictory");
    }
    let selector = navigation
        .get("selector")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow::anyhow!("change impact selector provenance is unavailable"))?;
    if selector.get("revision").and_then(Value::as_str) != Some(expected_revision.as_str()) {
        anyhow::bail!("change snapshot changed during relation impact");
    }
    let selector_line = selector
        .get("line")
        .and_then(Value::as_u64)
        .and_then(|line| usize::try_from(line).ok());
    let selector_name = selector
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty());
    if selector_line != Some(selected_line) || selector_name.is_none() {
        anyhow::bail!("change impact selector provenance is contradictory");
    }
    let degraded = match navigation.get("degraded") {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => anyhow::bail!("change impact degraded provenance is malformed"),
    };
    let provider = navigation
        .get("provider")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("change impact provider provenance is unavailable"))?;
    let precision = navigation
        .get("precision")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("change impact precision provenance is unavailable"))?;
    let raw_routing = navigation
        .get("routing")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("change impact routing provenance is unavailable"))?;
    let degraded_from = navigation.get("degraded_from").and_then(Value::as_str);
    if degraded {
        if provider != "tree-sitter+search"
            || precision != "syntax"
            || raw_routing != "degraded_syntax_and_keyword_search"
            || degraded_from != Some("lsp")
        {
            anyhow::bail!("change impact degraded provenance is contradictory");
        }
        for key in ["references", "implementations"] {
            if let Some(value) = navigation.get(key) {
                let Some(items) = value.as_array() else {
                    anyhow::bail!("change impact degraded semantic relations are malformed");
                };
                if !items.is_empty() {
                    anyhow::bail!("change impact degraded semantic relations are contradictory");
                }
            }
        }
    } else {
        let query_line = navigation
            .get("line")
            .and_then(Value::as_u64)
            .and_then(|line| usize::try_from(line).ok());
        let query_character = navigation
            .get("character")
            .and_then(Value::as_u64)
            .filter(|character| *character > 0);
        let selector_character = selector
            .get("character")
            .and_then(Value::as_u64)
            .filter(|character| *character > 0);
        if query_line != selector_line
            || query_character.is_none()
            || query_character != selector_character
        {
            anyhow::bail!("change impact semantic query provenance is contradictory");
        }
        if precision != "semantic"
            || raw_routing != "cross_file_semantic"
            || !provider.starts_with("lsp:")
            || navigation
                .get("degraded_from")
                .is_some_and(|value| !value.is_null())
        {
            anyhow::bail!("change impact semantic provenance is contradictory");
        }
        if let Some(value) = navigation.get("keyword_matches") {
            if !value.as_array().is_some_and(Vec::is_empty) {
                anyhow::bail!("change impact semantic payload contains degraded search candidates");
            }
        }
        for key in ["incoming_calls", "references", "implementations"] {
            if navigation
                .get(key)
                .and_then(Value::as_array)
                .is_some_and(|items| {
                    items.iter().any(|item| {
                        item.get("node_id")
                            .is_some_and(|node_id| !node_id.is_null())
                    })
                })
            {
                anyhow::bail!("change impact semantic relation contains syntax node identity");
            }
        }
    }
    let public_routing = if degraded { "syntax-degraded" } else { "lsp" };
    let (incoming_calls, incoming_partial) = if degraded {
        bounded_change_locations(navigation.pointer("/syntax_calls/incoming_calls"), true)
    } else {
        bounded_change_locations(navigation.get("incoming_calls"), false)
    };
    let (references, references_partial) = if degraded {
        (Vec::new(), false)
    } else {
        bounded_change_locations(navigation.get("references"), false)
    };
    let (implementations, implementations_partial) = if degraded {
        (Vec::new(), false)
    } else {
        bounded_change_locations(navigation.get("implementations"), false)
    };
    let (search_matches, search_partial) = if degraded {
        bounded_change_search_matches(path, selected_line, navigation.get("keyword_matches"))
    } else {
        (Vec::new(), false)
    };
    let degraded_collection_partial = degraded
        && (!navigation
            .pointer("/syntax_calls/incoming_calls")
            .is_some_and(Value::is_array)
            || !navigation
                .get("keyword_matches")
                .is_some_and(Value::is_array));
    let semantic_collection_partial = !degraded
        && ["incoming_calls", "references", "implementations"]
            .iter()
            .any(|key| !navigation.get(*key).is_some_and(Value::is_array));
    let semantic_query_partial = !degraded
        && (!navigation
            .get("queried")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items.len() == 3
                    && ["references", "implementations", "calls"]
                        .iter()
                        .all(|expected| items.iter().any(|item| item.as_str() == Some(*expected)))
            })
            || ["unsupported", "failures"]
                .iter()
                .any(|key| match navigation.get(*key) {
                    Some(Value::Array(items)) => !items.is_empty(),
                    _ => true,
                }));
    let syntax_partial = partial_or_malformed_flag(navigation.pointer("/syntax_calls/truncated"))
        || partial_or_malformed_flag(navigation.pointer("/syntax_calls/graph_truncated"))
        || partial_or_malformed_flag(navigation.pointer("/syntax_calls/scan_truncated"));
    Ok(json!({
        "path": path,
        "snapshot_id": snapshot_id,
        "node_id": node_id,
        "source_sha256": source_sha256,
        "provider": provider,
        "precision": precision,
        "routing": public_routing,
        "degraded": degraded,
        "degraded_from": degraded_from,
        "reason": navigation.get("reason").and_then(Value::as_str),
        "incoming_calls": incoming_calls,
        "references": references,
        "implementations": implementations,
        "search_matches": search_matches,
        "partial": partial_or_malformed_flag(navigation.get("truncated"))
            || syntax_partial
            || degraded_collection_partial
            || semantic_collection_partial
            || semantic_query_partial
            || search_partial
            || incoming_partial
            || references_partial
            || implementations_partial,
    }))
}

pub(crate) fn normalize_change_relation_impact_for_symbol(
    path: &str,
    snapshot_id: &str,
    node_id: &str,
    source_sha256: &str,
    selected_line: usize,
    selected_name: &str,
    navigation: Value,
) -> AnyResult<Value> {
    let actual_name = navigation
        .get("selector")
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .map(str::trim);
    if selected_name.trim().is_empty() || actual_name != Some(selected_name.trim()) {
        anyhow::bail!("change impact selector provenance is contradictory");
    }
    normalize_change_relation_impact(
        path,
        snapshot_id,
        node_id,
        source_sha256,
        selected_line,
        navigation,
    )
}

pub(crate) async fn intelligence_web_change_impact(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<IntelligenceChangeImpactQuery>,
) -> Response {
    let (workspace_id, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return *response,
    };
    if !query.node_id.starts_with("symbol:ts:")
        || query.expected_snapshot.trim().is_empty()
        || query.expected_code_revision.trim().is_empty()
    {
        return bad_request(
            "node_id, expected_snapshot, and expected_code_revision are required for change impact",
        );
    }
    let harness = state.harness.clone();
    let workspace_for_revision = workspace.clone();
    let repository_revision_before = match mcp_tools::run_blocking(move || {
        harness.current_revision(&workspace_for_revision)
    })
    .await
    {
        Ok(revision) => revision,
        Err(_) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                [(header::CACHE_CONTROL, "no-store")],
                Json(json!({"error":"Repository revision is unavailable for impact navigation.","code":"revision_unavailable"})),
            )
                .into_response();
        }
    };
    if repository_revision_before.code != query.expected_code_revision
        || repository_revision_before.design.as_deref() != query.expected_design_revision.as_deref()
    {
        return (
            StatusCode::CONFLICT,
            [(header::CACHE_CONTROL, "no-store")],
            Json(json!({"error":"Repository revision changed. Reload before tracing impact.","code":"stale_change"})),
        )
            .into_response();
    }

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(25),
        workspace.change_view(
            &query.path,
            query.layer,
            Some(query.expected_snapshot.as_str()),
        ),
    )
    .await;
    let view = match result {
        Ok(Ok(view)) => view,
        Ok(Err(error)) if error.to_string().starts_with("change snapshot changed") => {
            return (
                StatusCode::CONFLICT,
                [(header::CACHE_CONTROL, "no-store")],
                Json(json!({"error":"Change snapshot changed. Reload the selected file.","code":"stale_change"})),
            )
                .into_response();
        }
        Ok(Err(_)) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                [(header::CACHE_CONTROL, "no-store")],
                Json(json!({"error":"Change impact is unavailable for this path or repository state.","code":"impact_unavailable"})),
            )
                .into_response();
        }
        Err(_) => {
            return (
                StatusCode::GATEWAY_TIMEOUT,
                [(header::CACHE_CONTROL, "no-store")],
                Json(json!({"error":"Change impact timed out.","code":"impact_timeout"})),
            )
                .into_response();
        }
    };
    if view.redacted
        || !view.after_source_matches_worktree
        || view.worktree_sha256.is_none()
        || view.snapshot_id != query.expected_snapshot
    {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            [(header::CACHE_CONTROL, "no-store")],
            Json(json!({"error":"Impact requires the exact current working-tree bytes for a changed symbol.","code":"impact_unavailable"})),
        )
            .into_response();
    }

    let source_sha256 = view.worktree_sha256.as_deref().unwrap_or_default();
    let ranges = view
        .after_changed_ranges
        .iter()
        .map(|range| (range.start_line, range.end_line))
        .collect::<Vec<_>>();
    let harness = state.harness.clone();
    let workspace_for_read = workspace.clone();
    let workspace_id_for_read = workspace_id.clone();
    let path_for_read = view.path.clone();
    let outline = mcp_tools::run_blocking(move || {
        harness.file_outline(
            workspace_id_for_read,
            &workspace_for_read,
            &path_for_read,
            512,
        )
    })
    .await;
    let mapped = match outline.and_then(|outline| {
        changed_symbol_impact(
            &view.path,
            &view.snapshot_id,
            source_sha256,
            "worktree",
            &ranges,
            view.changed_ranges_truncated,
            outline,
        )
    }) {
        Ok(mapped) => mapped,
        Err(error)
            if error
                .to_string()
                .contains("change snapshot changed during symbol mapping") =>
        {
            return (
                StatusCode::CONFLICT,
                [(header::CACHE_CONTROL, "no-store")],
                Json(json!({"error":"Change snapshot changed. Reload the selected file.","code":"stale_change"})),
            )
                .into_response();
        }
        Err(_) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                [(header::CACHE_CONTROL, "no-store")],
                Json(json!({"error":"Changed-symbol mapping is unavailable for this snapshot.","code":"impact_unavailable"})),
            )
                .into_response();
        }
    };
    let Some(symbol) = mapped
        .get("after_symbols")
        .and_then(Value::as_array)
        .and_then(|symbols| {
            symbols.iter().find(|symbol| {
                symbol.get("node_id").and_then(Value::as_str) == Some(query.node_id.as_str())
                    && symbol.get("counterpart_only").and_then(Value::as_bool) == Some(false)
            })
        })
    else {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            [(header::CACHE_CONTROL, "no-store")],
            Json(json!({"error":"The requested symbol is not one of the directly changed definitions in this snapshot.","code":"impact_unavailable"})),
        )
            .into_response();
    };
    let selector = symbol
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .or_else(|| symbol.get("qualified_name").and_then(Value::as_str))
        .unwrap_or("")
        .trim()
        .to_owned();
    let selected_line = symbol
        .get("start_line")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0);
    if selector.is_empty() || selected_line.is_none() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            [(header::CACHE_CONTROL, "no-store")],
            Json(json!({"error":"Changed-symbol identity is unavailable for impact navigation.","code":"impact_unavailable"})),
        )
            .into_response();
    }

    let request = crate::harness::SemanticNavigationRequest {
        path: view.path.clone(),
        symbol: Some(selector.clone()),
        line: selected_line,
        character: None,
        intent: crate::semantic_provider::SemanticNavigationIntent::Impact,
        max_results: MAX_CHANGE_RELATIONS,
        new_name: None,
        max_files: MAX_CHANGE_RELATIONS,
    };
    let navigation = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        state
            .harness
            .semantic_navigation(&workspace_id, &workspace, &request),
    )
    .await;
    let impact = match navigation {
        Ok(Ok(navigation)) => match normalize_change_relation_impact_for_symbol(
            &view.path,
            &view.snapshot_id,
            &query.node_id,
            source_sha256,
            selected_line.unwrap_or_default(),
            selector.as_str(),
            navigation,
        ) {
            Ok(impact) => impact,
            Err(error)
                if error
                    .to_string()
                    .contains("change snapshot changed during relation impact") =>
            {
                return (
                    StatusCode::CONFLICT,
                    [(header::CACHE_CONTROL, "no-store")],
                    Json(json!({"error":"Change snapshot changed. Reload the selected file.","code":"stale_change"})),
                )
                    .into_response();
            }
            Err(_) => {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    [(header::CACHE_CONTROL, "no-store")],
                    Json(json!({"error":"Impact navigation is unavailable for this changed symbol.","code":"impact_unavailable"})),
                )
                    .into_response();
            }
        },
        Ok(Err(_)) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                [(header::CACHE_CONTROL, "no-store")],
                Json(json!({"error":"Impact navigation is unavailable for this changed symbol.","code":"impact_unavailable"})),
            )
                .into_response();
        }
        Err(_) => {
            return (
                StatusCode::GATEWAY_TIMEOUT,
                [(header::CACHE_CONTROL, "no-store")],
                Json(json!({"error":"Change impact timed out.","code":"impact_timeout"})),
            )
                .into_response();
        }
    };
    let harness = state.harness.clone();
    let workspace_for_revision = workspace.clone();
    let repository_revision = match mcp_tools::run_blocking(move || {
        harness.current_revision(&workspace_for_revision)
    })
    .await
    {
        Ok(revision) if revision == repository_revision_before => revision,
        Ok(_) => {
            return (
                StatusCode::CONFLICT,
                [(header::CACHE_CONTROL, "no-store")],
                Json(json!({"error":"Repository revision changed while tracing impact. Reload before using these relations.","code":"stale_change"})),
            )
                .into_response();
        }
        Err(_) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                [(header::CACHE_CONTROL, "no-store")],
                Json(json!({"error":"Repository revision is unavailable for impact navigation.","code":"revision_unavailable"})),
            )
                .into_response();
        }
    };
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({"workspace":workspace_id,"repository_revision":repository_revision,"impact":impact})),
    )
        .into_response()
}
