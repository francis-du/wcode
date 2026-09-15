use super::*;

pub(super) fn is_sensitive_project_task(task: &str) -> bool {
    let task = task.to_ascii_lowercase();
    if [
        "install-global",
        "global-install",
        "uninstall-global",
        "global-uninstall",
        "registry-publish",
        "delete-production",
        "drop-production",
        "migrate-production",
    ]
    .iter()
    .any(|marker| task.contains(marker))
    {
        return true;
    }
    task.split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .any(|part| {
            matches!(
                part,
                "publish"
                    | "deploy"
                    | "upload"
                    | "push"
                    | "credential"
                    | "secret"
                    | "token"
                    | "login"
                    | "logout"
                    | "owner"
                    | "destroy"
            )
        })
}

pub(super) fn is_autonomous_repository_task(task: &str) -> bool {
    !task.trim().is_empty() && !task.starts_with('-') && !is_sensitive_project_task(task)
}

fn safe_make_jobs(value: &str) -> bool {
    value
        .parse::<usize>()
        .is_ok_and(|jobs| (1..=128).contains(&jobs))
}

fn safe_make_assignment(value: &str) -> bool {
    let Some((name, value)) = value.split_once('=') else {
        return false;
    };
    match name {
        "CI" => matches!(value, "" | "0" | "1" | "false" | "true"),
        "NO_COLOR" => matches!(value, "" | "0" | "1" | "false" | "true"),
        "V" | "VERBOSE" => matches!(value, "0" | "1" | "false" | "true"),
        "JOBS" | "NPROC" => safe_make_jobs(value),
        "PORT" => value.parse::<u16>().is_ok_and(|port| port != 0),
        "CARGO_TERM_COLOR" => matches!(value, "auto" | "always" | "never"),
        _ => false,
    }
}

pub(super) fn validate_make_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    if args.is_empty() {
        return Ok(());
    }
    let mut targets = Vec::new();
    let mut index = 0usize;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--" => {
                targets.extend(args[index + 1..].iter().map(String::as_str));
                break;
            }
            "-j" | "--jobs" => {
                let Some(value) = args.get(index + 1) else {
                    return require_risky_exec("unbounded make parallelism", allow_risky_exec);
                };
                if !safe_make_jobs(value) {
                    return require_risky_exec("unbounded make parallelism", allow_risky_exec);
                }
                index += 2;
                continue;
            }
            "-C" | "--directory" | "-f" | "--file" => {
                let Some(value) = args.get(index + 1) else {
                    return require_risky_exec("incomplete make path option", allow_risky_exec);
                };
                if value.is_empty() || value.starts_with('-') {
                    return require_risky_exec("invalid make path option", allow_risky_exec);
                }
                index += 2;
                continue;
            }
            "-k"
            | "--keep-going"
            | "-s"
            | "--silent"
            | "--no-print-directory"
            | "-B"
            | "--always-make"
            | "-n"
            | "--just-print"
            | "--dry-run"
            | "--recon"
            | "-q"
            | "--question"
            | "-w"
            | "--print-directory"
            | "-r"
            | "--no-builtin-rules"
            | "-R"
            | "--no-builtin-variables"
            | "-p"
            | "--print-data-base"
            | "--warn-undefined-variables"
            | "--trace"
            | "-O"
            | "--output-sync" => {
                index += 1;
                continue;
            }
            _ if arg.starts_with('-')
                && arg.len() > 2
                && arg[1..]
                    .chars()
                    .all(|flag| matches!(flag, 'r' | 'R' | 'k' | 's' | 'B' | 'n' | 'q' | 'w')) =>
            {
                index += 1;
                continue;
            }
            _ if arg.starts_with("-j") && arg.len() > 2 && safe_make_jobs(&arg[2..]) => {
                index += 1;
                continue;
            }
            _ if arg.strip_prefix("--jobs=").is_some_and(safe_make_jobs) => {
                index += 1;
                continue;
            }
            _ if arg
                .strip_prefix("-O")
                .is_some_and(|mode| matches!(mode, "none" | "line" | "target" | "recurse")) =>
            {
                index += 1;
                continue;
            }
            _ if arg
                .strip_prefix("--output-sync=")
                .is_some_and(|mode| matches!(mode, "none" | "line" | "target" | "recurse")) =>
            {
                index += 1;
                continue;
            }
            _ if arg.starts_with("--directory=") || arg.starts_with("--file=") => {
                index += 1;
                continue;
            }
            _ if safe_make_assignment(arg) => {
                index += 1;
                continue;
            }
            _ if arg.contains('=') => {
                return require_risky_exec("make variable assignment", allow_risky_exec);
            }
            _ if arg.starts_with('-') => {
                return require_risky_exec("make option evaluation", allow_risky_exec);
            }
            _ => targets.push(arg.as_str()),
        }
        index += 1;
    }
    if !targets.is_empty()
        && targets
            .iter()
            .all(|target| is_autonomous_repository_task(target))
    {
        Ok(())
    } else {
        require_risky_exec("make repository task evaluation", allow_risky_exec)
    }
}

