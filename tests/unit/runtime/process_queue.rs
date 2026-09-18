use super::*;
use std::time::Duration;

#[tokio::test]
async fn queue_capacity_and_wait_metrics_remain_bounded() {
    for limit in [1, 2, 4, 32] {
        let queue = ProcessQueue::new(limit);
        let mut held = Vec::new();
        for _ in 0..limit {
            held.push(queue.acquire().await.unwrap());
        }
        assert_eq!(queue.snapshot().active, limit);
        assert!(
            tokio::time::timeout(Duration::from_millis(25), queue.acquire())
                .await
                .is_err()
        );
        let snapshot = queue.snapshot();
        assert_eq!(snapshot.waiting, 0);
        assert_eq!(snapshot.waits, 1);
        assert!(snapshot.total_wait_ms >= 20);
        assert!(snapshot.max_wait_ms <= snapshot.total_wait_ms);
        assert!(snapshot.last_wait_ms >= 20);
        assert!(snapshot.last_wait_age_ms.is_some_and(|age| age < 1_000));
        drop(held);
        assert_eq!(queue.snapshot().active, 0);
        assert!(queue.acquire().await.is_ok());
    }
}

#[tokio::test]
async fn cancelled_process_waiter_does_not_leak_metrics_or_permits() {
    let queue = Arc::new(ProcessQueue::new(1));
    let held = queue.acquire().await.unwrap();
    let worker_queue = queue.clone();
    let mut workers = tokio::task::JoinSet::new();
    workers.spawn(async move { worker_queue.acquire().await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while queue.snapshot().waiting != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    workers.abort_all();
    while workers.join_next().await.is_some() {}
    assert_eq!(queue.snapshot().waiting, 0);
    assert_eq!(queue.snapshot().active, 1);
    drop(held);
    assert_eq!(queue.snapshot().active, 0);
    assert!(queue.acquire().await.is_ok());
}

#[tokio::test]
async fn closed_process_queue_cleans_up_existing_waiters() {
    let queue = Arc::new(ProcessQueue::new(1));
    let held = queue.acquire().await.unwrap();
    let worker_queue = queue.clone();
    let mut workers = tokio::task::JoinSet::new();
    workers.spawn(async move { worker_queue.acquire().await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while queue.snapshot().waiting != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    queue.slots.close();
    assert!(workers.join_next().await.unwrap().unwrap().is_err());
    assert_eq!(queue.snapshot().waiting, 0);
    assert!(queue.acquire().await.is_err());
    assert_eq!(queue.snapshot().waits, 1);
    drop(held);
}
