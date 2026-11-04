use super::*;
use crate::workspace::Workspace;
use std::fs;
use std::path::Path;

fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn changed_file(path: &str) -> ChangedFileReview {
    ChangedFileReview {
        path: path.to_owned(),
        status: " M".to_owned(),
        staged: false,
        unstaged: true,
        untracked: false,
        category: "source".to_owned(),
        additions: Some(1),
        deletions: Some(1),
        binary: false,
        risk_reasons: Vec::new(),
    }
}

#[tokio::test]
async fn file_cap_does_not_mark_a_complete_early_file_truncated() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-q"]);
    git(root.path(), &["config", "user.email", "wcode@example.test"]);
    git(root.path(), &["config", "user.name", "wcode test"]);
    fs::create_dir_all(root.path().join("src")).unwrap();
    let mut paths = Vec::new();
    for index in 0..=500 {
        let path = format!("src/file{index:03}.rs");
        fs::write(root.path().join(&path), "pub fn value() -> u32 { 0 }\n").unwrap();
        paths.push(path);
    }
    git(root.path(), &["add", "."]);
    git(
        root.path(),
        &["-c", "commit.gpgsign=false", "commit", "-qm", "initial"],
    );
    fs::write(root.path().join(&paths[0]), "pub fn value() -> u32 { 1 }\n").unwrap();

    let workspace = Workspace::new(root.path(), false, true).unwrap();
    let snapshot = workspace.changed_line_ranges(&paths).await.unwrap();
    let early = changed_file(&paths[0]);
    let (before, after, truncated) = change_ranges_view(&early, Some(&snapshot));
    assert_eq!(
        before,
        Some(vec![crate::workspace::ChangeLineRange {
            start_line: 1,
            end_line: 1
        }])
    );
    assert_eq!(
        after,
        Some(vec![crate::workspace::ChangeLineRange {
            start_line: 1,
            end_line: 1
        }])
    );
    assert!(
        !truncated,
        "reaching the 500-file probe cap must not mark a complete early file truncated"
    );
    assert!(!snapshot.files[&paths[0]].truncated);
}
