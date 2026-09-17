use super::*;

#[path = "command_policy/autonomy.rs"]
mod autonomy;
#[path = "command_policy/dev_tools.rs"]
mod dev_tools;
#[path = "command_policy/focused.rs"]
mod focused;
#[path = "command_policy/git.rs"]
mod git;
#[path = "command_policy/github.rs"]
mod github;
#[path = "command_policy/infrastructure.rs"]
mod infrastructure;
#[path = "command_policy/language_tools.rs"]
mod language_tools;
#[path = "command_policy/project_tools.rs"]
mod project_tools;
pub(super) use autonomy::command_requires_workspace_write;
use autonomy::{
    validate_bundle_command, validate_dart_command, validate_dune_command,
    validate_flutter_command, validate_mix_command, validate_node_command,
    validate_php_quality_command, validate_python_command,
};
use dev_tools::{validate_fd_command, validate_jq_command};
use git::validate_git_command;
use github::validate_gh_command;
use infrastructure::{
    validate_docker_command, validate_kubectl_command, validate_terraform_command,
};
use project_tools::{
    validate_biome_command, validate_deno_command, validate_dotnet_command,
    validate_generic_project_tool, validate_known_project_runner, validate_language_dev_tool,
    validate_local_http_probe, validate_make_command, validate_repository_runner,
    validate_ruff_command, validate_uv_command,
};

pub(super) fn validate_authorizable_program(program: &str) -> Result<()> {
    if program.is_empty() || program.len() > 256 || program.trim() != program {
        bail!("command program name is invalid");
    }
    if program.contains(['\0', '\n', '\r', '/', '\\', ':'])
        || program.chars().any(char::is_whitespace)
        || Path::new(program).is_absolute()
    {
        bail!("command program must be a bare executable name without path traversal or control characters");
    }
    let normalized = program.to_ascii_lowercase();
    if matches!(
        normalized.as_str(),
        "sh" | "bash"
            | "zsh"
            | "fish"
            | "dash"
            | "ksh"
            | "csh"
            | "tcsh"
            | "pwsh"
            | "powershell"
            | "powershell.exe"
            | "cmd"
            | "cmd.exe"
            | "command.com"
            | "wscript"
            | "wscript.exe"
            | "cscript"
            | "cscript.exe"
    ) {
        bail!("shell interpreters are permanently blocked by the no-shell execution boundary");
    }
    Ok(())
}

