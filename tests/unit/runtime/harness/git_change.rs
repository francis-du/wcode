use super::*;
use crate::verification::change::GitChangeAuthority;
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn commit(root: &Path, message: &str) {
    git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "--quiet",
            "--no-verify",
            "--allow-empty",
            "-m",
            message,
        ],
    );
}

fn fixture() -> (tempfile::TempDir, Workspace, ToolHarness) {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "--quiet"]);
    std::fs::write(root.path().join("README.md"), "# Fixture\n").unwrap();
    std::fs::write(root.path().join("main.rs"), "fn main() {}\n").unwrap();
    git(root.path(), &["add", "."]);
    commit(root.path(), "base");
    let workspace = Workspace::new(root.path(), false, true).unwrap();
    (root, workspace, ToolHarness::new(2).unwrap())
}

#[tokio::test]
async fn git_execution_binding_tracks_same_bytes_commits_dirty_and_stage() {
    let (root, workspace, harness) = fixture();
    let first = harness
        .execution_git_binding(&workspace)
        .await
        .unwrap()
        .unwrap();
    assert!(first.valid());
    assert!(!first.dirty);
    assert_eq!(first.head_sha, git(root.path(), &["rev-parse", "HEAD"]));
    assert_eq!(
        first.tree_sha,
        git(root.path(), &["rev-parse", "HEAD^{tree}"])
    );
    commit(root.path(), "same bytes new commit");
    let second = harness
        .execution_git_binding(&workspace)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(first.head_sha, second.head_sha);
    assert_eq!(first.tree_sha, second.tree_sha);
    assert_eq!(first.repository, second.repository);
    assert_eq!(first.index_fingerprint, second.index_fingerprint);
    let error = harness
        .ensure_execution_git_binding(&workspace, &Some(first))
        .await
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("results are stale, no evidence was recorded"));
    std::fs::write(root.path().join("README.md"), "# Changed\n").unwrap();
    let dirty = harness
        .execution_git_binding(&workspace)
        .await
        .unwrap()
        .unwrap();
    assert!(dirty.dirty);
    assert_eq!(dirty.index_fingerprint, second.index_fingerprint);
    git(root.path(), &["add", "README.md"]);
    let staged = harness
        .execution_git_binding(&workspace)
        .await
        .unwrap()
        .unwrap();
    assert!(staged.dirty);
    assert_ne!(staged.index_fingerprint, dirty.index_fingerprint);
    assert_eq!(staged.head_sha, second.head_sha);
    harness
        .ensure_execution_git_binding(&workspace, &Some(staged.clone()))
        .await
        .unwrap();

    let mut value = serde_json::to_value(&staged).unwrap();
    value["trusted"] = json!(true);
    assert!(serde_json::from_value::<ExecutionGitBinding>(value).is_err());
    let mut invalid = staged;
    invalid.head_sha = "HEAD".into();
    assert!(!invalid.valid());
}

#[tokio::test]
async fn git_change_snapshot_preserves_clean_commit_rename_and_modes() {
    let (root, workspace, harness) = fixture();
    let base = git(root.path(), &["rev-parse", "HEAD"]);
    git(root.path(), &["mv", "main.rs", "renamed 源.rs"]);
    std::fs::write(root.path().join("README.md"), "# Changed\n").unwrap();
    git(root.path(), &["add", "."]);
    commit(root.path(), "change");
    let head = git(root.path(), &["rev-parse", "HEAD"]);
    let snapshot = harness
        .git_change_snapshot(
            &workspace,
            &base,
            GitChangeTarget::Commit {
                revision: head.clone(),
            },
        )
        .await
        .unwrap();
    assert!(snapshot.complete, "{snapshot:?}");
    assert_eq!(snapshot.base_sha.as_deref(), Some(base.as_str()));
    assert_eq!(snapshot.target_sha.as_deref(), Some(head.as_str()));
    assert_eq!(snapshot.authority, GitChangeAuthority::MetadataOnly);
    assert!(snapshot.unknown_reasons.is_empty());
    assert_eq!(snapshot.regular_file_changes, Some(true));
    assert_eq!(snapshot.executable_changes, Some(false));
    let renamed = snapshot
        .changes
        .iter()
        .find(|row| row.status.starts_with('R'))
        .unwrap();
    assert_eq!(renamed.old_path.as_deref(), Some("main.rs"));
    assert_eq!(renamed.new_path.as_deref(), Some("renamed 源.rs"));
    assert_eq!(renamed.old_mode.as_deref(), Some("100644"));
    assert_eq!(renamed.new_mode.as_deref(), Some("100644"));

    // Exercise porcelain's opposite rename ordering through real Git output.
    git(root.path(), &["mv", "renamed 源.rs", "another name.rs"]);
    let output = workspace
        .change_probe(&[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            ".",
        ])
        .await
        .unwrap();
    let status = parse_status_changes(&complete_git_output(output).unwrap(), 500).unwrap();
    let renamed = status.iter().find(|row| row.status.contains('R')).unwrap();
    assert_eq!(renamed.old_path.as_deref(), Some("renamed 源.rs"));
    assert_eq!(renamed.new_path.as_deref(), Some("another name.rs"));
}

