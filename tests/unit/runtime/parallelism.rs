use super::*;

#[test]
fn perf_foreground_capacity_uses_hardware_without_raising_background_budget() {
    let host = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(8)
        .max(1);
    let limits = ResourceLimits::new(10.0, 512, 32).unwrap();
    assert_eq!(limits.cpu_burst_threads, host.min(8));
    assert_eq!(limits.rayon_threads, limits.cpu_burst_threads);
    assert_eq!(
        limits.interactive_cpu_percent,
        limits.cpu_burst_threads as f64 * 100.0
    );
    assert_eq!(limits.max_cpu_percent, 10.0);
    assert_eq!(
        limits.child_processes,
        (512usize / 160)
            .clamp(1, 8)
            .min(limits.cpu_burst_threads.div_ceil(limits.child_threads))
            .min(limits.effective_parallel_tools),
        "heavy process capacity must scale with real CPU/memory headroom instead of tool slots"
    );
    let small = ResourceLimits::new(10.0, 128, 1).unwrap();
    assert_eq!(small.cpu_burst_threads, 1);
    assert_eq!(small.interactive_cpu_percent, 100.0);
}

fn blocking_workers_started(blocking_limit: usize) -> usize {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(blocking_limit)
        .enable_all()
        .build()
        .unwrap();
    let started = std::sync::Arc::new((Mutex::new(0usize), Condvar::new()));
    let released = std::sync::Arc::new((Mutex::new(false), Condvar::new()));
    let mut handles = Vec::new();
    for _ in 0..32 {
        let started = started.clone();
        let released = released.clone();
        handles.push(runtime.spawn_blocking(move || {
            let (count, changed) = &*started;
            *count.lock().unwrap() += 1;
            changed.notify_all();
            let (flag, changed) = &*released;
            let guard = flag.lock().unwrap();
            drop(changed.wait_while(guard, |released| !*released).unwrap());
        }));
    }
    let (count, changed) = &*started;
    let observed = {
        let guard = count.lock().unwrap();
        let (guard, _) = changed
            .wait_timeout_while(guard, Duration::from_secs(2), |count| *count < 32)
            .unwrap();
        *guard
    };
    *released.0.lock().unwrap() = true;
    released.1.notify_all();
    runtime.block_on(async {
        for handle in handles {
            handle.await.unwrap();
        }
    });
    observed
}

#[test]
fn perf_blocking_pool_can_start_all_32_independent_requests() {
    assert_eq!(blocking_workers_started(16), 16, "legacy pool control");
    assert_eq!(
        blocking_workers_started(TOKIO_MAX_BLOCKING_THREADS),
        32,
        "ready requests must not queue behind a hidden 16-thread ceiling"
    );
}
