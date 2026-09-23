use super::*;

#[cfg(test)]
const MAX_RECOMMENDED_TOOLS: usize = 14;

pub(super) fn manifest(
    query: &str,
    requested_scopes: &[String],
    execution: Option<&Value>,
) -> Value {
    let normalized = super::super::harness_retrieval::intent_query(query);
    let phase = execution
        .and_then(|value| value.get("phase"))
        .and_then(Value::as_str);
    let scopes = crate::scopes::canonicalize(requested_scopes);
    let mut recommended = crate::harness::default_coding_tools().to_vec();

    if execution.is_some() {
        promote_unique(
            &mut recommended,
            &["execution_status", "execution_propose", "worklist_status"],
        );
    }
    if execution.is_some()
        && contains_any(
            &normalized,
            &[
                "steer",
                "refine objective",
                "change scope",
                "stronger verification",
                "strengthen verification",
                "调整目标",
                "变更范围",
                "加强验证",
                "转向",
            ],
        )
    {
        promote_unique(&mut recommended, &["execution_steer"]);
    }
    if execution.is_some()
        && contains_any(
            &normalized,
            &[
                "handoff",
                "clean context",
                "fresh context",
                "交接",
                "干净上下文",
            ],
        )
    {
        promote_unique(&mut recommended, &["execution_handoff"]);
    }
    if execution
        .and_then(|value| value.pointer("/checkpoint/reconciliation_plan_id"))
        .is_some_and(|value| !value.is_null())
    {
        promote_unique(&mut recommended, &["reconciliation_execution_status"]);
    }
    if execution
        .and_then(|value| value.pointer("/checkpoint/verification_plan_id"))
        .is_some_and(|value| !value.is_null())
    {
        promote_unique(&mut recommended, &["verification_status"]);
    }

    let semantic = contains_any(
        &normalized,
        &[
            "caller",
            "callee",
            "reference",
            "implementation",
            "semantic",
            "symbol",
            "impact",
            "调用",
            "引用",
            "实现",
            "影响",
        ],
    ) || scopes
        .iter()
        .any(|scope| scope == "semantics" || scope == "graph");
    let planning = contains_any(
        &normalized,
        &[
            "plan",
            "design",
            "architecture",
            "requirement",
            "reconcile",
            "规划",
            "设计",
            "架构",
            "需求",
        ],
    ) || scopes
        .iter()
        .any(|scope| scope == "design" || scope == "reconciliation");
    let verification = phase == Some("verifying")
        || contains_any(
            &normalized,
            &[
                "verify", "test", "review", "evidence", "验证", "测试", "审查", "证据",
            ],
        )
        || scopes
            .iter()
            .any(|scope| scope == "verification" || scope == "evidence");

    if semantic {
        promote_unique(
            &mut recommended,
            &["semantic_navigation", "find_symbol", "symbol_context"],
        );
    }
    if planning {
        promote_unique(
            &mut recommended,
            &[
                "design_status",
                "traceability_status",
                "reconciliation_status",
            ],
        );
    }
    if verification {
        promote_unique(
            &mut recommended,
            &[
                "verification_status",
                "evidence_status",
                "verification_history",
            ],
        );
    }
    let profile = if phase == Some("verifying") || verification {
        "verification"
    } else if planning {
        "planning"
    } else if semantic {
        "semantic_navigation"
    } else {
        "coding"
    };
    let active_scopes = if scopes.is_empty() {
        inferred_scopes(profile)
    } else {
        scopes
    };
    let active = active_scopes
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let deferred_scopes = crate::scopes::ProductScope::ALL
        .into_iter()
        .map(|scope| scope.as_str())
        .filter(|scope| !active.contains(scope))
        .collect::<Vec<_>>();

    let mut result = json!({
        "profile": profile,
        "disclosure": "metadata_first",
        "catalog": "stable",
        "recommended_tools": recommended,
        "active_product_scopes": active_scopes,
        "deferred_product_scopes": deferred_scopes,
        "mandatory_controls": [
            "workspace_boundary",
            "authorization",
            "sha_preconditions",
            "verification_evidence"
        ],
        "host_contract": "Preload only dev.wcode/preloadRecommended tools when the host supports progressive disclosure. After agent_context, prefer recommended_actions and expand on-demand tools by their task-manifest group or dev.wcode/productScopes. Explicit user tool requests and required recovery/safety/verification actions remain reachable."
    });
    // Pending user steering must survive every bounded specialist selection.
    crate::harness::prioritize_model_tools(&mut result, &required_recovery_tools(execution));
    result
}

pub(super) fn required_recovery_tools(execution: Option<&Value>) -> Vec<String> {
    let mut tools = Vec::new();
    if execution
        .and_then(|value| value.get("pending_directive"))
        .is_some_and(|value| !value.is_null())
    {
        tools.push("worklist_update".to_owned());
    }
    if execution
        .and_then(|value| value.get("replan_required"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        tools.extend(["reconciliation_plan", "reconciliation_status"].map(str::to_owned));
    }
    tools
}

fn inferred_scopes(profile: &str) -> Vec<String> {
    let values: &[&str] = match profile {
        "planning" => &[
            "design",
            "traceability",
            "reconciliation",
            "verification",
            "workspace",
        ],
        "verification" => &["verification", "evidence", "workspace"],
        "semantic_navigation" => &["graph", "semantics", "workspace"],
        _ => &["workspace", "graph", "verification"],
    };
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn promote_unique<'a>(target: &mut Vec<&'a str>, values: &[&'a str]) {
    for (index, value) in values.iter().enumerate() {
        if let Some(existing) = target.iter().position(|candidate| candidate == value) {
            target.remove(existing);
        }
        target.insert(index.min(target.len()), value);
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/runtime/harness/capability.rs"]
mod tests;
