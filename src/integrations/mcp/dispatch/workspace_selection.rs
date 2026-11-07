use super::*;

/// The Workspace a request resolved to, and how that choice was made.
pub(super) struct Selection {
    pub workspace_id: String,
    pub workspace: Workspace,
    pub mode: &'static str,
}

/// Resolve the Workspace for a request. An explicit handle wins; otherwise a
/// query that names a non-default Workspace routes to it, and anything else
/// falls back to the default.
pub(super) fn resolve(state: &AppState, args: &Value, query: &str) -> Result<Selection, String> {
    if workspace_arg(args)?.is_some() {
        let (workspace_id, workspace) = selected_workspace(state, args)?;
        return Ok(Selection {
            workspace_id,
            workspace,
            mode: "explicit",
        });
    }
    if let Some(preferred) = state.workspaces.prefer_specific_for_query(query) {
        let (workspace_id, workspace) = state
            .workspaces
            .select(Some(&preferred))
            .map_err(|error| error.to_string())?;
        return Ok(Selection {
            workspace_id,
            workspace,
            mode: "query_routed",
        });
    }
    let (workspace_id, workspace) = selected_workspace(state, args)?;
    Ok(Selection {
        workspace_id,
        workspace,
        mode: "default",
    })
}

/// Report the selection on the response and name the handle to reuse. A routed
/// or default selection is per-request state: the stateless transport cannot
/// carry it into later calls, so a multi-root session would silently fall back
/// to a different repository without an explicit handle.
pub(super) fn apply(context: &mut Value, selection: &Selection, default_workspace: &str) {
    context["workspace_selection"] = json!({
        "mode": selection.mode,
        "workspace": selection.workspace_id,
        "default_workspace": default_workspace,
    });
    if selection.mode == "explicit" {
        return;
    }
    let reason = if selection.mode == "query_routed" {
        "matched by the query"
    } else {
        "the default selection"
    };
    if let Some(workflow) = context["workflow"].as_array_mut() {
        workflow.push(json!(format!(
            "This context resolved to Workspace `{}` ({reason}); the default Workspace is `{default_workspace}`. Pass `workspace: \"{}\"` on every later tool call in this session so reads, edits and verification stay in the same repository.",
            selection.workspace_id, selection.workspace_id
        )));
    }
}
