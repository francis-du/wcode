use super::*;

pub(super) fn validate_python_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-c" | "-" | "-i"))
    {
        bail!("inline or interactive Python execution is blocked; execute a workspace script or module instead");
    }
    if let Some(index) = args.iter().position(|arg| arg == "-m") {
        let Some(module) = args.get(index + 1) else {
            bail!("python -m requires a module name");
        };
        if matches!(module.as_str(), "pip" | "ensurepip") {
            return require_risky_exec("Python environment package management", allow_risky_exec);
        }
        return Ok(());
    }
    if args.iter().any(|arg| {
        !arg.starts_with('-')
            && [".py", ".pyw"]
                .iter()
                .any(|suffix| arg.to_ascii_lowercase().ends_with(suffix))
    }) {
        return Ok(());
    }
    require_risky_exec("python interpreter execution", allow_risky_exec)
}

pub(super) fn validate_node_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    if args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "-e" | "--eval" | "-p" | "--print" | "-i" | "--interactive"
        )
    }) {
        bail!("inline or interactive Node execution is blocked; execute a workspace script or test instead");
    }
    if args
        .iter()
        .any(|arg| arg == "--test" || arg.starts_with("--test="))
    {
        return Ok(());
    }
    if args.iter().any(|arg| {
        !arg.starts_with('-')
            && [".js", ".mjs", ".cjs", ".ts", ".mts", ".cts"]
                .iter()
                .any(|suffix| arg.to_ascii_lowercase().ends_with(suffix))
    }) {
        return Ok(());
    }
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--check" | "-c"))
    {
        return Ok(());
    }
    require_risky_exec("node interpreter execution", allow_risky_exec)
}

pub(super) fn validate_dart_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    let Some(command) = args.first().map(String::as_str) else {
        bail!("dart requires an explicit project command");
    };
    match command {
        "analyze" | "test" | "format" | "run" | "compile" | "fix" | "create" => Ok(()),
        "pub" => match args.get(1).map(String::as_str) {
            Some("get" | "upgrade" | "downgrade" | "add" | "remove" | "outdated" | "deps") => {
                Ok(())
            }
            Some("token") => bail!(
                "dart pub token is blocked because credential flows must remain operator-owned"
            ),
            Some("publish" | "global" | "uploader") => require_risky_exec(
                &format!("Dart pub externally consequential operation: {}", args[1]),
                allow_risky_exec,
            ),
            Some("cache") => require_risky_exec("Dart pub cache operation", allow_risky_exec),
            Some(_) => Ok(()),
            None => bail!("dart pub requires an explicit subcommand"),
        },
        "devtools" => Ok(()),
        _ => Ok(()),
    }
}

pub(super) fn validate_flutter_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    if matches!(args, [first, second, ..] if first == "pub" && matches!(second.as_str(), "publish" | "global"))
    {
        return require_risky_exec("Flutter remote/global package operation", allow_risky_exec);
    }
    if matches!(
        args.first().map(String::as_str),
        Some("upgrade" | "channel" | "config" | "precache")
    ) {
        return require_risky_exec("Flutter host toolchain operation", allow_risky_exec);
    }
    Ok(())
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
    let task = args.first().map(String::as_str).unwrap_or_default();
    if matches!(task, "hex.publish" | "hex.user" | "hex.organization")
        || matches!(task, "local.hex" | "local.rebar")
    {
        require_risky_exec("Mix remote/global operation", allow_risky_exec)
    } else {
        Ok(())
    }
}

pub(super) fn validate_dune_command(args: &[String], _allow_risky_exec: bool) -> Result<()> {
    if args.first().is_some_and(|task| task == "exec") {
        if let Some(program) = args.get(1) {
            super::project_tools::validate_wrapped_development_program(program)?;
        }
    }
    Ok(())
}

pub(super) fn validate_bundle_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("exec") => {
            let Some(program) = args.get(1) else {
                bail!("bundle exec requires a project executable");
            };
            super::project_tools::validate_wrapped_development_program(program)
        }
        Some("config")
            if args
                .iter()
                .any(|arg| matches!(arg.as_str(), "--global" | "global")) =>
        {
            require_risky_exec("Bundler global configuration", allow_risky_exec)
        }
        Some("gem") | Some("install") | Some("update") | Some("add") | Some("remove")
        | Some("lock") | Some("cache") | Some("package") | Some("check") | Some("list")
        | Some("show") | Some("outdated") | Some("platform") | Some("doctor") | Some("clean")
        | Some("config") => Ok(()),
        None => Ok(()),
        _ => Ok(()),
    }
}

