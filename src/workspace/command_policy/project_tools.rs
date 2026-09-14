use super::*;

pub(super) fn is_local_development_task(task: &str) -> bool {
    let task = task.to_ascii_lowercase();
    if matches!(
        task.as_str(),
        "check"
            | "test"
            | "tests"
            | "lint"
            | "fmt"
            | "format"
            | "build"
            | "verify"
            | "ci"
            | "dev"
            | "start"
            | "serve"
            | "run"
            | "bench"
            | "benchmark"
            | "typecheck"
            | "compile"
            | "generate"
            | "codegen"
            | "smoke"
            | "smoke-test"
            | "test-smoke"
    ) {
        return true;
    }
    let smoke_task = task.ends_with("-smoke") || task.ends_with("_smoke");
    let sensitive = [
        "publish",
        "deploy",
        "release",
        "upload",
        "install",
        "uninstall",
        "push",
        "production",
        "prod",
        "credential",
        "secret",
    ]
    .iter()
    .any(|marker| task.contains(marker));
    smoke_task && !sensitive
}

pub(super) fn validate_repository_runner(
    program: &str,
    args: &[String],
    allow_risky_exec: bool,
) -> Result<()> {
    let Some(task) = args.first().map(String::as_str) else {
        bail!("{program} requires an explicit repository task");
    };
    if !task.starts_with('-') && is_local_development_task(task) {
        return Ok(());
    }
    require_risky_exec(
        &format!("{program} repository task evaluation"),
        allow_risky_exec,
    )
}

pub(super) fn validate_uv_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    let subcommand = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| anyhow!("uv subcommand is required"))?;
    if args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--allow-insecure-host"
                | "--trusted-host"
                | "--index"
                | "--default-index"
                | "--config-file"
                | "--project"
                | "--directory"
                | "--cache-dir"
        ) || [
            "--allow-insecure-host=",
            "--trusted-host=",
            "--index=",
            "--default-index=",
            "--config-file=",
            "--project=",
            "--directory=",
            "--cache-dir=",
        ]
        .iter()
        .any(|prefix| arg.starts_with(prefix))
    }) {
        bail!("uv path/index/config redirection is blocked; use the selected workspace and repository configuration");
    }
    match subcommand {
        "lock" if args.iter().any(|arg| matches!(arg.as_str(), "--check" | "--locked" | "--check-exists" | "--frozen")) => Ok(()),
        "tree" if args.iter().any(|arg| matches!(arg.as_str(), "--locked" | "--frozen")) => Ok(()),
        "audit" => Ok(()),
        "run" if uv_run_is_bounded_check(args) || uv_run_is_local_development(args) => Ok(()),
        "sync" | "lock" | "format" | "check" | "add" | "remove" => Ok(()),
        "auth" | "tool" | "python" | "self" | "cache" | "pip" => bail!("uv {subcommand} is blocked because it can alter host-wide tools, credentials, interpreters, caches, or unmanaged environments"),
        _ => require_risky_exec(&format!("uv user-authorized project operation: {subcommand}"), allow_risky_exec),
    }
}

fn uv_run_is_bounded_check(args: &[String]) -> bool {
    args.iter()
        .skip(1)
        .find(|arg| !arg.starts_with('-'))
        .is_some_and(|program| matches!(program.as_str(), "pytest" | "ruff" | "mypy" | "pyright"))
}

fn uv_run_is_local_development(args: &[String]) -> bool {
    args.iter()
        .skip(1)
        .find(|arg| !arg.starts_with('-'))
        .is_some_and(|program| {
            matches!(program.as_str(), "python" | "python3" | "node")
                || [".py", ".js", ".mjs", ".cjs"]
                    .iter()
                    .any(|suffix| program.ends_with(suffix))
        })
}

pub(super) fn validate_ruff_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    let subcommand = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| anyhow!("ruff subcommand is required"))?;
    match subcommand {
        "check" if args.iter().any(|arg| arg == "--watch") => {
            require_risky_exec("ruff watch execution", allow_risky_exec)
        }
        "check" | "format" | "rule" | "config" | "linter" => Ok(()),
        _ => require_risky_exec(
            &format!("ruff user-authorized operation: {subcommand}"),
            allow_risky_exec,
        ),
    }
}

pub(super) fn validate_biome_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    let subcommand = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| anyhow!("biome subcommand is required"))?;
    match subcommand {
        "check" | "lint" | "format" | "ci" => Ok(()),
        _ => require_risky_exec(
            &format!("Biome user-authorized operation: {subcommand}"),
            allow_risky_exec,
        ),
    }
}

pub(super) fn validate_deno_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    let subcommand = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| anyhow!("deno subcommand is required"))?;
    match subcommand {
        "lint" | "check" => Ok(()),
        "fmt" if args.iter().any(|arg| arg == "--check") => Ok(()),
        "test" if !args.iter().any(|arg| arg.starts_with("--allow-")) => Ok(()),
        "install" | "uninstall" | "upgrade" => bail!(
            "deno {subcommand} is blocked because it can alter host-wide tools or the runtime"
        ),
        "fmt" => Ok(()),
        "test" | "run" if !args.iter().any(|arg| arg.starts_with("--allow-")) => Ok(()),
        "task"
            if args
                .get(1)
                .is_some_and(|task| is_local_development_task(task)) =>
        {
            Ok(())
        }
        "test" | "run" | "task" => {
            require_risky_exec(&format!("deno {subcommand}"), allow_risky_exec)
        }
        _ => require_risky_exec(
            &format!("deno user-authorized operation: {subcommand}"),
            allow_risky_exec,
        ),
    }
}

