//! Command policy and authorization boundaries: direct checks, repository
//! execution, language tool authorization and revocation records.
use super::*;

#[test]
fn command_policy_keeps_direct_checks_safe_and_repository_execution_exact() {
    // Explicit opt-in hardening: the default policy is permissive, so these
    // boundary checks exercise the restrictive configuration an embedder builds.
    let safe = WorkspaceSecurity {
        allow_risky_exec: false,
        allow_unrestricted_commands: false,
        ..WorkspaceSecurity::default()
    };
    assert!(validate_command_policy("git", &["status".to_owned()], safe).is_ok());
    assert!(validate_command_policy("git", &["clean".to_owned(), "-fd".to_owned()], safe).is_err());
    assert!(validate_command_policy(
        "python3",
        &["-c".to_owned(), "print('unsafe')".to_owned()],
        safe,
    )
    .is_err());
    assert!(validate_command_policy("cargo", &["fmt".to_owned()], safe).is_ok());
    assert!(
        validate_command_policy("cargo", &["fmt".to_owned(), "--check".to_owned()], safe,).is_ok()
    );
    assert!(validate_command_policy("cargo", &["check".to_owned()], safe).is_ok());
    assert!(
        validate_command_policy("cargo", &["check".to_owned(), "--locked".to_owned()], safe,)
            .is_ok()
    );
    assert!(validate_command_policy("cargo", &["test".to_owned()], safe).is_ok());
    assert!(
        validate_command_policy("cargo", &["test".to_owned(), "--locked".to_owned()], safe,)
            .is_ok()
    );
    assert!(validate_command_policy(
        "cargo",
        &[
            "clippy".to_owned(),
            "--locked".to_owned(),
            "--".to_owned(),
            "-D".to_owned(),
            "warnings".to_owned(),
        ],
        safe,
    )
    .is_ok());
    assert!(validate_command_policy(
        "cargo",
        &[
            "build".to_owned(),
            "--release".to_owned(),
            "--locked".to_owned()
        ],
        safe,
    )
    .is_ok());
    assert!(validate_command_policy(
        "cargo",
        &["check".to_owned(), "--workspace".to_owned()],
        safe,
    )
    .is_ok());
    assert!(validate_command_policy("cargo", &["metadata".to_owned()], safe).is_ok());
    assert!(validate_command_policy(
        "cargo",
        &[
            "metadata".to_owned(),
            "--format-version".to_owned(),
            "1".to_owned(),
            "--no-deps".to_owned(),
        ],
        safe,
    )
    .is_ok());
    assert!(validate_command_policy("go", &["list".to_owned()], safe).is_ok());
    assert!(validate_command_policy("go", &["list".to_owned(), "./...".to_owned()], safe,).is_ok());
    assert!(validate_command_policy("npm", &["list".to_owned()], safe).is_ok());
    assert!(validate_command_policy("pnpm", &["why".to_owned(), "serde".to_owned()], safe).is_ok());
    assert!(validate_command_policy("dotnet", &["list".to_owned()], safe).is_ok());
    assert!(
        validate_command_policy("rg", &["needle".to_owned(), "--hidden".to_owned()], safe,)
            .is_err()
    );
    for arguments in [
        vec!["-f".to_owned(), ".env".to_owned()],
        vec!["--file".to_owned(), ".env".to_owned()],
        vec!["--ignore-file".to_owned(), ".env".to_owned()],
        vec!["--glob".to_owned(), ".env".to_owned(), "needle".to_owned()],
        vec!["--type-add=secret:*.env".to_owned(), "needle".to_owned()],
    ] {
        assert!(
            validate_command_policy("rg", &arguments, safe).is_err(),
            "ripgrep helper/file-selection bypass was accepted: {arguments:?}"
        );
    }
    assert!(
        validate_command_policy("git", &["show".to_owned(), "HEAD:.env".to_owned()], safe,).is_ok()
    );
    assert!(validate_command_policy(
        "git",
        &["log".to_owned(), "--show-signature".to_owned()],
        safe,
    )
    .is_err());
    assert!(
        validate_command_policy("git", &["log".to_owned(), "--format=%G?".to_owned()], safe,)
            .is_err()
    );
    assert!(validate_command_policy("rg", &["TOKEN".to_owned(), ".env".to_owned()], safe,).is_ok());

    let trusted = WorkspaceSecurity {
        allow_risky_exec: true,
        ..safe
    };
    assert!(validate_command_policy("cargo", &["test".to_owned()], trusted).is_ok());
    assert!(validate_command_policy("cargo", &["metadata".to_owned()], trusted).is_ok());
    assert!(validate_command_policy("go", &["list".to_owned()], trusted).is_ok());
    assert!(validate_command_policy("npm", &["list".to_owned()], trusted).is_ok());
    assert!(validate_command_policy("cargo", &["run".to_owned()], safe).is_ok());
    assert!(validate_command_policy("cargo", &["run".to_owned()], trusted).is_ok());
    assert!(validate_command_policy("cargo", &["clean".to_owned()], safe).is_err());
    assert!(validate_command_policy("cargo", &["clean".to_owned()], trusted).is_ok());
    assert!(validate_command_policy("npm", &["install".to_owned()], safe).is_ok());
    assert!(validate_command_policy("npm", &["install".to_owned()], trusted).is_ok());
    for (program, arguments) in [
        ("cargo", vec!["publish".to_owned()]),
        ("npm", vec!["publish".to_owned()]),
        ("deno", vec!["install".to_owned()]),
        ("go", vec!["env".to_owned()]),
    ] {
        assert!(
            validate_command_policy(program, &arguments, trusted).is_err(),
            "permanent host/credential/publish boundary became authorizable: {program} {arguments:?}"
        );
    }
    assert!(validate_command_policy(
        "cargo",
        &[
            "metadata".to_owned(),
            "--config".to_owned(),
            "build.rustc-wrapper=tool".to_owned(),
        ],
        trusted,
    )
    .is_err());
    assert!(validate_command_policy(
        "go",
        &["list".to_owned(), "-C".to_owned(), "subdir".to_owned()],
        trusted,
    )
    .is_err());
    assert!(validate_command_policy(
        "npm",
        &[
            "list".to_owned(),
            "--prefix".to_owned(),
            "subdir".to_owned()
        ],
        trusted,
    )
    .is_err());
    assert!(validate_command_policy("rustc", &["@args.txt".to_owned()], trusted).is_err());
}

