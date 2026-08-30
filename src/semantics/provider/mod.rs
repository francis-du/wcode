use crate::authorization::AuthorizationKind;
use crate::graph::{
    EdgeKind, GraphImportEdge, GraphImportNode, GraphPrecision, GraphProviderImport, NodeKind,
};
use crate::workspace::{SourceDocument, Workspace};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::{timeout, Duration};
use url::Url;

const MAX_PROVIDER_FILES: usize = 256;
const MAX_PROVIDER_SYMBOLS: usize = 2_000;
const MAX_PROVIDER_EDGES: usize = 8_000;
const LSP_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const LSP_PROVIDER_TIMEOUT: Duration = Duration::from_secs(90);

#[path = "registry.rs"]
mod registry;
use registry::{ProviderCandidate, PROVIDERS};

#[path = "client.rs"]
mod client;
use client::LspClient;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticLanguage {
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

impl SemanticLanguage {
    pub const ALL: [Self; 22] = [
        Self::Bash,
        Self::C,
        Self::Cpp,
        Self::CSharp,
        Self::Css,
        Self::Dart,
        Self::Elixir,
        Self::Go,
        Self::Html,
        Self::Java,
        Self::JavaScript,
        Self::Lua,
        Self::Ocaml,
        Self::OcamlInterface,
        Self::Php,
        Self::Python,
        Self::R,
        Self::Ruby,
        Self::Rust,
        Self::Swift,
        Self::TypeScript,
        Self::Tsx,
    ];

    pub const fn as_str(self) -> &'static str {
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

    pub const fn lsp_language_id(self) -> &'static str {
        match self {
            Self::Bash => "shellscript",
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
            Self::Ocaml | Self::OcamlInterface => "ocaml",
            Self::Php => "php",
            Self::Python => "python",
            Self::R => "r",
            Self::Ruby => "ruby",
            Self::Rust => "rust",
            Self::Swift => "swift",
            Self::TypeScript => "typescript",
            Self::Tsx => "typescriptreact",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SemanticProviderStatus {
    pub language: SemanticLanguage,
    pub provider: Option<String>,
    pub executable: Option<String>,
    pub available: bool,
    pub runnable: bool,
    pub precision: &'static str,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SemanticProviderRun {
    pub provider: String,
    pub languages: Vec<SemanticLanguage>,
    pub files: usize,
    pub nodes: usize,
    pub edges: usize,
    pub call_hierarchy: bool,
    pub implementation_resolution: bool,
    pub cached: bool,
    pub revision: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SemanticProviderRefresh {
    pub statuses: Vec<SemanticProviderStatus>,
    pub runs: Vec<SemanticProviderRun>,
    pub imports: Vec<GraphProviderImport>,
    pub failures: Vec<String>,
    pub truncated: bool,
}

pub fn status(workspace: &Workspace) -> Result<Vec<SemanticProviderStatus>> {
    let files = workspace.source_files(".", 5_000)?.0;
    let languages = files
        .iter()
        .filter_map(|path| language_for_path(path))
        .collect::<BTreeSet<_>>();
    Ok(SemanticLanguage::ALL
        .into_iter()
        .map(|language| {
            let present = languages.contains(&language);
            let selected = select_provider(language);
            let available = selected.is_some();
            let authorized = workspace.risky_operation_authorized(&refresh_authorization_operation(
                ".", 128, 1_000,
            ));
            let runnable = present && available && workspace.exec_enabled() && authorized;
            let reason = if !present {
                "no matching source files detected".to_owned()
            } else if !workspace.exec_enabled() {
                "command execution is disabled".to_owned()
            } else if !authorized {
                "semantic LSP execution requires an exact local RiskyExecution approval (or process-wide --allow-risky-exec) because language servers load repository-controlled project configuration".to_owned()
            } else if !available {
                "no supported language server was found on PATH; syntax precision remains available".to_owned()
            } else {
                "semantic LSP provider is available".to_owned()
            };
            SemanticProviderStatus {
                language,
                provider: selected.as_ref().map(|(spec, _)| spec.id.to_owned()),
                executable: selected
                    .as_ref()
                    .map(|(_, path)| path.display().to_string()),
                available,
                runnable,
                precision: if runnable { "semantic" } else { "syntax-fallback" },
                reason,
            }
        })
        .collect())
}

fn refresh_authorization_operation(path: &str, max_files: usize, max_symbols: usize) -> String {
    format!("semantic_provider_refresh\0{path}\0{max_files}\0{max_symbols}")
}

pub async fn refresh(
    workspace: &Workspace,
    path: &str,
    max_files: usize,
    max_symbols: usize,
    existing: &BTreeMap<String, GraphProviderImport>,
) -> Result<SemanticProviderRefresh> {
    let statuses = status(workspace)?;
    if !workspace.exec_enabled() {
        bail!("semantic provider refresh requires command execution; restart without --no-exec");
    }
    if !workspace.risky_exec_enabled() {
        workspace.authorize_risky_operation(
            AuthorizationKind::RiskyExecution,
            &refresh_authorization_operation(path, max_files, max_symbols),
            "allow semantic-provider refresh; language servers may evaluate repository-controlled project configuration",
        )?;
    }
    let max_files = max_files.clamp(1, MAX_PROVIDER_FILES);
    let max_symbols = max_symbols.clamp(1, MAX_PROVIDER_SYMBOLS);
    let (paths, scan_truncated) = workspace.source_files(path, max_files)?;
    let mut assignments =
        BTreeMap::<String, (ProviderCandidate, PathBuf, Vec<(String, SemanticLanguage)>)>::new();
    for source_path in paths {
        let Some(language) = language_for_path(&source_path) else {
            continue;
        };
        let Some((provider, executable)) = select_provider(language) else {
            continue;
        };
        assignments
            .entry(provider.id.to_owned())
            .or_insert_with(|| (provider, executable, Vec::new()))
            .2
            .push((source_path, language));
    }

    let mut runs = Vec::new();
    let mut imports = Vec::new();
    let mut failures = Vec::new();
    let mut truncated = scan_truncated;
    for (_, (provider, executable, files)) in assignments {
        let prepared = match prepare_sources(workspace, &files) {
            Ok(prepared) => prepared,
            Err(error) => {
                failures.push(format!("{}: {error}", provider.id));
                continue;
            }
        };
        let revision = provider_revision(provider, &executable, max_symbols, &prepared);
        let provider_name = format!("lsp:{}", provider.id);
        if let Some(cached) = existing
            .get(&provider_name)
            .filter(|cached| cached.revision == revision)
        {
            runs.push(SemanticProviderRun {
                provider: provider_name,
                languages: prepared
                    .iter()
                    .map(|source| source.language)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                files: prepared.len(),
                nodes: cached.nodes.len(),
                edges: cached.edges.len(),
                call_hierarchy: cached.edges.iter().any(|edge| edge.kind == EdgeKind::Calls),
                implementation_resolution: cached
                    .edges
                    .iter()
                    .any(|edge| edge.kind == EdgeKind::Implements),
                cached: true,
                revision,
            });
            continue;
        }
        let outcome = timeout(
            LSP_PROVIDER_TIMEOUT,
            build_provider_import(
                workspace,
                provider,
                &executable,
                &prepared,
                max_symbols,
                revision,
            ),
        )
        .await;
        match outcome {
            Ok(Ok((import, run, was_truncated))) => {
                truncated |= was_truncated;
                runs.push(run);
                imports.push(import);
            }
            Ok(Err(error)) => failures.push(format!("{}: {error}", provider.id)),
            Err(_) => failures.push(format!("{}: provider timed out", provider.id)),
        }
    }
    Ok(SemanticProviderRefresh {
        statuses,
        runs,
        imports,
        failures,
        truncated,
    })
}

struct PreparedSemanticSource {
    language: SemanticLanguage,
    source: SourceDocument,
}

fn prepare_sources(
    workspace: &Workspace,
    files: &[(String, SemanticLanguage)],
) -> Result<Vec<PreparedSemanticSource>> {
    files
        .iter()
        .map(|(path, language)| {
            Ok(PreparedSemanticSource {
                language: *language,
                source: workspace.load_source(path)?,
            })
        })
        .collect()
}

fn provider_revision(
    provider: ProviderCandidate,
    executable: &Path,
    max_symbols: usize,
    files: &[PreparedSemanticSource],
) -> String {
    let executable_fingerprint = std::fs::metadata(executable)
        .ok()
        .map(|metadata| {
            let modified = metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos())
                .unwrap_or_default();
            format!("{}:{modified}", metadata.len())
        })
        .unwrap_or_else(|| "unknown".to_owned());
    let mut fingerprints = files
        .iter()
        .map(|source| format!("{}:{}", source.source.path, source.source.sha256))
        .collect::<Vec<_>>();
    fingerprints.sort();
    let input = format!(
        "semantic-provider-v2\n{}\n{}\n{}\n{}",
        provider.id,
        executable_fingerprint,
        max_symbols,
        fingerprints.join("\n")
    );
    format!("sha256:{:x}", Sha256::digest(input.as_bytes()))
}

async fn build_provider_import(
    workspace: &Workspace,
    provider: ProviderCandidate,
    executable: &Path,
    files: &[PreparedSemanticSource],
    max_symbols: usize,
    revision: String,
) -> Result<(GraphProviderImport, SemanticProviderRun, bool)> {
    let mut client = LspClient::start(workspace, provider, executable).await?;
    let root_uri = Url::from_directory_path(workspace.root())
        .map_err(|_| anyhow!("workspace root could not be converted to a file URI"))?
        .to_string();
    let capabilities = client.initialize(&root_uri).await?;
    let call_hierarchy = capabilities
        .get("callHierarchyProvider")
        .is_some_and(|value| value.as_bool().unwrap_or(!value.is_null()));
    let implementation_resolution = capabilities
        .get("implementationProvider")
        .is_some_and(|value| value.as_bool().unwrap_or(!value.is_null()));

    let mut nodes = BTreeMap::<String, GraphImportNode>::new();
    let mut edges = Vec::<GraphImportEdge>::new();
    let mut symbol_positions = Vec::new();
    let mut truncated = false;

    for prepared in files {
        if nodes.len() >= max_symbols {
            truncated = true;
            break;
        }
        let source = &prepared.source;
        let language = prepared.language;
        let uri = Url::from_file_path(workspace.root().join(&source.path))
            .map_err(|_| anyhow!("source path could not be converted to a file URI"))?
            .to_string();
        client
            .notify(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": uri,
                        "languageId": language.lsp_language_id(),
                        "version": 1,
                        "text": source.content,
                    }
                }),
            )
            .await?;
        let result = match client
            .request(
                "textDocument/documentSymbol",
                json!({"textDocument":{"uri":uri}}),
            )
            .await
        {
            Ok(value) => value,
            Err(_) => continue,
        };
        let mut symbols = Vec::new();
        flatten_document_symbols(&result, &source.path, None, &mut symbols);
        for symbol in symbols {
            if nodes.len() >= max_symbols {
                truncated = true;
                break;
            }
            let id = semantic_node_id(
                provider.id,
                &symbol.path,
                symbol.line,
                symbol.character,
                &symbol.name,
            );
            let mut attributes = BTreeMap::new();
            attributes.insert("path".into(), json!(symbol.path));
            attributes.insert("source_sha256".into(), json!(source.sha256.as_str()));
            attributes.insert("name".into(), json!(symbol.name));
            attributes.insert("qualified_name".into(), json!(symbol.qualified_name));
            attributes.insert("lsp_kind".into(), json!(symbol.kind));
            attributes.insert("line".into(), json!(symbol.line + 1));
            attributes.insert("character".into(), json!(symbol.character + 1));
            nodes.entry(id.clone()).or_insert(GraphImportNode {
                id: id.clone(),
                kind: lsp_node_kind(symbol.kind),
                label: symbol.qualified_name.clone(),
                attributes,
            });
            symbol_positions.push((id, uri.clone(), symbol.line, symbol.character));
        }
    }

    if call_hierarchy {
        for (from_id, uri, line, character) in symbol_positions.iter().take(max_symbols) {
            if edges.len() >= MAX_PROVIDER_EDGES {
                truncated = true;
                break;
            }
            let prepared = match client
                .request(
                    "textDocument/prepareCallHierarchy",
                    json!({
                        "textDocument":{"uri":uri},
                        "position":{"line":line,"character":character}
                    }),
                )
                .await
            {
                Ok(value) => value,
                Err(_) => continue,
            };
            let Some(item) = prepared.as_array().and_then(|items| items.first()).cloned() else {
                continue;
            };
            let outgoing = match client
                .request("callHierarchy/outgoingCalls", json!({"item":item}))
                .await
            {
                Ok(value) => value,
                Err(_) => continue,
            };
            for call in outgoing.as_array().into_iter().flatten() {
                if edges.len() >= MAX_PROVIDER_EDGES {
                    truncated = true;
                    break;
                }
                let Some(target) = call.get("to") else {
                    continue;
                };
                let Some(target_node) = call_hierarchy_node(workspace, provider.id, target) else {
                    continue;
                };
                let to_id = target_node.id.clone();
                nodes.entry(to_id.clone()).or_insert(target_node);
                if from_id != &to_id
                    && !edges.iter().any(|edge| {
                        edge.from == *from_id && edge.to == to_id && edge.kind == EdgeKind::Calls
                    })
                {
                    edges.push(GraphImportEdge {
                        from: from_id.clone(),
                        to: to_id,
                        kind: EdgeKind::Calls,
                    });
                }
            }
        }
    }

    if implementation_resolution {
        for (interface_id, uri, line, character) in symbol_positions.iter().take(max_symbols) {
            if edges.len() >= MAX_PROVIDER_EDGES {
                truncated = true;
                break;
            }
            let Some(interface) = nodes.get(interface_id).cloned() else {
                continue;
            };
            let implementations = match client
                .request(
                    "textDocument/implementation",
                    json!({
                        "textDocument":{"uri":uri},
                        "position":{"line":line,"character":character}
                    }),
                )
                .await
            {
                Ok(value) => value,
                Err(_) => continue,
            };
            for (path, target_line, target_character) in
                implementation_locations(workspace, &implementations)
            {
                if edges.len() >= MAX_PROVIDER_EDGES {
                    truncated = true;
                    break;
                }
                let existing_target = nodes
                    .values()
                    .find(|node| node_at_location(node, &path, target_line, target_character))
                    .map(|node| node.id.clone());
                let target_id = existing_target.unwrap_or_else(|| {
                    semantic_node_id(
                        provider.id,
                        &path,
                        target_line,
                        target_character,
                        &interface.label,
                    )
                });
                if !nodes.contains_key(&target_id) {
                    let source_sha256 = workspace
                        .load_source(&path)
                        .ok()
                        .map(|source| source.sha256);
                    let mut attributes = BTreeMap::new();
                    attributes.insert("path".into(), json!(path));
                    if let Some(source_sha256) = source_sha256 {
                        attributes.insert("source_sha256".into(), json!(source_sha256));
                    }
                    attributes.insert("name".into(), json!(interface.label));
                    attributes.insert("qualified_name".into(), json!(interface.label));
                    attributes.insert("line".into(), json!(target_line + 1));
                    attributes.insert("character".into(), json!(target_character + 1));
                    nodes.insert(
                        target_id.clone(),
                        GraphImportNode {
                            id: target_id.clone(),
                            kind: interface.kind,
                            label: format!("implementation of {}", interface.label),
                            attributes,
                        },
                    );
                }
                if target_id != *interface_id
                    && !edges.iter().any(|edge| {
                        edge.from == target_id
                            && edge.to == *interface_id
                            && edge.kind == EdgeKind::Implements
                    })
                {
                    edges.push(GraphImportEdge {
                        from: target_id,
                        to: interface_id.clone(),
                        kind: EdgeKind::Implements,
                    });
                }
            }
        }
    }

    let _ = client.shutdown().await;
    if nodes.is_empty() {
        bail!("language server returned no semantic document symbols");
    }
    let import = GraphProviderImport {
        provider: format!("lsp:{}", provider.id),
        precision: GraphPrecision::Semantic,
        revision: revision.clone(),
        nodes: nodes.into_values().collect(),
        edges,
    };
    import.validate()?;
    let languages = files
        .iter()
        .map(|source| source.language)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let run = SemanticProviderRun {
        provider: import.provider.clone(),
        languages,
        files: files.len(),
        nodes: import.nodes.len(),
        edges: import.edges.len(),
        call_hierarchy,
        implementation_resolution,
        cached: false,
        revision,
    };
    Ok((import, run, truncated))
}

