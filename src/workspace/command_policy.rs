use super::*;

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
        "rg" => validate_rg_command(args),
        "cargo" => validate_cargo_command(args, security.allow_risky_exec),
        "go" => validate_go_command(args, security.allow_risky_exec),
        "npm" | "pnpm" | "yarn" | "bun" => {
            validate_package_command(program, args, security.allow_risky_exec)
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

fn validate_git_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    let subcommand_index = args
        .iter()
        .position(|arg| !arg.starts_with('-'))
        .ok_or_else(|| anyhow!("git subcommand is required"))?;
    for option in &args[..subcommand_index] {
        if !matches!(option.as_str(), "--no-pager" | "--literal-pathspecs") {
            bail!("git global option is blocked by the workspace policy: {option}");
        }
    }
    let subcommand = args[subcommand_index].as_str();
    let tail = &args[subcommand_index + 1..];
    if matches!(
        subcommand,
        "status" | "diff" | "log" | "show" | "rev-parse" | "ls-files"
    ) {
        for arg in tail {
            if arg == "--ext-diff"
                || arg == "--textconv"
                || arg == "--open-files-in-pager"
                || arg == "--show-signature"
                || arg == "--output"
                || arg.starts_with("--output=")
                || arg.starts_with("--git-dir")
                || arg.starts_with("--work-tree")
                || arg.contains("%G")
            {
                bail!("git option can execute helpers or write outside the result stream: {arg}");
            }
        }
        return Ok(());
    }

    match subcommand {
        "add" => validate_git_add(tail)?,
        "commit" => validate_git_commit(tail)?,
        "push" => validate_git_push(tail)?,
        _ => bail!("git mutation subcommand is permanently blocked: {subcommand}"),
    }
    require_risky_exec("git repository mutation", allow_risky_exec)
}

fn validate_git_add(args: &[String]) -> Result<()> {
    let mut path_count = 0usize;
    for arg in args {
        if arg == "--" {
            continue;
        }
        if arg.starts_with('-') {
            bail!("git add option is blocked; authorize explicit pathspecs only: {arg}");
        }
        if arg.starts_with(':') {
            bail!("git add magic pathspecs are blocked: {arg}");
        }
        if matches!(arg.as_str(), "." | "./") {
            bail!("git add requires explicit files/directories; broad dot pathspecs are blocked");
        }
        path_count += 1;
    }
    if path_count == 0 {
        bail!("git add requires at least one explicit pathspec");
    }
    Ok(())
}

fn validate_git_commit(args: &[String]) -> Result<()> {
    if args.is_empty() {
        bail!("git commit requires an explicit -m/--message to avoid opening an editor");
    }
    let mut index = 0usize;
    let mut messages = 0usize;
    while index < args.len() {
        let arg = &args[index];
        if matches!(arg.as_str(), "-m" | "--message") {
            let Some(message) = args.get(index + 1) else {
                bail!("git commit message value is required");
            };
            if message.is_empty() {
                bail!("git commit message must not be empty");
            }
            messages += 1;
            index += 2;
            continue;
        }
        if let Some(message) = arg.strip_prefix("--message=") {
            if message.is_empty() {
                bail!("git commit message must not be empty");
            }
            messages += 1;
            index += 1;
            continue;
        }
        bail!("git commit option is blocked; only explicit -m/--message is supported: {arg}");
    }
    if messages == 0 {
        bail!("git commit requires an explicit -m/--message");
    }
    Ok(())
}

fn validate_git_push(args: &[String]) -> Result<()> {
    let mut positional = 0usize;
    for arg in args {
        if matches!(arg.as_str(), "-u" | "--set-upstream") {
            continue;
        }
        if arg.starts_with('-') {
            bail!("git push option is blocked; force/delete/mirror/all/tag pushes are not authorizable: {arg}");
        }
        if arg.starts_with('+') || arg.ends_with(':') {
            bail!("git push force/delete refspecs are permanently blocked: {arg}");
        }
        positional += 1;
        if positional > 2 {
            bail!("git push accepts at most an explicit remote and one refspec");
        }
    }
    if positional != 2 {
        bail!("git push requires an explicit remote and one explicit refspec for auditable authorization");
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
        ["fmt"]
            | ["fmt", "--check"]
            | ["check"]
            | ["check", "--locked"]
            | ["test"]
            | ["test", "--locked"]
            | ["clippy", "--", "-D", "warnings"]
            | ["clippy", "--locked", "--", "-D", "warnings"]
            | ["build", "--release"]
            | ["build", "--release", "--locked"]
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
            "{label} can execute project-controlled code and is disabled by default; restart with --allow-risky-exec only for a trusted repository"
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
    let overrides = [
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
        "core.sshCommand=false".to_owned(),
        "core.gitProxy=".to_owned(),
        "http.extraHeader=".to_owned(),
        "protocol.ext.allow=never".to_owned(),
        "protocol.file.allow=never".to_owned(),
        "maintenance.auto=false".to_owned(),
        "gc.auto=0".to_owned(),
    ];
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
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
            validate_git_command(&args(&["commit", "-m", "docs: refresh screenshots"]), true)
                .is_ok()
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
            validate_command_arguments("git", &args(&["https://example.com/repository.git"]),)
                .is_ok()
        );
    }
}

pub(super) fn scrub_sensitive_environment(command: &mut Command, program: &str) {
    for (key, _) in std::env::vars() {
        let upper = key.to_ascii_uppercase();
        if (program == "git" && upper.starts_with("GIT_"))
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
            )
        {
            command.env_remove(key);
        }
    }
    command.env("NO_COLOR", "1");
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
