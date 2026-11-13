use super::*;

#[path = "intelligence_call.rs"]
mod intelligence_call;
pub(super) use intelligence_call::call;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Detail {
    Summary,
    Full,
}

pub(super) fn detail_arg(args: &Value) -> Result<Detail, String> {
    match string_arg(args, "detail") {
        None | Some("summary") => Ok(Detail::Summary),
        Some("full") => Ok(Detail::Full),
        Some(other) => Err(format!(
            "detail must be \"summary\" or \"full\", got {other:?}"
        )),
    }
}

const TRACEABILITY_REFERENCE_SAMPLE: usize = 4;
const TRACEABILITY_MESSAGE_LIMIT: usize = 160;

/// A requirement trace carries one fully resolved reference object per mapped
/// file, symbol, test and acceptance link. Under a real design state that is
/// the bulk of the response and far more than a status check needs, so the
/// summary keeps requirement identity, per-kind resolved/unresolved counts and
/// a small bounded sample, and declares the omission. `detail=full` restores
/// every reference with its full provenance message.
pub(super) fn compact_traceability(value: &mut Value) {
    let Some(requirements) = value.get_mut("requirements").and_then(Value::as_array_mut) else {
        if let Some(object) = value.as_object_mut() {
            object.insert("detail".to_owned(), Value::String("summary".to_owned()));
        }
        return;
    };
    for requirement in requirements {
        let Some(object) = requirement.as_object_mut() else {
            continue;
        };
        for key in ["implementation", "verification"] {
            let Some(references) = object.get_mut(key).and_then(Value::as_array_mut) else {
                continue;
            };
            let total = references.len();
            let resolved = references
                .iter()
                .filter(|reference| reference.get("resolved") == Some(&Value::Bool(true)))
                .count();
            references.truncate(TRACEABILITY_REFERENCE_SAMPLE);
            for reference in references {
                let Some(reference) = reference.as_object_mut() else {
                    continue;
                };
                if let Some(message) = reference.get("message").and_then(Value::as_str) {
                    if message.chars().count() > TRACEABILITY_MESSAGE_LIMIT {
                        let bounded: String =
                            message.chars().take(TRACEABILITY_MESSAGE_LIMIT).collect();
                        reference.insert("message".to_owned(), Value::String(bounded));
                        reference.insert("message_truncated".to_owned(), Value::Bool(true));
                    }
                }
            }
            object.insert(format!("{key}_total"), Value::from(total));
            object.insert(format!("{key}_resolved"), Value::from(resolved));
            object.insert(
                format!("{key}_unresolved"),
                Value::from(total.saturating_sub(resolved)),
            );
            if total > TRACEABILITY_REFERENCE_SAMPLE {
                object.insert(format!("{key}_truncated"), Value::Bool(true));
            }
        }
    }
    if let Some(object) = value.as_object_mut() {
        object.insert("detail".to_owned(), Value::String("summary".to_owned()));
    }
}

/// The status contract (identity, producer, revision, confidence, policy and
/// result) is what callers read first. Per-check execution receipts and git
/// bindings are retained for `detail=full`; carrying them for every record
/// made the default response an order of magnitude larger than the status it
/// describes. The omission is declared, never silent.
fn compact_evidence_status(value: &mut Value) {
    let Some(evidence) = value.get_mut("evidence").and_then(Value::as_array_mut) else {
        return;
    };
    for record in evidence {
        let Some(object) = record.as_object_mut() else {
            continue;
        };
        for key in [
            "execution_receipt",
            "execution_git_binding",
            "execution_policy_binding",
            "external_receipt",
        ] {
            object.remove(key);
        }
    }
    if let Some(object) = value.as_object_mut() {
        object.insert("receipts_omitted".to_owned(), Value::Bool(true));
    }
}

/// A worklist accumulates one entry per completed task, so a long-running
/// worklist returns its whole history even though only unfinished items are
/// actionable; agent_context already summarizes this way. The summary keeps the
/// goal, counts, coordination and unfinished items, and declares the omission.
fn compact_worklist_status(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let Some(mut items) = object
        .get_mut("items")
        .and_then(Value::as_array_mut)
        .map(std::mem::take)
    else {
        return;
    };
    let total = items.len();
    let (kept, done) = items
        .drain(..)
        .partition(|item| item.get("status").and_then(Value::as_str) != Some("done"));
    object.insert("items".to_owned(), Value::Array(kept));
    object.insert("items_total".to_owned(), Value::from(total));
    object.insert("items_omitted".to_owned(), Value::from(done.len()));
    object.insert("detail".to_owned(), Value::String("summary".to_owned()));
}

async fn convention_status_tool(state: &AppState, args: &Value) -> AnyResult<Value> {
    let (_workspace_id, workspace) = selected_workspace(state, args).map_err(anyhow::Error::msg)?;
    let harness = state.harness.clone();
    run_blocking(move || {
        harness
            .convention_status(&workspace)
            .and_then(|status| serde_json::to_value(status).map_err(Into::into))
    })
    .await
}

#[path = "intelligence_handles.rs"]
pub(super) mod handles_catalog;
pub(super) use handles_catalog::handles;

async fn workspace_git_binding(
    harness: &ToolHarness,
    workspace: &Workspace,
) -> Option<crate::verification::change::ExecutionGitBinding> {
    harness
        .execution_git_binding(workspace)
        .await
        .ok()
        .flatten()
}
