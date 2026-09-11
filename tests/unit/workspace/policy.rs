use super::*;
use crate::{authorization::AuthorizationKind, workspace::Workspace};
use std::fs;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[tokio::test]
async fn repository_commands_request_exact_authorization_instead_of_staying_hard_blocked() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname='authorized-run'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "fn main() { println!(\"authorized\"); }\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, true).unwrap();

    let blocked = workspace
        .run_command("cargo", &["run".to_owned()], ".", 30)
        .await
        .unwrap_err();
    assert!(blocked.to_string().contains("authorization required"));
    let request = workspace.authorization.latest_pending().unwrap();
    assert_eq!(request.kind, AuthorizationKind::RiskyExecution);
    assert!(workspace.authorization.approve_session(&request.id));

    let result = workspace
        .run_command("cargo", &["run".to_owned()], ".", 30)
        .await
        .unwrap();
    assert!(result.success, "cargo run failed: {}", result.stderr);
    assert!(result.stdout.contains("authorized"));
}

#[test]
fn git_probe_routing_is_exact_and_never_promotes_mutations_or_helpers() {
    for command in [
        vec!["status", "--short", "--untracked-files=all"],
        vec!["status", "--short", "--branch"],
        vec!["diff", "--check"],
        vec!["diff", "--cached", "--numstat"],
        vec!["branch", "--show-current"],
    ] {
        assert!(is_git_probe("git", &args(&command)));
    }
    for command in [
        vec!["commit", "-m", "status"],
        vec!["status", "--ignored"],
        vec!["diff", "--numstat", "--ext-diff"],
        vec!["diff", "--check", "--output=result.txt"],
        vec!["log", "--all"],
        vec!["-c", "core.fsmonitor=helper", "status"],
        vec!["push", "origin", "main"],
    ] {
        assert!(!is_git_probe("git", &args(&command)), "{command:?}");
    }
    assert!(!is_git_probe("cargo", &args(&["check"])));
    assert!(!is_git_probe("python3", &args(&["--version"])));
}

