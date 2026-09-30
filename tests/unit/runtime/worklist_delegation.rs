use super::*;

fn setup() -> (tempfile::TempDir, Workspace, ToolHarness) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/a.rs"), "pub fn a() {}\n").unwrap();
    fs::write(root.path().join("src/b.rs"), "pub fn b() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    update(
        &workspace,
        WorklistUpdate {
            expected_revision: 0,
            goal: Some("implement isolated tasks".into()),
            restart: false,
            items: vec![
                patch("a", &["src/a.rs"], &[]),
                patch("overlap", &["src"], &[]),
                patch("readonly", &[], &[]),
                patch("dependent", &["src/b.rs"], &["a"]),
            ],
        },
    )
    .unwrap();
    (root, workspace, ToolHarness::new(2).unwrap())
}

fn patch(id: &str, paths: &[&str], deps: &[&str]) -> WorkItemPatch {
    WorkItemPatch {
        id: id.into(),
        title: Some(format!("Inspect task {id} source")),
        status: None,
        depends_on: Some(deps.iter().map(|s| s.to_string()).collect()),
        note: None,
        write_paths: Some(paths.iter().map(|s| s.to_string()).collect()),
    }
}

fn take(harness: &ToolHarness, workspace: &Workspace, item: &str, revision: u64) -> Result<Value> {
    claim(
        harness,
        "demo",
        workspace,
        WorklistClaimInput {
            expected_revision: revision,
            expected_repository_revision: harness.current_revision(workspace)?,
            item_id: item.into(),
            actor: "worker-a".into(),
            claim_id: None,
        },
    )
}

fn report(
    harness: &ToolHarness,
    workspace: &Workspace,
    revision: u64,
    token: &str,
) -> Result<Value> {
    submit(
        harness,
        "demo",
        workspace,
        WorklistSubmitInput {
            expected_revision: revision,
            expected_repository_revision: harness.current_revision(workspace)?,
            item_id: "a".into(),
            claim_id: token.into(),
            outcome: WorkItemOutcome::Complete,
            summary: "Completed source inspection.".into(),
            evidence_ids: vec![],
        },
    )
}

#[test]
fn worklist_claim_is_private_scoped_and_excludes_live_overlapping_lanes() {
    let (_root, workspace, harness) = setup();
    let owned = take(&harness, &workspace, "a", 1).unwrap();
    let token = owned["claim_id"].as_str().unwrap();
    let public = status(&workspace).unwrap();
    assert!(!serde_json::to_string(&public).unwrap().contains(token));
    assert!(!serde_json::to_string(&active_summary(&workspace).unwrap())
        .unwrap()
        .contains(token));
    assert_eq!(owned["worklist"]["revision"], 2);
    assert_eq!(public["runnable"], json!(["readonly"]));
    assert_eq!(owned["handoff"]["write_paths"], json!(["src/a.rs"]));
    assert!(owned["handoff"]["source"][0]["metadata"]["sha256"].is_string());
    assert!(owned["handoff"]["agent_context"]["checks"].is_array());
    assert!(owned["handoff"]["agent_context"].get("worklist").is_none());
    assert!(take(&harness, &workspace, "overlap", 2)
        .unwrap_err()
        .to_string()
        .contains("conflicts"));
    assert!(take(&harness, &workspace, "dependent", 2)
        .unwrap_err()
        .to_string()
        .contains("dependencies"));
    assert_eq!(status(&workspace).unwrap()["revision"], 2);
}

#[test]
fn worklist_claim_rejects_false_stale_tokens_and_protects_active_task_updates() {
    let (_root, workspace, harness) = setup();
    let owned = take(&harness, &workspace, "a", 1).unwrap();
    let token = owned["claim_id"].as_str().unwrap();
    assert!(report(&harness, &workspace, 2, "WC-false").is_err());
    assert!(report(&harness, &workspace, 1, token)
        .unwrap_err()
        .to_string()
        .contains("revision changed"));
    assert!(update(
        &workspace,
        WorklistUpdate {
            expected_revision: 2,
            goal: None,
            restart: false,
            items: vec![WorkItemPatch {
                id: "a".into(),
                title: None,
                status: Some(WorkItemStatus::Done),
                depends_on: None,
                note: None,
                write_paths: None
            }]
        }
    )
    .is_err());
    let renewed = claim(
        &harness,
        "demo",
        &workspace,
        WorklistClaimInput {
            expected_revision: 2,
            expected_repository_revision: harness.current_revision(&workspace).unwrap(),
            item_id: "a".into(),
            actor: "worker-a".into(),
            claim_id: Some(token.into()),
        },
    )
    .unwrap();
    assert_eq!(renewed["claim_id"], token);
    assert!(renewed["handoff"]["agent_context"].is_null());
    assert_eq!(status(&workspace).unwrap()["revision"], 3);
}

