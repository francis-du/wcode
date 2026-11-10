use super::*;

pub(super) fn batch_succeeded(name: &str, value: &mut Value) -> bool {
    let key = match name {
        "read_files" => "files",
        "create_files" | "apply_file_edits" | "move_paths" => "results",
        _ => return true,
    };
    let Some(items) = value.get(key).and_then(Value::as_array) else {
        value["error"] = json!("batch did not return per-item outcomes");
        return false;
    };
    let succeeded = items.iter().filter(|item| item["ok"] == true).count();
    let failed = items.len().saturating_sub(succeeded);
    value["succeeded"] = json!(succeeded);
    value["failed"] = json!(failed);
    value["status"] = json!(if failed == 0 {
        "complete"
    } else if succeeded == 0 {
        "failed"
    } else {
        "partial"
    });
    if failed > 0 {
        value["retry_guidance"] = json!(
            "Successful items remain applied. Inspect outcomes and retry only failed items; refresh SHA preconditions before editing."
        );
    }
    failed == 0
}

const DEFAULT_COMMAND_TIMEOUT_SECONDS: u64 = 600;
const MAX_COMMAND_TIMEOUT_SECONDS: u64 = 1800;

pub(super) fn command_timeout_seconds(args: &Value) -> Result<u64, String> {
    if let Some(task_mode) = args.get("task_mode") {
        task_mode
            .as_bool()
            .ok_or("task_mode must be a boolean when provided")?;
    }
    match args.get("timeout_seconds") {
        None => Ok(DEFAULT_COMMAND_TIMEOUT_SECONDS),
        Some(value) => value
            .as_u64()
            .filter(|seconds| (1..=MAX_COMMAND_TIMEOUT_SECONDS).contains(seconds))
            .ok_or_else(|| "timeout_seconds must be an integer between 1 and 1800".to_owned()),
    }
}

/// The OS sandbox is opt-in per call; the default lane executes directly.
pub(super) fn command_sandbox_requested(args: &Value) -> Result<bool, String> {
    match args.get("sandbox") {
        None => Ok(false),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| "sandbox must be a boolean when provided".to_owned()),
    }
}

pub(super) fn preflight_sync_command(state: &AppState, args: &Value) -> Result<(), String> {
    let (_workspace_id, workspace) = selected_workspace(state, args)?;
    required_string(args, "program")?;
    command_arguments(args)?;
    command_environment(args)?;
    match args.get("cwd") {
        None => {}
        Some(value) => {
            value
                .as_str()
                .filter(|cwd| !cwd.is_empty())
                .ok_or("cwd must be a non-empty string when provided")?;
        }
    }
    command_timeout_seconds(args)?;
    command_sandbox_requested(args)?;
    // `--no-exec` is an explicit operator hardening choice, so it wins over a
    // session-wide command grant.
    if !workspace.exec_enabled() {
        return Err("command execution is disabled; restart without --no-exec or explicitly authorize all commands for this Workspace".to_owned());
    }
    Ok(())
}

pub(super) fn command_arguments(args: &Value) -> Result<Vec<String>, String> {
    match args.get("args") {
        None => Ok(Vec::new()),
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|_| "args must be an array of strings when provided".to_owned()),
    }
}

fn command_environment(args: &Value) -> Result<Vec<(String, String)>, String> {
    let Some(value) = args.get("env") else {
        return Ok(Vec::new());
    };
    let object = value
        .as_object()
        .ok_or("env must be an object of bounded string overrides when provided")?;
    if object.len() > 5 {
        return Err("env accepts at most 5 bounded overrides".to_owned());
    }
    object
        .iter()
        .map(|(key, value)| {
            value
                .as_str()
                .map(|value| (key.clone(), value.to_owned()))
                .ok_or_else(|| "env values must be strings".to_owned())
        })
        .collect()
}

