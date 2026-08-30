use super::*;

#[test]
fn intelligence_check_requires_valid_complete_scoped_state() {
    let healthy = vec![json!({
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
        "conventions": {"errors": 0, "warnings": 2, "truncated": false}
    })];
    assert!(intelligence_check_failures(&healthy).is_empty());

    let broken = vec![json!({
        "workspace": "demo",
        "design": {"initialized": true, "valid": false},
        "traceability": {
            "requirement_to_component": {"percent": 100},
            "design_to_implementation": {"percent": 99},
            "acceptance_to_verification": {"percent": 100}
        },
        "product_scope_required": true,
        "scope_status": {
            "source_files": 12,
            "mapped_files": 11,
            "unmapped_files": ["src/orphan.rs"],
            "truncated": false
        },
        "conventions": {"errors": 1, "warnings": 0, "truncated": false}
    })];
    let failures = intelligence_check_failures(&broken);
    assert!(failures
        .iter()
        .any(|failure| failure.contains("Design State is invalid")));
    assert!(failures
        .iter()
        .any(|failure| failure.contains("design→implementation")));
    assert!(failures
        .iter()
        .any(|failure| failure.contains("Product Scope")));
    assert!(failures
        .iter()
        .any(|failure| failure.contains("Convention")));

    let generic = vec![json!({
        "workspace": "third-party",
        "design": {"initialized": true, "valid": true},
        "traceability": {
            "requirement_to_component": {"percent": 100},
            "design_to_implementation": {"percent": 100},
            "acceptance_to_verification": {"percent": 100}
        },
        "product_scope_required": false,
        "scope_status": {
            "source_files": 12,
            "mapped_files": 0,
            "unmapped_files": ["src/lib.rs"],
            "truncated": false
        },
        "conventions": {"errors": 0, "warnings": 0, "truncated": false}
    })];
    assert!(intelligence_check_failures(&generic).is_empty());
}
