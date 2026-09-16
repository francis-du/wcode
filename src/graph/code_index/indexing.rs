use super::symbols::{
    assign_containers, contains_case_insensitive, inclusive_end_line, line_excerpt,
    matching_symbols, matching_symbols_many, node_range, normalize_symbol_kind, prune_ast_cache,
    prune_ast_cache_to, prune_file_cache, remove_file_record, semantic_extent, symbol_id,
    symbol_query_leaf, syntactic_container_hint,
};
use super::*;

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
        let could_match = queries
            .iter()
            .any(|query| contains_case_insensitive(&source.content, symbol_query_leaf(query)));
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
