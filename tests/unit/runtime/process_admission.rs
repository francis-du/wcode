use super::super::*;

#[test]
fn inspection_capacity_scales_with_memory_cpu_and_tool_bounds() {
    for memory_mb in [128, 256, 512, 1_024, 2_048] {
        for tool_limit in [1, 4, 32] {
            let limits = ResourceLimits::new(10.0, memory_mb, tool_limit).unwrap();
            let probes = limits.probe_process_limit();
            assert!((1..=8).contains(&probes));
            assert!(probes <= limits.cpu_burst_threads);
            assert!(probes <= limits.effective_parallel_tools);
            assert!(probes <= (memory_mb / 64) as usize);
        }
    }
    let default = ResourceLimits::new(10.0, 512, 32).unwrap();
    assert_eq!(
        default.child_processes,
        (512usize / 128)
            .clamp(1, 8)
            .min(default.cpu_burst_threads.div_ceil(default.child_threads))
            .min(default.effective_parallel_tools)
    );
}

#[tokio::test]
async fn independent_process_queues_keep_compiler_and_probe_limits() {
    let limits = ResourceLimits::new(10.0, 512, 32).unwrap();
    let governor = ResourceGovernor::new(limits);
    let host_limit = limits.host_child_process_limit();
    let mut heavy = Vec::new();
    for _ in 0..host_limit {
        heavy.push(governor.acquire_child().await.unwrap());
    }
    let mut probes = Vec::new();
    for _ in 0..limits.probe_process_limit() {
        probes.push(governor.acquire_probe().await.unwrap());
    }
    let snapshot = governor.snapshot();
    assert_eq!(snapshot.child_processes, limits.child_processes);
    assert_eq!(snapshot.host_child_processes, host_limit);
    assert_eq!(snapshot.child_queue.active, host_limit);
    assert_eq!(snapshot.probe_queue.active, limits.probe_process_limit());
    assert!(
        tokio::time::timeout(Duration::from_millis(25), governor.acquire_child())
            .await
            .is_err()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), governor.acquire_probe())
            .await
            .is_err()
    );
    assert_eq!(governor.snapshot().child_queue.waiting, 0);
    assert_eq!(governor.snapshot().probe_queue.waiting, 0);
    drop(probes);
    let probe = governor.acquire_probe().await.unwrap();
    assert_eq!(governor.snapshot().child_queue.active, host_limit);
    drop(probe);
    drop(heavy);
    assert_eq!(governor.snapshot().child_queue.active, 0);
    assert_eq!(governor.snapshot().probe_queue.active, 0);
}

#[tokio::test]
async fn bounded_child_wait_expires_without_leaking_capacity_or_waiters() {
    let limits = ResourceLimits::new(10.0, 512, 32).unwrap();
    let governor = ResourceGovernor::new(limits);
    let root = tempfile::tempdir().unwrap();
    let mut held = Vec::new();
    for _ in 0..limits.child_processes {
        held.push(
            governor
                .acquire_child_for_workspace(root.path())
                .await
                .unwrap(),
        );
    }

    let started = Instant::now();
    let error = governor
        .acquire_child_for_workspace_with_wait_timeout(root.path(), Duration::from_millis(25))
        .await
        .err()
        .expect("bounded subspace admission must time out while local capacity is held");
    assert!(error.contains("command was not started"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(
        governor.snapshot().child_queue.active,
        limits.child_processes,
        "timed-out local admission must not consume an extra host permit"
    );

    drop(held);
    assert_eq!(governor.snapshot().child_queue.active, 0);
    let recovered = governor
        .acquire_child_for_workspace(root.path())
        .await
        .expect("released local capacity must be reusable");
    drop(recovered);
    assert_eq!(governor.snapshot().child_queue.active, 0);
}

fn subspace_test_limits() -> ResourceLimits {
    ResourceLimits {
        max_cpu_percent: 100.0,
        interactive_cpu_percent: 800.0,
        max_memory_bytes: 1024 * 1024 * 1024,
        requested_parallel_tools: 8,
        effective_parallel_tools: 8,
        cpu_burst_threads: 8,
        rayon_threads: 8,
        child_processes: 2,
        child_threads: 2,
    }
}

#[tokio::test]
async fn subspace_process_admission_isolated_under_shared_host_capacity() {
    let limits = subspace_test_limits();
    assert!(limits.host_child_process_limit() > limits.child_processes);
    let governor = ResourceGovernor::new(limits);
    let left = tempfile::tempdir().unwrap();
    let right = tempfile::tempdir().unwrap();
    let mut left_permits = Vec::new();
    for _ in 0..limits.child_processes {
        left_permits.push(
            governor
                .acquire_child_for_workspace(left.path())
                .await
                .unwrap(),
        );
    }

    let right_permit = governor
        .acquire_child_for_workspace_with_wait_timeout(right.path(), Duration::from_millis(50))
        .await
        .expect("another subspace keeps its independent local quota")
        .0;
    let error = governor
        .acquire_child_for_workspace_with_wait_timeout(left.path(), Duration::from_millis(25))
        .await
        .err()
        .expect("exhausted subspace quota must reject another local child");
    assert!(error.contains("command was not started"), "{error}");
    assert_eq!(
        governor.snapshot().child_queue.active,
        limits.child_processes + 1
    );
    drop((right_permit, left_permits));
    assert_eq!(governor.snapshot().child_queue.active, 0);
}

#[tokio::test]
async fn host_process_cap_bounds_aggregate_subspace_activity() {
    let limits = subspace_test_limits();
    let host_limit = limits.host_child_process_limit();
    assert_eq!(host_limit, 8);
    let governor = ResourceGovernor::new(limits);
    let roots = (0..host_limit.div_ceil(limits.child_processes) + 1)
        .map(|_| tempfile::tempdir().unwrap())
        .collect::<Vec<_>>();
    let mut held = Vec::new();
    for root in roots
        .iter()
        .take(host_limit.div_ceil(limits.child_processes))
    {
        for _ in 0..limits.child_processes {
            if held.len() == host_limit {
                break;
            }
            held.push(
                governor
                    .acquire_child_for_workspace(root.path())
                    .await
                    .unwrap(),
            );
        }
    }
    assert_eq!(held.len(), host_limit);
    assert_eq!(governor.snapshot().child_queue.active, host_limit);

    let started = Instant::now();
    let error = governor
        .acquire_child_for_workspace_with_wait_timeout(
            roots.last().unwrap().path(),
            Duration::from_millis(25),
        )
        .await
        .err()
        .expect("exhausted host quota must reject another child");
    assert!(error.contains("command was not started"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(governor.snapshot().child_queue.waiting, 0);

    drop(held);
    assert_eq!(governor.snapshot().child_queue.active, 0);
}

#[tokio::test]
async fn inspection_queue_still_obeys_resource_pressure_admission() {
    let governor = ResourceGovernor::new(ResourceLimits::new(10.0, 512, 32).unwrap());
    {
        let mut telemetry = lock_recover(&governor.telemetry);
        telemetry.memory_pressure = MemoryPressure::OverLimit;
        telemetry.last_sample_at = Some(Instant::now());
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(25), governor.acquire_probe())
            .await
            .is_err()
    );
    assert_eq!(governor.snapshot().probe_queue.active, 0);
    assert_eq!(governor.snapshot().probe_queue.waiting, 0);
    {
        let mut telemetry = lock_recover(&governor.telemetry);
        telemetry.memory_pressure = MemoryPressure::Normal;
        telemetry.last_sample_at = Some(Instant::now());
    }
    assert!(governor.acquire_probe().await.is_ok());
}
