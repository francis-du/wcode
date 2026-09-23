use super::*;

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
            "traceability_status",
            "impact_analysis",
            "risk_status",
        ],
        "verification" => &[
            "verification_status",
            "evidence_status",
            "verification_plan",
        ],
        "execution" => &[
            "execution_status",
            "worklist_status",
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
    let actions = tools
        .iter()
        .filter_map(Value::as_str)
        .map(|name| {
            json!({
                "tool": name,
                "group": model_tool_group(name),
                "disclosure": model_tool_disclosure(name),
            })
        })
        .collect::<Vec<_>>();
    let Some(object) = capabilities.as_object_mut() else {
        return;
    };
    object.insert("recommended_tool_count".to_owned(), json!(tools.len()));
    object.insert("recommended_tools".to_owned(), Value::Array(tools));
    if compacted {
        object.remove("recommended_actions");
    } else {
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
    } else if name == "run_command" {
        "runtime"
    } else if name.starts_with("execution_") || name.starts_with("worklist_") {
        "execution"
    } else if name.starts_with("verification_")
        || matches!(
            name,
            "verify_project" | "review_changes" | "evidence_status"
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

impl ToolHarness {
    pub fn capabilities(&self) -> Value {
        let repository_scanning = json!({
            "gitignore": true,
            "dot_ignore": true,
            "git_global": true,
            "git_exclude": true,
            "generated_directories_pruned": true,
            "explicit_ignored_paths_queryable": true,
        });
        let observatory = json!({
            "cached_snapshots": true,
            "background_single_flight_refresh": true,
            "revision_stamped_cache": true,
            "code_graph_lazy_loaded": true,
        });
        let limits = crate::resource::limits();
        let execution_admission = json!({
            "limit": Self::execution_limit(self.max_parallel),
            "process_capacity": limits.child_processes,
            "subspace_process_capacity": limits.child_processes,
            "host_process_capacity": limits.host_child_process_limit(),
            "queued_process_headroom": Self::execution_limit(self.max_parallel)
                .saturating_sub(limits.host_child_process_limit()),
            "read_headroom": self.max_parallel - Self::execution_limit(self.max_parallel),
            "total_limit": self.max_parallel,
            "tool_slot_wait_cap_ms": u64::try_from(TOOL_SLOT_WAIT_CAP.as_millis()).unwrap_or(u64::MAX),
        });
        let digital_twin = json!({
            "code_graph": true,
            "modes": ["calls", "impact", "all"],
            "max_depth": 4,
            "max_nodes": 240,
            "precision": [
                "declared",
                "syntax",
                "semantic",
                "runtime",
                "deterministic",
                "heuristic",
                "mixed"
            ],
            "design_ownership": true,
            "test_and_proof_edges": true,
            "runtime_provider_edges": true,
            "history_navigation": true,
        });
        let decision_plane = json!({
            "schema_version": crate::decision::DECISION_SCHEMA_VERSION,
            "provider": "wcode-deterministic-v1",
            "local_decision_plane": true,
            "jev_optional": "jev",
            "question_set": "wcode.agent_context@5",
            "typed_distributions": true,
            "concentration_metrics": ["top1_margin", "normalized_entropy"],
            "authority": "advisory_only",
            "shadow_ab": true,
            "independent_fitness_calibration": true,
            "primitives": ["probability", "choice", "score"],
        });
        let semantic_providers = json!({
            "languages": 22,
            "adapter": "warm-lsp-session-document-symbol-navigation",
            "precision": "semantic-when-lsp-is-live-syntax-otherwise",
            "mode": "automatic-hardened-lsp-with-explicit-trust-for-others",
            "default_enabled": true,
            "opt_out": "--no-semantic",
            "requires_risky_exec": "non-automatic-providers-only",
            "warm_sessions": true,
            "incremental_document_sync": true,
            "navigation": [
                "definition",
                "references",
                "implementations",
                "incoming_calls",
                "outgoing_calls",
                "hover"
            ],
            "routing": "tree-sitter-for-localization-lsp-for-cross-file-relations",
            "session_pool": self.semantic_sessions.status()
        });
        let stage_executors = json!({
            "builtin_discovery": true,
            "config": ".wcode/executors.yaml",
            "no_shell": true,
            "languages": 22,
            "stages": ["property", "mutation", "fuzz", "runtime_canary"],
            "execution_policy": "bounded-no-shell-repository-executors-autonomous",
            "requires_risky_exec": false
        });
        let capability_routing = json!({
            "disclosure": "metadata_first",
            "core_tool_count": MODEL_PRELOAD_TOOLS.len(),
            "dynamic_tool_list": false,
            "dynamic_tool_list_policy": "task_independent_protocol_catalog",
            "tool_grouping": true,
            "task_manifest": "agent_context.capabilities",
            "host_metadata": [
                "dev.wcode/preloadRecommended",
                "dev.wcode/productScopes"
            ],
            "action_groups": "task_manifest_only"
        });
        let software_intelligence = json!({
            "design_state": true,
            "software_graph": "composite-declared-syntax-external",
            "graph_history": graph_store::capabilities(),
            "graph_providers": graph_provider_store::capabilities(),
            "engineering_digital_twin": digital_twin,
            "decision_plane": decision_plane,
            "semantic_providers": semantic_providers,
            "traceability": true,
            "software_context": true,
            "drift": true,
            "impact_analysis": true,
            "risk": true,
            "reconciliation_plan": true,
            "verification_mesh": verification_store::capabilities(),
            "migration_audit": crate::migration_audit::capabilities(),
            "stage_executors": stage_executors,
            "evidence": evidence_store::capabilities(),
            "experience": crate::experience_store::capabilities(),
            "semantics": semantic_store::capabilities(),
            "reconciliation": reconciliation_store::capabilities(),
            "reconciliation_execution": reconciliation_execution_store::capabilities(),
            "persistent_store": [
                "verification-state",
                "evidence",
                "experience",
                "semantics",
                "graph-providers",
                "graph-history",
                "reconciliation-plans",
                "reconciliation-execution"
            ],
            "automatic_reconciliation": "orchestrated-safe-task-execution"
        });
        json!({
            "tools": QUALITY_HARNESS_TOOLS,
            "project_context": true,
            "context_cache": true,
            "review_changes": true,
            "parallel_change_review": true,
            "adversarial_review": true,
            "adversarial_review_policy": "challenge-packet-not-evidence",
            "verify_project": true,
            "phased_parallel_verification": true,
            "verification_exec_without_risky_flag": true,
            "verification_levels": ["quick", "full"],
            "max_verification_checks": MAX_VERIFICATION_CHECKS,
            "max_review_files": MAX_REVIEW_FILES,
            "max_parallel_tools": self.max_parallel,
            "repository_scanning": repository_scanning,
            "observatory": observatory,
            "execution_admission": execution_admission,
            "resource_governor": crate::resource::capabilities(),
            "software_intelligence": software_intelligence,
            "capability_routing": capability_routing,
            "code_index": self.code_index.capabilities(),
        })
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/runtime/harness/capabilities.rs"]
mod tests;
