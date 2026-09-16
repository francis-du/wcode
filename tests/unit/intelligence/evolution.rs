use serde_json::json;

fn healthy_release_state() -> serde_json::Value {
    json!({
        "workspace": "demo",
        "design": {"initialized": true, "valid": true},
        "traceability": {
            "requirement_to_component": {"percent": 100},
            "design_to_implementation": {"percent": 100},
            "acceptance_to_verification": {"percent": 100}
        },
        "product_scope_required": true,
        "scope_status": {
            "source_files": 12,
            "mapped_files": 12,
            "unmapped_files": [],
            "truncated": false
        },
        "conventions": {"errors": 0, "warnings": 0, "truncated": false}
    })
}

#[test]
fn release_gate_evolution_replay_has_no_sticky_success_or_failure() {
    let healthy = healthy_release_state();
    assert!(crate::intelligence::release_gate::failures(std::slice::from_ref(&healthy)).is_empty());

    let mut broken_trace = healthy.clone();
    broken_trace["traceability"]["design_to_implementation"]["percent"] = json!(99);
    let failures = crate::intelligence::release_gate::failures(&[broken_trace]);
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("design→implementation"));

    assert!(crate::intelligence::release_gate::failures(std::slice::from_ref(&healthy)).is_empty());

    let mut broken_convention = healthy.clone();
    broken_convention["conventions"]["errors"] = json!(1);
    let failures = crate::intelligence::release_gate::failures(&[broken_convention]);
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("Convention"));

    assert!(crate::intelligence::release_gate::failures(&[healthy]).is_empty());
}