#[test]
fn worklist_submit_accepts_guarded_edits_but_report_is_not_verification() {
    let (root, workspace, harness) = setup();
    let owned = take(&harness, &workspace, "a", 1).unwrap();
    let token = owned["claim_id"].as_str().unwrap();
    let initial = harness.current_revision(&workspace).unwrap();
    fs::write(
        root.path().join("src/a.rs"),
        "pub fn a() { let _value = 1; }\n",
    )
    .unwrap();
    let stale = WorklistSubmitInput {
        expected_revision: 2,
        expected_repository_revision: initial,
        item_id: "a".into(),
        claim_id: token.into(),
        outcome: WorkItemOutcome::Complete,
        summary: "Changed source.".into(),
        evidence_ids: vec![],
    };
    assert!(submit(&harness, "demo", &workspace, stale)
        .unwrap_err()
        .to_string()
        .contains("repository revision changed"));
    let reported = report(&harness, &workspace, 2, token).unwrap();
    assert_eq!(reported["result"]["proof_status"], "not_reported");
    assert_eq!(reported["result"]["outcome"], "complete");
    assert_ne!(
        reported["result"]["base_revision"],
        reported["result"]["repository_revision"]
    );
    let id = reported["result"]["id"].as_str().unwrap();
    let public = status(&workspace).unwrap();
    assert_eq!(public["items"][0]["result"]["id"], id);
    assert!(public["items"][0].get("claim").is_none());
    assert!(public["runnable"]
        .as_array()
        .unwrap()
        .contains(&json!("dependent")));
    assert!(report(&harness, &workspace, 3, token).is_err());
}

#[test]
fn expired_worklist_claim_is_visible_reclaimable_and_old_token_cannot_submit() {
    let (_root, workspace, harness) = setup();
    let owned = take(&harness, &workspace, "a", 1).unwrap();
    let token = owned["claim_id"].as_str().unwrap();
    let mut list = load(&workspace).unwrap().unwrap();
    let claim = list.items[0].claim.as_mut().unwrap();
    claim.claimed_at_ms = 1;
    claim.expires_at_ms = 2;
    commit(&workspace, &mut list).unwrap();
    let public = status(&workspace).unwrap();
    assert_eq!(public["items"][0]["claim"]["expired"], true);
    assert!(public["runnable"].as_array().unwrap().contains(&json!("a")));
    assert!(report(&harness, &workspace, 3, token).is_err());
    let next = take(&harness, &workspace, "a", 3).unwrap();
    assert_ne!(next["claim_id"], token);
}

#[test]
fn worklist_scopes_reject_root_traversal_protected_and_symlinks() {
    let (root, workspace, _harness) = setup();
    for path in [".", "../src", ".git/config", "src\\a.rs", ".env"] {
        assert!(
            canonical_paths(&workspace, &[path.into()]).is_err(),
            "{path}"
        );
    }
    assert_eq!(
        canonical_paths(&workspace, &["./src/a.rs".into(), "src/a.rs".into()]).unwrap(),
        vec!["src/a.rs"]
    );
    assert!(!overlaps(&["src/a".into()], &["src/another".into()]));
    assert!(overlaps(&["src".into()], &["src/a.rs".into()]));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.path().join("src"), root.path().join("linked")).unwrap();
        assert!(canonical_paths(&workspace, &["linked/a.rs".into()]).is_err());
    }
}

#[test]
fn worklist_submit_rejects_fabricated_evidence_without_releasing_claim() {
    let (_root, workspace, harness) = setup();
    let owned = take(&harness, &workspace, "a", 1).unwrap();
    let request = WorklistSubmitInput {
        expected_revision: 2,
        expected_repository_revision: harness.current_revision(&workspace).unwrap(),
        item_id: "a".into(),
        claim_id: owned["claim_id"].as_str().unwrap().into(),
        outcome: WorkItemOutcome::Complete,
        summary: "Claimed all checks pass.".into(),
        evidence_ids: vec!["EV-invented".into()],
    };
    assert!(submit(&harness, "demo", &workspace, request)
        .unwrap_err()
        .to_string()
        .contains("evidence reference"));
    let public = status(&workspace).unwrap();
    assert_eq!(public["revision"], 2);
    assert!(public["items"][0].get("claim").is_some());
    assert!(public["items"][0].get("result").is_none());
}

