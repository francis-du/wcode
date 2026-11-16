use super::test_support::*;
use super::*;
use crate::workspace::Workspace;
use std::fs;

#[test]
fn audit_git_literal_messages_remain_text_without_triggering_path_guards() {
    for command in [
        vec!["commit", "-m", "../migration note"],
        vec!["commit", "--message=.env"],
        vec!["commit", "-m", "/healthz endpoint"],
        vec!["tag", "-a", "v-test", "-m", ".env"],
    ] {
        assert!(validate_command_policy("git", &args(&command), hardened()).is_ok());
    }
    for command in [
        vec!["add", "--", "../outside"],
        vec!["commit", "--file", "../message"],
        vec!["tag", "-a", "v-test", "-m", "note", "../outside"],
    ] {
        assert!(validate_command_policy(
            "git",
            &args(&command),
            WorkspaceSecurity {
                allow_risky_exec: true,
                ..hardened()
            }
        )
        .is_err());
    }
}

#[test]
fn ordinary_git_lifecycle_is_autonomous_while_destructive_variants_stay_blocked() {
    for command in [
        vec!["branch", "feature/example"],
        vec!["branch", "feature/example", "HEAD"],
        vec!["switch", "feature/example"],
        vec!["switch", "-c", "feature/example"],
        vec!["tag", "v0.6.2"],
        vec!["tag", "-a", "v0.6.2", "-m", "release candidate"],
        vec!["restore", "--staged", "--", "src/lib.rs"],
    ] {
        assert!(
            validate_git_command(&args(&command), false).is_ok(),
            "{command:?}"
        );
    }
    for command in [
        vec!["branch"],
        vec!["branch", "--show-current"],
        vec!["tag", "--list"],
        vec!["branch", "--list", "feature/*"],
        vec!["remote", "-v"],
        vec!["remote", "get-url", "origin"],
    ] {
        assert!(
            validate_git_command(&args(&command), false).is_ok(),
            "{command:?}"
        );
    }
    for command in [
        vec!["tag", "-f", "v0.6.2"],
        vec!["branch", "-D", "main"],
        vec!["switch", "--discard-changes", "main"],
        vec!["tag", "-a", "v0.6.2"],
        vec!["restore", "src/lib.rs"],
        vec!["restore", "--staged", "--worktree", "src/lib.rs"],
    ] {
        assert!(
            validate_git_command(&args(&command), true).is_err(),
            "{command:?}"
        );
    }
}

#[test]
fn bounded_git_inspection_includes_repository_grep_without_helper_or_scope_escape() {
    let safe = hardened();
    for command in [
        vec!["grep", "-n", "needle", "--", "src", "tests"],
        vec!["grep", "-n", "-E", "foo|bar", "--", "src"],
    ] {
        assert!(
            validate_command_policy("git", &args(&command), safe).is_ok(),
            "bounded repository grep was rejected: {command:?}"
        );
    }
    for command in [
        vec!["grep", "--textconv", "needle"],
        vec!["grep", "--open-files-in-pager=less", "needle"],
        vec!["grep", "--no-index", "needle"],
        vec!["grep", "--untracked", "needle"],
        vec!["grep", "--recurse-submodules", "needle"],
    ] {
        assert!(
            validate_command_policy("git", &args(&command), safe).is_err(),
            "unsafe repository grep mode was accepted: {command:?}"
        );
    }
    assert!(validate_command_policy("git", &args(&["grep", "needle", "--", ".env"]), safe).is_ok());
}

#[tokio::test]
async fn invalid_commands_fail_without_creating_approval_requests() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    for (program, arguments, cwd) in [
        ("unlisted-example", vec!["../outside"], "."),
        ("unlisted-example", vec!["--version"], "missing"),
        ("git", vec!["branch", "feature"], "missing"),
    ] {
        assert!(workspace
            .run_command(program, &args(&arguments), cwd, 1)
            .await
            .is_err());
        assert!(workspace.authorization.requests(10).is_empty());
    }
    // A policy-valid command is no longer rejected before execution.
    let outcome = workspace
        .run_command("git", &args(&["branch", "-D", "main"]), ".", 1)
        .await
        .unwrap();
    assert!(!outcome.success);
    assert!(workspace.authorization.requests(10).is_empty());
}