pub(super) fn validate_local_http_probe(args: &[String], allow_risky_exec: bool) -> Result<()> {
    let mut url = None::<&str>;
    let mut index = 0usize;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "-f" | "--fail" | "-s" | "--silent" | "-S" | "--show-error" | "-I" | "--head" => {}
            "-m" | "--max-time" | "--connect-timeout" => {
                let Some(value) = args.get(index + 1) else {
                    return require_risky_exec("curl timeout option", allow_risky_exec);
                };
                if !value
                    .parse::<u64>()
                    .is_ok_and(|seconds| (1..=30).contains(&seconds))
                {
                    return require_risky_exec("curl timeout option", allow_risky_exec);
                }
                index += 1;
            }
            _ if arg
                .strip_prefix("--max-time=")
                .or_else(|| arg.strip_prefix("--connect-timeout="))
                .is_some_and(|value| {
                    value
                        .parse::<u64>()
                        .is_ok_and(|seconds| (1..=30).contains(&seconds))
                }) => {}
            _ if arg.starts_with("-m")
                && arg.len() > 2
                && arg[2..]
                    .parse::<u64>()
                    .is_ok_and(|seconds| (1..=30).contains(&seconds)) => {}
            _ if arg.starts_with('-')
                && arg.len() > 1
                && arg[1..]
                    .chars()
                    .all(|flag| matches!(flag, 'f' | 's' | 'S' | 'I')) => {}
            _ if arg.starts_with('-') => {
                return require_risky_exec("curl option evaluation", allow_risky_exec);
            }
            _ if url.is_none() => url = Some(arg),
            _ => return require_risky_exec("multiple curl URLs", allow_risky_exec),
        }
        index += 1;
    }
    let Some(url) = url else {
        return require_risky_exec("curl request without URL", allow_risky_exec);
    };
    let parsed = url::Url::parse(url).map_err(|error| anyhow!("invalid curl URL: {error}"))?;
    let local_host = matches!(
        parsed.host_str(),
        Some("127.0.0.1" | "localhost" | "::1" | "[::1]")
    );
    if matches!(parsed.scheme(), "http" | "https")
        && local_host
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.fragment().is_none()
    {
        Ok(())
    } else {
        require_risky_exec("curl external network request", allow_risky_exec)
    }
}

pub(super) fn validate_repository_runner(
    program: &str,
    args: &[String],
    allow_risky_exec: bool,
) -> Result<()> {
    let task = args
        .iter()
        .find(|arg| !arg.starts_with('-') && !arg.contains('='));
    let Some(task) = task.map(String::as_str) else {
        return Ok(());
    };
    if is_autonomous_repository_task(task) {
        return Ok(());
    }
    require_risky_exec(
        &format!("{program} repository task evaluation"),
        allow_risky_exec,
    )
}

pub(super) fn validate_wrapped_development_program(program: &str) -> Result<()> {
    if program.starts_with("./") || program.contains('/') || program.contains('\\') {
        let path = Path::new(program);
        if path.is_absolute() || program.split(['/', '\\']).any(|part| part == "..") {
            bail!("wrapped development executable must stay inside the selected workspace");
        }
        reject_protected_path(path)?;
        return Ok(());
    }
    validate_authorizable_program(program)?;
    if matches!(
        program.to_ascii_lowercase().as_str(),
        "rm" | "rmdir"
            | "mv"
            | "cp"
            | "dd"
            | "chmod"
            | "chown"
            | "curl"
            | "wget"
            | "ssh"
            | "scp"
            | "sftp"
            | "nc"
            | "netcat"
            | "socat"
            | "sudo"
            | "su"
            | "env"
            | "xargs"
            | "find"
            | "kill"
            | "pkill"
            | "killall"
            | "open"
            | "osascript"
            | "launchctl"
    ) {
        bail!(
            "wrapped host utility is not an autonomous project-development executable: {program}"
        );
    }
    Ok(())
}

