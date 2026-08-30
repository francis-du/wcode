use crate::graph::{
    EdgeKind, GraphBuildFailure, GraphEdge, GraphNode, GraphPrecision, GraphProvenance, NodeKind,
    SoftwareGraph, SoftwareGraphSnapshot,
};
use crate::workspace::{redact_sensitive_text, SourceDocument, SourceStamp, Workspace};
use anyhow::{anyhow, bail, Context, Result};
use memchr::{memchr, memchr2};
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

mod api;
#[path = "graph.rs"]
mod graph_build;
mod indexing;
mod symbols;

#[cfg(test)]
#[path = "../../../tests/unit/graph/code_index.rs"]
mod tests;