#[test]
fn worklist_failed_handoff_neither_publishes_claim_nor_holds_update_lock() {
    let (_root, workspace, harness) = setup();
    let result = claim_with_context(
        &harness,
        &workspace,
        WorklistClaimInput {
            expected_revision: 1,
            expected_repository_revision: harness.current_revision(&workspace).unwrap(),
            item_id: "a".into(),
            actor: "worker".into(),
            claim_id: None,
        },
        |_| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while update_lock().try_lock().is_err() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "context builder held the global update lock"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            bail!("context fixture unavailable")
        },
    );
    assert!(result.unwrap_err().to_string().contains("context fixture"));
    let public = status(&workspace).unwrap();
    assert_eq!(public["revision"], 1);
    assert!(public["items"][0].get("claim").is_none());
}

#[test]
fn worklist_parallel_runnable_is_a_nonoverlapping_subset() {
    let (_root, workspace, _harness) = setup();
    let public = status(&workspace).unwrap();
    assert_eq!(public["runnable"], json!(["a", "overlap", "readonly"]));
    assert_eq!(public["parallel_runnable"], json!(["a", "readonly"]));
}

#[test]
fn simultaneous_worklist_claims_cannot_duplicate_one_lane() {
    let (_root, workspace, harness) = setup();
    let revision = harness.current_revision(&workspace).unwrap();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let run = || {
            claim_with_context(
                &harness,
                &workspace,
                WorklistClaimInput {
                    expected_revision: 1,
                    expected_repository_revision: revision.clone(),
                    item_id: "a".into(),
                    actor: "worker".into(),
                    claim_id: None,
                },
                |_| {
                    barrier.wait();
                    Ok(json!({"checks":[]}))
                },
            )
        };
        let first = scope.spawn(run);
        let second = scope.spawn(run);
        let successes = usize::from(first.join().unwrap().is_ok())
            + usize::from(second.join().unwrap().is_ok());
        assert_eq!(successes, 1);
    });
    assert_eq!(status(&workspace).unwrap()["revision"], 2);
}

#[test]
fn worklist_reports_bound_unicode_scalars_and_keep_live_dependencies_complete() {
    let (_root, workspace, harness) = setup();
    update(
        &workspace,
        WorklistUpdate {
            expected_revision: 1,
            goal: None,
            restart: false,
            items: vec![WorkItemPatch {
                id: "a".into(),
                title: None,
                status: Some(WorkItemStatus::Done),
                depends_on: None,
                note: None,
                write_paths: None,
            }],
        },
    )
    .unwrap();
    let owned = take(&harness, &workspace, "dependent", 2).unwrap();
    let invalid = update(
        &workspace,
        WorklistUpdate {
            expected_revision: 3,
            goal: None,
            restart: false,
            items: vec![WorkItemPatch {
                id: "a".into(),
                title: None,
                status: Some(WorkItemStatus::Pending),
                depends_on: None,
                note: None,
                write_paths: None,
            }],
        },
    );
    assert!(invalid
        .unwrap_err()
        .to_string()
        .contains("cannot reopen a dependency"));
    assert_eq!(status(&workspace).unwrap()["revision"], 3);
    let token = owned["claim_id"].as_str().unwrap();
    let make = |summary: String| WorklistSubmitInput {
        expected_revision: 3,
        expected_repository_revision: harness.current_revision(&workspace).unwrap(),
        item_id: "dependent".into(),
        claim_id: token.into(),
        outcome: WorkItemOutcome::Complete,
        summary,
        evidence_ids: vec![],
    };
    assert!(submit(&harness, "demo", &workspace, make("测".repeat(1001))).is_err());
    let reported = submit(&harness, "demo", &workspace, make("测".repeat(1000))).unwrap();
    assert_eq!(
        reported["result"]["summary"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        1000
    );
    assert_eq!(reported["result"]["proof_status"], "not_reported");
}

#[test]
fn expired_claim_can_be_replanned_without_old_capability() {
    let (_root, workspace, harness) = setup();
    let owned = take(&harness, &workspace, "a", 1).unwrap();
    let token = owned["claim_id"].as_str().unwrap();
    let mut list = load(&workspace).unwrap().unwrap();
    let claim = list.items[0].claim.as_mut().unwrap();
    claim.claimed_at_ms = 1;
    claim.expires_at_ms = 2;
    commit(&workspace, &mut list).unwrap();
    let replanned = update(
        &workspace,
        WorklistUpdate {
            expected_revision: 3,
            goal: None,
            restart: false,
            items: vec![WorkItemPatch {
                id: "a".into(),
                title: None,
                status: Some(WorkItemStatus::Pending),
                depends_on: None,
                note: Some("Replanned after expired worker.".into()),
                write_paths: Some(vec!["src/b.rs".into()]),
            }],
        },
    )
    .unwrap();
    assert!(replanned["items"][0].get("claim").is_none());
    assert_eq!(replanned["items"][0]["write_paths"], json!(["src/b.rs"]));
    assert!(report(&harness, &workspace, 4, token).is_err());
}
