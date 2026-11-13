use super::*;

fn parallel_tool_allowed(name: &str) -> bool {
    PARALLEL_READ_TOOLS.contains(&name) || PARALLEL_WRITE_TOOLS.contains(&name)
}

fn inherit_parallel_workspace(arguments: &mut Value, workspace: &str) {
    let Some(object) = arguments.as_object_mut() else {
        return;
    };
    object
        .entry("workspace".to_owned())
        .or_insert_with(|| json!(workspace));
}

pub(super) async fn parallel_tools(state: &AppState, args: &mut Value) -> Result<Value, String> {
    let owner = mcp_writer::current_owner();
    let dry_run = match args.get("dry_run") {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err("dry_run must be a boolean; no tasks executed".to_owned()),
    };
    let inherited_workspace = workspace_arg(args)?
        .unwrap_or(state.workspaces.default_id())
        .to_owned();
    let items = args
        .get_mut("tasks")
        .and_then(Value::as_array_mut)
        .ok_or("tasks must be an array")?;
    if !(2..=MAX_PARALLEL_FANOUT_ITEMS).contains(&items.len()) {
        return Err(format!(
            "tasks must contain between 2 and {MAX_PARALLEL_FANOUT_ITEMS} items"
        ));
    }

    for (index, item) in items.iter().enumerate() {
        let tool = item.get("tool").and_then(Value::as_str);
        if tool == Some("command_task")
            || (tool == Some("verify_project")
                && (item.pointer("/arguments/task_id").is_some()
                    || item
                        .pointer("/arguments/action")
                        .is_some_and(|action| action.as_str() != Some("run"))))
            || (tool == Some("run_command")
                && item.pointer("/arguments/task_mode") == Some(&Value::Bool(true)))
        {
            return Err(format!(
                "parallel task {} failed preflight: durable command tasks require a top-level tools/call; no tasks executed",
                index + 1
            ));
        }
        if let Some(arguments) = item.get("arguments") {
            workspace_arg(arguments).map_err(|error| {
                format!(
                    "parallel task {} failed preflight: {error}; no tasks executed",
                    index + 1
                )
            })?;
        }
    }
    state
        .workspaces
        .select(Some(&inherited_workspace))
        .map_err(|error| error.to_string())?;
    let (prepared, aliases, skipped) =
        scheduler::coalesce_apply_edits(&inherited_workspace, items)?;
    let mut prepared = match prepared {
        std::borrow::Cow::Borrowed(_) => std::mem::take(items),
        std::borrow::Cow::Owned(prepared) => {
            items.clear();
            prepared
        }
    };
    let task_count = prepared.len();
    let started = std::time::Instant::now();
    let mut results = vec![None; task_count];
    let mut workloads = Vec::new();
    let mut execution_arguments = vec![None; task_count];

    for (index, item) in prepared.iter_mut().enumerate() {
        if skipped.contains(&index) {
            continue;
        }
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("task-{}", index + 1));
        let Some(name) = item.get("tool").and_then(Value::as_str).map(str::to_owned) else {
            results[index] = Some(parallel_item_error(id, "unknown", "missing tool name"));
            continue;
        };
        if !parallel_tool_allowed(&name) {
            results[index] = Some(parallel_item_error(
                id,
                &name,
                "parallel_tools only accepts bounded read/discovery tools and workspace file primitives",
            ));
            continue;
        }
        let mut arguments = item
            .get_mut("arguments")
            .map(std::mem::take)
            .unwrap_or_else(|| json!({}));
        if !arguments.is_object() {
            results[index] = Some(parallel_item_error(
                id,
                &name,
                "arguments must be an object",
            ));
            continue;
        }
        inherit_parallel_workspace(&mut arguments, &inherited_workspace);
        if name == "run_command" {
            if let Err(error) = leaf_workspace::preflight_sync_command(state, &arguments) {
                results[index] = Some(parallel_item_error(id, name, error));
                continue;
            }
        }
        match scheduler::resource_model(&inherited_workspace, &name, &arguments).and_then(
            |resources| {
                selected_workspace(state, &arguments)
                    .map(|(_, workspace)| resources.in_root(workspace.root()))
            },
        ) {
            Ok(resources) => {
                execution_arguments[index] = Some(arguments);
                workloads.push((index, resources));
            }
            Err(error) => results[index] = Some(parallel_item_error(id, name, error)),
        }
    }

    // An invalid resource model cannot safely disappear from the dependency
    // graph: a later mutation might otherwise run against stale input. Reject
    // the entire batch before spawning anything when preflight is incomplete.
    if let Some((index, item)) = results
        .iter()
        .enumerate()
        .find_map(|(index, item)| item.as_ref().map(|item| (index, item)))
    {
        return Err(format!(
            "parallel task {} failed preflight: {}; no tasks executed",
            index + 1,
            item["error"].as_str().unwrap_or("invalid task")
        ));
    }

    let active = workloads
        .iter()
        .map(|(index, _)| *index)
        .collect::<BTreeSet<_>>();
    let graph = scheduler::dependency_graph(&workloads, task_count);
    let dependency_edges = graph.predecessors.iter().map(BTreeSet::len).sum::<usize>();
    if dry_run {
        let layers = graph.layers(&active)?;
        let plan = scheduler::preview(
            &graph,
            &layers,
            &workloads,
            &aliases,
            state.harness.max_parallel(),
        );
        return Ok(tool_result(plan, false));
    }
    let mut fanout_response_bytes = 0usize;

    let mut completion_schedule = graph.completion_schedule(&active)?;
    let mut pending = active.len();
    let mut failed_tasks = vec![false; task_count];
    let mut task_depths = vec![0usize; task_count];
    let mut dependency_layers = 0usize;
    // JoinSet owns the children: cancellation must not detach queued writes.
    let mut running = tokio::task::JoinSet::new();
    let mut task_indices = std::collections::HashMap::new();
    let mut task_ids = (0..task_count)
        .map(|_| None)
        .collect::<Vec<Option<tokio::task::Id>>>();
    while pending > 0 || !running.is_empty() {
        for index in completion_schedule.take_ready() {
            pending = pending.saturating_sub(1);
            let dependency_failed = graph.predecessors[index]
                .iter()
                .any(|dependency| failed_tasks[*dependency]);
            let depth = graph.predecessors[index]
                .iter()
                .map(|dependency| task_depths[*dependency])
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            task_depths[index] = depth;
            dependency_layers = dependency_layers.max(depth);
            let item = &prepared[index];
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("task-{}", index + 1));
            let name = item
                .get("tool")
                .and_then(Value::as_str)
                .expect("scheduled tasks were validated before graph construction");
            let arguments = execution_arguments[index]
                .take()
                .expect("scheduled task arguments were prepared during preflight");
            let child_state = state.clone();
            let child_owner = owner.clone();
            let child_name = name.to_owned();
            let id_for_task = id.clone();
            let name_for_task = child_name.clone();
            let handle = running.spawn(async move {
                let response = if dependency_failed {
                    Err("dependency failed; task was not executed".to_owned())
                } else {
                    mcp_writer::with_owner(
                        child_owner,
                        call_leaf_tool_structured(&child_state, &name_for_task, arguments),
                    )
                    .await
                };
                let result = match response {
                    Ok(response) => {
                        parallel_item_from_response(id_for_task, name_for_task, response)
                    }
                    Err(error) => {
                        let item = parallel_item_error(id_for_task, name_for_task, error);
                        let bytes = serialized_size(&item);
                        (item, bytes)
                    }
                };
                (index, result)
            });
            let task_id = handle.id();
            task_indices.insert(task_id, index);
            task_ids[index] = Some(task_id);
        }

        // Release a successor as soon as its own predecessors finish, without
        // waiting for an unrelated slow branch in the same topological layer.
        let (index, outcome) = match running.join_next().await {
            Some(Ok((index, result))) => {
                if let Some(task_id) = task_ids[index].take() {
                    task_indices.remove(&task_id);
                }
                (index, Ok(result))
            }
            Some(Err(error)) => {
                let task_id = error.id();
                let index = task_indices
                    .remove(&task_id)
                    .ok_or("parallel task identity was lost")?;
                task_ids[index] = None;
                (index, Err(error.to_string()))
            }
            None => return Err("parallel scheduler made no progress".to_owned()),
        };
        let id = prepared[index]
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("task-{}", index + 1));
        let name = prepared[index]["tool"]
            .as_str()
            .unwrap_or("unknown")
            .to_owned();
        {
            let item = match outcome {
                Ok((item, item_bytes))
                    if fanout_response_bytes.saturating_add(item_bytes)
                        <= MAX_PARALLEL_FANOUT_RESPONSE_BYTES =>
                {
                    fanout_response_bytes = fanout_response_bytes.saturating_add(item_bytes);
                    item
                }
                Ok((_item, item_bytes)) => parallel_item_error(
                    id.clone(),
                    name.clone(),
                    format!(
                        "parallel fan-out response budget exceeded ({item_bytes}B item, {}B aggregate limit); narrow paths, line ranges, or result limits",
                        MAX_PARALLEL_FANOUT_RESPONSE_BYTES
                    ),
                ),
                Err(error) => parallel_item_error(
                    id.clone(),
                    name.clone(),
                    format!("task join failed: {error}"),
                ),
            };
            if item["ok"].as_bool() != Some(true) {
                failed_tasks[index] = true;
            }
            completion_schedule.complete(index);
            if let Some(alias_items) = aliases.get(&index) {
                for (alias_index, alias_id) in alias_items {
                    let mut alias_item = item.clone();
                    alias_item["id"] = json!(alias_id);
                    alias_item["coalesced_with"] = json!(id);
                    alias_item["coalesced"] = json!(true);
                    let alias_bytes = serialized_size(&alias_item);
                    if fanout_response_bytes.saturating_add(alias_bytes)
                        <= MAX_PARALLEL_FANOUT_RESPONSE_BYTES
                    {
                        fanout_response_bytes = fanout_response_bytes.saturating_add(alias_bytes);
                        results[*alias_index] = Some(alias_item);
                    } else {
                        results[*alias_index] = Some(parallel_item_error(
                            alias_id.clone(),
                            "apply_edits",
                            "parallel fan-out response budget exceeded while expanding coalesced result",
                        ));
                    }
                }
            }
            results[index] = Some(item);
        }
    }

    let items = results
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            item.unwrap_or_else(|| {
                parallel_item_error(
                    format!("task-{}", index + 1),
                    "unknown",
                    "task did not produce a result",
                )
            })
        })
        .collect::<Vec<_>>();
    let succeeded = items
        .iter()
        .filter(|item| item.get("ok").and_then(Value::as_bool) == Some(true))
        .count();
    let failed = items.len().saturating_sub(succeeded);

    Ok(tool_result(
        json!({
            "execution": "parallel-fanout",
            "scheduler": "dependency-graph",
            "dispatch": "completion-driven",
            "dependency_layers": dependency_layers,
            "dependency_edges": dependency_edges,
            "max_parallel": state.harness.max_parallel(),
            "tasks": items.len(),
            "succeeded": succeeded,
            "failed": failed,
            "elapsed_ms": started.elapsed().as_millis(),
            "response_bytes": fanout_response_bytes,
            "item_limit_bytes": MAX_PARALLEL_FANOUT_ITEM_BYTES,
            "response_limit_bytes": MAX_PARALLEL_FANOUT_RESPONSE_BYTES,
            "coalesced_same_file_edits": aliases.values().map(Vec::len).sum::<usize>(),
            "items": items,
        }),
        failed > 0,
    ))
}
