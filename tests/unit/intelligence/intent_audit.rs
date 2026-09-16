use super::*;

fn core_policy_intent(path: &str, policy: &str) -> ChangeIntent {
    ChangeIntent::ChangeBehavior {
        target: path.to_owned(),
        desired: serde_json::json!({
            "state": "conform_to_core_policy",
            "policy": policy,
        }),
        constraints: vec!["CONSTRAINT-SOURCE-DECOMPOSITION".to_owned()],
    }
}

#[test]
fn core_policy_intent_audit_blocks_false_convergence_until_structure_changes() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("src/oversized.rs"),
        "// line\n".repeat(1_001),
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let intents = vec![core_policy_intent(
        "src/oversized.rs",
        "oversized-source-module",
    )];

    let (checked, blockers) = reconciliation_intent_audit(&workspace, &intents).unwrap();
    assert_eq!(checked, 1);
    assert_eq!(blockers.len(), 1);
    assert!(blockers[0].contains("oversized-source-module"));
    assert!(blockers[0].contains("src/oversized.rs"));

    std::fs::write(root.path().join("src/oversized.rs"), "// line\n".repeat(10)).unwrap();
    let (checked, blockers) = reconciliation_intent_audit(&workspace, &intents).unwrap();
    assert_eq!(checked, 1);
    assert!(blockers.is_empty());
}

#[test]
fn malformed_core_policy_intent_fails_closed() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let intents = vec![ChangeIntent::ChangeBehavior {
        target: "src/lib.rs".to_owned(),
        desired: serde_json::json!({"state": "conform_to_core_policy"}),
        constraints: Vec::new(),
    }];

    let (checked, blockers) = reconciliation_intent_audit(&workspace, &intents).unwrap();
    assert_eq!(checked, 1);
    assert_eq!(blockers.len(), 1);
    assert!(blockers[0].contains("missing policy"));
}
