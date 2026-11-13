use super::*;

pub(super) fn run_command_task_mode(params: &Value) -> bool {
    params
        .pointer("/arguments/task_mode")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(crate) async fn create_tool_task(
    state: Arc<AppState>,
    params: Value,
    owner: String,
) -> Result<Value, TaskRpcError> {
    Ok(modern_result(
        start_tool_task(state, params, owner).await?.create_result(),
    ))
}

pub(super) async fn start_tool_task(
    state: Arc<AppState>,
    params: Value,
    owner: String,
) -> Result<TaskRecord, TaskRpcError> {
    start_tool_task_bound(state, params, owner, None).await
}

pub(crate) struct VerificationTaskBinding {
    pub revision: crate::evidence::Revision,
    pub git: Option<crate::verification::change::ExecutionGitBinding>,
}

pub(crate) async fn start_tool_task_bound(
    state: Arc<AppState>,
    params: Value,
    owner: String,
    verification_binding: Option<VerificationTaskBinding>,
) -> Result<TaskRecord, TaskRpcError> {
    let tool_name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| TaskRpcError::invalid("tools/call is missing params.name"))?
        .to_owned();
    if !task_augmented_tool(&params) {
        return Err(TaskRpcError::invalid(
            "tool does not support task augmentation for these arguments",
        ));
    }
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let (workspace_id, workspace) =
        selected_workspace(&state, &args).map_err(TaskRpcError::invalid)?;
    if tool_name == "verify_project" {
        crate::mcp::verification_options(&args).map_err(TaskRpcError::invalid)?;
    }
    if verification_binding.is_some()
        && (tool_name != "verify_project" || !workspace.exec_enabled())
    {
        return Err(TaskRpcError::invalid(
            "bound verification requires enabled execution",
        ));
    }
    let task_owner = owner.clone();
    let mut record = TaskRecord::working(
        owner,
        workspace_id,
        tool_name,
        state.auth.instance_id().to_owned(),
    );
    record.verification_revision = verification_binding
        .as_ref()
        .map(|binding| binding.revision.clone());
    record.verification_git_binding = verification_binding
        .as_ref()
        .and_then(|binding| binding.git.clone());
    let deadline = Instant::now() + Duration::from_millis(record.ttl_ms);
    // Creation and registration are one transition: a concurrent poll must
    // never observe a working record before its worker has been registered.
    let _guard = state
        .tasks
        .state_lock
        .lock()
        .map_err(|_| TaskRpcError::internal("MCP task state lock poisoned"))?;
    task_store::persist(&workspace, &record)
        .map_err(|error| TaskRpcError::internal(error.to_string()))?;

    let task_id = record.task_id.clone();
    let worker_task_id = task_id.clone();
    let worker_state = state.clone();
    let worker_workspace = workspace.clone();
    let (start_tx, start_rx) = oneshot::channel::<()>();
    let join = tokio::spawn(async move {
        if start_rx.await.is_err() {
            worker_state.tasks.remove(&worker_task_id);
            return;
        }
        run_task_worker(
            worker_state,
            worker_workspace,
            worker_task_id,
            params,
            task_owner,
            deadline,
            verification_binding,
        )
        .await;
    });
    state
        .tasks
        .register(task_id, record.workspace.clone(), join.abort_handle());
    let _ = start_tx.send(());
    Ok(record)
}

