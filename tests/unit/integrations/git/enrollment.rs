use super::*;
use std::fs;

fn workspace(root: &Path, write: bool) -> Workspace {
    fs::create_dir_all(root.join(".wcode")).unwrap();
    Workspace::new(root, write, false).unwrap()
}

#[test]
fn github_enrollment_round_trips_without_credentials_or_remote_authority() {
    let root = tempfile::tempdir().unwrap();
    let workspace = workspace(root.path(), true);
    assert!(GitHubEnrollment::load_workspace(&workspace)
        .unwrap()
        .is_none());

    let enrollment = GitHubEnrollment::new("Francis-Du/WCode", 123, 456, None).unwrap();
    assert_eq!(enrollment.repository, "francis-du/wcode");
    assert_eq!(enrollment.check_name, "wcode/change-acceptance");
    enrollment.create_in_workspace(&workspace).unwrap();

    let loaded = GitHubEnrollment::load(root.path()).unwrap().unwrap();
    assert_eq!(loaded, enrollment);
    loaded.github_config().unwrap();

    let text = fs::read_to_string(root.path().join(GITHUB_ENROLLMENT_FILE)).unwrap();
    for forbidden in [
        "token",
        "credential",
        "remote_url",
        "private_key",
        "installation_token",
    ] {
        assert!(!text.contains(forbidden), "{forbidden} leaked: {text}");
    }
}

#[test]
fn github_enrollment_rejects_unknown_secret_fields_invalid_identity_and_overwrite() {
    for repository in [
        "owner",
        "owner/repo/extra",
        "https://github.com/owner/repo",
        "a@b/repo",
    ] {
        assert!(GitHubEnrollment::new(repository, 1, 2, None).is_err());
    }
    assert!(GitHubEnrollment::new("owner/repo", 0, 2, None).is_err());
    assert!(GitHubEnrollment::new("owner/repo", 1, 0, None).is_err());

    let root = tempfile::tempdir().unwrap();
    let workspace = workspace(root.path(), true);
    fs::write(
        root.path().join(GITHUB_ENROLLMENT_FILE),
        "schema_version: 1\nrepository: owner/repo\nrepository_id: 1\napp_id: 2\ncheck_name: gate\ntoken: SECRET\n",
    )
    .unwrap();
    assert!(GitHubEnrollment::load_workspace(&workspace).is_err());

    fs::remove_file(root.path().join(GITHUB_ENROLLMENT_FILE)).unwrap();
    let enrollment = GitHubEnrollment::new("owner/repo", 1, 2, None).unwrap();
    enrollment.create_in_workspace(&workspace).unwrap();
    assert!(enrollment.create_in_workspace(&workspace).is_err());
}

#[test]
fn github_enrollment_rejects_whitespace_check_names_and_invalid_parent() {
    assert!(GitHubEnrollment::new("owner/repo", 1, 2, Some("   ")).is_err());
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".wcode"), "not a configuration directory").unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    assert!(GitHubEnrollment::load_workspace(&workspace).is_err());
}

#[cfg(unix)]
#[test]
fn github_enrollment_missing_manifest_does_not_hide_symlinked_parent() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), root.path().join(".wcode")).unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    assert!(GitHubEnrollment::load_workspace(&workspace).is_err());
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn github_enrollment_rejects_symlinked_file() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    fs::create_dir_all(root.path().join(".wcode")).unwrap();
    symlink(outside.path(), root.path().join(GITHUB_ENROLLMENT_FILE)).unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    assert!(GitHubEnrollment::load_workspace(&workspace).is_err());
}