pub(super) fn validate_command_policy(
    program: &str,
    args: &[String],
    security: WorkspaceSecurity,
) -> Result<()> {
    validate_authorizable_program(program)?;
    validate_command_arguments(program, args)?;
    if matches!(args, [flag] if matches!(flag.as_str(), "--version" | "-V" | "version")) {
        return Ok(());
    }
    match program {
        "git" => validate_git_command(args, security.allow_risky_exec),
        "gh" => validate_gh_command(args, security.allow_risky_exec),
        "rg" => validate_rg_command(args),
        "cargo" => validate_cargo_command(args, security.allow_risky_exec),
        "go" => validate_go_command(args, security.allow_risky_exec),
        "npm" | "pnpm" | "yarn" | "bun" => {
            validate_package_command(program, args, security.allow_risky_exec)
        }
        "just" | "task" => validate_repository_runner(program, args, security.allow_risky_exec),
        "bazel" | "bazelisk" | "buck2" | "pants" | "meson" | "ctest" | "sbt" | "lein"
        | "rebar3" | "poetry" | "pdm" | "hatch" | "tox" | "nox" | "nx" | "turbo" | "vite"
        | "webpack" | "rollup" | "esbuild" | "tsup" | "parcel" | "rspack" | "rolldown" | "rake"
        | "cabal" | "stack" => {
            validate_generic_project_tool(program, args, security.allow_risky_exec)
        }
        "uv" => validate_uv_command(args, security.allow_risky_exec),
        "ruff" => validate_ruff_command(args, security.allow_risky_exec),
        "biome" => validate_biome_command(args, security.allow_risky_exec),
        "deno" => validate_deno_command(args, security.allow_risky_exec),
        "dart" => validate_dart_command(args, security.allow_risky_exec),
        "flutter" => validate_flutter_command(args, security.allow_risky_exec),
        "mix" => validate_mix_command(args, security.allow_risky_exec),
        "dune" => validate_dune_command(args, security.allow_risky_exec),
        "bundle" => validate_bundle_command(args, security.allow_risky_exec),
        "phpstan" | "psalm" | "phpunit" | "php-cs-fixer" => {
            validate_php_quality_command(program, args, security.allow_risky_exec)
        }
        "docker" => validate_docker_command(args, security.allow_risky_exec),
        "kubectl" => validate_kubectl_command(args, security.allow_risky_exec),
        "terraform" => validate_terraform_command(args, security.allow_risky_exec),
        "fd" => validate_fd_command(args),
        "jq" => validate_jq_command(args),
        "dotnet" => validate_dotnet_command(args, security.allow_risky_exec),
        "cmake" | "ninja" | "mvn" | "gradle" | "swift" | "zig" | "pre-commit" | "act" => {
            validate_known_project_runner(program, args, security.allow_risky_exec)
        }
        "pytest" => Ok(()),
        "shellcheck" | "shfmt" | "stylua" | "luacheck" | "busted" | "clang-format"
        | "clang-tidy" | "cargo-audit" | "cargo-mutants" | "cargo-fuzz" | "gofmt"
        | "staticcheck" | "govulncheck" | "mypy" | "pyright" | "bandit" | "eslint" | "tsc"
        | "stylelint" | "Rscript" | "swift-format" | "swiftlint" | "mutmut" | "dotnet-stryker"
        | "infection" | "muter" => {
            validate_language_dev_tool(program, args, security.allow_risky_exec)
        }
        "python3" => validate_python_command(args, security.allow_risky_exec),
        "node" => validate_node_command(args, security.allow_risky_exec),
        "curl" => validate_local_http_probe(args, security.allow_risky_exec),
        "make" => validate_make_command(args, security.allow_risky_exec),
        "rustc" => Ok(()),
        _ if LANGUAGE_DEVELOPMENT_COMMANDS.contains(&program) => {
            language_tools::validate_language_development_tool(program, args)
        }
        _ => require_risky_exec(
            "user-authorized external command",
            security.allow_risky_exec,
        ),
    }
}

fn project_target_argument(program: &str, value: &str) -> Result<bool> {
    if !matches!(program, "bazel" | "bazelisk" | "buck2") {
        return Ok(false);
    }
    let local = if let Some(path) = value.strip_prefix("//") {
        Some(path)
    } else if value.starts_with('@') && value.contains("//") {
        // External repository labels are resolved by the repository's checked-in
        // build configuration rather than as host filesystem paths.
        return Ok(true);
    } else {
        None
    };
    let Some(local) = local else { return Ok(false) };
    let path = local
        .split(':')
        .next()
        .unwrap_or_default()
        .trim_end_matches("/...");
    if !path.is_empty() {
        reject_protected_path(Path::new(path))?;
    }
    Ok(true)
}

pub(super) fn validate_command_arguments(program: &str, args: &[String]) -> Result<()> {
    let literal_messages = if program == "git" {
        git::literal_message_indices(args)
    } else {
        Vec::new()
    };
    let rg_pattern_index = (program == "rg")
        .then(|| args.iter().position(|arg| !arg.starts_with('-')))
        .flatten();
    for (index, arg) in args.iter().enumerate() {
        if arg.contains('\0') || arg.contains(['\n', '\r']) {
            bail!("command arguments contain forbidden control characters");
        }
        if literal_messages.contains(&index) {
            continue;
        }
        if program == "rustc" && arg.contains('@') {
            bail!(
                "rustc response-file arguments are blocked because they bypass argument inspection"
            );
        }
        let value = arg.split_once('=').map(|(_, value)| value).unwrap_or(arg);
        if value.starts_with("file://") {
            bail!("file:// arguments are blocked");
        }
        if value.starts_with("http://") || value.starts_with("https://") {
            let parsed = url::Url::parse(value)
                .map_err(|error| anyhow!("invalid URL command argument: {error}"))?;
            if !parsed.username().is_empty() || parsed.password().is_some() {
                bail!("URL command arguments must not embed credentials");
            }
            if parsed.fragment().is_some() {
                bail!("URL command arguments must not contain fragments");
            }
            continue;
        }
        if rg_pattern_index == Some(index) || project_target_argument(program, value)? {
            continue;
        }
        reject_protected_command_argument(value)?;
        let windows_absolute = value.len() >= 3
            && value.as_bytes()[1] == b':'
            && matches!(value.as_bytes()[2], b'\\' | b'/');
        let parent_component = value.split(['/', '\\']).any(|component| component == "..");
        if Path::new(value).is_absolute() || windows_absolute || parent_component {
            bail!("command argument may escape the selected workspace: {arg}");
        }
    }
    Ok(())
}

