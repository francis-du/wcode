use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn project_snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn visit(root: &Path, at: &Path, entries: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        for entry in std::fs::read_dir(at).unwrap() {
            let path = entry.unwrap().path();
            let kind = std::fs::symlink_metadata(&path).unwrap().file_type();
            assert!(!kind.is_symlink(), "fixture snapshot must not follow links");
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            if kind.is_dir() {
                entries.insert(relative, None);
                visit(root, &path, entries);
            } else {
                entries.insert(relative, Some(std::fs::read(path).unwrap()));
            }
        }
    }
    let mut entries = BTreeMap::new();
    visit(root, root, &mut entries);
    entries
}

fn enrollment_command(
    root: &Path,
    repository: &str,
    id: &str,
    check: &str,
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_wcode"))
        .current_dir(root)
        .args([
            "setup",
            "--project",
            "--json",
            "--github-repository",
            repository,
            "--github-repository-id",
            id,
            "--github-app-id",
            "456",
            "--github-check-name",
            check,
        ])
        .output()
        .unwrap()
}

#[test]
fn setup_invalid_github_identity_does_not_write_host_or_design_configuration() {
    let mut violations = Vec::new();
    for (repository, id, check) in [
        ("owner", "123", "gate"),
        ("owner/repo", "0", "gate"),
        ("owner/repo", "123", "   "),
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("README.md"), "# Keep existing content\n").unwrap();
        let before = project_snapshot(root.path());
        let output = enrollment_command(root.path(), repository, id, check);
        if output.status.success() || project_snapshot(root.path()) != before {
            violations.push(format!(
                "{repository:?}/{id}/{check:?}: status={}",
                output.status
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "invalid setup modified project: {violations:?}"
    );
}

#[test]
fn setup_conflicting_or_malformed_enrollment_preserves_all_project_files() {
    for enrollment in [
        "schema_version: 1\nrepository: owner/original\nrepository_id: 42\napp_id: 456\ncheck_name: gate\n",
        "schema_version: [invalid\n",
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".wcode")).unwrap();
        std::fs::write(root.path().join(".wcode/github-publisher.yaml"), enrollment).unwrap();
        let before = project_snapshot(root.path());
        let output = enrollment_command(root.path(), "owner/replacement", "123", "gate");
        assert!(!output.status.success());
        assert!(
            project_snapshot(root.path()) == before,
            "rejected setup wrote configuration"
        );
    }
}

#[test]
fn setup_project_github_enrollment_is_dry_run_safe_and_credential_free() {
    let root = tempfile::tempdir().unwrap();
    let enrollment_args = [
        "setup",
        "--project",
        "--json",
        "--github-repository",
        "Owner/Repo",
        "--github-repository-id",
        "123",
        "--github-app-id",
        "456",
    ];

    let preview = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .current_dir(root.path())
        .args(enrollment_args)
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let preview_json: Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(
        preview_json["github_publisher_enrollment"]["status"],
        "planned"
    );
    assert_eq!(
        preview_json["github_publisher_enrollment"]["credential_stored"],
        false
    );
    assert_eq!(
        preview_json["github_publisher_enrollment"]["remote_changes"],
        false
    );
    assert!(!root.path().join(".wcode/github-publisher.yaml").exists());

    let applied = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .current_dir(root.path())
        .args(enrollment_args)
        .output()
        .unwrap();
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    let applied_json: Value = serde_json::from_slice(&applied.stdout).unwrap();
    assert_eq!(
        applied_json["github_publisher_enrollment"]["status"],
        "enrolled"
    );
    assert_eq!(
        applied_json["github_publisher_enrollment"]["enrollment"]["repository"],
        "owner/repo"
    );
    let enrollment =
        std::fs::read_to_string(root.path().join(".wcode/github-publisher.yaml")).unwrap();
    assert!(!enrollment.contains("token"));
    assert!(!enrollment.contains("credential"));

    let repeated = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .current_dir(root.path())
        .args(enrollment_args)
        .output()
        .unwrap();
    assert!(
        repeated.status.success(),
        "{}",
        String::from_utf8_lossy(&repeated.stderr)
    );
    let repeated_json: Value = serde_json::from_slice(&repeated.stdout).unwrap();
    assert_eq!(
        repeated_json["github_publisher_enrollment"]["status"],
        "existing"
    );
}

#[test]
fn setup_github_enrollment_rejects_partial_and_conflicting_identity() {
    let root = tempfile::tempdir().unwrap();
    let partial = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .current_dir(root.path())
        .args([
            "setup",
            "--project",
            "--json",
            "--github-repository",
            "owner/repo",
            "--github-app-id",
            "456",
        ])
        .output()
        .unwrap();
    assert!(!partial.status.success());
    assert!(!root.path().join(".wcode/github-publisher.yaml").exists());

    let enrolled = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .current_dir(root.path())
        .args([
            "setup",
            "--project",
            "--json",
            "--github-repository",
            "owner/repo",
            "--github-repository-id",
            "123",
            "--github-app-id",
            "456",
        ])
        .output()
        .unwrap();
    assert!(enrolled.status.success());

    let before = std::fs::read_to_string(root.path().join(".wcode/github-publisher.yaml")).unwrap();
    let conflict = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .current_dir(root.path())
        .args([
            "setup",
            "--project",
            "--json",
            "--github-repository",
            "owner/other",
            "--github-repository-id",
            "999",
            "--github-app-id",
            "456",
        ])
        .output()
        .unwrap();
    assert!(!conflict.status.success());
    assert_eq!(
        std::fs::read_to_string(root.path().join(".wcode/github-publisher.yaml")).unwrap(),
        before
    );
}
