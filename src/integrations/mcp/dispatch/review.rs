use super::*;
use crate::verification::change::{full_oid, GitChangeTarget};

pub(super) async fn review_changes_tool(state: &AppState, args: &Value) -> Result<Value, String> {
    let full_details = match args.get("detail") {
        None => false,
        Some(Value::String(value)) if value == "summary" => false,
        Some(Value::String(value)) if value == "full" => true,
        Some(_) => return Err("detail must be summary or full when provided".to_owned()),
    };
    let adversarial = match args.get("adversarial") {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err("adversarial must be a boolean when provided".to_owned()),
    };
    let base = revision_arg(args, "base_revision", false)?;
    let target = revision_arg(args, "target_revision", true)?;
    if target.is_some() && base.is_none() {
        return Err("target_revision requires base_revision".to_owned());
    }
    if adversarial && base.is_some() {
        return Err(
            "adversarial questions inspect the current worktree; inspect base revisions separately"
                .to_owned(),
        );
    }
    let (workspace_id, workspace) = selected_workspace(state, args)?;
    if let Some(base) = base {
        let target = match target.unwrap_or("HEAD") {
            "worktree" => GitChangeTarget::Worktree,
            revision => GitChangeTarget::Commit {
                revision: revision.to_owned(),
            },
        };
        let snapshot = state
            .harness
            .git_change_snapshot(&workspace, base, target)
            .await
            .map_err(|error| error.to_string())?;
        let incomplete = !snapshot.complete;
        return Ok(tool_result(
            json!({
                "workspace": workspace_id,
                "inspection_scope": "base_change",
                "git_change": snapshot
            }),
            incomplete,
        ));
    }
    let timeout_seconds = args
        .get("timeout_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(30);
    match state
        .harness
        .review_changes(workspace_id, &workspace, timeout_seconds, &state.monitor)
        .await
    {
        Ok(report) => {
            let mut value = serde_json::to_value(&report).map_err(|error| error.to_string())?;
            checkpoint::augment_review_checkpoint(state, &report, adversarial, &mut value).await;
            if adversarial {
                let permit = acquire_tool_permit(state, false).await?;
                let harness = state.harness.clone();
                let packet = super::super::mcp_tools::BLOCKING_PERMIT
                    .scope(
                        permit,
                        run_blocking(move || {
                            Ok(harness.adversarial_review_with_candidates(&workspace, &report))
                        }),
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                value["adversarial"] =
                    serde_json::to_value(packet).map_err(|error| error.to_string())?;
            }
            compact_review_files(&mut value, full_details);
            Ok(tool_result(value, false))
        }
        Err(error) => Ok(tool_result(json!({"error": error.to_string()}), true)),
    }
}

// Wire projection only: full findings, review/checkpoint and adversarial inputs
// are preserved. Preview omissions are distinct from incomplete Git discovery.
fn compact_review_files(value: &mut Value, full_details: bool) {
    if full_details {
        return;
    }
    let Some(files) = value.get_mut("files").and_then(Value::as_array_mut) else {
        return;
    };
    let available = files.len();
    const PREVIEW: usize = 32;
    if available <= PREVIEW {
        return;
    }
    files.truncate(PREVIEW);
    value["file_details"] = json!({
        "included": PREVIEW,
        "available": available,
        "omitted": available - PREVIEW,
        "complete": false,
        "scope": "reported_worktree_file_details",
        "retrieve": {"tool": "review_changes", "arguments": {
            "workspace": value["workspace"], "detail": "full"
        }},
        "guidance": "All findings and totals are retained. Full file details require detail=full; a new call observes the current worktree, not a cached snapshot."
    });
}

fn revision_arg<'a>(
    args: &'a Value,
    name: &str,
    allow_worktree: bool,
) -> Result<Option<&'a str>, String> {
    match args.get(name) {
        None => Ok(None),
        Some(Value::String(value))
            if value == "HEAD" || full_oid(value) || (allow_worktree && value == "worktree") =>
        {
            Ok(Some(value))
        }
        Some(_) => Err(format!(
            "{name} must be HEAD or a complete commit object ID{}",
            if allow_worktree { " or worktree" } else { "" }
        )),
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/integrations/mcp/review_git.rs"]
mod tests;
