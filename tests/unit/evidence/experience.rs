use super::*;
use crate::evidence::Revision;

thread_local! {
    static TEMPORAL_EVALUATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn record_temporal_evaluation() {
    TEMPORAL_EVALUATIONS.with(|count| count.set(count.get() + 1));
}

fn activation_fixture() -> (tempfile::TempDir, Workspace, Vec<VerifiedChangeExperience>) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    for name in ["a.rs", "b.rs", "c.rs"] {
        fs::write(root.path().join("src").join(name), "fn example() {}\n").unwrap();
    }
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    for (timestamp, companion) in [(1_000, "src/b.rs"), (2_000, "src/c.rs")] {
        write_evaluation_record(
            &workspace,
            timestamp,
            &format!("revision-{timestamp}"),
            "full",
            &["src/a.rs", companion],
        );
    }
    let records = load(&workspace).unwrap();
    (root, workspace, records)
}

#[test]
fn activation_cache_invalidates_prior_record_rewrites_with_unchanged_tail() {
    let (_root, workspace, mut records) = activation_fixture();
    assert_eq!(
        activation_for_records(&workspace, &records).evaluable_records,
        2
    );
    let last = serde_json::to_value(records.last().unwrap()).unwrap();
    records[0].paths = vec!["src/a.rs".into(), "src/missing.rs".into()];
    assert!(valid_record(&records[0]));
    assert_eq!(serde_json::to_value(records.last().unwrap()).unwrap(), last);
    let expected =
        activation_from_evaluation(&evaluate_history_from_records(&workspace, &records).unwrap());
    assert_eq!(expected.evaluable_records, 1);
    assert_eq!(
        activation_for_records(&workspace, &records).evaluable_records,
        expected.evaluable_records
    );
}

#[test]
fn activation_cache_invalidates_file_removal_directory_replacement_and_restore() {
    let (root, workspace, records) = activation_fixture();
    assert_eq!(
        activation_for_records(&workspace, &records).evaluable_records,
        2
    );
    let path = root.path().join("src/b.rs");
    fs::remove_file(&path).unwrap();
    assert_eq!(
        activation_for_records(&workspace, &records).evaluable_records,
        1
    );
    fs::create_dir(&path).unwrap();
    assert_eq!(
        activation_for_records(&workspace, &records).evaluable_records,
        1
    );
    fs::remove_dir(&path).unwrap();
    fs::write(&path, "fn restored() {}\n").unwrap();
    assert_eq!(
        activation_for_records(&workspace, &records).evaluable_records,
        2
    );
}

#[cfg(unix)]
#[test]
fn activation_cache_rejects_symlink_replacement_without_changing_history() {
    let (root, workspace, records) = activation_fixture();
    assert_eq!(
        activation_for_records(&workspace, &records).evaluable_records,
        2
    );
    let path = root.path().join("src/b.rs");
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(root.path().join("src/c.rs"), &path).unwrap();
    assert_eq!(
        activation_for_records(&workspace, &records).evaluable_records,
        1
    );
}

#[test]
fn activation_cache_reuses_identical_inputs_but_tracks_trajectory_and_policy_inputs() {
    let (root, workspace, mut records) = activation_fixture();
    TEMPORAL_EVALUATIONS.with(|count| count.set(0));
    for _ in 0..4 {
        activation_for_records(&workspace, &records);
    }
    assert_eq!(TEMPORAL_EVALUATIONS.with(|count| count.get()), 1);
    fs::write(
        root.path().join("src/a.rs"),
        "fn changed_contents_only() {}\n",
    )
    .unwrap();
    activation_for_records(&workspace, &records);
    assert_eq!(
        TEMPORAL_EVALUATIONS.with(|count| count.get()),
        1,
        "membership-based replay must not hash source contents"
    );
    records[0].context_paths = vec!["src/c.rs".into()];
    activation_for_records(&workspace, &records);
    records[0].retrieval_intent = Some("edit_to_ripple".into());
    activation_for_records(&workspace, &records);
    records[0].level = "quick".into();
    activation_for_records(&workspace, &records);
    records[0].revision.design = Some("design:changed".into());
    activation_for_records(&workspace, &records);
    assert_eq!(TEMPORAL_EVALUATIONS.with(|count| count.get()), 5);
}