#[test]
fn git_arguments_disable_repository_helpers_and_external_config_paths() {
    let status = hardened_command_args("git", &["status".to_owned(), "--short".to_owned()]);
    assert!(status
        .windows(2)
        .any(|pair| pair[0] == "-c" && pair[1] == "core.fsmonitor=false"));
    assert!(status.iter().any(|arg| arg.starts_with("core.hooksPath=")));
    assert!(status
        .iter()
        .any(|arg| arg.starts_with("core.attributesFile=")));
    assert!(status
        .iter()
        .any(|arg| arg.starts_with("core.excludesFile=")));
    for blocked_helper in [
        "credential.helper=",
        "core.askPass=",
        "core.sshCommand=false",
        "core.gitProxy=",
        "http.extraHeader=",
    ] {
        assert!(status.iter().any(|arg| arg == blocked_helper));
    }
    assert_eq!(
        status
            .get(status.len().saturating_sub(2))
            .map(String::as_str),
        Some("status")
    );
    assert_eq!(status.last().map(String::as_str), Some("--short"));

    let diff = hardened_command_args("git", &["diff".to_owned(), "--cached".to_owned()]);
    let subcommand = diff.iter().position(|arg| arg == "diff").unwrap();
    assert_eq!(diff[subcommand + 1], "--no-ext-diff");
    assert_eq!(diff[subcommand + 2], "--no-textconv");
    assert_eq!(diff.last().map(String::as_str), Some("--cached"));

    let push = hardened_command_args(
        "git",
        &[
            "push".to_owned(),
            "origin".to_owned(),
            "HEAD:main".to_owned(),
        ],
    );
    assert!(push.iter().any(|arg| {
        arg == "core.sshCommand=ssh -oBatchMode=yes -oStrictHostKeyChecking=accept-new"
    }));
    assert!(!push.iter().any(|arg| arg == "core.sshCommand=false"));
    assert!(push.iter().any(|arg| arg == "credential.helper="));

    let lfs_push = hardened_command_args(
        "git",
        &[
            "lfs".to_owned(),
            "push".to_owned(),
            "origin".to_owned(),
            "main".to_owned(),
        ],
    );
    assert!(lfs_push.iter().any(|arg| {
        arg == "core.sshCommand=ssh -oBatchMode=yes -oStrictHostKeyChecking=accept-new"
    }));
    assert!(!lfs_push.iter().any(|arg| arg == "core.sshCommand=false"));
}

