use super::*;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;

fn validate(harness: &ToolHarness, workspace: &Workspace, kind: usize) -> Result<()> {
    match kind {
        0 => harness.load_project_profile(workspace).map(|_| ()),
        1 => harness.convention_status_cached(workspace).map(|_| ()),
        2 => harness
            .repo_map_graph("fixture", workspace, ".")
            .map(|_| ()),
        _ => unreachable!(),
    }
}

fn contended_validation_releases_worker(kind: usize) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname='parallel-fixture'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn answer() -> u8 { 42 }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let flight = match kind {
        0 => harness.project_flight(workspace.root()).unwrap(),
        1 => harness.convention_flight(workspace.root()).unwrap(),
        _ => harness
            .repo_map_flight(&(workspace.root().to_path_buf(), ".".to_owned()))
            .unwrap(),
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for warm in [false, true] {
        if warm {
            validate(&harness, &workspace, kind).unwrap();
        }
        let generation = flight.generation.load(Ordering::Acquire);
        let guard = flight.gate.lock().unwrap();
        let permit = runtime.block_on(harness.acquire_tool(false)).unwrap();
        let worker_harness = harness.clone();
        let worker_workspace = workspace.clone();
        let (tx, rx) = mpsc::channel();
        pool.spawn(move || {
            let result = validate(&worker_harness, &worker_workspace, kind)
                .map_err(|error| error.to_string());
            drop(permit);
            let _ = tx.send(result);
        });
        // The holder may itself be waiting for work on this pool. A follower
        // must not park the only worker, even if a warm cache already exists.
        let early = rx.recv_timeout(Duration::from_millis(500));
        drop(guard); // Always unblock the old implementation before asserting.
        let returned_early = early.is_ok();
        let result = early.unwrap_or_else(|_| rx.recv_timeout(Duration::from_secs(5)).unwrap());
        assert_eq!(harness.slots.available_permits(), 4);
        assert_eq!(flight.coalescible_callers.load(Ordering::Acquire), 0);
        assert!(returned_early, "cache kind {kind} (warm={warm}) pinned a Rayon worker and its tool permit while waiting for validation");
        assert!(result.unwrap_err().contains("validation busy"));
        assert_eq!(flight.generation.load(Ordering::Acquire), generation);
        validate(&harness, &workspace, kind).unwrap();
    }
}

#[test]
fn parallel_liveness_profile_contention_releases_worker_and_permit() {
    contended_validation_releases_worker(0);
}

#[test]
fn parallel_liveness_convention_contention_releases_worker_and_permit() {
    contended_validation_releases_worker(1);
}

#[test]
fn parallel_liveness_repo_map_contention_releases_worker_and_permit() {
    contended_validation_releases_worker(2);
}

// Isolate stress/reentrant scheduling so a regression cannot strand the test
// runner's global pool. The parent always reaps the exact child on its deadline.
fn isolated_case(case: &str, test_name: &str, work: impl FnOnce()) {
    const CHILD: &str = "WCODE_PARALLEL_LIVENESS_CHILD";
    if std::env::var(CHILD).as_deref() == Ok(case) {
        eprintln!("liveness case={case} child_started");
        work();
        eprintln!("liveness case={case} child_completed");
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let log = directory.path().join("child.log");
    let output = fs::File::create(&log).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--nocapture"])
        .env(CHILD, case)
        .stdin(std::process::Stdio::null())
        .stdout(output.try_clone().unwrap())
        .stderr(output)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(40);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    // Reaping does not close the Windows Child's redirected handles.
    // Release them before reading/removing the fixture directory.
    drop(child);
    let diagnostic = fs::read_to_string(log).unwrap();
    assert!(
        status.is_some_and(|status| status.success()),
        "{case} did not finish successfully: {status:?}\n{diagnostic}"
    );
    assert!(
        diagnostic.contains("1 passed"),
        "child did not execute the named regression: {diagnostic}"
    );
}

fn fixture() -> (tempfile::TempDir, Workspace) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname='parallel-fixture'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn answer() -> u8 { 42 }\n",
    )
    .unwrap();
    for index in 0..8 {
        fs::write(
            root.path().join(format!("src/caller{index}.rs")),
            format!("pub fn caller{index}() -> u8 {{ crate::answer() }}\n"),
        )
        .unwrap();
    }
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    (root, workspace)
}

#[test]
fn parallel_liveness_saturated_followers_release_all_64_slots() {
    isolated_case("saturated", "harness::tests::parallel_liveness::parallel_liveness_saturated_followers_release_all_64_slots", || {
        let (_root, workspace) = fixture();
        let harness = ToolHarness::new(64).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let pool = rayon::ThreadPoolBuilder::new().num_threads(2).build().unwrap();
        let flight = harness.repo_map_flight(&(workspace.root().to_path_buf(), ".".to_owned())).unwrap();
        let guard = flight.gate.lock().unwrap();
        let permits = runtime.block_on(async {
            let mut permits = Vec::new();
            for _ in 0..64 { permits.push(harness.acquire_tool(false).await.unwrap()); }
            permits
        });
        assert_eq!(harness.admission_snapshot().slots_in_use, 64);
        let (tx, rx) = mpsc::channel();
        for permit in permits {
            let harness = harness.clone();
            let workspace = workspace.clone();
            let tx = tx.clone();
            pool.spawn(move || {
                let result = validate(&harness, &workspace, 2).map_err(|error| error.to_string());
                drop(permit);
                let _ = tx.send(result);
            });
        }
        drop(tx);
        for _ in 0..64 {
            let result = rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(result.unwrap_err().contains("validation busy"));
        }
        assert_eq!(harness.admission_snapshot().slots_in_use, 0);
        assert_eq!(flight.coalescible_callers.load(Ordering::Acquire), 0);
        assert_eq!(flight.generation.load(Ordering::Acquire), 0);
        drop(guard);
        validate(&harness, &workspace, 2).unwrap();
        assert!(!harness.repo_map_cache.lock().unwrap().is_empty());
    });
}

