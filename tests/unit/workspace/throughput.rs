use super::*;
use std::time::{Duration, Instant};

#[test]
fn perf_io_batches_share_a_bound_and_preserve_order() {
    let active = AtomicUsize::new(0);
    let peak = AtomicUsize::new(0);
    let work = |number: &usize| {
        let current = active.fetch_add(1, Ordering::SeqCst) + 1;
        peak.fetch_max(current, Ordering::SeqCst);
        // Controlled I/O wait, not a claim about disk or end-to-end latency.
        std::thread::sleep(Duration::from_millis(5));
        active.fetch_sub(1, Ordering::SeqCst);
        number * 2
    };
    let inputs = (0..64).collect::<Vec<_>>();
    std::thread::scope(|scope| {
        let first = scope.spawn(|| crate::resource::parallel_io(&inputs, work).unwrap());
        let second = scope.spawn(|| crate::resource::parallel_io(&inputs, work).unwrap());
        let expected = inputs.iter().map(|value| value * 2).collect::<Vec<_>>();
        assert_eq!(first.join().unwrap(), expected);
        assert_eq!(second.join().unwrap(), expected);
    });
    assert_eq!(active.load(Ordering::SeqCst), 0);
    assert!(peak.load(Ordering::SeqCst) <= crate::resource::limits().io_parallelism());
    assert!(peak.load(Ordering::SeqCst) > 4);
    assert!(
        crate::resource::parallel_io::<usize, usize, _>(&[], |value| *value)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn perf_parallel_edits_keep_partial_outcomes_and_stale_sha_checks() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let files = (0..32)
        .map(|number| CreateFileRequest {
            path: format!("file_{number}.txt"),
            content: format!("original {number}\n"),
        })
        .collect::<Vec<_>>();
    let created = workspace.create_files(&files).unwrap();
    assert!(created.iter().all(|item| item.ok));
    let edits = created
        .into_iter()
        .enumerate()
        .map(|(number, item)| FileEditRequest {
            path: item.path,
            expected_sha256: if number == 13 {
                "stale".into()
            } else {
                item.result.unwrap().sha256_after
            },
            edits: vec![TextEdit {
                old_text: "original".into(),
                new_text: "updated".into(),
                start_line: Some(1),
                end_line: Some(1),
            }],
        })
        .collect::<Vec<_>>();
    let results = workspace.apply_file_edits(&edits).unwrap();
    for (number, item) in results.iter().enumerate() {
        assert_eq!(item.path, files[number].path);
        assert_eq!(item.ok, number != 13);
        let expected = if number == 13 { "original" } else { "updated" };
        assert_eq!(
            fs::read_to_string(root.path().join(&item.path)).unwrap(),
            format!("{expected} {number}\n")
        );
    }
    let readonly = Workspace::new(root.path(), false, false).unwrap();
    assert!(readonly
        .apply_file_edits(&edits)
        .unwrap()
        .iter()
        .all(|item| !item.ok));
}

fn elapsed_us<T>(work: impl FnOnce() -> T) -> (T, u128) {
    let started = Instant::now();
    let result = work();
    (result, started.elapsed().as_micros())
}

fn legacy_edits(
    pool: &rayon::ThreadPool,
    workspace: &Workspace,
    edits: &[FileEditRequest],
) -> Vec<BatchEditItem> {
    pool.install(|| {
        edits
            .par_iter()
            .map(|file| {
                match workspace.apply_edits(&file.path, &file.edits, &file.expected_sha256) {
                    Ok(result) => BatchEditItem {
                        path: file.path.clone(),
                        ok: true,
                        result: Some(result),
                        error: None,
                    },
                    Err(error) => BatchEditItem {
                        path: file.path.clone(),
                        ok: false,
                        result: None,
                        error: Some(error.to_string()),
                    },
                }
            })
            .collect()
    })
}

#[test]
fn perf_measure_guarded_file_batches_against_four_worker_baseline() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let baseline = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    crate::resource::parallel_io(&[0, 1], |value| *value).unwrap();
    let body = format!("needle=v0\n{}", "bounded fixture content\n".repeat(2800));
    let read_paths = (0..32)
        .map(|number| format!("read_{number}.txt"))
        .collect::<Vec<_>>();
    for path in &read_paths {
        fs::write(root.path().join(path), &body).unwrap();
    }
    for side in ["old", "new"] {
        for number in 0..32 {
            fs::write(root.path().join(format!("{side}_{number}.txt")), &body).unwrap();
        }
    }
    let expected_sha = sha256(body.as_bytes());
    let old_read = || {
        baseline.install(|| {
            read_paths
                .par_iter()
                .map(|path| match workspace.read_file(path, 1, Some(12)) {
                    Ok(file) => serde_json::json!({"path":path,"ok":true,"file":file}),
                    Err(error) => {
                        serde_json::json!({"path":path,"ok":false,"error":error.to_string()})
                    }
                })
                .collect::<Vec<_>>()
        })
    };
    // Keep CPU capacity identical for the unchanged read path. Production's
    // foreground hardware scaling is tested independently.
    let new_read = || baseline.install(|| workspace.read_files(&read_paths, 1, Some(12)).unwrap());
    let mut reads = Vec::new();
    let mut edits = Vec::new();
    for round in 0..5 {
        let (before, after) = if round % 2 == 0 {
            (elapsed_us(old_read), elapsed_us(new_read))
        } else {
            let after = elapsed_us(new_read);
            (elapsed_us(old_read), after)
        };
        assert_eq!(before.0, after.0);
        assert!(after
            .0
            .iter()
            .all(|item| item["file"]["sha256"] == expected_sha));
        reads.push(serde_json::json!({"baseline":before.1,"candidate":after.1}));
        let (old, new) = if round % 2 == 0 {
            ("v0", "v1")
        } else {
            ("v1", "v0")
        };
        let current = body.replacen("v0", old, 1);
        let expected = sha256(current.as_bytes());
        let requests = |side: &str| {
            (0..32)
                .map(|number| FileEditRequest {
                    path: format!("{side}_{number}.txt"),
                    expected_sha256: expected.clone(),
                    edits: vec![TextEdit {
                        old_text: format!("needle={old}"),
                        new_text: format!("needle={new}"),
                        start_line: Some(1),
                        end_line: Some(1),
                    }],
                })
                .collect::<Vec<_>>()
        };
        let old_requests = requests("old");
        let new_requests = requests("new");
        let old_edit = || legacy_edits(&baseline, &workspace, &old_requests);
        let new_edit = || workspace.apply_file_edits(&new_requests).unwrap();
        let (before, after) = if round % 2 == 0 {
            (elapsed_us(old_edit), elapsed_us(new_edit))
        } else {
            let after = elapsed_us(new_edit);
            (elapsed_us(old_edit), after)
        };
        assert!(before.0.iter().chain(&after.0).all(|item| item.ok));
        let final_sha = sha256(body.replacen("v0", new, 1).as_bytes());
        assert!(before.0.iter().chain(&after.0).all(|item| item
            .result
            .as_ref()
            .unwrap()
            .sha256_after
            == final_sha));
        edits.push(serde_json::json!({"baseline":before.1,"candidate":after.1}));
    }
    let report = serde_json::json!({
        "kind":"paired-local-file-benchmark", "unit":"microseconds", "tasks_per_batch":32,
        "bytes_per_file":body.len(), "trials":5, "alternating_order":true,
        "baseline_workers":4, "candidate_workers":crate::resource::limits().io_parallelism(),
        "read_workers_both_arms":4, "candidate_workers_apply_to":"atomic edits",
        "rejected_trial":"16-worker warm reads: median 28675 us vs baseline 27488 us; reverted",
        "test_cpu_lanes":crate::resource::limits().cpu_burst_threads,
        "host_parallelism":std::thread::available_parallelism().map(usize::from).unwrap_or(1),
        "read_batches":reads, "atomic_edit_batches":edits,
        "correctness":"every read payload/SHA matched; every edit result and final SHA matched",
        "limits":"debug test build; local temp files; no injected latency; not model or network speed"
    });
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/wcode-throughput.json");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
