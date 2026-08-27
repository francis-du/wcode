use crate::graph::{
    EdgeKind, GraphBuildFailure, GraphEdge, GraphNode, GraphPrecision, GraphProvenance, NodeKind,
    SoftwareGraph, SoftwareGraphSnapshot,
};
use crate::workspace::{redact_sensitive_text, SourceDocument, SourceStamp, Workspace};
use anyhow::{anyhow, bail, Context, Result};
use rayon::prelude::*;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Parser, Point, Query, QueryCursor, Tree};

const MAX_AST_CACHE_FILES: usize = 128;
const MAX_INDEX_SCAN_FILES: usize = 50_000;
const MAX_OUTLINE_SYMBOLS: usize = 1_000;
const MAX_SYMBOL_RESULTS: usize = 200;
const MAX_CONTEXT_BODY_LINES: usize = 500;
const MAX_REPORTED_SCAN_ERRORS: usize = 8;
const MAX_GRAPH_FILES: usize = 5_000;
const MAX_GRAPH_SYMBOLS: usize = 5_000;

const BASH_TAGS_QUERY: &str = r#"
(function_definition
  name: (word) @name) @definition.function

(command
  name: (command_name
    (word) @name)) @reference.call
"#;

const C_CALLS_QUERY: &str = r#"
(call_expression
  function: (identifier) @name) @reference.call

(call_expression
  function: (field_expression
    field: (field_identifier) @name)) @reference.call
"#;

const CSS_TAGS_QUERY: &str = r#"
(rule_set
  (selectors) @name) @definition.selector

(keyframes_statement
  (keyframes_name) @name) @definition.keyframes

