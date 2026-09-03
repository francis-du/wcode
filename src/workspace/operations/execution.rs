use super::*;

impl Workspace {
    pub async fn run_command(
        &self,
        program: &str,
        args: &[String],
        cwd: &str,
        timeout_seconds: u64,
    ) -> Result<CommandResult> {
        if !self.allow_exec {
            bail!("command execution is disabled; restart without --no-exec");
        }
        validate_authorizable_program(program)?;
        if !self
            .commands
            .read()
            .expect("workspace command allowlist lock poisoned")
            .contains(program)
        {
            let fingerprint = self.command_access_fingerprint(program);
            let request = self.authorization.request_command(
                self.authorization_workspace_id(),
                program,
                fingerprint,
            );
            return Err(AuthorizationRequired::new(request).into());
        }
        if program == "cargo" && args == ["fmt"] && !self.allow_write {
            bail!("cargo fmt modifies source files and is blocked in a read-only workspace");
        }
        let mut effective_security = self.security;
        if !effective_security.allow_risky_exec
            && validate_command_policy(program, args, effective_security).is_err()
        {
            let mut elevated = effective_security;
            elevated.allow_risky_exec = true;
            if validate_command_policy(program, args, elevated).is_ok() {
                let operation = format!("run_command\0{program}\0{}\0{cwd}", args.join("\0"));
                self.authorize_risky_operation(
                    AuthorizationKind::RiskyExecution,
                    &operation,
                    &format!(
                        "allow repository-aware command: {program} {}",
                        args.join(" ")
                    ),
                )?;
                effective_security = elevated;
            }
        }
        validate_command_policy(program, args, effective_security)?;
        let cwd = self.existing_path(cwd)?;
        if !cwd.is_dir() {
            bail!("cwd is not a directory");
        }
        let _child_permit = crate::resource::global()
            .acquire_child()
            .await
            .map_err(anyhow::Error::msg)?;
        let effective_args = hardened_command_args(program, args);
        let mut command = Command::new(program);
        command
            .args(&effective_args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        scrub_sensitive_environment(
            &mut command,
            program,
            args,
            effective_security.allow_risky_exec,
        );
        crate::resource::apply_child_limits(&mut command);
        if program == "git" {
            command
                .env("GIT_CEILING_DIRECTORIES", &self.root)
                .env("GIT_DISCOVERY_ACROSS_FILESYSTEM", "0");
        }

        let mut child = command.spawn().context("failed to start command")?;
        let mut child_group = crate::resource::supervise_child(&child);
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("command stdout is unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("command stderr is unavailable"))?;
        let stdout_task = tokio::spawn(read_bounded_stream(stdout));
        let stderr_task = tokio::spawn(read_bounded_stream(stderr));
        let status = match timeout(
            Duration::from_secs(timeout_seconds.clamp(1, 300)),
            child.wait(),
        )
        .await
        {
            Ok(result) => result?,
            Err(_) => {
                crate::resource::terminate_child(&mut child);
                let _ = child.wait().await;
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                bail!("command timed out and was terminated");
            }
        };
        child_group.terminate();
        let (stdout, stdout_cut) = stdout_task
            .await
            .map_err(|error| anyhow!("stdout reader failed: {error}"))??;
        let (stderr, stderr_cut) = stderr_task
            .await
            .map_err(|error| anyhow!("stderr reader failed: {error}"))??;
        let (stdout, stdout_redacted) = redact_sensitive_text(&stdout);
        let (stderr, stderr_redacted) = redact_sensitive_text(&stderr);
        Ok(CommandResult {
            program: program.to_owned(),
            args: args.to_vec(),
            exit_code: status.code(),
            success: status.success(),
            stdout,
            stderr,
            truncated: stdout_cut || stderr_cut,
            redacted: stdout_redacted || stderr_redacted,
        })
    }

    pub(crate) async fn run_verification_command(
        &self,
        program: &str,
        args: &[String],
        cwd: &str,
        timeout_seconds: u64,
    ) -> Result<CommandResult> {
        validate_verification_command_shape(program, args)?;
        let mut verification_workspace = self.clone();
        verification_workspace.security.allow_risky_exec = true;
        verification_workspace
            .run_command(program, args, cwd, timeout_seconds)
            .await
    }

    pub(crate) fn workspace_program_available(&self, program: &str) -> bool {
        program.contains(['/', '\\'])
            && self.existing_path(program).is_ok_and(|path| path.is_file())
    }

    pub(crate) async fn run_trusted_runtime_command(
        &self,
        program: &str,
        args: &[String],
        cwd: &str,
        timeout_seconds: u64,
    ) -> Result<CommandResult> {
        if !self.allow_exec {
            bail!("runtime executor requires command execution; restart without --no-exec");
        }
        if !self.security.allow_risky_exec {
            let operation = format!("runtime_executor\0{program}\0{}\0{cwd}", args.join("\0"));
            self.authorize_risky_operation(
                AuthorizationKind::RuntimeExecutor,
                &operation,
                &format!(
                    "allow repository-defined executor: {program} {}",
                    args.join(" ")
                ),
            )?;
        }
        if program.trim().is_empty()
            || program.len() > 512
            || program.contains(['\0', '\n', '\r'])
            || Path::new(program).is_absolute()
            || program
                .split(['/', '\\'])
                .any(|component| component == "..")
        {
            bail!("runtime executor program is invalid or escapes the workspace");
        }
        let executable = if program.contains(['/', '\\']) {
            let executable = self.existing_path(program)?;
            if !executable.is_file() {
                bail!("runtime executor program is not a regular file");
            }
            executable
        } else {
            PathBuf::from(program)
        };
        validate_command_arguments(program, args)?;
        let cwd = self.existing_path(cwd)?;
        if !cwd.is_dir() {
            bail!("runtime executor cwd is not a directory");
        }
        let _child_permit = crate::resource::global()
            .acquire_child()
            .await
            .map_err(anyhow::Error::msg)?;
        let mut command = Command::new(executable);
        command
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        scrub_sensitive_environment(&mut command, program, args, false);
        crate::resource::apply_child_limits(&mut command);
        let mut child = command
            .spawn()
            .with_context(|| format!("failed to start runtime executor {program}"))?;
        let mut child_group = crate::resource::supervise_child(&child);
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("runtime executor stdout is unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("runtime executor stderr is unavailable"))?;
        let stdout_task = tokio::spawn(read_bounded_stream(stdout));
        let stderr_task = tokio::spawn(read_bounded_stream(stderr));
        let status = match timeout(
            Duration::from_secs(timeout_seconds.clamp(1, 300)),
            child.wait(),
        )
        .await
        {
            Ok(result) => result?,
            Err(_) => {
                crate::resource::terminate_child(&mut child);
                let _ = child.wait().await;
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                bail!("runtime executor timed out and was terminated");
            }
        };
        child_group.terminate();
        let (stdout, stdout_cut) = stdout_task
            .await
            .map_err(|error| anyhow!("runtime executor stdout reader failed: {error}"))??;
        let (stderr, stderr_cut) = stderr_task
            .await
            .map_err(|error| anyhow!("runtime executor stderr reader failed: {error}"))??;
        let (stdout, stdout_redacted) = redact_sensitive_text(&stdout);
        let (stderr, stderr_redacted) = redact_sensitive_text(&stderr);
        Ok(CommandResult {
            program: program.to_owned(),
            args: args.to_vec(),
            exit_code: status.code(),
            success: status.success(),
            stdout,
            stderr,
            truncated: stdout_cut || stderr_cut,
            redacted: stdout_redacted || stderr_redacted,
        })
    }
}

pub(crate) fn redact_sensitive_text(text: &str) -> (String, bool) {
    let mut redacted_any = false;
    let mut in_private_key = false;
    let mut output = Vec::new();
    for line in text.lines() {
        let upper = line.to_ascii_uppercase();
        if upper.contains("-----BEGIN") && upper.contains("PRIVATE KEY") {
            in_private_key = true;
            redacted_any = true;
            output.push("[REDACTED PRIVATE KEY]".to_owned());
            continue;
        }
        if in_private_key {
            redacted_any = true;
            if upper.contains("-----END") && upper.contains("PRIVATE KEY") {
                in_private_key = false;
            }
            continue;
        }
        let (safe, redacted) = redact_sensitive_line(line);
        redacted_any |= redacted;
        output.push(safe);
    }
    (output.join("\n"), redacted_any)
}

pub(super) fn redact_sensitive_line(line: &str) -> (String, bool) {
    let sensitive = [
        "api_key",
        "apikey",
        "access_token",
        "auth_token",
        "token",
        "secret",
        "password",
        "passwd",
        "client_secret",
        "private_key",
    ];
    let lower = line.to_ascii_lowercase();
    let Some(separator) = line.find('=').or_else(|| line.find(':')) else {
        return (line.to_owned(), false);
    };
    let key_side = &lower[..separator.min(lower.len())];
    if !sensitive.iter().any(|needle| key_side.contains(needle)) {
        return (line.to_owned(), false);
    }
    let value = line[separator + 1..].trim();
    let looks_literal = value.starts_with('"')
        || value.starts_with('\'')
        || value.starts_with('`')
        || (!value.is_empty() && !value.contains(char::is_whitespace));
    if !looks_literal {
        return (line.to_owned(), false);
    }
    (
        format!(
            "{}{} [REDACTED]",
            &line[..separator],
            &line[separator..=separator]
        ),
        true,
    )
}
