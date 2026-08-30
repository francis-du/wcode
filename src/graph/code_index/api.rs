use super::graph_build::{append_cross_file_call_edges, append_file_graph, definition_count};
use super::symbols::remove_file_record;
use super::*;

impl CodeIndex {
    pub fn new() -> Result<Self> {
        // The TypeScript grammar intentionally ships a narrow tags query focused on
        // declarations unique to TypeScript. Merge the JavaScript query so ordinary
        // classes, methods, functions, arrow functions, and calls remain discoverable.
        let typescript_tags = format!(
            "{}\n{}",
            tree_sitter_javascript::TAGS_QUERY,
            tree_sitter_typescript::TAGS_QUERY
        );
        let c_tags = format!("{}\n{}", tree_sitter_c::TAGS_QUERY, C_CALLS_QUERY);
        let cpp_tags = format!("{}\n{}", tree_sitter_cpp::TAGS_QUERY, C_CALLS_QUERY);
        let ocaml_interface_tags = format!(
            "{}\n{}",
            tree_sitter_ocaml::TAGS_QUERY,
            OCAML_INTERFACE_TAGS_QUERY
        );
        let configs = [
            LanguageConfig::new(
                LanguageId::Bash,
                tree_sitter_bash::LANGUAGE.into(),
                BASH_TAGS_QUERY,
            )?,
            LanguageConfig::new(LanguageId::C, tree_sitter_c::LANGUAGE.into(), &c_tags)?,
            LanguageConfig::new(LanguageId::Cpp, tree_sitter_cpp::LANGUAGE.into(), &cpp_tags)?,
            LanguageConfig::new(
                LanguageId::CSharp,
                tree_sitter_c_sharp::LANGUAGE.into(),
                tree_sitter_c_sharp::TAGS_QUERY,
            )?,
            LanguageConfig::new(
                LanguageId::Css,
                tree_sitter_css::LANGUAGE.into(),
                CSS_TAGS_QUERY,
            )?,
            LanguageConfig::new(
                LanguageId::Dart,
                tree_sitter_dart::LANGUAGE.into(),
                tree_sitter_dart::TAGS_QUERY,
            )?,
            LanguageConfig::new(
                LanguageId::Elixir,
                tree_sitter_elixir::LANGUAGE.into(),
                tree_sitter_elixir::TAGS_QUERY,
            )?,
            LanguageConfig::new(
                LanguageId::Go,
                tree_sitter_go::LANGUAGE.into(),
                tree_sitter_go::TAGS_QUERY,
            )?,
            LanguageConfig::new(
                LanguageId::Html,
                tree_sitter_html::LANGUAGE.into(),
                HTML_TAGS_QUERY,
            )?,
            LanguageConfig::new(
                LanguageId::Java,
                tree_sitter_java::LANGUAGE.into(),
                tree_sitter_java::TAGS_QUERY,
            )?,
            LanguageConfig::new(
                LanguageId::JavaScript,
                tree_sitter_javascript::LANGUAGE.into(),
                tree_sitter_javascript::TAGS_QUERY,
            )?,
            LanguageConfig::new(
                LanguageId::Lua,
                tree_sitter_lua::LANGUAGE.into(),
                tree_sitter_lua::TAGS_QUERY,
            )?,
            LanguageConfig::new(
                LanguageId::Ocaml,
                tree_sitter_ocaml::LANGUAGE_OCAML.into(),
                tree_sitter_ocaml::TAGS_QUERY,
            )?,
            LanguageConfig::new(
                LanguageId::OcamlInterface,
                tree_sitter_ocaml::LANGUAGE_OCAML_INTERFACE.into(),
                &ocaml_interface_tags,
            )?,
            LanguageConfig::new(
                LanguageId::Php,
                tree_sitter_php::LANGUAGE_PHP.into(),
                tree_sitter_php::TAGS_QUERY,
            )?,
            LanguageConfig::new(
                LanguageId::Python,
                tree_sitter_python::LANGUAGE.into(),
                tree_sitter_python::TAGS_QUERY,
            )?,
            LanguageConfig::new(
                LanguageId::R,
                tree_sitter_r::LANGUAGE.into(),
                tree_sitter_r::TAGS_QUERY,
            )?,
            LanguageConfig::new(
                LanguageId::Ruby,
                tree_sitter_ruby::LANGUAGE.into(),
                tree_sitter_ruby::TAGS_QUERY,
            )?,
            LanguageConfig::new(
                LanguageId::Rust,
                tree_sitter_rust::LANGUAGE.into(),
                tree_sitter_rust::TAGS_QUERY,
            )?,
            LanguageConfig::new(
                LanguageId::Swift,
                tree_sitter_swift::LANGUAGE.into(),
                tree_sitter_swift::TAGS_QUERY,
            )?,
            LanguageConfig::new(
                LanguageId::TypeScript,
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                &typescript_tags,
            )?,
            LanguageConfig::new(
                LanguageId::Tsx,
                tree_sitter_typescript::LANGUAGE_TSX.into(),
                &typescript_tags,
            )?,
        ]
        .into_iter()
        .map(|config| (config.id, Arc::new(config)))
        .collect();

        Ok(Self {
            configs: Arc::new(configs),
            state: Arc::new(Mutex::new(IndexState::default())),
        })
    }