(
  (declaration
    (property_name) @name) @definition.variable
  (#match? @name "^--")
)
"#;

const HTML_TAGS_QUERY: &str = r#"
(
  (element
    (start_tag
      (attribute
        (attribute_name) @_attribute
        [
          (attribute_value) @name
          (quoted_attribute_value
            (attribute_value) @name)
        ]))) @definition.element
  (#eq? @_attribute "id")
)

(
  (self_closing_tag
    (attribute
      (attribute_name) @_attribute
      [
        (attribute_value) @name
        (quoted_attribute_value
          (attribute_value) @name)
      ])) @definition.element
  (#eq? @_attribute "id")
)

(
  (script_element
    (start_tag
      (attribute
        (attribute_name) @_attribute
        [
          (attribute_value) @name
          (quoted_attribute_value
            (attribute_value) @name)
        ]))) @definition.element
  (#eq? @_attribute "id")
)

(
  (style_element
    (start_tag
      (attribute
        (attribute_name) @_attribute
        [
          (attribute_value) @name
          (quoted_attribute_value
            (attribute_value) @name)
        ]))) @definition.element
  (#eq? @_attribute "id")
)

(
  (element
    (start_tag
      (tag_name) @name)) @definition.component
  (#match? @name "-")
)

(
  (self_closing_tag
    (tag_name) @name) @definition.component
  (#match? @name "-")
)
"#;

const OCAML_INTERFACE_TAGS_QUERY: &str = r#"
(value_specification
  (value_name) @name) @definition.variable
"#;

#[derive(Clone)]
pub struct CodeIndex {
    configs: Arc<HashMap<LanguageId, Arc<LanguageConfig>>>,
    state: Arc<Mutex<IndexState>>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum LanguageId {
    Bash,
    C,
    Cpp,
    CSharp,
    Css,
    Dart,
    Elixir,
    Go,
    Html,
    Java,
    JavaScript,
    Lua,
    Ocaml,
    OcamlInterface,
    Php,
    Python,
    R,
    Ruby,
    Rust,
    Swift,
    TypeScript,
    Tsx,
}

impl LanguageId {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::CSharp => "csharp",
            Self::Css => "css",
            Self::Dart => "dart",
            Self::Elixir => "elixir",
            Self::Go => "go",
            Self::Html => "html",
            Self::Java => "java",
            Self::JavaScript => "javascript",
            Self::Lua => "lua",
            Self::Ocaml => "ocaml",
            Self::OcamlInterface => "ocaml-interface",
            Self::Php => "php",
            Self::Python => "python",
            Self::R => "r",
            Self::Ruby => "ruby",
            Self::Rust => "rust",
            Self::Swift => "swift",
            Self::TypeScript => "typescript",
            Self::Tsx => "tsx",
        }
    }

    const fn qualifier_separator(self) -> &'static str {
        match self {
            Self::Rust | Self::Cpp | Self::Php | Self::Ruby => "::",
            Self::Bash
            | Self::C
            | Self::CSharp
            | Self::Css
            | Self::Dart
            | Self::Elixir
            | Self::Go
            | Self::Html
            | Self::Java
            | Self::JavaScript
            | Self::Lua
            | Self::Ocaml
            | Self::OcamlInterface
            | Self::Python
            | Self::R
            | Self::Swift
            | Self::TypeScript
            | Self::Tsx => ".",
        }
    }
}

struct LanguageConfig {
    id: LanguageId,
    language: Language,
    tags_query: Query,
}

impl LanguageConfig {
    fn new(id: LanguageId, language: Language, tags_query: &str) -> Result<Self> {
        let tags_query = Query::new(&language, tags_query)
            .with_context(|| format!("failed to compile {} Tree-sitter tags query", id.as_str()))?;
        Ok(Self {
            id,
            language,
            tags_query,
        })
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct FileKey {
    root: PathBuf,
    path: String,
}

impl FileKey {
    fn new(root: &Path, path: impl Into<String>) -> Self {
        Self {
            root: root.to_path_buf(),
            path: path.into(),
        }
    }
}

#[derive(Default)]
struct IndexState {
    files: HashMap<FileKey, Arc<FileRecord>>,
    symbol_files: HashMap<String, FileKey>,
    ast_cache: HashMap<FileKey, AstEntry>,
    access_tick: u64,
}

struct AstEntry {
    hash: String,
    source: Arc<str>,
    tree: Tree,
    last_used: u64,
}

struct FileRecord {
    path: String,
    stamp: SourceStamp,
    sha256: String,
    language: LanguageId,
    source_bytes: usize,
    line_count: usize,
    parse_errors: bool,
    symbols: Vec<CodeSymbol>,
}

#[derive(Clone, Debug, Serialize)]
struct SourceRange {
    start_line: usize,
    start_column: usize,
    end_line: usize,
    end_column: usize,
    end_exclusive: bool,
}

#[derive(Clone, Debug, Serialize)]
struct CodeSymbol {
    id: String,
    name: String,
    qualified_name: String,
    kind: String,
    language: String,
    path: String,
    range: SourceRange,
    name_range: SourceRange,
    container: Option<String>,
    signature: String,
    signature_redacted: bool,
    is_definition: bool,
    provider: &'static str,
    precision: &'static str,
    #[serde(skip)]
    start_byte: usize,
    #[serde(skip)]
    end_byte: usize,
    #[serde(skip)]
    body_end_line: usize,
    #[serde(skip)]
    container_hint: Option<String>,
}

struct ParsedFile {
    record: FileRecord,
    source: Arc<str>,
    tree: Tree,
}

struct EnsureResult {
    record: Arc<FileRecord>,
    symbol_cache_hit: bool,
    ast_cache_hit: bool,
}

struct FileSearchOutcome {
    matches: Vec<(u8, CodeSymbol)>,
    cache_hit: bool,
    parsed: bool,
}

struct FileMultiSearchOutcome {
    matches: Vec<(usize, u8, CodeSymbol)>,
    cache_hit: bool,
    parsed: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct SymbolResolution {
    pub id: String,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub path: String,
    pub revision: String,
}

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
        queries.dedup();
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

    fn search_file(
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

        if let Some(record) = self.cached_record_if_fresh(&key, &stamp)? {
            return Ok(FileSearchOutcome {
                matches: matching_symbols(&record, query, kind),
                cache_hit: true,
                parsed: false,
            });
        }

        let source = workspace.load_source(path)?;
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
        let record = self.store_parsed_file(key, parsed)?;
        Ok(FileSearchOutcome {
            matches: matching_symbols(&record, query, kind),
            cache_hit: false,
            parsed: true,
        })
    }

    fn ensure_indexed(
        &self,
        workspace: &Workspace,
        path: &str,
        retain_ast: bool,
    ) -> Result<EnsureResult> {
        let config = self.config_for_path(path).ok_or_else(|| {
            anyhow!(
                "unsupported source file; supported languages are Bash, C, C++, C#, CSS, Dart, Elixir, Go, HTML, Java, JavaScript, Lua, OCaml/OCaml Interface, PHP, Python, R, Ruby, Rust, Swift, and TypeScript/TSX"
            )
        })?;
        let stamp = workspace.source_stamp(path)?;
        let key = FileKey::new(workspace.root(), path.to_owned());
        if let Some(record) = self.cached_record_if_fresh(&key, &stamp)? {
            let ast_cache_hit = self.touch_ast(&key, &record.sha256)?;
            if !retain_ast || ast_cache_hit {
                return Ok(EnsureResult {
                    record,
                    symbol_cache_hit: true,
                    ast_cache_hit,
                });
            }

            let source = workspace.load_source(path)?;
            let parsed = self.parse_source(workspace.root(), &config, source)?;
            let record = self.store_parsed_file(key, parsed)?;
            return Ok(EnsureResult {
                record,
                symbol_cache_hit: true,
                ast_cache_hit: false,
            });
        }

        let source = workspace.load_source(path)?;
        let parsed = self.parse_source(workspace.root(), &config, source)?;
        let record = self.store_parsed_file(key, parsed)?;
        Ok(EnsureResult {
            record,
            symbol_cache_hit: false,
            ast_cache_hit: false,
        })
    }

    fn search_file_many(
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
        if let Some(record) = self.cached_record_if_fresh(&key, &stamp)? {
            return Ok(FileMultiSearchOutcome {
                matches: matching_symbols_many(&record, queries, kind),
                cache_hit: true,
                parsed: false,
            });
        }

        let source = workspace.load_source(path)?;
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
        let record = self.store_parsed_file(key, parsed)?;
        Ok(FileMultiSearchOutcome {
            matches: matching_symbols_many(&record, queries, kind),
            cache_hit: false,
            parsed: true,
        })
    }

    fn parse_source(
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
        let source_text: Arc<str> = Arc::from(source.content);

        Ok(ParsedFile {
            record: FileRecord {
                path: source.path,
                stamp: source.stamp,
                sha256: source.sha256,
                language: config.id,
                source_bytes,
                line_count,
                parse_errors,
                symbols,
            },
            source: source_text,
            tree,
        })
    }

    fn config_for_path(&self, path: &str) -> Option<Arc<LanguageConfig>> {
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
                "sh" | "bash" | "zsh" | "ksh" | "command" => LanguageId::Bash,
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

    fn cached_record_if_fresh(
        &self,
        key: &FileKey,
        stamp: &SourceStamp,
    ) -> Result<Option<Arc<FileRecord>>> {
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow!("code index state poisoned"))?;
        Ok(state
            .files
            .get(key)
            .filter(|record| &record.stamp == stamp)
            .cloned())
    }

    fn store_parsed_file(&self, key: FileKey, parsed: ParsedFile) -> Result<Arc<FileRecord>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("code index state poisoned"))?;
        remove_file_record(&mut state, &key);
        let record = Arc::new(parsed.record);
        for symbol in &record.symbols {
            state.symbol_files.insert(symbol.id.clone(), key.clone());
        }
        state.files.insert(key.clone(), record.clone());
        state.access_tick = state.access_tick.saturating_add(1);
        let tick = state.access_tick;
        state.ast_cache.insert(
            key,
            AstEntry {
                hash: record.sha256.clone(),
                source: parsed.source,
                tree: parsed.tree,
                last_used: tick,
            },
        );
        prune_ast_cache(&mut state);
        Ok(record)
    }

    fn touch_ast(&self, key: &FileKey, expected_hash: &str) -> Result<bool> {
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

    fn ast_info(&self, key: &FileKey, expected_hash: &str) -> Value {
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
            "source_bytes": ast.source.len(),
            "has_error": ast.tree.root_node().has_error(),
        })
    }

    fn stats_for_root(&self, root: &Path) -> Value {
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
        json!({
            "indexed_files": files.len(),
            "symbols": symbols,
            "ast_cached_files": ast_files,
            "ast_cache_limit": MAX_AST_CACHE_FILES,
        })
    }
}

fn append_file_graph(
    graph: &mut SoftwareGraph,
    record: &FileRecord,
    max_symbols: usize,
) -> Result<usize> {
    let provenance = graph_provenance(record);
    let file_id = format!("file:{}", record.path);
    let mut file_attributes = BTreeMap::new();
    file_attributes.insert("path".to_owned(), json!(record.path));
    file_attributes.insert("language".to_owned(), json!(record.language.as_str()));
    file_attributes.insert("sha256".to_owned(), json!(record.sha256));
    file_attributes.insert("source_bytes".to_owned(), json!(record.source_bytes));
    file_attributes.insert("line_count".to_owned(), json!(record.line_count));
    file_attributes.insert("parse_errors".to_owned(), json!(record.parse_errors));
    graph.add_node(GraphNode {
        id: file_id.clone(),
        kind: NodeKind::File,
        label: record.path.clone(),
        attributes: file_attributes,
        provenance: provenance.clone(),
    })?;

    let definitions = record
        .symbols
        .iter()
        .filter(|symbol| symbol.is_definition)
        .take(max_symbols)
        .collect::<Vec<_>>();
    let mut targets_by_name = HashMap::<&str, Vec<&CodeSymbol>>::new();
    for symbol in &definitions {
        targets_by_name
            .entry(symbol.name.as_str())
            .or_default()
            .push(symbol);
        let node_id = graph_symbol_id(symbol);
        let mut attributes = BTreeMap::new();
        attributes.insert("path".to_owned(), json!(symbol.path));
        attributes.insert("name".to_owned(), json!(symbol.name));
        attributes.insert("qualified_name".to_owned(), json!(symbol.qualified_name));
        attributes.insert("symbol_kind".to_owned(), json!(symbol.kind));
        attributes.insert("language".to_owned(), json!(symbol.language));
        attributes.insert("range".to_owned(), serde_json::to_value(&symbol.range)?);
        graph.add_node(GraphNode {
            id: node_id.clone(),
            kind: graph_node_kind(&symbol.kind),
            label: symbol.qualified_name.clone(),
            attributes,
            provenance: provenance.clone(),
        })?;
        graph.add_edge(GraphEdge {
            from: file_id.clone(),
            to: node_id,
            kind: EdgeKind::Defines,
            provenance: provenance.clone(),
        })?;
    }

    let included = definitions
        .iter()
        .map(|symbol| symbol.id.as_str())
        .collect::<HashSet<_>>();
    let mut call_edges = HashSet::new();
    for call in record
        .symbols
        .iter()
        .filter(|symbol| !symbol.is_definition && symbol.kind == "call")
    {
        let Some(caller) = definitions
            .iter()
            .copied()
            .filter(|symbol| {
                symbol.start_byte <= call.start_byte && symbol.end_byte >= call.end_byte
            })
            .min_by_key(|symbol| symbol.end_byte.saturating_sub(symbol.start_byte))
        else {
            continue;
        };
        let Some(targets) = targets_by_name.get(call.name.as_str()) else {
            continue;
        };
        if targets.len() != 1 {
            continue;
        }
        let target = targets[0];
        if caller.id == target.id
            || !included.contains(caller.id.as_str())
            || !included.contains(target.id.as_str())
            || !call_edges.insert((caller.id.as_str(), target.id.as_str()))
        {
            continue;
        }
        graph.add_edge(GraphEdge {
            from: graph_symbol_id(caller),
            to: graph_symbol_id(target),
            kind: EdgeKind::Calls,
            provenance: provenance.clone(),
        })?;
    }

    Ok(definitions.len())
}

fn append_cross_file_call_edges(
    graph: &mut SoftwareGraph,
    records: &[Arc<FileRecord>],
) -> Result<()> {
    let mut targets_by_name = HashMap::<&str, Vec<(&FileRecord, &CodeSymbol)>>::new();
    for record in records {
        for symbol in record.symbols.iter().filter(|symbol| symbol.is_definition) {
            if graph.nodes.contains_key(&graph_symbol_id(symbol)) {
                targets_by_name
                    .entry(symbol.name.as_str())
                    .or_default()
                    .push((record, symbol));
            }
        }
    }

    let mut existing = graph
        .edges
        .iter()
        .filter(|edge| edge.kind == EdgeKind::Calls)
        .map(|edge| (edge.from.clone(), edge.to.clone()))
        .collect::<HashSet<_>>();

    for record in records {
        let definitions = record
            .symbols
            .iter()
            .filter(|symbol| symbol.is_definition)
            .collect::<Vec<_>>();
        for call in record
            .symbols
            .iter()
            .filter(|symbol| !symbol.is_definition && symbol.kind == "call")
        {
            let Some(caller) = definitions
                .iter()
                .copied()
                .filter(|symbol| {
                    symbol.start_byte <= call.start_byte && symbol.end_byte >= call.end_byte
                })
                .min_by_key(|symbol| symbol.end_byte.saturating_sub(symbol.start_byte))
            else {
                continue;
            };
            let Some(targets) = targets_by_name.get(call.name.as_str()) else {
                continue;
            };
            if targets.len() != 1 {
                continue;
            }
            let (target_record, target) = targets[0];
            if record.path == target_record.path {
                continue;
            }
            let from = graph_symbol_id(caller);
            let to = graph_symbol_id(target);
            if from == to
                || !graph.nodes.contains_key(&from)
                || !graph.nodes.contains_key(&to)
                || !existing.insert((from.clone(), to.clone()))
            {
                continue;
            }
            graph.add_edge(GraphEdge {
                from,
                to,
                kind: EdgeKind::Calls,
                provenance: GraphProvenance {
                    provider: "tree-sitter/global-name-resolution".to_owned(),
                    precision: GraphPrecision::Syntax,
                    revision: format!(
                        "caller:{};target:{}",
                        &record.sha256[..record.sha256.len().min(64)],
                        &target_record.sha256[..target_record.sha256.len().min(64)]
                    ),
                },
            })?;
        }
    }
    Ok(())
}

fn graph_provenance(record: &FileRecord) -> GraphProvenance {
    GraphProvenance {
        provider: "tree-sitter".to_owned(),
        precision: GraphPrecision::Syntax,
        revision: format!("sha256:{}", record.sha256),
    }
}

fn graph_symbol_id(symbol: &CodeSymbol) -> String {
    format!("symbol:{}", symbol.id)
}

fn graph_node_kind(kind: &str) -> NodeKind {
    match kind {
        "function" | "method" => NodeKind::Function,
        "struct" => NodeKind::Struct,
        "trait" => NodeKind::Trait,
        "class" => NodeKind::Class,
        "interface" => NodeKind::Interface,
        _ => NodeKind::Symbol,
    }
}

fn definition_count(record: &FileRecord) -> usize {
    record
        .symbols
        .iter()
        .filter(|symbol| symbol.is_definition)
        .count()
}

fn remove_file_record(state: &mut IndexState, key: &FileKey) {
    if let Some(record) = state.files.remove(key) {
        for symbol in &record.symbols {
            if state.symbol_files.get(&symbol.id) == Some(key) {
                state.symbol_files.remove(&symbol.id);
            }
        }
    }
}

fn prune_ast_cache(state: &mut IndexState) {
    while state.ast_cache.len() > MAX_AST_CACHE_FILES {
        let Some(oldest) = state
            .ast_cache
            .iter()
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        state.ast_cache.remove(&oldest);
    }
}

fn matching_symbols_many(
    record: &FileRecord,
    queries: &[String],
    kind: Option<&str>,
) -> Vec<(usize, u8, CodeSymbol)> {
    queries
        .iter()
        .enumerate()
        .flat_map(|(query_index, query)| {
            matching_symbols(record, query, kind)
                .into_iter()
                .map(move |(score, symbol)| (query_index, score, symbol))
        })
        .collect()
}

fn matching_symbols(record: &FileRecord, query: &str, kind: Option<&str>) -> Vec<(u8, CodeSymbol)> {
    record
        .symbols
        .iter()
        .filter(|symbol| symbol.is_definition)
        .filter(|symbol| kind.is_none_or(|kind| symbol.kind.eq_ignore_ascii_case(kind)))
        .filter_map(|symbol| symbol_score(symbol, query).map(|score| (score, symbol.clone())))
        .collect()
}

fn symbol_score(symbol: &CodeSymbol, query: &str) -> Option<u8> {
    if symbol.name == query || symbol.qualified_name == query {
        return Some(0);
    }
    let query = query.to_lowercase();
    let name = symbol.name.to_lowercase();
    let qualified = symbol.qualified_name.to_lowercase();
    if name == query || qualified == query {
        Some(1)
    } else if name.starts_with(&query) || qualified.starts_with(&query) {
        Some(2)
    } else if name.contains(&query) || qualified.contains(&query) {
        Some(3)
    } else {
        None
    }
}

fn normalize_symbol_kind(kind: &str) -> &str {
    match kind {
        "send" => "call",
        _ => kind,
    }
}

fn semantic_extent<'tree>(
    language: LanguageId,
    node: Node<'tree>,
    kind: &str,
    is_definition: bool,
) -> Node<'tree> {
    if !is_definition
        || !matches!(language, LanguageId::C | LanguageId::Cpp)
        || !matches!(kind, "function" | "method")
    {
        return node;
    }

    let mut current = Some(node);
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "function_definition" | "declaration" | "field_declaration"
        ) {
            return candidate;
        }
        if candidate.kind() == "translation_unit" {
            break;
        }
        current = candidate.parent();
    }
    node
}

