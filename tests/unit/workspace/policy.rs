use super::*;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
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
