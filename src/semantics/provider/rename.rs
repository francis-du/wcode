use super::client::render_error_chain as render;
use super::navigation::{byte_column_to_lsp, capability_enabled};
use super::*;

const MAX_RENAME_FILES: usize = 64;
const MAX_RENAME_EDITS_PER_FILE: usize = 128;
const MAX_RENAME_GUARDED_EDITS: usize = 512;

#[derive(Clone, Debug)]
struct LspRenameEdit {
    line: u64,
    start_character: u64,
    end_character: u64,
    new_text: String,
}

pub(crate) struct RenamePlanRequest<'a> {
    pub(crate) path: &'a str,
    pub(crate) line: u64,
    pub(crate) character: u64,
    pub(crate) old_name: &'a str,
    pub(crate) new_name: &'a str,
    pub(crate) max_files: usize,
}

pub(super) struct GuardedRenameRequest<'a> {
    pub(super) old_name: &'a str,
    pub(super) new_name: &'a str,
    pub(super) target_path: &'a str,
    pub(super) target_sha: &'a str,
    pub(super) encoding: &'a str,
    pub(super) max_files: usize,
}

pub(crate) async fn rename_plan(
    sessions: &SemanticSessionPool,
    workspace: &Workspace,
    request: RenamePlanRequest<'_>,
) -> Result<Value> {
    let RenamePlanRequest {
        path,
        line,
        character,
        old_name,
        new_name,
        max_files,
    } = request;
    if line == 0 || character == 0 {
        bail!("LSP rename line and character are 1-based");
    }
    validate_rename_name(old_name, "old")?;
    validate_rename_name(new_name, "new")?;
    if old_name == new_name {
        bail!("semantic rename requires a different new_name");
    }
    if !workspace.exec_enabled() {
        bail!("LSP rename requires command execution; restart without --no-exec");
    }
    if !workspace.semantic_exec_enabled() {
        bail!("LSP rename is disabled; restart without --no-semantic");
    }

    let source = workspace.load_source(path)?;
    let source_sha = source.sha256.clone();
    let language = language_for_path(&source.path)
        .ok_or_else(|| anyhow!("LSP rename does not support this source language"))?;
    let candidates = provider_candidates(workspace, language);
    if candidates.is_empty() {
        bail!(
            "no trusted LSP server is available for {}; semantic rename has no syntax fallback",
            language.as_str()
        );
    }

    let mut startup_failures = Vec::new();
    let mut selected = None;
    for (provider, executable) in candidates {
        if let Err(error) = authorize_provider_session(workspace, provider, &executable) {
            if startup_failures.is_empty() {
                return Err(error);
            }
            startup_failures.push(format!("{} authorization: {}", provider.id, render(&error)));
            break;
        }
        let handle = match sessions.handle(workspace, provider, &executable) {
            Ok(handle) => handle,
            Err(error) => {
                startup_failures.push(format!("{} session: {}", provider.id, render(&error)));
                continue;
            }
        };
        match handle.ensure_started(workspace, provider).await {
            Ok(reused) => {
                selected = Some((provider, executable, handle, reused));
                break;
            }
            Err(error) => {
                sessions.invalidate(workspace, provider, &executable);
                startup_failures.push(format!("{} initialize: {}", provider.id, render(&error)));
            }
        }
    }
    let (provider, _executable, handle, session_reused) = selected.ok_or_else(|| {
        anyhow!(
            "no installed LSP server could initialize for {}: {}",
            language.as_str(),
            startup_failures.join("; ")
        )
    })?;

    let mut guard = handle.lock().await;
    let session = guard
        .as_mut()
        .ok_or_else(|| anyhow!("LSP session failed to initialize"))?;
    if !capability_enabled(&session.capabilities, "renameProvider") {
        bail!(
            "LSP server {} does not advertise renameProvider; semantic rename will not guess",
            provider.id
        );
    }
    let (uri, document_sync) = session.sync_document(workspace, &source, language).await?;
    let position = json!({
        "line": line - 1,
        "character": byte_column_to_lsp(
            &source.content,
            line,
            character,
            &session.position_encoding
        )?
    });

    let prepare_checked = session
        .capabilities
        .pointer("/renameProvider/prepareProvider")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if prepare_checked {
        let prepared = session
            .request(
                "textDocument/prepareRename",
                json!({"textDocument":{"uri":uri},"position":position}),
            )
            .await
            .with_context(|| {
                format!(
                    "LSP stage=prepare_rename server={} action=inspect_symbol_or_provider",
                    provider.id
                )
            })?;
        if prepared.is_null() {
            bail!(
                "LSP server {} rejected prepareRename at the selected symbol",
                provider.id
            );
        }
    }

    let workspace_edit = session
        .request(
            "textDocument/rename",
            json!({
                "textDocument":{"uri":uri},
                "position":position,
                "newName":new_name
            }),
        )
        .await
        .with_context(|| {
            format!(
                "LSP stage=rename server={} action=inspect_symbol_or_provider",
                provider.id
            )
        })?;
    if workspace_edit.is_null() {
        bail!("LSP server {} returned no rename edits", provider.id);
    }
    let position_encoding = session.position_encoding.clone();
    drop(guard);

    let files = workspace_edit_to_guarded_files(
        workspace,
        &workspace_edit,
        GuardedRenameRequest {
            old_name,
            new_name,
            target_path: &source.path,
            target_sha: &source_sha,
            encoding: &position_encoding,
            max_files,
        },
    )?;
    let guarded_edits = files
        .iter()
        .filter_map(|file| file.get("edits").and_then(Value::as_array))
        .map(Vec::len)
        .sum::<usize>();
    Ok(json!({
        "provider": format!("lsp:{}", provider.id),
        "precision": "semantic",
        "operation": "rename_plan",
        "completeness": "all_or_nothing",
        "path": source.path,
        "line": line,
        "character": character,
        "old_name": old_name,
        "new_name": new_name,
        "position_encoding": position_encoding,
        "session_reused": session_reused,
        "document_sync": document_sync,
        "prepare_checked": prepare_checked,
        "files": files,
        "file_count": files.len(),
        "guarded_edit_count": guarded_edits,
        "apply_tool": "apply_file_edits",
        "apply_arguments": {"files": files},
        "safety": {
            "resource_operations": "rejected",
            "external_paths": "rejected",
            "stale_target_revision": "rejected",
            "range_text_mismatch": "rejected",
            "partial_or_truncated_rename": "rejected"
        }
    }))
}

