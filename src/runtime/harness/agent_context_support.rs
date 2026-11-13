use super::*;

pub(super) fn compact_core_constraints() -> Vec<Value> {
    vec![
        json!({
            "id": "CONSTRAINT-SOURCE-DECOMPOSITION",
            "rule": "maintained source <=1000 lines; split before growth; generated/binary outputs exempt; filenames <=32 chars; Rust stems <=24",
        }),
        json!({
            "id": "CONSTRAINT-TEST-ROOT",
            "rule": "standalone automated tests live under tests/; avoid growing large inline test modules",
        }),
        json!({
            "id": "CONSTRAINT-DESIGN-SYNC",
            "rule": "responsibility/path/test/trust/transport moves update matching .wcode Design State in the same change",
        }),
        json!({
            "id": "CONSTRAINT-PARALLEL-FIRST",
            "rule": "independent lanes must run concurrently; prefer one-traversal bulk tools and shared caches before per-item fan-out; serialize only true dependencies",
        }),
    ]
}

pub(super) fn compact_convention_report(report: &ConventionReport) -> Value {
    json!({
        "provider": report.provider,
        "errors": report.errors,
        "warnings": report.warnings,
        "truncated": report.truncated,
        "findings": report.findings.iter().take(12).map(|finding| json!({
            "code": finding.code,
            "severity": finding.severity,
            "path": finding.path,
            "language": finding.language,
            "message": short_text(&finding.message, 260),
        })).collect::<Vec<_>>(),
    })
}

