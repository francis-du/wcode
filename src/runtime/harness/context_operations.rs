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
        "run app" | "run the app" | "run project" | "start app" | "start the app"
        | "start project" | "start server" | "run server" | "启动项目" | "运行项目"
        | "启动应用" | "运行应用" | "启动服务" | "运行服务" => "launch",
        _ => return Ok(None),
    };
    let budget = requested_budget.unwrap_or(MIN_AGENT_CONTEXT_BUDGET);
    let next_actions = if !profile.exec_enabled {
        Vec::new()
    } else {
        match intent {
            "verification" => vec!["review_changes", "verify_project"],
            "git_commit" => vec!["run_command", "review_changes"],
            "launch" => vec!["workspace_info", "run_command"],
            _ => vec!["run_command"],
        }
    };
    let workflow = match intent {
        "launch" => vec![
            "This is an operator launch workflow, not a source-edit task. No source index or Design scan was requested.",
            "Call workspace_info and inspect only the selected Workspace's bounded launch_profiles. Discovery is read-only, never auto-runs a profile, and never transfers script bodies into command arguments.",
            "Treat program_available=false as a preflight stop: do not call run_command and do not silently substitute another runner. program_available=true proves only bounded PATH presence of the bare executable, not that a subcommand/plugin is installed.",
            "Execute only the selected profile's program and args through run_command; do not guess a hidden script body or infer network trust from a discovered entry.",
            "For a long-lived app or development server, prefer task_mode=true so Tasks owns live output, cancellation and the supervised process tree; never detach a background process.",
            "If the selected profile exposes status_probe, use only that exact bounded probe for follow-up runtime observation. Report its declared precision literally: Docker Health may be empty, which means unknown rather than healthy.",
            "If discovery yields no unambiguous profile, inspect the relevant recognized manifest with bounded repository reads before choosing an explicit command.",
        ],
        "verification" => vec![
            "This is an operator verification workflow, not a source-edit task. No source index or Design scan was requested.",
            "Review the current change set first, then run verify_project at the required level. Repair deterministic failures before retrying and never weaken a gate to obtain green output.",
        ],
        "git_commit" => vec![
            "This is an operator Git workflow, not a source-edit task. No source index or Design scan was requested.",
            "First run git status --short --branch and review the actual diff; avoid duplicate commits and preserve unrelated changes.",
            "Stage only reviewed files, request exact human approval when required, commit, then inspect the new commit and worktree state.",
            "A commit does not imply permission to push, tag, publish, install or restart. Do those only when the user requests them.",
        ],
        _ => vec![
            "This is an operator Git status workflow, not a source-edit task. No source index or Design scan was requested.",
            "Run git status --short --branch and report the observed repository state without mutating it.",
        ],
    };
    let mut pack = json!({
        "workspace":workspace_id,
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
            "islands":profile.islands,
            "write_enabled":profile.write_enabled,
            "exec_enabled":profile.exec_enabled,
        },
        "targets":[],"files":[],"hot_source":[],"tests":[],"design":[],
        "core_constraints":compact_core_constraints(),
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
        "workflow":workflow,
    });
    // There is no measured full-context baseline on this fast route. Report
    // zero savings rather than manufacturing a comparison without building it.
    finalize_agent_context(&mut pack, 0, budget)?;
    Ok(Some(pack))
}
