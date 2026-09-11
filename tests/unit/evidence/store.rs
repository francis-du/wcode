use super::*;
use crate::evidence::{Confidence, EvidenceKind, EvidenceResult, Revision};

#[test]
fn evidence_signal_detects_late_records_and_removal_without_loading_json() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let empty = change_fingerprint(&workspace).unwrap();
    let mut record = Evidence::new(
        "EV-newer".into(),
        "check".into(),
        EvidenceKind::UnitTest,
        "test-runner".into(),
        Revision {
            code: "code:1".into(),
            design: None,
        },
        EvidenceResult::Pass,
        Confidence::Deterministic,
    )
    .unwrap();
    record.timestamp_ms = 100;
    persist(&workspace, &record).unwrap();
    LOAD_CALLS.with(|count| count.set(0));
    let before = change_fingerprint(&workspace).unwrap();
    assert_ne!(empty, before);
    assert_eq!(before, change_fingerprint(&workspace).unwrap());
    record.id = "EV-late".into();
    record.timestamp_ms = 1;
    persist(&workspace, &record).unwrap();
    let late = change_fingerprint(&workspace).unwrap();
    assert_ne!(before, late, "a backdated arrival still changes the set");
    assert_eq!(LOAD_CALLS.with(|count| count.get()), 0);
    let paths = evidence_paths(&evidence_directory(&workspace).unwrap()).unwrap();
    fs::remove_file(&paths[0]).unwrap();
    assert_ne!(late, change_fingerprint(&workspace).unwrap());
}

#[cfg(unix)]
#[test]
fn evidence_signal_rejects_symlink_records_without_following_them() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let directory = evidence_directory(&workspace).unwrap();
    fs::create_dir_all(&directory).unwrap();
    let destination = root.path().join("fixture.txt");
    fs::write(&destination, "not evidence").unwrap();
    std::os::unix::fs::symlink(&destination, directory.join("linked.json")).unwrap();
    assert!(change_fingerprint(&workspace).is_err());
    assert_eq!(fs::read_to_string(destination).unwrap(), "not evidence");
}

#[test]
fn evidence_survives_a_fresh_load_and_is_workspace_scoped() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let first_workspace = Workspace::new(first.path(), false, false).unwrap();
    let second_workspace = Workspace::new(second.path(), false, false).unwrap();
    let evidence = Evidence::new(
        "EV-PERSIST-1".into(),
        "REQ-1".into(),
        EvidenceKind::UnitTest,
        "cargo-test".into(),
        Revision {
            design: None,
            code: "sha256:fixture".into(),
        },
        EvidenceResult::Pass,
        Confidence::Deterministic,
    )
    .unwrap();

    persist(&first_workspace, &evidence).unwrap();
    assert_eq!(load(&first_workspace).unwrap(), vec![evidence]);
    assert!(load(&second_workspace).unwrap().is_empty());
}
