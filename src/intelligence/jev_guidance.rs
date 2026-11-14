use super::*;

pub(super) fn apply_agent_context_guidance(context: &mut Value, telemetry: &Value) -> Value {
    if telemetry.get("status").and_then(Value::as_str) != Some("active") {
        return json!({"applied":false,"actions":[]});
    }
    let guidance = telemetry
        .get("guidance")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let candidate_next_action = telemetry
        .get("candidate_next_action")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let target_count = context
        .get("targets")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let hot_source_count = context
        .get("hot_source")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let retrieval_action = if target_count == 0 {
        "find_symbol"
    } else if hot_source_count < target_count {
        "symbol_context"
    } else {
        "software_context"
    };

    let needs_semantic = guidance.iter().any(|item| {
        item.as_str() == Some("jev:prefer_semantic_navigation")
            || item.as_str() == Some("jev:next_action:semantic_navigation")
    }) || candidate_next_action == "semantic_navigation";
    let needs_review = guidance
        .iter()
        .any(|item| item.as_str() == Some("jev:next_action:review_worktree"))
        || candidate_next_action == "review_worktree";
    let needs_retrieval = guidance.iter().any(|item| {
        matches!(
            item.as_str(),
            Some("jev:retrieve_more_evidence")
                | Some("jev:choice_uncertain_collect_evidence")
                | Some("jev:next_action:retrieve")
        )
    }) || candidate_next_action == "retrieve";
    let verification_escalation = guidance
        .iter()
        .any(|item| item.as_str() == Some("jev:preserve_or_raise_verification"));
    let capability_group = guidance.iter().find_map(|item| {
        item.as_str()
            .and_then(|value| value.strip_prefix("jev:capability_group:"))
            .filter(|group| !crate::model_tools::model_tools_for_group(group).is_empty())
    });
    let original_capabilities = context.get("capabilities").cloned();
    let lsp_install_required = context
        .pointer("/readiness/advisories")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.as_str() == Some("lsp_install_required"))
        });

    let Some(readiness) = context.get_mut("readiness").and_then(Value::as_object_mut) else {
        return json!({"applied":false,"actions":[]});
    };
    let original = readiness.clone();
    let actions = readiness
        .entry("next_actions")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut();
    let Some(actions) = actions else {
        return json!({"applied":false,"actions":[]});
    };

    let mut applied = Vec::<String>::new();
    if needs_retrieval {
        promote_before_edit(actions, retrieval_action);
        applied.push(retrieval_action.to_owned());
    }
    if needs_review {
        promote_before_edit(actions, "review_changes");
        applied.push("review_changes".to_owned());
    }
    if needs_semantic {
        if lsp_install_required {
            promote_before_edit(actions, "semantic_provider_install");
            promote_before_edit(actions, "semantic_provider_refresh");
            applied.push("semantic_provider_install".to_owned());
            applied.push("semantic_provider_refresh".to_owned());
        }
        promote_before_edit(actions, "semantic_navigation");
        applied.push("semantic_navigation".to_owned());
    }
    if verification_escalation {
        promote_before_action(actions, "verification_plan", "verify_project");
        applied.push("verification_plan".to_owned());
    }

    let advisories = readiness
        .entry("advisories")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut();
    if let Some(advisories) = advisories {
        for advisory in [
            needs_retrieval.then_some("jev_retrieval_review"),
            needs_review.then_some("jev_worktree_review"),
            needs_semantic.then_some("jev_semantic_navigation"),
            verification_escalation.then_some("jev_verification_escalation"),
        ]
        .into_iter()
        .flatten()
        {
            if !advisories
                .iter()
                .any(|item| item.as_str() == Some(advisory))
            {
                advisories.push(Value::String(advisory.to_owned()));
            }
        }
    }
    let mut promoted_tools = applied.clone();
    if let Some(group) = capability_group {
        for tool in crate::model_tools::model_tools_for_group(group) {
            if !promoted_tools.iter().any(|candidate| candidate == tool) {
                promoted_tools.push((*tool).to_owned());
            }
        }
    }
    if !applied.is_empty() || capability_group.is_some() {
        readiness.insert(
            "decision_assist".to_owned(),
            json!({
                "provider":"jev",
                "authority":"increase_only",
                "candidate_next_action": candidate_next_action,
                "capability_group": capability_group,
                "applied_actions": applied,
                "promoted_tools": promoted_tools,
            }),
        );
    }
    if let Some(capabilities) = context.get_mut("capabilities") {
        crate::model_tools::promote_model_tools(capabilities, &promoted_tools);
    }

    if !context_fits_budget(context) {
        if let Some(readiness) = context.get_mut("readiness").and_then(Value::as_object_mut) {
            *readiness = original;
        }
        if let Some(original_capabilities) = original_capabilities {
            context["capabilities"] = original_capabilities;
        }
        return json!({
            "applied":false,
            "actions":[],
            "reason":"context_budget"
        });
    }

    json!({
        "applied":!applied.is_empty() || capability_group.is_some(),
        "actions":applied,
        "capability_group":capability_group,
        "promoted_tools":promoted_tools,
        "verification_escalation":verification_escalation
    })
}