#[tokio::test]
async fn git_change_snapshot_rejects_dirty_commit_nonhead_and_unknown_untracked_mode() {
    let (root, workspace, harness) = fixture();
    let base = git(root.path(), &["rev-parse", "HEAD"]);
    commit(root.path(), "new head");
    let old_target = harness
        .git_change_snapshot(
            &workspace,
            &base,
            GitChangeTarget::Commit {
                revision: base.clone(),
            },
        )
        .await
        .unwrap();
    assert!(!old_target.complete);
    assert!(old_target
        .unknown_reasons
        .iter()
        .any(|reason| reason == "target_not_current_head"));
    std::fs::write(root.path().join("README.md"), "# Dirty\n").unwrap();
    let dirty = harness
        .git_change_snapshot(
            &workspace,
            &base,
            GitChangeTarget::Commit {
                revision: "HEAD".into(),
            },
        )
        .await
        .unwrap();
    assert!(!dirty.complete);
    assert!(dirty
        .unknown_reasons
        .iter()
        .any(|reason| reason == "dirty_commit_candidate"));
    let working = harness
        .git_change_snapshot(&workspace, &base, GitChangeTarget::Worktree)
        .await
        .unwrap();
    assert!(working.complete, "{working:?}");
    assert!(working
        .changes
        .iter()
        .any(|row| row.new_path.as_deref() == Some("README.md")));
    std::fs::write(root.path().join("new docs.md"), "untracked\n").unwrap();
    let untracked = harness
        .git_change_snapshot(&workspace, &base, GitChangeTarget::Worktree)
        .await
        .unwrap();
    assert!(!untracked.complete);
    assert_eq!(untracked.regular_file_changes, None);
    assert_eq!(untracked.executable_changes, None);
    assert!(untracked
        .unknown_reasons
        .iter()
        .any(|reason| reason == "file_mode_unknown"));
    assert!(untracked.changes.iter().any(|row| row.status == "??"
        && row.old_path.is_none()
        && row.new_path.as_deref() == Some("new docs.md")));
    for invalid in ["HEAD~1", "--all", "abc123", "refs/heads/main"] {
        assert!(harness
            .git_change_snapshot(&workspace, invalid, GitChangeTarget::Worktree)
            .await
            .is_err());
    }
}

#[tokio::test]
async fn git_execution_binding_keeps_subworkspace_scope_and_linked_worktree_identity() {
    let (root, workspace, harness) = fixture();
    std::fs::create_dir(root.path().join("sub")).unwrap();
    std::fs::write(root.path().join("sub/value.rs"), "pub fn value() {}\n").unwrap();
    git(root.path(), &["add", "."]);
    commit(root.path(), "scope");
    let sub = Workspace::new(root.path().join("sub"), false, true).unwrap();
    let parent_binding = harness
        .execution_git_binding(&workspace)
        .await
        .unwrap()
        .unwrap();
    let sub_binding = harness.execution_git_binding(&sub).await.unwrap().unwrap();
    assert_eq!(parent_binding.head_sha, sub_binding.head_sha);
    assert_ne!(parent_binding.repository, sub_binding.repository);
    assert_ne!(
        parent_binding.index_fingerprint,
        sub_binding.index_fingerprint
    );
    std::fs::write(root.path().join("README.md"), "# Outside sub scope\n").unwrap();
    assert_eq!(
        harness.execution_git_binding(&sub).await.unwrap().unwrap(),
        sub_binding
    );
    assert!(
        harness
            .execution_git_binding(&workspace)
            .await
            .unwrap()
            .unwrap()
            .dirty
    );
    let candidate = harness
        .git_change_snapshot(&sub, "HEAD", GitChangeTarget::Worktree)
        .await
        .unwrap();
    assert!(!candidate.complete);
    assert!(candidate
        .unknown_reasons
        .iter()
        .any(|reason| reason == "workspace_not_git_root"));

    let linked_parent = tempfile::tempdir().unwrap();
    let linked_path = linked_parent.path().join("linked");
    git(
        root.path(),
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            linked_path.to_str().unwrap(),
            "HEAD",
        ],
    );
    assert!(linked_path.join(".git").is_file());
    let linked = Workspace::new(&linked_path, false, true).unwrap();
    let binding = harness
        .execution_git_binding(&linked)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(binding.head_sha, parent_binding.head_sha);
    assert!(!binding.dirty);
    assert_ne!(binding.repository, parent_binding.repository);
    assert!(
        harness
            .git_change_snapshot(
                &linked,
                "HEAD",
                GitChangeTarget::Commit {
                    revision: "HEAD".into()
                }
            )
            .await
            .unwrap()
            .complete
    );
}

