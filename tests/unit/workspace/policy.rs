use super::*;
use crate::{semantic_provider::SemanticLanguage, workspace::Workspace};
use std::fs;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[tokio::test]
async fn local_repository_development_runs_without_exact_authorization() {
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

    let result = workspace
        .run_command("cargo", &["run".to_owned()], ".", 30)
        .await
        .unwrap();
    assert!(result.success, "cargo run failed: {}", result.stderr);
    assert!(result.stdout.contains("authorized"));
    assert!(workspace.authorization.requests(10).is_empty());
}

#[test]
fn inspection_probe_routing_is_exact_and_never_promotes_mutations_or_helpers() {
    for command in [
        vec!["status", "--short", "--untracked-files=all"],
        vec!["status", "--short", "--branch"],
        vec!["diff", "--check"],
        vec!["diff", "--cached", "--numstat"],
        vec!["branch", "--show-current"],
    ] {
        assert!(is_inspection_probe("git", &args(&command)));
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
        assert!(!is_inspection_probe("git", &args(&command)), "{command:?}");
    }
    assert!(is_inspection_probe("cargo", &args(&["fmt", "--check"])));
    assert!(is_inspection_probe(
        "cargo",
        &args(&["metadata", "--no-deps", "--format-version", "1"]),
    ));
    assert!(!is_inspection_probe("cargo", &args(&["metadata"])));
    assert!(!is_inspection_probe("cargo", &args(&["check"])));
    assert!(!is_inspection_probe("cargo", &args(&["fmt"])));
    assert!(!is_inspection_probe("python3", &args(&["--version"])));
}

#[tokio::test]
async fn focused_rust_test_filter_runs_without_runtime_authorization() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname='focused-filter'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn value() -> u8 { 1 }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn focused_smoke() { assert_eq!(super::value(), 1); }\n}\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, true).unwrap();
    let result = workspace
        .run_verification_command("cargo", &args(&["test", "focused_smoke"]), ".", 30)
        .await
        .expect("bounded focused test should remain in the autonomous verification lane");
    assert!(result.success, "focused test failed: {}", result.stderr);
    assert!(workspace.authorization.latest_pending().is_none());
}

#[test]
fn focused_rust_test_filter_is_exact_and_does_not_open_arbitrary_test_arguments() {
    for values in [
        vec!["test", "focused_smoke"],
        vec!["test", "--locked", "focused_smoke_2"],
        vec!["test", "200"],
        vec!["test", "module::focused_smoke"],
    ] {
        assert!(validate_verification_command_shape("cargo", &args(&values)).is_ok());
    }
    for values in [
        vec!["test", "--locked", "focused_smoke", "--", "--nocapture"],
        vec!["test", "focused::"],
        vec!["test", "../focused"],
        vec!["test", "--exact"],
        vec!["test", "--manifest-path", "other/Cargo.toml"],
    ] {
        assert!(validate_verification_command_shape("cargo", &args(&values)).is_err());
    }
}

