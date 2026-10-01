use super::*;

fn fixture() -> (tempfile::TempDir, Workspace) {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    (root, workspace)
}

#[test]
fn task_listing_is_bounded_scoped_owner_filtered_and_metadata_only() {
    let (_root, workspace) = fixture();
    for index in 0..36 {
        let mut record = TaskRecord::working(
            "a".repeat(64),
            "selected".into(),
            "verify_project".into(),
            "runtime".into(),
        );
        record.created_at_ms += index;
        record.updated_at_ms = record.created_at_ms;
        record.complete(json!({"private":"DO_NOT_DISCOVER_PAYLOAD"}));
        persist(&workspace, &record).unwrap();
    }
    let listing = list_recent(
        &workspace,
        "selected",
        "verify_project",
        Some(&"a".repeat(64)),
    )
    .unwrap();
    assert_eq!(listing.records.len(), MAX_DISCOVERY_ITEMS);
    assert!(listing.truncated);
    assert!(listing
        .records
        .iter()
        .all(|record| record.result.is_none() && record.live_output.is_none()));
    assert!(list_recent(&workspace, "other", "verify_project", None)
        .unwrap()
        .records
        .is_empty());
    assert!(list_recent(
        &workspace,
        "selected",
        "verify_project",
        Some(&"b".repeat(64))
    )
    .unwrap()
    .records
    .is_empty());
    assert!(list_recent(&workspace, "selected", "run_command", None)
        .unwrap()
        .records
        .is_empty());
}

#[test]
fn task_listing_damaged_latest_is_partial_not_reused_success() {
    let (_root, workspace) = fixture();
    let mut record = TaskRecord::working(
        "a".repeat(64),
        "selected".into(),
        "run_command".into(),
        "runtime".into(),
    );
    record.complete(json!({"success":true}));
    persist(&workspace, &record).unwrap();
    let directory = task_directory(&workspace, &record.task_id).unwrap();
    fs::write(
        directory.join(format!("{}-{}.json", "9".repeat(20), "a".repeat(24))),
        "{broken",
    )
    .unwrap();
    assert!(load_for_observation(&workspace, &record.task_id).is_err());
    let listing = list_recent(&workspace, "selected", "run_command", None).unwrap();
    assert!(listing.records.is_empty());
    assert!(listing.truncated);
}

#[test]
fn task_listing_oversized_and_excess_scan_stay_bounded_without_deletion() {
    let (_root, workspace) = fixture();
    let record = TaskRecord::working(
        "a".repeat(64),
        "selected".into(),
        "run_command".into(),
        "runtime".into(),
    );
    persist(&workspace, &record).unwrap();
    let directory = task_directory(&workspace, &record.task_id).unwrap();
    for index in 0..MAX_TASK_SNAPSHOTS * 2 {
        fs::write(directory.join(format!(".pending-{index}")), "").unwrap();
    }
    assert!(load_for_observation(&workspace, &record.task_id).is_err());
    assert!(
        list_recent(&workspace, "selected", "run_command", None)
            .unwrap()
            .truncated
    );
    assert_eq!(
        fs::read_dir(directory).unwrap().count(),
        MAX_TASK_SNAPSHOTS * 2 + 1
    );
}

#[cfg(unix)]
#[test]
fn task_listing_rejects_symlink_and_hardlinked_snapshots() {
    use std::os::unix::fs::symlink;
    let (_root, workspace) = fixture();
    let record = TaskRecord::working(
        "a".repeat(64),
        "selected".into(),
        "run_command".into(),
        "runtime".into(),
    );
    persist(&workspace, &record).unwrap();
    let path = snapshot_paths(&task_directory(&workspace, &record.task_id).unwrap())
        .unwrap()
        .pop()
        .unwrap();
    let external = tempfile::tempdir().unwrap();
    fs::hard_link(&path, external.path().join("linked")).unwrap();
    assert!(load_for_observation(&workspace, &record.task_id).is_err());
    fs::remove_file(external.path().join("linked")).unwrap();
    let bytes = fs::read(&path).unwrap();
    fs::remove_file(&path).unwrap();
    fs::write(external.path().join("target"), bytes).unwrap();
    symlink(external.path().join("target"), &path).unwrap();
    assert!(load_for_observation(&workspace, &record.task_id).is_err());
    assert!(
        list_recent(&workspace, "selected", "run_command", None)
            .unwrap()
            .truncated
    );
}
