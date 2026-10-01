use super::*;
use crate::risk::RiskLevel;

#[test]
fn verification_state_round_trips_across_runtime_instances() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let state = fixture_state("VP-persist", "sha256:fixture");
    persist(&workspace, &state.workspace_snapshot("demo")).unwrap();
    let loaded = load(&workspace).unwrap().unwrap();
    let status = loaded.status("VP-persist").unwrap();
    assert_eq!(status.plan.workspace, "demo");
    assert_eq!(
        status
            .plan
            .revision
            .as_ref()
            .and_then(|revision| revision.design.as_deref()),
        Some("sha256:design-fixture")
    );
    assert_eq!(status.plan.stage_targets, vec!["language:rust"]);
    assert_eq!(status.plan.automation_gaps, vec!["property:language:rust"]);
    assert_eq!(status.queued, 1);
}

fn fixture_state(plan_id: &str, code: &str) -> VerificationState {
    let mut state = VerificationState::default();
    append_plan(&mut state, plan_id, code);
    state
}

fn append_plan(state: &mut VerificationState, plan_id: &str, code: &str) {
    state
        .create_plan(
            plan_id.into(),
            "demo".into(),
            "change:fixture".into(),
            crate::verification::VerificationPlanBinding {
                required_checks: None,
                revision: crate::evidence::Revision {
                    design: Some("sha256:design-fixture".into()),
                    code: code.into(),
                },
                stage_targets: vec!["language:rust".into()],
                automation_gaps: vec!["property:language:rust".into()],
            },
            RiskLevel::Low,
            [format!("VJ-{plan_id}")].into_iter(),
        )
        .unwrap();
}

#[test]
fn latest_bad_verification_snapshot_never_rolls_back_and_can_recover() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let directory = state_directory(&workspace).unwrap();
    fs::create_dir_all(&directory).unwrap();
    let state = fixture_state("VP-prior", "code:1");
    let prior = directory.join("00000000000000000001-prior.json");
    let good = serde_json::to_value(&state).unwrap();
    fs::write(&prior, serde_json::to_vec(&good).unwrap()).unwrap();
    let latest = directory.join(format!(
        "s{:020}-{}.json",
        state.persistence_generation(),
        Uuid::new_v4().simple()
    ));
    let mut unknown_state = good.clone();
    unknown_state["approval_override"] = serde_json::json!(true);
    let mut unknown_plan = good.clone();
    unknown_plan["plans"]["VP-prior"]["approval_override"] = serde_json::json!(true);
    let mut unknown_job = good.clone();
    unknown_job["jobs"]["VJ-VP-prior"]["approval_override"] = serde_json::json!(true);
    let mut invalid = good.clone();
    invalid["plan_order"] = serde_json::json!(["missing-plan"]);
    let runtime = crate::intelligence::SoftwareIntelligenceRuntime::default();
    let revision = state.status("VP-prior").unwrap().plan.revision.unwrap();
    for bytes in [
        b"{".to_vec(),
        serde_json::to_vec(&unknown_state).unwrap(),
        serde_json::to_vec(&unknown_plan).unwrap(),
        serde_json::to_vec(&unknown_job).unwrap(),
        serde_json::to_vec(&invalid).unwrap(),
        vec![b' '; MAX_STATE_BYTES as usize + 1],
    ] {
        fs::write(&latest, bytes).unwrap();
        assert!(
            load(&workspace).is_err(),
            "latest state must never fall back to a prior plan"
        );
        assert!(runtime
            .verification_status_for_revision("demo", &workspace, &revision)
            .is_err());
    }
    // A new, valid revision snapshot repairs the head; old history stays retained.
    let mut new_state = state.clone();
    append_plan(&mut new_state, "VP-current", "code:2");
    assert!(new_state.persistence_generation() > state.persistence_generation());
    persist(&workspace, &new_state).unwrap();
    let loaded = load(&workspace).unwrap().unwrap();
    assert_eq!(
        loaded.persistence_generation(),
        new_state.persistence_generation()
    );
    let current_revision = loaded.status("VP-current").unwrap().plan.revision.unwrap();
    assert_eq!(current_revision.code, "code:2");
    let status = runtime
        .verification_status_for_revision("demo", &workspace, &current_revision)
        .unwrap()
        .unwrap();
    assert_eq!(
        status.plan.id, "VP-current",
        "failed loads must not mark an incomplete runtime as loaded"
    );
    let restarted = crate::intelligence::SoftwareIntelligenceRuntime::default();
    assert_eq!(
        restarted
            .verification_status_for_revision("demo", &workspace, &current_revision)
            .unwrap()
            .unwrap()
            .plan
            .id,
        "VP-current"
    );
}

