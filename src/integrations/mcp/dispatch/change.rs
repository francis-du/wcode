use super::*;

pub(super) async fn change_intelligence_tool(
    state: &AppState,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    let (workspace_id, workspace) = selected_workspace(state, args)?;
    // Validate the bounded-detail argument before spending a repository scan:
    // a bad argument must fail closed regardless of whether the review could run.
    if name == "risk_status" {
        super::leaf_intelligence::detail_arg(args)?;
    }
    let timeout_seconds = bounded_usize_arg_range(args, "timeout_seconds", 30, 1, 120)? as u64;
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
        "risk_status" => {
            let detail = super::leaf_intelligence::detail_arg(args)?;
            let outcome: AnyResult<Value> = state
                .harness
                .risk_status(workspace_id.clone(), &workspace, &review)
                .and_then(|status| serde_json::to_value(status).map_err(Into::into));
            match outcome {
                Ok(mut value) => {
                    // Risk status embeds the whole traceability projection.
                    // Compacting it keeps the risk verdict, drift and bug
                    // candidates intact while the requirement traces stay
                    // available through traceability_status.
                    if detail == super::leaf_intelligence::Detail::Summary {
                        if let Some(traceability) = value.get_mut("traceability") {
                            super::leaf_intelligence::compact_traceability(traceability);
                        }
                    }
                    Ok(value)
                }
                Err(error) => Err(error),
            }
        }
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
