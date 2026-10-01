use serde_json::{json, Value};

const MODEL_PRELOAD_TOOLS: [&str; 4] = [
    "agent_context",
    "workspace_info",
    "execution_status",
    "worklist_status",
];

const MAX_MODEL_RECOMMENDED_TOOLS: usize = 14;

const DEFAULT_CODING_TOOLS: [&str; 6] = [
    "agent_context",
    "search_many",
    "read_files",
    "apply_file_edits",
    "review_changes",
    "verify_project",
];

pub(crate) fn model_preload_tool_count() -> usize {
    MODEL_PRELOAD_TOOLS.len()
}

pub(crate) fn default_coding_tools() -> &'static [&'static str] {
    &DEFAULT_CODING_TOOLS
}

pub(crate) fn model_tool_preload_recommended(name: &str) -> bool {
    MODEL_PRELOAD_TOOLS.contains(&name)
}

pub(crate) fn model_tool_disclosure(name: &str) -> &'static str {
    if model_tool_preload_recommended(name) {
        "core"
    } else {
        "on_demand"
    }
}

pub(crate) fn model_tools_for_group(group: &str) -> &'static [&'static str] {
    match group {
        "repository_read" => &["search_many", "read_files", "file_outline"],
        "semantics" => &[
            "semantic_provider_status",
            "semantic_navigation",
            "find_symbol",
            "symbol_context",
        ],
        "graph" => &["software_graph", "graph_query", "graph_history"],
        "governance" => &[
            "design_status",
            "acceptance_policy",
            "traceability_status",
            "impact_analysis",
            "risk_status",
        ],
        "verification" => &[
            "change_acceptance",
            "verification_status",
            "evidence_status",
            "verification_plan",
        ],
        "execution" => &[
            "execution_status",
            "worklist_status",
            "worklist_claim",
            "worklist_submit",
            "execution_policy_status",
        ],
        "reconciliation" => &[
            "reconciliation_status",
            "reconciliation_plan",
            "reconciliation_execution_status",
        ],
        "quality" => &["language_quality_status"],
        "runtime" => &["workspace_info"],
        _ => &[],
    }
}

fn recommended_model_tools(capabilities: &Value) -> Vec<Value> {
    capabilities
        .get("recommended_tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn write_model_tools(capabilities: &mut Value, mut tools: Vec<Value>) {
    let compacted = capabilities
        .get("compacted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    tools.truncate(MAX_MODEL_RECOMMENDED_TOOLS);
    let Some(object) = capabilities.as_object_mut() else {
        return;
    };
    object.insert("recommended_tools".to_owned(), Value::Array(tools));
    if compacted {
        object.remove("recommended_tool_count");
        object.remove("recommended_actions");
    } else {
        let actions = object["recommended_tools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|name| {
                json!({
                    "tool": name,
                    "group": model_tool_group(name),
                    "disclosure": model_tool_disclosure(name),
                })
            })
            .collect::<Vec<_>>();
        object.insert("recommended_tool_count".to_owned(), json!(actions.len()));
        object.insert("recommended_actions".to_owned(), Value::Array(actions));
    }
}

pub(crate) fn promote_model_tools(capabilities: &mut Value, names: &[String]) {
    let mut tools = recommended_model_tools(capabilities);
    for name in names {
        if tools.len() >= MAX_MODEL_RECOMMENDED_TOOLS {
            break;
        }
        if model_tool_group(name) == "other"
            || tools
                .iter()
                .any(|value| value.as_str() == Some(name.as_str()))
        {
            continue;
        }
        tools.push(Value::String(name.clone()));
    }
    write_model_tools(capabilities, tools);
}

pub(crate) fn prioritize_model_tools(capabilities: &mut Value, names: &[String]) {
    let mut tools = recommended_model_tools(capabilities);
    for name in names.iter().rev() {
        if model_tool_group(name) == "other" {
            continue;
        }
        if let Some(index) = tools
            .iter()
            .position(|value| value.as_str() == Some(name.as_str()))
        {
            tools.remove(index);
        }
        tools.insert(0, Value::String(name.clone()));
    }
    write_model_tools(capabilities, tools);
}

pub(crate) fn model_tool_group(name: &str) -> &'static str {
    if matches!(
        name,
        "agent_context" | "software_context" | "project_context"
    ) {
        "context"
    } else if name == "workspace_info" {
        "workspace"
    } else if matches!(name, "parallel_tools") {
        "orchestration"
    } else if matches!(name, "run_command" | "command_task") {
        "runtime"
    } else if name.starts_with("execution_") || name.starts_with("worklist_") {
        "execution"
    } else if name.starts_with("verification_")
        || matches!(
            name,
            "verify_project" | "review_changes" | "evidence_status" | "change_acceptance"
        )
    {
        "verification"
    } else if name.starts_with("reconciliation_") {
        "reconciliation"
    } else if name.starts_with("semantic_") {
        "semantics"
    } else if name.starts_with("language_quality_") {
        "quality"
    } else if name == "software_graph" || name.starts_with("graph_") {
        "graph"
    } else if matches!(
        name,
        "design_status"
            | "design_init"
            | "acceptance_policy"
            | "convention_status"
            | "scope_status"
            | "traceability_status"
            | "drift_status"
            | "risk_status"
            | "impact_analysis"
    ) {
        "governance"
    } else if matches!(
        name,
        "list_files"
            | "search_code"
            | "search_many"
            | "scan_patterns"
            | "search_syntax"
            | "file_outline"
            | "find_symbol"
            | "symbol_context"
            | "read_file"
            | "read_files"
            | "read_media"
            | "path_info"
    ) {
        "repository_read"
    } else if matches!(
        name,
        "replace_text"
            | "apply_edits"
            | "write_file"
            | "create_directory"
            | "create_file"
            | "create_files"
            | "apply_file_edits"
            | "move_path"
            | "move_paths"
            | "delete_path"
    ) {
        "repository_write"
    } else {
        "other"
    }
}