fn validate_rename_name(name: &str, label: &str) -> Result<()> {
    if name.is_empty() || name.len() > 300 {
        bail!("semantic rename {label} name must contain 1-300 UTF-8 bytes");
    }
    if name
        .chars()
        .any(|ch| ch.is_control() || ch == '\n' || ch == '\r')
    {
        bail!("semantic rename {label} name must not contain control characters");
    }
    Ok(())
}

pub(super) fn workspace_edit_to_guarded_files(
    workspace: &Workspace,
    workspace_edit: &Value,
    request: GuardedRenameRequest<'_>,
) -> Result<Vec<Value>> {
    let GuardedRenameRequest {
        old_name,
        new_name,
        target_path,
        target_sha,
        encoding,
        max_files,
    } = request;
    let mut by_uri = BTreeMap::<String, Vec<LspRenameEdit>>::new();
    if let Some(changes) = workspace_edit.get("changes").and_then(Value::as_object) {
        for (uri, edits) in changes {
            append_lsp_rename_edits(&mut by_uri, uri, edits)?;
        }
    }
    if let Some(document_changes) = workspace_edit.get("documentChanges") {
        let document_changes = document_changes
            .as_array()
            .ok_or_else(|| anyhow!("LSP rename documentChanges must be an array"))?;
        for change in document_changes {
            let Some(text_document) = change.get("textDocument") else {
                bail!("LSP rename resource operations are not supported; only TextDocumentEdit is allowed");
            };
            let uri = text_document
                .get("uri")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    anyhow!("LSP rename TextDocumentEdit is missing textDocument.uri")
                })?;
            let edits = change
                .get("edits")
                .ok_or_else(|| anyhow!("LSP rename TextDocumentEdit is missing edits"))?;
            append_lsp_rename_edits(&mut by_uri, uri, edits)?;
        }
    }
    if by_uri.is_empty() {
        bail!("LSP rename returned no text edits");
    }

    let max_files = max_files.clamp(1, MAX_RENAME_FILES);
    if by_uri.len() > max_files {
        bail!(
            "LSP rename touches {} files, exceeding the bounded {max_files}-file rename limit; no partial rename plan was produced",
            by_uri.len()
        );
    }

    let mut files = Vec::with_capacity(by_uri.len());
    let mut total_guarded_edits = 0usize;
    for (uri, edits) in by_uri {
        let path = workspace_path_for_uri(workspace, &uri)?;
        let source = workspace.load_source(&path)?;
        if path == target_path && source.sha256 != target_sha {
            bail!("semantic rename target changed while LSP was computing edits; rerun semantic_rename_plan");
        }
        let guarded = guarded_line_edits(&source.content, &edits, old_name, new_name, encoding)?;
        if guarded.len() > MAX_RENAME_EDITS_PER_FILE {
            bail!(
                "semantic rename requires {} guarded edits in {}, exceeding the per-file {} edit limit",
                guarded.len(),
                path,
                MAX_RENAME_EDITS_PER_FILE
            );
        }
        total_guarded_edits = total_guarded_edits.saturating_add(guarded.len());
        if total_guarded_edits > MAX_RENAME_GUARDED_EDITS {
            bail!(
                "semantic rename exceeds the {} guarded-edit transaction limit; no partial plan was produced",
                MAX_RENAME_GUARDED_EDITS
            );
        }
        files.push(json!({
            "path": path,
            "expected_sha256": source.sha256,
            "edits": guarded
        }));
    }
    Ok(files)
}

