use super::*;

#[test]
fn coding_manifest_keeps_mandatory_controls_and_defers_specialized_scopes() {
    let manifest = manifest("fix the parser bug", &[], None);
    assert_eq!(manifest["profile"], "coding");
    assert_eq!(manifest["disclosure"], "metadata_first");
    assert!(manifest["recommended_tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool == "agent_context"));
    assert!(manifest["recommended_tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool == "verify_project"));
    for control in [
        "workspace_boundary",
        "authorization",
        "sha_preconditions",
        "verification_evidence",
    ] {
        assert!(manifest["mandatory_controls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == control));
    }
    assert!(manifest["deferred_product_scopes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|scope| scope == "semantics"));
}

#[test]
fn execution_phase_and_query_promote_only_relevant_specialized_tools() {
    let execution = serde_json::json!({
        "phase": "verifying",
        "checkpoint": {
            "reconciliation_plan_id": "RC-1",
            "verification_plan_id": "VP-1"
        }
    });
    let manifest = manifest(
        "review semantic callers before verification",
        &["semantics".to_owned()],
        Some(&execution),
    );
    assert_eq!(manifest["profile"], "verification");
    let tools = manifest["recommended_tools"].as_array().unwrap();
    for expected in [
        "execution_status",
        "execution_propose",
        "worklist_status",
        "reconciliation_execution_status",
        "verification_status",
        "semantic_navigation",
    ] {
        assert!(
            tools.iter().any(|tool| tool == expected),
            "missing recommended tool {expected}"
        );
    }
    assert!(
        tools.len() <= MAX_RECOMMENDED_TOOLS,
        "manifest must remain bounded"
    );
}

#[test]
fn pending_steering_promotes_authoritative_apply_and_replan_tools() {
    let execution = serde_json::json!({
        "phase": "blocked",
        "pending_directive": {
            "kind": "change_scope",
            "summary": "Expand scope"
        },
        "replan_required": true,
        "checkpoint": {
            "reconciliation_plan_id": "RC-old"
        }
    });
    let continuation = manifest("continue after scope change", &[], Some(&execution));
    let tools = continuation["recommended_tools"].as_array().unwrap();
    for expected in [
        "worklist_update",
        "reconciliation_plan",
        "reconciliation_status",
    ] {
        assert!(
            tools.iter().any(|tool| tool == expected),
            "missing {expected}"
        );
    }
    assert!(tools.len() <= MAX_RECOMMENDED_TOOLS);

    let steering = manifest("steer: change scope", &[], Some(&execution));
    assert!(steering["recommended_tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool == "execution_steer"));
    let handoff = manifest("handoff to a clean context", &[], Some(&execution));
    assert!(handoff["recommended_tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool == "execution_handoff"));
}

#[test]
fn explicit_design_scope_promotes_planning_without_provider_specific_routing() {
    let manifest = manifest("inspect current state", &["design".to_owned()], None);
    assert_eq!(manifest["profile"], "planning");
    let encoded = serde_json::to_string(&manifest).unwrap();
    assert!(encoded.contains("design_status"));
    assert!(encoded.contains("traceability_status"));
    assert!(!encoded.to_ascii_lowercase().contains("claude"));
    assert!(!encoded.to_ascii_lowercase().contains("openai"));
    assert!(encoded.len() < 3_000);
}