    pub fn capabilities(&self) -> Value {
        json!({
            "provider": "tree-sitter",
            "precision": "syntax",
            "languages": [
                "bash", "c", "cpp", "csharp", "css", "dart", "elixir", "go",
                "html", "java", "javascript", "lua", "ocaml", "ocaml-interface",
                "php", "python", "r", "ruby", "rust", "swift", "typescript", "tsx"
            ],
            "language_count": self.configs.len(),
            "tools": ["file_outline", "find_symbol", "symbol_context"],
            "lazy_hash_index": true,
            "in_memory_ast": true,
            "ast_cache_files": MAX_AST_CACHE_FILES,
            "max_scan_files": MAX_INDEX_SCAN_FILES,
            "semantic_types": false,
            "lsp_fallback": false,
        })
    }

    pub fn software_graph(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        path: &str,
        max_files: usize,
        max_symbols: usize,
    ) -> Result<SoftwareGraphSnapshot> {
        let workspace_id = workspace_id.into();
        let max_files = max_files.clamp(1, MAX_GRAPH_FILES);
        let max_symbols = max_symbols.clamp(1, MAX_GRAPH_SYMBOLS);
        let (paths, scan_truncated) = workspace.source_files(path, max_files)?;
        let supported = paths
            .into_iter()
            .filter(|path| self.config_for_path(path).is_some())
            .collect::<Vec<_>>();
        let outcomes = supported
            .par_iter()
            .map(|file| self.ensure_indexed(workspace, file, false))
            .collect::<Vec<_>>();

        let mut graph = SoftwareGraph::default();
        let mut files_indexed = 0usize;
        let mut files_failed = 0usize;
        let mut failures = Vec::new();
        let mut symbols_added = 0usize;
        let mut graph_truncated = false;
        let mut indexed_records = Vec::new();

        for (path, outcome) in supported.iter().zip(outcomes) {
            match outcome {
                Ok(ensured) => {
                    files_indexed = files_indexed.saturating_add(1);
                    let remaining = max_symbols.saturating_sub(symbols_added);
                    let appended = append_file_graph(&mut graph, &ensured.record, remaining)?;
                    symbols_added = symbols_added.saturating_add(appended);
                    if appended < definition_count(&ensured.record) {
                        graph_truncated = true;
                    }
                    indexed_records.push(ensured.record);
                }
                Err(error) => {
                    files_failed = files_failed.saturating_add(1);
                    if failures.len() < MAX_REPORTED_SCAN_ERRORS {
                        failures.push(GraphBuildFailure {
                            path: path.clone(),
                            error: error.to_string(),
                        });
                    }
                }
            }
        }
        append_cross_file_call_edges(&mut graph, &indexed_records)?;
        graph.validate()?;
        let node_count = graph.nodes.len();
        let edge_count = graph.edges.len();
        Ok(SoftwareGraphSnapshot {
            workspace: workspace_id,
            path: path.to_owned(),
            provider: "tree-sitter".to_owned(),
            precision: GraphPrecision::Syntax,
            files_considered: supported.len(),
            files_indexed,
            files_failed,
            scan_truncated,
            truncated: graph_truncated || files_failed > failures.len(),
            node_count,
            edge_count,
            failures,
            graph,
        })
    }

