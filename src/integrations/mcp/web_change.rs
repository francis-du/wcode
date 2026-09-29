use super::*;

#[derive(serde::Deserialize)]
pub(crate) struct IntelligenceChangeQuery {
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) layer: crate::workspace::ChangeLayer,
    pub(crate) expected_snapshot: Option<String>,
}

const MAX_CHANGE_SYMBOLS: usize = 128;

pub(crate) fn changed_symbol_impact(
    path: &str,
    snapshot_id: &str,
    source_sha256: &str,
    source_state: &'static str,
    ranges: &[(usize, usize)],
    ranges_partial: bool,
    outline: Value,
) -> AnyResult<Value> {
    if outline.get("sha256").and_then(Value::as_str) != Some(source_sha256) {
        anyhow::bail!("change snapshot changed during symbol mapping");
    }
    let provider = outline
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or("tree-sitter");
    let precision = outline
        .get("precision")
        .and_then(Value::as_str)
        .unwrap_or("syntax");
    let parse_errors = outline
        .get("parse_errors")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if parse_errors {
        return Ok(json!({
            "path": path,
            "snapshot_id": snapshot_id,
            "source_sha256": source_sha256,
            "source_state": source_state,
            "precision": precision,
            "provider": provider,
            "mapping": "after_line_overlap",
            "before_symbols_available": false,
            "before_unavailable_reason": "baseline_source_not_parsed",
            "after_symbols": [],
            "partial": true,
            "unavailable_reason": "syntax_parse_error",
        }));
    }
    let outline_symbols = outline.get("symbols").and_then(Value::as_array);
    let mut mapping_partial = outline_symbols.is_none();
    let mut symbols = Vec::new();
    for symbol in outline_symbols.into_iter().flatten() {
        match symbol.get("is_definition").and_then(Value::as_bool) {
            Some(true) => {}
            Some(false) => continue,
            None => {
                mapping_partial = true;
                continue;
            }
        }
        let Some(range) = symbol.get("range") else {
            mapping_partial = true;
            continue;
        };
        let Some(start_line) = range
            .get("start_line")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
        else {
            mapping_partial = true;
            continue;
        };
        let Some(end_line) = range
            .get("end_line")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
        else {
            mapping_partial = true;
            continue;
        };
        if start_line == 0 || end_line < start_line {
            mapping_partial = true;
            continue;
        }
        let changed_ranges = ranges
            .iter()
            .filter_map(|(changed_start, changed_end)| {
                let overlap_start = start_line.max(*changed_start);
                let overlap_end = end_line.min(*changed_end);
                (overlap_start <= overlap_end)
                    .then(|| json!({"start_line": overlap_start, "end_line": overlap_end}))
            })
            .collect::<Vec<_>>();
        if changed_ranges.is_empty() {
            continue;
        }
        let Some(id) = symbol.get("id").and_then(Value::as_str) else {
            mapping_partial = true;
            continue;
        };
        if !(id.starts_with("ts:") || id.starts_with("symbol:ts:")) {
            mapping_partial = true;
            continue;
        }
        symbols.push(json!({
            "node_id": if id.starts_with("symbol:") { id.to_owned() } else { format!("symbol:{id}") },
            "name": symbol.get("name").and_then(Value::as_str).unwrap_or(""),
            "qualified_name": symbol.get("qualified_name").and_then(Value::as_str).unwrap_or(""),
            "kind": symbol.get("kind").and_then(Value::as_str).unwrap_or("symbol"),
            "signature": symbol.get("signature").and_then(Value::as_str).unwrap_or(""),
            "signature_redacted": symbol.get("signature_redacted").and_then(Value::as_bool).unwrap_or(true),
            "start_line": start_line,
            "end_line": end_line,
            "changed_ranges": changed_ranges,
            "counterpart_only": false,
        }));
    }
    let symbols_truncated = symbols.len() > MAX_CHANGE_SYMBOLS;
    symbols.truncate(MAX_CHANGE_SYMBOLS);
    let outline_truncated = outline
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(json!({
        "path": path,
        "snapshot_id": snapshot_id,
        "source_sha256": source_sha256,
        "source_state": source_state,
        "precision": precision,
        "provider": provider,
        "mapping": "after_line_overlap",
        "before_symbols_available": false,
        "before_unavailable_reason": "baseline_source_not_parsed",
        "after_symbols": symbols,
        "partial": ranges_partial || outline_truncated || symbols_truncated || mapping_partial,
        "unavailable_reason": null,
    }))
}

