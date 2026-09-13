use super::*;

pub(super) fn validate_python_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    match args {
        [flag, module, ..]
            if flag == "-m"
                && matches!(
                    module.as_str(),
                    "pytest" | "unittest" | "compileall" | "mypy" | "pyright" | "ruff"
                ) =>
        {
            Ok(())
        }
        _ => require_risky_exec("python interpreter execution", allow_risky_exec),
    }
}

pub(super) fn validate_node_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    if args.first().is_some_and(|arg| {
        arg == "--test" || arg.starts_with("--test=") || arg == "--check" || arg == "-c"
    }) {
        Ok(())
    } else {
        require_risky_exec("node interpreter execution", allow_risky_exec)
    }
}

pub(super) fn validate_dart_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    if args_equal(args, &["analyze"])
        || args_equal(args, &["test"])
        || args_equal(
            args,
            &["format", "-o", "none", "--set-exit-if-changed", "."],
        )
    {
        Ok(())
    } else {
        require_risky_exec("Dart project execution", allow_risky_exec)
    }
}

pub(super) fn validate_flutter_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    if safe_flutter_verification(args) {
        return Ok(());
    }
    if matches!(args, [first, second, ..] if first == "pub" && second == "publish") {
        bail!("flutter pub publish is blocked because it can mutate a remote package registry");
    }
    require_risky_exec("Flutter project execution", allow_risky_exec)
}

pub(super) fn safe_flutter_verification(args: &[String]) -> bool {
    match args {
        [command, rest @ ..] if command == "analyze" => rest.iter().all(|arg| arg == "--no-pub"),
        [command, rest @ ..] if command == "test" => rest.iter().all(|arg| {
            matches!(arg.as_str(), "--no-pub" | "--coverage")
                || safe_flutter_concurrency(arg)
                || matches!(
                    arg.strip_prefix("--reporter="),
                    Some("compact" | "expanded" | "json" | "github" | "silent")
                )
                || safe_flutter_file_reporter(arg)
        }),
        [command, target, rest @ ..]
            if command == "build"
                && matches!(
                    target.as_str(),
                    "web" | "apk" | "appbundle" | "ios" | "ipa" | "macos" | "linux" | "windows"
                ) =>
        {
            rest.iter().all(|arg| {
                matches!(
                    arg.as_str(),
                    "--release" | "--debug" | "--profile" | "--no-pub" | "--wasm"
                )
            })
        }
        _ => false,
    }
}

fn safe_flutter_concurrency(arg: &str) -> bool {
    arg.strip_prefix("--concurrency=")
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|value| (1..=64).contains(&value))
}

fn safe_flutter_file_reporter(arg: &str) -> bool {
    let Some(spec) = arg.strip_prefix("--file-reporter=") else {
        return false;
    };
    let Some((reporter, path)) = spec.split_once(':') else {
        return false;
    };
    if !matches!(
        reporter,
        "compact" | "expanded" | "json" | "github" | "silent"
    ) || path.is_empty()
        || Path::new(path).is_absolute()
        || path
            .split(['/', '\\'])
            .any(|component| matches!(component, "" | "." | ".."))
    {
        return false;
    }
    reject_protected_path(Path::new(path)).is_ok()
}

pub(super) fn validate_mix_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    if args_equal(args, &["format", "--check-formatted"])
        || args_equal(args, &["compile", "--warnings-as-errors"])
        || args_equal(args, &["test"])
    {
        Ok(())
    } else {
        require_risky_exec("Mix project execution", allow_risky_exec)
    }
}

pub(super) fn validate_dune_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    if args_equal(args, &["build"])
        || args_equal(args, &["runtest"])
        || args_equal(args, &["build", "@fmt"])
    {
        Ok(())
    } else {
        require_risky_exec("Dune project execution", allow_risky_exec)
    }
}

pub(super) fn validate_bundle_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    if args_equal(args, &["exec", "rubocop", "--format", "json"])
        || args_equal(args, &["exec", "rspec"])
    {
        Ok(())
    } else {
        require_risky_exec("Bundler project execution", allow_risky_exec)
    }
}

pub(super) fn validate_php_quality_command(
    program: &str,
    args: &[String],
    allow_risky_exec: bool,
) -> Result<()> {
    let safe = match program {
        "phpstan" => args_equal(args, &["analyse", "--error-format=json"]),
        "psalm" => args_equal(args, &["--output-format=json"]),
        "phpunit" => args.is_empty(),
        "php-cs-fixer" => args_equal(args, &["fix", "--dry-run", "--diff"]),
        _ => false,
    };
    if safe {
        Ok(())
    } else {
        require_risky_exec("PHP quality-tool execution", allow_risky_exec)
    }
}

pub(in crate::workspace) fn command_requires_workspace_write(
    program: &str,
    args: &[String],
) -> bool {
    match program {
        "git" => {
            let Some(index) = args.iter().position(|arg| !arg.starts_with('-')) else {
                return false;
            };
            let command = args[index].as_str();
            let tail = &args[index + 1..];
            match command {
                "add" | "commit" | "push" | "switch" | "restore" => true,
                "branch" => {
                    let list_only = tail.is_empty()
                        || args_equal(tail, &["--show-current"])
                        || (tail.len() <= 2
                            && tail
                                .first()
                                .is_some_and(|arg| matches!(arg.as_str(), "--list" | "-l")));
                    !list_only
                }
                "tag" => {
                    let list_only = tail.is_empty()
                        || (tail.len() <= 2
                            && tail
                                .first()
                                .is_some_and(|arg| matches!(arg.as_str(), "--list" | "-l")));
                    !list_only
                }
                "lfs" => tail.first().is_some_and(|action| {
                    matches!(action.as_str(), "push" | "track" | "untrack" | "checkout")
                }),
                _ => false,
            }
        }
        "cargo" => {
            args.iter().any(|arg| arg == "fmt") && !args.iter().any(|arg| arg == "--check")
                || args.iter().any(|arg| arg == "update")
        }
        "ruff" => {
            args.first().is_some_and(|arg| arg == "format")
                && !args
                    .iter()
                    .any(|arg| matches!(arg.as_str(), "--check" | "--diff"))
                || args.first().is_some_and(|arg| arg == "check")
                    && args.iter().any(|arg| {
                        matches!(arg.as_str(), "--fix" | "--fix-only" | "--unsafe-fixes")
                    })
        }
        "biome" => args
            .iter()
            .any(|arg| matches!(arg.as_str(), "--write" | "--fix") || arg.starts_with("--write=")),
        "deno" => {
            args.first().is_some_and(|arg| arg == "fmt") && !args.iter().any(|arg| arg == "--check")
        }
        "uv" => args.first().is_some_and(|arg| {
            matches!(arg.as_str(), "format" | "lock" | "sync" | "add" | "remove")
        }),
        "go" => {
            matches!(args.first().map(String::as_str), Some("mod"))
                && args.get(1).is_some_and(|action| action == "tidy")
        }
        "npm" | "pnpm" | "yarn" | "bun" => args.first().is_some_and(|command| {
            command == "ci"
                || command == "install" && args.iter().skip(1).all(|arg| arg.starts_with('-'))
        }),
        _ => false,
    }
}
