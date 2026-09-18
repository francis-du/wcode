use super::*;

pub(super) fn update_agent_readiness(value: &mut Value) {
    let compact_output = value.pointer("/readiness/parallelism/strategy").is_some()
        && value
            .pointer("/readiness/parallelism/instruction")
            .is_none();
    let targets = value
        .get("targets")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let hot_source = value
        .get("hot_source")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let files = value
        .get("files")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let target_paths = value
        .get("targets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|target| target.get("path").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    let target_files = files
        .iter()
        .filter(|file| {
            file.get("path")
                .and_then(Value::as_str)
                .is_some_and(|path| target_paths.contains(path))
        })
        .collect::<Vec<_>>();
    let sha_files = target_files
        .iter()
        .filter(|file| file.get("sha256").and_then(Value::as_str).is_some())
        .count();
    let editable_files = target_files
        .iter()
        .filter(|file| {
            file.get("sha256").and_then(Value::as_str).is_some()
                && file.get("readonly").and_then(Value::as_bool) != Some(true)
        })
        .count();
    let oversized_target_files = files
        .iter()
        .filter(|file| {
            file.get("readonly").and_then(Value::as_bool) != Some(true)
                && file.get("source_oversized").and_then(Value::as_bool) == Some(true)
        })
        .count();
    let write_enabled = value
        .pointer("/project/write_enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let tests = value
        .get("tests")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let resolved_tests = tests
        .iter()
        .filter(|test| test.get("resolved").and_then(Value::as_bool) == Some(true))
        .count();
    let convention_errors = value
        .pointer("/conventions/errors")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let convention_scan_truncated = value
        .pointer("/conventions/truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let graph_truncated = value
        .pointer("/repo_map/truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let graph_precision = covered_repo_map_precision(value);
    let semantic_relationship_task = value
        .get("query")
        .and_then(Value::as_str)
        .is_some_and(query_needs_semantic_relationships);
    let recommend_semantic_navigation =
        semantic_relationship_task && graph_precision == "syntax" && targets > 0;
    let semantic_provider_actions = value
        .get("semantic_provider_hints")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let semantic_provider_authorization = semantic_provider_actions
        .iter()
        .any(|provider| provider.get("action").and_then(Value::as_str) == Some("authorize_lsp"));
    let semantic_provider_initialization = semantic_provider_actions
        .iter()
        .any(|provider| provider.get("action").and_then(Value::as_str) == Some("initialize_lsp"));
    let semantic_provider_missing = semantic_provider_actions
        .iter()
        .any(|provider| provider.get("action").and_then(Value::as_str) == Some("install_lsp"));

    let usable_sources = value["targets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|target| target_has_usable_source(value, target, &files))
        .count();
    let edit = if !write_enabled {
        "read_only_workspace"
    } else if targets == 0 {
        "needs_target"
    } else if sha_files > 0 && editable_files == 0 {
        "read_only_target"
    } else if editable_files == 0 {
        "needs_sha"
    } else if usable_sources == 0 {
        "needs_source"
    } else {
        "ready"
    };
    let verify = if convention_errors > 0 || convention_scan_truncated {
        "blocked_by_core_policy"
    } else if tests.is_empty() {
        "needs_mapping"
    } else if resolved_tests == tests.len() {
        "ready"
    } else if resolved_tests == 0 {
        "unresolved"
    } else {
        "partial"
    };
    let mut advisories = Vec::new();
    if !write_enabled {
        advisories.push("workspace_write_disabled");
    } else if sha_files > 0 && editable_files == 0 {
        advisories.push("target_files_read_only");
    }
    if convention_errors > 0 {
        advisories.push("hard_convention_violations");
    }
    if oversized_target_files > 0 {
        advisories.push("oversized_target_requires_decomposition");
    }
    if convention_scan_truncated {
        advisories.push("convention_scan_truncated");
    }
    if graph_truncated {
        advisories.push("repo_map_truncated");
    }
    if value
        .pointer("/repo_map/routing/reason")
        .and_then(Value::as_str)
        == Some("ambiguous_retrieval_signals")
    {
        advisories.push("retrieval_specialization_abstained");
    }
    if graph_precision == "syntax" {
        advisories.push("syntax_only_relationships");
    }
    if recommend_semantic_navigation {
        advisories.push("semantic_navigation_recommended");
    }
    if semantic_provider_authorization {
        advisories.push("lsp_authorization_required");
    }
    if semantic_provider_missing {
        advisories.push("lsp_install_required");
    }
    if usable_sources == 0 && targets > 0 {
        advisories.push("source_body_not_in_pack");
    } else if usable_sources < targets {
        advisories.push("target_source_coverage_incomplete");
    }
    if value["hot_source"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|source| source.pointer("/body/redacted").and_then(Value::as_bool) == Some(true))
    {
        advisories.push("source_body_redacted");
    }
    if tests.is_empty() {
        advisories.push("no_verification_mapping");
    } else if resolved_tests < tests.len() {
        advisories.push("verification_mapping_incomplete");
    }

    let edit_tool = if editable_files <= 1 {
        "apply_edits"
    } else {
        "apply_file_edits"
    };
    let mut next_actions = Vec::<&str>::new();
    if convention_errors > 0 || convention_scan_truncated {
        next_actions.push("convention_status");
    }
    match edit {
        "ready" => {
            if recommend_semantic_navigation
                && (semantic_provider_authorization || semantic_provider_initialization)
            {
                next_actions.push("semantic_provider_refresh");
            }
            if recommend_semantic_navigation {
                next_actions.push("semantic_navigation");
            }
            next_actions.push(edit_tool);
        }
        "needs_source" => {
            if recommend_semantic_navigation
                && (semantic_provider_authorization || semantic_provider_initialization)
            {
                next_actions.push("semantic_provider_refresh");
            }
            if recommend_semantic_navigation {
                next_actions.push("semantic_navigation");
            }
            next_actions.push(if value.get("retrieval").is_some() {
                "read_file"
            } else {
                "symbol_context"
            });
            next_actions.push(edit_tool);
        }
        "needs_target" => {
            next_actions.push("find_symbol");
            if recommend_semantic_navigation
                && (semantic_provider_authorization || semantic_provider_initialization)
            {
                next_actions.push("semantic_provider_refresh");
            }
            if recommend_semantic_navigation {
                next_actions.push("semantic_navigation");
            }
            next_actions.push("symbol_context");
            next_actions.push(edit_tool);
        }
        "needs_sha" => {
            if recommend_semantic_navigation
                && (semantic_provider_authorization || semantic_provider_initialization)
            {
                next_actions.push("semantic_provider_refresh");
            }
            if recommend_semantic_navigation {
                next_actions.push("semantic_navigation");
            }
            next_actions.push("path_info");
            next_actions.push(edit_tool);
        }
        "read_only_workspace" | "read_only_target" => {}
        _ => {}
    }
    if !matches!(edit, "read_only_workspace" | "read_only_target") {
        if verify != "ready" {
            next_actions.push("traceability_status");
        }
        next_actions.push("review_changes");
        if convention_errors > 0 {
            next_actions.push("reconciliation_plan");
        }
        next_actions.push("verify_project");
    }

    let previous_candidate_lanes = value
        .pointer("/readiness/parallelism/candidate_lanes")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0);
    let discovery_lanes = if targets == 0 {
        value["scopes"].as_array().map_or(0, Vec::len)
    } else {
        0
    };
    let worklist_lanes = value
        .pointer("/worklist/parallel_runnable")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let max_parallel = value
        .pointer("/readiness/parallelism/max_parallel")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(1) as usize;
    let candidate_lanes = target_paths
        .len()
        .max(editable_files)
        .max(discovery_lanes)
        .max(worklist_lanes)
        .max(previous_candidate_lanes)
        .max(1);
    let parallel_required = candidate_lanes > 1;
    if parallel_required {
        advisories.push("parallel_execution_required");
    }
    let parallel_strategy = if parallel_required {
        "parallel_required"
    } else {
        "single_lane"
    };
    let architecture_intent = value
        .get("query")
        .and_then(Value::as_str)
        .is_some_and(query_requests_architecture_change);
    let inferred_change_strategy = if architecture_intent && target_paths.len() >= 3 {
        "cross_module_change"
    } else if target_paths.len() > 1 || oversized_target_files > 0 {
        "localized_refactor"
    } else {
        "minimal_patch"
    };
    let previous_change_strategy = value
        .pointer("/readiness/change_strategy")
        .and_then(Value::as_str);
    let change_strategy = match (previous_change_strategy, inferred_change_strategy) {
        (Some("cross_module_change"), _) => "cross_module_change",
        (Some("localized_refactor"), "minimal_patch") => "localized_refactor",
        _ => inferred_change_strategy,
    };
    let complexity_budget = match change_strategy {
        "minimal_patch" => json!({
            "new_production_files": 0,
            "new_abstractions": 0,
            "new_config_knobs": 0,
            "public_api_changes": 0,
        }),
        "localized_refactor" => json!({
            "new_production_files": 1,
            "new_abstractions": 1,
            "new_config_knobs": 0,
            "public_api_changes": 0,
        }),
        _ => json!({
            "new_production_files": "evidence_required",
            "new_abstractions": "evidence_required",
            "new_config_knobs": "only_if_required",
            "public_api_changes": "only_if_required",
        }),
    };

    value["readiness"] = json!({
        "edit": edit,
        "verify": verify,
        "graph_precision": graph_precision,
        "oversized_target_files": oversized_target_files,
        "next_actions": next_actions,
        "parallelism": {
            "strategy": parallel_strategy,
            "required": parallel_required,
            "candidate_lanes": candidate_lanes,
            "execution_bias": if parallel_required { "parallel_first" } else { "single_lane" },
            "max_parallel": max_parallel,
            "recommended_concurrency": candidate_lanes.min(max_parallel),
            "lane_targets": if !target_paths.is_empty() {
                target_paths.iter().copied().collect::<Vec<_>>()
            } else {
                value["scopes"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>()
            },
            "instruction": if parallel_required {
                "Parallel execution is required for the independent lanes in the next action. Use concurrent top-level calls when supported; otherwise use wcode parallel_tools for compact known operations. Serialize only true data dependencies or overlapping writes."
            } else {
                "Keep this task in one lane unless new independent targets are discovered."
            },
            "serialize_only": ["overlapping file writes", "shared mutable state", "output-dependent follow-ups"],
            "fallback_tool": if parallel_required { "parallel_tools" } else { "none" }
        },
        "change_strategy": change_strategy,
        "complexity_budget": complexity_budget,
        "direct_targets": targets,
        "hot_source_items": hot_source,
        "direct_target_files": target_paths.len(),
        "sha_targets": sha_files,
        "editable_sha_targets": editable_files,
        "recommended_edit_tool": edit_tool,
        "verification_refs": tests.len(),
        "resolved_verification_refs": resolved_tests,
        "graph_truncated": graph_truncated,
        "hard_constraint_violations": convention_errors,
        "convention_scan_truncated": convention_scan_truncated,
        "advisories": advisories,
    });
    if compact_output {
        let readiness = value["readiness"]
            .as_object_mut()
            .expect("readiness was just constructed as an object");
        if let Some(parallelism) = readiness
            .get_mut("parallelism")
            .and_then(Value::as_object_mut)
        {
            for key in [
                "execution_bias",
                "instruction",
                "serialize_only",
                "fallback_tool",
                "lane_targets",
            ] {
                parallelism.remove(key);
            }
        }
        for key in [
            "direct_targets",
            "hot_source_items",
            "direct_target_files",
            "sha_targets",
            "verification_refs",
            "resolved_verification_refs",
            "graph_truncated",
        ] {
            readiness.remove(key);
        }
        if let Some(complexity) = readiness
            .get_mut("complexity_budget")
            .and_then(Value::as_object_mut)
        {
            complexity.remove("public_api_changes");
        }
    }
}

// A body somewhere in the pack is not evidence that a direct target is
// editable. Bind its identity and revision to that target's writable file.
fn target_has_usable_source(value: &Value, target: &Value, files: &[Value]) -> bool {
    let Some(path) = target.get("path").and_then(Value::as_str) else {
        return false;
    };
    value["hot_source"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|source| {
            if source.get("path").and_then(Value::as_str) != Some(path)
                || source.pointer("/body/redacted").and_then(Value::as_bool) == Some(true)
                || !source
                    .pointer("/body/content")
                    .and_then(Value::as_str)
                    .is_some_and(|text| !text.is_empty())
            {
                return false;
            }
            let same_target = match (
                target.get("id").and_then(Value::as_str),
                source.get("id").and_then(Value::as_str),
            ) {
                (Some(expected), Some(actual)) => expected == actual,
                (None, _) => target.get("kind").and_then(Value::as_str) == Some("file"),
                _ => false,
            };
            same_target
                && source
                    .get("sha256")
                    .and_then(Value::as_str)
                    .is_some_and(|sha| {
                        !sha.is_empty()
                            && files.iter().any(|file| {
                                file.get("path").and_then(Value::as_str) == Some(path)
                                    && file.get("sha256").and_then(Value::as_str) == Some(sha)
                                    && file.get("readonly").and_then(Value::as_bool) != Some(true)
                            })
                    })
        })
}

pub(super) fn query_requests_architecture_change(query: &str) -> bool {
    let query = query.to_ascii_lowercase();
    [
        "architecture",
        "architectural",
        "cross-module",
        "cross module",
        "redesign",
        "re-architect",
        "架构",
        "跨模块",
        "重构架构",
    ]
    .iter()
    .any(|needle| query.contains(needle))
}

pub(super) fn query_needs_semantic_relationships(query: &str) -> bool {
    super::super::harness_retrieval::query_needs_semantic_relationships(query)
}

pub(super) fn covered_repo_map_precision(value: &Value) -> &'static str {
    value
        .pointer("/repo_map/precision")
        .and_then(Value::as_str)
        .and_then(canonical_precision)
        .unwrap_or("syntax")
}

fn canonical_precision(value: &str) -> Option<&'static str> {
    match value {
        "runtime" => Some("runtime"),
        "semantic" => Some("semantic"),
        "deterministic" => Some("deterministic"),
        "syntax" => Some("syntax"),
        "declared" => Some("declared"),
        "heuristic" => Some("heuristic"),
        _ => None,
    }
}