fn append_lsp_rename_edits(
    by_uri: &mut BTreeMap<String, Vec<LspRenameEdit>>,
    uri: &str,
    edits: &Value,
) -> Result<()> {
    let edits = edits
        .as_array()
        .ok_or_else(|| anyhow!("LSP rename edits must be an array"))?;
    for edit in edits {
        let range = edit
            .get("range")
            .ok_or_else(|| anyhow!("LSP rename text edit is missing range"))?;
        let start_line = range
            .pointer("/start/line")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("LSP rename edit start.line is missing"))?;
        let end_line = range
            .pointer("/end/line")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("LSP rename edit end.line is missing"))?;
        if start_line != end_line {
            bail!("LSP rename produced a multi-line text edit; fail closed instead of guessing a guarded anchor");
        }
        let start_character = range
            .pointer("/start/character")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("LSP rename edit start.character is missing"))?;
        let end_character = range
            .pointer("/end/character")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("LSP rename edit end.character is missing"))?;
        if end_character <= start_character {
            bail!("LSP rename produced an empty or reversed text range");
        }
        let new_text = edit
            .get("newText")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("LSP rename text edit is missing newText"))?;
        by_uri
            .entry(uri.to_owned())
            .or_default()
            .push(LspRenameEdit {
                line: start_line,
                start_character,
                end_character,
                new_text: new_text.to_owned(),
            });
    }
    Ok(())
}

