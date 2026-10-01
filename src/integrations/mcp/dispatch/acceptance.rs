use super::*;
use crate::verification::change::GitChangeTarget;
use anyhow::{bail, Context};
use serde::Deserialize;

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    #[default]
    Inspect,
    Plan,
    Verify,
    Record,
    History,
    Metrics,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    #[serde(default)]
    action: Action,
    workspace: Option<String>,
    base_revision: Option<String>,
    target_revision: Option<String>,
    timeout_seconds: Option<u64>,
}

impl Args {
    fn parse(value: &Value) -> AnyResult<Self> {
        let args: Self = serde_json::from_value(value.clone())?;
        if value
            .as_object()
            .context("change_acceptance arguments must be an object")?
            .values()
            .any(Value::is_null)
        {
            bail!("change_acceptance arguments must not be null");
        }
        if args
            .workspace
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 300 || id.chars().any(char::is_control))
        {
            bail!("invalid workspace");
        }
        for revision in [
            args.base_revision.as_deref(),
            args.target_revision.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if revision != "HEAD"
                && revision != "worktree"
                && !crate::verification::change::full_oid(revision)
            {
                bail!("revision must be HEAD, worktree, or a complete Git SHA");
            }
        }
        if args.base_revision.as_deref() == Some("worktree") {
            bail!("base revision cannot be worktree");
        }
        if matches!(args.action, Action::History | Action::Metrics)
            && (args.base_revision.is_some() || args.target_revision.is_some())
        {
            bail!("history/metrics do not accept a current candidate");
        }
        if args
            .timeout_seconds
            .is_some_and(|timeout| !(1..=1800).contains(&timeout))
            || (args.timeout_seconds.is_some() && !matches!(args.action, Action::Verify))
        {
            bail!("only verify accepts timeout_seconds (1..1800)");
        }
        Ok(args)
    }
}

pub(super) async fn call(state: &AppState, value: &Value) -> AnyResult<Value> {
    let args = Args::parse(value)?;
    let (workspace_id, workspace) = selected_workspace(state, value).map_err(anyhow::Error::msg)?;
    let base = args.base_revision.as_deref().unwrap_or("HEAD");
    let target = match args.target_revision.as_deref().unwrap_or("worktree") {
        "worktree" => GitChangeTarget::Worktree,
        revision => GitChangeTarget::Commit {
            revision: revision.to_owned(),
        },
    };
    let harness = &state.harness;
    match args.action {
        Action::Plan => serde_json::to_value(
            harness
                .acceptance_plan(&workspace_id, &workspace, base, target)
                .await?,
        )
        .map_err(Into::into),
        Action::History => harness.acceptance_history(&workspace_id, &workspace),
        Action::Metrics => harness.acceptance_metrics(&workspace_id, &workspace),
        Action::Inspect | Action::Record | Action::Verify => {
            let native = match args.action {
                Action::Inspect => {
                    harness
                        .capture_acceptance(&workspace_id, &workspace, base, target)
                        .await?
                }
                Action::Record => {
                    harness
                        .record_acceptance(&workspace_id, &workspace, base, target)
                        .await?
                }
                Action::Verify => {
                    harness
                        .acceptance_verify(
                            &workspace_id,
                            &workspace,
                            base,
                            target,
                            args.timeout_seconds.unwrap_or(600),
                            &state.monitor,
                        )
                        .await?
                }
                _ => unreachable!(),
            };
            serde_json::to_value(native.record()).map_err(Into::into)
        }
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/integrations/mcp/acceptance.rs"]
mod tests;
