use super::*;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn concurrent_project_profile_requests_share_one_validation_flight() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"profile-flight\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let flight = harness.project_flight(workspace.root()).unwrap();
    let guard = flight
        .gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let generation_before = flight.generation.load(Ordering::Acquire);
    let entrants_before = flight.entrants.load(Ordering::Acquire);

    let cache_hits = std::thread::scope(|scope| {
        let (started_tx, started_rx) = mpsc::channel();
        let mut handles = Vec::new();
        for _ in 0..2 {
            let started_tx = started_tx.clone();
            let harness = &harness;
            let workspace = &workspace;
            handles.push(scope.spawn(move || {
                started_tx.send(()).unwrap();
                harness.load_project_profile(workspace).unwrap().1
            }));
        }
        drop(started_tx);
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while flight.entrants.load(Ordering::Acquire) < entrants_before + 2 {
            assert!(
                Instant::now() < deadline,
                "project profile requests did not join the shared validation flight"
            );
            std::thread::yield_now();
        }
        drop(guard);
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });

    assert_eq!(cache_hits.iter().filter(|&&hit| !hit).count(), 1);
    assert_eq!(cache_hits.iter().filter(|&&hit| hit).count(), 1);
    assert_eq!(
        flight.generation.load(Ordering::Acquire),
        generation_before + 2,
        "overlapping project profile calls should perform one tree validation"
    );

    let generation_before_hot_call = flight.generation.load(Ordering::Acquire);
    assert!(harness.load_project_profile(&workspace).unwrap().1);
    assert_eq!(
        flight.generation.load(Ordering::Acquire),
        generation_before_hot_call + 2,
        "a non-overlapping project profile call must still validate freshness"
    );

    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"profile-flight-updated\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    assert!(
        !harness.load_project_profile(&workspace).unwrap().1,
        "manifest edits must invalidate the cached profile without a TTL window"
    );
}

#[test]
fn shared_project_profile_tail_validation_rejects_midflight_manifest_changes() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"profile-tail\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    harness.load_project_profile(&workspace).unwrap();

    let expected_fingerprint = harness
        .project_cache
        .lock()
        .unwrap()
        .get(workspace.root())
        .unwrap()
        .fingerprint;
    let flight = harness.project_flight(workspace.root()).unwrap();
    let observed_generation = flight.generation.load(Ordering::Acquire);
    let owner =
        super::harness_cache_flight::ValidationParticipant::join(&flight, observed_generation);
    let follower =
        super::harness_cache_flight::ValidationParticipant::join(&flight, observed_generation);
    assert!(owner.has_coalescible_peer());

    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"profile-tail-changed\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let error = super::harness_profile::ensure_shared_project_fingerprint_current(
        &owner,
        &workspace,
        expected_fingerprint,
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("project profile changed during shared validation"));
    drop(follower);
    assert!(!owner.has_coalescible_peer());
}

#[test]
fn concurrent_convention_requests_share_one_validation_flight() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn stable() {}\n").unwrap();

    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let flight = harness.convention_flight(workspace.root()).unwrap();
    let guard = flight
        .gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let generation_before = flight.generation.load(Ordering::Acquire);
    let entrants_before = flight.entrants.load(Ordering::Acquire);

    let reports = std::thread::scope(|scope| {
        let (started_tx, started_rx) = mpsc::channel();
        let mut handles = Vec::new();
        for _ in 0..2 {
            let started_tx = started_tx.clone();
            let harness = &harness;
            let workspace = &workspace;
            handles.push(scope.spawn(move || {
                started_tx.send(()).unwrap();
                harness.convention_status_cached(workspace).unwrap()
            }));
        }
        drop(started_tx);
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let deadline = Instant::now() + Duration::from_secs(2);
        while flight.entrants.load(Ordering::Acquire) < entrants_before + 2 {
            assert!(
                Instant::now() < deadline,
                "convention requests did not join the shared validation flight"
            );
            std::thread::yield_now();
        }
        drop(guard);
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });

    assert_eq!(reports.len(), 2);
    assert_eq!(
        flight.generation.load(Ordering::Acquire),
        generation_before + 2,
        "overlapping callers should perform one workspace fingerprint scan"
    );

    let generation_before_hot_call = flight.generation.load(Ordering::Acquire);
    harness.convention_status_cached(&workspace).unwrap();
    assert_eq!(
        flight.generation.load(Ordering::Acquire),
        generation_before_hot_call + 2,
        "a non-overlapping caller must still validate workspace freshness"
    );
}

