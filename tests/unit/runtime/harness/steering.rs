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
                write_paths: None,
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

#[test]
fn scope_completion_corrupt_execution_remains_unknown_in_context_and_handoff() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(root.path().join("src/lib.rs"), "pub fn scope_entry() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    crate::worklist::update(
        &workspace,
        crate::worklist::WorklistUpdate {
            expected_revision: 0,
            goal: Some("Retain scope after damaged state".into()),
            restart: false,
            items: vec![crate::worklist::WorkItemPatch {
                id: "retained".into(),
                title: Some("Retained requirement".into()),
                status: Some(crate::worklist::WorkItemStatus::InProgress),
                write_paths: None,
                depends_on: None,
                note: None,
            }],
        },
    )
    .unwrap();
    crate::execution::refresh(&harness, "demo", &workspace, false).unwrap();
    let directory = crate::evidence_store::workspace_state_directory(&workspace)
        .unwrap()
        .join("execution");
    let damaged = directory.join("99999999999999999999.json");
    std::fs::write(&damaged, "{PRIVATE_CORRUPT_STATE").unwrap();
    assert!(crate::execution::stored_status(&workspace).is_err());
    for budget in [1_000, 2_000] {
        let pack = harness
            .agent_context("demo", &workspace, "inspect scope_entry", budget, &[])
            .unwrap();
        assert_eq!(pack["execution"]["scope_completion"]["allowed"], false);
        assert_eq!(
            pack["execution"]["scope_completion"]["open_items"],
            Value::Null
        );
        assert_eq!(
            pack["execution"]["scope_completion"]["required_action"],
            "restore_execution_state"
        );
        assert_eq!(pack["worklist"]["complete"], false);
        assert!(!serde_json::to_string(&pack)
            .unwrap()
            .contains("PRIVATE_CORRUPT_STATE"));
        assert!(pack["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == "src/lib.rs"));
    }
    let handoff = harness
        .agent_handoff_context("demo", &workspace, "inspect scope_entry")
        .unwrap();
    assert_eq!(handoff["execution"]["scope_completion"]["allowed"], false);
    assert_eq!(
        handoff["execution"]["scope_completion"]["required_action"],
        "restore_execution_state"
    );
    assert_eq!(
        std::fs::read_to_string(&damaged).unwrap(),
        "{PRIVATE_CORRUPT_STATE"
    );
}

#[test]
fn scope_completion_survives_context_compaction_and_worker_handoff() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(root.path().join("src/lib.rs"), "pub fn scope_entry() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    crate::worklist::update(
        &workspace,
        crate::worklist::WorklistUpdate {
            expected_revision: 0,
            goal: Some("Finish retained declared scope".into()),
            restart: false,
            items: ["scope-entry", "retained-requirement"]
                .iter()
                .map(|id| crate::worklist::WorkItemPatch {
                    write_paths: None,
                    id: (*id).into(),
                    title: Some((*id).into()),
                    status: Some(crate::worklist::WorkItemStatus::InProgress),
                    depends_on: None,
                    note: None,
                })
                .collect(),
        },
    )
    .unwrap();
    for budget in [1_000, 2_000] {
        let pack = harness
            .agent_context("demo", &workspace, "Inspect scope_entry", budget, &[])
            .unwrap();
        assert_eq!(pack["execution"]["scope_completion"]["allowed"], false);
        assert_eq!(pack["execution"]["scope_completion"]["open_items"], 2);
        assert_eq!(pack["worklist"]["complete"], false);
        let bytes = serde_json::to_vec(&pack).unwrap().len() as u64;
        assert_eq!(pack["serialized_bytes"], bytes);
        assert!(bytes.div_ceil(4) <= pack["budget"].as_u64().unwrap());
        assert!(pack["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == "src/lib.rs"
                && file["sha256"].as_str().is_some_and(|sha| sha.len() == 64)));
    }
    let handoff = harness
        .agent_handoff_context("demo", &workspace, "Inspect scope_entry")
        .unwrap();
    assert!(handoff.get("worklist").is_none());
    assert!(handoff["execution"].get("checkpoint").is_none());
    assert_eq!(handoff["execution"]["scope_completion"]["allowed"], false);
    assert_eq!(handoff["execution"]["scope_completion"]["open_items"], 2);
    let bytes = serde_json::to_vec(&handoff).unwrap().len() as u64;
    assert_eq!(handoff["serialized_bytes"], bytes);
    assert!(bytes.div_ceil(4) <= handoff["budget"].as_u64().unwrap());
}