#[test]
fn focused_python_and_go_test_filters_are_exact_and_path_bounded() {
    let safe = WorkspaceSecurity::default();
    for (program, values) in [
        ("pytest", vec!["-q", "tests/test_engine.py::test_run"]),
        ("go", vec!["test", ".", "-run", "^TestRun$"]),
        ("go", vec!["test", "./pkg/engine", "-run", "^TestRun2$"]),
    ] {
        let command = args(&values);
        assert!(validate_verification_command_shape(program, &command).is_ok());
        assert!(validate_command_policy(program, &command, safe).is_ok());
    }

    for (program, values) in [
        ("pytest", vec!["-q", "../tests/test_engine.py::test_run"]),
        ("pytest", vec!["-q", "tests/test_engine.py::TestRun"]),
        (
            "pytest",
            vec!["-q", "tests/test_engine.py::Suite::test_run"],
        ),
        (
            "pytest",
            vec!["-q", "tests/test_engine.py::test_run", "-k", "smoke"],
        ),
        ("go", vec!["test", "./...", "-run", "^TestRun$"]),
        ("go", vec!["test", "../pkg", "-run", "^TestRun$"]),
        ("go", vec!["test", "./pkg", "-run", "TestRun"]),
        ("go", vec!["test", "./pkg", "-run", "^Testlower$"]),
        ("go", vec!["test", "./pkg", "-run", "^TestRun$", "-count=1"]),
    ] {
        assert!(
            validate_verification_command_shape(program, &args(&values)).is_err(),
            "focused verification shape unexpectedly accepted {program} {values:?}"
        );
    }
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
    ] {
        let command = args(&values);
        assert!(validate_verification_command_shape("cargo", &command).is_err());
        assert!(validate_command_policy("cargo", &command, safe).is_ok());
    }
    for values in [
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
fn common_full_suite_cargo_and_flutter_verification_is_autonomous() {
    let safe = WorkspaceSecurity::default();
    for (program, values) in [
        ("cargo", vec!["test", "--workspace", "--no-fail-fast"]),
        ("cargo", vec!["check", "--workspace", "--all-targets"]),
        (
            "cargo",
            vec![
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        ("cargo", vec!["fmt", "--all", "--", "--check"]),
        ("flutter", vec!["analyze", "--no-pub"]),
        ("flutter", vec!["test", "--no-pub"]),
        (
            "flutter",
            vec!["test", "--no-pub", "--file-reporter=json:test-results.json"],
        ),
        (
            "flutter",
            vec!["build", "web", "--release", "--wasm", "--no-pub"],
        ),
    ] {
        let command = args(&values);
        assert!(
            validate_verification_command_shape(program, &command).is_ok(),
            "verification shape unexpectedly gated: {program} {values:?}"
        );
        assert!(
            validate_command_policy(program, &command, safe).is_ok(),
            "safe command policy unexpectedly gated: {program} {values:?}"
        );
    }

    for (program, values) in [
        (
            "cargo",
            vec!["test", "--manifest-path", "../other/Cargo.toml"],
        ),
        ("cargo", vec!["clippy", "--fix", "--", "-D", "warnings"]),
        ("flutter", vec!["pub", "publish"]),
        (
            "flutter",
            vec!["test", "--file-reporter=json:/tmp/out.json"],
        ),
        (
            "flutter",
            vec!["test", "--file-reporter=json:.wcode-test.json"],
        ),
        ("flutter", vec!["test", "--concurrency=0"]),
        (
            "flutter",
            vec!["build", "web", "--dart-define=TOKEN=secret"],
        ),
    ] {
        assert!(
            validate_verification_command_shape(program, &args(&values)).is_err(),
            "unsafe verification shape unexpectedly accepted: {program} {values:?}"
        );
    }
}

#[test]
fn polyglot_native_verification_shapes_are_bounded_and_autonomous() {
    let safe = WorkspaceSecurity::default();
    for (program, values) in [
        ("go", vec!["vet", "./..."]),
        ("mvn", vec!["-q", "-DskipTests", "compile"]),
        ("mvn", vec!["test"]),
        ("gradle", vec!["classes"]),
        ("gradle", vec!["check"]),
        ("swift", vec!["build"]),
        ("swift", vec!["test"]),
        ("dart", vec!["analyze"]),
        ("dart", vec!["test"]),
        (
            "dart",
            vec!["format", "-o", "none", "--set-exit-if-changed", "."],
        ),
        ("deno", vec!["fmt", "--check"]),
        ("deno", vec!["lint"]),
        ("deno", vec!["check", "--frozen", "."]),
        ("deno", vec!["test", "--frozen"]),
        ("deno", vec!["audit", "--frozen"]),
        ("mix", vec!["format", "--check-formatted"]),
        ("mix", vec!["compile", "--warnings-as-errors"]),
        ("mix", vec!["test"]),
        ("dune", vec!["build"]),
        ("dune", vec!["runtest"]),
        ("bundle", vec!["exec", "rubocop", "--format", "json"]),
        ("biome", vec!["format", ".", "--reporter=json"]),
        (
            "biome",
            vec![
                "check",
                ".",
                "--formatter-enabled=false",
                "--assist-enabled=false",
                "--reporter=json",
            ],
        ),
        ("prettier", vec![".", "--check"]),
        ("vitest", vec!["run"]),
        ("jest", vec!["--runInBand"]),
        ("htmlhint", vec!["**/*.html", "--format", "json"]),
        ("bats", vec!["tests/smoke.bats", "tests/api.bats"]),
        ("bundle", vec!["exec", "standardrb", "--format", "json"]),
        ("bundle", vec!["exec", "rspec"]),
        (
            "Rscript",
            vec!["--vanilla", "-e", "styler::style_pkg(dry=\"fail\")"],
        ),
        ("phpstan", vec!["analyse", "--error-format=json"]),
        ("psalm", vec!["--output-format=json"]),
        ("phpunit", vec![]),
        ("php-cs-fixer", vec!["fix", "--dry-run", "--diff"]),
        ("composer", vec!["audit", "--locked", "--format=json"]),
    ] {
        let command = args(&values);
        assert!(validate_verification_command_shape(program, &command).is_ok());
        assert!(validate_command_policy(program, &command, safe).is_ok());
    }
    for (program, values) in [
        ("mvn", vec!["deploy"]),
        ("gradle", vec!["publish"]),
        ("swift", vec!["sdk", "list"]),
        ("dart", vec!["pub", "publish"]),
        ("deno", vec!["fmt", "--watch"]),
        ("deno", vec!["test", "--watch"]),
        ("deno", vec!["audit", "--fix"]),
        ("deno", vec!["check", "."]),
        ("deno", vec!["test"]),
        ("deno", vec!["check", "--config", "../deno.json", "."]),
        ("mix", vec!["hex.publish"]),
        ("dune", vec!["exec", "./tool.exe"]),
        ("bundle", vec!["exec", "rake", "db:migrate"]),
        ("phpstan", vec!["analyse", "--debug"]),
        ("phpunit", vec!["--filter", "smoke"]),
        ("php-cs-fixer", vec!["fix"]),
        ("composer", vec!["audit", "--format=json"]),
        ("htmlhint", vec!["**/*.html", "--rulesdir", "tools/rules"]),
        ("bats", vec!["--recursive", "tests"]),
        ("bats", vec!["tests/not-bats.sh"]),
    ] {
        assert!(validate_verification_command_shape(program, &args(&values)).is_err());
    }
    for (program, values) in [
        ("mvn", vec!["deploy"]),
        ("gradle", vec!["publish"]),
        ("swift", vec!["sdk", "list"]),
        ("dart", vec!["pub", "publish"]),
        ("mix", vec!["hex.publish"]),
    ] {
        assert!(validate_command_policy(program, &args(&values), safe).is_err());
    }
    for (program, values) in [
        ("dune", vec!["exec", "./tool.exe"]),
        ("bundle", vec!["exec", "rake", "db:migrate"]),
        ("phpstan", vec!["analyse", "--debug"]),
        ("phpunit", vec!["--filter", "smoke"]),
        ("php-cs-fixer", vec!["fix"]),
    ] {
        assert!(validate_command_policy(program, &args(&values), safe).is_ok());
    }
}

#[test]
fn dart_routine_project_development_is_autonomous_but_remote_and_global_pub_stay_blocked() {
    let safe = WorkspaceSecurity::default();
    for values in [
        vec!["run", "bin/server.dart"],
        vec!["run", "tool/codegen.dart", "--watch=false"],
        vec!["analyze", "lib"],
        vec!["test", "test/widget_test.dart"],
        vec!["format", "lib", "test"],
        vec!["compile", "exe", "bin/server.dart", "-o", "build/server"],
        vec!["fix", "--dry-run"],
        vec!["fix", "--apply"],
        vec!["pub", "get"],
        vec!["pub", "upgrade"],
        vec!["pub", "outdated"],
        vec!["devtools"],
        vec!["pub", "workspace", "list"],
    ] {
        assert!(
            validate_command_policy("dart", &args(&values), safe).is_ok(),
            "routine Dart development unexpectedly required authorization: {values:?}"
        );
    }
    for values in [
        vec!["pub", "publish"],
        vec!["pub", "global", "activate", "melos"],
        vec!["pub", "token", "add", "https://pub.dev"],
        vec!["pub", "cache", "repair"],
    ] {
        assert!(
            validate_command_policy("dart", &args(&values), safe).is_err(),
            "remote/global Dart operation unexpectedly bypassed authorization: {values:?}"
        );
    }
    assert!(command_requires_workspace_write(
        "dart",
        &args(&["run", "bin/server.dart"])
    ));
    assert!(command_requires_workspace_write(
        "dart",
        &args(&["format", "lib"])
    ));
    assert!(!command_requires_workspace_write(
        "dart",
        &args(&["format", "-o", "none", "--set-exit-if-changed", "."])
    ));
    assert!(command_requires_workspace_write(
        "dart",
        &args(&["pub", "get"])
    ));
    assert!(!command_requires_workspace_write(
        "dart",
        &args(&["pub", "outdated"])
    ));
}

#[test]
fn every_supported_language_has_an_autonomous_development_tool_path() {
    let safe = WorkspaceSecurity::default();
    let cases = [
        ("bash", "shellcheck", vec!["--format=json", "script.sh"]),
        (
            "c",
            "clang-format",
            vec!["--dry-run", "--Werror", "src/main.c"],
        ),
        ("cpp", "clang-tidy", vec!["src/main.cpp", "-p", "."]),
        ("csharp", "dotnet", vec!["test", "--no-restore"]),
        ("css", "stylelint", vec!["**/*.css", "--formatter", "json"]),
        ("dart", "dart", vec!["run", "tool.dart"]),
        ("elixir", "mix", vec!["test"]),
        ("go", "go", vec!["test", "./..."]),
        ("html", "eslint", vec![".", "--format", "json"]),
        ("java", "mvn", vec!["test"]),
        ("javascript", "npm", vec!["run", "test"]),
        ("lua", "busted", vec![]),
        ("ocaml", "dune", vec!["runtest"]),
        ("ocaml-interface", "dune", vec!["build"]),
        ("php", "phpunit", vec![]),
        ("python", "pytest", vec!["-q"]),
        (
            "r",
            "Rscript",
            vec!["--vanilla", "-e", "testthat::test_local()"],
        ),
        ("ruby", "bundle", vec!["exec", "rspec"]),
        ("rust", "cargo", vec!["test"]),
        ("swift", "swift", vec!["test"]),
        ("typescript", "tsc", vec!["--noEmit"]),
        ("tsx", "npm", vec!["run", "typecheck"]),
    ];
    assert_eq!(cases.len(), SemanticLanguage::ALL.len());
    for (language, program, values) in cases {
        assert!(
            validate_command_policy(program, &args(&values), safe).is_ok(),
            "{language} development command unexpectedly requires authorization: {program} {values:?}"
        );
    }
    assert!(validate_command_policy("biome", &args(&["format", ".", "--write"]), safe).is_ok());
    assert!(validate_command_policy("prettier", &args(&[".", "--write"]), safe).is_ok());
    assert!(validate_command_policy("vitest", &args(&["watch"]), safe).is_ok());
    assert!(validate_command_policy("jest", &args(&["--watch"]), safe).is_ok());
    for (program, values) in [
        (
            "biome",
            vec![
                "check",
                ".",
                if cfg!(windows) {
                    r"--config-path=C:\biome.json"
                } else {
                    "--config-path=/tmp/biome.json"
                },
            ],
        ),
        (
            "prettier",
            vec![
                ".",
                "--check",
                "--config",
                if cfg!(windows) {
                    r"C:\prettier.json"
                } else {
                    "/tmp/prettier.json"
                },
            ],
        ),
        ("Rscript", vec!["-e", "testthat::test_local()"]),
        (
            "Rscript",
            vec!["--vanilla", "-e", "system('curl example.com')"],
        ),
        (
            "cargo-fuzz",
            vec!["fuzz", "run", "../target", "--", "-max_total_time=5"],
        ),
        (
            "clang-format",
            vec!["--style=file:/tmp/other", "src/main.c"],
        ),
    ] {
        assert!(validate_command_policy(program, &args(&values), safe).is_err());
    }
}

#[test]
fn deno_workspace_write_classification_covers_lock_snapshots_and_reports() {
    assert!(!command_requires_workspace_write(
        "deno",
        &args(&["fmt", "--check"])
    ));
    assert!(!command_requires_workspace_write("deno", &args(&["lint"])));
    assert!(!command_requires_workspace_write(
        "deno",
        &args(&["check", "--frozen", "."])
    ));
    assert!(!command_requires_workspace_write(
        "deno",
        &args(&["test", "--frozen"])
    ));
    assert!(command_requires_workspace_write("deno", &args(&["check"])));
    assert!(command_requires_workspace_write(
        "deno",
        &args(&["run", "main.ts"])
    ));
    assert!(command_requires_workspace_write(
        "deno",
        &args(&["lint", "--fix"])
    ));
    assert!(command_requires_workspace_write(
        "deno",
        &args(&["test", "--frozen", "--update-snapshots"])
    ));
    assert!(command_requires_workspace_write(
        "deno",
        &args(&["test", "--frozen", "--coverage=coverage"])
    ));
    assert!(command_requires_workspace_write(
        "deno",
        &args(&["task", "test"])
    ));
}

#[test]
fn smoke_named_repository_targets_are_autonomous_without_opening_release_paths() {
    let safe = WorkspaceSecurity::default();
    for target in [
        "smoke",
        "smoke-test",
        "test-smoke",
        "backend-smoke",
        "api_smoke",
        "release-smoke",
        "install-smoke",
        "production-smoke",
        "clean-smoke",
    ] {
        assert!(
            validate_command_policy("make", &args(&[target]), safe).is_ok(),
            "safe smoke target unexpectedly required authorization: {target}"
        );
    }
    for target in [
        "publish-smoke",
        "deploy-smoke",
        "upload-smoke",
        "secret-smoke",
        "token-smoke",
        "login-smoke",
    ] {
        assert!(
            validate_command_policy("make", &args(&[target]), safe).is_err(),
            "sensitive smoke-shaped target unexpectedly bypassed authorization: {target}"
        );
    }
}

#[test]
fn make_allows_bounded_local_development_flags_and_checks_every_target() {
    let safe = WorkspaceSecurity::default();
    for values in [
        vec![],
        vec!["dev"],
        vec!["-j8", "dev"],
        vec!["--jobs=8", "test"],
        vec!["-j", "8", "backend-test", "lint"],
        vec!["--no-print-directory", "docs-check"],
        vec!["CI=1", "NO_COLOR=1", "preflight"],
        vec!["PORT=8765", "server"],
        vec!["-C", "crate", "unit-test"],
        vec!["--directory=crate", "integration-test"],
        vec!["--file=Makefile.dev", "frontend-build"],
        vec!["internal-proto-v7"],
        vec!["clean"],
        vec!["test", "release"],
    ] {
        assert!(
            validate_command_policy("make", &args(&values), safe).is_ok(),
            "bounded local make invocation unexpectedly required authorization: {values:?}"
        );
    }
    for values in [
        vec!["-j8", "deploy"],
        vec!["--jobs=0", "test"],
        vec!["--jobs=9999", "test"],
        vec!["CC=sh", "test"],
        vec!["test", "publish-smoke"],
        vec!["--eval", "value:=unsafe", "test"],
        vec!["-C", "../other", "test"],
    ] {
        assert!(
            validate_command_policy("make", &args(&values), safe).is_err(),
            "sensitive or unbounded make invocation unexpectedly bypassed authorization: {values:?}"
        );
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

    assert!(validate_repository_runner("just", &args(&["check"]), false).is_ok());
    assert!(validate_repository_runner("task", &args(&["test"]), false).is_ok());
    assert!(validate_repository_runner("just", &args(&["dev"]), false).is_ok());
    assert!(validate_repository_runner("task", &args(&["codegen"]), false).is_ok());
    assert!(validate_repository_runner("just", &args(&["deploy"]), false).is_err());
    assert!(validate_uv_command(&args(&["lock", "--check"]), false).is_ok());
    assert!(validate_uv_command(&args(&["tree", "--locked"]), false).is_ok());
    assert!(validate_uv_command(&args(&["run", "--locked", "pytest"]), false).is_ok());
    assert!(validate_uv_command(&args(&["run", "tool.py"]), false).is_ok());
    assert!(validate_uv_command(&args(&["run", "custom-script"]), false).is_ok());
    assert!(validate_uv_command(&args(&["auth", "login"]), true).is_err());

    assert!(validate_ruff_command(&args(&["check", "."]), false).is_ok());
    assert!(validate_ruff_command(&args(&["check", "--fix", "."]), false).is_ok());
    assert!(validate_ruff_command(&args(&["format", "--check", "."]), false).is_ok());
    assert!(validate_biome_command(&args(&["ci", "."]), false).is_ok());
    assert!(validate_biome_command(&args(&["check", "--write", "."]), false).is_ok());
    assert!(validate_biome_command(
        &args(&["check", ".", "--config-path=/tmp/other.json"]),
        false
    )
    .is_err());
    assert!(validate_deno_command(&args(&["lint"]), false).is_ok());
    assert!(validate_deno_command(&args(&["fmt", "--check"]), false).is_ok());
    assert!(validate_deno_command(&args(&["run", "main.ts"]), false).is_ok());
    for values in [
        vec!["run", "--allow-all", "main.ts"],
        vec!["run", "-A", "main.ts"],
        vec!["test", "--watch"],
        vec!["audit", "--fix"],
        vec!["check", "--config", "../deno.json", "."],
        vec!["eval", "console.log('unsafe')"],
    ] {
        assert!(
            validate_deno_command(&args(&values), false).is_err(),
            "expanded Deno execution unexpectedly bypassed risky authorization: {values:?}"
        );
    }

    assert!(validate_docker_command(&args(&["compose", "config"]), false).is_ok());
    assert!(validate_docker_command(&args(&["compose", "up", "-d"]), false).is_ok());
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
    assert!(validate_dotnet_command(&args(&["test", "--no-restore"]), false).is_ok());
    assert!(validate_dotnet_command(&args(&["tool", "install", "x"]), false).is_err());
    assert!(validate_dotnet_command(&args(&["tool", "install", "x"]), true).is_ok());
    for program in [
        "cmake",
        "ninja",
        "mvn",
        "gradle",
        "swift",
        "zig",
        "pre-commit",
    ] {
        assert!(validate_known_project_runner(program, &args(&["check"]), false).is_ok());
    }
    assert!(validate_known_project_runner("act", &args(&["check"]), false).is_ok());
    for (program, values) in [
        ("mvn", vec!["deploy"]),
        ("gradle", vec!["publish"]),
        ("swift", vec!["sdk", "list"]),
        ("act", vec!["--privileged"]),
    ] {
        assert!(validate_known_project_runner(program, &args(&values), false).is_err());
        assert!(validate_known_project_runner(program, &args(&values), true).is_ok());
    }

    assert!(validate_cargo_command(&args(&["nextest", "run"]), false).is_ok());
    assert!(validate_cargo_command(&args(&["nextest", "run", "--locked"]), false).is_ok());
    assert!(validate_cargo_command(&args(&["nextest", "run", "name(test)"]), false).is_ok());
    assert!(validate_cargo_command(&args(&["nextest", "archive"]), false).is_ok());
    assert!(validate_git_command(&args(&["lfs", "status"]), false).is_ok());
    assert!(validate_git_command(&args(&["lfs", "push", "origin", "main"]), false).is_err());
    assert!(validate_package_command("npm", &args(&["test"]), false).is_ok());
    assert!(validate_package_command("npm", &args(&["ci"]), false).is_ok());
    assert!(
        validate_package_command("pnpm", &args(&["install", "--frozen-lockfile"]), false).is_ok()
    );
    assert!(validate_package_command("npm", &args(&["install", "left-pad"]), false).is_ok());
    assert!(validate_package_command("pnpm", &args(&["run", "build"]), false).is_ok());
    assert!(validate_package_command("yarn", &args(&["run", "lint"]), false).is_ok());
    assert!(validate_package_command("npm", &args(&["run", "deploy"]), false).is_err());
    assert!(validate_python_command(&args(&["-m", "pytest", "-q"]), false).is_ok());
    assert!(validate_python_command(&args(&["-m", "unittest"]), false).is_ok());
    assert!(validate_python_command(&args(&["-m", "http.server", "8000"]), false).is_ok());
    assert!(validate_python_command(&args(&["-c", "print('x')"]), false).is_err());
    assert!(validate_node_command(&args(&["--test"]), false).is_ok());
    assert!(validate_node_command(&args(&["--check", "index.js"]), false).is_ok());
    assert!(validate_node_command(&args(&["index.js"]), false).is_ok());
    assert!(validate_node_command(&args(&["--watch", "index.js"]), false).is_ok());
    assert!(validate_node_command(&args(&["--inspect", "server.ts"]), false).is_ok());
    assert!(validate_node_command(&args(&["-e", "process.exit(0)"]), false).is_err());
    assert!(validate_cargo_command(&args(&["fetch", "--locked"]), false).is_ok());
    assert!(validate_cargo_command(&args(&["update"]), false).is_ok());
    assert!(validate_cargo_command(&args(&["run"]), false).is_ok());
    assert!(validate_cargo_command(&args(&["add", "serde"]), false).is_ok());
    assert!(validate_go_command(&args(&["mod", "download"]), false).is_ok());
    assert!(validate_go_command(&args(&["mod", "tidy"]), false).is_ok());
    assert!(validate_go_command(&args(&["generate", "./..."]), false).is_ok());
    assert!(validate_go_command(&args(&["env", "GOMODCACHE"]), false).is_ok());
    assert!(validate_go_command(&args(&["env", "-w", "GOTOOLCHAIN=auto"]), false).is_err());
    assert!(validate_go_command(&args(&["env", "-w", "GOTOOLCHAIN=auto"]), true).is_ok());
    assert!(validate_git_command(&args(&["lfs", "push", "origin", "main"]), true).is_ok());
    assert!(validate_git_command(&args(&["lfs", "push", "--all", "origin"]), true).is_err());
}

#[test]
fn audit_git_literal_messages_remain_text_without_triggering_path_guards() {
    for command in [
        vec!["commit", "-m", "../migration note"],
        vec!["commit", "--message=.env"],
        vec!["commit", "-m", "/healthz endpoint"],
        vec!["tag", "-a", "v-test", "-m", ".env"],
    ] {
        assert!(
            validate_command_policy("git", &args(&command), WorkspaceSecurity::default()).is_ok()
        );
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
    let safe = WorkspaceSecurity::default();
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
    assert!(
        validate_command_policy("git", &args(&["grep", "needle", "--", ".env"]), safe).is_err()
    );
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
    assert!(!workspace.security.allow_risky_exec);
}

#[test]
fn git_mutations_require_exact_risky_authorization_and_keep_hard_boundaries() {
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