fn reject_protected_command_argument(value: &str) -> Result<()> {
    let mut candidates = vec![value];
    if let Some((_, suffix)) = value.rsplit_once(':') {
        if !suffix.is_empty() {
            candidates.push(suffix);
        }
    }
    for candidate in candidates {
        let candidate = candidate
            .trim_start_matches(":(glob)")
            .trim_start_matches(":(literal)")
            .trim_start_matches(":(top)")
            .trim_start_matches(':');
        if candidate.is_empty() || candidate.starts_with('-') {
            continue;
        }
        if let Err(error) = reject_protected_path(Path::new(candidate)) {
            bail!("command argument targets a protected path: {value} ({error})");
        }
    }
    Ok(())
}

fn validate_rg_command(args: &[String]) -> Result<()> {
    for arg in args {
        if matches!(
            arg.as_str(),
            "--pre"
                | "--pre-glob"
                | "-L"
                | "--follow"
                | "--hidden"
                | "-u"
                | "-uu"
                | "-uuu"
                | "--no-ignore"
                | "--no-ignore-vcs"
                | "--no-ignore-dot"
                | "--no-ignore-global"
                | "--no-ignore-parent"
                | "--no-ignore-files"
                | "-f"
                | "--file"
                | "--ignore-file"
                | "-g"
                | "--glob"
                | "--iglob"
                | "--type-add"
                | "--type-clear"
        ) || arg.starts_with("--pre=")
            || arg.starts_with("--pre-glob=")
            || arg.starts_with("--file=")
            || arg.starts_with("--ignore-file=")
            || arg.starts_with("--glob=")
            || arg.starts_with("--iglob=")
            || arg.starts_with("--type-add=")
            || arg.starts_with("--type-clear=")
            || (arg.starts_with("-f") && arg.len() > 2)
            || (arg.starts_with("-g") && arg.len() > 2)
        {
            bail!("ripgrep option is blocked because it can read helper files or bypass protected paths: {arg}");
        }
    }
    Ok(())
}

fn args_equal(args: &[String], expected: &[&str]) -> bool {
    args.len() == expected.len()
        && args
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual == expected)
}