fn guarded_line_edits(
    content: &str,
    edits: &[LspRenameEdit],
    old_name: &str,
    new_name: &str,
    encoding: &str,
) -> Result<Vec<Value>> {
    let mut by_line = BTreeMap::<u64, Vec<&LspRenameEdit>>::new();
    for edit in edits {
        by_line.entry(edit.line).or_default().push(edit);
    }
    let mut guarded = Vec::with_capacity(by_line.len());
    for (zero_line, mut line_edits) in by_line {
        let original = source_line(content, zero_line)?;
        let mut ranges = Vec::with_capacity(line_edits.len());
        for edit in line_edits.drain(..) {
            if !rename_text_matches_requested(&edit.new_text, new_name) {
                bail!(
                    "LSP rename proposed text {:?} that does not match requested new_name {:?}",
                    edit.new_text,
                    new_name
                );
            }
            let start = strict_lsp_byte_offset(original, edit.start_character, encoding)?;
            let end = strict_lsp_byte_offset(original, edit.end_character, encoding)?;
            if end <= start {
                bail!("LSP rename produced an empty or reversed byte range");
            }
            let selected = &original[start..end];
            if !rename_text_matches_requested(selected, old_name) {
                bail!(
                    "LSP rename range text {:?} does not match the resolved symbol name {:?}; source may be stale",
                    selected,
                    old_name
                );
            }
            ranges.push((start, end, edit.new_text.as_str()));
        }
        ranges.sort_unstable_by_key(|range| range.0);
        for pair in ranges.windows(2) {
            if pair[0].1 > pair[1].0 {
                bail!(
                    "LSP rename produced overlapping edits on line {}",
                    zero_line + 1
                );
            }
        }
        let mut updated = String::with_capacity(original.len());
        let mut copied = 0usize;
        for (start, end, replacement) in ranges {
            updated.push_str(&original[copied..start]);
            updated.push_str(replacement);
            copied = end;
        }
        updated.push_str(&original[copied..]);
        let (_, original_redacted) = redact_sensitive_text(original);
        let (_, updated_redacted) = redact_sensitive_text(&updated);
        if original_redacted || updated_redacted {
            bail!(
                "semantic rename intersects sensitive source on line {}; guarded edit is withheld",
                zero_line + 1
            );
        }
        guarded.push(json!({
            "old_text": original,
            "new_text": updated,
            "start_line": zero_line + 1,
            "end_line": zero_line + 1
        }));
    }
    Ok(guarded)
}

fn rename_text_matches_requested(text: &str, requested: &str) -> bool {
    text == requested
        || text.strip_prefix("r#") == Some(requested)
        || text.strip_prefix('@') == Some(requested)
        || text.strip_prefix('#') == Some(requested)
}

pub(super) fn source_line(content: &str, zero_line: u64) -> Result<&str> {
    let index = usize::try_from(zero_line).map_err(|_| anyhow!("LSP rename line is too large"))?;
    let raw = content
        .split('\n')
        .nth(index)
        .ok_or_else(|| anyhow!("LSP rename line is outside the current source file"))?;
    Ok(raw.strip_suffix('\r').unwrap_or(raw))
}

pub(super) fn strict_lsp_byte_offset(text: &str, character: u64, encoding: &str) -> Result<usize> {
    let target =
        usize::try_from(character).map_err(|_| anyhow!("LSP rename character is too large"))?;
    match encoding {
        "utf-8" => {
            if target > text.len() || !text.is_char_boundary(target) {
                bail!("LSP rename UTF-8 character is outside a character boundary");
            }
            Ok(target)
        }
        "utf-32" => {
            if target == text.chars().count() {
                return Ok(text.len());
            }
            text.char_indices()
                .nth(target)
                .map(|(index, _)| index)
                .ok_or_else(|| anyhow!("LSP rename UTF-32 character is outside the source line"))
        }
        _ => {
            let mut units = 0usize;
            for (index, ch) in text.char_indices() {
                if units == target {
                    return Ok(index);
                }
                let next = units.saturating_add(ch.len_utf16());
                if target < next {
                    bail!("LSP rename UTF-16 character splits a surrogate pair");
                }
                units = next;
            }
            if units == target {
                Ok(text.len())
            } else {
                bail!("LSP rename UTF-16 character is outside the source line")
            }
        }
    }
}

pub(super) fn workspace_path_for_uri(workspace: &Workspace, uri: &str) -> Result<String> {
    let url = Url::parse(uri).context("LSP rename returned an invalid URI")?;
    if url.scheme() != "file" {
        bail!("LSP rename returned non-file URI {uri:?}; external/resource edits are rejected");
    }
    let file = url
        .to_file_path()
        .map_err(|_| anyhow!("LSP rename URI could not be converted to a file path"))?;
    let canonical = file.canonicalize().with_context(|| {
        format!(
            "LSP rename target {} is not an existing workspace file",
            file.display()
        )
    })?;
    if !canonical.starts_with(workspace.root()) {
        bail!("LSP rename attempted to edit a file outside the selected workspace");
    }
    let relative = canonical
        .strip_prefix(workspace.root())
        .map_err(|_| anyhow!("LSP rename target escaped the selected workspace"))?;
    Ok(relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}
