use super::*;
use std::time::Duration;

#[tokio::test]
async fn admission_regression_internal_review_preserves_read_headroom() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let monitor = TaskMonitor::new(["fixture".to_owned()]);
    let mut held = Vec::new();
    for _ in 0..ToolHarness::execution_limit(4) {
        held.push(harness.acquire_tool(true).await.unwrap());
    }
    let mut review = Box::pin(harness_review::run_review_probe(
        harness.clone(),
        monitor.clone(),
        "fixture".to_owned(),
        workspace,
        ReviewProbeSpec {
            id: "admission-fixture",
            args: vec!["status".to_owned()],
        },
        1,
    ));
    assert!(futures_util::poll!(review.as_mut()).is_pending());
    assert_eq!(
        harness.slots.available_permits(),
        1,
        "internal review must not bypass the execution lane and take the reserved read slot"
    );
    let read = harness.acquire_tool(false).await.unwrap();
    drop((read, review, held));
    assert_eq!(harness.slots.available_permits(), 4);
    assert_eq!(monitor.connection_status().queued_tasks, 0);
}

#[tokio::test]
async fn admission_regression_resource_wait_never_holds_execution_or_global_slots() {
    let harness = ToolHarness::new(4).unwrap();
    let mut blocked = Box::pin(harness.acquire_after_resources(
        true,
        Duration::from_secs(1),
        std::future::pending(),
    ));
    assert!(futures_util::poll!(blocked.as_mut()).is_pending());
    let observed = harness.admission_snapshot();
    assert_eq!(observed.waiting_for_resource, 1);
    assert_eq!(observed.slots_in_use, 0);
    assert_eq!(observed.execution_in_use, 0);
    let diagnostic = harness.overload_diagnostic().unwrap();
    assert_eq!(diagnostic["admission"]["waiting_for_resource"], 1);
    let read = harness.acquire_tool(false).await.unwrap();
    drop((read, blocked));
    assert_eq!(harness.admission_snapshot().waiting_for_resource, 0);
    assert_eq!(harness.slots.available_permits(), 4);
    assert_eq!(harness.execution_slots.available_permits(), 3);
}

#[tokio::test]
async fn admission_regression_resource_rejection_and_timeout_are_distinct_and_leak_free() {
    let harness = ToolHarness::new(4).unwrap();
    let error = harness
        .acquire_after_resources(true, Duration::from_secs(1), async {
            Err("memory budget exceeded".to_owned())
        })
        .await
        .err()
        .unwrap();
    assert!(error.contains("resource admission rejected: memory budget exceeded"));
    let error = harness
        .acquire_after_resources(true, Duration::from_millis(25), std::future::pending())
        .await
        .err()
        .unwrap();
    assert!(error.contains("stage=resource"), "{error}");
    assert!(error.contains("slots=0/4"), "{error}");
    assert_eq!(harness.admission_snapshot().waiting_for_resource, 0);
    assert_eq!(harness.slots.available_permits(), 4);
    assert_eq!(harness.execution_slots.available_permits(), 3);
}

#[tokio::test]
async fn admission_regression_execution_timeout_preserves_read_capacity() {
    let harness = ToolHarness::new(4).unwrap();
    let mut held = Vec::new();
    for _ in 0..3 {
        held.push(harness.acquire_tool(true).await.unwrap());
    }
    let error = harness
        .acquire_tool_with_wait_timeout(true, Duration::from_millis(25))
        .await
        .err()
        .unwrap();
    assert!(error.contains("stage=execution"), "{error}");
    assert_eq!(harness.admission_snapshot().waiting_for_execution, 0);
    let read = harness.acquire_tool(false).await.unwrap();
    drop((held, read));
    assert_eq!(harness.slots.available_permits(), 4);
    assert_eq!(harness.execution_slots.available_permits(), 3);
}

