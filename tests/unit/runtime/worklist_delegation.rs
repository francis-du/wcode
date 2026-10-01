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
fn single_lane_replies_do_not_repeat_unrelated_worklist_history() {
    let (_root, workspace, harness) = setup();
    let history = (0..60)
        .map(|index| {
            let mut item = patch(&format!("history-{index}"), &[], &[]);
            item.status = Some(WorkItemStatus::Done);
            item.note = Some("Unrelated completed history. ".repeat(30));
            item
        })
        .collect();
    update(
        &workspace,
        WorklistUpdate {
            expected_revision: 1,
            goal: None,
            restart: false,
            items: history,
        },
    )
    .unwrap();

    let owned = take(&harness, &workspace, "a", 2).unwrap();
    let after_claim = status(&workspace).unwrap();
    let claim_id = owned["claim_id"].as_str().unwrap();
    let renewed = claim(
        &harness,
        "demo",
        &workspace,
        WorklistClaimInput {
            expected_revision: 3,
            expected_repository_revision: harness.current_revision(&workspace).unwrap(),
            item_id: "a".into(),
            actor: "worker-a".into(),
            claim_id: Some(claim_id.into()),
        },
    )
    .unwrap();
    let after_renewal = status(&workspace).unwrap();
    let reported = report(&harness, &workspace, 4, claim_id).unwrap();
    let full = status(&workspace).unwrap();
    assert_eq!(full["items"].as_array().unwrap().len(), 64);
    assert_eq!(full["counts"]["done"], 61);
    assert_eq!(full["items"][0]["result"], reported["result"]);
    assert_eq!(owned["handoff"]["item"]["id"], "a");
    assert!(renewed["handoff"]["agent_context"].is_null());

    for (kind, response, retained) in [
        ("claim", &owned, &after_claim),
        ("renewal", &renewed, &after_renewal),
        ("submit", &reported, &full),
    ] {
        let mut expanded = response.clone();
        expanded["worklist"] = retained.clone();
        let scoped_bytes = serde_json::to_vec(response).unwrap().len();
        let expanded_bytes = serde_json::to_vec(&expanded).unwrap().len();
        println!(
            "lane_reply_payload kind={kind} scoped_bytes={scoped_bytes} expanded_bytes={expanded_bytes}"
        );
        assert_eq!(response["worklist"]["items_included"], false, "{kind}");
        assert!(
            response["worklist"]["items"].as_array().unwrap().is_empty(),
            "{kind}"
        );
        assert!(scoped_bytes * 4 < expanded_bytes, "{kind}");
        for field in [
            "revision",
            "counts",
            "runnable",
            "parallel_runnable",
            "complete",
        ] {
            assert_eq!(
                response["worklist"][field], retained[field],
                "{kind} {field}"
            );
        }
    }
    assert_eq!(owned["worklist"]["revision"], 3);
    assert_eq!(renewed["worklist"]["revision"], 4);
    assert_eq!(reported["worklist"]["revision"], 5);
    assert_eq!(reported["result"]["proof_status"], "not_reported");
    assert_eq!(full["items_included"], true);
    let pack = &owned["handoff"]["agent_context"];
    let delivered_bytes = serde_json::to_vec(pack).unwrap().len() as u64;
    assert_eq!(pack["serialized_bytes"], delivered_bytes);
    assert_eq!(pack["estimated_tokens"], delivered_bytes.div_ceil(4));
}