#[derive(Clone)]
struct DocumentSymbol {
    name: String,
    qualified_name: String,
    kind: u64,
    path: String,
    line: u64,
    character: u64,
}

fn flatten_document_symbols(
    value: &Value,
    path: &str,
    parent: Option<&str>,
    output: &mut Vec<DocumentSymbol>,
) {
    let Some(items) = value.as_array() else {
        return;
    };
    for item in items {
        let Some(name) = item.get("name").and_then(Value::as_str) else {
            continue;
        };
        let kind = item.get("kind").and_then(Value::as_u64).unwrap_or(13);
        let range = item
            .get("selectionRange")
            .or_else(|| item.get("range"))
            .or_else(|| item.pointer("/location/range"));
        let line = range
            .and_then(|range| range.pointer("/start/line"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let character = range
            .and_then(|range| range.pointer("/start/character"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let qualified_name = parent
            .map(|parent| format!("{parent}::{name}"))
            .unwrap_or_else(|| name.to_owned());
        output.push(DocumentSymbol {
            name: name.to_owned(),
            qualified_name: qualified_name.clone(),
            kind,
            path: path.to_owned(),
            line,
            character,
        });
        if let Some(children) = item.get("children") {
            flatten_document_symbols(children, path, Some(&qualified_name), output);
        }
    }
}

fn call_hierarchy_node(
    workspace: &Workspace,
    provider: &str,
    item: &Value,
) -> Option<GraphImportNode> {
    let uri = item.get("uri")?.as_str()?;
    let url = Url::parse(uri).ok()?;
    let path = url.to_file_path().ok()?;
    let canonical = path.canonicalize().ok()?;
    if !canonical.starts_with(workspace.root()) {
        return None;
    }
    let relative = canonical
        .strip_prefix(workspace.root())
        .ok()?
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    let name = item.get("name")?.as_str()?;
    let kind = item.get("kind").and_then(Value::as_u64).unwrap_or(13);
    let range = item.get("selectionRange").or_else(|| item.get("range"));
    let line = range
        .and_then(|range| range.pointer("/start/line"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let character = range
        .and_then(|range| range.pointer("/start/character"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let id = semantic_node_id(provider, &relative, line, character, name);
    let source_sha256 = workspace
        .load_source(&relative)
        .ok()
        .map(|source| source.sha256);
    let mut attributes = BTreeMap::new();
    attributes.insert("path".into(), json!(relative));
    if let Some(source_sha256) = source_sha256 {
        attributes.insert("source_sha256".into(), json!(source_sha256));
    }
    attributes.insert("name".into(), json!(name));
    attributes.insert("qualified_name".into(), json!(name));
    attributes.insert("lsp_kind".into(), json!(kind));
    attributes.insert("line".into(), json!(line + 1));
    attributes.insert("character".into(), json!(character + 1));
    Some(GraphImportNode {
        id,
        kind: lsp_node_kind(kind),
        label: name.to_owned(),
        attributes,
    })
}

fn implementation_locations(workspace: &Workspace, value: &Value) -> Vec<(String, u64, u64)> {
    let items = if let Some(items) = value.as_array() {
        items.iter().collect::<Vec<_>>()
    } else if value.is_object() {
        vec![value]
    } else {
        Vec::new()
    };
    items
        .into_iter()
        .filter_map(|item| {
            let uri = item
                .get("uri")
                .or_else(|| item.get("targetUri"))?
                .as_str()?;
            let url = Url::parse(uri).ok()?;
            let path = url.to_file_path().ok()?.canonicalize().ok()?;
            if !path.starts_with(workspace.root()) {
                return None;
            }
            let relative = path
                .strip_prefix(workspace.root())
                .ok()?
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            let range = item
                .get("range")
                .or_else(|| item.get("targetSelectionRange"))
                .or_else(|| item.get("targetRange"))?;
            let line = range.pointer("/start/line")?.as_u64()?;
            let character = range.pointer("/start/character")?.as_u64()?;
            Some((relative, line, character))
        })
        .collect()
}

fn node_at_location(node: &GraphImportNode, path: &str, line: u64, character: u64) -> bool {
    node.attributes.get("path").and_then(Value::as_str) == Some(path)
        && node.attributes.get("line").and_then(Value::as_u64) == Some(line + 1)
        && node.attributes.get("character").and_then(Value::as_u64) == Some(character + 1)
}

fn semantic_node_id(provider: &str, path: &str, line: u64, character: u64, name: &str) -> String {
    let digest =
        Sha256::digest(format!("{provider}\n{path}\n{line}\n{character}\n{name}").as_bytes());
    format!("semantic:{:x}", digest)
}

fn lsp_node_kind(kind: u64) -> NodeKind {
    match kind {
        2..=4 => NodeKind::Module,
        5 => NodeKind::Class,
        6 | 9 | 12 => NodeKind::Function,
        11 => NodeKind::Interface,
        23 => NodeKind::Struct,
        _ => NodeKind::Symbol,
    }
}

fn select_provider(language: SemanticLanguage) -> Option<(ProviderCandidate, PathBuf)> {
    PROVIDERS
        .iter()
        .copied()
        .filter(|provider| provider.languages.contains(&language))
        .find_map(|provider| find_executable(provider.executable).map(|path| (provider, path)))
}

fn find_executable(name: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(name);
    if candidate.components().count() > 1 && candidate.is_file() {
        return Some(candidate);
    }
    let path = env::var_os("PATH")?;
    #[cfg(windows)]
    let extensions = env::var_os("PATHEXT")
        .map(|value| {
            value
                .to_string_lossy()
                .split(';')
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![".EXE".into(), ".CMD".into(), ".BAT".into()]);
    for directory in env::split_paths(&path) {
        let plain = directory.join(name);
        if plain.is_file() {
            return Some(plain);
        }
        #[cfg(windows)]
        for extension in &extensions {
            let with_extension = directory.join(format!("{name}{extension}"));
            if with_extension.is_file() {
                return Some(with_extension);
            }
        }
    }
    None
}

pub fn language_for_path(path: &str) -> Option<SemanticLanguage> {
    let path = Path::new(path);
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    let extension = path
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match name.as_str() {
        ".bashrc" | ".bash_profile" | ".bash_login" | ".profile" | ".zshrc" | ".zprofile"
        | ".zshenv" | ".zlogin" => Some(SemanticLanguage::Bash),
        "gemfile" | "rakefile" | "guardfile" | "podfile" | "fastfile" | "appfile"
        | "deliverfile" | "brewfile" | "vagrantfile" => Some(SemanticLanguage::Ruby),
        _ => match extension.as_str() {
            "sh" | "bash" | "zsh" | "ksh" | "command" => Some(SemanticLanguage::Bash),
            "c" | "h" => Some(SemanticLanguage::C),
            "cc" | "cpp" | "cxx" | "c++" | "hh" | "hpp" | "hxx" | "h++" | "ipp" | "tpp" | "inl" => {
                Some(SemanticLanguage::Cpp)
            }
            "cs" | "cake" => Some(SemanticLanguage::CSharp),
            "css" => Some(SemanticLanguage::Css),
            "dart" => Some(SemanticLanguage::Dart),
            "ex" | "exs" => Some(SemanticLanguage::Elixir),
            "go" => Some(SemanticLanguage::Go),
            "html" | "htm" | "xhtml" => Some(SemanticLanguage::Html),
            "java" => Some(SemanticLanguage::Java),
            "js" | "jsx" | "mjs" | "cjs" => Some(SemanticLanguage::JavaScript),
            "lua" => Some(SemanticLanguage::Lua),
            "ml" => Some(SemanticLanguage::Ocaml),
            "mli" => Some(SemanticLanguage::OcamlInterface),
            "php" | "php3" | "php4" | "php5" | "phtml" => Some(SemanticLanguage::Php),
            "py" | "pyi" => Some(SemanticLanguage::Python),
            "r" => Some(SemanticLanguage::R),
            "rb" | "rake" | "gemspec" | "ru" | "jbuilder" => Some(SemanticLanguage::Ruby),
            "rs" => Some(SemanticLanguage::Rust),
            "swift" => Some(SemanticLanguage::Swift),
            "ts" | "mts" | "cts" => Some(SemanticLanguage::TypeScript),
            "tsx" => Some(SemanticLanguage::Tsx),
            _ => None,
        },
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/semantics/provider.rs"]
mod tests;