fn bug_pattern_selection(args: &Value) -> Result<Option<(String, Vec<String>)>, String> {
    const GO_COMMON_BUGS: &[&str] = &[
        "nil_deref",
        "err_swallowed",
        "index_mismatch",
        "empty_test",
        "unguarded_subscript",
    ];
    let preset = string_arg(args, "preset");
    let pattern = string_arg(args, "pattern");
    if preset.is_some() && pattern.is_some() {
        return Err("preset and pattern are mutually exclusive".to_owned());
    }
    if let Some(preset) = preset {
        if preset != "go_common_bugs" {
            return Err("preset must be go_common_bugs".to_owned());
        }
        return Ok(Some((
            preset.to_owned(),
            GO_COMMON_BUGS
                .iter()
                .map(|pattern| (*pattern).to_owned())
                .collect(),
        )));
    }
    if let Some(pattern) = pattern {
        if !GO_COMMON_BUGS.contains(&pattern) {
            return Err(format!("unsupported bug pattern: {pattern}"));
        }
        return Ok(Some((pattern.to_owned(), vec![pattern.to_owned()])));
    }
    Ok(None)
}

fn search_request(name: &str, args: &Value) -> Result<crate::workspace::SearchRequest, String> {
    let key = match name {
        "search_code" => "query",
        "search_many" => "queries",
        _ => "patterns",
    };
    let queries = if name == "search_code" && args.get(key).is_some_and(Value::is_string) {
        vec![required_string(args, key)?.to_owned()]
    } else {
        string_array_arg(args, key, 32)?
    };
    let text = |key: &str, default: &str| -> Result<String, String> {
        match args.get(key) {
            None => Ok(default.to_owned()),
            Some(value) => value
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| format!("{key} must be a non-empty string")),
        }
    };
    let number = |key: &str, default: usize, min: usize, max: usize| -> Result<usize, String> {
        match args.get(key) {
            None => Ok(default),
            Some(value) => value
                .as_u64()
                .and_then(|v| usize::try_from(v).ok())
                .filter(|v| (min..=max).contains(v))
                .ok_or_else(|| format!("{key} must be an integer between {min} and {max}")),
        }
    };
    let default_mode = match name {
        "scan_patterns" => "regex",
        "search_code" if queries.len() == 1 => "auto",
        _ => "exact",
    };
    let auto_page = match args.get("auto_page") {
        None => false,
        Some(value) => value
            .as_bool()
            .ok_or_else(|| "auto_page must be a boolean".to_owned())?,
    };
    let default_max_results = if auto_page {
        2_000
    } else if name == "scan_patterns" {
        500
    } else {
        100
    };
    let max_results = if auto_page {
        number("max_results", 2_000, 1, 2_000)?
    } else {
        number("max_results", default_max_results, 1, 2_000)?
    };
    let include_comments = if name == "scan_patterns" {
        match args.get("include_comments") {
            None => false,
            Some(value) => value
                .as_bool()
                .ok_or_else(|| "include_comments must be a boolean".to_owned())?,
        }
    } else {
        true
    };
    Ok(crate::workspace::SearchRequest {
        queries,
        path: text("path", ".")?,
        mode: crate::workspace::SearchMode::parse(&text("mode", default_mode)?)
            .map_err(|e| e.to_string())?,
        context_lines: number(
            "context_lines",
            if name == "scan_patterns" { 2 } else { 0 },
            0,
            20,
        )?,
        include_comments,
        max_results,
        offset: number("offset", 0, 0, 10_000)?,
        output_mode: text("output_mode", "content")?,
    })
}

const GUIDANCE_EXCERPT_LIMIT: usize = 4_000;
const CONVENTION_FINDING_SAMPLE: usize = 16;

fn bound_string(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
    limit: usize,
    marker: &str,
) {
    let Some(text) = object.get(key).and_then(Value::as_str) else {
        return;
    };
    if text.chars().count() <= limit {
        return;
    }
    let bounded: String = text.chars().take(limit).collect();
    object.insert(key.to_owned(), Value::String(bounded));
    object.insert(marker.to_owned(), Value::Bool(true));
}