#[tokio::test]
async fn git_execution_binding_distinguishes_non_git_denied_unborn_and_broken_git() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, true).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    assert!(harness
        .execution_git_binding(&workspace)
        .await
        .unwrap()
        .is_none());
    let non_git = harness
        .git_change_snapshot(&workspace, "HEAD", GitChangeTarget::Worktree)
        .await
        .unwrap();
    assert!(!non_git.complete);
    assert_eq!(non_git.unknown_reasons, ["not_git"]);
    let denied = Workspace::new(root.path(), false, false).unwrap();
    assert!(harness.execution_git_binding(&denied).await.is_err());
    assert!(
        !harness
            .git_change_snapshot(&denied, "HEAD", GitChangeTarget::Worktree)
            .await
            .unwrap()
            .complete
    );
    git(root.path(), &["init", "--quiet"]);
    assert!(harness.execution_git_binding(&workspace).await.is_err());
    let broken = tempfile::tempdir().unwrap();
    std::fs::write(
        broken.path().join(".git"),
        "gitdir: unavailable-repository\n",
    )
    .unwrap();
    let broken = Workspace::new(broken.path(), false, true).unwrap();
    assert!(harness.execution_git_binding(&broken).await.is_err());
}

#[tokio::test]
async fn git_change_snapshot_preserves_executable_and_nonregular_commit_modes() {
    let (root, workspace, harness) = fixture();
    let base = git(root.path(), &["rev-parse", "HEAD"]);
    git(root.path(), &["config", "core.fileMode", "false"]);
    git(root.path(), &["update-index", "--chmod=+x", "README.md"]);
    commit(root.path(), "executable docs");
    let executable = harness
        .git_change_snapshot(
            &workspace,
            &base,
            GitChangeTarget::Commit {
                revision: "HEAD".into(),
            },
        )
        .await
        .unwrap();
    assert!(executable.complete, "{executable:?}");
    assert_eq!(executable.executable_changes, Some(true));
    assert_eq!(executable.regular_file_changes, Some(true));
    assert!(executable
        .changes
        .iter()
        .any(|row| row.old_mode.as_deref() == Some("100644")
            && row.new_mode.as_deref() == Some("100755")));

    let link_base = git(root.path(), &["rev-parse", "HEAD"]);
    let nested = root.path().join("dependency.md");
    std::fs::create_dir(&nested).unwrap();
    git(&nested, &["init", "--quiet"]);
    std::fs::write(nested.join("value.txt"), "nested\n").unwrap();
    git(&nested, &["add", "."]);
    commit(&nested, "gitlink");
    git(root.path(), &["add", "dependency.md"]);
    commit(root.path(), "nonregular docs suffix");
    let gitlink = harness
        .git_change_snapshot(
            &workspace,
            &link_base,
            GitChangeTarget::Commit {
                revision: "HEAD".into(),
            },
        )
        .await
        .unwrap();
    assert!(gitlink.complete, "{gitlink:?}");
    assert_eq!(gitlink.regular_file_changes, Some(false));
    assert_eq!(gitlink.executable_changes, Some(false));
    assert!(gitlink
        .changes
        .iter()
        .any(|row| row.new_mode.as_deref() == Some("160000")));
    #[cfg(unix)]
    {
        let before = git(root.path(), &["rev-parse", "HEAD"]);
        std::os::unix::fs::symlink("README.md", root.path().join("symlink.md")).unwrap();
        git(root.path(), &["add", "symlink.md"]);
        commit(root.path(), "symlink docs suffix");
        let symlink = harness
            .git_change_snapshot(
                &workspace,
                &before,
                GitChangeTarget::Commit {
                    revision: "HEAD".into(),
                },
            )
            .await
            .unwrap();
        assert!(symlink.complete, "{symlink:?}");
        assert_eq!(symlink.regular_file_changes, Some(false));
        assert_eq!(symlink.executable_changes, Some(false));
        assert!(symlink
            .changes
            .iter()
            .any(|row| row.new_mode.as_deref() == Some("120000")));
    }
}

#[tokio::test]
async fn git_metadata_redaction_truncation_and_path_cap_never_mean_complete() {
    let (root, workspace, harness) = fixture();
    std::fs::write(root.path().join("password=synthetic-value"), "fixture\n").unwrap();
    assert!(harness.execution_git_binding(&workspace).await.is_err());
    let redacted = harness
        .git_change_snapshot(&workspace, "HEAD", GitChangeTarget::Worktree)
        .await
        .unwrap();
    assert!(!redacted.complete);
    assert!(redacted.binding.is_none());
    std::fs::remove_file(root.path().join("password=synthetic-value")).unwrap();
    for index in 0..501 {
        std::fs::write(root.path().join(format!("change-{index}.md")), "fixture\n").unwrap();
    }
    let capped = harness
        .git_change_snapshot(&workspace, "HEAD", GitChangeTarget::Worktree)
        .await
        .unwrap();
    assert!(!capped.complete);
    assert!(capped.changes.len() <= 500);
    // Force actual native output capture truncation rather than mocking flags.
    for index in 0..1_600 {
        std::fs::write(
            root.path()
                .join(format!("long-{index}-{}.md", "x".repeat(180))),
            "fixture\n",
        )
        .unwrap();
    }
    assert!(harness.execution_git_binding(&workspace).await.is_err());
    let truncated = harness
        .git_change_snapshot(&workspace, "HEAD", GitChangeTarget::Worktree)
        .await
        .unwrap();
    assert!(!truncated.complete);
    assert_eq!(truncated.regular_file_changes, None);
}
