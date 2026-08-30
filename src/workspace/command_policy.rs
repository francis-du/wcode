use super::*;

#[path = "command_policy/dev_tools.rs"]
mod dev_tools;
#[path = "command_policy/git.rs"]
mod git;
#[path = "command_policy/github.rs"]
mod github;
#[path = "command_policy/infrastructure.rs"]
mod infrastructure;
use dev_tools::{validate_fd_command, validate_jq_command};
use git::validate_git_command;
use github::validate_gh_command;
use infrastructure::{
    validate_docker_command, validate_kubectl_command, validate_terraform_command,
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
        "just" | "task" => validate_repository_runner(program, security.allow_risky_exec),
        "uv" => validate_uv_command(args, security.allow_risky_exec),
        "ruff" => validate_ruff_command(args, security.allow_risky_exec),
        "biome" => validate_biome_command(args, security.allow_risky_exec),
        "deno" => validate_deno_command(args, security.allow_risky_exec),
        "docker" => validate_docker_command(args, security.allow_risky_exec),
        "kubectl" => validate_kubectl_command(args, security.allow_risky_exec),
        "terraform" => validate_terraform_command(args, security.allow_risky_exec),
        "fd" => validate_fd_command(args),
        "jq" => validate_jq_command(args),
        "dotnet" => validate_dotnet_command(args, security.allow_risky_exec),
        "cmake" | "ninja" | "mvn" | "gradle" | "swift" | "zig" | "pre-commit" | "act" => {
            validate_known_project_runner(program, args, security.allow_risky_exec)
        }
        "rustc" | "node" | "python3" | "pytest" | "make" => {
            require_risky_exec(program, security.allow_risky_exec)
        }
        _ => require_risky_exec(
            "user-authorized external command",
            security.allow_risky_exec,
        ),
    }
}

pub(super) fn validate_command_arguments(program: &str, args: &[String]) -> Result<()> {
    let rg_pattern_index = (program == "rg")
        .then(|| args.iter().position(|arg| !arg.starts_with('-')))
        .flatten();
    for (index, arg) in args.iter().enumerate() {
        if arg.contains('\0') || arg.contains(['\n', '\r']) {
            bail!("command arguments contain forbidden control characters");
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
        if rg_pattern_index == Some(index) {
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

fn validate_repository_runner(program: &str, allow_risky_exec: bool) -> Result<()> {
    require_risky_exec(
        &format!("{program} repository task evaluation"),
        allow_risky_exec,
    )
}

fn validate_uv_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
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
        "lock"
            if args.iter().any(|arg| {
                matches!(
                    arg.as_str(),
                    "--check" | "--locked" | "--check-exists" | "--frozen"
                )
            }) =>
        {
            Ok(())
        }
        "tree"
            if args
                .iter()
                .any(|arg| matches!(arg.as_str(), "--locked" | "--frozen")) =>
        {
            Ok(())
        }
        "audit" => Ok(()),
        "run" | "sync" | "lock" | "format" | "check" | "add" | "remove" => {
            require_risky_exec(&format!("uv {subcommand}"), allow_risky_exec)
        }
        "auth" | "tool" | "python" | "self" | "cache" | "pip" => {
            bail!("uv {subcommand} is blocked because it can alter host-wide tools, credentials, interpreters, caches, or unmanaged environments")
        }
        _ => bail!("uv subcommand is blocked by the bounded project policy: {subcommand}"),
    }
}

fn validate_ruff_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    let subcommand = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| anyhow!("ruff subcommand is required"))?;
    match subcommand {
        "check" => {
            if args.iter().any(|arg| {
                matches!(
                    arg.as_str(),
                    "--fix" | "--fix-only" | "--unsafe-fixes" | "--watch"
                )
            }) {
                require_risky_exec("ruff source modification/watch execution", allow_risky_exec)
            } else {
                Ok(())
            }
        }
        "format" => {
            if args
                .iter()
                .any(|arg| matches!(arg.as_str(), "--check" | "--diff"))
            {
                Ok(())
            } else {
                require_risky_exec("ruff source formatting", allow_risky_exec)
            }
        }
        "rule" | "config" | "linter" => Ok(()),
        _ => bail!("ruff subcommand is blocked by the bounded quality policy: {subcommand}"),
    }
}

fn validate_biome_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    let subcommand = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| anyhow!("biome subcommand is required"))?;
    match subcommand {
        "check" | "lint" | "format" | "ci" => {
            if args.iter().any(|arg| {
                matches!(arg.as_str(), "--write" | "--fix") || arg.starts_with("--write=")
            }) {
                require_risky_exec("Biome source modification", allow_risky_exec)
            } else {
                Ok(())
            }
        }
        _ => bail!("biome subcommand is blocked by the bounded quality policy: {subcommand}"),
    }
}

fn validate_deno_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    let subcommand = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| anyhow!("deno subcommand is required"))?;
    match subcommand {
        "lint" | "check" => Ok(()),
        "fmt" if args.iter().any(|arg| arg == "--check") => Ok(()),
        "test" if !args.iter().any(|arg| arg.starts_with("--allow-")) => Ok(()),
        "fmt" | "test" | "run" | "task" => {
            require_risky_exec(&format!("deno {subcommand}"), allow_risky_exec)
        }
        _ => bail!("deno subcommand is blocked by the bounded runtime policy: {subcommand}"),
    }
}

