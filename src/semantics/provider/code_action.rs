use super::client::render_error_chain as render;
use super::navigation::{byte_column_to_lsp, capability_enabled};
use super::*;

const MAX_CODE_ACTION_EDITS: usize = 256;
const MAX_CODE_ACTION_PLAN_BYTES: usize = 64 * 1024;
const MAX_CODE_ACTION_RESULTS: usize = 64;
const MAX_QUICK_FIX_DIAGNOSTICS: usize = 16;
const QUICK_FIX_DIAGNOSTIC_WAIT: Duration = Duration::from_millis(250);

#[derive(Clone, Debug)]
struct LspTextEdit {
    start_line: u64,
    start_character: u64,
    end_line: u64,
    end_character: u64,
    new_text: String,
}

pub(crate) async fn organize_imports_plan(
    sessions: &SemanticSessionPool,
    workspace: &Workspace,
    path: &str,
) -> Result<Value> {
    if !workspace.exec_enabled() {
        bail!("LSP code actions require command execution; restart without --no-exec");
    }
    if !workspace.semantic_exec_enabled() {
        bail!("LSP code actions are disabled; restart without --no-semantic");
    }

    let source = workspace.load_source(path)?;
    let source_sha = source.sha256.clone();
    let language = language_for_path(&source.path)
        .ok_or_else(|| anyhow!("LSP organize imports does not support this source language"))?;
    let candidates = provider_candidates(workspace, language);
    if candidates.is_empty() {
        bail!(
            "no trusted LSP server is available for {}; organize imports has no syntax fallback",
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
    if !capability_enabled(&session.capabilities, "codeActionProvider") {
        bail!(
            "LSP server {} does not advertise codeActionProvider; organize imports will not guess",
            provider.id
        );
    }

    let (uri, document_sync) = session.sync_document(workspace, &source, language).await?;
    let end = document_end_position(&source.content, &session.position_encoding);
    let actions = session
        .request(
            "textDocument/codeAction",
            json!({
                "textDocument":{"uri":uri},
                "range":{"start":{"line":0,"character":0},"end":end},
                "context":{
                    "diagnostics":[],
                    "only":["source.organizeImports"],
                    "triggerKind":1
                }
            }),
        )
        .await
        .with_context(|| {
            format!(
                "LSP stage=code_action server={} action=inspect_provider_or_document",
                provider.id
            )
        })?;

    let actions = actions
        .as_array()
        .ok_or_else(|| anyhow!("LSP codeAction response must be an array"))?;
    let resolve_supported = session
        .capabilities
        .pointer("/codeActionProvider/resolveProvider")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut safe = Vec::new();
    let mut rejected_commands = 0usize;
    let mut unresolved = 0usize;

    for raw in actions {
        if raw.get("disabled").is_some_and(|value| !value.is_null()) {
            continue;
        }
        let kind = raw.get("kind").and_then(Value::as_str).unwrap_or_default();
        if !kind.starts_with("source.organizeImports") {
            continue;
        }
        let mut action = raw.clone();
        if has_provider_command(&action) {
            rejected_commands = rejected_commands.saturating_add(1);
            continue;
        }
        if action.get("edit").is_none() && resolve_supported && action.get("data").is_some() {
            action = session
                .request("codeAction/resolve", action)
                .await
                .with_context(|| {
                    format!(
                        "LSP stage=code_action_resolve server={} action=inspect_provider",
                        provider.id
                    )
                })?;
        }
        if has_provider_command(&action) {
            rejected_commands = rejected_commands.saturating_add(1);
            continue;
        }
        if let Some(edit) = action.get("edit").filter(|value| value.is_object()) {
            safe.push((
                action
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Organize Imports")
                    .to_owned(),
                edit.clone(),
            ));
        } else {
            unresolved = unresolved.saturating_add(1);
        }
    }

    if safe.is_empty() {
        bail!(
            "LSP server {} returned no edit-only organize-imports action; rejected_commands={} unresolved_or_editless={}. Provider commands are never executed by this planning surface.",
            provider.id,
            rejected_commands,
            unresolved
        );
    }
    if safe.len() != 1 {
        bail!(
            "LSP server {} returned {} edit-only organize-imports actions; wcode refuses to guess between ambiguous refactors",
            provider.id,
            safe.len()
        );
    }
    let (title, workspace_edit) = safe.pop().expect("exactly one safe action");
    let position_encoding = session.position_encoding.clone();
    drop(guard);

    let files = workspace_edit_to_guarded_files(
        workspace,
        &workspace_edit,
        &position_encoding,
        &source.path,
        &source_sha,
    )?;
    Ok(json!({
        "provider": format!("lsp:{}", provider.id),
        "precision": "semantic",
        "operation": "organize_imports_plan",
        "completeness": "all_or_nothing",
        "path": source.path,
        "title": title,
        "position_encoding": position_encoding,
        "session_reused": session_reused,
        "document_sync": document_sync,
        "files": files,
        "file_count": files.len(),
        "apply_tool": "apply_file_edits",
        "apply_arguments": {"files": files},
        "safety": {
            "provider_commands": "rejected",
            "resource_operations": "rejected",
            "external_paths": "rejected",
            "stale_revision": "rejected_by_sha",
            "ambiguous_actions": "rejected",
            "partial_or_truncated_plan": "rejected"
        }
    }))
}

pub(crate) async fn quick_fix_plan(
    sessions: &SemanticSessionPool,
    workspace: &Workspace,
    path: &str,
    line: u64,
    character: u64,
    max_results: usize,
) -> Result<Value> {
    if line == 0 || character == 0 {
        bail!("LSP quick-fix line and character are 1-based");
    }
    if !workspace.exec_enabled() {
        bail!("LSP code actions require command execution; restart without --no-exec");
    }
    if !workspace.semantic_exec_enabled() {
        bail!("LSP code actions are disabled; restart without --no-semantic");
    }

    let source = workspace.load_source(path)?;
    let source_sha = source.sha256.clone();
    let language = language_for_path(&source.path)
        .ok_or_else(|| anyhow!("LSP quick fix does not support this source language"))?;
    let provider_candidates = provider_candidates(workspace, language);
    if provider_candidates.is_empty() {
        bail!(
            "no trusted LSP server is available for {}; quick fix has no syntax fallback",
            language.as_str()
        );
    }

    let mut startup_failures = Vec::new();
    let mut selected = None;
    for (provider, executable) in provider_candidates {
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
    if !capability_enabled(&session.capabilities, "codeActionProvider") {
        bail!(
            "LSP server {} does not advertise codeActionProvider; quick fix will not guess",
            provider.id
        );
    }

    let (uri, document_sync) = session.sync_document(workspace, &source, language).await?;
    let lsp_character =
        byte_column_to_lsp(&source.content, line, character, &session.position_encoding)?;
    let position = json!({"line": line - 1, "character": lsp_character});
    if session.current_diagnostics(&source.path, &uri).is_none() {
        session
            .drain_notifications(QUICK_FIX_DIAGNOSTIC_WAIT)
            .await
            .with_context(|| {
                format!(
                    "LSP stage=diagnostics server={} action=collect_current_document_diagnostics",
                    provider.id
                )
            })?;
    }
    let Some((published, diagnostics_truncated)) = session.current_diagnostics(&source.path, &uri)
    else {
        bail!(
            "LSP server {} has no version-matched diagnostics for the current document; quick fix refuses stale or unversioned diagnostics",
            provider.id
        );
    };
    if diagnostics_truncated {
        bail!(
            "LSP diagnostics for the current document exceeded the bounded cache; quick fix refuses a partial diagnostic set"
        );
    }
    let matching = published
        .into_iter()
        .filter(|diagnostic| diagnostic_contains_position(diagnostic, line - 1, lsp_character))
        .collect::<Vec<_>>();
    if matching.is_empty() {
        bail!(
            "LSP server {} has no current diagnostic at the selected position",
            provider.id
        );
    }
    let diagnostic_limit = max_results.clamp(1, MAX_QUICK_FIX_DIAGNOSTICS);
    if matching.len() > diagnostic_limit {
        bail!(
            "LSP server {} has {} matching diagnostics, exceeding the bounded {}-diagnostic quick-fix limit; no partial plan was produced",
            provider.id,
            matching.len(),
            diagnostic_limit
        );
    }
    let diagnostic_count = matching.len();

    let actions = session
        .request(
            "textDocument/codeAction",
            json!({
                "textDocument":{"uri":uri},
                "range":{"start":position,"end":position},
                "context":{
                    "diagnostics":matching,
                    "only":["quickfix"],
                    "triggerKind":1
                }
            }),
        )
        .await
        .with_context(|| {
            format!(
                "LSP stage=quick_fix server={} action=inspect_provider_or_diagnostic",
                provider.id
            )
        })?;
    let actions = actions
        .as_array()
        .ok_or_else(|| anyhow!("LSP quick-fix codeAction response must be an array"))?;
    if actions.len() > MAX_CODE_ACTION_RESULTS {
        bail!(
            "LSP server {} returned {} quick-fix actions, exceeding the bounded {}-action limit",
            provider.id,
            actions.len(),
            MAX_CODE_ACTION_RESULTS
        );
    }
    let resolve_supported = session
        .capabilities
        .pointer("/codeActionProvider/resolveProvider")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut candidates = Vec::new();
    let mut rejected_commands = 0usize;
    let mut unresolved = 0usize;
    for raw in actions {
        if raw.get("disabled").is_some_and(|value| !value.is_null()) {
            continue;
        }
        let kind = raw.get("kind").and_then(Value::as_str).unwrap_or_default();
        if !kind.starts_with("quickfix") {
            continue;
        }
        let mut action = raw.clone();
        if has_provider_command(&action) {
            rejected_commands = rejected_commands.saturating_add(1);
            continue;
        }
        if action.get("edit").is_none() && resolve_supported && action.get("data").is_some() {
            action = session
                .request("codeAction/resolve", action)
                .await
                .with_context(|| {
                    format!(
                        "LSP stage=quick_fix_resolve server={} action=inspect_provider",
                        provider.id
                    )
                })?;
        }
        if has_provider_command(&action) {
            rejected_commands = rejected_commands.saturating_add(1);
            continue;
        }
        let Some(edit) = action.get("edit").filter(|value| value.is_object()) else {
            unresolved = unresolved.saturating_add(1);
            continue;
        };
        let title = action
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Quick Fix");
        let (title, _) = redact_sensitive_text(title);
        candidates.push((
            title.chars().take(300).collect::<String>(),
            action
                .get("isPreferred")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            edit.clone(),
        ));
    }
    let position_encoding = session.position_encoding.clone();
    drop(guard);

    let mut safe = Vec::new();
    let mut rejected_unsafe_edits = 0usize;
    for (title, preferred, edit) in candidates {
        match workspace_edit_to_guarded_files(
            workspace,
            &edit,
            &position_encoding,
            &source.path,
            &source_sha,
        ) {
            Ok(files) => safe.push((title, preferred, files)),
            Err(_) => rejected_unsafe_edits = rejected_unsafe_edits.saturating_add(1),
        }
    }
    if safe.is_empty() {
        bail!(
            "LSP server {} returned no safe edit-only quick fix; rejected_commands={} unresolved_or_editless={} rejected_unsafe_edits={}",
            provider.id,
            rejected_commands,
            unresolved,
            rejected_unsafe_edits
        );
    }
    let preferred = safe
        .iter()
        .map(|(_, preferred, _)| *preferred)
        .collect::<Vec<_>>();
    let selected_index = select_quick_fix_index(&preferred, provider.id)?;
    let (title, preferred, files) = safe.swap_remove(selected_index);
    Ok(json!({
        "provider": format!("lsp:{}", provider.id),
        "precision": "semantic",
        "operation": "quick_fix_plan",
        "completeness": "all_or_nothing",
        "path": source.path,
        "line": line,
        "character": character,
        "title": title,
        "preferred": preferred,
        "diagnostic_count": diagnostic_count,
        "diagnostic_revision": "exact_document_version",
        "position_encoding": position_encoding,
        "session_reused": session_reused,
        "document_sync": document_sync,
        "files": files,
        "file_count": files.len(),
        "apply_tool": "apply_file_edits",
        "apply_arguments": {"files": files},
        "safety": {
            "provider_commands": "rejected",
            "resource_operations": "rejected",
            "external_paths": "rejected",
            "stale_revision": "rejected_by_sha_and_diagnostic_version",
            "ambiguous_actions": "rejected",
            "truncated_diagnostics": "rejected",
            "partial_or_truncated_plan": "rejected"
        }
    }))
}

fn select_quick_fix_index(preferred: &[bool], provider: &str) -> Result<usize> {
    if preferred.len() == 1 {
        return Ok(0);
    }
    let preferred = preferred
        .iter()
        .enumerate()
        .filter_map(|(index, preferred)| (*preferred).then_some(index))
        .collect::<Vec<_>>();
    if preferred.len() != 1 {
        bail!(
            "LSP server {provider} returned multiple safe quick fixes without one unique preferred action; wcode refuses to guess"
        );
    }
    Ok(preferred[0])
}

fn diagnostic_contains_position(diagnostic: &Value, line: u64, character: u64) -> bool {
    let Some(start_line) = diagnostic
        .pointer("/range/start/line")
        .and_then(Value::as_u64)
    else {
        return false;
    };
    let Some(start_character) = diagnostic
        .pointer("/range/start/character")
        .and_then(Value::as_u64)
    else {
        return false;
    };
    let Some(end_line) = diagnostic
        .pointer("/range/end/line")
        .and_then(Value::as_u64)
    else {
        return false;
    };
    let Some(end_character) = diagnostic
        .pointer("/range/end/character")
        .and_then(Value::as_u64)
    else {
        return false;
    };
    let position = (line, character);
    let start = (start_line, start_character);
    let end = (end_line, end_character);
    if start == end {
        position == start
    } else {
        start <= position && position < end
    }
}

fn has_provider_command(action: &Value) -> bool {
    action.get("command").is_some_and(|value| !value.is_null())
}

pub(super) fn workspace_edit_to_guarded_files(
    workspace: &Workspace,
    workspace_edit: &Value,
    encoding: &str,
    target_path: &str,
    target_sha: &str,
) -> Result<Vec<Value>> {
    let mut by_uri = BTreeMap::<String, Vec<LspTextEdit>>::new();
    let changes = workspace_edit.get("changes");
    let document_changes = workspace_edit.get("documentChanges");
    if changes.is_some() && document_changes.is_some() {
        bail!("LSP code action returned both changes and documentChanges; ambiguous mixed edit shapes are rejected");
    }
    if let Some(changes) = changes {
        let changes = changes
            .as_object()
            .ok_or_else(|| anyhow!("LSP code-action changes must be an object"))?;
        for (uri, edits) in changes {
            append_text_edits(&mut by_uri, uri, edits)?;
        }
    }
    if let Some(document_changes) = document_changes {
        let document_changes = document_changes
            .as_array()
            .ok_or_else(|| anyhow!("LSP code-action documentChanges must be an array"))?;
        for change in document_changes {
            let Some(text_document) = change.get("textDocument") else {
                bail!(
                    "LSP code action contains a resource operation; only TextDocumentEdit is allowed"
                );
            };
            let uri = text_document
                .get("uri")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("LSP TextDocumentEdit is missing textDocument.uri"))?;
            let edits = change
                .get("edits")
                .ok_or_else(|| anyhow!("LSP TextDocumentEdit is missing edits"))?;
            append_text_edits(&mut by_uri, uri, edits)?;
        }
    }
    if by_uri.is_empty() {
        bail!("LSP code action returned no text edits");
    }

    if by_uri.len() != 1 {
        bail!(
            "LSP code action touches {} files; organize-imports planning is intentionally restricted to exactly one target file",
            by_uri.len()
        );
    }

    let mut total_edits = 0usize;
    let mut total_plan_bytes = 0usize;
    let mut files = Vec::with_capacity(by_uri.len());
    for (uri, edits) in by_uri {
        total_edits = total_edits.saturating_add(edits.len());
        if total_edits > MAX_CODE_ACTION_EDITS {
            bail!(
                "LSP code action exceeds the {}-edit plan limit; no partial plan was produced",
                MAX_CODE_ACTION_EDITS
            );
        }
        let path = workspace_path_for_uri(workspace, &uri)?;
        if path != target_path {
            bail!("LSP code action attempted to edit a file other than the requested source");
        }
        let source = workspace.load_source(&path)?;
        if source.sha256 != target_sha {
            bail!("semantic code action target changed while LSP was computing edits; rerun organize_imports_plan");
        }
        let guarded = guarded_file_edit(&source.content, &edits, encoding)?;
        let old_text = guarded["old_text"].as_str().unwrap_or_default();
        let new_text = guarded["new_text"].as_str().unwrap_or_default();
        total_plan_bytes = total_plan_bytes
            .saturating_add(old_text.len())
            .saturating_add(new_text.len());
        if total_plan_bytes > MAX_CODE_ACTION_PLAN_BYTES {
            bail!(
                "LSP code action guarded plan exceeds the {}-byte limit; no partial plan was produced",
                MAX_CODE_ACTION_PLAN_BYTES
            );
        }
        files.push(json!({
            "path": path,
            "expected_sha256": source.sha256,
            "edits": [guarded]
        }));
    }
    Ok(files)
}

fn append_text_edits(
    by_uri: &mut BTreeMap<String, Vec<LspTextEdit>>,
    uri: &str,
    edits: &Value,
) -> Result<()> {
    let edits = edits
        .as_array()
        .ok_or_else(|| anyhow!("LSP code-action edits must be an array"))?;
    for edit in edits {
        let range = edit
            .get("range")
            .ok_or_else(|| anyhow!("LSP code-action text edit is missing range"))?;
        let start_line = range
            .pointer("/start/line")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("LSP code-action edit start.line is missing"))?;
        let start_character = range
            .pointer("/start/character")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("LSP code-action edit start.character is missing"))?;
        let end_line = range
            .pointer("/end/line")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("LSP code-action edit end.line is missing"))?;
        let end_character = range
            .pointer("/end/character")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("LSP code-action edit end.character is missing"))?;
        if (end_line, end_character) < (start_line, start_character) {
            bail!("LSP code action produced a reversed text range");
        }
        let new_text = edit
            .get("newText")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("LSP code-action text edit is missing newText"))?;
        by_uri.entry(uri.to_owned()).or_default().push(LspTextEdit {
            start_line,
            start_character,
            end_line,
            end_character,
            new_text: new_text.to_owned(),
        });
    }
    Ok(())
}

