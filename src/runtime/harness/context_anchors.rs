use super::*;
use crate::harness::harness_retrieval::{
    query_contains_failure_trace, query_requests_comment_context,
};
use std::path::{Component, Path};

const MAX_ANCHORS: usize = 4;
const MAX_ANCHOR_CHARS: usize = 1_600;

#[derive(Debug)]
struct Anchor {
    path: String,
    line: Option<usize>,
    column: Option<usize>,
}

// Locations are evidence supplied by the caller, never an authorization grant
// or proof that the named stack frame is the root cause of a failure.
fn query_anchors(query: &str) -> Vec<Anchor> {
    let mut seen = BTreeSet::new();
    let mut anchors = Vec::new();
    // Structured line markers belong to this physical diagnostic line, never
    // to a later stack frame or an unrelated path elsewhere in the prompt.
    for line in query.lines() {
        let words = anchor_words(line);
        for (index, word) in words.iter().enumerate() {
            let Some(mut anchor) = parse_anchor_word(word) else {
                continue;
            };
            let following = |offset| {
                words
                    .get(index + offset)
                    .copied()
                    .unwrap_or("")
                    .trim_matches([',', ';', ':'])
            };
            let line_offset = if index > 0 && words[index - 1] == "File" && following(1) == "line" {
                Some(2)
            } else if following(1) == "on" && following(2) == "line" {
                Some(3)
            } else {
                None
            };
            if let Some(offset) = line_offset {
                // A recognized but invalid location stays invalid; it must not
                // silently become a valid line-one inspection.
                anchor.line = Some(location_number(following(offset)));
            }
            let identity = (anchor.path.clone(), anchor.line);
            if seen.insert(identity.clone()) {
                anchors.push(anchor);
                if anchors.len() == MAX_ANCHORS {
                    return anchors;
                }
            } else if anchor.column.is_some() {
                if let Some(existing) = anchors
                    .iter_mut()
                    .find(|existing| (existing.path.clone(), existing.line) == identity)
                {
                    if existing.column.is_none() {
                        existing.column = anchor.column;
                    }
                }
            }
        }
    }
    anchors
}

pub(super) fn diagnostic_locations(query: &str) -> Vec<Value> {
    query_anchors(query)
        .into_iter()
        .filter(|anchor| anchor.line.is_some())
        .map(|anchor| {
            json!({
                "path": anchor.path,
                "line": anchor.line,
                "column": anchor.column,
                "precision": "diagnostic_text"
            })
        })
        .collect()
}

// Keep quoted paths (including whitespace) as one token. Do not decode escapes,
// URLs or shell syntax. Malformed quotes are discarded as a whole so a URL or
// out-of-scope path cannot be reinterpreted as a local filename suffix.
fn anchor_words(line: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut quote_allowed = true;
    for (index, character) in line.char_indices() {
        if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            }
        } else if character.is_whitespace()
            || matches!(
                character,
                '，' | '。'
                    | '；'
                    | '、'
                    | '：'
                    | '（'
                    | '）'
                    | '【'
                    | '】'
                    | '「'
                    | '」'
                    | '“'
                    | '”'
            )
        {
            if start < index {
                words.push(&line[start..index]);
            }
            start = index + character.len_utf8();
            quote_allowed = true;
        } else if quote_allowed && matches!(character, '\"' | '\'' | '`') {
            quote = Some(character);
            quote_allowed = false;
        } else if !matches!(character, '(' | '[') {
            quote_allowed = false;
        }
    }
    if start < line.len() && quote.is_none() {
        words.push(&line[start..]);
    }
    words
}

fn unquote_location(value: &str) -> &str {
    for quote in ['\"', '\'', '`'] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner;
        }
    }
    value
}

fn location_number(value: &str) -> usize {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return 0;
    }
    value.parse().unwrap_or(0)
}