#[test]
fn scoped_handoff_budgets_only_delivered_coordination_state() {
    for lines in [0, 12, 48, 90] {
        let source = format!(
            "pub fn feature_entry() -> usize {{\n{}    1\n}}\n",
            "    let value = 1;\n".repeat(lines)
        );
        let build = |noisy: bool, scoped: bool| {
            let (root, workspace, harness) = setup();
            fs::write(root.path().join("src/a.rs"), &source).unwrap();
            // A symbol-only inspection avoids the intentional path-anchor window.
            let mut own = patch("a", &[], &[]);
            own.title = Some("Inspect feature_entry implementation; keep the current required verification and source SHA preconditions.".into());
            let mut items = vec![own];
            if noisy {
                items.extend((0..60).map(|index| {
                    let mut item = patch(&format!("unrelated-{index}"), &[], &[]);
                    item.note = Some("Unrelated parallel task detail. ".repeat(30));
                    item.status = Some(WorkItemStatus::Blocked);
                    item
                }));
            }
            update(
                &workspace,
                WorklistUpdate {
                    expected_revision: 1,
                    goal: None,
                    restart: false,
                    items,
                },
            )
            .unwrap();
            let execution = crate::execution::refresh(&harness, "demo", &workspace, false).unwrap();
            let steered = crate::execution::steer(
                &workspace,
                crate::execution::ExecutionDirectiveInput {
                    expected_revision: execution["revision"].as_u64().unwrap(),
                    kind: crate::execution::ExecutionDirectiveKind::StrengthenVerification,
                    summary: "Retain mandatory adversarial verification for this inspection."
                        .into(),
                    requested_by: "user:test".into(),
                    objective: None,
                    scopes: vec![],
                    verification_strength: Some("adversarial".into()),
                },
            )
            .unwrap();
            let owned = if scoped {
                take(&harness, &workspace, "a", 2)
            } else {
                // The previous pipeline budgeted ordinary context, then filtered it.
                claim_with_context(
                    &harness,
                    &workspace,
                    WorklistClaimInput {
                        expected_revision: 2,
                        expected_repository_revision: harness.current_revision(&workspace).unwrap(),
                        item_id: "a".into(),
                        actor: "worker-a".into(),
                        claim_id: None,
                    },
                    |query| harness.agent_context("demo", &workspace, query, 0, &[]),
                )
            }
            .unwrap();
            let pack = owned["handoff"]["agent_context"].clone();
            assert!(pack.get("worklist").is_none());
            if scoped {
                assert_eq!(pack["readiness"]["parallelism"]["candidate_lanes"], 1);
                assert_eq!(pack["readiness"]["parallelism"]["required"], false);
                assert_eq!(
                    pack["readiness"]["parallelism"]["recommended_concurrency"],
                    1
                );
            }
            assert_eq!(
                pack["execution"]["pending_directive"],
                steered["pending_directive"]
            );
            assert_eq!(
                pack["execution"]["verification_floor"],
                steered["verification_floor"]
            );
            assert_eq!(
                pack["execution"]["replan_required"],
                steered["replan_required"]
            );
            assert!(pack["execution"].get("checkpoint").is_none());
            let bytes = serde_json::to_vec(&pack).unwrap().len() as u64;
            assert_eq!(pack["serialized_bytes"], bytes);
            assert_eq!(pack["estimated_tokens"], bytes.div_ceil(4));
            assert!(bytes.div_ceil(4) <= pack["budget"].as_u64().unwrap());
            let bodies = pack["hot_source"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| {
                    json!({
                        "path": item["path"],
                        "sha256": item["sha256"],
                        "body": item["body"],
                    })
                })
                .collect::<Vec<_>>();
            assert!(!bodies.is_empty());
            for body in &bodies {
                let file = pack["files"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|file| file["path"] == body["path"])
                    .expect("source body must retain its file precondition");
                assert_eq!(file["sha256"], body["sha256"]);
            }
            (pack, bodies)
        };
        let (clean, clean_bodies) = build(false, true);
        let (noisy, noisy_bodies) = build(true, true);
        println!(
            "handoff_source lines={lines} clean_bytes={} noisy_bytes={} clean_body_bytes={} noisy_body_bytes={}",
            clean["serialized_bytes"],
            noisy["serialized_bytes"],
            serde_json::to_vec(&clean_bodies).unwrap().len(),
            serde_json::to_vec(&noisy_bodies).unwrap().len()
        );
        assert_eq!(
            noisy_bodies, clean_bodies,
            "unrelated Worklist notes must not consume scoped source at {lines} lines"
        );
        assert_eq!(noisy["checks"], clean["checks"]);
        if lines == 48 {
            let primary = clean_bodies
                .iter()
                .find(|body| body["path"] == "src/a.rs")
                .expect("explicit function must have source");
            assert_eq!(
                primary["body"]["content"],
                source.trim_end_matches('\n'),
                "symbol-only source within the selected budget must retain the complete small function"
            );
            assert_eq!(primary["body"]["truncated"], false);
            let (legacy, legacy_bodies) = build(true, false);
            let legacy_primary = legacy_bodies
                .iter()
                .find(|body| body["path"] == "src/a.rs")
                .expect("legacy pipeline still returns a bounded source prefix");
            let legacy_bytes = legacy_primary["body"]["content"].as_str().unwrap().len();
            let delivered_bytes = primary["body"]["content"].as_str().unwrap().len();
            println!(
                "handoff_source_budget legacy_source_bytes={legacy_bytes} delivered_source_bytes={delivered_bytes} legacy_pack_bytes={} delivered_pack_bytes={}",
                legacy["serialized_bytes"], clean["serialized_bytes"]
            );
            println!(
                "handoff_coordination legacy_lanes={} scoped_lanes={}",
                legacy["readiness"]["parallelism"]["candidate_lanes"],
                clean["readiness"]["parallelism"]["candidate_lanes"]
            );
            assert_eq!(legacy_bytes, delivered_bytes);
        }
    }
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
