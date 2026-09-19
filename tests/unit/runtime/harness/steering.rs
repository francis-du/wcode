use super::*;

#[test]
fn rejects_unbounded_parallelism() {
    assert!(ToolHarness::new(0).is_err());
    assert_eq!(
        ToolHarness::new(MAX_PARALLEL_TOOLS)
            .expect("documented maximum should be accepted")
            .max_parallel(),
        MAX_PARALLEL_TOOLS
    );
    assert!(ToolHarness::new(MAX_PARALLEL_TOOLS + 1).is_err());
}

#[test]
fn agent_context_restores_pending_execution_steering() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(root.path().join("src/lib.rs"), "pub fn entry() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();

    crate::worklist::update(
        &workspace,
        crate::worklist::WorklistUpdate {
            expected_revision: 0,
            goal: Some("resume structured steering".to_owned()),
            restart: false,
            items: vec![crate::worklist::WorkItemPatch {
                id: "steer".to_owned(),
                title: Some("Apply steering first".to_owned()),
                status: Some(crate::worklist::WorkItemStatus::InProgress),
                depends_on: None,
                note: None,
            }],
        },
    )
    .unwrap();
    let execution = crate::execution::refresh(&harness, "demo", &workspace, false).unwrap();
    let steered = crate::execution::steer(
        &workspace,
        crate::execution::ExecutionDirectiveInput {
            expected_revision: execution["revision"].as_u64().unwrap(),
            kind: crate::execution::ExecutionDirectiveKind::StrengthenVerification,
            summary: "Run adversarial verification before settlement".to_owned(),
            requested_by: "user:test".to_owned(),
            objective: None,
            scopes: Vec::new(),
            verification_strength: Some("adversarial".to_owned()),
        },
    )
    .unwrap();

    let context = harness
        .agent_context("demo", &workspace, "continue implementation", 2_000, &[])
        .unwrap();
    assert_eq!(
        context["execution"]["pending_directive"],
        steered["pending_directive"]
    );
    assert_eq!(
        context["execution"]["pending_directive"]["kind"],
        "strengthen_verification"
    );
    assert_eq!(
        context["execution"]["pending_directive"]["verification_strength"],
        "adversarial"
    );
}