fn parse_anchor_word(word: &str) -> Option<Anchor> {
    let mut word = word.trim_end_matches([',', ';', ':']);
    // Strip balanced presentation wrappers before unquoting, but retain the
    // compiler's trailing (line,column) because that is part of the location.
    loop {
        let inner = [('(', ')'), ('[', ']')]
            .into_iter()
            .find_map(|(open, close)| {
                word.strip_prefix(open)
                    .and_then(|rest| rest.strip_suffix(close))
            });
        match inner {
            Some(inner) => word = inner,
            None => break,
        }
    }
    let word = word
        .trim_start_matches(['(', '['])
        .trim_end_matches([']', ',', ';', ':']);
    if word.contains("://") || word.len() > 512 || word.chars().any(char::is_control) {
        return None;
    }
    let word = unquote_location(word);
    let (path, line, column) = if let Some((path, point)) = word
        .rsplit_once('(')
        .filter(|(_, point)| point.ends_with(')'))
    {
        let coordinates = point.trim_end_matches(')').split(',').collect::<Vec<_>>();
        let valid =
            coordinates.len() <= 2 && coordinates.iter().all(|number| location_number(number) > 0);
        (
            path,
            Some(if valid {
                location_number(coordinates[0])
            } else {
                0
            }),
            valid
                .then(|| coordinates.get(1).map(|value| location_number(value)))
                .flatten(),
        )
    } else {
        let word = word.trim_end_matches(')');
        if let Some((path, line)) = word.split_once("#L") {
            (path, Some(location_number(line)), None)
        } else {
            let mut path = word;
            let mut numbers = Vec::new();
            for _ in 0..2 {
                let Some((prefix, suffix)) = path.rsplit_once(':') else {
                    break;
                };
                if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
                    break;
                }
                numbers.push(location_number(suffix));
                path = prefix;
            }
            (
                path,
                numbers.last().copied(),
                (numbers.len() == 2).then(|| numbers[0]),
            )
        }
    };
    let path = unquote_location(path).replace('\\', "/");
    let filename = path.rsplit('/').next()?;
    let extension = filename
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    let canonical_source = crate::semantic_provider::language_for_path(&path).is_some();
    let auxiliary_source_or_config = [
        "kt", "kts", "scala", "vue", "svelte", "json", "jsonc", "toml", "yaml", "yml", "md", "txt",
        "env", "ini", "cfg", "xml", "sql", "proto", "graphql", "gql",
    ]
    .contains(&extension.as_str());
    (canonical_source || auxiliary_source_or_config).then_some(Anchor { path, line, column })
}

fn relative_anchor(workspace: &Workspace, path: &str) -> Option<String> {
    if path.chars().any(char::is_control) {
        return None;
    }
    // Reject traversal rather than normalizing across a possible symlink.
    if Path::new(path)
        .components()
        .any(|part| part == Component::ParentDir)
    {
        return None;
    }
    let root = workspace.root().to_str()?.replace('\\', "/");
    // Windows canonical roots may carry a verbatim prefix. Compare both
    // spellings as portable strings, then let Workspace validate the relative path.
    let root = if cfg!(windows) {
        root.strip_prefix("//?/").unwrap_or(&root)
    } else {
        &root
    };
    let path = if cfg!(windows) {
        path.strip_prefix("//?/").unwrap_or(path)
    } else {
        path
    };
    let relative = if path.starts_with('/') || path.as_bytes().get(1) == Some(&b':') {
        path.strip_prefix(&format!("{}/", root.trim_end_matches('/')))?
    } else {
        path
    };
    let relative = relative.trim_start_matches("./");
    if relative.is_empty() || relative.contains(':') {
        return None;
    }
    Some(relative.to_owned())
}

// Resolve exact locations before considering repository-wide symbol discovery.
// Design, confirmed semantics, verification references and risks still use the
// canonical context builder; only redundant lexical discovery is bypassed.
pub(super) fn build_context(
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &Workspace,
    known_checks: &HashSet<String>,
    request: &SoftwareContextRequest,
) -> Result<(SoftwareContext, Vec<Value>)> {
    let mut symbols = Vec::new();
    let anchors = retrieve(
        harness,
        workspace_id,
        workspace,
        &request.query,
        &mut symbols,
    )?;
    let context = harness.intelligence.software_context_with_symbols(
        workspace_id,
        workspace,
        &harness.code_index,
        known_checks,
        request,
        (!anchors.is_empty()).then_some(symbols.as_slice()),
    )?;
    Ok((context, anchors))
}

pub(super) fn repo_map(
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &Workspace,
    query: &str,
    context: &SoftwareContext,
    no_anchors: bool,
    max_items: usize,
) -> Result<Value> {
    if no_anchors
        || !context.scopes.is_empty()
        || query_needs_semantic_relationships(query)
        || query_requests_architecture_change(query)
        || query_requests_comment_context(query)
        || query_contains_failure_trace(query)
    {
        return harness.ranked_repo_map(workspace_id, workspace, query, context, max_items);
    }
    Ok(json!({
        "provider": "tree-sitter", "precision": "syntax", "items": [],
        "deferred": true, "truncated": false, "files_indexed": 0,
        "guidance": "Repository graph expansion is deferred for this explicit location. Request callers, impact or architecture context when cross-file relationships are needed."
    }))
}