pub(super) fn validate_verification_command_shape(program: &str, args: &[String]) -> Result<()> {
    let exact = match program {
        "git" => args_equal(args, &["diff", "--check"]),
        "cargo" => {
            args_equal(args, &["nextest", "run"])
                || args_equal(args, &["nextest", "run", "--locked"])
        }
        "go" => args_equal(args, &["vet", "./..."]) || args_equal(args, &["test", "./..."]),
        "pytest" => args_equal(args, &["-q"]),
        "mvn" => args_equal(args, &["-q", "-DskipTests", "compile"]) || args_equal(args, &["test"]),
        "gradle" => args_equal(args, &["classes"]) || args_equal(args, &["check"]),
        "swift" => args_equal(args, &["build"]) || args_equal(args, &["test"]),
        "dotnet" => {
            args_equal(args, &["format", "--verify-no-changes", "--no-restore"])
                || args_equal(args, &["build", "--no-restore"])
                || args_equal(args, &["test", "--no-restore"])
        }
        "dart" => {
            args_equal(args, &["analyze"])
                || args_equal(args, &["test"])
                || args_equal(
                    args,
                    &["format", "-o", "none", "--set-exit-if-changed", "."],
                )
        }
        "deno" => {
            args_equal(args, &["fmt", "--check"])
                || args_equal(args, &["lint"])
                || args_equal(args, &["check", "--frozen", "."])
                || args_equal(args, &["test", "--frozen"])
                || args_equal(args, &["audit", "--frozen"])
        }
        "mix" => {
            args_equal(args, &["format", "--check-formatted"])
                || args_equal(args, &["compile", "--warnings-as-errors"])
                || args_equal(args, &["test"])
        }
        "dune" => args_equal(args, &["build"]) || args_equal(args, &["runtest"]),
        "bundle" => {
            args_equal(args, &["exec", "rubocop", "--format", "json"])
                || args_equal(args, &["exec", "standardrb", "--format", "json"])
                || args_equal(args, &["exec", "rspec"])
        }
        "phpstan" | "vendor/bin/phpstan" => args_equal(args, &["analyse", "--error-format=json"]),
        "psalm" | "vendor/bin/psalm" => args_equal(args, &["--output-format=json"]),
        "phpunit" | "vendor/bin/phpunit" => args.is_empty(),
        "php-cs-fixer" | "vendor/bin/php-cs-fixer" => {
            args_equal(args, &["fix", "--dry-run", "--diff"])
        }
        "composer" => args_equal(args, &["audit", "--locked", "--format=json"]),
        program if node_quality_program(program, "biome") => {
            args_equal(args, &["format", ".", "--reporter=json"])
                || args_equal(
                    args,
                    &[
                        "check",
                        ".",
                        "--formatter-enabled=false",
                        "--assist-enabled=false",
                        "--reporter=json",
                    ],
                )
        }
        program if node_quality_program(program, "prettier") => args_equal(args, &[".", "--check"]),
        program if node_quality_program(program, "vitest") => args_equal(args, &["run"]),
        program if node_quality_program(program, "jest") => args_equal(args, &["--runInBand"]),
        program if node_quality_program(program, "htmlhint") => {
            args_equal(args, &["**/*.html", "--format", "json"])
        }
        "bats" => {
            !args.is_empty()
                && args.len() <= 128
                && args
                    .iter()
                    .all(|arg| !arg.starts_with('-') && arg.to_ascii_lowercase().ends_with(".bats"))
        }
        "Rscript" => matches!(
            args,
            [vanilla, flag, expr]
                if vanilla == "--vanilla"
                    && flag == "-e"
                    && matches!(
                        expr.as_str(),
                        "quit(status=if(length(lintr::lint_package()))1 else 0)"
                            | "styler::style_pkg(dry=\"fail\")"
                            | "testthat::test_local()"
                            | "testthat::test_dir('tests/testthat')"
                    )
        ),
        "make" => args.len() == 1 && matches!(args[0].as_str(), "check" | "lint" | "test"),
        "npm" | "pnpm" | "yarn" | "bun" => {
            args.len() == 2
                && args[0] == "run"
                && matches!(
                    args[1].as_str(),
                    "lint" | "typecheck" | "check" | "format:check" | "test" | "build"
                )
        }
        _ => false,
    };
    let allowed = exact
        || bounded_common_verification_command(program, args)
        || focused::bounded_test_filter(program, args);
    if allowed {
        Ok(())
    } else {
        bail!(
            "command is not an approved inferred verification shape: {}",
            format_command(program, args)
        )
    }
}

fn node_quality_program(program: &str, name: &str) -> bool {
    program == name || program.strip_prefix("node_modules/.bin/") == Some(name)
}

fn bounded_common_verification_command(program: &str, args: &[String]) -> bool {
    match program {
        "cargo" => bounded_cargo_verification(args),
        "flutter" => autonomy::safe_flutter_verification(args),
        _ => false,
    }
}