fn symbol_identity(symbol: &Value) -> Option<(String, String)> {
    let qualified_name = symbol.get("qualified_name")?.as_str()?.trim();
    let kind = symbol.get("kind")?.as_str()?.trim();
    if qualified_name.is_empty() || kind.is_empty() {
        return None;
    }
    Some((qualified_name.to_owned(), kind.to_owned()))
}

fn outline_counterpart(outline: &Value, target: &Value) -> Option<Value> {
    let identity = symbol_identity(target)?;
    let mut matches = outline
        .get("symbols")
        .and_then(Value::as_array)?
        .iter()
        .filter(|symbol| symbol.get("is_definition").and_then(Value::as_bool) == Some(true))
        .filter(|symbol| symbol_identity(symbol).as_ref() == Some(&identity));
    let symbol = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let range = symbol.get("range")?;
    let start_line = range
        .get("start_line")?
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())?;
    let end_line = range
        .get("end_line")?
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())?;
    if start_line == 0 || end_line < start_line {
        return None;
    }
    let id = symbol.get("id")?.as_str()?;
    Some(json!({
        "node_id": if id.starts_with("symbol:") { id.to_owned() } else { format!("symbol:{id}") },
        "name": symbol.get("name").and_then(Value::as_str).unwrap_or(""),
        "qualified_name": symbol.get("qualified_name").and_then(Value::as_str).unwrap_or(""),
        "kind": symbol.get("kind").and_then(Value::as_str).unwrap_or("symbol"),
        "signature": symbol.get("signature").and_then(Value::as_str).unwrap_or(""),
        "signature_redacted": symbol.get("signature_redacted").and_then(Value::as_bool).unwrap_or(true),
        "start_line": start_line,
        "end_line": end_line,
        "changed_ranges": [],
        "counterpart_only": true,
    }))
}