#[tokio::test]
async fn admission_regression_slot_timeout_releases_already_reserved_execution_permit() {
    let harness = ToolHarness::new(4).unwrap();
    let mut reads = Vec::new();
    for _ in 0..4 {
        reads.push(harness.acquire_tool(false).await.unwrap());
    }
    let error = harness
        .acquire_tool_with_wait_timeout(true, Duration::from_millis(25))
        .await
        .err()
        .unwrap();
    assert!(error.contains("stage=slot"), "{error}");
    assert_eq!(harness.admission_snapshot().waiting_for_slot, 0);
    assert_eq!(harness.execution_slots.available_permits(), 3);
    drop(reads);
    assert_eq!(harness.slots.available_permits(), 4);
}

#[test]
fn execution_admission_preserves_read_headroom_without_reusing_subspace_process_quota() {
    assert_eq!(ToolHarness::execution_limit_for(32, 4), 28);
    assert_eq!(ToolHarness::execution_limit_for(16, 4), 14);
    assert_eq!(ToolHarness::execution_limit_for(4, 4), 3);
    assert_eq!(ToolHarness::execution_limit_for(1, 4), 1);
}

#[tokio::test]
async fn execution_headroom_keeps_total_cap_and_single_slot_compatibility() {
    for total in [1, 2, 4, 8, 32, 256] {
        let harness = ToolHarness::new(total).unwrap();
        let limit = ToolHarness::execution_limit(total);
        let outer_limit = total.saturating_sub(total.div_ceil(8).min(4)).max(1);
        assert_eq!(limit, outer_limit);
        let mut commands = Vec::new();
        for _ in 0..limit {
            commands.push(harness.acquire_tool(true).await.unwrap());
        }
        let mut waiting = Box::pin(harness.acquire_tool(true));
        assert!(futures_util::poll!(waiting.as_mut()).is_pending());
        let mut reads = Vec::new();
        for _ in limit..total {
            reads.push(harness.acquire_tool(false).await.unwrap());
        }
        assert_eq!(harness.slots.available_permits(), 0);
        drop(waiting);
        drop((commands, reads));
        assert_eq!(harness.slots.available_permits(), total);
        assert_eq!(harness.execution_slots.available_permits(), limit);
    }
}

#[tokio::test]
async fn bounded_tool_admission_fails_before_transport_deadlines() {
    let harness = ToolHarness::new(1).unwrap();
    let held = harness.acquire_tool(false).await.unwrap();
    let started = std::time::Instant::now();
    let error = harness
        .acquire_tool_with_wait_timeout(false, Duration::from_millis(25))
        .await
        .err()
        .expect("bounded admission should time out while capacity is held");
    assert!(error.contains("tool capacity remained busy for 25 ms"));
    assert!(error.contains("request was not started"));
    assert!(started.elapsed() < Duration::from_secs(1));
    drop(held);
    let recovered = harness
        .acquire_tool_with_wait_timeout(false, Duration::from_secs(1))
        .await
        .unwrap();
    drop(recovered);
}

#[tokio::test]
async fn execution_waiters_keep_fifo_order_and_cancel_without_leaks() {
    let harness = ToolHarness::new(4).unwrap();
    let mut held = Vec::new();
    for _ in 0..ToolHarness::execution_limit(4) {
        held.push(harness.acquire_tool(true).await.unwrap());
    }
    let mut first = Box::pin(harness.acquire_tool(true));
    let mut second = Box::pin(harness.acquire_tool(true));
    assert!(futures_util::poll!(first.as_mut()).is_pending());
    assert!(futures_util::poll!(second.as_mut()).is_pending());
    held.pop();
    assert!(futures_util::poll!(second.as_mut()).is_pending());
    let first = tokio::time::timeout(Duration::from_secs(1), first)
        .await
        .unwrap()
        .unwrap();
    drop(second);
    drop((first, held));
    assert_eq!(harness.slots.available_permits(), 4);

    let execution_limit = ToolHarness::execution_limit(4);
    let mut recovered = Vec::new();
    for _ in 0..execution_limit {
        recovered.push(
            tokio::time::timeout(
                Duration::from_secs(1),
                harness.execution_slots.clone().acquire_owned(),
            )
            .await
            .expect("cancelled waiter must not strand execution capacity")
            .expect("execution semaphore stays open"),
        );
    }
    assert_eq!(harness.execution_slots.available_permits(), 0);
    drop(recovered);
    assert_eq!(harness.execution_slots.available_permits(), execution_limit);
}