fn bounded_cargo_verification(args: &[String]) -> bool {
    let Some((subcommand, tail)) = args.split_first() else {
        return false;
    };
    match subcommand.as_str() {
        "test" => tail.iter().all(|arg| {
            matches!(
                arg.as_str(),
                "--locked" | "--workspace" | "--all" | "--all-targets" | "--no-fail-fast"
            )
        }),
        "check" => tail.iter().all(|arg| {
            matches!(
                arg.as_str(),
                "--locked" | "--workspace" | "--all" | "--all-targets"
            )
        }),
        "build" => tail
            .iter()
            .all(|arg| matches!(arg.as_str(), "--locked" | "--workspace" | "--release")),
        "fmt" => args_equal(tail, &["--check"]) || args_equal(tail, &["--all", "--", "--check"]),
        "clippy" => {
            let Some(separator) = tail.iter().position(|arg| arg == "--") else {
                return false;
            };
            tail[..separator]
                .iter()
                .all(|arg| matches!(arg.as_str(), "--locked" | "--workspace" | "--all-targets"))
                && args_equal(&tail[separator + 1..], &["-D", "warnings"])
        }
        _ => false,
    }
}

fn format_command(program: &str, args: &[String]) -> String {
    if args.is_empty() {
        program.to_owned()
    } else {
        format!("{program} {}", args.join(" "))
    }
}

fn is_default_safe_cargo_command(args: &[String]) -> bool {
    args_equal(args, &["fmt", "--check"])
        || args_equal(args, &["check"])
        || args_equal(args, &["check", "--locked"])
        || args_equal(args, &["metadata", "--no-deps"])
        || args_equal(args, &["metadata", "--no-deps", "--format-version", "1"])
        || args_equal(args, &["metadata", "--format-version", "1", "--no-deps"])
        || args_equal(args, &["clippy", "--", "-D", "warnings"])
        || args_equal(args, &["clippy", "--locked", "--", "-D", "warnings"])
        || args_equal(args, &["clippy", "--all-targets", "--", "-D", "warnings"])
        || args_equal(
            args,
            &[
                "clippy",
                "--locked",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        )
}

fn validate_cargo_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    for arg in args {
        if matches!(
            arg.as_str(),
            "--config" | "--manifest-path" | "--target-dir" | "-C"
        ) || arg.starts_with("--config=")
            || arg.starts_with("--manifest-path=")
            || arg.starts_with("--target-dir=")
        {
            bail!("cargo option is blocked because it can redirect configuration or filesystem access: {arg}");
        }
    }
    if is_default_safe_cargo_command(args) {
        return Ok(());
    }
    let subcommand = args
        .iter()
        .find(|arg| !arg.starts_with('-'))
        .map(String::as_str)
        .ok_or_else(|| anyhow!("cargo subcommand is required"))?;
    match subcommand {
        "metadata" | "fmt" | "check" | "test" | "clippy" | "build" | "fetch" | "update" | "run"
        | "bench" | "doc" | "add" | "remove" | "new" | "init" | "package" => Ok(()),
        "clean" => require_risky_exec("cargo clean", allow_risky_exec),
        "nextest" => {
            let index = args
                .iter()
                .position(|arg| !arg.starts_with('-'))
                .expect("cargo subcommand already resolved");
            let action = args
                .get(index + 1)
                .map(String::as_str)
                .ok_or_else(|| anyhow!("cargo nextest action is required"))?;
            if !matches!(action, "run" | "list" | "archive") {
                bail!("cargo nextest {action} is blocked by the bounded cargo-nextest policy");
            }
            Ok(())
        }
        "login" | "logout" => {
            bail!(
                "cargo {subcommand} is blocked because credential flows must remain operator-owned"
            )
        }
        "owner" | "publish" | "yank" => {
            bail!("cargo {subcommand} is blocked because registry publication/ownership must remain operator-owned")
        }
        "install" | "uninstall" => require_risky_exec(
            &format!("cargo host tool operation: {subcommand}"),
            allow_risky_exec,
        ),
        _ if project_tools::is_sensitive_project_task(subcommand) => require_risky_exec(
            &format!("cargo externally consequential project operation: {subcommand}"),
            allow_risky_exec,
        ),
        _ => Ok(()),
    }
}

