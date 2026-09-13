use crate::engineering_journal::{change_fingerprint, load_recent, persist, EngineeringMilestone};
use crate::workspace::Workspace;
use std::fs;

#[test]
fn engineering_journal_persists_only_bounded_structured_milestones() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn value() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let mut milestone = EngineeringMilestone::new(
        "apply_edits",
        "change",
        "succeeded",
        12,
        vec!["src/lib.rs".into()],
    )
    .unwrap();
    milestone.verification_level = None;
    persist(&workspace, &milestone).unwrap();

    let history = load_recent(&workspace, 16).unwrap();
    assert_eq!(history.retained_records, 1);
    assert!(!history.truncated);
    assert_eq!(history.records, vec![milestone]);

    let encoded = serde_json::to_string(&history.records[0]).unwrap();
    for forbidden in [
        "prompt",
        "query",
        "chain_of_thought",
        "arguments",
        "source",
        "old_text",
        "new_text",
    ] {
        assert!(!encoded.contains(forbidden));
    }
}

#[test]
fn engineering_journal_keeps_prompt_free_context_trajectory_metadata() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn value() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let mut milestone = EngineeringMilestone::new(
        "agent_context",
        "understand",
        "succeeded",
        7,
        vec!["src/lib.rs".into()],
    )
    .unwrap();
    milestone.retrieval_intent = Some("trace_to_code".into());
    persist(&workspace, &milestone).unwrap();
    let history = load_recent(&workspace, 4).unwrap();
    assert_eq!(
        history.records[0].retrieval_intent.as_deref(),
        Some("trace_to_code")
    );
    let encoded = serde_json::to_string(&history.records[0]).unwrap();
    assert!(!encoded.contains("prompt"));
    assert!(!encoded.contains("query"));
    assert!(EngineeringMilestone::new(
        "agent_context",
        "understand",
        "succeeded",
        7,
        vec!["src/lib.rs".into()],
    )
    .is_ok());
}

#[test]
fn engineering_journal_rejects_escaping_paths_and_fingerprints_metadata() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    assert!(EngineeringMilestone::new(
        "apply_edits",
        "change",
        "succeeded",
        1,
        vec!["../outside.rs".into()],
    )
    .is_err());
    let before = change_fingerprint(&workspace).unwrap();
    let milestone =
        EngineeringMilestone::new("verify_project", "prove", "succeeded", 42, Vec::new()).unwrap();
    persist(&workspace, &milestone).unwrap();
    let after = change_fingerprint(&workspace).unwrap();
    assert_ne!(before, after);
}