#[test]
fn verification_policy_allows_only_inferred_quality_shapes() {
    assert!(validate_verification_command_shape(
        "cargo",
        &["check".to_owned(), "--locked".to_owned()],
    )
    .is_ok());
    assert!(validate_verification_command_shape(
        "cargo",
        &[
            "clippy".to_owned(),
            "--locked".to_owned(),
            "--".to_owned(),
            "-D".to_owned(),
            "warnings".to_owned(),
        ],
    )
    .is_ok());
    assert!(validate_verification_command_shape(
        "cargo",
        &[
            "build".to_owned(),
            "--release".to_owned(),
            "--locked".to_owned(),
        ],
    )
    .is_ok());
    assert!(validate_verification_command_shape(
        "cargo",
        &[
            "nextest".to_owned(),
            "run".to_owned(),
            "--locked".to_owned(),
        ],
    )
    .is_ok());
    assert!(validate_verification_command_shape(
        "cargo",
        &[
            "nextest".to_owned(),
            "run".to_owned(),
            "name(test)".to_owned(),
        ],
    )
    .is_err());
    assert!(validate_verification_command_shape(
        "pnpm",
        &["run".to_owned(), "typecheck".to_owned()],
    )
    .is_ok());
    assert!(
        validate_verification_command_shape("go", &["test".to_owned(), "./...".to_owned()],)
            .is_ok()
    );
    assert!(validate_verification_command_shape("cargo", &["run".to_owned()]).is_err());
    assert!(validate_verification_command_shape(
        "npm",
        &["run".to_owned(), "postinstall".to_owned()],
    )
    .is_err());
    assert!(validate_verification_command_shape(
        "python3",
        &["-c".to_owned(), "print('no')".to_owned()],
    )
    .is_err());
}

#[tokio::test]
async fn bounded_direct_and_harness_checks_run_without_risky_exec() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn value() -> u8 { 1 }\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, true).unwrap();

    let direct = workspace
        .run_command("cargo", &["check".to_owned()], ".", 30)
        .await
        .expect("bounded direct cargo check should not require risky-exec");
    assert!(direct.success, "cargo check failed: {}", direct.stderr);

    let verified = workspace
        .run_verification_command("cargo", &["check".to_owned()], ".", 30)
        .await
        .expect("exact Harness verification shape may run without the global risky flag");
    assert!(verified.success, "cargo check failed: {}", verified.stderr);
}