#[tokio::test]
async fn bounded_git_commit_and_branch_run_without_authorization() {
    let root = tempfile::tempdir().unwrap();
    let git = |arguments: &[&str]| {
        std::process::Command::new("git")
            .args(arguments)
            .current_dir(root.path())
            .output()
            .unwrap()
            .status
            .success()
    };
    assert!(git(&["init", "-q"]));
    assert!(git(&["config", "user.name", "Fixture"]));
    assert!(git(&["config", "user.email", "fixture@example.test"]));
    assert!(git(&["config", "commit.gpgsign", "false"]));
    assert!(git(&["commit", "--allow-empty", "-m", "initial"]));
    fs::write(root.path().join("tracked.txt"), "autonomous\n").unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let add = workspace
        .run_command("git", &args(&["add", "--", "tracked.txt"]), ".", 10)
        .await
        .unwrap();
    assert!(add.success, "git add failed: {}", add.stderr);
    let commit = workspace
        .run_command(
            "git",
            &args(&["commit", "-m", "autonomous update"]),
            ".",
            10,
        )
        .await
        .unwrap();
    assert!(commit.success, "git commit failed: {}", commit.stderr);
    let branch = workspace
        .run_command("git", &args(&["branch", "autonomous-feature"]), ".", 10)
        .await
        .unwrap();
    assert!(branch.success, "git branch failed: {}", branch.stderr);
    assert!(workspace.authorization.requests(10).is_empty());
    assert!(git(&[
        "rev-parse",
        "--verify",
        "refs/heads/autonomous-feature"
    ]));
    assert!(workspace.security.allow_risky_exec);
}

#[test]
fn bounded_git_lifecycle_is_autonomous_and_keeps_hard_boundaries() {
    assert!(
        validate_git_command(&args(&["ls-remote", "origin", "refs/heads/main"]), false).is_ok()
    );
    assert!(validate_git_command(&args(&["fetch", "origin", "main"]), false).is_ok());
    assert!(validate_git_command(&args(&["pull", "--ff-only", "origin", "main"]), false).is_ok());
    for command in [
        vec![
            "ls-remote",
            "https://example.com/repository.git",
            "refs/heads/main",
        ],
        vec!["ls-remote", "origin", "refs/heads/*"],
        vec!["fetch", "origin", "--all"],
        vec!["fetch", "origin", "+main:main"],
        vec!["fetch", "origin", "../main"],
        vec!["fetch", "origin"],
    ] {
        assert!(
            validate_git_command(&args(&command), true).is_err(),
            "unsafe remote-read shape was accepted: {command:?}"
        );
    }
    assert!(command_requires_workspace_write(
        "git",
        &args(&["fetch", "origin", "main"])
    ));
    assert!(!command_requires_workspace_write(
        "git",
        &args(&["ls-remote", "origin", "refs/heads/main"])
    ));

    assert!(validate_git_command(&args(&["push", "origin", "main"]), false).is_ok());
    assert!(validate_git_command(&args(&["push", "origin", "main"]), true).is_ok());
    assert!(validate_git_command(&args(&["push"]), false).is_ok());
    assert!(validate_git_command(&args(&["push", "--force"]), false)
        .unwrap_err()
        .to_string()
        .contains("force/delete/mirror"));
    assert!(validate_git_command(&args(&["push", "--force"]), true).is_err());
    assert!(validate_git_command(&args(&["push", "origin", "+HEAD:main"]), true).is_err());
    assert!(validate_git_command(&args(&["push", "origin", "main:"]), true).is_err());

    assert!(
        validate_git_command(&args(&["commit", "-m", "docs: refresh screenshots"]), false).is_ok()
    );
    assert!(validate_git_command(&args(&["commit", "--amend", "-m", "no"]), true).is_err());
    assert!(validate_git_command(&args(&["commit"]), true).is_err());

    assert!(validate_git_command(&args(&["add", "--", "docs/index.html"]), true).is_ok());
    assert!(validate_git_command(&args(&["add", "."]), true).is_ok());
    assert!(validate_git_command(&args(&["add", "-A"]), true).is_ok());
    assert!(validate_git_command(&args(&["reset", "--hard"]), true).is_err());

    assert!(validate_command_arguments(
        "git",
        &args(&["https://user:secret@example.com/repository.git"]),
    )
    .is_err());
    assert!(
        validate_command_arguments("git", &args(&["https://example.com/repository.git"]),).is_ok()
    );
}
