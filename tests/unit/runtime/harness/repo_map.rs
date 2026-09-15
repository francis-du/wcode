use super::*;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn concurrent_repo_map_requests_share_one_validation_flight() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn entry() { helper(); }\nfn helper() {}\n",
    )
    .unwrap();

    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let cache_key = (workspace.root().to_path_buf(), "src".to_owned());
    let flight = harness.repo_map_flight(&cache_key).unwrap();
    let guard = flight
        .gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let generation_before = flight.generation.load(Ordering::Acquire);
    let entrants_before = flight.entrants.load(Ordering::Acquire);

    let results = std::thread::scope(|scope| {
        let (started_tx, started_rx) = mpsc::channel();
        let mut handles = Vec::new();
        for _ in 0..2 {
            let started_tx = started_tx.clone();
            let harness = &harness;
            let workspace = &workspace;
            handles.push(scope.spawn(move || {
                started_tx.send(()).unwrap();
                harness.repo_map_graph("demo", workspace, "src").unwrap().1
            }));
        }
        drop(started_tx);
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let deadline = Instant::now() + Duration::from_secs(2);
        while flight.entrants.load(Ordering::Acquire) < entrants_before + 2 {
            assert!(
                Instant::now() < deadline,
                "repo map requests did not join the shared flight"
            );
            std::thread::yield_now();
        }
        drop(guard);
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });

    assert_eq!(
        flight.generation.load(Ordering::Acquire),
        generation_before + 2,
        "overlapping requests should perform one complete freshness validation"
    );
    assert_eq!(results.iter().filter(|&&cache_hit| !cache_hit).count(), 1);
    assert_eq!(results.iter().filter(|&&cache_hit| cache_hit).count(), 1);

    let generation_before_hot_call = flight.generation.load(Ordering::Acquire);
    let (_, cache_hit) = harness.repo_map_graph("demo", &workspace, "src").unwrap();
    assert!(cache_hit);
    assert_eq!(
        flight.generation.load(Ordering::Acquire),
        generation_before_hot_call + 2,
        "a non-overlapping request must still perform its own complete freshness validation"
    );

    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn entry() { changed(); }\nfn changed() {}\n",
    )
    .unwrap();
    let (_, cache_hit_after_edit) = harness.repo_map_graph("demo", &workspace, "src").unwrap();
    assert!(
        !cache_hit_after_edit,
        "external source edits must invalidate the cached repo map without a TTL window"
    );
}

#[test]
fn shared_repo_map_tail_validation_rejects_midflight_source_changes() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn stable() {}\n").unwrap();

    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    harness.repo_map_graph("demo", &workspace, "src").unwrap();

    let cache_key = (workspace.root().to_path_buf(), "src".to_owned());
    let expected_fingerprint = harness
        .repo_map_cache
        .lock()
        .unwrap()
        .get(&cache_key)
        .unwrap()
        .fingerprint;
    let flight = harness.repo_map_flight(&cache_key).unwrap();
    let observed_generation = flight.generation.load(Ordering::Acquire);
    let owner =
        super::harness_cache_flight::ValidationParticipant::join(&flight, observed_generation);
    let follower =
        super::harness_cache_flight::ValidationParticipant::join(&flight, observed_generation);
    assert!(owner.has_coalescible_peer());

    fs::write(root.path().join("src/new.rs"), "pub fn newly_added() {}\n").unwrap();
    let error = super::harness_repo_map::ensure_shared_repo_map_fingerprint_current(
        &owner,
        &workspace,
        "src",
        expected_fingerprint,
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("repo map changed during shared validation"));
    drop(follower);
    assert!(!owner.has_coalescible_peer());
}

