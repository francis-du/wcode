use super::*;

pub(super) fn provider_session_operation(
    workspace: &Workspace,
    provider: ProviderCandidate,
    executable: &Path,
) -> Result<String> {
    Ok(format!(
        "semantic_provider_session\0{}",
        session::session_key(workspace, provider, executable)?
    ))
}

pub(super) fn authorize_provider_session(
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
            "allow warm LSP session for {} at the current server-binary identity; the server may load repository-controlled project configuration",
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
        bail!("LSP refresh requires command execution; restart without --no-exec");
    }
    if !workspace.semantic_exec_enabled() {
        bail!("LSP execution is disabled; restart without --no-semantic");
    }
    let max_files = max_files.clamp(1, MAX_PROVIDER_FILES);
    let max_symbols = max_symbols.clamp(1, MAX_PROVIDER_SYMBOLS);
    let discovery_limit = auto::automatic_scan_limit(max_files);
    let (paths, scan_truncated) = if automatic_only {
        workspace.source_files_background(path, discovery_limit)?
    } else {
        workspace.source_files(path, discovery_limit)?
    };
    let mut assignments =
        BTreeMap::<String, (ProviderCandidate, PathBuf, Vec<(String, SemanticLanguage)>)>::new();
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
        assignments
            .entry(provider.id.to_owned())
            .or_insert_with(|| (provider, executable, Vec::new()))
            .2
            .push((source_path, language));
    }
    let assignment_truncated = auto::trim_provider_assignments(&mut assignments, max_files);

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
                failures.push(format!("{}: {}", provider.id, render(&error)));
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
                            failures.push(format!(
                                "{}: warm session failed: {}",
                                provider.id,
                                render(&error)
                            ));
                        }
                    }
                    Err(error) => failures.push(format!(
                        "{}: warm session failed: {}",
                        provider.id,
                        render(&error)
                    )),
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
                    failures.push(format!("{}: {}", provider.id, render(&primary_error)));
                    continue;
                };
                if let Err(error) =
                    authorize_provider_session(workspace, alternate, &alternate_executable)
                {
                    failures.push(format!(
                        "{}: {}; fallback {} authorization failed: {}",
                        provider.id,
                        render(&primary_error),
                        alternate.id,
                        render(&error)
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
                            "{}: {}; fallback {}: {}",
                            provider.id,
                            render(&primary_error),
                            alternate.id,
                            render(&alternate_error)
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

pub(super) fn alternate_provider_for_sources(
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

pub(super) struct PreparedSemanticSource {
    pub(super) language: SemanticLanguage,
    pub(super) source: SourceDocument,
}

#[cfg(test)]
pub(super) fn prepare_sources(
    workspace: &Workspace,
    files: &[(String, SemanticLanguage)],
) -> Result<Vec<PreparedSemanticSource>> {
    prepare_sources_with_class(workspace, files, crate::resource::WorkClass::Interactive)
}

pub(super) fn prepare_sources_with_class(
    workspace: &Workspace,
    files: &[(String, SemanticLanguage)],
    work_class: crate::resource::WorkClass,
) -> Result<Vec<PreparedSemanticSource>> {
    crate::resource::parallel_io(files, |(path, language)| {
        let _cpu = crate::resource::cpu_work(work_class);
        workspace
            .load_source(path)
            .map(|source| PreparedSemanticSource {
                language: *language,
                source,
            })
    })?
    .into_iter()
    .collect()
}

pub(super) fn provider_revision(
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
    let mut fingerprints = files.iter().collect::<Vec<_>>();
    fingerprints.sort_by(|left, right| left.source.path.cmp(&right.source.path));
    let mut hasher = Sha256::new();
    hasher.update(b"semantic-provider-v2\n");
    hasher.update(provider.id.as_bytes());
    hasher.update(b"\n");
    hasher.update(executable_fingerprint.as_bytes());
    hasher.update(b"\n");
    hasher.update(max_symbols.to_string().as_bytes());
    hasher.update(b"\n");
    for (index, source) in fingerprints.into_iter().enumerate() {
        if index > 0 {
            hasher.update(b"\n");
        }
        hasher.update(source.source.path.as_bytes());
        hasher.update(b":");
        hasher.update(source.source.sha256.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}

#[derive(Clone)]
pub(super) struct DocumentSymbol {
    pub(super) name: String,
    pub(super) qualified_name: String,
    pub(super) kind: u64,
    pub(super) path: String,
    pub(super) line: u64,
    pub(super) character: u64,
}

pub(super) fn flatten_document_symbols(
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

pub(super) fn call_hierarchy_node(
    workspace: &Workspace,
    provider: &str,
    item: &Value,
    source_sha_by_path: &mut std::collections::HashMap<String, Option<String>>,
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
    let source_sha256 = source_sha_by_path
        .entry(relative.clone())
        .or_insert_with(|| {
            workspace
                .load_source(&relative)
                .ok()
                .map(|source| source.sha256)
        })
        .clone();
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

pub(super) fn implementation_locations(
    workspace: &Workspace,
    value: &Value,
) -> Vec<(String, u64, u64)> {
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

pub(super) fn call_hierarchy_candidate(kind: u64) -> bool {
    matches!(kind, 6 | 9 | 12)
}

pub(super) fn implementation_candidate(kind: u64) -> bool {
    matches!(kind, 5 | 6 | 11 | 12 | 23)
}

pub(super) fn semantic_node_id(
    provider: &str,
    path: &str,
    line: u64,
    character: u64,
    name: &str,
) -> String {
    let digest =
        Sha256::digest(format!("{provider}\n{path}\n{line}\n{character}\n{name}").as_bytes());
    format!("semantic:{:x}", digest)
}

pub(super) fn lsp_node_kind(kind: u64) -> NodeKind {
    match kind {
        2..=4 => NodeKind::Module,
        5 => NodeKind::Class,
        6 | 9 | 12 => NodeKind::Function,
        10 => NodeKind::Enum,
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

pub(super) fn provider_candidates(
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

pub(super) fn select_provider(
    workspace: &Workspace,
    language: SemanticLanguage,
) -> Option<(ProviderCandidate, PathBuf)> {
    provider_candidates(workspace, language).into_iter().next()
}

pub(super) fn provider_launch_args(
    workspace: &Workspace,
    provider: ProviderCandidate,
) -> Result<Vec<String>> {
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

#[cfg(test)]
#[path = "../../../tests/unit/semantics/provider.rs"]
mod tests;
