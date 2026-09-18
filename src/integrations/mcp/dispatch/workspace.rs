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

pub(super) fn command_arguments(args: &Value) -> Result<Vec<String>, String> {
    match args.get("args") {
        None => Ok(Vec::new()),
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|_| "args must be an array of strings when provided".to_owned()),
    }
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
        2_000
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

pub(super) async fn call(
    state: &AppState,
    name: &str,
    args: &Value,
) -> Result<AnyResult<Value>, String> {
    let outcome: AnyResult<Value> = match name {
        "project_context" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .project_context(workspace_id, &workspace)
                    .and_then(|context| serde_json::to_value(context).map_err(Into::into))
            })
            .await
        }
        "list_files" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let path = string_arg(args, "path").unwrap_or(".").to_owned();
            let limit = usize_arg(args, "max_entries").unwrap_or(2_000);
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
                    2_000
                } else {
                    usize_arg(args, "max_results")
                        .unwrap_or(500)
                        .clamp(1, 2_000)
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
                    include_comments: false,
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
            let max_files = usize_arg(args, "max_files").unwrap_or(50_000);
            let max_results = usize_arg(args, "max_results").unwrap_or(200);
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
            let max_symbols = usize_arg(args, "max_symbols").unwrap_or(500);
            let harness = state.harness.clone();
            run_blocking(move || harness.file_outline(workspace_id, &workspace, &path, max_symbols))
                .await
        }
        "find_symbol" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let query = required_string(args, "query")?.to_owned();
            let path = string_arg(args, "path").unwrap_or(".").to_owned();
            let kind = string_arg(args, "kind").map(str::to_owned);
            let max_results = usize_arg(args, "max_results").unwrap_or(50);
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
            let max_body_lines = usize_arg(args, "max_body_lines").unwrap_or(1_000);
            let harness = state.harness.clone();
            run_blocking(move || {
                harness.symbol_context(workspace_id, &workspace, &symbol_id, max_body_lines)
            })
            .await
        }
        "read_file" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let path = required_string(args, "path")?.to_owned();
            let start = usize_arg(args, "start_line").unwrap_or(1);
            let end = usize_arg(args, "end_line");
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
            let start = usize_arg(args, "start_line").unwrap_or(1);
            let end = usize_arg(args, "end_line");
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
            let start_line = usize_arg(args, "start_line");
            let end_line = usize_arg(args, "end_line");
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
            let cwd = match args.get("cwd") {
                None => ".",
                Some(value) => value
                    .as_str()
                    .filter(|cwd| !cwd.is_empty())
                    .ok_or("cwd must be a non-empty string when provided")?,
            };
            let timeout_seconds = match args.get("timeout_seconds") {
                None => 120,
                Some(value) => value
                    .as_u64()
                    .filter(|seconds| (1..=1800).contains(seconds))
                    .ok_or("timeout_seconds must be an integer between 1 and 1800")?,
            };
            let revision_key =
                if workspace.verification_command_shape_allowed(&program, &command_args) {
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
                        .run_command(&program, &command_args, cwd, timeout_seconds)
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