fn augment_symbol_counterparts(
    mut impact: Value,
    before_outline: Option<&Value>,
    after_outline: Option<&Value>,
) -> Value {
    if impact.get("mapping").and_then(Value::as_str) != Some("before_after_line_overlap") {
        return impact;
    }
    let mut before = impact
        .get("before_symbols")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut after = impact
        .get("after_symbols")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut partial = impact
        .get("partial")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    if let Some(outline) = before_outline {
        for target in after.clone() {
            let Some(identity) = symbol_identity(&target) else {
                continue;
            };
            if before
                .iter()
                .any(|symbol| symbol_identity(symbol).as_ref() == Some(&identity))
            {
                continue;
            }
            if before.len() >= MAX_CHANGE_SYMBOLS {
                partial = true;
                break;
            }
            if let Some(counterpart) = outline_counterpart(outline, &target) {
                before.push(counterpart);
            }
        }
    }
    if let Some(outline) = after_outline {
        for target in before.clone() {
            let Some(identity) = symbol_identity(&target) else {
                continue;
            };
            if after
                .iter()
                .any(|symbol| symbol_identity(symbol).as_ref() == Some(&identity))
            {
                continue;
            }
            if after.len() >= MAX_CHANGE_SYMBOLS {
                partial = true;
                break;
            }
            if let Some(counterpart) = outline_counterpart(outline, &target) {
                after.push(counterpart);
            }
        }
    }

    let outline_complete = |outline: &Value| {
        outline.get("parse_errors").and_then(Value::as_bool) == Some(false)
            && outline.get("truncated").and_then(Value::as_bool) == Some(false)
            && outline.get("symbols").and_then(Value::as_array).is_some()
    };
    let definition_match_count = |outline: &Value, target: &Value| -> Option<usize> {
        if !outline_complete(outline) {
            return None;
        }
        let identity = symbol_identity(target)?;
        Some(
            outline
                .get("symbols")
                .and_then(Value::as_array)?
                .iter()
                .filter(|symbol| {
                    symbol.get("is_definition").and_then(Value::as_bool) == Some(true)
                        && symbol_identity(symbol).as_ref() == Some(&identity)
                })
                .count(),
        )
    };
    let definition_change_basis = match (before_outline, after_outline) {
        (Some(before_outline), Some(after_outline))
            if outline_complete(before_outline) && outline_complete(after_outline) =>
        {
            Some("qualified_name_kind_complete_syntax_outlines")
        }
        _ => None,
    };
    if let (Some(before_outline), Some(after_outline), Some(_)) =
        (before_outline, after_outline, definition_change_basis)
    {
        for symbol in &mut before {
            let change = match definition_match_count(after_outline, symbol) {
                Some(0) => "removed",
                Some(1) => "modified",
                _ => "unknown",
            };
            symbol["definition_change"] = json!(change);
        }
        for symbol in &mut after {
            let change = match definition_match_count(before_outline, symbol) {
                Some(0) => "added",
                Some(1) => "modified",
                _ => "unknown",
            };
            symbol["definition_change"] = json!(change);
        }
    } else {
        for symbol in before.iter_mut().chain(after.iter_mut()) {
            symbol["definition_change"] = json!("unknown");
        }
    }

    impact["before_symbols"] = Value::Array(before);
    impact["after_symbols"] = Value::Array(after);
    impact["counterpart_basis"] = if before_outline.is_some() && after_outline.is_some() {
        json!("qualified_name_kind_syntax")
    } else {
        Value::Null
    };
    impact["definition_change_basis"] = definition_change_basis
        .map(Value::from)
        .unwrap_or(Value::Null);
    impact["partial"] = json!(partial);
    impact
}

fn merge_before_symbol_impact(
    mut after: Value,
    before: Value,
    source_state: &'static str,
    source_sha256: &str,
) -> Value {
    let before_unavailable = before
        .get("unavailable_reason")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let before_partial = before
        .get("partial")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let after_partial = after
        .get("partial")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    after["mapping"] = json!("before_after_line_overlap");
    after["before_symbols_available"] = json!(before_unavailable.is_none());
    after["before_source_state"] = json!(source_state);
    after["before_source_sha256"] = json!(source_sha256);
    after["before_symbols"] = before
        .get("after_symbols")
        .cloned()
        .unwrap_or_else(|| json!([]));
    after["before_unavailable_reason"] =
        before_unavailable.map(Value::String).unwrap_or(Value::Null);
    after["partial"] = json!(after_partial || before_partial);
    after
}

fn unavailable_changed_symbol_impact(
    path: &str,
    snapshot_id: &str,
    source_sha256: Option<&str>,
    reason: &'static str,
) -> Value {
    json!({
        "path": path,
        "snapshot_id": snapshot_id,
        "source_sha256": source_sha256,
        "source_state": null,
        "precision": "syntax",
        "provider": "tree-sitter",
        "mapping": "after_line_overlap",
        "before_symbols_available": false,
        "before_unavailable_reason": "baseline_source_not_parsed",
        "after_symbols": [],
        "partial": true,
        "unavailable_reason": reason,
    })
}