#[test]
fn legacy_verification_snapshot_remains_readable_without_new_optional_fields() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let state = fixture_state("VP-legacy", "code:legacy");
    let mut legacy = serde_json::to_value(&state).unwrap();
    legacy.as_object_mut().unwrap().remove("plan_order");
    legacy.as_object_mut().unwrap().remove("generation");
    for plan in legacy["plans"].as_object_mut().unwrap().values_mut() {
        for field in [
            "required_checks",
            "stage_targets",
            "automation_gaps",
            "revision",
        ] {
            plan.as_object_mut().unwrap().remove(field);
        }
    }
    for job in legacy["jobs"].as_object_mut().unwrap().values_mut() {
        job.as_object_mut().unwrap().remove("guidance");
    }
    let directory = state_directory(&workspace).unwrap();
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("legacy.json"),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();
    let restored = load(&workspace).unwrap().unwrap();
    assert_eq!(restored.persistence_generation(), 0);
    let plan = restored.status("VP-legacy").unwrap().plan;
    assert!(plan.required_checks.is_none());
    assert!(plan.revision.is_none());
}

#[test]
fn verification_enumeration_errors_and_bounds_fail_closed_at_capacity() {
    let errors = std::iter::once(Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "entry denied",
    )));
    assert!(collect_snapshot_paths(errors).is_err());
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let directory = state_directory(&workspace).unwrap();
    fs::create_dir_all(&directory).unwrap();
    let bytes = serde_json::to_vec(&VerificationState::default()).unwrap();
    for index in 0..MAX_SNAPSHOTS {
        fs::write(
            directory.join(format!("00000000000000000000-{index:04}.json")),
            &bytes,
        )
        .unwrap();
    }
    persist(&workspace, &fixture_state("VP-new", "code:new")).unwrap();
    assert_eq!(snapshot_paths(&directory).unwrap().len(), MAX_SNAPSHOTS);
    assert!(load(&workspace).unwrap().unwrap().status("VP-new").is_ok());
    fs::write(directory.join("00000000000000000001-overflow.json"), &bytes).unwrap();
    assert!(load(&workspace).is_err());
    fs::write(directory.join("00000000000000000002-overflow.json"), &bytes).unwrap();
    assert!(snapshot_paths(&directory).is_err());
}

#[cfg(unix)]
#[test]
fn latest_linked_snapshot_and_dangling_store_never_fall_back() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let directory = state_directory(&workspace).unwrap();
    fs::create_dir_all(&directory).unwrap();
    let bytes = serde_json::to_vec(&fixture_state("VP-prior", "code:1")).unwrap();
    fs::write(directory.join("00000000000000000001-prior.json"), &bytes).unwrap();
    let target = root.path().join("snapshot.json");
    fs::write(&target, &bytes).unwrap();
    let latest = directory.join("00000000000000000002-latest.json");
    std::os::unix::fs::symlink(&target, &latest).unwrap();
    assert!(load(&workspace).is_err());
    fs::remove_file(&target).unwrap();
    assert!(load(&workspace).is_err());
    fs::remove_dir_all(&directory).unwrap();
    std::os::unix::fs::symlink(root.path().join("missing"), &directory).unwrap();
    assert!(load(&workspace).is_err());
}

fn claim_fixture(
    state: &mut VerificationState,
    reviewer: &str,
) -> crate::verification::VerificationJob {
    state
        .claim(
            "demo",
            reviewer,
            &std::collections::BTreeSet::from(["correctness_review".into()]),
            None,
        )
        .unwrap()
}

fn fail_fixture(state: &mut VerificationState, job_id: &str, reviewer: &str) {
    state
        .submit(
            "demo",
            job_id,
            reviewer,
            crate::verification::ReviewSubmission {
                verdict: crate::verification::ReviewVerdict::Fail,
                summary: "real persisted failure state".into(),
                claims: vec![],
                risks: vec![],
                model: None,
            },
        )
        .unwrap();
}

fn generation_path(directory: &Path, generation: u64, suffix: &str) -> PathBuf {
    directory.join(format!("s{generation:020}-{suffix}.json"))
}

#[test]
fn native_generation_rejects_late_capture_and_restores_the_newest_failure() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let mut state = fixture_state("VP-cas", "code:1");
    let captured_plan = state.clone();
    persist(&workspace, &state).unwrap();
    persist(&workspace, &state).unwrap();
    assert_eq!(
        snapshot_paths(&state_directory(&workspace).unwrap())
            .unwrap()
            .len(),
        1,
        "equal content is idempotent"
    );
    let job = claim_fixture(&mut state, "reviewer-a");
    let captured_claim = state.clone();
    persist(&workspace, &state).unwrap();
    fail_fixture(&mut state, &job.id, "reviewer-a");
    persist(&workspace, &state).unwrap();
    assert!(persist(&workspace, &captured_plan).is_err());
    assert!(persist(&workspace, &captured_claim).is_err());
    let restored = load(&Workspace::new(root.path(), false, false).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(restored.persistence_generation(), 3);
    let status = restored.status("VP-cas").unwrap();
    assert_eq!(status.reviewer_failures, 1);
    assert_eq!(status.jobs[0].claimed_by.as_deref(), Some("reviewer-a"));
    assert_eq!(
        status.jobs[0].submission.as_ref().unwrap().verdict,
        crate::verification::ReviewVerdict::Fail
    );
}