#[test]
fn all_target_clippy_is_check_only_and_keeps_exact_policy_boundaries() {
    let safe = WorkspaceSecurity::default();
    for values in [
        vec!["clippy", "--all-targets", "--", "-D", "warnings"],
        vec![
            "clippy",
            "--locked",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    ] {
        let command = args(&values);
        assert!(validate_verification_command_shape("cargo", &command).is_ok());
        assert!(validate_command_policy("cargo", &command, safe).is_ok());
    }
    for values in [
        vec!["clippy", "--all-targets", "--fix", "--", "-D", "warnings"],
        vec!["clippy", "--all-targets", "--", "-A", "warnings"],
        vec!["clippy", "--all-targets", "--config", "other.toml"],
        vec![
            "clippy",
            "--all-targets",
            "--manifest-path",
            "other/Cargo.toml",
        ],
    ] {
        let command = args(&values);
        assert!(validate_verification_command_shape("cargo", &command).is_err());
        assert!(validate_command_policy("cargo", &command, safe).is_err());
    }
}

#[test]
fn common_development_tools_have_bounded_read_verify_and_mutation_policies() {
    assert!(
        validate_gh_command(&args(&["pr", "view", "42", "--json", "title,url"]), false).is_ok()
    );
    assert!(validate_gh_command(
        &args(&[
            "pr",
            "create",
            "--title",
            "feat: bounded gh",
            "--body",
            "details",
            "--head",
            "feature",
            "--base",
            "main"
        ]),
        false,
    )
    .is_err());
    assert!(validate_gh_command(
        &args(&[
            "pr",
            "create",
            "--title",
            "feat: bounded gh",
            "--body",
            "details",
            "--head",
            "feature",
            "--base",
            "main"
        ]),
        true,
    )
    .is_ok());
    assert!(validate_gh_command(&args(&["pr", "create", "--fill"]), true).is_err());
    assert!(validate_gh_command(&args(&["api", "repos/example/example"]), true).is_err());
    assert!(validate_gh_command(&args(&["secret", "list"]), true).is_err());
    assert!(validate_gh_command(
        &args(&[
            "release",
            "create",
            "v0.4.0",
            "--verify-tag",
            "--generate-notes",
            "--title",
            "wcode 0.4.0"
        ]),
        true,
    )
    .is_ok());
    assert!(validate_gh_command(
        &args(&[
            "release",
            "create",
            "v0.4.0",
            "dist/wcode.tar.gz",
            "--verify-tag",
            "--generate-notes"
        ]),
        true,
    )
    .is_err());
    assert!(validate_gh_command(&args(&["pr", "merge", "42", "--squash"]), true).is_ok());
    assert!(
        validate_gh_command(&args(&["pr", "merge", "42", "--admin", "--squash"]), true).is_err()
    );

    assert!(validate_repository_runner("just", false).is_err());
    assert!(validate_repository_runner("task", true).is_ok());
    assert!(validate_uv_command(&args(&["lock", "--check"]), false).is_ok());
    assert!(validate_uv_command(&args(&["tree", "--locked"]), false).is_ok());
    assert!(validate_uv_command(&args(&["run", "--locked", "pytest"]), false).is_err());
    assert!(validate_uv_command(&args(&["run", "--locked", "pytest"]), true).is_ok());
    assert!(validate_uv_command(&args(&["auth", "login"]), true).is_err());

    assert!(validate_ruff_command(&args(&["check", "."]), false).is_ok());
    assert!(validate_ruff_command(&args(&["check", "--fix", "."]), false).is_err());
    assert!(validate_ruff_command(&args(&["format", "--check", "."]), false).is_ok());
    assert!(validate_biome_command(&args(&["ci", "."]), false).is_ok());
    assert!(validate_biome_command(&args(&["check", "--write", "."]), false).is_err());
    assert!(validate_deno_command(&args(&["lint"]), false).is_ok());
    assert!(validate_deno_command(&args(&["fmt", "--check"]), false).is_ok());
    assert!(validate_deno_command(&args(&["run", "main.ts"]), false).is_err());

    assert!(validate_docker_command(&args(&["compose", "config"]), false).is_err());
    assert!(validate_docker_command(&args(&["compose", "config"]), true).is_ok());
    assert!(validate_docker_command(&args(&["compose", "up", "-d"]), false).is_err());
    assert!(validate_docker_command(&args(&["compose", "up", "-d"]), true).is_ok());
    assert!(validate_docker_command(&args(&["compose", "down", "--volumes"]), true).is_err());
    assert!(validate_kubectl_command(&args(&["api-resources"]), false).is_ok());
    assert!(validate_kubectl_command(&args(&["get", "pods"]), false).is_err());
    assert!(validate_kubectl_command(&args(&["get", "pods"]), true).is_ok());
    assert!(validate_kubectl_command(&args(&["get", "pods", "--token", "secret"]), true).is_err());
    assert!(validate_kubectl_command(&args(&["apply", "-f", "deploy.yaml"]), true).is_err());
    assert!(validate_terraform_command(&args(&["validate"]), false).is_ok());
    assert!(validate_terraform_command(&args(&["fmt", "-check"]), false).is_ok());
    assert!(validate_terraform_command(&args(&["plan"]), false).is_err());
    assert!(validate_terraform_command(&args(&["plan"]), true).is_ok());
    assert!(validate_terraform_command(&args(&["apply"]), true).is_err());
    assert!(validate_terraform_command(&args(&["show", "-json"]), true).is_err());

    assert!(validate_fd_command(&args(&["handler", "src"])).is_ok());
    assert!(validate_fd_command(&args(&["-H", "handler", "."])).is_err());
    assert!(validate_fd_command(&args(&["handler", "-x", "cat"])).is_err());
    assert!(validate_jq_command(&args(&[".version", "package.json"])).is_ok());
    assert!(validate_jq_command(&args(&["--rawfile", "secret", ".env", "."])).is_err());

    assert!(validate_dotnet_command(&args(&["--info"]), false).is_ok());
    assert!(validate_dotnet_command(&args(&["test", "--no-restore"]), false).is_err());
    assert!(validate_dotnet_command(&args(&["test", "--no-restore"]), true).is_ok());
    assert!(validate_dotnet_command(&args(&["tool", "install", "x"]), true).is_err());
    for program in [
        "cmake",
        "ninja",
        "mvn",
        "gradle",
        "swift",
        "zig",
        "pre-commit",
        "act",
    ] {
        assert!(validate_known_project_runner(program, &args(&["check"]), false).is_err());
        assert!(validate_known_project_runner(program, &args(&["check"]), true).is_ok());
    }
    assert!(validate_known_project_runner("mvn", &args(&["deploy"]), true).is_err());
    assert!(validate_known_project_runner("gradle", &args(&["publish"]), true).is_err());
    assert!(validate_known_project_runner("swift", &args(&["sdk", "list"]), true).is_err());
    assert!(validate_known_project_runner("act", &args(&["--privileged"]), true).is_err());

    assert!(validate_cargo_command(&args(&["nextest", "run"]), false).is_err());
    assert!(validate_cargo_command(&args(&["nextest", "run", "--locked"]), false).is_err());
    assert!(validate_cargo_command(&args(&["nextest", "run", "name(test)"]), false).is_err());
    assert!(validate_cargo_command(&args(&["nextest", "run", "name(test)"]), true).is_ok());
    assert!(validate_cargo_command(&args(&["nextest", "archive"]), true).is_err());
    assert!(validate_git_command(&args(&["lfs", "status"]), false).is_ok());
    assert!(validate_git_command(&args(&["lfs", "push", "origin", "main"]), false).is_err());
    assert!(validate_git_command(&args(&["lfs", "push", "origin", "main"]), true).is_ok());
    assert!(validate_git_command(&args(&["lfs", "push", "--all", "origin"]), true).is_err());
}

#[tokio::test]
async fn audit_git_literal_messages_request_approval_without_path_misclassification() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    for command in [
        vec!["commit", "-m", "../migration note"],
        vec!["commit", "--message=.env"],
        vec!["commit", "-m", "/healthz endpoint"],
        vec!["tag", "-a", "v-test", "-m", ".env"],
    ] {
        let error = workspace
            .run_command("git", &args(&command), ".", 1)
            .await
            .unwrap_err();
        let required = error
            .downcast_ref::<crate::authorization::AuthorizationRequired>()
            .unwrap_or_else(|| {
                panic!("literal message must request approval: {command:?}: {error}")
            });
        assert_eq!(required.request.kind, AuthorizationKind::RiskyExecution);
        assert!(workspace.authorization.deny(&required.request.id));
    }
    for command in [
        vec!["add", "--", "../outside"],
        vec!["add", "--", ".env"],
        vec!["commit", "--file", "../message"],
        vec!["tag", "-a", "v-test", "-m", "note", "../outside"],
    ] {
        assert!(validate_command_policy(
            "git",
            &args(&command),
            WorkspaceSecurity {
                allow_risky_exec: true,
                ..WorkspaceSecurity::default()
            }
        )
        .is_err());
    }
}

