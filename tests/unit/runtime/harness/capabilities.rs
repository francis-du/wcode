use super::*;

#[test]
fn v080_capabilities_are_runtime_declared_and_bounded() {
    let capabilities = ToolHarness::new(8).unwrap().capabilities();

    let scan = &capabilities["repository_scanning"];
    assert_eq!(scan["gitignore"], true);
    assert_eq!(scan["dot_ignore"], true);
    assert_eq!(scan["generated_directories_pruned"], true);
    assert_eq!(scan["explicit_ignored_paths_queryable"], true);

    let observatory = &capabilities["observatory"];
    assert_eq!(observatory["cached_snapshots"], true);
    assert_eq!(observatory["background_single_flight_refresh"], true);
    assert_eq!(observatory["revision_stamped_cache"], true);
    assert_eq!(observatory["code_graph_lazy_loaded"], true);

    let twin = &capabilities["software_intelligence"]["engineering_digital_twin"];
    assert_eq!(twin["code_graph"], true);
    assert_eq!(twin["max_depth"], 4);
    assert_eq!(twin["max_nodes"], 240);
    assert_eq!(twin["history_navigation"], true);
    assert!(twin["modes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|mode| mode == "impact"));

    let decision = &capabilities["software_intelligence"]["decision_plane"];
    assert_eq!(decision["authority"], "advisory_only");
    assert_eq!(decision["shadow_ab"], true);
    assert_eq!(decision["independent_fitness_calibration"], true);
    assert_eq!(decision["question_set"], "wcode.agent_context@5");

    let routing = &capabilities["capability_routing"];
    assert_eq!(routing["disclosure"], "metadata_first");
    assert_eq!(routing["core_tool_count"], 4);
    assert_eq!(routing["dynamic_tool_list"], false);
    assert_eq!(
        routing["dynamic_tool_list_policy"],
        "task_independent_protocol_catalog"
    );
    assert_eq!(routing["task_manifest"], "agent_context.capabilities");
}

#[test]
fn model_action_registry_keeps_core_small_and_groups_on_demand_tools() {
    assert_eq!(MODEL_PRELOAD_TOOLS.len(), 4);
    for bootstrap in [
        "agent_context",
        "workspace_info",
        "execution_status",
        "worklist_status",
    ] {
        assert!(model_tool_preload_recommended(bootstrap));
    }
    assert!(!model_tool_preload_recommended("verify_project"));
    assert!(!model_tool_preload_recommended("semantic_navigation"));
    assert_eq!(model_tool_disclosure("agent_context"), "core");
    assert_eq!(model_tool_disclosure("verify_project"), "on_demand");
    assert_eq!(model_tool_disclosure("semantic_navigation"), "on_demand");
    assert_eq!(model_tool_group("semantic_navigation"), "semantics");
    assert_eq!(model_tool_group("verification_plan"), "verification");
    assert_eq!(model_tool_group("apply_file_edits"), "repository_write");
    assert_eq!(model_tool_group("search_many"), "repository_read");
    assert_eq!(model_tool_group("definitely_unknown"), "other");
    for group in [
        "repository_read",
        "semantics",
        "graph",
        "governance",
        "verification",
        "execution",
        "reconciliation",
        "quality",
        "runtime",
    ] {
        let tools = model_tools_for_group(group);
        assert!(!tools.is_empty(), "group {group} has no tools");
        for tool in tools {
            assert_ne!(model_tool_group(tool), "repository_write");
            assert_ne!(*tool, "run_command");
        }
    }
    assert_eq!(
        default_coding_tools(),
        [
            "agent_context",
            "search_many",
            "read_files",
            "apply_file_edits",
            "review_changes",
            "verify_project"
        ]
    );
}

#[test]
fn readiness_priority_displaces_optional_defaults_without_growing_manifest() {
    let mut capabilities = json!({
        "recommended_tools": [
            "agent_context", "search_many", "read_files", "apply_file_edits",
            "review_changes", "verify_project", "find_symbol", "symbol_context",
            "semantic_navigation", "design_status", "traceability_status",
            "impact_analysis", "risk_status", "evidence_status"
        ]
    });
    prioritize_model_tools(
        &mut capabilities,
        &["workspace_info".into(), "run_command".into()],
    );
    let tools = capabilities["recommended_tools"].as_array().unwrap();
    assert_eq!(tools.len(), 14);
    assert_eq!(tools[0], "workspace_info");
    assert_eq!(tools[1], "run_command");
    assert!(capabilities["recommended_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action["tool"] == "run_command" && action["group"] == "runtime"));
}
