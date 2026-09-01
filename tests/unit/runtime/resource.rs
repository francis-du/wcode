use super::*;

#[test]
fn burst_friendly_limits_keep_tool_concurrency_and_bound_cpu_workers() {
    let limits = ResourceLimits::new(10.0, 512, DEFAULT_MAX_PARALLEL_TOOLS).unwrap();
    assert_eq!(limits.effective_parallel_tools, 32);
    assert_eq!(limits.interactive_cpu_percent, 100.0);
    assert!((1..=4).contains(&limits.cpu_burst_threads));
    assert_eq!(limits.rayon_threads, limits.cpu_burst_threads);
    assert_eq!(limits.child_processes, 2);
    assert!((1..=2).contains(&limits.child_threads));
    assert_eq!(limits.indexed_file_limit(), REPO_MAP_MAX_FILES);
    assert_eq!(limits.ast_file_limit(), 32);
    assert_eq!(limits.ast_byte_limit(), 32 * 1024 * 1024);
    assert_eq!(limits.semantic_session_limit(), 2);
    assert_eq!(limits.repo_map_cache_limit(), 2);

    let explicitly_raised = ResourceLimits::new(10.0, 512, 120).unwrap();
    assert_eq!(explicitly_raised.effective_parallel_tools, 64);
}

#[test]
fn invalid_resource_limits_fail_closed() {
    assert!(ResourceLimits::new(0.0, 512, 4).is_err());
    assert!(ResourceLimits::new(101.0, 512, 4).is_err());
    assert!(ResourceLimits::new(10.0, 64, 4).is_err());
    assert!(ResourceLimits::new(10.0, 512, 0).is_err());
}

#[test]
fn memory_pressure_preserves_temporary_burst_headroom() {
    let limit = 1_000;
    assert_eq!(memory_pressure(799, limit), MemoryPressure::Normal);
    assert_eq!(memory_pressure(800, limit), MemoryPressure::Elevated);
    assert_eq!(memory_pressure(1_000, limit), MemoryPressure::Critical);
    assert_eq!(memory_pressure(1_249, limit), MemoryPressure::Critical);
    assert_eq!(memory_pressure(1_250, limit), MemoryPressure::OverLimit);
}

#[test]
fn credit_refill_respects_interactive_and_background_caps() {
    let limits = ResourceLimits::new(10.0, 512, 32).unwrap();
    let mut budget = CpuBudget {
        last_refill: Instant::now() - Duration::from_secs(100),
        background_credit_seconds: -10.0,
        tokens: -10.0,
    };
    refill_budget(&mut budget, limits);
    assert!(budget.tokens <= limits.interactive_burst_seconds());
    assert!(budget.background_credit_seconds <= limits.background_burst_seconds());
    assert!(budget.tokens > budget.background_credit_seconds);
}

#[test]
fn sustained_cpu_pressure_requires_a_grace_period() {
    let limits = ResourceLimits::new(10.0, 512, 32).unwrap();
    let mut telemetry = Telemetry {
        cpu_pressure_since: Some(Instant::now() - SUSTAINED_CPU_GRACE / 2),
        ..Telemetry::default()
    };
    assert!(!snapshot_from(limits, &telemetry, Instant::now()).cpu_pressure);
    telemetry.cpu_pressure_since = Some(Instant::now() - SUSTAINED_CPU_GRACE * 2);
    assert!(snapshot_from(limits, &telemetry, Instant::now()).cpu_pressure);
}

#[tokio::test(flavor = "current_thread")]
async fn elevated_memory_does_not_reject_normal_tool_admission() {
    let governor = ResourceGovernor::new(ResourceLimits::new(10.0, 512, 32).unwrap());
    {
        let mut telemetry = lock_recover(&governor.telemetry);
        telemetry.memory_pressure = MemoryPressure::Elevated;
        telemetry.resident_memory_bytes = Some(430 * 1024 * 1024);
        telemetry.last_sample_at = Some(Instant::now());
    }
    governor.admit_tool().await.unwrap();
}

#[test]
fn background_cpu_charge_preserves_the_next_foreground_burst() {
    let governor = ResourceGovernor::new(ResourceLimits::new(10.0, 512, 32).unwrap());
    {
        let mut budget = lock_recover(&governor.cpu_budget);
        budget.last_refill = Instant::now();
        budget.tokens = 1.0;
        budget.background_credit_seconds = 1.0;
    }
    governor.finish_cpu_work(
        None,
        Instant::now() - Duration::from_millis(100),
        WorkClass::Background,
    );
    let budget = lock_recover(&governor.cpu_budget);
    assert!(budget.tokens >= 0.99);
    assert!(budget.background_credit_seconds < 0.95);
}

#[test]
fn stale_telemetry_does_not_freeze_background_maintenance() {
    let governor = ResourceGovernor::new(ResourceLimits::new(10.0, 512, 32).unwrap());
    {
        let mut telemetry = lock_recover(&governor.telemetry);
        telemetry.memory_pressure = MemoryPressure::OverLimit;
        telemetry.sustained_cpu_percent = Some(500.0);
        telemetry.cpu_pressure_since = Some(Instant::now() - SUSTAINED_CPU_GRACE * 2);
        telemetry.last_sample_at = Some(Instant::now() - TELEMETRY_STALE_AFTER * 2);
    }
    assert!(governor.background_ready());
}

#[tokio::test(flavor = "current_thread")]
async fn sustained_cpu_pressure_adds_bounded_backpressure_instead_of_freezing_tools() {
    let governor = ResourceGovernor::new(ResourceLimits::new(10.0, 512, 32).unwrap());
    {
        let mut telemetry = lock_recover(&governor.telemetry);
        telemetry.sustained_cpu_percent = Some(200.0);
        telemetry.cpu_pressure_since = Some(Instant::now() - SUSTAINED_CPU_GRACE * 2);
        telemetry.last_sample_at = Some(Instant::now());
    }
    tokio::time::timeout(Duration::from_secs(1), governor.admit_tool())
        .await
        .expect("CPU backpressure must remain bounded")
        .unwrap();
    assert_eq!(lock_recover(&governor.telemetry).admission_delays, 1);
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn governed_child_guard_owns_and_terminates_its_process_group() {
    let mut command = Command::new("sleep");
    command.arg("5");
    apply_child_limits(&mut command);
    let mut child = command.spawn().expect("sleep must start");
    let pid = i32::try_from(child.id().expect("child PID")).unwrap();
    // SAFETY: pid belongs to the live child created immediately above.
    assert_eq!(unsafe { libc::getpgid(pid) }, pid);
    drop(supervise_child(&child));
    tokio::time::timeout(Duration::from_secs(1), child.wait())
        .await
        .expect("dropping the process-group guard must terminate the child")
        .expect("terminated child must remain waitable");
}
