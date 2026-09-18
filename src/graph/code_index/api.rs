use super::graph_build::{
    append_cross_file_call_edges, append_file_graph, definition_count, select_graph_definitions,
};
use super::symbols::remove_file_record;
use super::*;

static LANGUAGE_CONFIGS: OnceLock<Result<Arc<LanguageConfigs>, String>> = OnceLock::new();

fn build_language_configs() -> Result<Arc<LanguageConfigs>> {
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
    let rust_tags = format!(
        "{}\n{}\n{}",
        tree_sitter_rust::TAGS_QUERY,
        RUST_CALLS_QUERY,
        RUST_IMPORTS_QUERY
    );
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
            &rust_tags,
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
    Ok(Arc::new(configs))
}

#[derive(Clone, Copy, Default)]
struct SymbolResolutionCandidates {
    qualified_index: Option<usize>,
    qualified_count: usize,
    name_index: Option<usize>,
    name_count: usize,
}

impl SymbolResolutionCandidates {
    fn record_qualified(&mut self, index: usize) {
        self.qualified_count = self.qualified_count.saturating_add(1);
        self.qualified_index.get_or_insert(index);
    }

    fn record_name(&mut self, index: usize) {
        self.name_count = self.name_count.saturating_add(1);
        self.name_index.get_or_insert(index);
    }

    fn selected(self) -> Option<usize> {
        if self.qualified_count == 1 {
            self.qualified_index
        } else if self.name_count == 1 {
            self.name_index
        } else {
            None
        }
    }
}

fn symbol_resolution_at(record: &FileRecord, index: usize) -> SymbolResolution {
    let selected = &record.symbols[index];
    SymbolResolution {
        id: selected.id.clone(),
        name: selected.name.clone(),
        qualified_name: selected.qualified_name.clone(),
        kind: selected.kind.clone(),
        path: selected.path.clone(),
        start_line: selected.name_range.start_line,
        start_column: selected.name_range.start_column,
        revision: format!("sha256:{}", record.sha256),
    }
}

fn symbol_resolution(record: &FileRecord, requested: &str) -> Option<SymbolResolution> {
    let mut candidates = SymbolResolutionCandidates::default();
    for (index, symbol) in record.symbols.iter().enumerate() {
        if !symbol.is_definition {
            continue;
        }
        if symbol.qualified_name == requested {
            candidates.record_qualified(index);
        }
        if symbol.name == requested {
            candidates.record_name(index);
        }
    }
    candidates
        .selected()
        .map(|index| symbol_resolution_at(record, index))
}