pub(super) async fn run_task_worker(
    state: Arc<AppState>,
    workspace: Workspace,
    task_id: String,
    params: Value,
    owner: String,
    deadline: Instant,
    verification_binding: Option<VerificationTaskBinding>,
) {
    // Disconnection does not own this worker. Its deadline and tasks/cancel do.
    // Drop the child set before persistence so queued work is cancelled even
    // when nobody polls or the terminal snapshot cannot be written.
    let outcome = if Instant::now() >= deadline {
        Err(TaskRpcError::internal("task exceeded its durable TTL"))
    } else {
        let progress_enabled = run_command_task_mode(&params);
        let (progress_tx, mut progress_rx) =
            tokio::sync::mpsc::channel(COMMAND_OUTPUT_PROGRESS_CHANNEL_CAPACITY);
        let tool_state = state.clone();
        let monitor_task_id = task_id.clone();
        let bound_workspace = workspace.clone();
        let mut tools = JoinSet::new();
        tools.spawn(async move {
            if let Some(expected) = verification_binding {
                let git = tool_state.harness.execution_git_binding(&bound_workspace).await
                    .map_err(|_| "verification Git inspection failed".to_owned())?;
                let harness = tool_state.harness.clone();
                let current = tokio::task::spawn_blocking(move || harness.current_revision(&bound_workspace))
                    .await.map_err(|_| "verification revision inspection failed".to_owned())?
                    .map_err(|_| "verification revision inspection failed".to_owned())?;
                if current != expected.revision || git != expected.git {
                    return Err("verification snapshot changed before execution; refresh before starting a new task".to_owned());
                }
            }
            let call = MONITOR_DURABLE_TASK_ID.scope(
                monitor_task_id,
                call_tool_owned(&tool_state, params, &owner),
            );
            if progress_enabled {
                crate::workspace::with_command_output_progress(progress_tx, call).await
            } else {
                drop(progress_tx);
                call.await
            }
        });
        let mut stdout = LiveCommandStream::default();
        let mut stderr = LiveCommandStream::default();
        let mut progress_open = progress_enabled;
        let mut progress_dirty = false;
        let mut persist_tick = tokio::time::interval(LIVE_COMMAND_PERSIST_INTERVAL);
        persist_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let _ = persist_tick.tick().await;
        loop {
            tokio::select! {
                joined = tools.join_next() => {
                    while let Ok(chunk) = progress_rx.try_recv() {
                        apply_live_command_chunk(&mut stdout, &mut stderr, chunk);
                        progress_dirty = true;
                    }
                    if progress_dirty {
                        persist_live_command_output(&state, &workspace, &task_id, &stdout, &stderr);
                    }
                    break match joined {
                        Some(Ok(outcome)) => outcome.map_err(TaskRpcError::invalid),
                        Some(Err(error)) => Err(TaskRpcError::internal(format!(
                            "task tool worker failed to join: {error}"
                        ))),
                        None => Err(TaskRpcError::internal("task tool worker did not start")),
                    };
                }
                chunk = progress_rx.recv(), if progress_open => {
                    match chunk {
                        Some(chunk) => {
                            apply_live_command_chunk(&mut stdout, &mut stderr, chunk);
                            progress_dirty = true;
                        }
                        None => {
                            progress_open = false;
                            if progress_dirty {
                                persist_live_command_output(&state, &workspace, &task_id, &stdout, &stderr);
                                progress_dirty = false;
                            }
                        }
                    }
                }
                _ = persist_tick.tick(), if progress_open && progress_dirty => {
                    persist_live_command_output(&state, &workspace, &task_id, &stdout, &stderr);
                    progress_dirty = false;
                }
                _ = tokio::time::sleep_until(deadline) => {
                    if progress_dirty {
                        persist_live_command_output(&state, &workspace, &task_id, &stdout, &stderr);
                    }
                    break Err(TaskRpcError::internal("task exceeded its durable TTL"));
                }
            }
        }
    };
    let Ok(_guard) = state.tasks.state_lock.lock() else {
        state.tasks.remove(&task_id);
        return;
    };
    let Ok(Some(mut current)) = task_store::load(&workspace, &task_id) else {
        state.tasks.remove(&task_id);
        return;
    };
    if current.status != TaskStatus::Working {
        state.tasks.remove(&task_id);
        return;
    }
    match outcome {
        // Completed describes delivery, not a successful tool/check outcome.
        Ok(value) => current.complete(modern_result(value)),
        Err(error) => current.fail(error.code, error.message),
    }
    if let Err(error) = persist_task_result(&workspace, &mut current) {
        tracing::warn!(%task_id, %error, "MCP task final state was not persisted");
    }
    state.tasks.remove(&task_id);
}
