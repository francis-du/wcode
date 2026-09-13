use super::*;

fn milestone_stage(name: &str) -> Option<&'static str> {
    match name {
        "agent_context" => Some("understand"),
        "replace_text" | "apply_edits" | "write_file" | "create_file" | "create_files"
        | "apply_file_edits" | "move_path" | "move_paths" | "delete_path" | "design_init" => {
            Some("change")
        }
        "review_changes"
        | "verify_project"
        | "language_quality_run"
        | "verification_submit"
        | "verification_execute_stages"
        | "verification_stage_submit"
        | "verification_approve" => Some("prove"),
        "semantic_provider_refresh"
        | "graph_provider_import"
        | "semantic_confirm"
        | "semantic_retire" => Some("model"),
        "reconciliation_submit" | "reconciliation_retry" => Some("converge"),
        _ => None,
    }
}

fn payload(value: &Value) -> &Value {
    value.get("structuredContent").unwrap_or(value)
}

fn push_path(paths: &mut BTreeSet<String>, path: Option<&str>) {
    let Some(path) = path.map(str::trim).filter(|path| !path.is_empty()) else {
        return;
    };
    if paths.len() >= 32 || path.len() > 512 || path.chars().any(char::is_control) {
        return;
    }
    paths.insert(path.replace('\\', "/"));
}

fn milestone_paths(name: &str, args: &Value, result: Option<&Value>, outcome: &str) -> Vec<String> {
    let mut paths = BTreeSet::new();
    if outcome == "succeeded" {
        match name {
            "agent_context" => {
                if let Some(payload) = result.map(payload) {
                    for file in payload
                        .get("files")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        push_path(&mut paths, file.get("path").and_then(Value::as_str));
                    }
                }
            }
            "replace_text" | "apply_edits" | "write_file" | "create_file" | "delete_path" => {
                push_path(&mut paths, args.get("path").and_then(Value::as_str));
            }
            "create_files" | "apply_file_edits" => {
                for file in args
                    .get("files")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    push_path(&mut paths, file.get("path").and_then(Value::as_str));
                }
            }
            "move_path" => {
                push_path(&mut paths, args.get("source").and_then(Value::as_str));
                push_path(&mut paths, args.get("destination").and_then(Value::as_str));
            }
            "move_paths" => {
                for item in args
                    .get("moves")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    push_path(&mut paths, item.get("source").and_then(Value::as_str));
                    push_path(&mut paths, item.get("destination").and_then(Value::as_str));
                }
            }
            _ => {}
        }
    }
    if name == "review_changes" {
        if let Some(payload) = result.map(payload) {
            for file in payload
                .get("files")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                push_path(&mut paths, file.get("path").and_then(Value::as_str));
            }
        }
    } else if name == "verify_project" {
        if let Some(payload) = result.map(payload) {
            for reason in payload
                .pointer("/impact/reasons")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                push_path(&mut paths, reason.get("source").and_then(Value::as_str));
            }
        }
    }
    paths.into_iter().collect()
}

pub(super) async fn record(
    state: &AppState,
    name: &str,
    args: &Value,
    outcome: &str,
    elapsed_ms: u64,
    result: Option<&Value>,
) {
    let Some(stage) = milestone_stage(name) else {
        return;
    };
    let Ok((_workspace_id, workspace)) = selected_workspace(state, args) else {
        return;
    };
    let paths = milestone_paths(name, args, result, outcome);
    let Ok(mut milestone) = crate::engineering_journal::EngineeringMilestone::new(
        name, stage, outcome, elapsed_ms, paths,
    ) else {
        return;
    };
    if name == "agent_context" {
        if let Some(payload) = result.map(payload) {
            milestone.retrieval_intent = payload
                .pointer("/repo_map/routing/intent")
                .and_then(Value::as_str)
                .filter(|intent| {
                    matches!(
                        *intent,
                        "balanced_context"
                            | "trace_to_code"
                            | "code_to_test"
                            | "comment_to_context"
                            | "failure_trace_to_code"
                            | "edit_to_ripple"
                    )
                })
                .map(str::to_owned);
        }
    } else if name == "verify_project" {
        if let Some(payload) = result.map(payload) {
            milestone.verification_level = payload
                .get("level")
                .and_then(Value::as_str)
                .filter(|level| matches!(*level, "quick" | "full"))
                .map(str::to_owned);
            milestone.checks_run = payload.get("checks_run").and_then(Value::as_u64);
            milestone.checks_failed = payload.get("checks_failed").and_then(Value::as_u64);
        }
    }
    let _ = tokio::task::spawn_blocking(move || {
        crate::engineering_journal::persist(&workspace, &milestone)
    })
    .await;
}