pub(super) fn validate_generic_project_tool(
    program: &str,
    args: &[String],
    allow_risky_exec: bool,
) -> Result<()> {
    let operation = args
        .iter()
        .find(|arg| !arg.starts_with('-'))
        .map(String::as_str);
    if matches!(program, "poetry" | "pdm" | "hatch") && operation == Some("run") {
        if let Some(index) = args.iter().position(|arg| arg == "run") {
            if let Some(inner) = args[index + 1..]
                .iter()
                .find(|arg| !arg.starts_with('-') && !arg.starts_with('+'))
            {
                validate_wrapped_development_program(inner)?;
            }
        }
    }
    let blocked = match program {
        "bazel" | "bazelisk" => matches!(operation, Some("mobile-install")),
        "buck2" => matches!(operation, Some("install")),
        "pants" => matches!(operation, Some("publish")),
        "meson" => matches!(operation, Some("install")),
        "sbt" => args.iter().any(|arg| {
            let lower = arg.to_ascii_lowercase();
            lower.contains("publish") || lower.contains("credentials")
        }),
        "lein" => matches!(operation, Some("deploy" | "install" | "release")),
        "rebar3" => matches!(operation, Some("hex")),
        "poetry" => matches!(operation, Some("publish" | "self" | "config")),
        "pdm" => matches!(operation, Some("publish" | "self" | "plugin")),
        "hatch" => matches!(operation, Some("publish")),
        "nx" => matches!(operation, Some("release" | "connect" | "login" | "logout")),
        "turbo" => matches!(operation, Some("login" | "logout" | "link" | "unlink")),
        "cabal" => matches!(operation, Some("upload" | "install" | "user-config")),
        "stack" => matches!(operation, Some("upload" | "install" | "setup" | "upgrade")),
        "rake" => operation.is_some_and(is_sensitive_project_task),
        _ => false,
    };
    if blocked {
        require_risky_exec(
            &format!("{program} externally consequential operation"),
            allow_risky_exec,
        )
    } else {
        Ok(())
    }
}

pub(super) fn validate_uv_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return Ok(());
    };
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
        "auth" | "tool" | "python" | "self" | "cache" | "pip" => bail!("uv {subcommand} is blocked because it can alter host-wide tools, credentials, interpreters, caches, or unmanaged environments"),
        "publish" => require_risky_exec("uv publish", allow_risky_exec),
        "run" => {
            let Some(program) = args.iter().skip(1).find(|arg| !arg.starts_with('-')) else {
                bail!("uv run requires a project command or script");
            };
            if [".py", ".js", ".mjs", ".cjs", ".ts", ".tsx"]
                .iter()
                .any(|suffix| program.ends_with(suffix))
            {
                Ok(())
            } else {
                validate_wrapped_development_program(program)
            }
        }
        _ => Ok(()),
    }
}

pub(super) fn validate_ruff_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    if args.iter().any(|arg| {
        matches!(arg.as_str(), "--config" | "--cache-dir")
            || arg.starts_with("--config=")
            || arg.starts_with("--cache-dir=")
    }) {
        bail!("Ruff config/cache redirection is blocked; use repository configuration inside the selected workspace");
    }
    if args_equal(args, &["check", "."])
        || args_equal(args, &["check", "--fix", "."])
        || args_equal(args, &["check", "--fix-only", "."])
        || args_equal(args, &["check", "--output-format", "json", "."])
        || args_equal(args, &["format", "--check", "."])
        || args_equal(args, &["format", "."])
    {
        return Ok(());
    }
    let subcommand = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| anyhow!("Ruff subcommand is required"))?;
    require_risky_exec(
        &format!("Ruff user-authorized operation: {subcommand}"),
        allow_risky_exec,
    )
}

pub(super) fn validate_biome_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    if args_equal(args, &["ci", "."])
        || args_equal(args, &["format", "."])
        || args_equal(args, &["format", ".", "--reporter=json"])
        || args_equal(args, &["format", ".", "--write"])
        || args_equal(args, &["format", "--write", "."])
        || args_equal(args, &["lint", "."])
        || args_equal(args, &["check", "."])
        || args_equal(args, &["check", "--write", "."])
        || args_equal(args, &["check", ".", "--write"])
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
    {
        return Ok(());
    }
    let subcommand = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| anyhow!("biome subcommand is required"))?;
    require_risky_exec(
        &format!("Biome user-authorized operation: {subcommand}"),
        allow_risky_exec,
    )
}