fn retrieve(
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &Workspace,
    query: &str,
    symbol_targets: &mut Vec<Value>,
) -> Result<Vec<Value>> {
    let anchors = query_anchors(query);
    if anchors.is_empty() {
        return Ok(Vec::new());
    }
    workspace.path_info(".")?;
    let mut records = Vec::new();
    let mut targets = Vec::new();
    for anchor in anchors {
        let mut record = json!({
            "path": anchor.path,
            "line": anchor.line,
            "status": "unavailable",
        });
        let Some(path) = relative_anchor(workspace, &anchor.path) else {
            record["status"] = json!("outside_boundary");
            records.push(record);
            continue;
        };
        let line = anchor.line.unwrap_or(1);
        if line == 0 {
            record["status"] = json!("invalid_location");
            records.push(record);
            continue;
        }
        // The diagnostic line comes first, so bounded prefix trimming cannot
        // replace the actual failing location with a distant function header.
        let view = match workspace.read_file(&path, line, Some(line.saturating_add(12))) {
            Ok(view) if line <= view.total_lines => view,
            _ => {
                records.push(record);
                continue;
            }
        };
        let info = match workspace.path_info(&path) {
            Ok(info) if info.sha256.as_deref() == Some(view.sha256.as_str()) => info,
            _ => {
                record["status"] = json!("changed_during_read");
                records.push(record);
                continue;
            }
        };
        let outline = harness
            .file_outline(workspace_id, workspace, &path, 1_000)
            .ok();
        let selected = anchor.line.and_then(|line| {
            outline
                .as_ref()
                .filter(|outline| outline["sha256"] == view.sha256)?
                .get("symbols")?
                .as_array()?
                .iter()
                .filter(|symbol| {
                    let start = symbol.pointer("/range/start_line").and_then(Value::as_u64);
                    let end = symbol.pointer("/range/end_line").and_then(Value::as_u64);
                    let end = end.map(|end| {
                        end.saturating_sub(u64::from(
                            symbol.pointer("/range/end_exclusive") == Some(&Value::Bool(true))
                                && symbol.pointer("/range/end_column").and_then(Value::as_u64)
                                    == Some(1),
                        ))
                    });
                    start
                        .zip(end)
                        .is_some_and(|(start, end)| start <= line as u64 && line as u64 <= end)
                })
                .min_by_key(|symbol| {
                    symbol["range"]["end_line"]
                        .as_u64()
                        .unwrap_or(u64::MAX)
                        .saturating_sub(symbol["range"]["start_line"].as_u64().unwrap_or(0))
                })
                .cloned()
        });
        let target = selected.unwrap_or_else(|| {
            json!({
                "id": null, "path": view.path, "kind": "file",
                "qualified_name": view.path, "signature": null,
                "provider": "workspace", "precision": "deterministic",
                "range": {"start_line": view.start_line, "end_line": view.end_line},
            })
        });
        if !targets.iter().any(|item: &Value| item == &target) {
            targets.push(target.clone());
        }
        record["path"] = json!(view.path);
        record["status"] = json!("resolved");
        record["file"] = json!({
            "path": view.path, "sha256": view.sha256,
            "size": info.size, "readonly": info.readonly,
            "reasons": ["explicit-location"],
        });
        let mut body = json!({
            "start_line": view.start_line, "end_line": view.end_line,
            "content": view.content, "redacted": view.redacted,
            "truncated": view.end_line < view.total_lines,
        });
        // Diagnostic excerpts are source, not prose. Reuse the same original-
        // prefix/line-range contract as direct symbol context.
        truncate_source_body(&mut body, MAX_ANCHOR_CHARS);
        record["source"] = json!({
            "id": target["id"], "path": view.path,
            "qualified_name": target["qualified_name"], "signature": target["signature"],
            "provider": "workspace", "precision": "deterministic",
            "sha256": view.sha256,
            "body": body,
            "calls": [],
        });
        records.push(record);
    }
    // Explicit missing/blocked locations must not turn unrelated lexical hits
    // into edit-ready targets. The repo map remains available for exploration.
    *symbol_targets = targets;
    Ok(records)
}

pub(super) fn merge(pack: &mut Value, mut records: Vec<Value>) {
    if records.is_empty() {
        return;
    }
    let mut sources = Vec::new();
    let mut files = Vec::new();
    for record in &mut records {
        if let Some(object) = record.as_object_mut() {
            if let Some(source) = object.remove("source") {
                sources.push(source);
            }
            if let Some(file) = object.remove("file") {
                if !files
                    .iter()
                    .any(|item: &Value| item["path"] == file["path"])
                {
                    files.push(file);
                }
            }
        }
    }
    for file in pack["files"].as_array().into_iter().flatten() {
        if !files.iter().any(|item| item["path"] == file["path"]) {
            files.push(file.clone());
        }
    }
    files.truncate(MAX_AGENT_FILES);
    pack["files"] = json!(files);
    pack["hot_source"] = json!(sources);
    pack["retrieval"] = json!({
        "strategy": "explicit-locations",
        "resolved": records.iter().filter(|record| record["status"] == "resolved").count(),
        "anchors": records,
        "guidance": "Locations are inspection anchors, not proven root causes. Missing or blocked anchors do not justify editing unrelated files."
    });
}

#[cfg(test)]
#[path = "../../../tests/unit/runtime/harness/context_anchors.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../tests/unit/runtime/harness/diagnostic_formats.rs"]
mod diagnostic_formats;