    pub(crate) fn resolve_symbol(
        &self,
        workspace: &Workspace,
        path: &str,
        requested: &str,
    ) -> Result<Option<SymbolResolution>> {
        let requested = requested.trim();
        if requested.is_empty() {
            return Ok(None);
        }
        let ensured = self.ensure_indexed(workspace, path, false)?;
        let definitions = ensured
            .record
            .symbols
            .iter()
            .filter(|symbol| symbol.is_definition)
            .collect::<Vec<_>>();
        let qualified = definitions
            .iter()
            .copied()
            .filter(|symbol| symbol.qualified_name == requested)
            .collect::<Vec<_>>();
        let selected = if qualified.len() == 1 {
            qualified.first().copied()
        } else {
            let by_name = definitions
                .iter()
                .copied()
                .filter(|symbol| symbol.name == requested)
                .collect::<Vec<_>>();
            (by_name.len() == 1).then(|| by_name[0])
        };
        Ok(selected.map(|symbol| SymbolResolution {
            id: symbol.id.clone(),
            name: symbol.name.clone(),
            qualified_name: symbol.qualified_name.clone(),
            kind: symbol.kind.clone(),
            path: symbol.path.clone(),
            start_line: symbol.name_range.start_line,
            start_column: symbol.name_range.start_column,
            revision: format!("sha256:{}", ensured.record.sha256),
        }))
    }

    pub fn invalidate(&self, root: &Path, path: &str) {
        let key = FileKey::new(root, path.to_owned());
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        remove_file_record(&mut state, &key);
        state.ast_cache.remove(&key);
    }

