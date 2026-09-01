use crate::authorization::AuthorizationKind;
use crate::evidence_store::workspace_state_directory;
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
const MAX_PROVIDER_RELATION_SYMBOLS: usize = 96;
const LSP_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const LSP_PROVIDER_TIMEOUT: Duration = Duration::from_secs(90);

#[path = "registry.rs"]
mod registry;
use registry::{automatic_provider, ProviderCandidate, PROVIDERS};

#[path = "auto.rs"]
mod auto;
pub(crate) use auto::{state as automatic_state, SemanticAutoState};

#[path = "client.rs"]
mod client;
use client::LspClient;

#[path = "session.rs"]
mod session;
use session::{DocumentSyncState, SemanticSession};
pub(crate) use session::{SemanticSessionPool, SemanticSessionPoolStatus};

#[path = "navigation.rs"]
mod navigation;
pub(crate) use navigation::navigate;
pub use navigation::SemanticNavigationIntent;

#[path = "index.rs"]
mod index;
use index::build_provider_import;

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
    pub detected: bool,
    pub available: bool,
    pub available_candidates: usize,
    pub canonical: bool,
    pub automatic: bool,
    pub launch_ready: bool,
    pub session_validated: bool,
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
    pub fallbacks: Vec<String>,
    pub failures: Vec<String>,
    pub truncated: bool,
}

pub fn status(
    workspace: &Workspace,
    sessions: Option<&SemanticSessionPool>,
) -> Result<Vec<SemanticProviderStatus>> {
    let files = workspace.source_files(".", 5_000)?.0;
    let languages = files
        .iter()
        .filter_map(|path| language_for_path(path))
        .collect::<BTreeSet<_>>();
    Ok(SemanticLanguage::ALL
        .into_iter()
        .map(|language| {
            let present = languages.contains(&language);
            let candidates = provider_candidates(workspace, language);
            let selected = sessions
                .and_then(|sessions| {
                    candidates.iter().find(|(provider, executable)| {
                        sessions.validated(workspace, *provider, executable)
                    })
                })
                .cloned()
                .or_else(|| candidates.first().cloned());
            let available = selected.is_some();
            let available_candidates = candidates.len();
            let canonical = selected
                .as_ref()
                .is_some_and(|(provider, _)| provider.canonical);
            let automatic = selected
                .as_ref()
                .is_some_and(|(provider, _)| automatic_provider(*provider));
            let authorized = automatic
                || workspace.risky_exec_enabled()
                || selected.as_ref().is_some_and(|(provider, executable)| {
                    provider_session_operation(workspace, *provider, executable).is_ok_and(
                        |operation| workspace.risky_operation_authorized(&operation),
                    )
                });
            let launch_ready = present
                && available
                && workspace.exec_enabled()
                && workspace.semantic_exec_enabled()
                && authorized;
            let session_validated = launch_ready
                && selected.as_ref().is_some_and(|(provider, executable)| {
                    sessions.is_some_and(|sessions| {
                        sessions.validated(workspace, *provider, executable)
                    })
                });
            let runnable = launch_ready && session_validated;
            let reason = if !present {
                "no matching source files detected".to_owned()
            } else if !available {
                "no supported language server was found on trusted PATH; syntax precision remains available".to_owned()
            } else if !workspace.exec_enabled() {
                "command execution is disabled".to_owned()
            } else if !workspace.semantic_exec_enabled() {
                "semantic LSP execution is disabled by --no-semantic".to_owned()
            } else if !authorized {
                "this language server requires a Workspace + Provider + binary-identity RiskyExecution approval (or process-wide --allow-risky-exec) before it may load repository-controlled project configuration".to_owned()
            } else if !session_validated {
                if automatic {
                    "automatic semantic provider is launch-ready; live validation is pending the first successful LSP initialize".to_owned()
                } else {
                    "semantic provider is launch-ready and authorized; live validation is pending the first successful LSP initialize".to_owned()
                }
            } else {
                "semantic provider completed live LSP initialization for the current provider binary".to_owned()
            };
            SemanticProviderStatus {
                language,
                provider: selected.as_ref().map(|(spec, _)| spec.id.to_owned()),
                executable: selected
                    .as_ref()
                    .map(|(_, path)| path.display().to_string()),
                detected: present,
                available,
                available_candidates,
                canonical,
                automatic,
                launch_ready,
                session_validated,
                runnable,
                precision: if runnable { "semantic" } else { "syntax-fallback" },
                reason,
            }
        })
        .collect())
}