/// A repository profile carries every convention finding, every quality
/// provider reason and whole guidance excerpts. Those are what make the
/// default response far larger than the guidance an agent acts on, so the
/// summary keeps the counts, a bounded sample and bounded text, and declares
/// every omission. `detail=full` restores the complete projection.
fn compact_project_context(value: &mut Value) {
    if let Some(guidance) = value.get_mut("guidance").and_then(Value::as_array_mut) {
        for document in guidance {
            let Some(object) = document.as_object_mut() else {
                continue;
            };
            bound_string(
                object,
                "excerpt",
                GUIDANCE_EXCERPT_LIMIT,
                "excerpt_truncated",
            );
        }
    }
    if let Some(conventions) = value.get_mut("conventions").and_then(Value::as_object_mut) {
        if let Some(mut findings) = conventions
            .get_mut("findings")
            .and_then(Value::as_array_mut)
            .map(std::mem::take)
        {
            let total = findings.len();
            let mut counts = std::collections::BTreeMap::<String, (usize, Value)>::new();
            for finding in &findings {
                let code = finding
                    .get("code")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let severity = finding
                    .get("severity")
                    .cloned()
                    .unwrap_or(Value::String("warning".to_owned()));
                counts
                    .entry(code)
                    .and_modify(|entry| entry.0 += 1)
                    .or_insert((1, severity));
            }
            let mut by_code = counts
                .into_iter()
                .map(|(code, (count, severity))| json!({"code": code, "count": count, "severity": severity}))
                .collect::<Vec<_>>();
            by_code.sort_by(|left, right| {
                right["count"]
                    .as_u64()
                    .cmp(&left["count"].as_u64())
                    .then_with(|| left["code"].as_str().cmp(&right["code"].as_str()))
            });
            findings.truncate(CONVENTION_FINDING_SAMPLE);
            conventions.insert("findings".to_owned(), Value::Array(findings));
            conventions.insert("findings_by_code".to_owned(), Value::Array(by_code));
            conventions.insert("findings_total".to_owned(), Value::from(total));
            if total > CONVENTION_FINDING_SAMPLE {
                conventions.insert("findings_truncated".to_owned(), Value::Bool(true));
            }
        }
    }
    if let Some(languages) = value
        .get_mut("language_quality")
        .and_then(|quality| quality.get_mut("languages"))
        .and_then(Value::as_array_mut)
    {
        for language in languages {
            let Some(providers) = language.get_mut("providers").and_then(Value::as_array_mut)
            else {
                continue;
            };
            for provider in providers {
                let Some(object) = provider.as_object_mut() else {
                    continue;
                };
                // The provider's command line, reason and execution lane are
                // what language_quality_status is for; the summary keeps the
                // identity and the availability decision only.
                for key in [
                    "root",
                    "source",
                    "covers",
                    "command",
                    "reason",
                    "execution_lane",
                    "check_only",
                    "external_advisory_data",
                    "authorization_required",
                    "machine_format",
                ] {
                    object.remove(key);
                }
            }
        }
    }
    if let Some(object) = value.as_object_mut() {
        object.insert("detail".to_owned(), Value::String("summary".to_owned()));
    }
}

