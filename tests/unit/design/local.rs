use super::*;
use std::fs;

#[test]
fn local_design_port_reads_canonical_metadata_without_writes_or_authority() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".wcode")).unwrap();
    let file = root.path().join(".wcode/project.yaml");
    fs::write(&file, "schema_version: 1\nname: Local Design\n").unwrap();
    let before = fs::read(&file).unwrap();
    let loaded = load_local_design(root.path()).unwrap();
    assert_eq!(loaded.error_count(), 0);
    assert_eq!(loaded.state.project.as_ref().unwrap().name, "Local Design");
    assert_eq!(fs::read(&file).unwrap(), before);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    assert_eq!(fs::read_dir(root.path().join(".wcode")).unwrap().count(), 1);
    fs::write(&file, "schema_version: [malformed").unwrap();
    let loaded = load_local_design(root.path()).unwrap();
    assert!(loaded.error_count() > 0);
    assert!(loaded.state.project.is_none());
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "schema_version: [malformed"
    );
}

#[cfg(unix)]
#[test]
fn local_design_port_preserves_symlink_rejection_without_reading_the_target() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    fs::write(
        other.path().join("project.yaml"),
        "schema_version: 1\nname: PRIVATE-TARGET\n",
    )
    .unwrap();
    fs::create_dir(root.path().join(".wcode")).unwrap();
    symlink(
        other.path().join("project.yaml"),
        root.path().join(".wcode/project.yaml"),
    )
    .unwrap();
    let loaded = load_local_design(root.path()).unwrap();
    assert!(loaded.state.project.is_none());
    assert!(loaded.error_count() > 0);
    assert!(!serde_json::to_string(&loaded)
        .unwrap()
        .contains("PRIVATE-TARGET"));
}
