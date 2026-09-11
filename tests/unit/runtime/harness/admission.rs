use super::*;
use std::time::Duration;

#[tokio::test]
async fn execution_headroom_keeps_total_cap_and_single_slot_compatibility() {
    for total in [1, 2, 4, 8, 32, 256] {
        let harness = ToolHarness::new(total).unwrap();
        let limit = ToolHarness::execution_limit(total);
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
    assert_eq!(harness.execution_slots.available_permits(), 3);
}