#[test]
fn same_generation_conflicts_and_bad_peers_never_select_by_uuid() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let base = fixture_state("VP-fork", "code:1");
    persist(&workspace, &base).unwrap();
    let mut first = base.clone();
    let mut fork = base.clone();
    claim_fixture(&mut first, "owner-a");
    claim_fixture(&mut fork, "owner-b");
    persist(&workspace, &first).unwrap();
    assert!(persist(&workspace, &fork).is_err());
    assert_eq!(
        snapshot_paths(&state_directory(&workspace).unwrap())
            .unwrap()
            .len(),
        2
    );
    // Simulate another process having passed CAS before either append existed.
    let directory = state_directory(&workspace).unwrap();
    let conflicting = generation_path(
        &directory,
        fork.persistence_generation(),
        "00000000000000000000000000000000",
    );
    fs::write(&conflicting, serde_json::to_vec(&fork).unwrap()).unwrap();
    assert!(
        load(&workspace).is_err(),
        "all highest-generation snapshots must agree"
    );
    assert!(
        persist(&workspace, &first).is_err(),
        "idempotence cannot conceal a conflicting peer"
    );
    fs::write(&conflicting, b"{").unwrap();
    assert!(
        load(&workspace).is_err(),
        "a bad low-UUID peer cannot be ignored"
    );
    fs::write(&conflicting, serde_json::to_vec_pretty(&first).unwrap()).unwrap();
    assert_eq!(
        load(&workspace).unwrap().unwrap().persistence_generation(),
        2
    );
}

#[test]
fn concurrent_same_generation_forks_have_only_one_persisted_winner() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let base = fixture_state("VP-concurrent", "code:1");
    persist(&workspace, &base).unwrap();
    let mut first = base.clone();
    let mut second = base;
    claim_fixture(&mut first, "owner-a");
    claim_fixture(&mut second, "owner-b");
    let barrier = std::sync::Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            barrier.wait();
            persist(&workspace, &first)
        });
        let b = scope.spawn(|| {
            barrier.wait();
            persist(&workspace, &second)
        });
        [a.join().unwrap(), b.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let restored = load(&workspace).unwrap().unwrap();
    assert_eq!(restored.persistence_generation(), 2);
    let owner = restored.status("VP-concurrent").unwrap().jobs[0]
        .claimed_by
        .clone()
        .unwrap();
    assert!(["owner-a", "owner-b"].contains(&owner.as_str()));
    assert_eq!(
        snapshot_paths(&state_directory(&workspace).unwrap())
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn generation_header_mismatch_blocks_load_until_a_genuinely_newer_mutation() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let mut state = fixture_state("VP-header", "code:1");
    persist(&workspace, &state).unwrap();
    let directory = state_directory(&workspace).unwrap();
    fs::write(
        generation_path(&directory, 2, "00000000000000000000000000000000"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    assert!(load(&workspace).is_err());
    let job = claim_fixture(&mut state, "reviewer");
    assert!(
        persist(&workspace, &state).is_err(),
        "same generation cannot repair mismatched content"
    );
    fail_fixture(&mut state, &job.id, "reviewer");
    persist(&workspace, &state).unwrap();
    let restored = load(&workspace).unwrap().unwrap();
    assert_eq!(restored.persistence_generation(), 3);
    assert_eq!(restored.status("VP-header").unwrap().reviewer_failures, 1);
}

#[test]
fn legacy_timestamp_ties_fail_closed_and_native_generations_dominate_legacy_clock() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let directory = state_directory(&workspace).unwrap();
    fs::create_dir_all(&directory).unwrap();
    let mut a = serde_json::to_value(fixture_state("VP-legacy-a", "code:1")).unwrap();
    let mut b = serde_json::to_value(fixture_state("VP-legacy-b", "code:2")).unwrap();
    a.as_object_mut().unwrap().remove("generation");
    b.as_object_mut().unwrap().remove("generation");
    let first = directory.join("00000000000000000042-ffffffffffffffffffffffffffffffff.json");
    let second = directory.join("00000000000000000042-00000000000000000000000000000000.json");
    fs::write(&first, serde_json::to_vec(&a).unwrap()).unwrap();
    fs::write(&second, serde_json::to_vec(&b).unwrap()).unwrap();
    assert!(
        load(&workspace).is_err(),
        "millisecond ties cannot use random UUID order"
    );
    fs::write(&second, serde_json::to_vec_pretty(&a).unwrap()).unwrap();
    let mut migrated = load(&workspace).unwrap().unwrap();
    assert_eq!(migrated.persistence_generation(), 0);
    append_plan(&mut migrated, "VP-modern", "code:modern");
    persist(&workspace, &migrated).unwrap();
    fs::write(
        directory.join("18446744073709551615-future.json"),
        serde_json::to_vec(&b).unwrap(),
    )
    .unwrap();
    let restored = load(&workspace).unwrap().unwrap();
    assert_eq!(restored.persistence_generation(), 1);
    assert!(
        restored.status("VP-modern").is_ok(),
        "legacy clocks cannot supersede native generations"
    );
}