pub(super) fn validate_dotnet_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    let subcommand = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| anyhow!("dotnet subcommand is required"))?;
    if matches!(subcommand, "--info" | "--list-sdks" | "--list-runtimes") {
        return Ok(());
    }
    if args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--interactive"
                | "--source"
                | "--configfile"
                | "--packages"
                | "--artifacts-path"
                | "--output"
                | "-o"
        ) || arg.starts_with("--source=")
            || arg.starts_with("--configfile=")
            || arg.starts_with("--packages=")
            || arg.starts_with("--artifacts-path=")
            || arg.starts_with("--output=")
    }) {
        bail!("dotnet source/config/output redirection or interactive execution is blocked");
    }
    match subcommand {
        "list" | "build" | "test" | "format" | "restore" | "run" | "pack" | "new" | "add" | "remove" => Ok(()),
        "tool" | "workload" | "nuget" | "sdk" => bail!("dotnet {subcommand} is blocked because it can alter host-wide tools, workloads, SDKs, or package sources"),
        "publish" => require_risky_exec("dotnet publish", allow_risky_exec),
        _ => require_risky_exec(&format!("dotnet user-authorized project operation: {subcommand}"), allow_risky_exec),
    }
}

pub(super) fn validate_language_dev_tool(
    program: &str,
    args: &[String],
    allow_risky_exec: bool,
) -> Result<()> {
    let exact = match program {
        "cargo-audit" => args.is_empty() || args_equal(args, &["--json"]),
        "cargo-mutants" => args.is_empty(),
        "cargo-fuzz" => {
            matches!(args, [fuzz, run, target, separator, limit]
                if fuzz == "fuzz"
                    && run == "run"
                    && !target.is_empty()
                    && !target.starts_with('-')
                    && !target.contains(['/', '\\'])
                    && separator == "--"
                    && limit.starts_with("-max_total_time=")
                    && limit.trim_start_matches("-max_total_time=").parse::<u64>().is_ok_and(|seconds| (1..=60).contains(&seconds)))
        }
        "shellcheck" => args.first().is_some_and(|arg| arg == "--format=json") && args.len() > 1,
        "shfmt" => matches!(args.first().map(String::as_str), Some("-d" | "-w")) && args.len() > 1,
        "stylua" => args_equal(args, &["--check", "."]) || args_equal(args, &["."]),
        "luacheck" => args_equal(args, &["."]),
        "busted" => args.is_empty(),
        "clang-format" => {
            (args.starts_with(&["--dry-run".into(), "--Werror".into()]) && args.len() > 2)
                || (args.first().is_some_and(|arg| arg == "-i") && args.len() > 1)
        }
        "clang-tidy" => args.len() >= 3 && args.ends_with(&["-p".into(), ".".into()]),
        "gofmt" => matches!(args.first().map(String::as_str), Some("-d" | "-w")) && args.len() > 1,
        "staticcheck" | "govulncheck" => args_equal(args, &["./..."]),
        "mypy" => args_equal(args, &["."]),
        "pyright" => args_equal(args, &["--outputjson", "."]),
        "bandit" => args_equal(args, &["-r", ".", "-f", "json"]),
        "eslint" => {
            args_equal(args, &[".", "--format", "json"])
                || args_equal(args, &["--fix", "."])
                || args_equal(args, &[".", "--fix"])
        }
        "tsc" => args_equal(args, &["--noEmit"]),
        "stylelint" => {
            args_equal(args, &["**/*.css", "--formatter", "json"])
                || args_equal(args, &["**/*.css", "--fix"])
        }
        "Rscript" => matches!(args, [flag, expression]
            if flag == "-e"
                && matches!(expression.as_str(),
                    "quit(status=if(length(lintr::lint_package()))1 else 0)"
                    | "testthat::test_local()"
                    | "testthat::test_dir('tests/testthat')")),
        "swift-format" => {
            args_equal(args, &["lint", "-r", "."]) || args_equal(args, &["format", "-i", "-r", "."])
        }
        "swiftlint" => args_equal(args, &["lint", "--strict"]),
        "mutmut" => args_equal(args, &["run"]),
        "dotnet-stryker" | "muter" => args.is_empty(),
        "infection" => args_equal(args, &["--no-progress"]),
        _ => false,
    };
    if exact {
        Ok(())
    } else {
        require_risky_exec(
            &format!("{program} non-standard development operation"),
            allow_risky_exec,
        )
    }
}

pub(super) fn validate_known_project_runner(
    program: &str,
    args: &[String],
    allow_risky_exec: bool,
) -> Result<()> {
    if args.is_empty() {
        bail!("{program} requires an explicit operation");
    }
    match program {
        "mvn" if args.iter().any(|arg| arg == "deploy") => {
            bail!("Maven deploy is blocked because it mutates a remote repository")
        }
        "gradle"
            if args.iter().any(|arg| {
                let task = arg.trim_start_matches(':').to_ascii_lowercase();
                task.contains("publish") || task.contains("upload") || task.contains("release")
            }) =>
        {
            bail!("Gradle publish/upload/release tasks are blocked by the bounded project policy")
        }
        "swift"
            if matches!(
                args.first().map(String::as_str),
                Some("sdk" | "package-registry" | "package-collection")
            ) =>
        {
            bail!("Swift SDK/registry/collection host configuration commands are blocked")
        }
        "act"
            if args.iter().any(|arg| {
                matches!(arg.as_str(), "--bind" | "--privileged")
                    || arg.starts_with("--bind=")
                    || arg.starts_with("--container-daemon-socket=")
            }) =>
        {
            bail!("act host bind/privileged/daemon redirection is blocked")
        }
        _ => {}
    }
    if program == "act" && args.iter().any(|arg| arg == "--container-daemon-socket") {
        require_risky_exec("act external daemon redirection", allow_risky_exec)
    } else {
        Ok(())
    }
}