pub(super) fn validate_php_quality_command(
    program: &str,
    args: &[String],
    allow_risky_exec: bool,
) -> Result<()> {
    if program == "php-cs-fixer"
        && args
            .first()
            .is_some_and(|command| matches!(command.as_str(), "self-update" | "selfupdate"))
    {
        require_risky_exec("PHP CS Fixer self-update", allow_risky_exec)
    } else {
        Ok(())
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
                "add" | "commit" | "fetch" | "pull" | "push" | "switch" | "restore" => true,
                "lfs" => tail.first().is_some_and(|action| {
                    matches!(
                        action.as_str(),
                        "fetch" | "pull" | "push" | "track" | "untrack" | "checkout"
                    )
                }),
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
        "deno" => match args.first().map(String::as_str) {
            Some("fmt") => !args.iter().any(|arg| arg == "--check"),
            Some("lint") => args.iter().any(|arg| arg == "--fix"),
            Some("audit") => args.iter().any(|arg| arg == "--fix"),
            Some("check" | "run") => !args.iter().any(|arg| arg == "--frozen"),
            Some("test") => {
                !args.iter().any(|arg| arg == "--frozen")
                    || args.iter().any(|arg| {
                        matches!(
                            arg.as_str(),
                            "--update-snapshots" | "-u" | "--coverage" | "--junit-path"
                        ) || arg.starts_with("--coverage=")
                            || arg.starts_with("--junit-path=")
                    })
            }
            Some("task") => true,
            _ => false,
        },
        "dart" => match args.first().map(String::as_str) {
            Some("run" | "compile" | "create") => true,
            Some("format") => {
                let check_only = args
                    .iter()
                    .any(|arg| matches!(arg.as_str(), "--output=none" | "-o=none"))
                    || args.windows(2).any(|pair| {
                        matches!(pair, [flag, value] if matches!(flag.as_str(), "-o" | "--output") && value == "none")
                    });
                !check_only
            }
            Some("fix") => args.iter().any(|arg| arg == "--apply"),
            Some("pub") => args.get(1).is_some_and(|subcommand| {
                matches!(
                    subcommand.as_str(),
                    "get" | "upgrade" | "downgrade" | "add" | "remove"
                )
            }),
            _ => false,
        },
        "uv" => args.first().is_some_and(|arg| {
            matches!(arg.as_str(), "format" | "lock" | "sync" | "add" | "remove")
        }),
        "go" => {
            matches!(args.first().map(String::as_str), Some("mod"))
                && args.get(1).is_some_and(|action| action == "tidy")
        }
        "npm" | "pnpm" | "yarn" | "bun" => args.first().is_some_and(|command| {
            matches!(
                command.as_str(),
                "ci" | "install"
                    | "add"
                    | "remove"
                    | "uninstall"
                    | "update"
                    | "run"
                    | "start"
                    | "build"
            ) || matches!(program, "yarn" | "bun" | "pnpm")
                && super::project_tools::is_autonomous_repository_task(command)
        }),
        "make" | "just" | "task" | "mix" | "dune" | "bundle" | "cmake" | "ninja" | "mvn"
        | "gradle" | "swift" | "zig" | "pre-commit" | "act" | "bazel" | "bazelisk" | "buck2"
        | "pants" | "meson" | "ctest" | "sbt" | "lein" | "rebar3" | "poetry" | "pdm" | "hatch"
        | "tox" | "nox" | "nx" | "turbo" | "vite" | "webpack" | "rollup" | "esbuild" | "tsup"
        | "parcel" | "rspack" | "rolldown" | "rake" | "cabal" | "stack" => true,
        "flutter" => !matches!(args.first().map(String::as_str), Some("devices" | "doctor")),
        "dotnet" => !matches!(
            args.first().map(String::as_str),
            Some("list" | "--info" | "--list-sdks" | "--list-runtimes")
        ),
        program if super::language_tools::language_tool_writes_workspace(program, args) => true,
        _ => false,
    }
}
