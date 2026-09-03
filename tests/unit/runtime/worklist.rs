use super::*;
use crate::workspace::Workspace;

#[test]
fn worklist_preserves_unfinished_items_and_exposes_runnable_parallel_lanes() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();

    let created = update(
        &workspace,
        WorklistUpdate {
            expected_revision: 0,
            goal: Some("ship the change".to_owned()),
            restart: false,
            items: vec![
                WorkItemPatch {
                    id: "research".to_owned(),
                    title: Some("Research current behavior".to_owned()),
                    status: None,
                    depends_on: None,
                    note: None,
                },
                WorkItemPatch {
                    id: "docs".to_owned(),
                    title: Some("Update docs".to_owned()),
                    status: None,
                    depends_on: None,
                    note: None,
                },
                WorkItemPatch {
                    id: "verify".to_owned(),
                    title: Some("Verify everything".to_owned()),
                    status: None,
                    depends_on: Some(vec!["research".to_owned(), "docs".to_owned()]),
                    note: None,
                },
            ],
        },
    )
    .unwrap();
    assert_eq!(created["revision"], 1);
    assert_eq!(
        created["parallel_runnable"],
        serde_json::json!(["docs", "research"])
    );

    let updated = update(
        &workspace,
        WorklistUpdate {
            expected_revision: 1,
            goal: None,
            restart: false,
            items: vec![WorkItemPatch {
                id: "research".to_owned(),
                title: None,
                status: Some(WorkItemStatus::Done),
                depends_on: None,
                note: None,
            }],
        },
    )
    .unwrap();
    assert_eq!(updated["revision"], 2);
    assert!(updated["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["id"] == "docs" && item["status"] == "pending"));
    assert_eq!(updated["runnable"], serde_json::json!(["docs"]));
}

#[test]
fn stale_revision_and_premature_restart_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    update(
        &workspace,
        WorklistUpdate {
            expected_revision: 0,
            goal: Some("keep progress".to_owned()),
            restart: false,
            items: vec![WorkItemPatch {
                id: "one".to_owned(),
                title: Some("First item".to_owned()),
                status: None,
                depends_on: None,
                note: None,
            }],
        },
    )
    .unwrap();

    assert!(update(
        &workspace,
        WorklistUpdate {
            expected_revision: 0,
            goal: None,
            restart: false,
            items: vec![],
        },
    )
    .unwrap_err()
    .to_string()
    .contains("revision changed"));
    assert!(update(
        &workspace,
        WorklistUpdate {
            expected_revision: 1,
            goal: Some("new work".to_owned()),
            restart: true,
            items: vec![],
        },
    )
    .unwrap_err()
    .to_string()
    .contains("unfinished items remain"));

    update(
        &workspace,
        WorklistUpdate {
            expected_revision: 1,
            goal: None,
            restart: false,
            items: vec![WorkItemPatch {
                id: "one".to_owned(),
                title: None,
                status: Some(WorkItemStatus::Blocked),
                depends_on: None,
                note: Some("waiting for user input".to_owned()),
            }],
        },
    )
    .unwrap();
    let blocked_restart = update(
        &workspace,
        WorklistUpdate {
            expected_revision: 2,
            goal: Some("new work".to_owned()),
            restart: true,
            items: vec![],
        },
    )
    .unwrap_err()
    .to_string();
    assert!(blocked_restart.contains("blocked items are preserved"));
}

#[test]
fn completed_worklist_can_restart_without_reusing_old_items() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    update(
        &workspace,
        WorklistUpdate {
            expected_revision: 0,
            goal: Some("first".to_owned()),
            restart: false,
            items: vec![WorkItemPatch {
                id: "one".to_owned(),
                title: Some("Finish first".to_owned()),
                status: Some(WorkItemStatus::Done),
                depends_on: None,
                note: None,
            }],
        },
    )
    .unwrap();
    let restarted = update(
        &workspace,
        WorklistUpdate {
            expected_revision: 1,
            goal: Some("second".to_owned()),
            restart: true,
            items: vec![WorkItemPatch {
                id: "two".to_owned(),
                title: Some("Start second".to_owned()),
                status: None,
                depends_on: None,
                note: None,
            }],
        },
    )
    .unwrap();
    assert_eq!(restarted["goal"], "second");
    assert_eq!(restarted["items"].as_array().unwrap().len(), 1);
    assert_eq!(restarted["items"][0]["id"], "two");
}