fn validate_go_command(args: &[String], _allow_risky_exec: bool) -> Result<()> {
    for arg in args {
        if matches!(
            arg.as_str(),
            "-C" | "-exec" | "-toolexec" | "-overlay" | "-modfile"
        ) || arg.starts_with("-C=")
            || arg.starts_with("-exec=")
            || arg.starts_with("-toolexec=")
            || arg.starts_with("-overlay=")
            || arg.starts_with("-modfile=")
        {
            bail!("go option is blocked because it can redirect execution or filesystem access: {arg}");
        }
    }
    let subcommand = args.first().map(String::as_str).unwrap_or_default();
    match subcommand {
        "list"
            if matches!(args, [command] if command == "list")
                || matches!(args, [command, target] if command == "list" && target == "./...") =>
        {
            Ok(())
        }
        "env"
            if args.iter().any(|arg| {
                matches!(arg.as_str(), "-w" | "-u")
                    || arg.starts_with("-w=")
                    || arg.starts_with("-u=")
            }) =>
        {
            require_risky_exec("go env host configuration mutation", _allow_risky_exec)
        }
        "install" | "telemetry" => {
            bail!("go {subcommand} is blocked because host-wide tool/telemetry changes must remain operator-owned")
        }
        "env" if args.len() == 1 => {
            bail!("go env without an explicit key is blocked because it exposes broad host configuration")
        }
        "list" | "test" | "vet" | "build" | "run" | "get" | "generate" | "clean" | "doc"
        | "env" | "tool" | "version" | "work" => Ok(()),
        "mod"
            if args.get(1).is_some_and(|action| {
                matches!(action.as_str(), "download" | "tidy" | "verify")
            }) =>
        {
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_package_command(program: &str, args: &[String], allow_risky_exec: bool) -> Result<()> {
    for arg in args {
        if matches!(
            arg.as_str(),
            "--prefix"
                | "--cwd"
                | "--dir"
                | "--global"
                | "-g"
                | "--userconfig"
                | "--config"
                | "--global-dir"
        ) || arg.starts_with("--prefix=")
            || arg.starts_with("--cwd=")
            || arg.starts_with("--dir=")
            || arg.starts_with("--userconfig=")
            || arg.starts_with("--config=")
            || arg.starts_with("--global-dir=")
        {
            bail!("{program} option is blocked because it can redirect configuration or filesystem access: {arg}");
        }
    }
    let subcommand = args.first().map(String::as_str).unwrap_or_default();
    if matches!(subcommand, "list" | "ls" | "why") {
        return Ok(());
    }
    if matches!(subcommand, "login" | "logout" | "token") {
        bail!(
            "{program} {subcommand} is blocked because credential flows must remain operator-owned"
        );
    }
    if matches!(subcommand, "publish" | "unpublish" | "owner") {
        bail!("{program} {subcommand} is blocked because registry publication/ownership must remain operator-owned");
    }
    if matches!(subcommand, "config" | "cache") {
        return require_risky_exec(
            &format!("{program} host configuration operation: {subcommand}"),
            allow_risky_exec,
        );
    }
    if matches!(
        subcommand,
        "test"
            | "build"
            | "lint"
            | "check"
            | "typecheck"
            | "ci"
            | "install"
            | "add"
            | "remove"
            | "uninstall"
            | "update"
            | "start"
    ) {
        return Ok(());
    }
    if subcommand == "run"
        && args
            .get(1)
            .is_some_and(|script| project_tools::is_autonomous_repository_task(script))
    {
        return Ok(());
    }
    if matches!(subcommand, "exec" | "dlx" | "x") {
        let Some(inner) = args.iter().skip(1).find(|arg| !arg.starts_with('-')) else {
            bail!("{program} {subcommand} requires a development executable or package");
        };
        return project_tools::validate_wrapped_development_program(inner);
    }
    if matches!(program, "yarn" | "bun" | "pnpm")
        && project_tools::is_autonomous_repository_task(subcommand)
    {
        return Ok(());
    }
    require_risky_exec(
        &format!("{program} user-authorized project operation: {subcommand}"),
        allow_risky_exec,
    )
}

fn require_risky_exec(label: &str, enabled: bool) -> Result<()> {
    if enabled {
        Ok(())
    } else {
        bail!(
            "{label} requires exact risky-operation authorization; approve the specific operation in the TUI/Web UI, or restart with --allow-risky-exec only for a trusted repository"
        )
    }
}

/// Only exact, bounded inspection shapes use the separate process queue.
/// This is consulted after policy checks, never as a grant or validation bypass.
pub(super) fn is_inspection_probe(program: &str, args: &[String]) -> bool {
    if program == "cargo" {
        return args_equal(args, &["fmt", "--check"])
            || args_equal(args, &["metadata", "--no-deps"])
            || args_equal(args, &["metadata", "--no-deps", "--format-version", "1"])
            || args_equal(args, &["metadata", "--format-version", "1", "--no-deps"]);
    }
    if program != "git" {
        return false;
    }
    match args {
        [one] => matches!(one.as_str(), "--version" | "status"),
        [first, second] => matches!(
            (first.as_str(), second.as_str()),
            ("status", "--short")
                | ("diff", "--check" | "--numstat")
                | ("branch", "--show-current")
                | (
                    "rev-parse",
                    "--show-toplevel" | "--show-prefix" | "--is-inside-work-tree"
                )
        ),
        [first, second, third] => matches!(
            (first.as_str(), second.as_str(), third.as_str()),
            ("status", "--short", "--branch" | "--untracked-files=all")
                | ("diff", "--cached", "--check" | "--numstat")
        ),
        _ => false,
    }
}

pub(super) fn hardened_command_args(program: &str, args: &[String]) -> Vec<String> {
    if program != "git" {
        return args.to_vec();
    }
    let Some(subcommand_index) = args.iter().position(|arg| !arg.starts_with('-')) else {
        return args.to_vec();
    };

    let null_path = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let is_push = args[subcommand_index] == "push"
        || (args[subcommand_index] == "lfs"
            && args
                .get(subcommand_index + 1)
                .is_some_and(|arg| arg == "push"));
    let mut overrides = vec![
        "core.fsmonitor=false".to_owned(),
        "core.untrackedCache=false".to_owned(),
        format!("core.hooksPath={null_path}"),
        format!("core.attributesFile={null_path}"),
        format!("core.excludesFile={null_path}"),
        "diff.external=".to_owned(),
        "commit.gpgSign=false".to_owned(),
        "tag.gpgSign=false".to_owned(),
        "credential.helper=".to_owned(),
        "core.askPass=".to_owned(),
        "core.gitProxy=".to_owned(),
        "http.extraHeader=".to_owned(),
        "protocol.ext.allow=never".to_owned(),
        "protocol.file.allow=never".to_owned(),
        "maintenance.auto=false".to_owned(),
        "gc.auto=0".to_owned(),
    ];
    overrides.push(if is_push {
        "core.sshCommand=ssh -oBatchMode=yes -oStrictHostKeyChecking=accept-new".to_owned()
    } else {
        "core.sshCommand=false".to_owned()
    });
    let extra_diff_args = if matches!(args[subcommand_index].as_str(), "diff" | "log" | "show") {
        2
    } else {
        0
    };
    let mut hardened = Vec::with_capacity(args.len() + overrides.len() * 2 + extra_diff_args);
    for override_value in overrides {
        hardened.push("-c".to_owned());
        hardened.push(override_value);
    }
    hardened.extend_from_slice(&args[..=subcommand_index]);
    if matches!(args[subcommand_index].as_str(), "diff" | "log" | "show") {
        hardened.push("--no-ext-diff".to_owned());
        hardened.push("--no-textconv".to_owned());
    }
    hardened.extend_from_slice(&args[subcommand_index + 1..]);
    hardened
}

pub(super) fn scrub_sensitive_environment(
    command: &mut Command,
    program: &str,
    args: &[String],
    allow_git_push_credentials: bool,
) {
    for (key, _) in std::env::vars() {
        let upper = key.to_ascii_uppercase();
        let gh_auth = program == "gh"
            && matches!(
                upper.as_str(),
                "GH_TOKEN" | "GITHUB_TOKEN" | "GH_ENTERPRISE_TOKEN"
            );
        let git_ssh_agent = program == "git"
            && allow_git_push_credentials
            && is_git_push_command(args)
            && upper == "SSH_AUTH_SOCK";
        let generic_secret = (program == "git" && upper.starts_with("GIT_"))
            || upper.contains("TOKEN")
            || upper.contains("SECRET")
            || upper.contains("PASSWORD")
            || upper.ends_with("_KEY")
            || upper.starts_with("AWS_")
            || upper.starts_with("AZURE_")
            || upper.starts_with("GOOGLE_")
            || upper.starts_with("GITHUB_")
            || upper.starts_with("GITLAB_")
            || matches!(
                upper.as_str(),
                "SSH_AUTH_SOCK" | "KUBECONFIG" | "DOCKER_CONFIG" | "NETRC" | "GIT_ASKPASS"
            );
        let tool_redirect = (program == "gh"
            && matches!(
                upper.as_str(),
                "GH_REPO" | "GH_HOST" | "GH_CONFIG_DIR" | "GH_EDITOR" | "GH_BROWSER"
            ))
            || (program == "docker"
                && matches!(
                    upper.as_str(),
                    "DOCKER_HOST" | "DOCKER_CONTEXT" | "DOCKER_CERT_PATH" | "DOCKER_TLS_VERIFY"
                ))
            || (program == "kubectl" && upper == "KUBECTL_EXTERNAL_DIFF")
            || (program == "terraform"
                && (upper.starts_with("TF_CLI_ARGS")
                    || upper.starts_with("TF_VAR_")
                    || matches!(
                        upper.as_str(),
                        "TF_CLI_CONFIG_FILE" | "TF_DATA_DIR" | "TF_WORKSPACE"
                    )))
            || (program == "uv"
                && matches!(
                    upper.as_str(),
                    "UV_PROJECT"
                        | "UV_WORKING_DIR"
                        | "UV_CONFIG_FILE"
                        | "UV_DEFAULT_INDEX"
                        | "UV_INDEX"
                        | "UV_INSECURE_HOST"
                        | "UV_KEYRING_PROVIDER"
                        | "UV_CACHE_DIR"
                ));
        if (!gh_auth && !git_ssh_agent && generic_secret) || tool_redirect {
            command.env_remove(key);
        }
    }
    command.env("NO_COLOR", "1");
    if program == "gh" {
        command
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_PAGER", "cat")
            .env("PAGER", "cat");
    }
    if program == "terraform" {
        command.env("TF_IN_AUTOMATION", "1");
    }
    if program == "git" {
        let null_config = if cfg!(windows) { "NUL" } else { "/dev/null" };
        command
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_SYSTEM", null_config)
            .env("GIT_CONFIG_GLOBAL", null_config)
            .env("GIT_ATTR_NOSYSTEM", "1")
            .env("GIT_PROTOCOL_FROM_USER", "0")
            .env("GIT_PAGER", "cat")
            .env("PAGER", "cat")
            .env("GIT_EDITOR", "false")
            .env("GIT_SEQUENCE_EDITOR", "false")
            .env("GIT_EXTERNAL_DIFF", "");
    }
}

pub(super) fn is_git_push_command(args: &[String]) -> bool {
    let Some(index) = args.iter().position(|arg| !arg.starts_with('-')) else {
        return false;
    };
    args[index] == "push"
        || (args[index] == "lfs" && args.get(index + 1).is_some_and(|arg| arg == "push"))
}

pub(super) async fn read_bounded_stream<R>(mut reader: R) -> std::io::Result<(String, bool)>
where
    R: AsyncRead + Unpin,
{
    let mut stored = Vec::with_capacity(MAX_OUTPUT_BYTES.min(16 * 1024));
    let mut buffer = [0u8; 8192];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = MAX_OUTPUT_BYTES.saturating_sub(stored.len());
        let keep = remaining.min(read);
        stored.extend_from_slice(&buffer[..keep]);
        truncated |= keep < read;
    }
    Ok((String::from_utf8_lossy(&stored).to_string(), truncated))
}

#[cfg(test)]
#[path = "../../tests/unit/workspace/policy_polyglot.rs"]
mod polyglot_tests;
#[cfg(test)]
#[path = "../../tests/unit/workspace/policy_relaxed.rs"]
mod relaxed_tests;
#[cfg(test)]
#[path = "../../tests/unit/workspace/policy_remote.rs"]
mod remote_tests;
#[cfg(test)]
#[path = "../../tests/unit/workspace/policy.rs"]
mod tests;
