use super::*;

#[cfg(test)]
#[path = "../../../tests/unit/graph/context.rs"]
mod tests;

impl CodeIndex {
    pub fn symbol_context(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        symbol_id: &str,
        max_body_lines: usize,
    ) -> Result<Value> {
        self.symbol_context_with_related(
            workspace_id.into(),
            workspace,
            symbol_id,
            max_body_lines,
            true,
        )
    }

    // Agent Context consumes only the direct body and call names. Do not read
    // up to eight related bodies just to discard them during compaction.
    pub(crate) fn symbol_hot_context(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        symbol_id: &str,
        max_body_lines: usize,
    ) -> Result<Value> {
        self.symbol_context_with_related(
            workspace_id.to_owned(),
            workspace,
            symbol_id,
            max_body_lines,
            false,
        )
    }

    fn symbol_context_with_related(
        &self,
        workspace_id: String,
        workspace: &Workspace,
        symbol_id: &str,
        max_body_lines: usize,
        include_related: bool,
    ) -> Result<Value> {
        let key = {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow!("code index state poisoned"))?;
            state
                .symbol_files
                .get(symbol_id)
                .filter(|key| key.root == workspace.root())
                .cloned()
        }
        .ok_or_else(|| anyhow!("unknown symbol_id; call find_symbol or file_outline first"))?;
        for _ in 0..2 {
            if let Some(context) = self.symbol_context_snapshot(
                &workspace_id,
                workspace,
                &key,
                symbol_id,
                max_body_lines,
                include_related,
            )? {
                return Ok(context);
            }
        }
        bail!("source changed repeatedly while reading symbol context; retry after edits settle")
    }

    pub(crate) fn syntax_call_navigation_from_matches(
        &self,
        workspace: &Workspace,
        symbol: &SymbolResolution,
        matches: &[Value],
        max_results: usize,
    ) -> Result<Value> {
        let mut paths = vec![symbol.path.clone()];
        let mut seen = HashSet::from([symbol.path.clone()]);
        for path in matches
            .iter()
            .filter_map(|item| item.get("path").and_then(Value::as_str))
        {
            if seen.insert(path.to_owned()) {
                paths.push(path.to_owned());
            }
        }
        let priority_ids = HashSet::from([symbol.id.clone()]);
        let priority_names = HashSet::from([symbol.name.clone()]);
        let graph = self.software_graph_from_paths(
            workspace,
            paths,
            matches.len() >= max_results,
            MAX_GRAPH_SYMBOLS,
            &priority_ids,
            &priority_names,
        )?;
        Ok(self.syntax_call_navigation(&graph, &symbol.id, max_results))
    }

    pub(crate) fn syntax_call_navigation(
        &self,
        snapshot: &SoftwareGraphSnapshot,
        symbol_id: &str,
        max_results: usize,
    ) -> Value {
        let target = format!("symbol:{symbol_id}");
        let max_results = max_results.clamp(1, 100);
        let mut incoming = Vec::new();
        let mut outgoing = Vec::new();
        let mut truncated = false;
        for edge in snapshot
            .graph
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Calls)
        {
            let (bucket, related_id) = if edge.to == target {
                (&mut incoming, &edge.from)
            } else if edge.from == target {
                (&mut outgoing, &edge.to)
            } else {
                continue;
            };
            if bucket.len() >= max_results {
                truncated = true;
                continue;
            }
            let Some(node) = snapshot.graph.nodes.get(related_id) else {
                continue;
            };
            let Some(path) = node.attributes.get("path").and_then(Value::as_str) else {
                continue;
            };
            let range = node.attributes.get("range");
            bucket.push(json!({
                "path": path,
                "name": node.attributes.get("qualified_name").and_then(Value::as_str).unwrap_or(node.label.as_str()),
                "line": range.and_then(|range| range.get("start_line")).and_then(Value::as_u64).unwrap_or(1),
                "character": range.and_then(|range| range.get("start_column")).and_then(Value::as_u64).unwrap_or(1),
                "provider": edge.provenance.provider,
                "precision": "syntax",
            }));
        }
        json!({
            "provider": "tree-sitter-software-graph",
            "precision": "syntax",
            "incoming_calls": incoming,
            "outgoing_calls": outgoing,
            "scan_truncated": snapshot.scan_truncated,
            "graph_truncated": snapshot.truncated,
            "truncated": truncated || snapshot.scan_truncated || snapshot.truncated,
        })
    }

    fn symbol_context_snapshot(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        key: &FileKey,
        symbol_id: &str,
        max_body_lines: usize,
        include_related: bool,
    ) -> Result<Option<Value>> {
        // Direct-body reads need a fresh symbol record, not a reconstructed AST.
        let ensured = self.ensure_indexed(workspace, &key.path, include_related)?;
        let symbol = ensured
            .record
            .symbols
            .iter()
            .find(|symbol| symbol.id == symbol_id)
            .cloned()
            .ok_or_else(|| anyhow!("symbol changed since it was indexed; run find_symbol again"))?;
        let max_body_lines = max_body_lines.clamp(1, MAX_CONTEXT_BODY_LINES);
        let start_line = symbol.range.start_line;
        let requested_end = start_line
            .saturating_add(max_body_lines.saturating_sub(1))
            .min(symbol.body_end_line.max(start_line));
        let body = workspace.read_file(&symbol.path, start_line, Some(requested_end))?;
        if body.sha256 != ensured.record.sha256 {
            // Never attach old ranges/signatures/relations to a new source body.
            self.invalidate(workspace.root(), &key.path);
            return Ok(None);
        }
        let body_truncated = requested_end < symbol.body_end_line;
        let mut calls = ensured
            .record
            .symbols
            .iter()
            .filter(|candidate| {
                !candidate.is_definition
                    && candidate.kind == "call"
                    && candidate.start_byte >= symbol.start_byte
                    && candidate.end_byte <= symbol.end_byte
            })
            .cloned()
            .collect::<Vec<_>>();
        calls.sort_by(|left, right| {
            left.start_byte
                .cmp(&right.start_byte)
                .then_with(|| left.name.cmp(&right.name))
        });
        let mut seen_calls = HashSet::new();
        calls.retain(|call| seen_calls.insert((call.name.clone(), call.range.start_line)));
        calls.truncate(100);
        let mut context = json!({
            "workspace": workspace_id,
            "symbol": symbol,
            "provider": "tree-sitter",
            "precision": "syntax",
            "symbol_cache_hit": ensured.symbol_cache_hit,
            "ast_cache_hit": ensured.ast_cache_hit,
            "parse_errors": ensured.record.parse_errors,
            "sha256": body.sha256,
            "source_bytes": ensured.record.source_bytes,
            "body": {
                "start_line": body.start_line, "end_line": body.end_line,
                "total_lines": body.total_lines, "content": body.content,
                "redacted": body.redacted, "truncated": body_truncated,
            },
            "syntax_calls": calls,
        });
        if !include_related {
            return Ok(Some(context));
        }
        let mut nested_symbols = ensured
            .record
            .symbols
            .iter()
            .filter(|candidate| {
                candidate.is_definition
                    && candidate.id != symbol.id
                    && candidate.start_byte >= symbol.start_byte
                    && candidate.end_byte <= symbol.end_byte
            })
            .cloned()
            .collect::<Vec<_>>();
        nested_symbols.sort_by_key(|candidate| candidate.start_byte);
        nested_symbols.truncate(100);
        let call_names = calls
            .iter()
            .map(|call| call.name.as_str())
            .collect::<HashSet<_>>();
        let mut local_definitions = ensured
            .record
            .symbols
            .iter()
            .filter(|candidate| {
                candidate.is_definition && call_names.contains(candidate.name.as_str())
            })
            .cloned()
            .collect::<Vec<_>>();
        local_definitions.sort_by(|left, right| left.name.cmp(&right.name));
        local_definitions.dedup_by(|left, right| left.id == right.id);
        local_definitions.truncate(50);
        let related_body_lines = max_body_lines.min(60);
        let related = |candidate: &CodeSymbol, relation: &str| -> Option<Value> {
            let start = candidate.range.start_line;
            let end = start
                .saturating_add(related_body_lines.saturating_sub(1))
                .min(candidate.body_end_line.max(start));
            let view = workspace
                .read_file(&candidate.path, start, Some(end))
                .ok()?;
            (view.sha256 == ensured.record.sha256).then(|| json!({
                "relation": relation, "relation_precision": "syntax-name-match", "symbol": candidate,
                "body": {"start_line": view.start_line, "end_line": view.end_line,
                    "content": view.content, "redacted": view.redacted,
                    "truncated": end < candidate.body_end_line}
            }))
        };
        let mut related_context = local_definitions
            .iter()
            .take(4)
            .filter_map(|target| related(target, "callee"))
            .collect::<Vec<_>>();
        let mut seen_callers = HashSet::new();
        for call in ensured.record.symbols.iter().filter(|candidate| {
            !candidate.is_definition && candidate.kind == "call" && candidate.name == symbol.name
        }) {
            let caller = ensured
                .record
                .symbols
                .iter()
                .filter(|candidate| {
                    candidate.is_definition
                        && candidate.id != symbol.id
                        && candidate.start_byte <= call.start_byte
                        && candidate.end_byte >= call.end_byte
                })
                .min_by_key(|candidate| candidate.end_byte.saturating_sub(candidate.start_byte));
            if let Some(caller) = caller.filter(|caller| seen_callers.insert(caller.id.clone())) {
                if let Some(context) = related(caller, "caller") {
                    related_context.push(context);
                    if seen_callers.len() >= 4 {
                        break;
                    }
                }
            }
        }
        context["same_file_call_targets"] = json!(local_definitions);
        context["same_file_related_context"] = json!(related_context);
        context["nested_symbols"] = json!(nested_symbols);
        context["ast"] = self.ast_info(key, &ensured.record.sha256);
        Ok(Some(context))
    }
}