fn symbol_query_leaf(query: &str) -> &str {
    query
        .rsplit([':', '.', '#', '/', '\\'])
        .find(|segment| !segment.is_empty())
        .unwrap_or(query)
}

fn contains_case_insensitive(content: &str, query: &str) -> bool {
    if content.contains(query) {
        return true;
    }
    content.to_lowercase().contains(&query.to_lowercase())
}

fn assign_containers(language: LanguageId, symbols: &mut [CodeSymbol]) {
    let mut definitions = symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| symbol.is_definition)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    definitions.sort_by(|left, right| {
        symbols[*left]
            .start_byte
            .cmp(&symbols[*right].start_byte)
            .then_with(|| symbols[*right].end_byte.cmp(&symbols[*left].end_byte))
    });

    let mut stack = Vec::<usize>::new();
    for index in definitions {
        while stack
            .last()
            .is_some_and(|parent| symbols[*parent].end_byte <= symbols[index].start_byte)
        {
            stack.pop();
        }
        let nested_parent = stack.last().copied().filter(|parent| {
            symbols[*parent].start_byte <= symbols[index].start_byte
                && symbols[*parent].end_byte >= symbols[index].end_byte
                && (symbols[*parent].start_byte != symbols[index].start_byte
                    || symbols[*parent].end_byte != symbols[index].end_byte)
        });
        let container = nested_parent
            .map(|parent| symbols[parent].qualified_name.clone())
            .or_else(|| symbols[index].container_hint.clone());
        if let Some(container) = container {
            symbols[index].container = Some(container.clone());
            symbols[index].qualified_name = format!(
                "{container}{}{}",
                language.qualifier_separator(),
                symbols[index].name
            );
        }
        stack.push(index);
    }
}

