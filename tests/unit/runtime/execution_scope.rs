use super::*;
use crate::worklist::{self, WorkItemPatch, WorkItemStatus, WorklistUpdate};

fn phase_signals(ready: bool) -> ExecutionSignals {
    // Settlement projection fixture; this is not verification Evidence.
    ExecutionSignals {
        repository_revision: Revision {
            design: None,
            code: "revision-a".into(),
        },
        reconciliation_plan_id: None,
        verification_plan_id: Some("VP-phase".into()),
        verification_ready: Some(ready),
        verification_blockers: if ready {
            vec![]
        } else {
            vec!["required_check_failed".into()]
        },
        reconciliation_converged: None,
        reconciliation_blockers: vec![],
    }
}

fn update_items(workspace: &Workspace, revision: u64, states: &[(&str, WorkItemStatus)]) {
    worklist::update(
        workspace,
        WorklistUpdate {
            expected_revision: revision,
            goal: (revision == 0)
                .then(|| "Complete all declared features, including blocked work".into()),
            restart: false,
            items: states
                .iter()
                .map(|(id, status)| WorkItemPatch {
                    write_paths: None,
                    id: (*id).into(),
                    title: Some((*id).into()),
                    status: Some(*status),
                    depends_on: None,
                    note: None,
                })
                .collect(),
        },
    )
    .unwrap();
}

#[test]
fn scope_completion_preserves_unfinished_work_even_when_phase_verification_passes() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    update_items(
        &workspace,
        0,
        &[
            ("implemented", WorkItemStatus::Done),
            ("next-feature", WorkItemStatus::Pending),
            ("retained-blocker", WorkItemStatus::Blocked),
        ],
    );
    let snapshot = worklist::snapshot(&workspace).unwrap().unwrap();
    let executing = sync(&workspace, &snapshot, None, false, phase_signals(true)).unwrap();
    assert_eq!(
        executing["scope_completion"],
        json!({
            "allowed":false, "open_items":2, "required_action":"continue"
        })
    );
    assert_eq!(executing["checkpoint"]["verification_ready"], true);
    assert_eq!(executing["active"], true);

    update_items(&workspace, 1, &[("next-feature", WorkItemStatus::Done)]);
    let snapshot = worklist::snapshot(&workspace).unwrap().unwrap();
    let blocked = sync(
        &workspace,
        &snapshot,
        load(&workspace).unwrap(),
        false,
        phase_signals(true),
    )
    .unwrap();
    assert_eq!(
        blocked["scope_completion"],
        json!({
            "allowed":false, "open_items":1, "required_action":"resolve_blockers"
        })
    );
    assert_eq!(
        snapshot.items.len(),
        3,
        "Blocked requirements must remain in scope"
    );

    update_items(&workspace, 2, &[("retained-blocker", WorkItemStatus::Done)]);
    let snapshot = worklist::snapshot(&workspace).unwrap().unwrap();
    let failed = sync(
        &workspace,
        &snapshot,
        load(&workspace).unwrap(),
        false,
        phase_signals(false),
    )
    .unwrap();
    assert_eq!(
        failed["scope_completion"],
        json!({
            "allowed":false, "open_items":0, "required_action":"verify"
        })
    );
    let completed = sync(
        &workspace,
        &snapshot,
        load(&workspace).unwrap(),
        false,
        phase_signals(true),
    )
    .unwrap();
    assert_eq!(
        completed["scope_completion"],
        json!({
            "allowed":true, "open_items":0, "required_action":"none"
        })
    );
    let projected = view::summary(&completed).expect("completed execution remains visible");
    assert_eq!(projected["phase"], "completed");
    assert_eq!(projected["scope_completion"], completed["scope_completion"]);
    assert!(view::summary(&empty_status()).is_none());
    assert_eq!(empty_status()["scope_completion"]["allowed"], false);
    assert_eq!(
        empty_status()["scope_completion"]["open_items"],
        Value::Null
    );
}
