use super::*;

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

fn fixture() -> (tempfile::TempDir, Workspace) {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-q"]);
    fs::write(root.path().join("main.rs"), "fn before() {}\n").unwrap();
    git(root.path(), &["add", "main.rs"]);
    git(
        root.path(),
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "-qm",
            "fixture",
        ],
    );
    let workspace = Workspace::new(root.path(), false, true).unwrap();
    (root, workspace)
}

#[tokio::test]
async fn change_view_separates_staged_unstaged_and_combined_without_writes() {
    let (root, workspace) = fixture();
    fs::write(root.path().join("main.rs"), "fn staged() {}\n").unwrap();
    git(root.path(), &["add", "main.rs"]);
    fs::write(root.path().join("main.rs"), "fn working() {}\n").unwrap();
    let staged = workspace
        .change_view("main.rs", ChangeLayer::Staged, None)
        .await
        .unwrap();
    assert!(staged.content.contains("-fn before()"));
    assert!(staged.content.contains("+fn staged()"));
    assert_eq!(
        staged.before_changed_ranges,
        vec![ChangeLineRange {
            start_line: 1,
            end_line: 1
        }]
    );
    assert_eq!(
        staged.after_changed_ranges,
        vec![ChangeLineRange {
            start_line: 1,
            end_line: 1
        }]
    );
    assert!(!staged.content.contains("working()"));
    assert!(!staged.after_source_matches_worktree);
    let staged_source = staged
        .after_source
        .as_ref()
        .expect("staged comparison must retain the exact index source for syntax mapping");
    assert_eq!(staged_source.content, "fn staged() {}\n");
    assert_ne!(
        Some(staged_source.sha256.as_str()),
        staged.worktree_sha256.as_deref(),
        "index source identity must stay distinct from a newer worktree"
    );
    let unstaged = workspace
        .change_view("main.rs", ChangeLayer::Unstaged, Some(&staged.snapshot_id))
        .await
        .unwrap();
    assert!(unstaged.content.contains("-fn staged()"));
    assert!(unstaged.content.contains("+fn working()"));
    assert!(unstaged.after_source_matches_worktree);
    let combined = workspace
        .change_view("main.rs", ChangeLayer::Working, Some(&staged.snapshot_id))
        .await
        .unwrap();
    assert!(combined.content.contains("-fn before()"));
    assert!(combined.content.contains("+fn working()"));
    assert!(combined.after_source_matches_worktree);
    assert!(!combined.truncated);
    assert_eq!(staged.snapshot_id, unstaged.snapshot_id);
    assert_eq!(
        fs::read_to_string(root.path().join("main.rs")).unwrap(),
        "fn working() {}\n"
    );
    assert!(!workspace.write_enabled());
}

#[test]
fn staged_after_source_identity_requires_a_clean_worktree_column() {
    assert!(staged_after_matches_worktree("M  main.rs\n"));
    assert!(staged_after_matches_worktree("A  new.rs\n"));
    assert!(!staged_after_matches_worktree("MM main.rs\n"));
    assert!(!staged_after_matches_worktree(" M main.rs\n"));
    assert!(!staged_after_matches_worktree("?? new.rs\n"));
}

#[test]
fn change_ranges_track_modified_lines_without_promoting_context() {
    let diff = "diff --git a/main.rs b/main.rs\n--- a/main.rs\n+++ b/main.rs\n@@ -2,3 +2,4 @@\n keep\n-old2\n+new2\n+insert\n keep\n@@ -10,2 +11,1 @@\n-delete_a\n-delete_b\n+replacement\n";
    let (before, after, capped) = unified_changed_ranges(diff);
    assert_eq!(
        before,
        vec![
            ChangeLineRange {
                start_line: 3,
                end_line: 3
            },
            ChangeLineRange {
                start_line: 10,
                end_line: 11
            },
        ]
    );
    assert_eq!(
        after,
        vec![
            ChangeLineRange {
                start_line: 3,
                end_line: 4
            },
            ChangeLineRange {
                start_line: 11,
                end_line: 11
            },
        ]
    );
    assert!(!capped);

    let (before, after, capped) = unified_changed_ranges("@@ -0,0 +1,2 @@\n+one\n+two\n");
    assert!(before.is_empty());
    assert_eq!(
        after,
        vec![ChangeLineRange {
            start_line: 1,
            end_line: 2
        }]
    );
    assert!(!capped);
}

#[tokio::test]
async fn change_view_rejects_stale_worktree_and_index_snapshots() {
    let (root, workspace) = fixture();
    let first = workspace
        .change_view("main.rs", ChangeLayer::Working, None)
        .await
        .unwrap();
    fs::write(root.path().join("main.rs"), "fn changed() {}\n").unwrap();
    assert!(workspace
        .change_view("main.rs", ChangeLayer::Working, Some(&first.snapshot_id))
        .await
        .unwrap_err()
        .to_string()
        .contains("snapshot changed"));
    let second = workspace
        .change_view("main.rs", ChangeLayer::Working, None)
        .await
        .unwrap();
    git(root.path(), &["add", "main.rs"]);
    assert!(workspace
        .change_view("main.rs", ChangeLayer::Working, Some(&second.snapshot_id))
        .await
        .unwrap_err()
        .to_string()
        .contains("snapshot changed"));
}