pub(super) fn validate_deno_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return Ok(());
    };
    let tail = &args[1..];
    let grants_runtime_permissions = tail
        .iter()
        .any(|arg| arg == "-A" || arg == "--allow-all" || arg.starts_with("--allow-"));
    let redirects_runtime_inputs = tail.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--config"
                | "-c"
                | "--import-map"
                | "--env-file"
                | "--cert"
                | "--lock"
                | "--node-modules-dir"
                | "--vendor"
        ) || [
            "--config=",
            "--import-map=",
            "--env-file=",
            "--cert=",
            "--lock=",
            "--node-modules-dir=",
            "--vendor=",
        ]
        .iter()
        .any(|prefix| arg.starts_with(prefix))
    });
    let long_running = tail.iter().any(|arg| {
        matches!(arg.as_str(), "--watch" | "--watch-hmr")
            || arg.starts_with("--watch=")
            || arg.starts_with("--watch-hmr=")
    });
    if grants_runtime_permissions || redirects_runtime_inputs || long_running {
        return require_risky_exec(
            &format!("deno {subcommand} expanded execution surface"),
            allow_risky_exec,
        );
    }
    match subcommand {
        "audit" if tail.iter().any(|arg| arg == "--fix") => {
            require_risky_exec("deno audit --fix", allow_risky_exec)
        }
        "audit" | "lint" | "check" => Ok(()),
        "fmt" if tail.iter().any(|arg| arg == "--check") => Ok(()),
        "test" => Ok(()),
        "install" | "uninstall" | "upgrade" => {
            bail!("deno {subcommand} is blocked because host-wide tool installation/upgrades must remain operator-owned")
        }
        "fmt" | "run" => Ok(()),
        "task"
            if args
                .get(1)
                .is_some_and(|task| is_autonomous_repository_task(task)) =>
        {
            Ok(())
        }
        "task" => require_risky_exec("deno task", allow_risky_exec),
        _ => require_risky_exec(
            &format!("deno user-authorized operation: {subcommand}"),
            allow_risky_exec,
        ),
    }
}

pub(super) fn validate_dotnet_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return Ok(());
    };
    if matches!(subcommand, "--info" | "--list-sdks" | "--list-runtimes") {
        return Ok(());
    }
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--interactive"))
    {
        bail!("dotnet interactive execution is blocked");
    }
    match subcommand {
        "tool" | "workload" | "nuget" | "sdk" => require_risky_exec(
            &format!("dotnet host/toolchain operation: {subcommand}"),
            allow_risky_exec,
        ),
        "publish" => require_risky_exec("dotnet publish", allow_risky_exec),
        _ => Ok(()),
    }
}

pub(super) fn validate_language_dev_tool(
    program: &str,
    args: &[String],
    _allow_risky_exec: bool,
) -> Result<()> {
    match program {
        "Rscript" | "clang-tidy" | "clang-format" | "gofmt" | "shellcheck" | "shfmt" | "stylua"
        | "luacheck" | "busted" | "staticcheck" | "govulncheck" | "mypy" | "pyright" | "bandit"
        | "eslint" | "tsc" | "stylelint" | "swift-format" | "swiftlint" => {
            super::language_tools::validate_language_development_tool(program, args)
        }
        "cargo-audit" | "cargo-mutants" | "cargo-fuzz" | "mutmut" | "dotnet-stryker" | "muter"
        | "infection" => Ok(()),
        _ => Ok(()),
    }
}

pub(super) fn validate_known_project_runner(
    program: &str,
    args: &[String],
    allow_risky_exec: bool,
) -> Result<()> {
    if args.is_empty() {
        return Ok(());
    }
    match program {
        "mvn" if args.iter().any(|arg| arg == "deploy") => {
            return require_risky_exec("Maven deploy", allow_risky_exec);
        }
        "gradle"
            if args.iter().any(|arg| {
                let task = arg.trim_start_matches(':').to_ascii_lowercase();
                task.contains("publish") || task.contains("upload")
            }) =>
        {
            return require_risky_exec("Gradle publish/upload task", allow_risky_exec);
        }
        "swift"
            if matches!(
                args.first().map(String::as_str),
                Some("sdk" | "package-registry" | "package-collection")
            ) =>
        {
            return require_risky_exec("Swift SDK/registry/collection operation", allow_risky_exec);
        }
        "act"
            if args.iter().any(|arg| {
                matches!(arg.as_str(), "--bind" | "--privileged")
                    || arg.starts_with("--bind=")
                    || arg.starts_with("--container-daemon-socket=")
            }) =>
        {
            return require_risky_exec(
                "act host bind/privileged/daemon redirection",
                allow_risky_exec,
            );
        }
        _ => {}
    }
    if program == "act" && args.iter().any(|arg| arg == "--container-daemon-socket") {
        require_risky_exec("act external daemon redirection", allow_risky_exec)
    } else {
        Ok(())
    }
}