#[test]
fn ordinary_git_lifecycle_uses_approval_instead_of_permanent_denial() {
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
            validate_git_command(&args(&command), false).is_err(),
            "{command:?}"
        );
        assert!(
            validate_git_command(&args(&command), true).is_ok(),
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

#[tokio::test]
async fn invalid_commands_do_not_create_useless_approval_requests() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    for (program, arguments, cwd) in [
        ("unlisted-example", vec!["../outside"], "."),
        ("unlisted-example", vec!["--version"], "missing"),
        ("git", vec!["branch", "feature"], "missing"),
        ("git", vec!["branch", "-D", "main"], "."),
    ] {
        assert!(workspace
            .run_command(program, &args(&arguments), cwd, 1)
            .await
            .is_err());
        assert!(workspace.authorization.requests(10).is_empty());
    }
}

#[tokio::test]
async fn git_branch_approval_is_effective_and_does_not_grant_other_operations() {
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
    assert!(git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.test",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "--allow-empty",
        "-m",
        "initial"
    ]));
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let command = args(&["branch", "approved-feature"]);
    assert!(workspace
        .run_command("git", &command, ".", 10)
        .await
        .is_err());
    let denied = workspace.authorization.latest_pending().unwrap();
    assert_eq!(denied.kind, AuthorizationKind::RiskyExecution);
    assert!(workspace.authorization.deny(&denied.id));
    assert!(!git(&[
        "rev-parse",
        "--verify",
        "refs/heads/approved-feature"
    ]));
    assert!(workspace
        .run_command("git", &command, ".", 10)
        .await
        .is_err());
    let approved = workspace.authorization.latest_pending().unwrap();
    assert!(workspace.authorization.approve_session(&approved.id));
    let result = workspace
        .run_command("git", &command, ".", 10)
        .await
        .unwrap();
    assert!(result.success);
    assert!(git(&[
        "rev-parse",
        "--verify",
        "refs/heads/approved-feature"
    ]));
    assert!(!workspace.security.allow_risky_exec);
    assert!(workspace
        .run_command("git", &args(&["tag", "not-approved"]), ".", 10)
        .await
        .is_err());
    assert!(!git(&["rev-parse", "--verify", "refs/tags/not-approved"]));
}

#[test]
fn git_mutations_require_exact_risky_authorization_and_keep_hard_boundaries() {
    assert!(validate_git_command(&args(&["push", "origin", "main"]), false).is_err());
    assert!(validate_git_command(&args(&["push", "origin", "main"]), true).is_ok());
    assert!(validate_git_command(&args(&["push"]), true).is_err());
    assert!(validate_git_command(&args(&["push", "--force"]), false)
        .unwrap_err()
        .to_string()
        .contains("force/delete/mirror"));
    assert!(validate_git_command(&args(&["push", "--force"]), true).is_err());
    assert!(validate_git_command(&args(&["push", "origin", "+HEAD:main"]), true).is_err());
    assert!(validate_git_command(&args(&["push", "origin", "main:"]), true).is_err());

    assert!(
        validate_git_command(&args(&["commit", "-m", "docs: refresh screenshots"]), true).is_ok()
    );
    assert!(validate_git_command(&args(&["commit", "--amend", "-m", "no"]), true).is_err());
    assert!(validate_git_command(&args(&["commit"]), true).is_err());

    assert!(validate_git_command(&args(&["add", "--", "docs/index.html"]), true).is_ok());
    assert!(validate_git_command(&args(&["add", "."]), true).is_err());
    assert!(validate_git_command(&args(&["add", "-A"]), true).is_err());
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