pub(super) async fn call(
    state: &AppState,
    name: &str,
    args: &Value,
) -> Result<AnyResult<Value>, String> {
    let outcome: AnyResult<Value> = match name {
        "project_context" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let detail = super::leaf_intelligence::detail_arg(args)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                let mut value = harness
                    .project_context(workspace_id, &workspace)
                    .and_then(|context| serde_json::to_value(context).map_err(Into::into))?;
                if detail == super::leaf_intelligence::Detail::Summary {
                    compact_project_context(&mut value);
                }
                Ok(value)
            })
            .await
        }
        "list_files" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let path = string_arg(args, "path").unwrap_or(".").to_owned();
            let limit = bounded_usize_arg_range(args, "max_entries", 2_000, 1, 10_000)?;
            run_blocking(move || {
                workspace.list_files(&path, limit).map(|files| {
                    json!({"workspace": workspace_id, "files": files, "count": files.len()})
                })
            })
            .await
        }
        "search_code" | "search_many" | "scan_patterns" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let bug_selection = if name == "scan_patterns" {
                bug_pattern_selection(args)?
            } else {
                None
            };
            if let Some((selection, bug_patterns)) = bug_selection {
                if args.get("patterns").is_some() {
                    return Err("patterns cannot be combined with preset or pattern".to_owned());
                }
                let path = string_arg(args, "path").unwrap_or(".").to_owned();
                let auto_page = match args.get("auto_page") {
                    None => false,
                    Some(value) => value
                        .as_bool()
                        .ok_or_else(|| "auto_page must be a boolean".to_owned())?,
                };
                let max_results = if auto_page {
                    bounded_usize_arg_range(args, "max_results", 2_000, 1, 2_000)?
                } else {
                    bounded_usize_arg_range(args, "max_results", 500, 1, 2_000)?
                };
                // The preset's node kinds never contain comment nodes, so this
                // flag cannot change which matches come back here. It is still
                // read so the response reports what was actually requested
                // instead of silently answering false.
                let include_comments = match args.get("include_comments") {
                    None => false,
                    Some(value) => value
                        .as_bool()
                        .ok_or_else(|| "include_comments must be a boolean".to_owned())?,
                };
                let harness = state.harness.clone();
                let request = crate::code_index::SyntaxSearchRequest {
                    path,
                    node_kinds: vec![
                        "index_expression".to_owned(),
                        "unary_expression".to_owned(),
                        "assignment_statement".to_owned(),
                        "call_expression".to_owned(),
                    ],
                    text_regex: None,
                    include_comments,
                    bug_patterns: bug_patterns.clone(),
                    max_files: 50_000,
                    max_results,
                };
                run_blocking(move || {
                    harness
                        .search_syntax(&workspace_id, &workspace, request)
                        .map(|mut value| {
                            let mut counts = std::collections::BTreeMap::<String, usize>::new();
                            if let Some(matches) = value.get("matches").and_then(Value::as_array) {
                                for row in matches {
                                    if let Some(patterns) =
                                        row.get("bug_patterns").and_then(Value::as_array)
                                    {
                                        for pattern in patterns.iter().filter_map(Value::as_str) {
                                            *counts.entry(pattern.to_owned()).or_default() += 1;
                                        }
                                    }
                                }
                            }
                            value["preset"] = json!(selection);
                            value["pattern_counts"] = json!(counts);
                            value["provider"] = json!("tree-sitter-bug-patterns");
                            value["precision"] = json!("syntax+guard");
                            value
                        })
                })
                .await
            } else {
                let request = search_request(name, args)?;
                let grouped = name == "scan_patterns";
                run_blocking(move || {
                    workspace
                        .search_report(&request)
                        .map(|report| report.into_value(&workspace_id, &request, grouped))
                })
                .await
            }
        }
        "search_syntax" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let node_kinds = string_array_arg(args, "node_kinds", 32)?;
            let path = string_arg(args, "path").unwrap_or(".").to_owned();
            let text_regex = string_arg(args, "text_regex").map(str::to_owned);
            let include_comments = match args.get("include_comments") {
                None => false,
                Some(value) => value
                    .as_bool()
                    .ok_or_else(|| "include_comments must be a boolean".to_owned())?,
            };
            let bug_patterns = optional_string_array_arg(args, "bug_patterns", 5)?;
            let max_files = bounded_usize_arg_range(args, "max_files", 50_000, 1, 50_000)?;
            let max_results = bounded_usize_arg_range(args, "max_results", 200, 1, 2_000)?;
            let harness = state.harness.clone();
            let request = crate::code_index::SyntaxSearchRequest {
                path,
                node_kinds,
                text_regex,
                include_comments,
                bug_patterns,
                max_files,
                max_results,
            };
            run_blocking(move || harness.search_syntax(&workspace_id, &workspace, request)).await
        }
        "file_outline" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let path = required_string(args, "path")?.to_owned();
            let max_symbols = bounded_usize_arg_range(args, "max_symbols", 500, 1, 1_000)?;
            let harness = state.harness.clone();
            run_blocking(move || harness.file_outline(workspace_id, &workspace, &path, max_symbols))
                .await
        }
        "find_symbol" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let query = required_string(args, "query")?.to_owned();
            let path = string_arg(args, "path").unwrap_or(".").to_owned();
            let kind = string_arg(args, "kind").map(str::to_owned);
            let max_results = bounded_usize_arg_range(args, "max_results", 50, 1, 200)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness.find_symbol(
                    workspace_id,
                    &workspace,
                    &query,
                    &path,
                    kind.as_deref(),
                    max_results,
                )
            })
            .await
        }
        "symbol_context" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let symbol_id = required_string(args, "symbol_id")?.to_owned();
            let max_body_lines = bounded_usize_arg_range(args, "max_body_lines", 1_000, 1, 1_000)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness.symbol_context(workspace_id, &workspace, &symbol_id, max_body_lines)
            })
            .await
        }
        "read_file" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let path = required_string(args, "path")?.to_owned();
            let start = optional_bounded_usize_arg(args, "start_line", 1)?.unwrap_or(1);
            let end = optional_bounded_usize_arg(args, "end_line", 1)?;
            run_blocking(move || {
                workspace.read_file(&path, start, end).and_then(|view| {
                    let mut value = serde_json::to_value(view)?;
                    value["workspace"] = json!(workspace_id);
                    Ok(value)
                })
            })
            .await
        }
        "read_files" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let paths = string_array_arg(args, "paths", 32)?;
            let start = optional_bounded_usize_arg(args, "start_line", 1)?.unwrap_or(1);
            let end = optional_bounded_usize_arg(args, "end_line", 1)?;
            run_blocking(move || {
                workspace.read_files(&paths, start, end).map(|files| {
                    json!({"workspace": workspace_id, "count": files.len(), "files": files})
                })
            })
            .await
        }
        "path_info" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let path = required_string(args, "path")?.to_owned();
            run_blocking(move || {
                workspace.path_info(&path).and_then(|info| {
                    let mut value = serde_json::to_value(info)?;
                    value["workspace"] = json!(workspace_id);
                    Ok(value)
                })
            })
            .await
        }
        "replace_text" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let path = required_string(args, "path")?.to_owned();
            let old_text = required_string(args, "old_text")?.to_owned();
            let new_text = required_string(args, "new_text")?.to_owned();
            let expected = required_string(args, "expected_sha256")?.to_owned();
            let start_line = optional_bounded_usize_arg(args, "start_line", 1)?;
            let end_line = optional_bounded_usize_arg(args, "end_line", 1)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                workspace
                    .apply_edits(
                        &path,
                        &[TextEdit {
                            old_text,
                            new_text,
                            start_line,
                            end_line,
                        }],
                        &expected,
                    )
                    .and_then(|result| {
                        harness.invalidate_code_file(&workspace, &result.path);
                        let mut value = serde_json::to_value(result)?;
                        value["workspace"] = json!(workspace_id);
                        Ok(value)
                    })
            })
            .await
        }
        "apply_edits" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let path = required_string(args, "path")?.to_owned();
            let expected = required_string(args, "expected_sha256")?.to_owned();
            let edits: Vec<TextEdit> =
                serde_json::from_value(args.get("edits").cloned().unwrap_or(Value::Null))
                    .map_err(|error| error.to_string())?;
            let harness = state.harness.clone();
            run_blocking(move || {
                workspace
                    .apply_edits(&path, &edits, &expected)
                    .and_then(|result| {
                        harness.invalidate_code_file(&workspace, &result.path);
                        let mut value = serde_json::to_value(result)?;
                        value["workspace"] = json!(workspace_id);
                        Ok(value)
                    })
            })
            .await
        }
        "write_file" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let path = required_string(args, "path")?.to_owned();
            let content = required_string(args, "content")?.to_owned();
            let expected = string_arg(args, "expected_sha256").map(str::to_owned);
            let harness = state.harness.clone();
            run_blocking(move || {
                workspace
                    .write_file(&path, &content, expected.as_deref())
                    .and_then(|result| {
                        harness.invalidate_code_file(&workspace, &result.path);
                        let mut value = serde_json::to_value(result)?;
                        value["workspace"] = json!(workspace_id);
                        Ok(value)
                    })
            })
            .await
        }
        "create_directory" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let path = required_string(args, "path")?.to_owned();
            run_blocking(move || {
                workspace.create_directory(&path).and_then(|result| {
                    let mut value = serde_json::to_value(result)?;
                    value["workspace"] = json!(workspace_id);
                    Ok(value)
                })
            })
            .await
        }
        "create_file" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let path = required_string(args, "path")?.to_owned();
            let content = required_string(args, "content")?.to_owned();
            let harness = state.harness.clone();
            run_blocking(move || {
                workspace.create_file(&path, &content).and_then(|result| {
                    harness.invalidate_code_file(&workspace, &result.path);
                    let mut value = serde_json::to_value(result)?;
                    value["workspace"] = json!(workspace_id);
                    Ok(value)
                })
            })
            .await
        }
        "create_files" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let files: Vec<CreateFileRequest> =
                serde_json::from_value(args.get("files").cloned().unwrap_or(Value::Null))
                    .map_err(|error| error.to_string())?;
            let harness = state.harness.clone();
            run_blocking(move || {
                let results = workspace.create_files(&files)?;
                for item in &results {
                    if item.ok {
                        harness.invalidate_code_file(&workspace, &item.path);
                    }
                }
                Ok(json!({"workspace":workspace_id,"count":results.len(),"results":results}))
            })
            .await
        }
        "apply_file_edits" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let files: Vec<FileEditRequest> =
                serde_json::from_value(args.get("files").cloned().unwrap_or(Value::Null))
                    .map_err(|error| error.to_string())?;
            let harness = state.harness.clone();
            run_blocking(move || {
                let results = workspace.apply_file_edits(&files)?;
                for item in &results {
                    if item.ok {
                        harness.invalidate_code_file(&workspace, &item.path);
                    }
                }
                Ok(json!({"workspace":workspace_id,"count":results.len(),"results":results}))
            })
            .await
        }
        "move_path" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let source = required_string(args, "source")?.to_owned();
            let destination = required_string(args, "destination")?.to_owned();
            let expected = string_arg(args, "expected_source_sha256").map(str::to_owned);
            let harness = state.harness.clone();
            run_blocking(move || {
                workspace
                    .move_path_checked(&source, &destination, expected.as_deref())
                    .and_then(|result| {
                        harness.invalidate_code_prefix(&workspace, &result.source);
                        harness.invalidate_code_prefix(&workspace, &result.destination);
                        let mut value = serde_json::to_value(result)?;
                        value["workspace"] = json!(workspace_id);
                        Ok(value)
                    })
            })
            .await
        }
        "move_paths" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let moves: Vec<MovePathRequest> =
                serde_json::from_value(args.get("moves").cloned().unwrap_or(Value::Null))
                    .map_err(|error| error.to_string())?;
            let harness = state.harness.clone();
            run_blocking(move || {
                let results = workspace.move_paths(&moves)?;
                for item in &results {
                    if item.ok {
                        harness.invalidate_code_prefix(&workspace, &item.source);
                        harness.invalidate_code_prefix(&workspace, &item.destination);
                    }
                }
                Ok(json!({"workspace":workspace_id,"count":results.len(),"results":results}))
            })
            .await
        }
        "delete_path" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let path = required_string(args, "path")?.to_owned();
            let expected = string_arg(args, "expected_sha256").map(str::to_owned);
            let harness = state.harness.clone();
            run_blocking(move || {
                workspace
                    .delete_path(&path, expected.as_deref())
                    .and_then(|result| {
                        harness.invalidate_code_prefix(&workspace, &result.path);
                        let mut value = serde_json::to_value(result)?;
                        value["workspace"] = json!(workspace_id);
                        Ok(value)
                    })
            })
            .await
        }
        "run_command" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let program = required_string(args, "program")?.to_owned();
            let command_args = command_arguments(args)?;
            let command_environment = command_environment(args)?;
            let cwd = match args.get("cwd") {
                None => ".",
                Some(value) => value
                    .as_str()
                    .filter(|cwd| !cwd.is_empty())
                    .ok_or("cwd must be a non-empty string when provided")?,
            };
            let timeout_seconds = command_timeout_seconds(args)?;
            let sandbox = command_sandbox_requested(args)?;
            let revision_key = if command_environment.is_empty()
                && !sandbox
                && workspace.verification_command_shape_allowed(&program, &command_args)
            {
                state
                    .harness
                    .current_workspace_revision_key(&workspace)
                    .map_err(|error| error.to_string())?
            } else {
                None
            };
            let result = match revision_key.as_deref() {
                Some(revision) => {
                    workspace
                        .run_command_at_revision(
                            &program,
                            &command_args,
                            cwd,
                            timeout_seconds,
                            revision,
                        )
                        .await
                }
                None => {
                    workspace
                        .run_command_with_environment(
                            &program,
                            &command_args,
                            cwd,
                            timeout_seconds,
                            &command_environment,
                            sandbox,
                        )
                        .await
                }
            };
            result.and_then(|result| {
                let mut value = serde_json::to_value(result)?;
                value["workspace"] = json!(workspace_id);
                Ok(value)
            })
        }
        _ => Err(anyhow!("unknown workspace tool: {name}")),
    };
    Ok(outcome)
}