fn syntactic_container_hint(language: LanguageId, node: Node<'_>, source: &str) -> Option<String> {
    if language != LanguageId::Rust {
        return None;
    }
    let mut ancestor = node.parent();
    while let Some(parent) = ancestor {
        if parent.kind() == "impl_item" {
            return parent
                .child_by_field_name("type")
                .and_then(|node| source.get(node.byte_range()))
                .map(collapse_whitespace)
                .filter(|value| !value.is_empty());
        }
        ancestor = parent.parent();
    }
    None
}

fn node_range(node: Node<'_>) -> SourceRange {
    let start = node.start_position();
    let end = node.end_position();
    SourceRange {
        start_line: start.row + 1,
        start_column: start.column + 1,
        end_line: end.row + 1,
        end_column: end.column + 1,
        end_exclusive: true,
    }
}

fn inclusive_end_line(start: Point, end: Point) -> usize {
    if end.column == 0 && end.row > start.row {
        end.row.max(1)
    } else {
        end.row + 1
    }
}

fn line_excerpt(source: &str, byte: usize, max_chars: usize) -> String {
    let bytes = source.as_bytes();
    let byte = byte.min(bytes.len());
    let mut start = byte;
    while start > 0 && bytes[start - 1] != b'\n' {
        start -= 1;
    }
    let mut end = byte;
    while end < bytes.len() && bytes[end] != b'\n' {
        end += 1;
    }
    let line = source.get(start..end).unwrap_or_default();
    truncate_chars(&collapse_whitespace(line), max_chars)
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let mut output = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    output.push('…');
    output
}

