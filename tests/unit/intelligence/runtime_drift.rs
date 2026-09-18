use super::*;

#[test]
fn runtime_queue_wait_deviation_is_explicit_and_bounded() {
    let cap =
        u64::try_from(crate::resource::PROCESS_QUEUE_WAIT_CAP.as_millis()).unwrap_or(u64::MAX);
    let mut queue = ProcessQueueSnapshot {
        limit: 4,
        ..ProcessQueueSnapshot::default()
    };
    queue.last_wait_ms = cap;
    queue.last_wait_age_ms = Some(0);
    assert!(queue_wait_deviation(&queue).is_none());

    queue.last_wait_ms = cap.saturating_mul(2);
    let deviation = queue_wait_deviation(&queue).expect("wait beyond the design bound must drift");
    assert_eq!(deviation.metric, "child_process_queue_wait");
    assert_eq!(deviation.expected_max, cap as f64);
    assert_eq!(deviation.observed, cap.saturating_mul(2) as f64);
    assert_eq!(deviation.deviation_percent, 100.0);
    assert_eq!(deviation.precision, "runtime_observed");
    assert!(!deviation.revision_bound);

    queue.last_wait_age_ms = Some(RECENT_RUNTIME_WAIT_MS + 1);
    assert!(
        queue_wait_deviation(&queue).is_none(),
        "old queue incidents must age out"
    );
}