#[test]
fn activation_cache_coalesces_concurrent_identical_replays() {
    let (_root, workspace, records) = activation_fixture();
    let barrier = std::sync::Barrier::new(6);
    let evaluations = std::thread::scope(|scope| {
        let handles = (0..6)
            .map(|_| {
                scope.spawn(|| {
                    TEMPORAL_EVALUATIONS.with(|count| count.set(0));
                    barrier.wait();
                    for _ in 0..3 {
                        assert_eq!(
                            activation_for_records(&workspace, &records).evaluable_records,
                            2
                        );
                    }
                    TEMPORAL_EVALUATIONS.with(|count| count.get())
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .sum::<usize>()
    });
    assert_eq!(
        evaluations, 1,
        "18 overlapping requests should execute one temporal replay"
    );
}

#[test]
fn metadata_membership_preparation_matches_hashed_lookup_without_source_reads() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    let names = (0..12)
        .map(|index| format!("src/file_{index:02}.rs"))
        .collect::<Vec<_>>();
    let payload = vec![b'x'; 512 * 1024];
    for name in &names {
        fs::write(root.path().join(name), &payload).unwrap();
    }
    let records = (1..=96)
        .map(|timestamp_ms| VerifiedChangeExperience {
            schema_version: EXPERIENCE_SCHEMA_VERSION,
            revision: Revision {
                code: format!("revision-{timestamp_ms}"),
                design: None,
            },
            level: "full".into(),
            paths: names.clone(),
            context_paths: names[..3].to_vec(),
            retrieval_intent: Some("edit_to_ripple".into()),
            timestamp_ms,
        })
        .collect::<Vec<_>>();
    let mut hashed_ms = Vec::new();
    let mut metadata_ms = Vec::new();
    for _ in 0..5 {
        let started = std::time::Instant::now();
        let legacy = crate::resource::parallel_io(&names, |path| {
            workspace
                .path_info(path)
                .is_ok_and(|info| info.kind == "file")
        })
        .unwrap();
        hashed_ms.push(started.elapsed().as_secs_f64() * 1_000.0);
        let started = std::time::Instant::now();
        let prepared = experience_io::prepare_evaluation_paths(&workspace, &records).unwrap();
        metadata_ms.push(started.elapsed().as_secs_f64() * 1_000.0);
        assert!(legacy.iter().all(|live| *live));
        assert_eq!(prepared.unique_paths, names.len());
        assert_eq!(prepared.live_paths, names.len());
        assert_eq!(prepared.stale_path_references, 0);
        for (index, record) in records.iter().enumerate() {
            assert_eq!(prepared.prepared[index], record.paths);
            assert_eq!(prepared.prepared_context[index], record.context_paths);
        }
    }
    hashed_ms.sort_by(f64::total_cmp);
    metadata_ms.sort_by(f64::total_cmp);
    let report = serde_json::json!({
        "fixture": "96 records, 12 unique files, 512 KiB per file",
        "source_bytes": names.len() * payload.len(), "trials": 5,
        "legacy_hashed_membership_median_ms": hashed_ms[2],
        "metadata_preparation_median_ms": metadata_ms[2],
        "prepared_paths_identical": true, "timing_is_informational": true,
        "scope": "synthetic guarded file membership, not end-to-end agent latency"
    });
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(
        target.join("wcode-experience-membership.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}

#[test]
fn verified_experience_is_deduped_workspace_scoped_and_ignores_missing_paths() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    fs::create_dir_all(first.path().join("src")).unwrap();
    fs::create_dir_all(second.path().join("src")).unwrap();
    fs::write(first.path().join("src/entry.rs"), "pub fn entry() {}\n").unwrap();
    fs::write(first.path().join("src/ripple.rs"), "pub fn ripple() {}\n").unwrap();
    let first_workspace = Workspace::new(first.path(), false, false).unwrap();
    let second_workspace = Workspace::new(second.path(), false, false).unwrap();
    let revision = Revision {
        code: "sha256:verified-fixture".into(),
        design: Some("sha256:design-fixture".into()),
    };
    let paths = vec!["src/entry.rs".into(), "src/ripple.rs".into()];

    assert!(persist_verified_change(&first_workspace, &revision, "full", &paths).unwrap());
    assert!(!persist_verified_change(&first_workspace, &revision, "full", &paths).unwrap());

    let anchors = BTreeSet::from(["src/entry.rs".to_owned()]);
    let matches = related_paths(&first_workspace, &anchors, 8).unwrap();
    assert!(matches.available);
    assert_eq!(matches.scanned_records, 1);
    assert_eq!(matches.matched_records, 1);
    assert_eq!(matches.weights.get("src/ripple.rs"), Some(&500));
    assert!(related_paths(&second_workspace, &anchors, 8)
        .unwrap()
        .weights
        .is_empty());

    fs::remove_file(first.path().join("src/ripple.rs")).unwrap();
    let stale = related_paths(&first_workspace, &anchors, 8).unwrap();
    assert_eq!(stale.matched_records, 1);
    assert!(
        stale.weights.is_empty(),
        "removed paths must never be promoted"
    );
}

fn write_evaluation_record(
    workspace: &Workspace,
    timestamp_ms: u64,
    revision: &str,
    level: &str,
    paths: &[&str],
) {
    let directory = experience_directory(workspace).unwrap();
    ensure_regular_directory(&directory).unwrap();
    let paths = paths
        .iter()
        .map(|path| (*path).to_owned())
        .collect::<Vec<_>>();
    let record = VerifiedChangeExperience {
        schema_version: EXPERIENCE_SCHEMA_VERSION,
        revision: Revision {
            code: revision.to_owned(),
            design: None,
        },
        level: level.to_owned(),
        paths: normalize_paths(&paths),
        context_paths: Vec::new(),
        retrieval_intent: None,
        timestamp_ms,
    };
    let name = format!("eval-{timestamp_ms}-{revision}.json").replace(':', "-");
    fs::write(directory.join(name), serde_json::to_vec(&record).unwrap()).unwrap();
}

#[test]
fn verified_context_trajectory_promotes_context_to_verified_change_without_storing_prompt() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    for path in ["context.rs", "target.rs", "companion.rs"] {
        fs::write(root.path().join("src").join(path), format!("// {path}\n")).unwrap();
    }
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let mut milestone = crate::engineering_journal::EngineeringMilestone::new(
        "agent_context",
        "understand",
        "succeeded",
        9,
        vec!["src/context.rs".into(), "src/target.rs".into()],
    )
    .unwrap();
    milestone.retrieval_intent = Some("edit_to_ripple".into());
    crate::engineering_journal::persist(&workspace, &milestone).unwrap();

    let revision = Revision {
        code: "sha256:trajectory-verified".into(),
        design: None,
    };
    assert!(persist_verified_change(
        &workspace,
        &revision,
        "full",
        &["src/target.rs".into(), "src/companion.rs".into()],
    )
    .unwrap());

    let records = load(&workspace).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].context_paths,
        vec!["src/context.rs".to_owned(), "src/target.rs".to_owned()]
    );
    assert_eq!(
        records[0].retrieval_intent.as_deref(),
        Some("edit_to_ripple")
    );
    let encoded = serde_json::to_string(&records[0]).unwrap();
    for forbidden in [
        "prompt",
        "chain_of_thought",
        "arguments",
        "old_text",
        "new_text",
    ] {
        assert!(!encoded.contains(forbidden));
    }

    let anchors = BTreeSet::from(["src/context.rs".to_owned()]);
    let matches = related_paths(&workspace, &anchors, 8).unwrap();
    assert_eq!(matches.matched_records, 1);
    assert!(matches
        .weights
        .get("src/target.rs")
        .is_some_and(|weight| *weight > 0));
    assert!(matches
        .weights
        .get("src/companion.rs")
        .is_some_and(|weight| *weight > 0));
}

#[test]
fn legacy_v1_experience_records_remain_readable() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    for path in ["a.rs", "b.rs"] {
        fs::write(root.path().join("src").join(path), format!("// {path}\n")).unwrap();
    }
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let directory = experience_directory(&workspace).unwrap();
    ensure_regular_directory(&directory).unwrap();
    let legacy = serde_json::json!({
        "schema_version": 1,
        "revision": {"code": "sha256:legacy", "design": null},
        "level": "full",
        "paths": ["src/a.rs", "src/b.rs"],
        "timestamp_ms": 1
    });
    fs::write(
        directory.join("legacy.json"),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();
    let records = load(&workspace).unwrap();
    assert_eq!(records.len(), 1);
    assert!(records[0].context_paths.is_empty());
    assert!(records[0].retrieval_intent.is_none());
}

#[test]
fn context_trajectory_reuse_is_conditioned_by_retrieval_intent() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    for path in [
        "context.rs",
        "edit_target.rs",
        "edit_peer.rs",
        "test_target.rs",
        "test_peer.rs",
    ] {
        fs::write(root.path().join("src").join(path), format!("// {path}\n")).unwrap();
    }
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let directory = experience_directory(&workspace).unwrap();
    ensure_regular_directory(&directory).unwrap();
    for (name, intent, paths) in [
        (
            "edit",
            "edit_to_ripple",
            vec![
                "src/edit_peer.rs".to_owned(),
                "src/edit_target.rs".to_owned(),
            ],
        ),
        (
            "test",
            "code_to_test",
            vec![
                "src/test_peer.rs".to_owned(),
                "src/test_target.rs".to_owned(),
            ],
        ),
    ] {
        let record = VerifiedChangeExperience {
            schema_version: EXPERIENCE_SCHEMA_VERSION,
            revision: Revision {
                code: format!("sha256:{name}"),
                design: None,
            },
            level: "full".to_owned(),
            paths: normalize_paths(&paths),
            context_paths: vec!["src/context.rs".to_owned()],
            retrieval_intent: Some(intent.to_owned()),
            timestamp_ms: now_ms(),
        };
        fs::write(
            directory.join(format!("intent-{name}.json")),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
    }

    let anchors = BTreeSet::from(["src/context.rs".to_owned()]);
    let edit = related_paths_for_intent(&workspace, &anchors, Some("edit_to_ripple"), 8).unwrap();
    let tests = related_paths_for_intent(&workspace, &anchors, Some("code_to_test"), 8).unwrap();
    assert!(edit.weights["src/edit_target.rs"] > edit.weights["src/test_target.rs"]);
    assert!(tests.weights["src/test_target.rs"] > tests.weights["src/edit_target.rs"]);
}

#[test]
fn activation_gate_degrades_only_after_repeated_temporal_regression() {
    let cold = activation_from_evaluation(&ExperienceEvaluation {
        evaluable_records: EXPERIENCE_ACTIVATION_MIN_RECORDS - 1,
        ..ExperienceEvaluation::default()
    });
    assert_eq!(cold.state, "cold_start");
    assert_eq!(cold.factor, 1.0);

    let degraded = activation_from_evaluation(&ExperienceEvaluation {
        evaluable_records: EXPERIENCE_ACTIVATION_MIN_RECORDS,
        evaluation_cases: 100,
        prediction_cases: 40,
        baseline_prediction_cases: 80,
        hit_cases: 10,
        baseline_hit_cases: 60,
        predictions: 40,
        baseline_predictions: 80,
        true_positives: 5,
        baseline_true_positives: 50,
        expected_targets: 100,
        ..ExperienceEvaluation::default()
    });
    assert_eq!(degraded.state, "degraded");
    assert_eq!(degraded.factor, EXPERIENCE_DEGRADED_FACTOR);

    let active = activation_from_evaluation(&ExperienceEvaluation {
        evaluable_records: EXPERIENCE_ACTIVATION_MIN_RECORDS,
        evaluation_cases: 100,
        prediction_cases: 80,
        baseline_prediction_cases: 70,
        hit_cases: 60,
        baseline_hit_cases: 45,
        predictions: 80,
        baseline_predictions: 70,
        true_positives: 55,
        baseline_true_positives: 40,
        expected_targets: 100,
        ..ExperienceEvaluation::default()
    });
    assert_eq!(active.state, "active");
    assert_eq!(active.factor, 1.0);
}

#[test]
fn temporal_holdout_evaluation_avoids_same_batch_leakage_and_ignores_stale_paths() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    for path in ["a.rs", "b.rs", "d.rs"] {
        fs::write(root.path().join("src").join(path), format!("// {path}\n")).unwrap();
    }
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    write_evaluation_record(
        &workspace,
        1_000,
        "sha256:first",
        "full",
        &["src/a.rs", "src/b.rs"],
    );
    write_evaluation_record(
        &workspace,
        1_000,
        "sha256:same-batch",
        "quick",
        &["src/a.rs", "src/d.rs"],
    );
    write_evaluation_record(
        &workspace,
        2_000,
        "sha256:future",
        "full",
        &["src/a.rs", "src/b.rs"],
    );
    write_evaluation_record(
        &workspace,
        3_000,
        "sha256:stale",
        "full",
        &["src/a.rs", "src/c.rs"],
    );

    let evaluation = evaluate_history(&workspace).unwrap();
    assert!(evaluation.available);
    assert_eq!(evaluation.records, 4);
    assert_eq!(evaluation.full_records, 3);
    assert_eq!(evaluation.quick_records, 1);
    assert_eq!(evaluation.unique_paths, 4);
    assert_eq!(evaluation.live_paths, 3);
    assert_eq!(evaluation.stale_path_references, 1);
    assert_eq!(evaluation.evaluable_records, 3);
    assert_eq!(evaluation.records_with_prediction, 1);
    assert_eq!(evaluation.baseline_records_with_prediction, 1);
    assert_eq!(evaluation.evaluation_cases, 6);
    assert_eq!(evaluation.prediction_cases, 2);
    assert_eq!(evaluation.baseline_prediction_cases, 2);
    assert_eq!(evaluation.hit_cases, 2);
    assert_eq!(evaluation.baseline_hit_cases, 2);
    assert_eq!(evaluation.predictions, 3);
    assert_eq!(evaluation.baseline_predictions, 3);
    assert_eq!(evaluation.true_positives, 2);
    assert_eq!(evaluation.baseline_true_positives, 2);
    assert_eq!(evaluation.expected_targets, 6);
    assert_eq!(evaluation.top_k, 5);
    assert_eq!(evaluation.latest_record_at_ms, Some(3_000));
}

#[test]
fn normalized_retrieval_downranks_broad_and_stale_evidence() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    for path in [
        "anchor.rs",
        "specific.rs",
        "old.rs",
        "broad.rs",
        "noise0.rs",
        "noise1.rs",
        "noise2.rs",
        "noise3.rs",
        "noise4.rs",
        "noise5.rs",
        "noise6.rs",
        "noise7.rs",
        "noise8.rs",
        "noise9.rs",
    ] {
        fs::write(root.path().join("src").join(path), format!("// {path}\n")).unwrap();
    }
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let now = now_ms();
    let day_ms = 24 * 60 * 60 * 1_000;
    write_evaluation_record(
        &workspace,
        now.saturating_sub(90 * day_ms),
        "sha256:old",
        "full",
        &["src/anchor.rs", "src/old.rs"],
    );
    write_evaluation_record(
        &workspace,
        now.saturating_sub(1_000),
        "sha256:specific",
        "quick",
        &["src/anchor.rs", "src/specific.rs"],
    );
    let mut broad = vec!["src/anchor.rs", "src/broad.rs"];
    broad.extend([
        "src/noise0.rs",
        "src/noise1.rs",
        "src/noise2.rs",
        "src/noise3.rs",
        "src/noise4.rs",
        "src/noise5.rs",
        "src/noise6.rs",
        "src/noise7.rs",
        "src/noise8.rs",
        "src/noise9.rs",
    ]);
    write_evaluation_record(&workspace, now, "sha256:broad", "full", &broad);

    let anchors = BTreeSet::from(["src/anchor.rs".to_owned()]);
    let matches = related_paths(&workspace, &anchors, 16).unwrap();
    let specific = matches.weights["src/specific.rs"];
    let old = matches.weights["src/old.rs"];
    let broad = matches.weights["src/broad.rs"];
    assert!(
        specific > old,
        "recent focused evidence should beat stale evidence"
    );
    assert!(
        specific > broad,
        "focused evidence should beat a broad full-verification sweep"
    );
}

