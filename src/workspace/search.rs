use super::*;
use regex::RegexSetBuilder;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const MAX_SCAN_FILES: usize = 50_000;
const MAX_SCAN_BYTES: u64 = 64 * 1024 * 1024;
const MAX_QUERY_BYTES: usize = 16 * 1024;
const MAX_RETAINED_BYTES: usize = 8 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SearchMode {
    Auto,
    Exact,
    Regex,
    TokensAll,
    TokensAny,
}

impl SearchMode {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "auto" => Ok(Self::Auto),
            "exact" => Ok(Self::Exact),
            "regex" => Ok(Self::Regex),
            "tokens_all" => Ok(Self::TokensAll),
            "tokens_any" => Ok(Self::TokensAny),
            _ => bail!("search mode must be one of auto, exact, regex, tokens_all, tokens_any"),
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Exact => "exact",
            Self::Regex => "regex",
            Self::TokensAll => "tokens_all",
            Self::TokensAny => "tokens_any",
        }
    }
}

pub(crate) struct SearchRequest {
    pub queries: Vec<String>,
    pub path: String,
    pub mode: SearchMode,
    pub context_lines: usize,
    pub max_results: usize,
    pub offset: usize,
    pub output_mode: String,
}

// The scanner stores one row per source line; query provenance is a bitset.
// Legacy Rust APIs expand rows at the boundary, not during file traversal.
enum SearchPlan<'a> {
    Exact(Vec<memchr::memmem::Finder<'a>>),
    Regex(regex::RegexSet, bool),
    Terms(Vec<Vec<memchr::memmem::Finder<'a>>>, bool),
}

impl<'a> SearchPlan<'a> {
    fn compile(queries: &'a [String], mode: SearchMode) -> Result<Self> {
        match mode {
            SearchMode::Exact | SearchMode::Auto => Ok(Self::Exact(
                queries
                    .iter()
                    .map(|q| memchr::memmem::Finder::new(q.as_bytes()))
                    .collect(),
            )),
            SearchMode::Regex => RegexSetBuilder::new(queries)
                .size_limit(4 * 1024 * 1024)
                .dfa_size_limit(2 * 1024 * 1024)
                .build()
                .map(|set| {
                    let file_prefilter = !queries.iter().any(|q| {
                        q.contains('^') || q.contains('$') || q.contains("\\A") || q.contains("\\z")
                    });
                    Self::Regex(set, file_prefilter)
                })
                .map_err(|error| anyhow!("invalid search regex: {error}")),
            SearchMode::TokensAll | SearchMode::TokensAny => {
                let sets = queries.iter().map(|q| {
                    let terms = q.split_whitespace().map(|term| memchr::memmem::Finder::new(term.as_bytes())).collect::<Vec<_>>();
                    if terms.is_empty() {
                        bail!("token search queries must contain at least one non-whitespace token");
                    }
                    Ok(terms)
                }).collect::<Result<Vec<_>>>()?;
                Ok(Self::Terms(sets, mode == SearchMode::TokensAll))
            }
        }
    }

    fn mask(&self, text: &str) -> u32 {
        let bytes = text.as_bytes();
        match self {
            Self::Exact(finders) => finders.iter().enumerate().fold(0, |mask, (i, f)| {
                mask | (u32::from(f.find(bytes).is_some()) << i)
            }),
            Self::Regex(set, _) => set
                .matches(text)
                .iter()
                .fold(0, |mask, i| mask | (1u32 << i)),
            Self::Terms(sets, all) => sets.iter().enumerate().fold(0, |mask, (i, terms)| {
                let found = if *all {
                    terms.iter().all(|f| f.find(bytes).is_some())
                } else {
                    terms.iter().any(|f| f.find(bytes).is_some())
                };
                mask | (u32::from(found) << i)
            }),
        }
    }

    fn file_may_match(&self, text: &str) -> bool {
        // A line regex is NOT a whole-file regex: ^/$ and \A/\z can match an
        // interior line even when they do not match the complete document.
        match self {
            Self::Regex(set, file_prefilter) => !file_prefilter || set.is_match(text),
            _ => self.mask(text) != 0,
        }
    }
}

#[derive(Default)]
struct LineMatches {
    rows: Vec<(usize, u32)>,
    counts: [usize; 32],
    total: usize,
    seen: u32,
}

impl LineMatches {
    fn add(&mut self, line: usize, mask: u32, capacity: usize) {
        if mask == 0 {
            return;
        }
        self.total += 1;
        let mut remaining = mask;
        while remaining != 0 {
            let index = remaining.trailing_zeros() as usize;
            self.counts[index] += 1;
            remaining &= remaining - 1;
        }
        // Keep a bounded prefix PLUS the first representative of rare patterns.
        if self.rows.len() < capacity || mask & !self.seen != 0 {
            self.rows.push((line, mask));
        }
        self.seen |= mask;
    }
}

struct ScannedFile {
    source: SourceDocument,
    primary: LineMatches,
    fallback: LineMatches,
}

#[derive(Default)]
struct Matches {
    rows: Vec<(u32, Value)>,
    files: Vec<Value>,
    counts: [usize; 32],
    total: usize,
    seen: u32,
    bytes: usize,
    storage_limited: bool,
}

impl Matches {
    fn merge(
        &mut self,
        source: &SourceDocument,
        found: LineMatches,
        request: &SearchRequest,
        capacity: usize,
    ) {
        self.total += found.total;
        for (i, count) in found.counts.iter().enumerate() {
            self.counts[i] += count;
        }
        if found.total == 0 {
            return;
        }
        self.files
            .push(json!({"path":source.path,"sha256":source.sha256,"count":found.total}));
        if request.output_mode != "content" {
            return;
        }
        let lines = source.content.lines().collect::<Vec<_>>();
        for (line, mask) in found.rows {
            let retain = self.rows.len() < capacity || mask & !self.seen != 0;
            self.seen |= mask;
            if !retain {
                continue;
            }
            let (text, redacted, text_truncated) = safe_excerpt(lines[line]);
            let mut row = json!({
                "path":source.path,"sha256":source.sha256,"line":line + 1,
                "text":text,"redacted":redacted,"text_truncated":text_truncated
            });
            if request.context_lines > 0 {
                row["context"] = search_context(&lines, line, request.context_lines.min(20));
            }
            let size = serde_json::to_vec(&row).map_or(MAX_RETAINED_BYTES, |bytes| bytes.len());
            if self.bytes.saturating_add(size) > MAX_RETAINED_BYTES {
                self.storage_limited = true;
                continue;
            }
            self.bytes += size;
            self.rows.push((mask, row));
        }
    }
}

pub(crate) struct SearchReport {
    matches: Matches,
    queries: Vec<String>,
    mode: SearchMode,
    requested_mode: SearchMode,
    files_considered: usize,
    files_scanned: usize,
    bytes_read: u64,
    skipped_files: usize,
    failed_files: usize,
    scan_truncated: bool,
    failures: Vec<Value>,
}

impl SearchReport {
    fn legacy_matches(self, include_query: bool, limit: usize) -> Vec<Value> {
        let mut result = Vec::new();
        for (mask, row) in self.matches.rows {
            for (i, query) in self.queries.iter().enumerate() {
                if mask & (1u32 << i) == 0 {
                    continue;
                }
                let mut value = row.clone();
                if include_query {
                    value["query"] = json!(query);
                }
                result.push(value);
                if !include_query {
                    break;
                }
            }
        }
        result.sort_by(|a, b| {
            a["path"]
                .as_str()
                .cmp(&b["path"].as_str())
                .then_with(|| a["line"].as_u64().cmp(&b["line"].as_u64()))
                .then_with(|| a["query"].as_str().cmp(&b["query"].as_str()))
        });
        result.truncate(limit);
        result
    }

    pub(crate) fn into_value(
        self,
        workspace: &str,
        request: &SearchRequest,
        grouped: bool,
    ) -> Value {
        let mut ordered = Vec::new();
        let mut represented = 0u32;
        let mut chosen = HashSet::new();
        // One representative per pattern first, then deterministic path/line order.
        for (i, (mask, _)) in self.matches.rows.iter().enumerate() {
            if mask & !represented != 0 {
                represented |= mask;
                chosen.insert(i);
                ordered.push(i);
            }
        }
        ordered.extend((0..self.matches.rows.len()).filter(|i| !chosen.contains(i)));
        let content = request.output_mode == "content";
        let ordered = if content {
            ordered
        } else {
            (0..self.matches.files.len()).collect()
        };
        let total = if content {
            self.matches.total
        } else {
            self.matches.files.len()
        };
        let mut page = Vec::new();
        let mut response_bytes = 0usize;
        for &position in ordered
            .iter()
            .skip(request.offset)
            .take(request.max_results)
        {
            let row = if content {
                let (mask, source_row) = &self.matches.rows[position];
                let mut row = source_row.clone();
                let queries = self
                    .queries
                    .iter()
                    .enumerate()
                    .filter_map(|(i, q)| (mask & (1u32 << i) != 0).then_some(q))
                    .collect::<Vec<_>>();
                row["queries"] = json!(queries);
                if queries.len() == 1 {
                    row["query"] = json!(queries[0]);
                }
                row
            } else {
                self.matches.files[position].clone()
            };
            let bytes = serde_json::to_vec(&row).map_or(MAX_RESPONSE_BYTES, |v| v.len());
            if response_bytes.saturating_add(bytes) > MAX_RESPONSE_BYTES {
                break;
            }
            response_bytes += bytes;
            page.push(row);
        }
        let count = page.len();
        let next = request.offset.saturating_add(count);
        let results_truncated = next < total;
        let coverage_complete =
            !self.scan_truncated && self.failed_files == 0 && self.skipped_files == 0;
        let query_counts = self
            .queries
            .iter()
            .enumerate()
            .map(|(i, q)| json!({"query":q,"matched_lines":self.matches.counts[i]}))
            .collect::<Vec<_>>();
        let mut value = json!({
            "workspace":workspace,"path":request.path,"provider":"workspace-search",
            "precision":"text","requested_mode":self.requested_mode.as_str(),"mode":self.mode.as_str(),
            "output_mode":request.output_mode,"matching_unit":"line","count":count,
            "total_matches":self.matches.total,"file_count":self.matches.files.len(),
            "pattern_count":self.queries.len(),"query_counts":query_counts,
            "order":if content {"pattern_coverage_then_path_line"} else {"path"},"offset":request.offset,
            "next_offset":if count > 0 && next < total && next <= 10_000 && !self.matches.storage_limited {Some(next)} else {None},
            "files_considered":self.files_considered,"files_scanned":self.files_scanned,
            "bytes_read":self.bytes_read,"traversals":1,"skipped_files":self.skipped_files,
            "failed_files":self.failed_files,"failures":self.failures,
            "scan_truncated":self.scan_truncated,"results_truncated":results_truncated,
            "storage_limited":self.matches.storage_limited,"coverage_complete":coverage_complete,
            "truncated":!coverage_complete || results_truncated,
            "snapshot_semantics":"each file SHA identifies the bytes scanned; reruns observe current files"
        });
        if grouped && content {
            value["files"] = grouped_matches(page);
        } else {
            value[if content { "matches" } else { "files" }] = json!(page);
        }
        if !coverage_complete
            || self.matches.storage_limited
            || (results_truncated && value["next_offset"].is_null())
        {
            value["guidance"] = json!("Partial coverage or retained-result budget reached: narrow path or patterns. Zero returned matches is not proof of absence.");
        }
        value
    }
}

impl Workspace {
    pub fn search(&self, query: &str, path: &str, max_results: usize) -> Result<Vec<Value>> {
        self.search_with_options(query, path, max_results, SearchMode::Exact, 0)
            .map(|(rows, _)| rows)
    }

    pub fn search_many(
        &self,
        queries: &[String],
        path: &str,
        max_results: usize,
    ) -> Result<Vec<Value>> {
        self.search_many_with_options(
            queries,
            path,
            max_results.clamp(1, 1000),
            SearchMode::Exact,
            0,
        )
    }

    pub(crate) fn search_with_options(
        &self,
        query: &str,
        path: &str,
        max_results: usize,
        mode: SearchMode,
        context_lines: usize,
    ) -> Result<(Vec<Value>, SearchMode)> {
        let request = SearchRequest {
            queries: vec![query.to_owned()],
            path: path.to_owned(),
            mode,
            context_lines,
            max_results: max_results.clamp(1, 500),
            offset: 0,
            output_mode: "content".into(),
        };
        let report = self.search_report(&request)?;
        let mode = report.mode;
        Ok((report.legacy_matches(false, request.max_results), mode))
    }

    pub(crate) fn search_many_with_options(
        &self,
        queries: &[String],
        path: &str,
        max_results: usize,
        mode: SearchMode,
        context_lines: usize,
    ) -> Result<Vec<Value>> {
        let request = SearchRequest {
            queries: queries.to_vec(),
            path: path.to_owned(),
            mode,
            context_lines,
            max_results: max_results.clamp(1, 2000),
            offset: 0,
            output_mode: "content".into(),
        };
        self.search_report(&request)
            .map(|report| report.legacy_matches(true, request.max_results))
    }

    pub(crate) fn search_report(&self, request: &SearchRequest) -> Result<SearchReport> {
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
            queries: queries.clone(),
            mode: request.mode,
            requested_mode: request.mode,
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
        for entry in WalkDir::new(start)
            .follow_links(false)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(visible_entry)
        {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    report.failed_files += 1;
                    continue;
                }
            };
            if !entry.file_type().is_file() {
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
        // Bounded batches retain at most eight raw documents at once. Auto exact
        // and fallback evidence is gathered from the SAME bytes in ONE traversal.
        for batch in paths.chunks(8) {
            if report.bytes_read > MAX_SCAN_BYTES {
                report.scan_truncated = true;
                break;
            }
            let outcomes = batch
                .par_iter()
                .map(|path| -> Result<ScannedFile> {
                    let _cpu = crate::resource::cpu_work(crate::resource::WorkClass::Interactive);
                    let source = self.load_source(path)?;
                    let mut found = LineMatches::default();
                    let mut alternative = LineMatches::default();
                    let want_primary = primary.file_may_match(&source.content);
                    let want_fallback = fallback
                        .as_ref()
                        .is_some_and(|p| p.file_may_match(&source.content));
                    if want_primary || want_fallback {
                        for (line, text) in source.content.lines().enumerate() {
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
                })
                .collect::<Vec<_>>();
            for (path, outcome) in batch.iter().zip(outcomes) {
                match outcome {
                    Ok(scanned) => {
                        report.files_scanned += 1;
                        report.bytes_read += scanned.source.content.len() as u64;
                        report
                            .matches
                            .merge(&scanned.source, scanned.primary, request, capacity);
                        fallback_matches.merge(
                            &scanned.source,
                            scanned.fallback,
                            request,
                            capacity,
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

fn safe_excerpt(text: &str) -> (String, bool, bool) {
    // Redact before clipping: cutting a credential prefix can hide it from the
    // redactor. The SHA always refers to original bytes, not this presentation.
    let (safe, redacted) = redact_sensitive_line(text);
    let truncated = safe.chars().nth(1000).is_some();
    (safe.chars().take(1000).collect(), redacted, truncated)
}

fn search_context(lines: &[&str], index: usize, radius: usize) -> Value {
    let start = index.saturating_sub(radius);
    let end = index
        .saturating_add(radius)
        .saturating_add(1)
        .min(lines.len());
    let context = lines[start..end].iter().enumerate().map(|(offset, line)| {
        let (text, redacted, truncated) = safe_excerpt(line);
        json!({"line":start+offset+1,"text":text,"redacted":redacted,"text_truncated":truncated})
    }).collect::<Vec<_>>();
    json!({"start_line":start+1,"end_line":end,"lines":context})
}

fn grouped_matches(rows: Vec<Value>) -> Value {
    let mut files = BTreeMap::<String, Vec<Value>>::new();
    for row in rows {
        if let Some(path) = row["path"].as_str() {
            files.entry(path.to_owned()).or_default().push(row);
        }
    }
    json!(files.into_iter().map(|(path, mut matches)| {
        let sha = matches[0]["sha256"].clone();
        let mut lines = BTreeMap::<u64, Value>::new();
        for row in &mut matches {
            if let Some(context) = row.as_object_mut().and_then(|map| map.remove("context")) {
                if let Some(items) = context["lines"].as_array() {
                    for item in items {
                        if let Some(line) = item["line"].as_u64() { lines.entry(line).or_insert_with(|| item.clone()); }
                    }
                }
            }
        }
        json!({"path":path,"sha256":sha,"count":matches.len(),"matches":matches,"context_lines":lines.into_values().collect::<Vec<_>>()})
    }).collect::<Vec<_>>())
}