#[test]
fn failed_owner_validation_is_not_reused_by_waiting_follower() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn stable() {}\n").unwrap();

    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    harness.repo_map_graph("demo", &workspace, "src").unwrap();

    let cache_key = (workspace.root().to_path_buf(), "src".to_owned());
    let flight = harness.repo_map_flight(&cache_key).unwrap();
    let guard = flight
        .gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let generation_before = flight.generation.load(Ordering::Acquire);
    assert_eq!(
        flight.successful_generation.load(Ordering::Acquire),
        generation_before
    );
    let entrants_before = flight.entrants.load(Ordering::Acquire);

    let cache_hit = std::thread::scope(|scope| {
        let handle = scope.spawn(|| harness.repo_map_graph("demo", &workspace, "src").unwrap().1);
        let deadline = Instant::now() + Duration::from_secs(2);
        while flight.entrants.load(Ordering::Acquire) < entrants_before + 1 {
            assert!(
                Instant::now() < deadline,
                "follower did not wait on the simulated owner"
            );
            std::thread::yield_now();
        }

        // Simulate an owner that started and then failed validation: generation
        // advances back to even, but successful_generation intentionally does not.
        flight.generation.fetch_add(1, Ordering::AcqRel);
        flight.generation.fetch_add(1, Ordering::Release);
        drop(guard);
        handle.join().unwrap()
    });

    assert!(
        cache_hit,
        "the follower may reuse cache only after revalidating it"
    );
    assert_eq!(
        flight.generation.load(Ordering::Acquire),
        generation_before + 4,
        "failed owner validation must force one follower validation"
    );
    assert_eq!(
        flight.successful_generation.load(Ordering::Acquire),
        generation_before + 4
    );
}

#[test]
fn invalidation_prevents_late_repo_map_validation_from_becoming_reusable() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn stable() {}\n").unwrap();

    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    harness.repo_map_graph("demo", &workspace, "src").unwrap();

    let cache_key = (workspace.root().to_path_buf(), "src".to_owned());
    let flight = harness.repo_map_flight(&cache_key).unwrap();
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
        "an invalidated validation must never be advertised as reusable"
    );
    assert!(harness.repo_map_cache.lock().unwrap().is_empty());

    let (_, cache_hit) = harness.repo_map_graph("demo", &workspace, "src").unwrap();
    assert!(!cache_hit, "invalidation must force a fresh repo map build");
}

#[test]
fn aggressive_memory_trim_invalidates_active_repo_map_validation() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn stable() {}\n").unwrap();

    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let cache_key = (workspace.root().to_path_buf(), "src".to_owned());
    let flight = harness.repo_map_flight(&cache_key).unwrap();
    let revision_before = flight.invalidation_revision.load(Ordering::Acquire);
    let mut validation = super::harness_cache_flight::ValidationGuard::begin(&flight);
    harness.trim_memory(true);
    validation.mark_success();
    drop(validation);

    assert_eq!(
        flight.invalidation_revision.load(Ordering::Acquire),
        revision_before + 1
    );
    assert_ne!(
        flight.successful_revision.load(Ordering::Acquire),
        revision_before + 1,
        "aggressive trim must reject late repo map publication"
    );
}

#[test]
fn request_joining_active_validation_rechecks_external_edits() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn old() {}\n").unwrap();

    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let (_, initial_hit) = harness.repo_map_graph("demo", &workspace, "src").unwrap();
    assert!(!initial_hit);

    let cache_key = (workspace.root().to_path_buf(), "src".to_owned());
    let flight = harness.repo_map_flight(&cache_key).unwrap();
    let guard = flight
        .gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let generation_before = flight.generation.load(Ordering::Acquire);
    assert_eq!(generation_before % 2, 0);

    // Simulate an owner whose fingerprint scan has already started. The source
    // then changes before a follower arrives. That follower observes the odd
    // generation and must run a new validation after the owner completes.
    flight.generation.fetch_add(1, Ordering::AcqRel);
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn externally_changed_and_longer() {}\n",
    )
    .unwrap();

    let entrants_before = flight.entrants.load(Ordering::Acquire);
    let cache_hit = std::thread::scope(|scope| {
        let handle = scope.spawn(|| harness.repo_map_graph("demo", &workspace, "src").unwrap().1);
        let deadline = Instant::now() + Duration::from_secs(2);
        while flight.entrants.load(Ordering::Acquire) < entrants_before + 1 {
            assert!(
                Instant::now() < deadline,
                "follower did not join active validation"
            );
            std::thread::yield_now();
        }
        flight.generation.fetch_add(1, Ordering::Release);
        drop(guard);
        handle.join().unwrap()
    });

    assert!(
        !cache_hit,
        "a caller that arrives after validation starts must not reuse a pre-edit fingerprint"
    );
    assert_eq!(
        flight.generation.load(Ordering::Acquire),
        generation_before + 4
    );
}
