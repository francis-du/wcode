use super::*;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn bounded_remote_git_lifecycle_is_autonomous_while_destructive_shapes_stay_blocked() {
    for command in [
        vec!["add", "-A"],
        vec!["ls-remote", "origin", "refs/heads/main"],
        vec!["ls-remote", "origin", "refs/tags/v0.7.5"],
        vec!["fetch", "origin", "main"],
        vec!["fetch", "origin", "refs/heads/main"],
    ] {
        assert!(
            validate_git_command(&args(&command), false).is_ok(),
            "bounded Git lifecycle was rejected: {command:?}"
        );
    }

    for command in [
        vec!["fetch", "https://example.com/repo.git", "main"],
        vec!["fetch", "origin", "+main:main"],
        vec!["push", "--force", "origin", "main"],
        vec!["push", "origin", ":main"],
    ] {
        assert!(
            validate_git_command(&args(&command), true).is_err(),
            "destructive or unbounded Git shape was accepted: {command:?}"
        );
    }
}

#[test]
fn bounded_github_repository_lifecycle_is_autonomous_but_sensitive_surfaces_stay_closed() {
    let safe = WorkspaceSecurity::default();
    for command in [
        vec!["api", "repos/{owner}/{repo}/commits/main", "--jq", ".sha"],
        vec![
            "pr", "create", "--title", "release", "--body", "verified", "--head", "release",
            "--base", "main",
        ],
        vec!["pr", "merge", "123", "--merge"],
        vec![
            "workflow",
            "run",
            "release.yml",
            "--ref",
            "main",
            "-f",
            "release_tag=v0.7.5",
        ],
        vec![
            "release",
            "create",
            "v0.7.5",
            "--verify-tag",
            "--generate-notes",
        ],
    ] {
        assert!(
            validate_command_policy("gh", &args(&command), safe).is_ok(),
            "bounded GitHub lifecycle was rejected: {command:?}"
        );
    }

    for command in [
        vec!["api", "repos/{owner}/{repo}/issues", "-f", "title=x"],
        vec!["api", "https://api.github.com/user"],
        vec!["auth", "token"],
        vec!["secret", "list"],
        vec!["config", "set", "prompt", "disabled"],
        vec!["pr", "merge", "123", "--admin"],
    ] {
        assert!(
            validate_command_policy("gh", &args(&command), safe).is_err(),
            "sensitive or unbounded GitHub surface was accepted: {command:?}"
        );
    }
}
