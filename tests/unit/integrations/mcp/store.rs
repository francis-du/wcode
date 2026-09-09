use super::*;

fn recovery_record() -> TaskRecord {
    TaskRecord::working(
        "a".repeat(64),
        "demo".into(),
        "semantic_provider_refresh".into(),
        "runtime-a".into(),
    )
}

#[test]
fn durable_recovery_multibyte_errors_fit_the_persistent_byte_bound() {
    for message in ["出错了🦀".repeat(1_000), "x".repeat(5_000_000)] {
        let root = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(root.path(), false, false).unwrap();
        let mut record = recovery_record();
        record.fail(-32603, message);
        persist(&workspace, &record).expect("bounded errors must remain persistable");
        let loaded = load(&workspace, &record.task_id).unwrap().unwrap();
        assert_eq!(loaded.status, TaskStatus::Failed);
        assert!(loaded.status_message.len() <= 2_000);
        assert!(loaded.error.unwrap()["message"].as_str().unwrap().len() <= 2_000);
        assert!(!loaded.status_message.is_empty());
    }
}

#[test]
fn durable_recovery_oversized_records_do_not_consume_store_capacity() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let mut record = recovery_record();
    record.complete(json!({"payload": "x".repeat(MAX_TASK_RECORD_BYTES as usize)}));
    assert!(persist(&workspace, &record).is_err());
    assert!(!task_directory(&workspace, &record.task_id)
        .unwrap()
        .exists());
}

#[test]
fn durable_recovery_corrupt_existing_snapshot_is_not_acknowledged() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let record = recovery_record();
    persist(&workspace, &record).unwrap();
    let directory = task_directory(&workspace, &record.task_id).unwrap();
    let path = snapshot_paths(&directory).unwrap().pop().unwrap();
    fs::write(&path, b"{\"schema_version\":").unwrap();
    assert!(persist(&workspace, &record).is_err());
    assert!(load(&workspace, &record.task_id).unwrap().is_none());
}

#[test]
fn durable_recovery_snapshot_retry_is_idempotent_without_temporary_files() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let mut record = recovery_record();
    persist(&workspace, &record).unwrap();
    persist(&workspace, &record).unwrap();
    record.cancel();
    persist(&workspace, &record).unwrap();
    let directory = task_directory(&workspace, &record.task_id).unwrap();
    assert_eq!(fs::read_dir(directory).unwrap().count(), 2);
    assert_eq!(
        load(&workspace, &record.task_id).unwrap().unwrap().status,
        TaskStatus::Cancelled
    );
}

#[cfg(unix)]
#[test]
fn durable_recovery_snapshot_symlinks_are_never_acknowledged() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let record = recovery_record();
    persist(&workspace, &record).unwrap();
    let directory = task_directory(&workspace, &record.task_id).unwrap();
    let path = snapshot_paths(&directory).unwrap().pop().unwrap();
    let original = fs::read(&path).unwrap();
    let outside = root.path().join("untouched.json");
    fs::write(&outside, &original).unwrap();
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    assert!(persist(&workspace, &record).is_err());
    assert_eq!(fs::read(&outside).unwrap(), original);
}

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