fn provider_session_operation(
    workspace: &Workspace,
    provider: ProviderCandidate,
    executable: &Path,
) -> Result<String> {
    Ok(format!(
        "semantic_provider_session\0{}",
        session::session_key(workspace, provider, executable)?
    ))
}

fn authorize_provider_session(
    workspace: &Workspace,
    provider: ProviderCandidate,
    executable: &Path,
) -> Result<()> {
    if automatic_provider(provider) || workspace.risky_exec_enabled() {
        return Ok(());
    }
    workspace.authorize_risky_operation(
        AuthorizationKind::RiskyExecution,
        &provider_session_operation(workspace, provider, executable)?,
        &format!(
            "allow warm semantic-provider session for {} at the current provider-binary identity; the language server may load repository-controlled project configuration",
            provider.id
        ),
    )
}

pub async fn refresh(
    sessions: &SemanticSessionPool,
    workspace: &Workspace,
    path: &str,
    max_files: usize,
    max_symbols: usize,
    existing: &BTreeMap<String, GraphProviderImport>,
) -> Result<SemanticProviderRefresh> {
    refresh_impl(
        sessions,
        workspace,
        path,
        max_files,
        max_symbols,
        existing,
        false,
    )
    .await
}

pub(crate) async fn refresh_automatic(
    sessions: &SemanticSessionPool,
    workspace: &Workspace,
    path: &str,
    max_files: usize,
    max_symbols: usize,
    existing: &BTreeMap<String, GraphProviderImport>,
) -> Result<SemanticProviderRefresh> {
    refresh_impl(
        sessions,
        workspace,
        path,
        max_files,
        max_symbols,
        existing,
        true,
    )
    .await
}