pub(super) fn compact_design_item(item: &crate::intelligence::DesignContextItem) -> Value {
    let relations = item
        .relations
        .iter()
        .take(4)
        .map(|(name, values)| {
            (
                name.clone(),
                Value::Array(
                    values
                        .iter()
                        .take(6)
                        .map(|value| Value::String(value.clone()))
                        .collect(),
                ),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    json!({
        "id": item.id,
        "kind": item.kind,
        "title": item.title,
        "summary": short_text(&item.summary, 360),
        "relations": relations,
    })
}

pub(super) fn compact_symbol(symbol: &Value) -> Value {
    json!({
        "id": symbol.get("id").cloned().unwrap_or(Value::Null),
        "path": symbol.get("path").cloned().unwrap_or(Value::Null),
        "kind": symbol.get("kind").cloned().unwrap_or(Value::Null),
        "qualified_name": symbol.get("qualified_name").cloned().unwrap_or(Value::Null),
        "signature": symbol.get("signature").cloned().unwrap_or(Value::Null),
        "start_line": symbol.pointer("/range/start_line").cloned().unwrap_or(Value::Null),
        "end_line": symbol.pointer("/range/end_line").cloned().unwrap_or(Value::Null),
        "language": symbol.get("language").cloned().unwrap_or(Value::Null),
        "provider": symbol.get("provider").cloned().unwrap_or(json!("tree-sitter")),
        "precision": symbol.get("precision").cloned().unwrap_or(json!("syntax")),
    })
}

pub(super) fn compact_hot_source(source: &Value, max_chars: usize) -> Value {
    let mut body = source.get("body").cloned().unwrap_or(Value::Null);
    truncate_source_body(&mut body, max_chars);
    let content = body
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let calls = source
        .get("syntax_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(8)
        .map(|call| {
            json!({
                "name": call.get("name").cloned().unwrap_or(Value::Null),
                "line": call.pointer("/range/start_line").cloned().unwrap_or(Value::Null),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "id": source.pointer("/symbol/id").cloned().unwrap_or(Value::Null),
        "path": source.pointer("/symbol/path").cloned().unwrap_or(Value::Null),
        "qualified_name": source.pointer("/symbol/qualified_name").cloned().unwrap_or(Value::Null),
        "signature": source.pointer("/symbol/signature").cloned().unwrap_or(Value::Null),
        "sha256": source.get("sha256").cloned().unwrap_or(Value::Null),
        "body": {
            "start_line": body.get("start_line").cloned().unwrap_or(Value::Null),
            "end_line": body.get("end_line").cloned().unwrap_or(Value::Null),
            "content": content,
            "redacted": body.get("redacted").cloned().unwrap_or(Value::Bool(false)),
            "truncated": body.get("truncated").and_then(Value::as_bool).unwrap_or(false),
        },
        "calls": calls,
    })
}

pub(super) fn implementation_hot_source(symbol: &Value) -> bool {
    if symbol["kind"].as_str() == Some("module") {
        return false;
    }
    let Some(path) = symbol["path"].as_str() else {
        return false;
    };
    let kind = symbol["kind"].as_str().unwrap_or_default();
    !super::harness_retrieval::test_path(path, kind)
}

pub(super) fn contains_implementation_hot_source(symbols: &[Value], ids: &[String]) -> bool {
    symbols.iter().any(|symbol| {
        implementation_hot_source(symbol)
            && symbol["id"]
                .as_str()
                .is_some_and(|id| ids.iter().any(|selected| selected == id))
    })
}

pub(super) fn primary_implementation_hot_source_id(
    symbols: &[Value],
    existing: &[String],
) -> Option<String> {
    symbols.iter().find_map(|symbol| {
        implementation_hot_source(symbol)
            .then(|| symbol["id"].as_str())
            .flatten()
            .filter(|id| !existing.iter().any(|selected| selected == id))
            .map(str::to_owned)
    })
}

pub(super) fn explicit_hot_source_ids(symbols: &[Value], query: &str) -> Vec<String> {
    let literals = crate::intelligence::code_query_literals(query)
        .into_iter()
        .collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    symbols
        .iter()
        .filter(|symbol| {
            ["name", "qualified_name"].iter().any(|key| {
                symbol[*key]
                    .as_str()
                    .is_some_and(|name| literals.contains(&name.to_ascii_lowercase()))
            })
        })
        .filter_map(|symbol| symbol["id"].as_str())
        .filter(|id| seen.insert((*id).to_owned()))
        .take(4)
        .map(str::to_owned)
        .collect()
}

// Code is not prose: keep an original UTF-8 prefix, prefer whole lines, and
// report only the lines actually included. Truncation is metadata, not an
// invented ellipsis that could be pasted into a guarded edit.
pub(super) fn truncate_source_body(body: &mut Value, max_chars: usize) -> bool {
    let Some(content) = body.get("content").and_then(Value::as_str) else {
        return false;
    };
    let Some((limit, _)) = content.char_indices().nth(max_chars) else {
        return false;
    };
    let end = content[..limit]
        .rfind('\n')
        .map_or(limit, |position| position + 1);
    let excerpt = content[..end].to_owned();
    let lines = excerpt.lines().count() as u64;
    let start = body.get("start_line").and_then(Value::as_u64).unwrap_or(1);
    body["end_line"] = json!(start.saturating_add(lines).saturating_sub(1));
    body["content"] = json!(excerpt);
    body["truncated"] = json!(true);
    true
}

pub(super) fn trace_target_path(target: &str) -> Option<String> {
    let candidate = target.split_once("::").map_or(target, |(path, _)| path);
    (candidate.contains('/') || candidate.contains('\\') || candidate.contains('.'))
        .then(|| candidate.to_owned())
}

pub(super) fn short_text(value: &str, limit: usize) -> String {
    short_text_with_truncation(value, limit).0
}

pub(super) fn short_text_with_truncation(value: &str, limit: usize) -> (String, bool) {
    let mut chars = value.chars();
    let mut text = chars.by_ref().take(limit).collect::<String>();
    let truncated = chars.next().is_some();
    if truncated {
        text.push('…');
    }
    (text, truncated)
}

pub(super) fn format_command(program: &str, args: &[String]) -> String {
    if args.is_empty() {
        program.to_owned()
    } else {
        format!("{program} {}", args.join(" "))
    }
}

pub(super) fn adaptive_agent_budget(
    context: &SoftwareContext,
    query: &str,
    requested_scopes: &[String],
) -> usize {
    let mut budget = 1_400usize;
    let symbol_count = context.symbols.len();
    // Retrieval uncertainty must not buy a large context pack by itself. A
    // missing direct symbol gets a small exploration allowance; real breadth
    // signals below (Requirements, Design, graph truncation, risks and scopes)
    // are what earn more model context.
    if symbol_count == 0 {
        budget = budget.saturating_add(200);
    } else if symbol_count > 2 {
        budget = budget.saturating_add(300);
    }

    let requirement_count = context.coverage.requirements.len();
    if requirement_count > 1 {
        budget = budget.saturating_add(300);
    }
    if requirement_count > 3 {
        budget = budget.saturating_add(300);
    }
    if context.design_items.len() > 4 {
        budget = budget.saturating_add(300);
    }
    if context.graph_context.truncated {
        budget = budget.saturating_add(300);
    }
    if !context.known_risks.is_empty() {
        budget = budget.saturating_add(300);
    }
    if requested_scopes.len() > 1 {
        budget = budget.saturating_add(250);
    }
    let query_terms = query
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|term| term.chars().count() >= 2)
        .count();
    let query_chars = query
        .chars()
        .filter(|character| !character.is_whitespace())
        .count();
    // Whitespace-delimited term counts systematically underestimate CJK and
    // other compact natural-language requests. Character length is a bounded
    // language-agnostic fallback so a long instruction can buy enough context
    // without changing the 4k adaptive ceiling.
    if query_terms > 12 || query_chars > 80 {
        budget = budget.saturating_add(250);
    }

    budget.clamp(1_200, 4_000)
}

pub(super) fn refresh_readiness_capabilities(value: &mut Value) {
    // Fast operator contexts own an explicit non-edit workflow. Generic edit
    // readiness would turn launch/status/commit requests back into source work
    // after budget trimming, so only recompute ordinary coding contexts here.
    if value.get("intent").and_then(Value::as_str).is_none() {
        update_agent_readiness(value);
    }
    sync_readiness_capabilities(value);
}

pub(super) fn sync_readiness_capabilities(value: &mut Value) {
    let mut tools = capability::required_recovery_tools(value.get("execution"));
    tools.extend(
        value
            .pointer("/readiness/next_actions")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
    );
    if let Some(capabilities) = value.get_mut("capabilities") {
        crate::harness::prioritize_model_tools(capabilities, &tools);
    }
}

pub(super) fn finalize_agent_context(
    value: &mut Value,
    baseline_context_bytes: u64,
    budget: usize,
) -> Result<()> {
    // The ordered canonical tool names already encode the active tool set.
    // Drop their reconstructible action/count copies before budgeting so every
    // delivered pack avoids this repeated discovery cost, even below the cap.
    context_budget::compact_capability_manifest(value);
    // Remove only repo-map metadata exactly reconstructible from current
    // source already in the pack; keep row identity, kind, reason and relations.
    context_budget::deduplicate_source_backed_repo_map(value);
    for _ in 0..32 {
        let previous = (
            value["serialized_bytes"].clone(),
            value["estimated_tokens"].clone(),
            value["context_bytes_avoided"].clone(),
            value["context_reduction_percent"].clone(),
        );
        let bytes = serialized_json_bytes(value)? as u64;
        let avoided = baseline_context_bytes.saturating_sub(bytes);
        let reduction_percent = if baseline_context_bytes == 0 {
            0.0
        } else {
            ((avoided as f64 / baseline_context_bytes as f64) * 10_000.0).round() / 100.0
        };
        value["serialized_bytes"] = json!(bytes);
        value["estimated_tokens"] = json!(bytes.div_ceil(4));
        value["context_bytes_avoided"] = json!(avoided);
        value["context_reduction_percent"] = json!(reduction_percent);

        let current_bytes = serialized_json_bytes(value)?;
        let current_tokens = current_bytes.div_ceil(4);
        // The query is needed for explicit-target/readiness routing during
        // trimming, but is removed from the delivered pack below. Do not evict
        // source to pay for this temporary echo. Round the allowance DOWN so
        // removing it still proves the original serialized byte cap.
        let query_allowance = value
            .get("query")
            .map(|query| {
                serialized_json_bytes(query).map(|bytes| (bytes + "\"query\":".len() + 1) / 4)
            })
            .transpose()?
            .unwrap_or(0);
        let packing_budget = budget.saturating_add(query_allowance);
        if current_tokens > packing_budget {
            trim_agent_context_from_tokens(value, packing_budget, current_bytes)?;
            refresh_readiness_capabilities(value);
            continue;
        }

        // Readiness may be recomputed after trimming and still needs the
        // original query for routing. Remove the redundant tool-input echo only
        // after the pack is already within budget, then iterate once more so
        // byte/token telemetry describes the final model-visible result.
        if value
            .as_object_mut()
            .and_then(|object| object.remove("query"))
            .is_some()
        {
            continue;
        }

        let current = (
            value["serialized_bytes"].clone(),
            value["estimated_tokens"].clone(),
            value["context_bytes_avoided"].clone(),
            value["context_reduction_percent"].clone(),
        );
        if current == previous {
            return Ok(());
        }
    }
    for _ in 0..8 {
        let bytes = serialized_json_bytes(value)? as u64;
        let tokens = bytes.div_ceil(4);
        value["serialized_bytes"] = json!(bytes);
        value["estimated_tokens"] = json!(tokens);
        let actual_bytes = serialized_json_bytes(value)? as u64;
        if actual_bytes == bytes
            && value["estimated_tokens"].as_u64() == Some(actual_bytes.div_ceil(4))
        {
            break;
        }
    }
    let bytes = serialized_json_bytes(value)? as u64;
    value["serialized_bytes"] = json!(bytes);
    value["estimated_tokens"] = json!(bytes.div_ceil(4));
    let bytes = serialized_json_bytes(value)?;
    if bytes.div_ceil(4) > budget {
        bail!("agent context could not satisfy the {budget}-token budget after finalization");
    }
    Ok(())
}
