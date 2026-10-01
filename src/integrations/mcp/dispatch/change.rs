use super::*;

pub(super) async fn change_intelligence_tool(
    state: &AppState,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    let (workspace_id, workspace) = selected_workspace(state, args)?;
    let timeout_seconds = args
        .get("timeout_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(30)
        .clamp(1, 120);
    let review = match state
        .harness
        .review_changes(
            workspace_id.clone(),
            &workspace,
            timeout_seconds,
            &state.monitor,
        )
        .await
    {
        Ok(review) => review,
        Err(error) => return Ok(tool_result(json!({"error": error.to_string()}), true)),
    };

    let result: AnyResult<Value> = match name {
        "drift_status" => state
            .harness
            .drift_status(workspace_id.clone(), &workspace, &review)
            .and_then(|status| serde_json::to_value(status).map_err(Into::into)),
        "risk_status" => state
            .harness
            .risk_status(workspace_id.clone(), &workspace, &review)
            .and_then(|status| serde_json::to_value(status).map_err(Into::into)),
        "impact_analysis" => state
            .harness
            .impact_analysis(workspace_id.clone(), &workspace, &review)
            .and_then(|impact| serde_json::to_value(impact).map_err(Into::into)),
        "verification_plan"
            if crate::verification::policy_store::load(&workspace, &workspace_id)
                .map_err(|error| error.to_string())?
                .is_some() =>
        {
            state
                .harness
                .acceptance_plan(
                    &workspace_id,
                    &workspace,
                    "HEAD",
                    crate::verification::change::GitChangeTarget::Worktree,
                )
                .await
                .and_then(|plan| serde_json::to_value(plan).map_err(Into::into))
        }
        "verification_plan" => state
            .harness
            .verification_plan(workspace_id.clone(), &workspace, &review)
            .and_then(|plan| serde_json::to_value(plan).map_err(Into::into)),
        "reconciliation_plan" => state
            .harness
            .reconciliation_plan(workspace_id.clone(), &workspace, &review)
            .and_then(|plan| serde_json::to_value(plan).map_err(Into::into)),
        _ => return Err(format!("unknown change intelligence tool: {name}")),
    };

    Ok(match result {
        Ok(value) => {
            state
                .monitor
                .record_intelligence_result(&workspace_id, name, &value);
            tool_result(value, false)
        }
        Err(error) => tool_result(json!({"error": error.to_string()}), true),
    })
}
