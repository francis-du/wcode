use super::*;
use crate::engineering_journal::EngineeringFailureCode;

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
    let path = path.replace('\\', "/");
    if crate::engineering_journal::valid_repository_path(&path) {
        paths.insert(path);
    }
}

fn milestone_paths(name: &str, args: &Value, result: Option<&Value>, outcome: &str) -> Vec<String> {
    let mut paths = BTreeSet::new();
    {
        match name {
            "agent_context" if outcome == "succeeded" => {
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

fn observation_outcome<'a>(name: &str, outcome: &'a str, result: Option<&Value>) -> &'a str {
    let failed = result.map(payload).is_some_and(|value| {
        matches!(
            name,
            "verify_project" | "language_quality_run" | "verification_execute_stages"
        ) && (value.get("passed").and_then(Value::as_bool) == Some(false)
            || value
                .get("checks_failed")
                .and_then(Value::as_u64)
                .is_some_and(|count| count > 0))
    });
    if failed {
        "failed"
    } else {
        outcome
    }
}

fn observation_stage(name: &str, outcome: &str) -> Option<&'static str> {
    milestone_stage(name).or_else(|| {
        (matches!(outcome, "failed" | "blocked")
            && super::super::mcp_tools::tools()
                .iter()
                .any(|tool| tool["name"].as_str() == Some(name)))
        .then_some("understand")
    })
}

fn classify_error(value: &Value, codes: &mut BTreeSet<EngineeringFailureCode>) {
    if value
        .get("authorization_required")
        .is_some_and(Value::is_object)
    {
        codes.insert(EngineeringFailureCode::AuthorizationRequired);
    }
    if value.get("timed_out").and_then(Value::as_bool) == Some(true) {
        codes.insert(EngineeringFailureCode::Timeout);
    }
    // Only this ephemeral native error field is inspected; command output,
    // arguments, prompts and diagnostics are never copied into the journal.
    let Some(error) = value.get("error").and_then(Value::as_str) else {
        return;
    };
    let error = error
        .chars()
        .take(4096)
        .collect::<String>()
        .to_ascii_lowercase();
    if error.starts_with("stale file: expected sha256 ") {
        codes.insert(EngineeringFailureCode::ShaMismatch);
    }
    if error.starts_with("authorization required:") {
        codes.insert(EngineeringFailureCode::AuthorizationRequired);
    }
    if error.starts_with("protected credential or repository-control path is not accessible:")
        || error.starts_with("wcode authority state paths are not accessible through file tools")
        || error.starts_with("symlink paths are blocked to preserve workspace isolation:")
    {
        codes.insert(EngineeringFailureCode::ProtectedPath);
    }
    if (error.starts_with("wcode core policy blocks creating ")
        || error.starts_with("wcode core policy blocks growing "))
        && error.contains("maintained source lines")
    {
        codes.insert(EngineeringFailureCode::SourceLimit);
    }
    if [
        "replan_required:",
        "stale verification plan",
        "verification revision changed",
        "verification git identity changed",
        "repository revision changed",
        "revision changed before stage evidence",
        "revision changed before human approval",
    ]
    .iter()
    .any(|prefix| error.starts_with(prefix))
    {
        codes.insert(EngineeringFailureCode::RevisionStale);
    }
}

fn failure_codes(name: &str, outcome: &str, result: Option<&Value>) -> Vec<EngineeringFailureCode> {
    if !matches!(outcome, "failed" | "blocked") {
        return Vec::new();
    }
    let Some(value) = result.map(payload) else {
        return Vec::new();
    };
    let mut codes = BTreeSet::new();
    classify_error(value, &mut codes);
    for item in value
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(64)
    {
        if item.get("ok").and_then(Value::as_bool) == Some(false) {
            classify_error(item, &mut codes);
        }
    }
    if matches!(
        name,
        "verify_project" | "language_quality_run" | "verification_execute_stages"
    ) {
        if value.get("passed").and_then(Value::as_bool) == Some(false)
            || value
                .get("checks_failed")
                .and_then(Value::as_u64)
                .is_some_and(|count| count > 0)
        {
            codes.insert(EngineeringFailureCode::VerificationFailure);
        }
        if value
            .pointer("/discovery/complete")
            .and_then(Value::as_bool)
            == Some(false)
        {
            codes.insert(EngineeringFailureCode::DiscoveryIncomplete);
        }
        for check in value
            .get("checks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(256)
        {
            if check.get("success").and_then(Value::as_bool) == Some(false) {
                if check.get("id").and_then(Value::as_str) == Some("profile-discovery-completeness")
                {
                    codes.insert(EngineeringFailureCode::DiscoveryIncomplete);
                }
                if check.get("timed_out").and_then(Value::as_bool) == Some(true) {
                    codes.insert(EngineeringFailureCode::Timeout);
                }
            }
        }
    }
    codes.into_iter().collect()
}

pub(super) async fn record(
    state: &AppState,
    name: &str,
    args: &Value,
    outcome: &str,
    elapsed_ms: u64,
    result: Option<&Value>,
) {
    let outcome = observation_outcome(name, outcome, result);
    let Some(stage) = observation_stage(name, outcome) else {
        return;
    };
    let Ok((_workspace_id, workspace)) = selected_workspace(state, args) else {
        return;
    };
    let mut paths = milestone_paths(name, args, result, outcome);
    if matches!(outcome, "failed" | "blocked") {
        paths.retain(|path| workspace.source_metadata_stamp(path).is_ok());
    }
    let Ok(mut milestone) = crate::engineering_journal::EngineeringMilestone::new(
        name, stage, outcome, elapsed_ms, paths,
    ) else {
        return;
    };
    milestone.failure_codes = failure_codes(name, outcome, result);
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
    // Do not inspect arguments, output, nested diagnostics or caller labels.
    // Reject over-bound text before allocating or crossing the worker boundary.
    let transient_error = result
        .filter(|_| matches!(outcome, "failed" | "blocked"))
        .map(payload)
        .and_then(|value| value.get("error"))
        .and_then(Value::as_str)
        .filter(|error| error.len() <= 4096)
        .map(str::to_owned);
    let harness = state.harness.clone();
    let _ = tokio::task::spawn_blocking(move || {
        // Reuse canonical validated caches; constructing a runtime per milestone
        // would discard them. Observation failure still leaves the event unbound.
        // Do not borrow an Execution checkpoint or a previous green revision.
        milestone.observed_revision = harness.current_revision(&workspace).ok();
        match transient_error.as_deref() {
            Some(error) => {
                crate::engineering_journal::persist_with_error(&workspace, &milestone, Some(error))
            }
            None => crate::engineering_journal::persist(&workspace, &milestone),
        }
    })
    .await;
}

#[cfg(test)]
#[path = "../../../../tests/unit/integrations/mcp/journal.rs"]
mod tests;