#[tokio::test]
async fn change_view_reads_deleted_file_diff_and_untracked_source_explicitly() {
    let (root, workspace) = fixture();
    fs::remove_file(root.path().join("main.rs")).unwrap();
    let deleted = workspace
        .change_view("main.rs", ChangeLayer::Working, None)
        .await
        .unwrap();
    assert!(deleted.content.contains("-fn before()"));
    assert_eq!(
        deleted.before_changed_ranges,
        vec![ChangeLineRange {
            start_line: 1,
            end_line: 1
        }]
    );
    assert!(deleted.after_changed_ranges.is_empty());
    assert!(deleted.worktree_sha256.is_none());
    fs::write(root.path().join("new.rs"), "fn new_source() {}\n").unwrap();
    let added = workspace
        .change_view("new.rs", ChangeLayer::Working, None)
        .await
        .unwrap();
    assert_eq!(added.kind, "untracked_source");
    assert_eq!(added.content, "fn new_source() {}");
    assert!(added.before_changed_ranges.is_empty());
    assert_eq!(
        added.after_changed_ranges,
        vec![ChangeLineRange {
            start_line: 1,
            end_line: 1
        }]
    );
    let staged = workspace
        .change_view("new.rs", ChangeLayer::Staged, None)
        .await
        .unwrap();
    assert_eq!(staged.kind, "unified_diff");
    assert!(
        staged.content.is_empty(),
        "untracked bytes must not be presented as staged"
    );
}

#[tokio::test]
async fn change_view_rejects_protected_traversal_directory_and_no_exec() {
    let (root, workspace) = fixture();
    for path in [
        ".",
        "../main.rs",
        ".git/config",
        ".env",
        ":(glob)**",
        "main.rs\nother",
    ] {
        assert!(
            workspace
                .change_view(path, ChangeLayer::Working, None)
                .await
                .is_err(),
            "{path}"
        );
    }
    let denied = Workspace::new(root.path(), false, false).unwrap();
    assert!(denied
        .change_view("main.rs", ChangeLayer::Working, None)
        .await
        .is_err());
}

#[tokio::test]
async fn change_view_literal_path_does_not_expand_wildcards() {
    let (root, workspace) = fixture();
    fs::write(root.path().join("main.rs"), "fn private_neighbor() {}\n").unwrap();
    // A missing literal path has no patch; it must not select all Rust files.
    let result = workspace
        .change_view("*.rs", ChangeLayer::Working, None)
        .await;
    if let Ok(view) = result {
        assert!(!view.content.contains("private_neighbor"));
    }
}

#[tokio::test]
async fn change_view_untracked_source_is_bounded_and_redacted() {
    let (root, workspace) = fixture();
    let secret_line = format!("{}={}\n", "API_TOKEN", "private-value-for-change-test");
    fs::write(
        root.path().join("new.txt"),
        format!("{secret_line}{}", "行\n".repeat(1400)),
    )
    .unwrap();
    let view = workspace
        .change_view("new.txt", ChangeLayer::Working, None)
        .await
        .unwrap();
    assert!(view.redacted && view.truncated);
    assert!(!view.content.contains("private-value-for-change-test"));
    assert!(view.content.len() <= MAX_CHANGE_VIEW_BYTES);
}

#[tokio::test]
async fn change_view_tracked_diff_redacts_secret_lines() {
    let (root, workspace) = fixture();
    let secret_line = format!("{}={}\n", "API_TOKEN", "private-patch-value");
    fs::write(
        root.path().join("main.rs"),
        format!("fn after() {{}}\n{secret_line}"),
    )
    .unwrap();
    let view = workspace
        .change_view("main.rs", ChangeLayer::Working, None)
        .await
        .unwrap();
    assert!(view.redacted);
    assert!(!view.content.contains("private-patch-value"));
    assert!(view.content.contains("fn after()"));
}

#[cfg(unix)]
#[tokio::test]
async fn change_view_disables_git_helpers_even_with_full_command_trust() {
    use std::os::unix::fs::PermissionsExt;
    let (root, mut workspace) = fixture();
    let helper = root.path().join("inspection-helper");
    fs::write(
        &helper,
        "#!/bin/sh\nprintf invoked > helper-invoked\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
    git(
        root.path(),
        &["config", "core.fsmonitor", helper.to_str().unwrap()],
    );
    git(
        root.path(),
        &[
            "config",
            "diff.inspection-probe.textconv",
            helper.to_str().unwrap(),
        ],
    );
    fs::write(
        root.path().join(".gitattributes"),
        "*.rs diff=inspection-probe\n",
    )
    .unwrap();
    fs::write(root.path().join("main.rs"), "fn after() {}\n").unwrap();
    workspace.security.allow_unrestricted_commands = true;
    let view = workspace
        .change_view("main.rs", ChangeLayer::Working, None)
        .await
        .unwrap();
    assert!(view.content.contains("-fn before()") && view.content.contains("+fn after()"));
    assert!(!root.path().join("helper-invoked").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn change_view_never_follows_symlinks() {
    let (root, workspace) = fixture();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("private.txt"), "outside-secret").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("private.txt"),
        root.path().join("link.txt"),
    )
    .unwrap();
    assert!(workspace
        .change_view("link.txt", ChangeLayer::Working, None)
        .await
        .is_err());
}