    pub fn invalidate_prefix(&self, root: &Path, path: &str) {
        let normalized = path.trim_end_matches('/');
        let prefix = format!("{normalized}/");
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let keys = state
            .files
            .keys()
            .filter(|key| {
                key.root == root
                    && (key.path == normalized || key.path.starts_with(prefix.as_str()))
            })
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            remove_file_record(&mut state, &key);
            state.ast_cache.remove(&key);
        }
    }

    pub fn file_outline(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        path: &str,
        max_symbols: usize,
    ) -> Result<Value> {
        let workspace_id = workspace_id.into();
        let max_symbols = max_symbols.clamp(1, MAX_OUTLINE_SYMBOLS);
        let ensured = self.ensure_indexed(workspace, path, true)?;
        let definitions = ensured
            .record
            .symbols
            .iter()
            .filter(|symbol| symbol.is_definition)
            .cloned()
            .collect::<Vec<_>>();
        let total_symbols = definitions.len();
        let truncated = total_symbols > max_symbols;
        let symbols = definitions
            .into_iter()
            .take(max_symbols)
            .collect::<Vec<_>>();
        let stats = self.stats_for_root(workspace.root());

        Ok(json!({
            "workspace": workspace_id,
            "path": ensured.record.path,
            "language": ensured.record.language.as_str(),
            "provider": "tree-sitter",
            "precision": "syntax",
            "symbol_cache_hit": ensured.symbol_cache_hit,
            "ast_cache_hit": ensured.ast_cache_hit,
            "parse_errors": ensured.record.parse_errors,
            "sha256": ensured.record.sha256,
            "source_bytes": ensured.record.source_bytes,
            "line_count": ensured.record.line_count,
            "symbol_count": symbols.len(),
            "total_symbols": total_symbols,
            "truncated": truncated,
            "symbols": symbols,
            "index": stats,
        }))
    }

    pub fn find_symbol(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        query: &str,
        path: &str,
        kind: Option<&str>,
        max_results: usize,
    ) -> Result<Value> {
        let workspace_id = workspace_id.into();
        let query = query.trim();
        if query.is_empty() {
            bail!("symbol query must not be empty");
        }
        let kind = kind.map(str::trim).filter(|value| !value.is_empty());
        let max_results = max_results.clamp(1, MAX_SYMBOL_RESULTS);
        let (paths, scan_truncated) = workspace.source_files(path, MAX_INDEX_SCAN_FILES)?;
        let supported = paths
            .into_iter()
            .filter(|path| self.config_for_path(path).is_some())
            .collect::<Vec<_>>();

        let outcomes = supported
            .par_iter()
            .map(|file| self.search_file(workspace, file, query, kind))
            .collect::<Vec<_>>();

        let mut matches = Vec::new();
        let mut cache_hits = 0usize;
        let mut files_parsed = 0usize;
        let mut failed_files = 0usize;
        let mut failures = Vec::new();
        for (path, outcome) in supported.iter().zip(outcomes) {
            match outcome {
                Ok(outcome) => {
                    cache_hits += usize::from(outcome.cache_hit);
                    files_parsed += usize::from(outcome.parsed);
                    matches.extend(outcome.matches);
                }
                Err(error) => {
                    failed_files = failed_files.saturating_add(1);
                    if failures.len() < MAX_REPORTED_SCAN_ERRORS {
                        failures.push(json!({"path": path, "error": error.to_string()}));
                    }
                }
            }
        }

        matches.sort_by(|(left_score, left), (right_score, right)| {
            left_score
                .cmp(right_score)
                .then_with(|| left.qualified_name.cmp(&right.qualified_name))
                .then_with(|| left.path.cmp(&right.path))
                .then_with(|| left.start_byte.cmp(&right.start_byte))
        });
        let mut seen = HashSet::new();
        matches.retain(|(_, symbol)| seen.insert(symbol.id.clone()));
        let total_matches = matches.len();
        let truncated = total_matches > max_results;
        let results = matches
            .into_iter()
            .take(max_results)
            .map(|(_, symbol)| symbol)
            .collect::<Vec<_>>();
        let stats = self.stats_for_root(workspace.root());

        Ok(json!({
            "workspace": workspace_id,
            "query": query,
            "path": path,
            "kind": kind,
            "provider": "tree-sitter",
            "precision": "syntax",
            "files_considered": supported.len(),
            "files_parsed": files_parsed,
            "file_cache_hits": cache_hits,
            "files_failed": failed_files,
            "failures_truncated": failed_files > failures.len(),
            "scan_truncated": scan_truncated,
            "result_count": results.len(),
            "total_matches": total_matches,
            "truncated": truncated,
            "failures": failures,
            "results": results,
            "index": stats,
        }))
    }

    pub(crate) fn find_symbols_many(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        queries: &[String],
        path: &str,
        kind: Option<&str>,
        max_results: usize,
    ) -> Result<Value> {
        let workspace_id = workspace_id.into();
        let mut queries = queries
            .iter()
            .map(|query| query.trim())
            .filter(|query| !query.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if queries.is_empty() {
            bail!("symbol queries must not be empty");
        }
        let mut seen_queries = HashSet::new();
        queries.retain(|query| seen_queries.insert(query.clone()));
        let kind = kind.map(str::trim).filter(|value| !value.is_empty());
        let max_results = max_results.clamp(1, MAX_SYMBOL_RESULTS);
        let (paths, scan_truncated) = workspace.source_files(path, MAX_INDEX_SCAN_FILES)?;
        let supported = paths
            .into_iter()
            .filter(|path| self.config_for_path(path).is_some())
            .collect::<Vec<_>>();
        let outcomes = supported
            .par_iter()
            .map(|file| self.search_file_many(workspace, file, &queries, kind))
            .collect::<Vec<_>>();

        let mut matches = Vec::new();
        let mut cache_hits = 0usize;
        let mut files_parsed = 0usize;
        let mut failed_files = 0usize;
        let mut failures = Vec::new();
        for (path, outcome) in supported.iter().zip(outcomes) {
            match outcome {
                Ok(outcome) => {
                    cache_hits += usize::from(outcome.cache_hit);
                    files_parsed += usize::from(outcome.parsed);
                    matches.extend(outcome.matches);
                }
                Err(error) => {
                    failed_files = failed_files.saturating_add(1);
                    if failures.len() < MAX_REPORTED_SCAN_ERRORS {
                        failures.push(json!({"path": path, "error": error.to_string()}));
                    }
                }
            }
        }
        matches.sort_by(
            |(left_query, left_score, left), (right_query, right_score, right)| {
                left_query
                    .cmp(right_query)
                    .then_with(|| left_score.cmp(right_score))
                    .then_with(|| left.qualified_name.cmp(&right.qualified_name))
                    .then_with(|| left.path.cmp(&right.path))
                    .then_with(|| left.start_byte.cmp(&right.start_byte))
            },
        );
        let mut seen = HashSet::new();
        matches.retain(|(_, _, symbol)| seen.insert(symbol.id.clone()));
        let total_matches = matches.len();
        let truncated = total_matches > max_results;
        let results = matches
            .into_iter()
            .take(max_results)
            .map(|(_, _, symbol)| symbol)
            .collect::<Vec<_>>();
        let stats = self.stats_for_root(workspace.root());
        Ok(json!({
            "workspace": workspace_id,
            "queries": queries,
            "query_count": queries.len(),
            "path": path,
            "kind": kind,
            "provider": "tree-sitter",
            "precision": "syntax",
            "files_considered": supported.len(),
            "files_parsed": files_parsed,
            "file_cache_hits": cache_hits,
            "files_failed": failed_files,
            "failures_truncated": failed_files > failures.len(),
            "scan_truncated": scan_truncated,
            "result_count": results.len(),
            "total_matches": total_matches,
            "truncated": truncated,
            "failures": failures,
            "results": results,
            "index": stats,
        }))
    }

    pub(crate) fn symbol_metadata(
        &self,
        workspace: &Workspace,
        graph_symbol_id: &str,
    ) -> Result<Value> {
        let symbol_id = graph_symbol_id
            .strip_prefix("symbol:")
            .unwrap_or(graph_symbol_id);
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
        .ok_or_else(|| anyhow!("unknown symbol_id; rebuild the repository map"))?;
        let ensured = self.ensure_indexed(workspace, &key.path, false)?;
        let symbol = ensured
            .record
            .symbols
            .iter()
            .find(|symbol| symbol.id == symbol_id)
            .ok_or_else(|| anyhow!("symbol changed since the repository map was built"))?;
        Ok(json!({
            "signature": symbol.signature,
            "signature_redacted": symbol.signature_redacted,
            "range": symbol.range,
            "language": symbol.language,
            "provider": symbol.provider,
            "precision": symbol.precision,
        }))
    }

    pub fn symbol_context(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        symbol_id: &str,
        max_body_lines: usize,
    ) -> Result<Value> {
        let workspace_id = workspace_id.into();
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

        let ensured = self.ensure_indexed(workspace, &key.path, true)?;
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

        let ast = self.ast_info(&key, &ensured.record.sha256);
        Ok(json!({
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
                "start_line": body.start_line,
                "end_line": body.end_line,
                "total_lines": body.total_lines,
                "content": body.content,
                "redacted": body.redacted,
                "truncated": body_truncated,
            },
            "syntax_calls": calls,
            "same_file_call_targets": local_definitions,
            "nested_symbols": nested_symbols,
            "ast": ast,
        }))
    }
}