impl CodeIndex {
    pub fn new() -> Result<Self> {
        let configs = LANGUAGE_CONFIGS
            .get_or_init(|| build_language_configs().map_err(|error| format!("{error:#}")));
        let configs = match configs {
            Ok(configs) => configs.clone(),
            Err(error) => bail!("cannot initialize Tree-sitter language configs: {error}"),
        };
        Ok(Self {
            configs,
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
            "ast_cache_files": crate::resource::limits().ast_file_limit().min(MAX_AST_CACHE_FILES),
            "ast_cache_bytes": crate::resource::limits().ast_byte_limit(),
            "indexed_file_limit": crate::resource::limits().indexed_file_limit(),
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
        let max_files = max_files.clamp(1, MAX_GRAPH_FILES);
        let (paths, scan_truncated) = workspace.source_files(path, max_files)?;
        let mut snapshot = self.software_graph_from_paths(
            workspace,
            paths,
            scan_truncated,
            max_symbols,
            &HashSet::new(),
            &HashSet::new(),
        )?;
        snapshot.workspace = workspace_id.into();
        snapshot.path = path.to_owned();
        Ok(snapshot)
    }

    pub(crate) fn software_graph_from_paths(
        &self,
        workspace: &Workspace,
        paths: Vec<String>,
        scan_truncated: bool,
        max_symbols: usize,
        priority_symbol_ids: &HashSet<String>,
        priority_symbol_names: &HashSet<String>,
    ) -> Result<SoftwareGraphSnapshot> {
        let max_symbols = max_symbols.clamp(1, MAX_GRAPH_SYMBOLS);
        let supported = paths
            .into_iter()
            .filter(|path| self.config_for_path(path).is_some())
            .collect::<Vec<_>>();
        let outcomes = supported
            .par_iter()
            .map(|file| self.ensure_indexed(workspace, file, false))
            .collect::<Vec<_>>();

        let mut graph = SoftwareGraph::default();
        let mut files_failed = 0usize;
        let mut failures = Vec::new();
        let mut graph_truncated = false;
        let mut indexed_records = Vec::new();

        for (path, outcome) in supported.iter().zip(outcomes) {
            match outcome {
                Ok(ensured) => indexed_records.push(ensured.record),
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
        let selected = select_graph_definitions(
            &indexed_records,
            max_symbols,
            priority_symbol_ids,
            priority_symbol_names,
        );
        let files_indexed = indexed_records.len();
        for (record, selection) in indexed_records.iter().zip(&selected) {
            let appended = append_file_graph(&mut graph, record, selection)?;
            if appended < definition_count(record) {
                graph_truncated = true;
            }
        }
        append_cross_file_call_edges(&mut graph, &indexed_records)?;
        graph.validate()?;
        let node_count = graph.nodes.len();
        let edge_count = graph.edges.len();
        Ok(SoftwareGraphSnapshot {
            workspace: String::new(),
            path: String::new(),
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
        Ok(symbol_resolution(&ensured.record, requested))
    }

    pub(crate) fn resolve_symbols(
        &self,
        workspace: &Workspace,
        path: &str,
        requested: &[String],
    ) -> Result<HashMap<String, Option<SymbolResolution>>> {
        let mut requested = requested
            .iter()
            .map(|symbol| symbol.trim())
            .filter(|symbol| !symbol.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        requested.sort();
        requested.dedup();
        if requested.is_empty() {
            return Ok(HashMap::new());
        }
        let ensured = self.ensure_indexed(workspace, path, false)?;
        let requested_index = requested
            .iter()
            .enumerate()
            .map(|(index, requested)| (requested.as_str(), index))
            .collect::<HashMap<_, _>>();
        let mut candidates = vec![SymbolResolutionCandidates::default(); requested.len()];
        for (symbol_index, symbol) in ensured.record.symbols.iter().enumerate() {
            if !symbol.is_definition {
                continue;
            }
            if let Some(&request_index) = requested_index.get(symbol.qualified_name.as_str()) {
                candidates[request_index].record_qualified(symbol_index);
            }
            if let Some(&request_index) = requested_index.get(symbol.name.as_str()) {
                candidates[request_index].record_name(symbol_index);
            }
        }
        drop(requested_index);
        Ok(requested
            .into_iter()
            .zip(candidates)
            .map(|(requested, candidates)| {
                let resolution = candidates
                    .selected()
                    .map(|index| symbol_resolution_at(&ensured.record, index));
                (requested, resolution)
            })
            .collect())
    }

    pub fn invalidate(&self, root: &Path, path: &str) {
        let key = FileKey::new(root, path.to_owned());
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if let Some(flight) = state.parsing.get(&key).and_then(Weak::upgrade) {
            flight.generation.fetch_add(1, Ordering::AcqRel);
        }
        remove_file_record(&mut state, &key);
        state.ast_cache.remove(&key);
    }

    pub fn invalidate_prefix(&self, root: &Path, path: &str) {
        let normalized = path.trim_end_matches('/');
        let prefix = format!("{normalized}/");
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        for (key, flight) in &state.parsing {
            if key.root == root && (key.path == normalized || key.path.starts_with(prefix.as_str()))
            {
                if let Some(flight) = flight.upgrade() {
                    flight.generation.fetch_add(1, Ordering::AcqRel);
                }
            }
        }
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
                left_score
                    .saturating_sub(1)
                    .cmp(&right_score.saturating_sub(1))
                    .then_with(|| left_query.cmp(right_query))
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

        // A broad early query can have enough equally strong matches to consume
        // the global result limit before a later exact query is represented.
        // When the caller gives us enough slots, reserve the best candidate for
        // every matched query first, then fill the remaining capacity by the
        // ordinary global ranking. This preserves multi-query recall without
        // increasing scan work or the model-visible result bound.
        let mut results = Vec::with_capacity(max_results.min(total_matches));
        let mut selected_ids = HashSet::new();
        if truncated && max_results >= queries.len() {
            let mut covered_queries = HashSet::new();
            for (query_index, _, symbol) in &matches {
                if covered_queries.insert(*query_index) && selected_ids.insert(symbol.id.clone()) {
                    results.push(symbol.clone());
                    if results.len() == max_results {
                        break;
                    }
                }
            }
        }
        for (_, _, symbol) in matches {
            if results.len() == max_results {
                break;
            }
            if selected_ids.insert(symbol.id.clone()) {
                results.push(symbol);
            }
        }
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

    /// Fast, bounded seed lookup for agent context when the caller already has
    /// a code-shaped exact identifier. This is intentionally not the public
    /// exhaustive symbol search: cached candidates are revalidated against the
    /// filesystem, and an empty result tells the caller to use the normal scan.
    pub(crate) fn cached_exact_symbols_many(
        &self,
        workspace: &Workspace,
        queries: &[String],
        kind: Option<&str>,
        max_results: usize,
    ) -> Result<Vec<Value>> {
        if queries.is_empty() {
            return Ok(Vec::new());
        }
        let max_results = max_results.clamp(1, MAX_SYMBOL_RESULTS);
        let candidate_paths = {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow!("code index state poisoned"))?;
            let mut paths = BTreeSet::new();
            for query in queries {
                let exact_key = query.to_ascii_lowercase();
                if let Some(files) = state.exact_symbol_files.get(&exact_key) {
                    paths.extend(
                        files
                            .iter()
                            .filter(|key| key.root == workspace.root())
                            .map(|key| key.path.clone()),
                    );
                }
            }
            paths.into_iter().collect::<Vec<_>>()
        };
        if candidate_paths.is_empty() {
            return Ok(Vec::new());
        }

        let mut matches = Vec::new();
        for path in candidate_paths {
            let ensured = match self.ensure_indexed(workspace, &path, false) {
                Ok(ensured) => ensured,
                Err(_) => return Ok(Vec::new()),
            };
            for symbol in ensured
                .record
                .symbols
                .iter()
                .filter(|symbol| symbol.is_definition)
            {
                if !kind.is_none_or(|kind| symbol.kind.eq_ignore_ascii_case(kind)) {
                    continue;
                }
                let Some(query_index) = queries.iter().position(|query| {
                    symbol.name.eq_ignore_ascii_case(query)
                        || symbol.qualified_name.eq_ignore_ascii_case(query)
                }) else {
                    continue;
                };
                matches.push((query_index, symbol.clone()));
            }
        }
        matches.sort_by(|(left_query, left), (right_query, right)| {
            left_query
                .cmp(right_query)
                .then_with(|| left.qualified_name.cmp(&right.qualified_name))
                .then_with(|| left.path.cmp(&right.path))
                .then_with(|| left.start_byte.cmp(&right.start_byte))
        });
        let mut seen = HashSet::new();
        Ok(matches
            .into_iter()
            .filter(|(_, symbol)| seen.insert(symbol.id.clone()))
            .take(max_results)
            .filter_map(|(_, symbol)| serde_json::to_value(symbol).ok())
            .collect())
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
}