fn guarded_file_edit(content: &str, edits: &[LspTextEdit], encoding: &str) -> Result<Value> {
    let starts = line_starts(content);
    let mut ranges = Vec::with_capacity(edits.len());
    for edit in edits {
        let start = lsp_position_to_offset(
            content,
            &starts,
            edit.start_line,
            edit.start_character,
            encoding,
        )?;
        let end = lsp_position_to_offset(
            content,
            &starts,
            edit.end_line,
            edit.end_character,
            encoding,
        )?;
        if end < start {
            bail!("LSP code action produced a reversed byte range");
        }
        ranges.push((
            start,
            end,
            edit.start_line,
            edit.end_line,
            edit.new_text.as_str(),
        ));
    }
    ranges.sort_unstable_by_key(|range| range.0);
    for pair in ranges.windows(2) {
        if pair[0].1 > pair[1].0
            || (pair[0].0 == pair[0].1 && pair[1].0 == pair[1].1 && pair[0].0 == pair[1].0)
        {
            bail!("LSP code action produced overlapping or ambiguous text edits");
        }
    }

    let first_line = ranges
        .iter()
        .map(|range| range.2)
        .min()
        .ok_or_else(|| anyhow!("LSP code action returned no text edits"))?;
    let last_line = ranges
        .iter()
        .map(|range| range.3)
        .max()
        .ok_or_else(|| anyhow!("LSP code action returned no text edits"))?;
    let start_line =
        usize::try_from(first_line).map_err(|_| anyhow!("LSP start line is too large"))?;
    let end_line = usize::try_from(last_line).map_err(|_| anyhow!("LSP end line is too large"))?;
    let block_start = *starts
        .get(start_line)
        .ok_or_else(|| anyhow!("LSP code-action start line is outside the source file"))?;
    let block_end = starts.get(end_line + 1).copied().unwrap_or(content.len());
    if block_end <= block_start {
        bail!(
            "LSP code action edits an empty terminal line; wcode cannot build a non-empty guarded anchor"
        );
    }
    let old_text = &content[block_start..block_end];

    let mut new_text = String::with_capacity(old_text.len());
    let mut copied = block_start;
    for (start, end, _, _, replacement) in ranges {
        if start < block_start || end > block_end {
            bail!("LSP code-action edit escaped its guarded source block");
        }
        new_text.push_str(&content[copied..start]);
        new_text.push_str(replacement);
        copied = end;
    }
    new_text.push_str(&content[copied..block_end]);

    if new_text == old_text {
        bail!("LSP code action produced no effective text change");
    }
    let (_, old_redacted) = redact_sensitive_text(old_text);
    let (_, new_redacted) = redact_sensitive_text(&new_text);
    if old_redacted || new_redacted {
        bail!("LSP code action intersects sensitive source; guarded edit is withheld");
    }

    Ok(json!({
        "old_text": old_text,
        "new_text": new_text,
        "start_line": first_line + 1,
        "end_line": last_line + 1
    }))
}

