use super::*;

impl Workspace {
    pub(super) fn search_report_with_file_diversity(
        &self,
        request: &SearchRequest,
        file_diversity: bool,
        language_evidence: bool,
    ) -> Result<SearchReport> {
        if request.queries.is_empty() || request.queries.len() > MAX_SEARCH_QUERIES {
            bail!("queries must contain between 1 and {MAX_SEARCH_QUERIES} strings");
        }
        if request.queries.iter().map(String::len).sum::<usize>() > 32 * 1024 {
            bail!("combined search queries must not exceed 32768 bytes");
        }
        if request
            .queries
            .iter()
            .any(|q| q.is_empty() || q.len() > MAX_QUERY_BYTES)
        {
            bail!("queries must be non-empty and at most {MAX_QUERY_BYTES} bytes each");
        }
        if request.offset > 10_000 || request.max_results == 0 || request.max_results > 2_000 {
            bail!("offset must be <= 10000 and max_results must be between 1 and 2000");
        }
        if !matches!(
            request.output_mode.as_str(),
            "content" | "files_with_matches" | "count_matches"
        ) {
            bail!("output_mode must be content, files_with_matches, or count_matches");
        }
        let mut seen = HashSet::new();
        let queries = request
            .queries
            .iter()
            .filter(|q| seen.insert(q.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if request.mode == SearchMode::Auto && queries.len() != 1 {
            bail!(
                "auto mode requires one query; use exact, regex, or token modes for a query array"
            );
        }
        let primary = SearchPlan::compile(&queries, request.mode)?;
        let fallback = (request.mode == SearchMode::Auto
            && queries[0].split_whitespace().count() > 1)
            .then(|| SearchPlan::compile(&queries, SearchMode::TokensAll))
            .transpose()?;
        let start = self.existing_path(&request.path)?;
        let mut report = SearchReport {
            matches: Matches::default(),
            language_matches: BTreeMap::new(),
            queries: queries.clone(),
            mode: request.mode,
            requested_mode: request.mode,
            file_diversity,
            files_considered: 0,
            files_scanned: 0,
            bytes_read: 0,
            skipped_files: 0,
            failed_files: 0,
            scan_truncated: false,
            failures: Vec::new(),
        };
        let mut paths = Vec::new();
        let mut planned_bytes = 0u64;
        let honor_parent_ignores = start == self.root;
        for entry in repository_walk_builder(&start, honor_parent_ignores).build() {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    report.failed_files += 1;
                    continue;
                }
            };
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            if language_evidence && language_for_path(&entry.path().to_string_lossy()).is_none() {
                continue;
            }
            report.files_considered += 1;
            let size = match entry.metadata() {
                Ok(metadata) => metadata.len(),
                Err(_) => {
                    report.failed_files += 1;
                    continue;
                }
            };
            if size > MAX_READ_BYTES {
                report.skipped_files += 1;
                continue;
            }
            if paths.len() == MAX_SCAN_FILES || planned_bytes.saturating_add(size) > MAX_SCAN_BYTES
            {
                report.scan_truncated = true;
                break;
            }
            planned_bytes += size;
            paths.push(portable_relative_path(
                entry.path().strip_prefix(&self.root)?,
            ));
        }
        paths.sort();
        let capacity = request
            .offset
            .saturating_add(request.max_results)
            .saturating_add(1);
        let mut fallback_matches = Matches::default();
        let files_only_exact = request.mode == SearchMode::Exact
            && request.output_mode == "files_with_matches"
            && fallback.is_none();
        // Bounded batches retain at most eight raw documents at once. Exact
        // files-only discovery is I/O-shaped: keep it on the bounded I/O pool
        // instead of charging one interactive CPU-governor acquisition per file.
        // Content/regex/token modes retain the CPU-governed parsing lane.
        for batch in paths.chunks(8) {
            if report.bytes_read > MAX_SCAN_BYTES {
                report.scan_truncated = true;
                break;
            }
            let scan = |path: &String, account_cpu: bool| -> Result<ScannedFile> {
                let _cpu = account_cpu
                    .then(|| crate::resource::cpu_work(crate::resource::WorkClass::Interactive));
                let source = self.load_source(path)?;
                let mut found = LineMatches::default();
                let mut alternative = LineMatches::default();
                let want_primary = primary.file_may_match(&source.content);
                let want_fallback = fallback
                    .as_ref()
                    .is_some_and(|p| p.file_may_match(&source.content));
                if want_primary || want_fallback {
                    let comment_style = CommentStyle::for_path(path);
                    let mut comment_state = CommentState::default();
                    for (line, text) in source.content.lines().enumerate() {
                        let comment_only =
                            comment_only_line(comment_style, text, &mut comment_state);
                        if !request.include_comments && comment_only {
                            continue;
                        }
                        if want_primary {
                            found.add(line, primary.mask(text), capacity);
                        }
                        if want_fallback {
                            if let Some(plan) = &fallback {
                                alternative.add(line, plan.mask(text), capacity);
                            }
                        }
                    }
                }
                Ok(ScannedFile {
                    source,
                    primary: found,
                    fallback: alternative,
                })
            };
            let outcomes = if files_only_exact || language_evidence {
                crate::resource::parallel_io(batch, |path| scan(path, false))?
            } else {
                batch
                    .par_iter()
                    .map(|path| scan(path, true))
                    .collect::<Vec<_>>()
            };
            for (path, outcome) in batch.iter().zip(outcomes) {
                match outcome {
                    Ok(scanned) => {
                        report.files_scanned += 1;
                        report.bytes_read += scanned.source.content.len() as u64;
                        if language_evidence {
                            if let Some(language) = language_for_path(path) {
                                *report.language_matches.entry(language).or_default() |=
                                    scanned.primary.seen;
                            }
                            continue;
                        }
                        report.matches.merge(
                            &scanned.source,
                            scanned.primary,
                            request,
                            capacity,
                            file_diversity,
                        );
                        fallback_matches.merge(
                            &scanned.source,
                            scanned.fallback,
                            request,
                            capacity,
                            file_diversity,
                        );
                    }
                    Err(error) => {
                        report.failed_files += 1;
                        if report.failures.len() < 8 {
                            let (message, _) = redact_sensitive_text(&error.to_string());
                            report.failures.push(json!({"path":path,"error":message}));
                        }
                    }
                }
            }
        }
        if request.mode == SearchMode::Auto {
            report.mode = SearchMode::Exact;
            if report.matches.total == 0 && fallback.is_some() {
                report.mode = SearchMode::TokensAll;
                report.matches = fallback_matches;
            }
        }
        Ok(report)
    }
}
