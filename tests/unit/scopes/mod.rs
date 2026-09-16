use super::*;

#[test]
fn product_scope_registry_matches_wcode_product_model() {
    assert_eq!(registry().len(), ProductScope::ALL.len());
    assert_eq!(parse("software graph"), Some(ProductScope::Graph));
    assert_eq!(parse("verification_mesh"), Some(ProductScope::Verification));
    assert_eq!(
        source_scope("src/evidence/store.rs"),
        Some(ProductScope::Evidence)
    );
    assert_eq!(
        source_scope("src/intelligence/risk.rs"),
        Some(ProductScope::Risk)
    );
    assert_eq!(
        source_scope("src/intelligence/observatory/architecture.rs"),
        Some(ProductScope::Traceability)
    );
    assert_eq!(
        source_scope("src/intelligence/observatory/files.rs"),
        Some(ProductScope::Traceability)
    );
    assert_eq!(source_scope("src/app/mod.rs"), Some(ProductScope::Runtime));
    assert_eq!(source_scope("src/lib.rs"), Some(ProductScope::Runtime));
    assert_eq!(
        source_scope("src/intelligence/runtime/design.rs"),
        Some(ProductScope::Traceability)
    );
    assert_eq!(
        source_scope("src/intelligence/release_gate.rs"),
        Some(ProductScope::Traceability)
    );
}

#[test]
fn every_declared_source_root_maps_back_to_its_scope() {
    for scope in ProductScope::ALL {
        for root in scope.source_roots() {
            let probe = if root.ends_with('/') {
                format!("{root}__scope_probe__.rs")
            } else {
                (*root).to_owned()
            };
            assert_eq!(
                source_scope(&probe),
                Some(scope),
                "declared Product Scope root `{root}` must be recognized by source_scope"
            );
        }
    }
}

#[test]
fn tool_scope_mapping_exposes_cross_scope_context() {
    let scopes = tool_scopes("software_context");
    assert!(scopes.contains(&ProductScope::Design));
    assert!(scopes.contains(&ProductScope::Graph));
    assert!(scopes.contains(&ProductScope::Semantics));
    assert!(scopes.contains(&ProductScope::Traceability));
}

#[test]
fn freeform_business_scopes_are_preserved_while_product_aliases_canonicalize() {
    assert_eq!(canonical_name("software graph"), "graph");
    assert_eq!(canonical_name("Analytics"), "analytics");
}
