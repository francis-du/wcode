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
}

// Locations are evidence supplied by the caller, never an authorization grant
// or proof that the named stack frame is the root cause of a failure.
fn query_anchors(query: &str) -> Vec<Anchor> {
    let mut seen = BTreeSet::new();
    query
        .split_whitespace()
        .filter_map(|word| {
            let word = word.trim_matches(['`', '\'', '"', '(', ')', '[', ']', ',', ';']);
            if word.contains("://") || word.len() > 512 {
                return None;
            }
            let word = word.trim_end_matches(':');
            let (path, line) = if let Some((path, line)) = word.split_once("#L") {
                (path, Some(line.parse::<usize>().unwrap_or(0)))
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
                    numbers.push(suffix.parse::<usize>().unwrap_or(0));
                    path = prefix;
                }
                (path, numbers.last().copied())
            };
            let path = path.replace('\\', "/");
            let filename = path.rsplit('/').next()?;
            let extension = filename.rsplit_once('.')?.1.to_ascii_lowercase();
            if ![
                "rs", "py", "js", "jsx", "ts", "tsx", "go", "c", "cc", "cpp", "h", "hpp", "cs",
                "java", "kt", "kts", "swift", "rb", "php", "lua", "ex", "exs", "sh", "bash",
                "dart", "ml", "mli", "r", "html", "css", "json", "toml", "yaml", "yml", "md",
                "txt", "env", "ini", "cfg", "xml", "sql", "vue", "svelte",
            ]
            .contains(&extension.as_str())
            {
                return None;
            }
            if !seen.insert((path.clone(), line)) {
                return None;
            }
            Some(Anchor { path, line })
        })
        .take(MAX_ANCHORS)
        .collect()
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
    let known_checks = harness.known_checks(workspace)?;
    let context = harness.intelligence.software_context_with_symbols(
        workspace_id,
        workspace,
        &harness.code_index,
        &known_checks,
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
) -> Result<Value> {
    if no_anchors
        || !context.scopes.is_empty()
        || query_needs_semantic_relationships(query)
        || query_requests_architecture_change(query)
        || query_requests_comment_context(query)
        || query_contains_failure_trace(query)
    {
        return harness.ranked_repo_map(
            workspace_id,
            workspace,
            query,
            context,
            MAX_AGENT_REPO_MAP,
        );
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
        record["source"] = json!({
            "id": target["id"], "path": view.path,
            "qualified_name": target["qualified_name"], "signature": target["signature"],
            "provider": "workspace", "precision": "deterministic",
            "sha256": view.sha256,
            "body": {
                "start_line": view.start_line, "end_line": view.end_line,
                "content": short_text(&view.content, MAX_ANCHOR_CHARS),
                "redacted": view.redacted,
                "truncated": view.end_line < view.total_lines
                    || view.content.chars().count() > MAX_ANCHOR_CHARS,
            },
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
