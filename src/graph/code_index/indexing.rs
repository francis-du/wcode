use super::symbols::{
    assign_containers, contains_case_insensitive, inclusive_end_line, line_excerpt,
    matching_symbols, matching_symbols_many, node_range, normalize_symbol_kind, path_symbol_score,
    prune_ast_cache, prune_ast_cache_to, prune_file_cache, remove_file_record, semantic_extent,
    symbol_id, symbol_query_leaf, syntactic_container_hint,
};
use super::*;

fn compact_go_code(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn redacted_guard_excerpt(text: &str) -> String {
    let excerpt = text.chars().take(240).collect::<String>();
    redact_sensitive_text(excerpt.trim()).0
}

fn early_exit_guard(prefix: &str, condition: &str) -> bool {
    let Some(position) = prefix.rfind(condition) else {
        return false;
    };
    prefix[position + condition.len()..]
        .chars()
        .take(240)
        .collect::<String>()
        .contains("{return")
}

fn go_structural_analysis(node: Node<'_>, source: &str) -> Option<Value> {
    let raw = source.get(node.byte_range()).unwrap_or_default();
    if node.kind() == "index_expression" {
        let open = raw.rfind('[')?;
        let close = raw[open + 1..].find(']')? + open + 1;
        let base = raw[..open].trim();
        let index = raw[open + 1..close].trim().parse::<usize>().ok()?;
        if base.is_empty() {
            return None;
        }
        let required_len = index.saturating_add(1);
        let base_compact = compact_go_code(base);
        let len_expr = format!("len({base_compact})");
        let mut guarded = false;
        let mut saw_guard = false;
        let mut evidence = Vec::new();
        let mut function_start = 0usize;
        let mut ancestor = node.parent();
        while let Some(parent) = ancestor {
            if parent.kind() == "if_statement" {
                if let Some(condition) = parent.child_by_field_name("condition") {
                    let condition_text = source.get(condition.byte_range()).unwrap_or_default();
                    let compact = compact_go_code(condition_text);
                    if compact.contains(&len_expr) {
                        saw_guard = true;
                        if compact.contains(&format!("{len_expr}>{index}"))
                            || compact.contains(&format!("{len_expr}>={required_len}"))
                            || compact.contains(&format!("{required_len}<={len_expr}"))
                            || compact.contains(&format!("{index}<{len_expr}"))
                        {
                            guarded = true;
                        }
                        evidence.push(redacted_guard_excerpt(condition_text));
                    }
                }
            }
            if matches!(
                parent.kind(),
                "function_declaration" | "method_declaration" | "func_literal"
            ) {
                function_start = parent.start_byte();
                break;
            }
            ancestor = parent.parent();
        }
        let prefix = source
            .get(function_start..node.start_byte())
            .map(compact_go_code)
            .unwrap_or_default();
        if prefix.contains(&len_expr) {
            saw_guard = true;
            let safe_conditions = [
                format!("if{len_expr}<{required_len}"),
                format!("if{len_expr}<={index}"),
            ];
            if safe_conditions
                .iter()
                .any(|condition| early_exit_guard(&prefix, condition))
                || (required_len == 1 && early_exit_guard(&prefix, &format!("if{len_expr}==0")))
            {
                guarded = true;
            }
        }
        let guard_mismatch = saw_guard && !guarded;
        let mut patterns = Vec::new();
        if guard_mismatch {
            patterns.push("index_mismatch");
        }
        if !guarded && base.contains('.') {
            patterns.push("unguarded_subscript");
        }
        return Some(json!({
            "guarded": guarded,
            "guard_kind": "length",
            "guard_mismatch": guard_mismatch,
            "required_len": required_len,
            "guard_evidence": evidence,
            "bug_patterns": patterns,
            "pattern_propagation": {"node_kind":"index_expression","required_len":required_len}
        }));
    }
    if node.kind() == "unary_expression" && raw.trim_start().starts_with('*') {
        let operand = raw.trim().trim_start_matches('*').trim();
        if operand.is_empty() {
            return None;
        }
        let operand_compact = compact_go_code(operand);
        let mut guarded = false;
        let mut evidence = Vec::new();
        let mut function_start = 0usize;
        let mut ancestor = node.parent();
        while let Some(parent) = ancestor {
            if parent.kind() == "if_statement" {
                if let Some(condition) = parent.child_by_field_name("condition") {
                    let condition_text = source.get(condition.byte_range()).unwrap_or_default();
                    let compact = compact_go_code(condition_text);
                    if compact.contains(&format!("{operand_compact}!=nil")) {
                        guarded = true;
                        evidence.push(redacted_guard_excerpt(condition_text));
                    }
                }
            }
            if matches!(
                parent.kind(),
                "function_declaration" | "method_declaration" | "func_literal"
            ) {
                function_start = parent.start_byte();
                break;
            }
            ancestor = parent.parent();
        }
        let prefix = source
            .get(function_start..node.start_byte())
            .map(compact_go_code)
            .unwrap_or_default();
        if early_exit_guard(&prefix, &format!("if{operand_compact}==nil")) {
            guarded = true;
        }
        return Some(json!({
            "guarded": guarded,
            "guard_kind": "nil",
            "bug_patterns": if guarded { Vec::<&str>::new() } else { vec!["nil_deref"] },
            "guard_evidence": evidence
        }));
    }
    if node.kind() == "assignment_statement" {
        let compact = compact_go_code(raw);
        if compact.starts_with("_=") && raw.contains('(') {
            return Some(json!({
                "guarded": false,
                "bug_patterns": ["err_swallowed"],
                "pattern_confidence": "heuristic",
                "pattern_propagation": {"node_kind":"assignment_statement","pattern":"err_swallowed"}
            }));
        }
    }
    if node.kind() == "call_expression"
        && raw.contains(".Run(")
        && raw.contains("func(")
        && ![
            "assert.",
            "require.",
            ".Error(",
            ".Errorf(",
            ".Fatal(",
            ".Fatalf(",
            ".Fail(",
            ".FailNow(",
        ]
        .iter()
        .any(|marker| raw.contains(marker))
    {
        return Some(json!({
            "guarded": false,
            "bug_patterns": ["empty_test"],
            "pattern_confidence": "heuristic",
            "pattern_propagation": {"node_kind":"call_expression","pattern":"empty_test"}
        }));
    }
    None
}

impl CodeIndex {
    pub(super) fn search_file(
        &self,
        workspace: &Workspace,
        path: &str,
        query: &str,
        kind: Option<&str>,
    ) -> Result<FileSearchOutcome> {
        let config = self
            .config_for_path(path)
            .ok_or_else(|| anyhow!("unsupported source language: {path}"))?;
        let stamp = workspace.source_stamp(path)?;
        let key = FileKey::new(workspace.root(), path.to_owned());

        if let Some(record) = self.cached_record_if_fresh(workspace, &key, &stamp)? {
            return Ok(FileSearchOutcome {
                matches: matching_symbols(&record, query, kind),
                cache_hit: true,
                parsed: false,
            });
        }

        let flight = self.parse_flight(&key)?;
        let _flight = flight
            .gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let generation = flight.generation.load(Ordering::Acquire);
        // Another entry point may have filled the cache while we waited.
        let stamp = workspace.source_stamp(path)?;
        if let Some(record) = self.cached_record_if_fresh(workspace, &key, &stamp)? {
            return Ok(FileSearchOutcome {
                matches: matching_symbols(&record, query, kind),
                cache_hit: true,
                parsed: false,
            });
        }
        let _cpu = crate::resource::cpu_work(crate::resource::WorkClass::Interactive);
        let source = workspace.load_source_at_stamp(path, &stamp)?;
        let prefilter = symbol_query_leaf(query);
        if !contains_case_insensitive(&source.content, prefilter) {
            self.invalidate(workspace.root(), path);
            return Ok(FileSearchOutcome {
                matches: Vec::new(),
                cache_hit: false,
                parsed: false,
            });
        }

        let parsed = self.parse_source(workspace.root(), &config, source)?;
        let record = self.store_parsed_file(workspace, key, parsed, &flight, generation)?;
        Ok(FileSearchOutcome {
            matches: matching_symbols(&record, query, kind),
            cache_hit: false,
            parsed: true,
        })
    }

    pub(super) fn ensure_indexed(
        &self,
        workspace: &Workspace,
        path: &str,
        retain_ast: bool,
    ) -> Result<EnsureResult> {
        let config = self.config_for_path(path).ok_or_else(|| {
            anyhow!("unsupported source file; use file reads for unsupported languages")
        })?;
        let stamp = workspace.source_stamp(path)?;
        let key = FileKey::new(workspace.root(), path.to_owned());
        if let Some(result) = self.cached_index(workspace, &key, &stamp, retain_ast)? {
            return Ok(result);
        }
        // Wait without holding an index-state lock or a CPU permit. Different
        // files keep separate flights, including during AST reconstruction.
        let flight = self.parse_flight(&key)?;
        let _flight = flight
            .gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let generation = flight.generation.load(Ordering::Acquire);
        let stamp = workspace.source_stamp(path)?;
        if let Some(result) = self.cached_index(workspace, &key, &stamp, retain_ast)? {
            return Ok(result);
        }
        let symbol_cache_hit = self
            .cached_record_if_fresh(workspace, &key, &stamp)?
            .is_some();
        let _cpu = crate::resource::cpu_work(crate::resource::WorkClass::Interactive);
        let source = workspace.load_source_at_stamp(path, &stamp)?;
        let parsed = self.parse_source(workspace.root(), &config, source)?;
        let record = self.store_parsed_file(workspace, key, parsed, &flight, generation)?;
        Ok(EnsureResult {
            record,
            symbol_cache_hit,
            ast_cache_hit: false,
        })
    }

    fn cached_index(
        &self,
        workspace: &Workspace,
        key: &FileKey,
        stamp: &SourceStamp,
        retain_ast: bool,
    ) -> Result<Option<EnsureResult>> {
        let Some(record) = self.cached_record_if_fresh(workspace, key, stamp)? else {
            return Ok(None);
        };
        let ast_cache_hit = self.touch_ast(key, &record.sha256)?;
        Ok((!retain_ast || ast_cache_hit).then_some(EnsureResult {
            record,
            symbol_cache_hit: true,
            ast_cache_hit,
        }))
    }

    pub(super) fn search_file_many(
        &self,
        workspace: &Workspace,
        path: &str,
        queries: &[String],
        kind: Option<&str>,
    ) -> Result<FileMultiSearchOutcome> {
        let config = self
            .config_for_path(path)
            .ok_or_else(|| anyhow!("unsupported source language: {path}"))?;
        let stamp = workspace.source_stamp(path)?;
        let key = FileKey::new(workspace.root(), path.to_owned());
        if let Some(record) = self.cached_record_if_fresh(workspace, &key, &stamp)? {
            return Ok(FileMultiSearchOutcome {
                matches: matching_symbols_many(&record, queries, kind),
                cache_hit: true,
                parsed: false,
            });
        }

        let flight = self.parse_flight(&key)?;
        let _flight = flight
            .gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let generation = flight.generation.load(Ordering::Acquire);
        let stamp = workspace.source_stamp(path)?;
        if let Some(record) = self.cached_record_if_fresh(workspace, &key, &stamp)? {
            return Ok(FileMultiSearchOutcome {
                matches: matching_symbols_many(&record, queries, kind),
                cache_hit: true,
                parsed: false,
            });
        }
        let _cpu = crate::resource::cpu_work(crate::resource::WorkClass::Interactive);
        let source = workspace.load_source_at_stamp(path, &stamp)?;
        let could_match = queries.iter().any(|query| {
            contains_case_insensitive(&source.content, symbol_query_leaf(query))
                || path_symbol_score(path, query).is_some()
        });
        if !could_match {
            self.invalidate(workspace.root(), path);
            return Ok(FileMultiSearchOutcome {
                matches: Vec::new(),
                cache_hit: false,
                parsed: false,
            });
        }

        let parsed = self.parse_source(workspace.root(), &config, source)?;
        let record = self.store_parsed_file(workspace, key, parsed, &flight, generation)?;
        Ok(FileMultiSearchOutcome {
            matches: matching_symbols_many(&record, queries, kind),
            cache_hit: false,
            parsed: true,
        })
    }

    pub(super) fn parse_flight(&self, key: &FileKey) -> Result<Arc<ParseFlight>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("code index state poisoned"))?;
        if let Some(flight) = state.parsing.get(key).and_then(Weak::upgrade) {
            return Ok(flight);
        }
        state.parsing.retain(|_, flight| flight.strong_count() > 0);
        if state.parsing.len() >= MAX_PARSE_FLIGHTS {
            bail!("too many independent index builds; retry after current builds complete");
        }
        let flight = Arc::new(ParseFlight::default());
        state.parsing.insert(key.clone(), Arc::downgrade(&flight));
        Ok(flight)
    }

    pub(super) fn parse_source(
        &self,
        root: &Path,
        config: &LanguageConfig,
        source: SourceDocument,
    ) -> Result<ParsedFile> {
        let mut parser = Parser::new();
        parser
            .set_language(&config.language)
            .with_context(|| format!("failed to load {} parser", config.id.as_str()))?;
        let tree = parser
            .parse(source.content.as_bytes(), None)
            .ok_or_else(|| anyhow!("Tree-sitter parsing was cancelled"))?;
        let parse_errors = tree.root_node().has_error();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(
            &config.tags_query,
            tree.root_node(),
            source.content.as_bytes(),
        );
        let capture_names = config.tags_query.capture_names();
        let mut candidates = HashMap::<(usize, usize, bool), (usize, CodeSymbol)>::new();

        while let Some(query_match) = matches.next() {
            let mut name_node = None;
            let mut semantic_node = None;
            let mut semantic_capture = None;
            for capture in query_match.captures {
                let Some(capture_name) = capture_names.get(capture.index as usize) else {
                    continue;
                };
                if *capture_name == "name" {
                    name_node = Some(capture.node);
                } else if capture_name.starts_with("definition.")
                    || capture_name.starts_with("reference.")
                {
                    semantic_node = Some(capture.node);
                    semantic_capture = Some(*capture_name);
                }
            }

            let (Some(name_node), Some(tag_node), Some(capture_name)) =
                (name_node, semantic_node, semantic_capture)
            else {
                continue;
            };
            if name_node.has_error() {
                continue;
            }
            let is_definition = capture_name.starts_with("definition.");
            let raw_kind = capture_name
                .split_once('.')
                .map(|(_, kind)| kind)
                .unwrap_or("symbol");
            let kind = normalize_symbol_kind(raw_kind).to_owned();
            let name_bytes = name_node.byte_range();
            let Some(name) = source.content.get(name_bytes.clone()) else {
                continue;
            };
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            let extent_node = semantic_extent(config.id, tag_node, &kind, is_definition);
            let extent_bytes = extent_node.byte_range();
            let start_byte = extent_bytes.start.min(name_bytes.start);
            let end_byte = extent_bytes.end.max(name_bytes.end);
            let id = symbol_id(
                root,
                &source.path,
                name,
                &kind,
                is_definition,
                name_bytes.start,
                name_bytes.end,
            );
            let container_hint = syntactic_container_hint(config.id, tag_node, &source.content);
            let raw_signature = line_excerpt(&source.content, name_bytes.start, 240);
            let (signature, signature_redacted) = redact_sensitive_text(&raw_signature);
            let symbol = CodeSymbol {
                id,
                name: name.to_owned(),
                qualified_name: name.to_owned(),
                kind,
                language: config.id.as_str().to_owned(),
                path: source.path.clone(),
                range: node_range(extent_node),
                name_range: node_range(name_node),
                container: None,
                signature,
                signature_redacted,
                is_definition,
                provider: "tree-sitter",
                precision: "syntax",
                start_byte,
                end_byte,
                body_end_line: inclusive_end_line(
                    extent_node.start_position(),
                    extent_node.end_position(),
                ),
                container_hint,
            };
            let key = (name_bytes.start, name_bytes.end, is_definition);
            match candidates.get(&key) {
                Some((existing_pattern, _)) if *existing_pattern <= query_match.pattern_index => {}
                _ => {
                    candidates.insert(key, (query_match.pattern_index, symbol));
                }
            }
        }

        let mut symbols = candidates
            .into_values()
            .map(|(_, symbol)| symbol)
            .collect::<Vec<_>>();
        assign_containers(config.id, &mut symbols);
        symbols.sort_by(|left, right| {
            left.start_byte
                .cmp(&right.start_byte)
                .then_with(|| right.end_byte.cmp(&left.end_byte))
                .then_with(|| left.kind.cmp(&right.kind))
        });
        let source_bytes = source.content.len();
        let line_count = source.content.lines().count();
        let generated_source = crate::conventions::generated_source(&source.path, &source.content);

        Ok(ParsedFile {
            record: FileRecord {
                path: source.path,
                stamp: source.stamp,
                sha256: source.sha256,
                language: config.id,
                source_bytes,
                line_count,
                generated_source,
                parse_errors,
                symbols,
            },
            tree,
        })
    }

    pub(super) fn config_for_path(&self, path: &str) -> Option<Arc<LanguageConfig>> {
        let path = Path::new(path);
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();

        let id = match file_name.as_str() {
            ".bashrc" | ".bash_profile" | ".bash_login" | ".profile" | ".zshrc" | ".zprofile"
            | ".zshenv" | ".zlogin" => LanguageId::Bash,
            "gemfile" | "rakefile" | "guardfile" | "podfile" | "fastfile" | "appfile"
            | "deliverfile" | "brewfile" | "vagrantfile" => LanguageId::Ruby,
            _ => match extension.as_str() {
                "sh" | "bash" | "zsh" | "ksh" | "command" | "bats" => LanguageId::Bash,
                "c" | "h" => LanguageId::C,
                "cc" | "cpp" | "cxx" | "c++" | "hh" | "hpp" | "hxx" | "h++" | "ipp" | "tpp"
                | "inl" => LanguageId::Cpp,
                "cs" | "cake" => LanguageId::CSharp,
                "css" => LanguageId::Css,
                "dart" => LanguageId::Dart,
                "ex" | "exs" => LanguageId::Elixir,
                "go" => LanguageId::Go,
                "html" | "htm" | "xhtml" => LanguageId::Html,
                "java" => LanguageId::Java,
                "js" | "jsx" | "mjs" | "cjs" => LanguageId::JavaScript,
                "lua" => LanguageId::Lua,
                "ml" => LanguageId::Ocaml,
                "mli" => LanguageId::OcamlInterface,
                "php" | "php3" | "php4" | "php5" | "phtml" => LanguageId::Php,
                "py" | "pyi" => LanguageId::Python,
                "r" => LanguageId::R,
                "rb" | "rake" | "gemspec" | "ru" | "jbuilder" => LanguageId::Ruby,
                "rs" => LanguageId::Rust,
                "swift" => LanguageId::Swift,
                "ts" | "mts" | "cts" => LanguageId::TypeScript,
                "tsx" => LanguageId::Tsx,
                _ => return None,
            },
        };
        self.configs.get(&id).cloned()
    }

    pub(super) fn cached_record_if_fresh(
        &self,
        _workspace: &Workspace,
        key: &FileKey,
        stamp: &SourceStamp,
    ) -> Result<Option<Arc<FileRecord>>> {
        let record = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| anyhow!("code index state poisoned"))?;
            let record = state
                .files
                .get(key)
                .filter(|record| &record.stamp == stamp)
                .cloned();
            if record.is_some() {
                state.access_tick = state.access_tick.saturating_add(1);
                let tick = state.access_tick;
                state.file_access.insert(key.clone(), tick);
            }
            record
        };
        #[cfg(not(unix))]
        if let Some(record) = record.as_ref() {
            let source = _workspace.load_source_at_stamp(&key.path, stamp)?;
            if source.sha256 != record.sha256 {
                self.invalidate(_workspace.root(), &key.path);
                return Ok(None);
            }
        }
        Ok(record)
    }

    pub(super) fn store_parsed_file(
        &self,
        workspace: &Workspace,
        key: FileKey,
        parsed: ParsedFile,
        flight: &ParseFlight,
        generation: u64,
    ) -> Result<Arc<FileRecord>> {
        if workspace.source_stamp(&key.path)? != parsed.record.stamp {
            bail!("source changed during indexing; retry the request");
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("code index state poisoned"))?;
        // Invalidation and publication share the state lock: a late old build
        // cannot repopulate a record explicitly invalidated by a completed edit.
        if flight.generation.load(Ordering::Acquire) != generation {
            bail!("source invalidated during indexing; retry the request");
        }
        remove_file_record(&mut state, &key);
        let record = Arc::new(parsed.record);
        for symbol in &record.symbols {
            state.symbol_files.insert(symbol.id.clone(), key.clone());
            if symbol.is_definition {
                state
                    .exact_symbol_files
                    .entry(symbol.name.to_ascii_lowercase())
                    .or_default()
                    .insert(key.clone());
                state
                    .exact_symbol_files
                    .entry(symbol.qualified_name.to_ascii_lowercase())
                    .or_default()
                    .insert(key.clone());
            }
        }
        state.files.insert(key.clone(), record.clone());
        state.access_tick = state.access_tick.saturating_add(1);
        let tick = state.access_tick;
        state.file_access.insert(key.clone(), tick);
        state.ast_cache.insert(
            key,
            AstEntry {
                hash: record.sha256.clone(),
                source_bytes: record.source_bytes,
                tree: parsed.tree,
                last_used: tick,
            },
        );
        prune_ast_cache(&mut state);
        prune_file_cache(&mut state, crate::resource::limits().indexed_file_limit());
        Ok(record)
    }

    pub(super) fn touch_ast(&self, key: &FileKey, expected_hash: &str) -> Result<bool> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("code index state poisoned"))?;
        state.access_tick = state.access_tick.saturating_add(1);
        let tick = state.access_tick;
        let Some(ast) = state.ast_cache.get_mut(key) else {
            return Ok(false);
        };
        if ast.hash != expected_hash {
            state.ast_cache.remove(key);
            return Ok(false);
        }
        ast.last_used = tick;
        Ok(true)
    }

    pub(crate) fn search_ast_nodes(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        request: &SyntaxSearchRequest,
    ) -> Result<Value> {
        if request.node_kinds.is_empty() || request.node_kinds.len() > 32 {
            bail!("node_kinds must contain between 1 and 32 Tree-sitter node kinds");
        }
        let kinds = request
            .node_kinds
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        const SUPPORTED_BUG_PATTERNS: &[&str] = &[
            "nil_deref",
            "err_swallowed",
            "index_mismatch",
            "empty_test",
            "unguarded_subscript",
        ];
        if request
            .bug_patterns
            .iter()
            .any(|pattern| !SUPPORTED_BUG_PATTERNS.contains(&pattern.as_str()))
        {
            bail!("unsupported bug pattern; expected nil_deref, err_swallowed, index_mismatch, empty_test, or unguarded_subscript");
        }
        let requested_bug_patterns = request
            .bug_patterns
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let text_regex = request
            .text_regex
            .as_deref()
            .map(regex::Regex::new)
            .transpose()
            .map_err(|error| anyhow!("invalid syntax text_regex: {error}"))?;
        let max_files = request.max_files.clamp(1, 50_000);
        let max_results = request.max_results.clamp(1, 2_000);
        let (files, scan_truncated) = workspace.source_files(&request.path, max_files)?;
        let found = AtomicU64::new(0);
        let stopped = std::sync::atomic::AtomicBool::new(false);
        let failed_files = AtomicU64::new(0);
        let root = workspace.root();
        let mut matches = files
            .par_iter()
            .filter_map(|path| {
                self.config_for_path(path)?;
                if found.load(Ordering::Relaxed) >= max_results as u64 {
                    stopped.store(true, Ordering::Relaxed);
                    return None;
                }
                let ensured = self
                    .ensure_indexed(workspace, path, true)
                    .inspect_err(|_| {
                        failed_files.fetch_add(1, Ordering::Relaxed);
                    })
                    .ok()?;
                let source = workspace
                    .load_source(path)
                    .inspect_err(|_| {
                        failed_files.fetch_add(1, Ordering::Relaxed);
                    })
                    .ok()?;
                if source.sha256 != ensured.record.sha256 {
                    self.invalidate(root, path);
                    failed_files.fetch_add(1, Ordering::Relaxed);
                    return None;
                }
                let key = FileKey::new(root, path.clone());
                let tree = self
                    .state
                    .lock()
                    .inspect_err(|_| {
                        failed_files.fetch_add(1, Ordering::Relaxed);
                    })
                    .ok()?
                    .ast_cache
                    .get(&key)
                    .filter(|ast| ast.hash == source.sha256)
                    .map(|ast| ast.tree.clone());
                let Some(tree) = tree else {
                    failed_files.fetch_add(1, Ordering::Relaxed);
                    return None;
                };
                let mut local = Vec::new();
                let mut stack = vec![tree.root_node()];
                while let Some(node) = stack.pop() {
                    if found.load(Ordering::Relaxed) >= max_results as u64 {
                        stopped.store(true, Ordering::Relaxed);
                        break;
                    }
                    if node.is_named()
                        && kinds.contains(node.kind())
                        && (request.include_comments || node.kind() != "comment")
                    {
                        let raw = source.content.get(node.byte_range()).unwrap_or_default();
                        if text_regex.as_ref().is_none_or(|regex| regex.is_match(raw)) {
                            let analysis = (ensured.record.language == LanguageId::Go)
                                .then(|| go_structural_analysis(node, &source.content))
                                .flatten();
                            if !requested_bug_patterns.is_empty() {
                                let matches_requested_pattern = analysis
                                    .as_ref()
                                    .and_then(|value| value.get("bug_patterns"))
                                    .and_then(Value::as_array)
                                    .is_some_and(|patterns| {
                                        patterns.iter().any(|pattern| {
                                            pattern.as_str().is_some_and(|pattern| {
                                                requested_bug_patterns.contains(pattern)
                                            })
                                        })
                                    });
                                if !matches_requested_pattern {
                                    let mut cursor = node.walk();
                                    let children = node.children(&mut cursor).collect::<Vec<_>>();
                                    stack.extend(children.into_iter().rev());
                                    continue;
                                }
                            }
                            let slot = found.fetch_add(1, Ordering::Relaxed);
                            if slot < max_results as u64 {
                                let excerpt = raw.chars().take(500).collect::<String>();
                                let (text, redacted) = redact_sensitive_text(excerpt.trim());
                                let mut row = json!({
                                    "path": path,
                                    "sha256": source.sha256,
                                    "language": ensured.record.language.as_str(),
                                    "node_kind": node.kind(),
                                    "range": node_range(node),
                                    "text": text,
                                    "text_truncated": raw.chars().count() > 500,
                                    "redacted": redacted,
                                    "parse_errors": ensured.record.parse_errors,
                                });
                                if let Some(analysis) = analysis {
                                    if let Some(fields) = analysis.as_object() {
                                        for (key, value) in fields {
                                            row[key] = value.clone();
                                        }
                                    }
                                }
                                local.push(row);
                            }
                        }
                    }
                    let mut cursor = node.walk();
                    let children = node.children(&mut cursor).collect::<Vec<_>>();
                    stack.extend(children.into_iter().rev());
                }
                (!local.is_empty()).then_some(local)
            })
            .flatten()
            .collect::<Vec<_>>();
        matches.sort_unstable_by(|left, right| {
            left["path"]
                .as_str()
                .cmp(&right["path"].as_str())
                .then_with(|| {
                    left["range"]["start_line"]
                        .as_u64()
                        .cmp(&right["range"]["start_line"].as_u64())
                })
        });
        matches.truncate(max_results);
        Ok(json!({
            "workspace": workspace_id,
            "provider": "tree-sitter",
            "precision": "syntax",
            "path": request.path,
            "node_kinds": request.node_kinds,
            "text_regex": text_regex.as_ref().map(regex::Regex::as_str),
            "include_comments": request.include_comments,
            "bug_patterns": request.bug_patterns,
            "files_considered": files.len(),
            "count": matches.len(),
            "matches": matches,
            "files_failed": failed_files.load(Ordering::Relaxed),
            "scan_truncated": scan_truncated,
            "results_truncated": stopped.load(Ordering::Relaxed) || found.load(Ordering::Relaxed) > matches.len() as u64,
            "coverage_complete": !scan_truncated && !stopped.load(Ordering::Relaxed) && failed_files.load(Ordering::Relaxed) == 0 && found.load(Ordering::Relaxed) == matches.len() as u64,
            "truncated": scan_truncated || stopped.load(Ordering::Relaxed) || failed_files.load(Ordering::Relaxed) > 0 || found.load(Ordering::Relaxed) > matches.len() as u64,
        }))
    }

    pub(super) fn ast_info(&self, key: &FileKey, expected_hash: &str) -> Value {
        let Ok(state) = self.state.lock() else {
            return json!({"cached": false});
        };
        let Some(ast) = state
            .ast_cache
            .get(key)
            .filter(|ast| ast.hash == expected_hash)
        else {
            return json!({"cached": false});
        };
        json!({
            "cached": true,
            "root_kind": ast.tree.root_node().kind(),
            "source_bytes": ast.source_bytes,
            "has_error": ast.tree.root_node().has_error(),
        })
    }

    pub(crate) fn trim_memory(&self, aggressive: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.parsing.retain(|_, flight| flight.strong_count() > 0);
        if aggressive {
            for flight in state.parsing.values().filter_map(Weak::upgrade) {
                flight.generation.fetch_add(1, Ordering::AcqRel);
            }
            state.ast_cache.clear();
            state.files.clear();
            state.file_access.clear();
            state.symbol_files.clear();
            state.exact_symbol_files.clear();
        } else {
            let limits = crate::resource::limits();
            prune_ast_cache_to(
                &mut state,
                limits.ast_file_limit().min(MAX_AST_CACHE_FILES) / 2,
                limits.ast_byte_limit() / 2,
            );
            prune_file_cache(&mut state, limits.indexed_file_limit() / 2);
        }
    }

    pub(super) fn stats_for_root(&self, root: &Path) -> Value {
        let Ok(state) = self.state.lock() else {
            return json!({"error": "index state unavailable"});
        };
        let files = state
            .files
            .iter()
            .filter(|(key, _)| key.root == root)
            .collect::<Vec<_>>();
        let symbols = files
            .iter()
            .map(|(_, record)| record.symbols.len())
            .sum::<usize>();
        let ast_files = state
            .ast_cache
            .keys()
            .filter(|key| key.root == root)
            .count();
        let limits = crate::resource::limits();
        json!({
            "indexed_files": files.len(),
            "indexed_file_limit": limits.indexed_file_limit(),
            "symbols": symbols,
            "ast_cached_files": ast_files,
            "ast_cache_limit": limits.ast_file_limit().min(MAX_AST_CACHE_FILES),
            "ast_cache_byte_limit": limits.ast_byte_limit(),
        })
    }
}
