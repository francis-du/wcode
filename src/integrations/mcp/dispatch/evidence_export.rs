use super::*;

const DEFAULT_EXPORT_LIMIT: usize = 50;
const MIN_EXPORT_LIMIT: u64 = 1;

pub(super) async fn call(state: &AppState, args: &Value) -> Result<AnyResult<Value>, String> {
    reject_unknown_arguments(args)?;
    let limit = export_limit(args)?;
    let (_workspace_id, workspace) = selected_workspace(state, args)?;
    let outcome: AnyResult<Value> = run_blocking(move || {
        let history = crate::engineering_journal::load_recent(&workspace, limit)?;
        let exported = crate::engineering_journal::export_records(&history.records, limit)?;
        Ok(json!({
            "format": "json-lines",
            "requested_limit": limit,
            "records": history.records.len(),
            "retained_records": history.retained_records,
            "truncated": history.truncated,
            "chain_valid": history.chain_valid,
            "export": exported,
        }))
    })
    .await;
    Ok(outcome)
}

fn reject_unknown_arguments(args: &Value) -> Result<(), String> {
    let Some(object) = args.as_object() else {
        return Err("arguments must be an object".to_owned());
    };
    if let Some(unknown) = object
        .keys()
        .find(|key| key.as_str() != "workspace" && key.as_str() != "limit")
    {
        return Err(format!("unknown argument: {unknown}"));
    }
    Ok(())
}

fn export_limit(args: &Value) -> Result<usize, String> {
    let Some(value) = args.get("limit") else {
        return Ok(DEFAULT_EXPORT_LIMIT);
    };
    let limit = value
        .as_u64()
        .filter(|limit| *limit >= MIN_EXPORT_LIMIT)
        .ok_or_else(|| {
            format!(
                "limit must be an integer between {MIN_EXPORT_LIMIT} and {} when provided",
                crate::engineering_journal::MAX_ENGINEERING_MILESTONES
            )
        })?;
    usize::try_from(limit).map_err(|_| "limit is too large for this platform".to_owned())
}

#[cfg(test)]
#[path = "../../../../tests/unit/integrations/mcp/evidence_export.rs"]
mod tests;
