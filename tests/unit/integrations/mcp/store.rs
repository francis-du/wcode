use super::*;

#[test]
fn rfc3339_timestamp_formatter_matches_epoch_and_leap_day() {
    assert_eq!(rfc3339_millis(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(
        rfc3339_millis(1_709_164_800_123),
        "2024-02-29T00:00:00.123Z"
    );
}

#[test]
fn task_state_is_durable_and_keeps_only_latest_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let mut record = TaskRecord::working(
        "a".repeat(64),
        "demo".into(),
        "semantic_provider_refresh".into(),
        "runtime-a".into(),
    );
    persist(&workspace, &record).unwrap();
    let loaded = load(&workspace, &record.task_id).unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Working);
    record.complete(json!({"content":[],"structuredContent":{"ok":true},"isError":false}));
    persist(&workspace, &record).unwrap();
    let loaded = load(&workspace, &record.task_id).unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Completed);
    assert_eq!(loaded.result.unwrap()["structuredContent"]["ok"], true);
}