#[test]
fn shared_convention_tail_validation_rejects_midflight_source_changes() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn stable() {}\n").unwrap();

    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    harness.convention_status_cached(&workspace).unwrap();

    let expected_fingerprint = harness
        .convention_cache
        .lock()
        .unwrap()
        .get(workspace.root())
        .unwrap()
        .fingerprint;
    let flight = harness.convention_flight(workspace.root()).unwrap();
    let observed_generation = flight.generation.load(Ordering::Acquire);
    let owner =
        super::harness_cache_flight::ValidationParticipant::join(&flight, observed_generation);
    let follower =
        super::harness_cache_flight::ValidationParticipant::join(&flight, observed_generation);
    assert!(owner.has_coalescible_peer());

    fs::write(root.path().join("src/new.rs"), "pub fn newly_added() {}\n").unwrap();
    let error = super::harness_convention_cache::ensure_shared_convention_fingerprint_current(
        &owner,
        &workspace,
        expected_fingerprint,
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("convention state changed during shared validation"));
    drop(follower);
    assert!(!owner.has_coalescible_peer());
}

#[test]
fn code_invalidation_rejects_late_convention_validation_success() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn stable() {}\n").unwrap();

    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    harness.convention_status_cached(&workspace).unwrap();

    let flight = harness.convention_flight(workspace.root()).unwrap();
    let generation_before = flight.generation.load(Ordering::Acquire);
    let revision_before = flight.invalidation_revision.load(Ordering::Acquire);
    {
        let mut validation = super::harness_cache_flight::ValidationGuard::begin(&flight);
        harness.invalidate_code_file(&workspace, "src/lib.rs");
        validation.mark_success();
    }

    assert_eq!(
        flight.invalidation_revision.load(Ordering::Acquire),
        revision_before + 1
    );
    assert_eq!(
        flight.generation.load(Ordering::Acquire),
        generation_before + 2
    );
    assert_ne!(
        flight.successful_generation.load(Ordering::Acquire),
        generation_before + 2,
        "an invalidated convention build must not become reusable"
    );
    assert!(harness.convention_cache.lock().unwrap().is_empty());

    harness.convention_status_cached(&workspace).unwrap();
    assert_eq!(
        flight.successful_revision.load(Ordering::Acquire),
        revision_before + 1,
        "the next validation should publish only the current revision"
    );
}

#[test]
fn concurrent_project_profiles_keep_same_root_permission_modes_separate() {
    // Exercise each independent restriction. Whichever caller owns the first
    // validation, the other must not inherit its write or execution mode.
    for restricted_mode in [(false, true), (true, false), (false, false)] {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname = \"profile-permissions\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let workspaces = [
            Workspace::new(root.path(), true, true).unwrap(),
            Workspace::new(root.path(), restricted_mode.0, restricted_mode.1).unwrap(),
        ];
        let harness = ToolHarness::new(4).unwrap();
        let flight = harness.project_flight(workspaces[0].root()).unwrap();
        let guard = flight
            .gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let generation_before = flight.generation.load(Ordering::Acquire);
        let entrants_before = flight.entrants.load(Ordering::Acquire);
        let profiles = std::thread::scope(|scope| {
            let (started_tx, started_rx) = mpsc::channel();
            let mut handles = Vec::new();
            for workspace in &workspaces {
                let started_tx = started_tx.clone();
                let harness = &harness;
                handles.push(scope.spawn(move || {
                    started_tx.send(()).unwrap();
                    harness.load_project_profile(workspace).unwrap()
                }));
            }
            drop(started_tx);
            started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            while flight.entrants.load(Ordering::Acquire) < entrants_before + 2 {
                assert!(
                    Instant::now() < deadline,
                    "different permission modes did not join the shared root flight"
                );
                std::thread::yield_now();
            }
            drop(guard);
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });
        for ((profile, cache_hit), workspace) in profiles.iter().zip(&workspaces) {
            assert!(profile.discovery.complete);
            assert_eq!(profile.write_enabled, workspace.write_enabled());
            assert_eq!(profile.exec_enabled, workspace.exec_enabled());
            assert!(
                !cache_hit,
                "a different permission mode must validate its own profile"
            );
        }
        assert_eq!(
            flight.generation.load(Ordering::Acquire),
            generation_before + 4,
            "different permission snapshots require separate completed validation epochs"
        );
        // Public consumers must also reflect the caller after either mode has
        // populated the single-root cache; validation remains fresh each time.
        for workspace in &workspaces {
            let context = harness
                .project_context("permission-fixture", workspace)
                .unwrap();
            assert_eq!(context.write_enabled, workspace.write_enabled());
            assert_eq!(context.exec_enabled, workspace.exec_enabled());
        }
    }
}
