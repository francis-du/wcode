use super::*;

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
        "search_code" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let query = required_string(args, "query")?.to_owned();
            let path = string_arg(args, "path").unwrap_or(".").to_owned();
            let limit = usize_arg(args, "max_results").unwrap_or(100);
            run_blocking(move || {
                workspace.search(&query, &path, limit).map(|matches| {
                    json!({"workspace": workspace_id, "matches": matches, "count": matches.len()})
                })
            })
            .await
        }
        "search_many" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let queries = string_array_arg(args, "queries", 32)?;
            let path = string_arg(args, "path").unwrap_or(".").to_owned();
            let limit = usize_arg(args, "max_results").unwrap_or(200);
            run_blocking(move || {
                workspace.search_many(&queries, &path, limit).map(|matches| {
                    json!({"workspace": workspace_id, "matches": matches, "count": matches.len()})
                })
            })
            .await
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
            let command_args = args
                .get("args")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let cwd = string_arg(args, "cwd").unwrap_or(".").to_owned();
            let timeout_seconds = args
                .get("timeout_seconds")
                .and_then(Value::as_u64)
                .unwrap_or(120);
            workspace
                .run_command(&program, &command_args, &cwd, timeout_seconds)
                .await
                .and_then(|result| {
                    let mut value = serde_json::to_value(result)?;
                    value["workspace"] = json!(workspace_id);
                    Ok(value)
                })
        }
        _ => Err(anyhow!("unknown workspace tool: {name}")),
    };
    Ok(outcome)
}