fn line_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(
        content
            .match_indices('\n')
            .map(|(index, _)| index.saturating_add(1)),
    );
    starts
}

fn lsp_position_to_offset(
    content: &str,
    starts: &[usize],
    line: u64,
    character: u64,
    encoding: &str,
) -> Result<usize> {
    let line = usize::try_from(line).map_err(|_| anyhow!("LSP line is too large"))?;
    let start = *starts
        .get(line)
        .ok_or_else(|| anyhow!("LSP code-action line is outside the source file"))?;
    let end = starts.get(line + 1).copied().unwrap_or(content.len());
    let raw = content[start..end]
        .strip_suffix('\n')
        .unwrap_or(&content[start..end]);
    let text = raw.strip_suffix('\r').unwrap_or(raw);
    let column = strict_lsp_byte_offset(text, character, encoding)?;
    Ok(start.saturating_add(column))
}

fn strict_lsp_byte_offset(text: &str, character: u64, encoding: &str) -> Result<usize> {
    let target = usize::try_from(character).map_err(|_| anyhow!("LSP character is too large"))?;
    match encoding {
        "utf-8" => {
            if target > text.len() || !text.is_char_boundary(target) {
                bail!("LSP UTF-8 character is outside a character boundary");
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
                .ok_or_else(|| anyhow!("LSP UTF-32 character is outside the source line"))
        }
        _ => {
            let mut units = 0usize;
            for (index, ch) in text.char_indices() {
                if units == target {
                    return Ok(index);
                }
                let next = units.saturating_add(ch.len_utf16());
                if target < next {
                    bail!("LSP UTF-16 character splits a surrogate pair");
                }
                units = next;
            }
            if units == target {
                Ok(text.len())
            } else {
                bail!("LSP UTF-16 character is outside the source line")
            }
        }
    }
}

fn workspace_path_for_uri(workspace: &Workspace, uri: &str) -> Result<String> {
    let url = Url::parse(uri).context("LSP code action returned an invalid URI")?;
    if url.scheme() != "file" {
        bail!("LSP code action returned a non-file URI; external/resource edits are rejected");
    }
    let file = url
        .to_file_path()
        .map_err(|_| anyhow!("LSP code-action URI could not be converted to a file path"))?;
    let canonical = file
        .canonicalize()
        .with_context(|| format!("LSP code-action target {} does not exist", file.display()))?;
    if !canonical.starts_with(workspace.root()) {
        bail!("LSP code action attempted to edit a file outside the selected workspace");
    }
    let relative = canonical
        .strip_prefix(workspace.root())
        .map_err(|_| anyhow!("LSP code-action target escaped the selected workspace"))?;
    Ok(relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

fn document_end_position(content: &str, encoding: &str) -> Value {
    let lines = content.split('\n').collect::<Vec<_>>();
    let line = lines.len().saturating_sub(1);
    let text = lines.last().copied().unwrap_or_default();
    let character = match encoding {
        "utf-8" => text.len(),
        "utf-32" => text.chars().count(),
        _ => text.encode_utf16().count(),
    };
    json!({"line":line,"character":character})
}

#[cfg(test)]
#[path = "../../../tests/unit/semantics/code_action.rs"]
mod tests;
