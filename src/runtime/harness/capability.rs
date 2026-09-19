use super::*;

const MAX_RECOMMENDED_TOOLS: usize = 14;

pub(super) fn manifest(
    query: &str,
    requested_scopes: &[String],
    execution: Option<&Value>,
) -> Value {
    let normalized = query.to_ascii_lowercase();
    let phase = execution
        .and_then(|value| value.get("phase"))
        .and_then(Value::as_str);
    let scopes = crate::scopes::canonicalize(requested_scopes);
    let mut recommended = vec![
        "agent_context",
        "search_many",
        "read_files",
        "apply_file_edits",
        "review_changes",
        "verify_project",
    ];

    if execution.is_some() {
        extend_unique(
            &mut recommended,
            &["execution_status", "execution_propose", "worklist_status"],
        );
    }
    let pending_steering = execution
        .and_then(|value| value.get("pending_directive"))
        .is_some_and(|value| !value.is_null());
    let replan_required = execution
        .and_then(|value| value.get("replan_required"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if pending_steering {
        extend_unique(&mut recommended, &["worklist_update"]);
        if replan_required {
            extend_unique(
                &mut recommended,
                &["reconciliation_plan", "reconciliation_status"],
            );
        }
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
        extend_unique(&mut recommended, &["execution_steer"]);
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
        extend_unique(&mut recommended, &["execution_handoff"]);
    }
    if execution
        .and_then(|value| value.pointer("/checkpoint/reconciliation_plan_id"))
        .is_some_and(|value| !value.is_null())
    {
        extend_unique(&mut recommended, &["reconciliation_execution_status"]);
    }
    if execution
        .and_then(|value| value.pointer("/checkpoint/verification_plan_id"))
        .is_some_and(|value| !value.is_null())
    {
        extend_unique(&mut recommended, &["verification_status"]);
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
        extend_unique(
            &mut recommended,
            &["semantic_navigation", "find_symbol", "symbol_context"],
        );
    }
    if planning {
        extend_unique(
            &mut recommended,
            &[
                "design_status",
                "traceability_status",
                "reconciliation_status",
            ],
        );
    }
    if verification {
        extend_unique(
            &mut recommended,
            &[
                "verification_status",
                "evidence_status",
                "verification_history",
            ],
        );
    }
    recommended.truncate(MAX_RECOMMENDED_TOOLS);

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

    json!({
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
        "host_contract": "Preload dev.wcode/preloadRecommended tools; use recommended_tools plus dev.wcode/productScopes to expand specialized capabilities on demand. Explicit user tool requests remain reachable."
    })
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

fn extend_unique<'a>(target: &mut Vec<&'a str>, values: &[&'a str]) {
    for value in values {
        if !target.contains(value) {
            target.push(value);
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/runtime/harness/capability.rs"]
mod tests;