fn validate_dotnet_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
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
        "build" | "test" | "format" | "restore" | "list" => {
            require_risky_exec(&format!("dotnet {subcommand}"), allow_risky_exec)
        }
        "run" | "publish" | "pack" | "tool" | "workload" | "nuget" | "new" | "sdk" => {
            bail!("dotnet {subcommand} is blocked by the bounded project/host policy")
        }
        _ => bail!("dotnet subcommand is blocked by the bounded project policy: {subcommand}"),
    }
}

fn validate_known_project_runner(
    program: &str,
    args: &[String],
    allow_risky_exec: bool,
) -> Result<()> {
    if args.is_empty() {
        bail!("{program} requires an explicit operation");
    }
    match program {
        "mvn" if args.iter().any(|arg| matches!(arg.as_str(), "deploy" | "install")) => {
            bail!("Maven install/deploy is blocked because it mutates the local artifact repository or a remote repository")
        }
        "gradle" if args.iter().any(|arg| {
            let task = arg.trim_start_matches(':').to_ascii_lowercase();
            task.contains("publish") || task.contains("upload") || task.contains("release") || task == "wrapper" || task == "init"
        }) => bail!("Gradle publish/upload/release/wrapper/init tasks are blocked by the bounded project policy"),
        "swift" if matches!(args.first().map(String::as_str), Some("sdk" | "package-registry" | "package-collection")) => {
            bail!("Swift SDK/registry/collection host configuration commands are blocked")
        }
        "act" if args.iter().any(|arg| matches!(arg.as_str(), "--bind" | "--privileged") || arg.starts_with("--bind=") || arg.starts_with("--container-daemon-socket=")) => {
            bail!("act host bind/privileged/daemon redirection is blocked")
        }
        _ => {}
    }
    require_risky_exec(&format!("{program} project execution"), allow_risky_exec)
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

pub(super) fn validate_verification_command_shape(program: &str, args: &[String]) -> Result<()> {
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let allowed = matches!(
        (program, args.as_slice()),
        ("git", ["diff", "--check"])
            | ("cargo", ["fmt", "--check"])
            | ("cargo", ["check"])
            | ("cargo", ["check", "--locked"])
            | ("cargo", ["test"])
            | ("cargo", ["test", "--locked"])
            | ("cargo", ["nextest", "run"])
            | ("cargo", ["nextest", "run", "--locked"])
            | ("cargo", ["clippy", "--", "-D", "warnings"])
            | ("cargo", ["clippy", "--locked", "--", "-D", "warnings"])
            | ("cargo", ["build", "--release"])
            | ("cargo", ["build", "--release", "--locked"])
            | ("go", ["test", "./..."])
            | ("pytest", ["-q"])
            | ("make", ["check" | "lint" | "test"])
            | (
                "npm" | "pnpm" | "yarn" | "bun",
                [
                    "run",
                    "lint" | "typecheck" | "check" | "format:check" | "test" | "build"
                ],
            )
    );
    if allowed {
        Ok(())
    } else {
        bail!(
            "command is not an approved inferred verification shape: {}",
            format_command(program, &args)
        )
    }
}

fn format_command(program: &str, args: &[&str]) -> String {
    std::iter::once(program)
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_default_safe_cargo_command(args: &[String]) -> bool {
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    matches!(
        args.as_slice(),
        ["fmt", "--check"]
            | ["check"]
            | ["check", "--locked"]
            | ["clippy", "--", "-D", "warnings"]
            | ["clippy", "--locked", "--", "-D", "warnings"]
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
        "metadata" => require_risky_exec("cargo metadata", allow_risky_exec),
        "fmt" if args.iter().any(|arg| arg == "--check") => {
            require_risky_exec("cargo formatting", allow_risky_exec)
        }
        "check" | "test" | "clippy" | "build" => {
            require_risky_exec("cargo project execution", allow_risky_exec)
        }
        "nextest" => {
            let index = args
                .iter()
                .position(|arg| !arg.starts_with('-'))
                .expect("cargo subcommand already resolved");
            let action = args
                .get(index + 1)
                .map(String::as_str)
                .ok_or_else(|| anyhow!("cargo nextest action is required"))?;
            if !matches!(action, "run" | "list") {
                bail!("cargo nextest {action} is blocked; only run/list enter exact project authorization");
            }
            require_risky_exec(&format!("cargo nextest {action}"), allow_risky_exec)
        }
        _ => bail!("cargo subcommand is blocked by the safe execution policy: {subcommand}"),
    }
}

fn validate_go_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
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
        "list" | "test" | "vet" | "build" => {
            require_risky_exec("go project inspection/execution", allow_risky_exec)
        }
        _ => bail!("go subcommand is blocked by the safe execution policy: {subcommand}"),
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
    if matches!(
        subcommand,
        "list" | "ls" | "why" | "run" | "test" | "build" | "lint" | "check" | "typecheck"
    ) {
        return require_risky_exec(
            &format!("{program} project inspection/script"),
            allow_risky_exec,
        );
    }
    bail!("{program} subcommand is blocked by the safe execution policy: {subcommand}")
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

fn is_git_push_command(args: &[String]) -> bool {
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
#[path = "../../tests/unit/workspace/policy.rs"]
mod tests;