async fn refresh_impl(
    sessions: &SemanticSessionPool,
    workspace: &Workspace,
    path: &str,
    max_files: usize,
    max_symbols: usize,
    existing: &BTreeMap<String, GraphProviderImport>,
    automatic_only: bool,
) -> Result<SemanticProviderRefresh> {
    if !workspace.exec_enabled() {
        bail!("semantic provider refresh requires command execution; restart without --no-exec");
    }
    if !workspace.semantic_exec_enabled() {
        bail!("semantic provider execution is disabled; restart without --no-semantic");
    }
    let max_files = max_files.clamp(1, MAX_PROVIDER_FILES);
    let max_symbols = max_symbols.clamp(1, MAX_PROVIDER_SYMBOLS);
    let discovery_limit = if automatic_only {
        auto::automatic_scan_limit(max_files)
    } else {
        max_files
    };
    let (paths, scan_truncated) = if automatic_only {
        workspace.source_files_background(path, discovery_limit)?
    } else {
        workspace.source_files(path, discovery_limit)?
    };
    let mut assignments =
        BTreeMap::<String, (ProviderCandidate, PathBuf, Vec<(String, SemanticLanguage)>)>::new();
    let mut assigned_files = 0usize;
    let mut assignment_truncated = false;
    for source_path in paths {
        let Some(language) = language_for_path(&source_path) else {
            continue;
        };
        let Some((provider, executable)) = select_provider(workspace, language) else {
            continue;
        };
        if automatic_only && !automatic_provider(provider) {
            continue;
        }
        if automatic_only && assigned_files == max_files {
            assignment_truncated = true;
            break;
        }
        assignments
            .entry(provider.id.to_owned())
            .or_insert_with(|| (provider, executable, Vec::new()))
            .2
            .push((source_path, language));
        assigned_files = assigned_files.saturating_add(1);
    }

    if !automatic_only {
        for (provider, executable, _) in assignments.values() {
            authorize_provider_session(workspace, *provider, executable)?;
        }
    }

    let mut runs = Vec::new();
    let mut imports = Vec::new();
    let mut fallbacks = Vec::new();
    let mut failures = Vec::new();
    let mut truncated = scan_truncated || assignment_truncated;
    for (_, (provider, executable, files)) in assignments {
        let work_class = if automatic_only {
            crate::resource::WorkClass::Background
        } else {
            crate::resource::WorkClass::Interactive
        };
        let prepared = match prepare_sources_with_class(workspace, &files, work_class) {
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
            if automatic_provider(provider) || !automatic_only {
                match sessions.handle(workspace, provider, &executable) {
                    Ok(handle) => {
                        if let Err(error) = handle.ensure_started(workspace, provider).await {
                            failures.push(format!("{}: warm session failed: {error}", provider.id));
                        }
                    }
                    Err(error) => {
                        failures.push(format!("{}: warm session failed: {error}", provider.id))
                    }
                }
            }
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
        match build_provider_import_timed(
            sessions,
            workspace,
            provider,
            &executable,
            &prepared,
            max_symbols,
            revision,
        )
        .await
        {
            Ok((import, run, was_truncated)) => {
                truncated |= was_truncated;
                runs.push(run);
                imports.push(import);
            }
            Err(primary_error) => {
                sessions.invalidate(workspace, provider, &executable);
                let fallback = (!automatic_only)
                    .then(|| alternate_provider_for_sources(workspace, provider, &prepared))
                    .flatten();
                let Some((alternate, alternate_executable)) = fallback else {
                    failures.push(format!("{}: {primary_error}", provider.id));
                    continue;
                };
                if let Err(error) =
                    authorize_provider_session(workspace, alternate, &alternate_executable)
                {
                    failures.push(format!(
                        "{}: {primary_error}; fallback {} authorization failed: {error}",
                        provider.id, alternate.id
                    ));
                    continue;
                }
                let alternate_revision =
                    provider_revision(alternate, &alternate_executable, max_symbols, &prepared);
                match build_provider_import_timed(
                    sessions,
                    workspace,
                    alternate,
                    &alternate_executable,
                    &prepared,
                    max_symbols,
                    alternate_revision,
                )
                .await
                {
                    Ok((import, run, was_truncated)) => {
                        fallbacks.push(format!(
                            "{} failed; used {} for {}",
                            provider.id,
                            alternate.id,
                            prepared
                                .iter()
                                .map(|source| source.language.as_str())
                                .collect::<BTreeSet<_>>()
                                .into_iter()
                                .collect::<Vec<_>>()
                                .join(",")
                        ));
                        truncated |= was_truncated;
                        runs.push(run);
                        imports.push(import);
                    }
                    Err(alternate_error) => {
                        sessions.invalidate(workspace, alternate, &alternate_executable);
                        failures.push(format!(
                            "{}: {primary_error}; fallback {}: {alternate_error}",
                            provider.id, alternate.id
                        ));
                    }
                }
            }
        }
    }
    Ok(SemanticProviderRefresh {
        statuses: status(workspace, Some(sessions))?,
        runs,
        imports,
        fallbacks,
        failures,
        truncated,
    })
}

async fn build_provider_import_timed(
    sessions: &SemanticSessionPool,
    workspace: &Workspace,
    provider: ProviderCandidate,
    executable: &Path,
    prepared: &[PreparedSemanticSource],
    max_symbols: usize,
    revision: String,
) -> Result<(GraphProviderImport, SemanticProviderRun, bool)> {
    timeout(
        LSP_PROVIDER_TIMEOUT,
        build_provider_import(
            sessions,
            workspace,
            provider,
            executable,
            prepared,
            max_symbols,
            revision,
        ),
    )
    .await
    .map_err(|_| anyhow!("provider timed out"))?
}

fn alternate_provider_for_sources(
    workspace: &Workspace,
    current: ProviderCandidate,
    prepared: &[PreparedSemanticSource],
) -> Option<(ProviderCandidate, PathBuf)> {
    let languages = prepared
        .iter()
        .map(|source| source.language)
        .collect::<BTreeSet<_>>();
    PROVIDERS
        .iter()
        .copied()
        .filter(|provider| provider.id != current.id)
        .filter(|provider| {
            languages
                .iter()
                .all(|language| provider.languages.contains(language))
        })
        .find_map(|provider| {
            provider
                .executables
                .iter()
                .find_map(|executable| find_executable(workspace, executable))
                .map(|path| (provider, path))
        })
}

struct PreparedSemanticSource {
    language: SemanticLanguage,
    source: SourceDocument,
}

#[cfg(test)]
fn prepare_sources(
    workspace: &Workspace,
    files: &[(String, SemanticLanguage)],
) -> Result<Vec<PreparedSemanticSource>> {
    prepare_sources_with_class(workspace, files, crate::resource::WorkClass::Interactive)
}

fn prepare_sources_with_class(
    workspace: &Workspace,
    files: &[(String, SemanticLanguage)],
    work_class: crate::resource::WorkClass,
) -> Result<Vec<PreparedSemanticSource>> {
    files
        .iter()
        .map(|(path, language)| {
            let _cpu = crate::resource::cpu_work(work_class);
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

fn call_hierarchy_candidate(kind: u64) -> bool {
    matches!(kind, 6 | 9 | 12)
}

fn implementation_candidate(kind: u64) -> bool {
    matches!(kind, 5 | 6 | 11 | 12 | 23)
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

pub(crate) fn provider_available_for_path(workspace: &Workspace, path: &str) -> bool {
    workspace.exec_enabled()
        && workspace.semantic_exec_enabled()
        && language_for_path(path)
            .and_then(|language| select_provider(workspace, language))
            .is_some()
}

fn provider_candidates(
    workspace: &Workspace,
    language: SemanticLanguage,
) -> Vec<(ProviderCandidate, PathBuf)> {
    PROVIDERS
        .iter()
        .copied()
        .filter(|provider| provider.languages.contains(&language))
        .filter_map(|provider| {
            provider
                .executables
                .iter()
                .find_map(|executable| find_executable(workspace, executable))
                .map(|path| (provider, path))
        })
        .collect()
}

fn select_provider(
    workspace: &Workspace,
    language: SemanticLanguage,
) -> Option<(ProviderCandidate, PathBuf)> {
    provider_candidates(workspace, language).into_iter().next()
}

fn provider_launch_args(workspace: &Workspace, provider: ProviderCandidate) -> Result<Vec<String>> {
    let mut args = provider
        .args
        .iter()
        .map(|arg| (*arg).to_owned())
        .collect::<Vec<_>>();
    match provider.id {
        "dart-language-server" => {
            args.extend([
                "--client-id".to_owned(),
                "wcode".to_owned(),
                "--client-version".to_owned(),
                env!("CARGO_PKG_VERSION").to_owned(),
            ]);
        }
        "jdtls" => {
            let data = workspace_state_directory(workspace)?
                .join("lsp/jdtls")
                .join(std::process::id().to_string());
            std::fs::create_dir_all(&data)?;
            args.extend(["-data".to_owned(), data.display().to_string()]);
        }
        _ => {}
    }
    Ok(args)
}

fn trusted_provider_path(workspace: &Workspace, candidate: &Path) -> Option<PathBuf> {
    let executable = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        env::current_dir().ok()?.join(candidate)
    };
    let canonical = executable.canonicalize().ok()?;
    (!canonical.starts_with(workspace.root())).then_some(executable)
}

fn find_executable(workspace: &Workspace, name: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(name);
    if candidate.components().count() > 1 && candidate.is_file() {
        return trusted_provider_path(workspace, &candidate);
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
            if let Some(path) = trusted_provider_path(workspace, &plain) {
                return Some(path);
            }
        }
        #[cfg(windows)]
        for extension in &extensions {
            let with_extension = directory.join(format!("{name}{extension}"));
            if with_extension.is_file() {
                if let Some(path) = trusted_provider_path(workspace, &with_extension) {
                    return Some(path);
                }
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