fn symbol_id(
    root: &Path,
    path: &str,
    name: &str,
    kind: &str,
    is_definition: bool,
    start: usize,
    end: usize,
) -> String {
    let mut digest = Sha256::new();
    digest.update(root.to_string_lossy().as_bytes());
    digest.update([0]);
    digest.update(path.as_bytes());
    digest.update([0]);
    digest.update(name.as_bytes());
    digest.update([0]);
    digest.update(kind.as_bytes());
    digest.update([u8::from(is_definition)]);
    digest.update(start.to_le_bytes());
    digest.update(end.to_le_bytes());
    let encoded = format!("{:x}", digest.finalize());
    format!("ts:{}", &encoded[..24])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn rust_outline_keeps_ast_and_qualifies_impl_methods() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("service.rs"),
            "pub struct Service;\n\nimpl Service {\n    pub fn run(&self) { helper(); }\n}\n\nfn helper() {}\n",
        )
        .unwrap();
        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let index = CodeIndex::new().unwrap();
        let outline = index
            .file_outline("demo", &workspace, "service.rs", 100)
            .unwrap();
        let symbols = outline["symbols"].as_array().unwrap();
        assert!(symbols.iter().any(|symbol| symbol["name"] == "Service"));
        assert!(symbols
            .iter()
            .any(|symbol| symbol["qualified_name"] == "Service::run"));
        assert_eq!(outline["index"]["ast_cached_files"], 1);

        let second = index
            .file_outline("demo", &workspace, "service.rs", 100)
            .unwrap();
        assert_eq!(second["symbol_cache_hit"], true);
        assert_eq!(second["ast_cache_hit"], true);
    }

    #[test]
    fn symbol_search_supports_multiple_languages_and_context() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("worker.py"),
            "class Worker:\n    def execute(self):\n        return helper()\n\ndef helper():\n    return 1\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("worker.ts"),
            "export class TsWorker { execute(): number { return 1; } }\n",
        )
        .unwrap();
        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let index = CodeIndex::new().unwrap();
        let search = index
            .find_symbol("demo", &workspace, "execute", ".", None, 20)
            .unwrap();
        assert_eq!(search["result_count"], 2);
        let python = search["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|symbol| symbol["language"] == "python")
            .unwrap();
        let symbol_id = python["id"].as_str().unwrap();
        let context = index
            .symbol_context("demo", &workspace, symbol_id, 50)
            .unwrap();
        assert!(context["body"]["content"]
            .as_str()
            .unwrap()
            .contains("def execute"));
        assert!(context["syntax_calls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|call| call["name"] == "helper"));
    }

    #[test]
    fn multi_query_symbol_search_scans_and_parses_each_file_once() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("service.rs"),
            "pub fn alpha_service() {}\npub fn beta_helper() {}\n",
        )
        .unwrap();
        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let index = CodeIndex::new().unwrap();
        let queries = vec!["alpha_service".to_owned(), "beta_helper".to_owned()];

        let first = index
            .find_symbols_many("demo", &workspace, &queries, ".", None, 10)
            .unwrap();
        assert_eq!(first["query_count"], 2);
        assert_eq!(first["files_considered"], 1);
        assert_eq!(first["files_parsed"], 1);
        assert_eq!(first["result_count"], 2);
        assert_eq!(first["results"][0]["name"], "alpha_service");
        assert_eq!(first["results"][1]["name"], "beta_helper");

        let cached = index
            .find_symbols_many("demo", &workspace, &queries, ".", None, 10)
            .unwrap();
        assert_eq!(cached["files_parsed"], 0);
        assert_eq!(cached["file_cache_hits"], 1);
        assert_eq!(cached["result_count"], 2);
    }

    #[test]
    fn common_language_grammars_produce_real_symbols() {
        let dir = tempfile::tempdir().unwrap();
        let fixtures = [
            (
                "build.sh",
                "build_project() { echo ready; }\nbuild_project\n",
                "bash",
                "build_project",
            ),
            (
                "engine.c",
                "int compute_c(int value) { return value + 1; }\n",
                "c",
                "compute_c",
            ),
            (
                "engine.cpp",
                "int compute_cpp(int value) { return value + 1; }\n",
                "cpp",
                "compute_cpp",
            ),
            (
                "Worker.cs",
                "class Worker { public int Execute() { return 1; } }\n",
                "csharp",
                "Execute",
            ),
            (
                "Worker.java",
                "class Worker { int execute() { return 1; } }\n",
                "java",
                "execute",
            ),
            (
                "worker.php",
                "<?php class Worker { public function execute() { return 1; } }\n",
                "php",
                "execute",
            ),
            (
                "worker.rb",
                "class Worker\n  def execute\n    1\n  end\nend\n",
                "ruby",
                "execute",
            ),
        ];
        for (path, source, _, _) in fixtures {
            fs::write(dir.path().join(path), source).unwrap();
        }

        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let index = CodeIndex::new().unwrap();
        let capabilities = index.capabilities();
        assert_eq!(
            capabilities["language_count"].as_u64(),
            capabilities["languages"]
                .as_array()
                .map(|languages| languages.len() as u64)
        );
        assert!(capabilities["language_count"].as_u64().unwrap_or_default() >= 13);

        for (path, _, language, expected_name) in fixtures {
            let outline = index.file_outline("demo", &workspace, path, 100).unwrap();
            assert_eq!(outline["language"], language, "wrong language for {path}");
            assert_eq!(outline["parse_errors"], false, "parse error in {path}");
            assert!(
                outline["symbols"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|symbol| symbol["name"] == expected_name),
                "{path} did not expose {expected_name}: {}",
                outline["symbols"]
            );
        }

        let qualified = index
            .find_symbol("demo", &workspace, "Worker.execute", ".", None, 20)
            .unwrap();
        assert!(
            qualified["results"]
                .as_array()
                .unwrap()
                .iter()
                .any(|symbol| symbol["language"] == "java"),
            "qualified-name search should use the leaf token as its source prefilter"
        );
    }

    #[test]
    fn extended_language_grammars_produce_real_symbols() {
        let dir = tempfile::tempdir().unwrap();
        let fixtures = [
            (
                "styles.css",
                ":root { --space: 8px; }\n.card, #main { color: red; }\n@keyframes fade { from { opacity: 0; } to { opacity: 1; } }\n",
                "css",
                ".card, #main",
            ),
            (
                "page.html",
                "<main id=\"app\"><user-card></user-card></main>\n",
                "html",
                "app",
            ),
            (
                "worker.dart",
                "class Worker { int execute() => 1; }\n",
                "dart",
                "execute",
            ),
            (
                "worker.ex",
                "defmodule Worker do\n  def execute, do: 1\nend\n",
                "elixir",
                "execute",
            ),
            (
                "worker.lua",
                "local function execute()\n  return 1\nend\n",
                "lua",
                "execute",
            ),
            ("worker.ml", "let execute () = 1\n", "ocaml", "execute"),
            (
                "worker.mli",
                "val execute : unit -> int\n",
                "ocaml-interface",
                "execute",
            ),
            (
                "worker.R",
                "execute <- function() {\n  1\n}\n",
                "r",
                "execute",
            ),
            (
                "Worker.swift",
                "final class Worker {\n  func execute() -> Int { 1 }\n}\n",
                "swift",
                "execute",
            ),
        ];
        for (path, source, _, _) in fixtures {
            fs::write(dir.path().join(path), source).unwrap();
        }

        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let index = CodeIndex::new().unwrap();
        let capabilities = index.capabilities();
        assert_eq!(
            capabilities["language_count"].as_u64(),
            capabilities["languages"]
                .as_array()
                .map(|languages| languages.len() as u64)
        );
        assert!(capabilities["language_count"].as_u64().unwrap_or_default() >= 20);

        for (path, _, language, expected_name) in fixtures {
            let outline = index.file_outline("demo", &workspace, path, 100).unwrap();
            assert_eq!(outline["language"], language, "wrong language for {path}");
            assert_eq!(outline["parse_errors"], false, "parse error in {path}");
            assert!(
                outline["symbols"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|symbol| symbol["name"] == expected_name),
                "{path} did not expose {expected_name}: {}",
                outline["symbols"]
            );
        }
    }

    #[test]
    fn html_and_css_outlines_keep_navigation_signal_without_tag_noise() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("page.html"),
            "<main id=\"app\">\n  <div>noise</div>\n  <user-card id=\"profile\"></user-card>\n</main>\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("styles.css"),
            ":root {\n  --space: 8px;\n}\n.card, #main { color: red; }\n@keyframes fade { from { opacity: 0; } to { opacity: 1; } }\n",
        )
        .unwrap();

        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let index = CodeIndex::new().unwrap();

        let html = index
            .file_outline("demo", &workspace, "page.html", 100)
            .unwrap();
        let html_symbols = html["symbols"].as_array().unwrap();
        assert!(html_symbols
            .iter()
            .any(|symbol| symbol["name"] == "app" && symbol["kind"] == "element"));
        assert!(html_symbols
            .iter()
            .any(|symbol| { symbol["name"] == "user-card" && symbol["kind"] == "component" }));
        assert!(html_symbols
            .iter()
            .all(|symbol| symbol["name"] != "main" && symbol["name"] != "div"));

        let app_id = html_symbols
            .iter()
            .find(|symbol| symbol["name"] == "app")
            .and_then(|symbol| symbol["id"].as_str())
            .unwrap();
        let html_context = index
            .symbol_context("demo", &workspace, app_id, 50)
            .unwrap();
        assert!(html_context["body"]["content"]
            .as_str()
            .unwrap()
            .contains("<user-card"));

        let css = index
            .file_outline("demo", &workspace, "styles.css", 100)
            .unwrap();
        let css_symbols = css["symbols"].as_array().unwrap();
        for (name, kind) in [
            (".card, #main", "selector"),
            ("--space", "variable"),
            ("fade", "keyframes"),
        ] {
            assert!(
                css_symbols
                    .iter()
                    .any(|symbol| symbol["name"] == name && symbol["kind"] == kind),
                "CSS outline did not expose {name} as {kind}: {}",
                css["symbols"]
            );
        }

        let fade = index
            .find_symbol("demo", &workspace, "fade", "styles.css", None, 10)
            .unwrap();
        let fade_id = fade["results"][0]["id"].as_str().unwrap();
        let css_context = index
            .symbol_context("demo", &workspace, fade_id, 50)
            .unwrap();
        assert!(css_context["body"]["content"]
            .as_str()
            .unwrap()
            .contains("@keyframes fade"));
    }

    #[test]
    fn extensionless_script_names_are_detected() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Rakefile"), "task :build do\nend\n").unwrap();
        fs::write(dir.path().join(".bashrc"), "load_env() { echo ready; }\n").unwrap();
        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let index = CodeIndex::new().unwrap();

        assert_eq!(
            index
                .file_outline("demo", &workspace, "Rakefile", 100)
                .unwrap()["language"],
            "ruby"
        );
        assert_eq!(
            index
                .file_outline("demo", &workspace, ".bashrc", 100)
                .unwrap()["language"],
            "bash"
        );
    }

    #[test]
    fn text_prefilter_avoids_building_ast_for_clear_misses() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..32 {
            fs::write(
                dir.path().join(format!("module_{index}.rs")),
                format!("pub fn unrelated_{index}() {{}}\n"),
            )
            .unwrap();
        }
        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let index = CodeIndex::new().unwrap();

        let result = index
            .find_symbol("demo", &workspace, "DefinitelyAbsentSymbol", ".", None, 20)
            .unwrap();

        assert_eq!(result["files_considered"], 32);
        assert_eq!(result["files_parsed"], 0);
        assert_eq!(result["result_count"], 0);
        assert_eq!(result["index"]["indexed_files"], 0);
        assert_eq!(result["index"]["ast_cached_files"], 0);
    }

    #[test]
    fn symbol_signatures_reuse_workspace_secret_redaction() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("settings.py"),
            "api_key = \"super-secret-value\"\n",
        )
        .unwrap();
        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let index = CodeIndex::new().unwrap();

        let outline = index
            .file_outline("demo", &workspace, "settings.py", 100)
            .unwrap();
        let symbol = outline["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .find(|symbol| symbol["name"] == "api_key")
            .unwrap();
        assert_eq!(symbol["signature_redacted"], true);
        assert!(symbol["signature"].as_str().unwrap().contains("[REDACTED]"));
        assert!(!outline.to_string().contains("super-secret-value"));
    }

    #[test]
    fn c_context_expands_function_extent_and_extracts_calls() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("engine.c"),
            "int helper(void) { return 1; }\nint compute(void) {\n    return helper();\n}\n",
        )
        .unwrap();
        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let index = CodeIndex::new().unwrap();

        let search = index
            .find_symbol("demo", &workspace, "compute", ".", None, 20)
            .unwrap();
        let symbol_id = search["results"][0]["id"].as_str().unwrap();
        let context = index
            .symbol_context("demo", &workspace, symbol_id, 50)
            .unwrap();
        assert!(context["body"]["content"]
            .as_str()
            .unwrap()
            .contains("return helper();"));
        assert!(context["syntax_calls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|call| call["name"] == "helper" && call["kind"] == "call"));
    }

    #[test]
    fn scan_failure_count_is_not_limited_by_diagnostic_sample() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..10 {
            fs::write(
                dir.path().join(format!("broken_{index}.py")),
                b"Needle = \xff\n",
            )
            .unwrap();
        }
        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let index = CodeIndex::new().unwrap();

        let result = index
            .find_symbol("demo", &workspace, "Needle", ".", None, 20)
            .unwrap();
        assert_eq!(result["files_failed"], 10);
        assert_eq!(result["failures"].as_array().unwrap().len(), 8);
        assert_eq!(result["failures_truncated"], true);
    }

    #[test]
    fn software_graph_reuses_indexed_symbols_and_marks_syntax_precision() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("engine.rs"),
            "fn helper() -> u8 { 1 }\nfn compute() -> u8 { helper() }\n",
        )
        .unwrap();
        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let index = CodeIndex::new().unwrap();

        let snapshot = index
            .software_graph("demo", &workspace, ".", 100, 100)
            .unwrap();
        assert_eq!(snapshot.provider, "tree-sitter");
        assert_eq!(snapshot.precision, GraphPrecision::Syntax);
        assert_eq!(snapshot.files_indexed, 1);
        assert!(!snapshot.truncated);

        let file = snapshot
            .graph
            .nodes
            .values()
            .find(|node| node.kind == NodeKind::File)
            .unwrap();
        let helper = snapshot
            .graph
            .nodes
            .values()
            .find(|node| node.label == "helper")
            .unwrap();
        let compute = snapshot
            .graph
            .nodes
            .values()
            .find(|node| node.label == "compute")
            .unwrap();
        assert_eq!(helper.provenance.precision, GraphPrecision::Syntax);
        assert!(snapshot.graph.edges.iter().any(|edge| {
            edge.kind == EdgeKind::Defines && edge.from == file.id && edge.to == helper.id
        }));
        assert!(snapshot.graph.edges.iter().any(|edge| {
            edge.kind == EdgeKind::Calls && edge.from == compute.id && edge.to == helper.id
        }));
    }

    #[test]
    fn software_graph_resolves_unique_cross_file_calls_at_syntax_precision() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("helper.rs"),
            "pub fn helper() -> u8 { 1 }\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("main.rs"),
            "fn compute() -> u8 { helper() }\n",
        )
        .unwrap();
        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let index = CodeIndex::new().unwrap();

        let snapshot = index
            .software_graph("demo", &workspace, ".", 100, 100)
            .unwrap();
        let helper = snapshot
            .graph
            .nodes
            .values()
            .find(|node| node.label == "helper")
            .unwrap();
        let compute = snapshot
            .graph
            .nodes
            .values()
            .find(|node| node.label == "compute")
            .unwrap();
        let edge = snapshot
            .graph
            .edges
            .iter()
            .find(|edge| {
                edge.kind == EdgeKind::Calls && edge.from == compute.id && edge.to == helper.id
            })
            .expect("unique cross-file call edge");
        assert_eq!(edge.provenance.precision, GraphPrecision::Syntax);
        assert_eq!(
            edge.provenance.provider,
            "tree-sitter/global-name-resolution"
        );
    }

    #[test]
    fn invalidation_rebuilds_changed_symbols() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("main.go"),
            "package main\nfunc OldName() {}\n",
        )
        .unwrap();
        let workspace = Workspace::new(dir.path(), true, false).unwrap();
        let index = CodeIndex::new().unwrap();
        let first = index
            .find_symbol("demo", &workspace, "OldName", ".", None, 10)
            .unwrap();
        assert_eq!(first["result_count"], 1);
        let view = workspace.read_file("main.go", 1, None).unwrap();
        workspace
            .replace_text("main.go", "OldName", "NewName", &view.sha256)
            .unwrap();
        index.invalidate(workspace.root(), "main.go");
        let second = index
            .find_symbol("demo", &workspace, "NewName", ".", None, 10)
            .unwrap();
        assert_eq!(second["result_count"], 1);
        assert_eq!(second["results"][0]["name"], "NewName");
    }
}