#[test]
fn normalized_temporal_ab_replay_beats_raw_popularity_bias() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    for path in [
        "anchor.rs",
        "z_specific.rs",
        "a_hot.rs",
        "b_hot.rs",
        "c_hot.rs",
        "d_hot.rs",
        "e_hot.rs",
        "common.rs",
    ] {
        fs::write(root.path().join("src").join(path), format!("// {path}\n")).unwrap();
    }
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    write_evaluation_record(
        &workspace,
        1_000,
        "sha256:specific-train",
        "full",
        &["src/anchor.rs", "src/z_specific.rs"],
    );
    for (hot_index, hot) in [
        "src/a_hot.rs",
        "src/b_hot.rs",
        "src/c_hot.rs",
        "src/d_hot.rs",
        "src/e_hot.rs",
    ]
    .iter()
    .enumerate()
    {
        write_evaluation_record(
            &workspace,
            1_000,
            &format!("sha256:anchor-hot-{hot_index}"),
            "full",
            &["src/anchor.rs", hot],
        );
        for occurrence in 0..4 {
            write_evaluation_record(
                &workspace,
                1_000,
                &format!("sha256:hot-{hot_index}-{occurrence}"),
                "full",
                &[hot, "src/common.rs"],
            );
        }
    }
    write_evaluation_record(
        &workspace,
        2_000,
        "sha256:specific-holdout",
        "full",
        &["src/anchor.rs", "src/z_specific.rs"],
    );

    let evaluation = evaluate_history(&workspace).unwrap();
    assert_eq!(evaluation.prediction_cases, 2);
    assert_eq!(evaluation.baseline_prediction_cases, 2);
    assert_eq!(evaluation.predictions, 6);
    assert_eq!(evaluation.baseline_predictions, 6);
    assert_eq!(evaluation.true_positives, 2);
    assert_eq!(evaluation.baseline_true_positives, 1);
    assert!(
        evaluation.true_positives > evaluation.baseline_true_positives,
        "normalized history should recover the specific relationship that raw popularity hides"
    );
}

#[test]
fn experience_rejects_partial_or_non_relational_learning_inputs() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let partial = Revision {
        code: "sha256:partial".into(),
        design: None,
    };
    let complete = Revision {
        code: "sha256:complete".into(),
        design: None,
    };

    assert!(!persist_verified_change(
        &workspace,
        &partial,
        "full",
        &["src/a.rs".into(), "src/b.rs".into()],
    )
    .unwrap());
    assert!(
        !persist_verified_change(&workspace, &complete, "quick", &["src/a.rs".into()]).unwrap()
    );
    assert!(
        persist_verified_change(&workspace, &complete, "unknown", &["a".into(), "b".into()])
            .is_err()
    );
}
