use super::*;

/// Only unambiguous operator requests use this route. Source questions and
/// explicit Product Scopes still get the ordinary code-aware context pack.
pub(super) fn build(
    profile: &ProjectProfile,
    workspace_id: &str,
    query: &str,
    requested_budget: Option<usize>,
    max_parallel: usize,
    cache_hit: bool,
) -> Result<Option<Value>> {
    let normalized = query.trim().to_ascii_lowercase();
    let intent = match normalized.as_str() {
        "git commit" | "commit changes" | "提交" | "提交代码" | "提交当前修改" => {
            "git_commit"
        }
        "git status" | "查看git状态" | "检查git状态" | "工作区状态" => "git_status",
        "verify project" | "运行检查" | "全量检查" => "verification",
        _ => return Ok(None),
    };
    let budget = requested_budget.unwrap_or(MIN_AGENT_CONTEXT_BUDGET);
    let next_actions = if !profile.exec_enabled {
        Vec::new()
    } else if intent == "verification" {
        vec!["review_changes", "verify_project"]
    } else if intent == "git_commit" {
        vec!["run_command", "review_changes"]
    } else {
        vec!["run_command"]
    };
    let mut pack = json!({
        "workspace":workspace_id,
        "query":query,
        "intent":intent,
        "budget":budget,
        "budget_mode":if requested_budget.is_some() {"explicit"} else {"adaptive"},
        "requested_budget":requested_budget,
        "cache_hit":cache_hit,
        "truncated":false,
        "baseline_context_bytes":0,
        "project":{
            "project_types":profile.project_types,
            "manifests":profile.manifests,
            "write_enabled":profile.write_enabled,
            "exec_enabled":profile.exec_enabled,
        },
        "targets":[],"files":[],"hot_source":[],"tests":[],"design":[],
        "repo_map":{"items":[],"truncated":false},
        "readiness":{
            "edit":"not_applicable",
            "verify":"not_run",
            "next_actions":next_actions,
            "parallelism":{"strategy":"single_lane","max_parallel":max_parallel,"recommended_concurrency":1},
            "advisories": if profile.exec_enabled {
                vec!["inspect_current_state_before_mutation"]
            } else {
                vec!["workspace_exec_disabled"]
            },
        },
        "workflow":[
            "This is an operator workflow, not a source-edit task. No source index or Design scan was requested.",
            "For Git, first run git status --short --branch and review the actual diff; avoid duplicate commits and preserve unrelated changes.",
            "Stage only reviewed files, request exact human approval when required, commit, then inspect the new commit and worktree state.",
            "A commit does not imply permission to push, tag, publish, install or restart. Do those only when the user requests them.",
            "Use verify_project for checks; fix failed gates before repeating. Do not call source-search tools unless the operation reveals a source problem."
        ],
    });
    // There is no measured full-context baseline on this fast route. Report
    // zero savings rather than manufacturing a comparison without building it.
    finalize_agent_context(&mut pack, 0, budget)?;
    Ok(Some(pack))
}