#[test]
fn parallel_liveness_framework_discovery_avoids_saturated_global_pool() {
    isolated_case(
        "framework-discovery",
        "harness::tests::parallel_liveness::parallel_liveness_framework_discovery_avoids_saturated_global_pool",
        || {
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build_global()
                .unwrap();
            let (_root, workspace) = fixture();
            let (ready_tx, ready_rx) = mpsc::channel();
            let mut releases = Vec::new();
            for _ in 0..2 {
                let (release_tx, release_rx) = mpsc::channel::<()>();
                releases.push(release_tx);
                let ready_tx = ready_tx.clone();
                rayon::spawn(move || {
                    ready_tx.send(()).unwrap();
                    let _ = release_rx.recv();
                });
            }
            for _ in 0..2 {
                ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            let (result_tx, result_rx) = mpsc::channel();
            let worker = std::thread::spawn(move || {
                let result = crate::stage_executor::registry(&workspace)
                    .map_err(|error| error.to_string());
                let _ = result_tx.send(result);
            });
            let early = result_rx.recv_timeout(Duration::from_secs(5));
            // Release every global worker even when the old scanner blocks.
            drop(releases);
            worker.join().unwrap();
            assert!(
                early.is_ok(),
                "framework discovery waited for the saturated global Rayon pool"
            );
            early.unwrap().unwrap();
        },
    );
}

#[test]
fn parallel_liveness_mixed_64_requests_drain_and_recover() {
    isolated_case(
        "mixed",
        "harness::tests::parallel_liveness::parallel_liveness_mixed_64_requests_drain_and_recover",
        || {
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build_global()
                .unwrap();
            eprintln!("liveness mixed global_pool_ready");
            let fixtures = (0..3).map(|_| fixture()).collect::<Vec<_>>();
            eprintln!("liveness mixed fixtures_ready");
            let harness = ToolHarness::new(64).unwrap();
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .max_blocking_threads(64)
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                for wave in 0..3 {
                    eprintln!("liveness mixed wave={wave} started");
                    let mut tasks = tokio::task::JoinSet::new();
                    for index in 0..64 {
                        let permit = harness.acquire_tool(false).await.unwrap();
                        let harness = harness.clone();
                        let workspace = fixtures[index % 3].1.clone();
                        tasks.spawn_blocking(move || {
                            let _permit = permit;
                            let operation = index % 4;
                            eprintln!(
                                "liveness mixed wave={wave} request={index} operation={operation} started"
                            );
                            let result = match operation {
                                0 => harness
                                    .agent_context(
                                        "fixture",
                                        &workspace,
                                        "show callers of answer",
                                        4_000,
                                        &[],
                                    )
                                    .map(|_| ()),
                                1 => harness.project_context("fixture", &workspace).map(|_| ()),
                                2 => workspace
                                    .read_files(
                                        &["src/lib.rs".to_owned(), "src/caller0.rs".to_owned()],
                                        1,
                                        None,
                                    )
                                    .map(|files| {
                                        assert!(files.iter().all(|file| file["ok"] == true));
                                    }),
                                _ => workspace
                                    .search_many(&["answer".to_owned()], "src", 100)
                                    .map(|hits| {
                                        assert!(!hits.is_empty());
                                    }),
                            };
                            eprintln!(
                                "liveness mixed wave={wave} request={index} completed ok={}",
                                result.is_ok()
                            );
                            (operation, result.map_err(|error| error.to_string()))
                        });
                    }
                    let mut completed = 0;
                    let mut completed_by_operation = [0usize; 4];
                    let mut successful = 0;
                    while let Some(result) = tasks.join_next().await {
                        let (operation, result) = result.unwrap();
                        completed_by_operation[operation] += 1;
                        match result {
                            Ok(()) => successful += 1,
                            Err(error) => assert!(error.contains("validation busy"), "{error}"),
                        }
                        completed += 1;
                    }
                    assert_eq!(completed, 64);
                    assert_eq!(
                        completed_by_operation,
                        [16, 16, 16, 16],
                        "every request class, including project_context, must drain"
                    );
                    assert!(
                        successful >= 32,
                        "independent reads and searches must remain usable"
                    );
                    let admission = harness.admission_snapshot();
                    assert_eq!(admission.slots_in_use, 0);
                    assert_eq!(admission.waiting_for_slot, 0);
                    eprintln!("liveness mixed wave={wave} drained successful={successful}");
                }
            });
            for (_, workspace) in &fixtures {
                harness
                    .agent_context("fixture", workspace, "answer", 4_000, &[])
                    .unwrap();
                assert_eq!(
                    fs::read_to_string(workspace.root().join("src/lib.rs")).unwrap(),
                    "pub fn answer() -> u8 { 42 }\n"
                );
            }
        },
    );
}
