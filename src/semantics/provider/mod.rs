use crate::authorization::AuthorizationKind;
use crate::evidence_store::workspace_state_directory;
use crate::graph::{
    EdgeKind, GraphImportEdge, GraphImportNode, GraphPrecision, GraphProviderImport, NodeKind,
};
use crate::workspace::{redact_sensitive_text, SourceDocument, StampedSourcePath, Workspace};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
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

#[path = "support.rs"]
mod support;
pub use support::refresh;
use support::*;
pub(crate) use support::{provider_available_for_path, refresh_automatic};

#[path = "discovery.rs"]
mod discovery;
use discovery::{executable_discovery_source, find_executable};

#[path = "install.rs"]
pub mod install;
#[cfg(test)]
use discovery::{
    executable_name, is_rustup_proxy, known_language_tool_paths_from,
    rustup_proxy_component_ready_uncached, trusted_provider_path,
};
#[path = "auto.rs"]
mod auto;
pub(crate) use auto::{state as automatic_state, SemanticAutoState};

#[path = "client.rs"]
mod client;
use client::{render_error_chain as render, LspClient};

#[path = "session.rs"]
mod session;
use session::{DocumentSyncState, SemanticSession};
pub(crate) use session::{SemanticSessionPool, SemanticSessionPoolStatus};

#[path = "navigation.rs"]
mod navigation;
pub(crate) use navigation::navigate;
pub use navigation::SemanticNavigationIntent;
#[path = "rename.rs"]
mod rename;
pub(crate) use rename::{rename_plan, RenamePlanRequest};
#[path = "code_action.rs"]
mod code_action;
pub(crate) use code_action::{organize_imports_plan, quick_fix_plan};

#[path = "index.rs"]
mod index;
use index::build_provider_import;
#[path = "language.rs"]
mod language;
pub use language::language_for_path;

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
    pub discovery: &'static str,
    pub action: Option<&'static str>,
    pub install: Option<install::SemanticProviderInstallPlan>,
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
    status_for_languages(workspace, sessions, &languages)
}

pub(crate) fn status_for_languages(
    workspace: &Workspace,
    sessions: Option<&SemanticSessionPool>,
    languages: &BTreeSet<SemanticLanguage>,
) -> Result<Vec<SemanticProviderStatus>> {
    Ok(SemanticLanguage::ALL
        .into_iter()
        .map(|language| {
            let present = languages.contains(&language);
            let candidates = if present {
                provider_candidates(workspace, language)
            } else {
                Vec::new()
            };
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
            let discovery = selected
                .as_ref()
                .map(|(_, executable)| executable_discovery_source(executable))
                .unwrap_or("missing");
            let install = if present && !available {
                install::install_plan(workspace, language).ok()
            } else {
                None
            };
            let action = if !present {
                None
            } else if !available {
                Some("install_lsp")
            } else if !workspace.exec_enabled() || !workspace.semantic_exec_enabled() {
                Some("enable_lsp")
            } else if !authorized {
                Some("authorize_lsp")
            } else if !session_validated {
                Some("initialize_lsp")
            } else {
                None
            };
            let reason = if !present {
                "no matching source files detected".to_owned()
            } else if !available {
                install
                    .as_ref()
                    .map(|plan| {
                        if plan.model_can_install {
                            format!(
                                "canonical LSP {} is missing; the model may call semantic_provider_install after exact approval, then refresh semantic providers; Tree-sitter syntax remains available",
                                plan.provider
                            )
                        } else {
                            format!(
                                "canonical LSP {} is missing and requires {}; follow the returned install plan, then refresh semantic providers; Tree-sitter syntax remains available",
                                plan.provider, plan.manager
                            )
                        }
                    })
                    .unwrap_or_else(|| "no supported LSP server was found; install the canonical server and refresh LSP status; Tree-sitter syntax remains available".to_owned())
            } else if !workspace.exec_enabled() {
                "command execution is disabled".to_owned()
            } else if !workspace.semantic_exec_enabled() {
                "LSP execution is disabled by --no-semantic".to_owned()
            } else if !authorized {
                "this LSP server requires RiskyExecution approval bound to Workspace + server + current binary identity (or process-wide --allow-risky-exec) before it may load repository-controlled project configuration".to_owned()
            } else if !session_validated {
                if automatic {
                    "automatic LSP server is launch-ready; live validation is pending the first successful initialize".to_owned()
                } else {
                    "LSP server is launch-ready and authorized; live validation is pending the first successful initialize".to_owned()
                }
            } else {
                "LSP server completed live initialization for the current binary".to_owned()
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
                precision: if runnable { "semantic" } else { "syntax" },
                discovery,
                action,
                install,
                reason,
            }
        })
        .collect())
}