/// Source-level drilldown shares the graph inspector's authenticated workspace.
/// No arbitrary command, revision expression or filesystem root crosses this API.
pub(crate) async fn intelligence_web_change_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<IntelligenceChangeQuery>,
) -> Response {
    let (workspace_id, workspace) = match intelligence_ui_workspace(&state, &headers) {
        Ok(selected) => selected,
        Err(response) => return *response,
    };
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
                Json(json!({"error": "Repository revision is unavailable for this change snapshot.", "code": "revision_unavailable"})),
            )
                .into_response();
        }
    };
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(25),
        workspace.change_view(&query.path, query.layer, query.expected_snapshot.as_deref()),
    )
    .await;
    let (status, body) = match result {
        Ok(Ok(view)) => {
            let path = view.path.clone();
            let snapshot_id = view.snapshot_id.clone();
            let source_sha256 = view.worktree_sha256.clone();
            let ranges = view
                .after_changed_ranges
                .iter()
                .map(|range| (range.start_line, range.end_line))
                .collect::<Vec<_>>();
            let mut after_outline_for_pairing = None;
            let symbol_impact = if view.redacted {
                unavailable_changed_symbol_impact(
                    &path,
                    &snapshot_id,
                    source_sha256.as_deref(),
                    "redacted_change",
                )
            } else if let Some(snapshot) = view.after_source.as_ref() {
                let harness = state.harness.clone();
                let workspace_for_read = workspace.clone();
                let workspace_id_for_read = workspace_id.clone();
                let path_for_read = path.clone();
                let snapshot_sha256 = snapshot.sha256.clone();
                let snapshot_content = snapshot.content.clone();
                let snapshot_sha_for_parse = snapshot_sha256.clone();
                let outline = mcp_tools::run_blocking(move || {
                    harness.snapshot_file_outline(
                        &workspace_id_for_read,
                        &workspace_for_read,
                        &path_for_read,
                        &snapshot_content,
                        &snapshot_sha_for_parse,
                        512,
                    )
                })
                .await;
                match outline {
                    Ok(outline) => {
                        after_outline_for_pairing = Some(outline.clone());
                        match changed_symbol_impact(
                            &path,
                            &snapshot_id,
                            &snapshot_sha256,
                            "index",
                            &ranges,
                            view.changed_ranges_truncated,
                            outline,
                        ) {
                            Ok(impact) => impact,
                            Err(_) => unavailable_changed_symbol_impact(
                                &path,
                                &snapshot_id,
                                Some(&snapshot_sha256),
                                "syntax_outline_unavailable",
                            ),
                        }
                    }
                    Err(_) => unavailable_changed_symbol_impact(
                        &path,
                        &snapshot_id,
                        Some(&snapshot_sha256),
                        "syntax_outline_unavailable",
                    ),
                }
            } else if query.layer == crate::workspace::ChangeLayer::Staged
                && !view.after_source_matches_worktree
            {
                unavailable_changed_symbol_impact(
                    &path,
                    &snapshot_id,
                    None,
                    "exact_after_source_unavailable",
                )
            } else if let Some(source_sha256) = source_sha256.as_deref() {
                let harness = state.harness.clone();
                let workspace_for_read = workspace.clone();
                let workspace_id_for_read = workspace_id.clone();
                let path_for_read = path.clone();
                let outline = mcp_tools::run_blocking(move || {
                    harness.file_outline(
                        workspace_id_for_read,
                        &workspace_for_read,
                        &path_for_read,
                        512,
                    )
                })
                .await;
                match outline {
                    Ok(outline) => {
                        after_outline_for_pairing = Some(outline.clone());
                        match changed_symbol_impact(
                            &path,
                            &snapshot_id,
                            source_sha256,
                            "worktree",
                            &ranges,
                            view.changed_ranges_truncated,
                            outline,
                        ) {
                            Ok(impact) => impact,
                            Err(error)
                                if error
                                    .to_string()
                                    .contains("change snapshot changed during symbol mapping") =>
                            {
                                return (
                                    StatusCode::CONFLICT,
                                    [(header::CACHE_CONTROL, "no-store")],
                                    Json(json!({"error": "Change snapshot changed. Reload the selected file.", "code": "stale_change"})),
                                )
                                    .into_response();
                            }
                            Err(_) => unavailable_changed_symbol_impact(
                                &path,
                                &snapshot_id,
                                Some(source_sha256),
                                "syntax_outline_unavailable",
                            ),
                        }
                    }
                    Err(_) => unavailable_changed_symbol_impact(
                        &path,
                        &snapshot_id,
                        Some(source_sha256),
                        "syntax_outline_unavailable",
                    ),
                }
            } else {
                unavailable_changed_symbol_impact(&path, &snapshot_id, None, "no_current_source")
            };
            let mut before_outline_for_pairing = None;
            let symbol_impact = if !view.redacted {
                if let Some(snapshot) = view.before_source.as_ref() {
                    let harness = state.harness.clone();
                    let workspace_for_read = workspace.clone();
                    let workspace_id_for_read = workspace_id.clone();
                    let path_for_read = path.clone();
                    let snapshot_sha256 = snapshot.sha256.clone();
                    let snapshot_content = snapshot.content.clone();
                    let snapshot_sha_for_parse = snapshot_sha256.clone();
                    let outline = mcp_tools::run_blocking(move || {
                        harness.snapshot_file_outline(
                            &workspace_id_for_read,
                            &workspace_for_read,
                            &path_for_read,
                            &snapshot_content,
                            &snapshot_sha_for_parse,
                            512,
                        )
                    })
                    .await;
                    let before_state = if query.layer == crate::workspace::ChangeLayer::Unstaged {
                        "index"
                    } else {
                        "head"
                    };
                    let before_ranges = view
                        .before_changed_ranges
                        .iter()
                        .map(|range| (range.start_line, range.end_line))
                        .collect::<Vec<_>>();
                    match outline {
                        Ok(outline) => {
                            before_outline_for_pairing = Some(outline.clone());
                            match changed_symbol_impact(
                                &path,
                                &snapshot_id,
                                &snapshot_sha256,
                                before_state,
                                &before_ranges,
                                view.changed_ranges_truncated,
                                outline,
                            ) {
                                Ok(before_impact) => merge_before_symbol_impact(
                                    symbol_impact,
                                    before_impact,
                                    before_state,
                                    &snapshot_sha256,
                                ),
                                Err(_) => symbol_impact,
                            }
                        }
                        Err(_) => symbol_impact,
                    }
                } else {
                    symbol_impact
                }
            } else {
                symbol_impact
            };
            let symbol_impact = augment_symbol_counterparts(
                symbol_impact,
                before_outline_for_pairing.as_ref(),
                after_outline_for_pairing.as_ref(),
            );
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
                        Json(json!({"error": "Repository revision changed while inspecting this file. Reload before using proof.", "code": "stale_change"})),
                    )
                        .into_response();
                }
                Err(_) => {
                    return (
                        StatusCode::UNPROCESSABLE_ENTITY,
                        [(header::CACHE_CONTROL, "no-store")],
                        Json(json!({"error": "Repository revision is unavailable for this change snapshot.", "code": "revision_unavailable"})),
                    )
                        .into_response();
                }
            };
            (
                StatusCode::OK,
                json!({"workspace": workspace_id, "repository_revision": repository_revision, "change": view, "symbol_impact": symbol_impact}),
            )
        }
        Ok(Err(error)) if error.to_string().starts_with("change snapshot changed") => (
            StatusCode::CONFLICT,
            json!({"error": "Change snapshot changed. Reload the selected file.", "code": "stale_change"}),
        ),
        Ok(Err(_)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({"error": "Change inspection unavailable for this path or repository state.", "code": "change_unavailable"}),
        ),
        Err(_) => (
            StatusCode::GATEWAY_TIMEOUT,
            json!({"error": "Change inspection timed out.", "code": "change_timeout"}),
        ),
    };
    (status, [(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}