#[test]
fn all_supported_language_development_tools_are_default_authorized() {
    for program in LANGUAGE_DEVELOPMENT_COMMANDS {
        assert!(
            COMMAND_CATALOG.contains(program),
            "supported language development tool must be in the default command catalog: {program}"
        );
    }
    for infrastructure in ["docker", "kubectl", "terraform"] {
        assert!(!LANGUAGE_DEVELOPMENT_COMMANDS.contains(&infrastructure));
    }

    let safe = WorkspaceSecurity::default();
    for (program, arguments) in [
        ("cargo", vec!["run"]),
        (
            "cargo-fuzz",
            vec!["fuzz", "run", "parser", "--", "-max_total_time=5"],
        ),
        ("shellcheck", vec!["--format=json", "script.sh"]),
        ("clang-format", vec!["--dry-run", "--Werror", "src.c"]),
        ("clang", vec!["-fsyntax-only", "src.c"]),
        ("gofmt", vec!["-d", "main.go"]),
        ("staticcheck", vec!["./..."]),
        ("mypy", vec!["."]),
        ("eslint", vec![".", "--format", "json"]),
        ("tsc", vec!["--noEmit"]),
        ("Rscript", vec!["--vanilla", "-e", "testthat::test_local()"]),
        ("ruby", vec!["app.rb"]),
        ("php", vec!["app.php"]),
        ("lua", vec!["app.lua"]),
        ("elixir", vec!["app.exs"]),
        ("ocamlc", vec!["main.ml"]),
        ("java", vec!["Main"]),
        ("composer", vec!["install"]),
        ("swift-format", vec!["lint", "-r", "."]),
    ] {
        let arguments = arguments.into_iter().map(str::to_owned).collect::<Vec<_>>();
        assert!(
            validate_command_policy(program, &arguments, safe).is_ok(),
            "bounded development shape must not require risky authorization: {program} {arguments:?}"
        );
    }
}

#[tokio::test]
async fn hardened_runtime_executor_runs_without_repetitive_authorization() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, true).unwrap();
    let result = workspace
        .run_trusted_runtime_command("rustc", &["--version".to_owned()], ".", 10)
        .await
        .expect("repository-declared test executors should use the hardened autonomous lane");
    assert!(result.success);
    assert!(workspace.authorization.requests(10).is_empty());
}

#[tokio::test]
async fn command_revocation_is_recorded_without_blocking_execution() {
    let test = "command_revocation_is_recorded_without_blocking_execution";
    if process_fixture::run_in_private_fixture(module_path!(), test).await {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([dir.path()], true, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    workspaces
        .revoke_command(Some(&workspace_id), "git")
        .unwrap();
    let (_, workspace) = workspaces.select(Some(&workspace_id)).unwrap();

    let result = workspace
        .run_command("git", &["status".to_owned()], ".", 10)
        .await
        .expect("revocation is bookkeeping only under the permissive default");
    assert!(!result.stderr.contains("authorization required"));
    assert!(workspaces.latest_pending_authorization().is_none());

    workspaces
        .revoke_command(Some(&workspace_id), "git")
        .unwrap();
    assert!(!workspace
        .allowed_commands()
        .iter()
        .any(|command| command == "git"));

    let arbitrary = workspace
        .run_command("wcode-definitely-not-installed", &[], ".", 10)
        .await
        .map(|result| result.stderr)
        .unwrap_or_else(|error| error.to_string());
    assert!(!arbitrary.contains("authorization required"));
    assert!(workspaces.latest_pending_authorization().is_none());
}

#[test]
fn workspace_command_allowlist_supports_defaults_and_operator_authorized_programs() {
    let dir = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([dir.path()], true, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let (_, selected_before) = workspaces.select(Some(&workspace_id)).unwrap();
    assert!(selected_before
        .allowed_commands()
        .iter()
        .any(|command| command == "git"));

    let revoked = workspaces
        .revoke_command(Some(&workspace_id), "git")
        .unwrap();
    assert_eq!(revoked["changed"].as_bool(), Some(true));
    assert!(!selected_before
        .allowed_commands()
        .iter()
        .any(|command| command == "git"));
    assert!(selected_before
        .available_commands()
        .iter()
        .any(|command| command == "git"));

    let restored = workspaces
        .allow_command(Some(&workspace_id), "git")
        .unwrap();
    assert_eq!(restored["changed"].as_bool(), Some(true));
    assert!(selected_before
        .allowed_commands()
        .iter()
        .any(|command| command == "git"));
    assert!(workspaces
        .allow_command(Some(&workspace_id), "hugo")
        .is_ok());
    assert!(selected_before
        .allowed_commands()
        .iter()
        .any(|command| command == "hugo"));
    assert!(workspaces
        .allow_command(Some(&workspace_id), "bash")
        .is_ok());
    assert!(selected_before
        .allowed_commands()
        .iter()
        .any(|command| command == "bash"));
}
