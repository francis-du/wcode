use super::super::*;

#[test]
fn inspection_capacity_scales_with_memory_cpu_and_tool_bounds() {
    for memory_mb in [128, 256, 512, 1_024, 2_048] {
        for tool_limit in [1, 4, 32] {
            let limits = ResourceLimits::new(10.0, memory_mb, tool_limit).unwrap();
            let probes = limits.probe_process_limit();
            assert!((1..=4).contains(&probes));
            assert!(probes <= limits.cpu_burst_threads);
            assert!(probes <= limits.effective_parallel_tools);
            assert!(probes <= (memory_mb / 128) as usize);
        }
    }
    assert_eq!(
        ResourceLimits::new(10.0, 512, 32).unwrap().child_processes,
        2
    );
}

#[tokio::test]
async fn independent_process_queues_keep_compiler_and_probe_limits() {
    let limits = ResourceLimits::new(10.0, 512, 32).unwrap();
    let governor = ResourceGovernor::new(limits);
    let mut heavy = Vec::new();
    for _ in 0..limits.child_processes {
        heavy.push(governor.acquire_child().await.unwrap());
    }
    let mut probes = Vec::new();
    for _ in 0..limits.probe_process_limit() {
        probes.push(governor.acquire_git_probe().await.unwrap());
    }
    let snapshot = governor.snapshot();
    assert_eq!(snapshot.child_queue.active, limits.child_processes);
    assert_eq!(snapshot.probe_queue.active, limits.probe_process_limit());
    assert!(
        tokio::time::timeout(Duration::from_millis(25), governor.acquire_child())
            .await
            .is_err()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), governor.acquire_git_probe())
            .await
            .is_err()
    );
    assert_eq!(governor.snapshot().child_queue.waiting, 0);
    assert_eq!(governor.snapshot().probe_queue.waiting, 0);
    drop(probes);
    let probe = governor.acquire_git_probe().await.unwrap();
    assert_eq!(
        governor.snapshot().child_queue.active,
        limits.child_processes
    );
    drop(probe);
    drop(heavy);
    assert_eq!(governor.snapshot().child_queue.active, 0);
    assert_eq!(governor.snapshot().probe_queue.active, 0);
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
        tokio::time::timeout(Duration::from_millis(25), governor.acquire_git_probe())
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
    assert!(governor.acquire_git_probe().await.is_ok());
}
