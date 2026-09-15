use super::*;

#[cfg(test)]
#[path = "../../../tests/unit/workspace/execution.rs"]
mod tests;

fn ensure_workspace_executable(path: &Path) -> Result<()> {
    ensure_single_link_file(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path.metadata()?.permissions().mode() & 0o111 == 0 {
            bail!("workspace executable does not have an executable permission bit");
        }
    }
    Ok(())
}

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
        // Reject malformed/unavailable operations before creating an approval
        // request, so the operator never approves something that cannot run.
        let mut admissible = self.security;
        admissible.allow_risky_exec = true;
        validate_command_policy(program, args, admissible)?;
        let development_program = LANGUAGE_DEVELOPMENT_COMMANDS.contains(&program);
        let mut safe_development = self.security;
        // Repository development tools are intentionally autonomous. Give
        // their existing bounded policy the elevated lane up front so normal
        // build/test/lint/codegen/package-manager workflows do not create a
        // repetitive RiskyExecution approval request. Permanent policy
        // rejections (shell interpreters, protected/escaping paths, credential
        // flows, and explicitly blocked host operations) still fail closed.
        safe_development.allow_risky_exec = development_program;
        let autonomous_development =
            validate_command_policy(program, args, safe_development).is_ok();
        let cwd_path = self.existing_path(cwd)?;
        if !cwd_path.is_dir() {
            bail!("cwd is not a directory");
        }
        if !self.allow_write
            && command_requires_workspace_write(program, args)
            && validate_verification_command_shape(program, args).is_err()
        {
            bail!("command modifies repository state and is blocked in a read-only workspace");
        }
        if !self.workspace_commands_granted()
            && !self
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
        let mut effective_security = self.security;
        if self.workspace_commands_granted() || (autonomous_development && development_program) {
            effective_security.allow_risky_exec = true;
        }
        if !effective_security.allow_risky_exec
            && !autonomous_development
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
        let governor = crate::resource::global();
        let (_child_permit, process_queue_wait_ms) = if is_inspection_probe(program, args) {
            governor.acquire_probe_with_wait().await
        } else {
            governor.acquire_child_with_wait().await
        }
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
            effective_security.allow_risky_exec || (program == "git" && is_git_push_command(args)),
        );
        crate::resource::apply_child_limits(&mut command);
        if program == "git" {
            command
                .env("GIT_CEILING_DIRECTORIES", &self.root)
                .env("GIT_DISCOVERY_ACROSS_FILESYSTEM", "0");
        }

        let child = command.spawn().context("failed to start command")?;
        collect_command_result(child, program, args, timeout_seconds, process_queue_wait_ms).await
    }

    pub(crate) fn verification_command_shape_allowed(
        &self,
        program: &str,
        args: &[String],
    ) -> bool {
        validate_verification_command_shape(program, args).is_ok()
    }

    pub(crate) fn development_command_shape_allowed(&self, program: &str, args: &[String]) -> bool {
        let mut security = self.security;
        security.allow_risky_exec = LANGUAGE_DEVELOPMENT_COMMANDS.contains(&program);
        validate_command_policy(program, args, security).is_ok()
    }

    pub(crate) async fn run_verification_command(
        &self,
        program: &str,
        args: &[String],
        cwd: &str,
        timeout_seconds: u64,
    ) -> Result<CommandResult> {
        validate_verification_command_shape(program, args)?;
        if program.contains(['/', '\\']) {
            return self
                .run_workspace_verification_executable(program, args, cwd, timeout_seconds)
                .await;
        }
        let mut verification_workspace = self.clone();
        verification_workspace.security.allow_risky_exec = true;
        verification_workspace
            .run_command(program, args, cwd, timeout_seconds)
            .await
    }

    pub(crate) fn workspace_program_available(&self, program: &str) -> bool {
        program.contains(['/', '\\'])
            && self
                .existing_path(program)
                .and_then(|path| ensure_workspace_executable(&path))
                .is_ok()
    }

    async fn run_workspace_verification_executable(
        &self,
        program: &str,
        args: &[String],
        cwd: &str,
        timeout_seconds: u64,
    ) -> Result<CommandResult> {
        if !self.allow_exec {
            bail!("project verification requires command execution; restart without --no-exec");
        }
        let executable = self.existing_path(program)?;
        ensure_workspace_executable(&executable)?;
        validate_command_arguments(program, args)?;
        let cwd = self.existing_path(cwd)?;
        if !cwd.is_dir() {
            bail!("cwd is not a directory");
        }
        let (_child_permit, process_queue_wait_ms) = crate::resource::global()
            .acquire_child_with_wait()
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
        let child = command
            .spawn()
            .context("failed to start workspace verification executable")?;
        collect_command_result(child, program, args, timeout_seconds, process_queue_wait_ms).await
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
        // Repository-declared test/quality executors use the hardened local
        // development lane and do not require repetitive human approval. The
        // executable and cwd remain workspace-bounded, sensitive environment
        // state is scrubbed, stdin is closed, and process/output/time are bounded.
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
            ensure_workspace_executable(&executable)?;
            executable
        } else {
            PathBuf::from(program)
        };
        validate_command_arguments(program, args)?;
        let cwd = self.existing_path(cwd)?;
        if !cwd.is_dir() {
            bail!("runtime executor cwd is not a directory");
        }
        let (_child_permit, process_queue_wait_ms) = crate::resource::global()
            .acquire_child_with_wait()
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
        let child = command
            .spawn()
            .with_context(|| format!("failed to start runtime executor {program}"))?;
        collect_command_result(child, program, args, timeout_seconds, process_queue_wait_ms).await
    }
}

// Both execution lanes share cancellation ownership, bounded cleanup and the
// same failed-result contract. A timeout does not undo already-applied effects.
async fn collect_command_result(
    mut child: tokio::process::Child,
    program: &str,
    args: &[String],
    timeout_seconds: u64,
    process_queue_wait_ms: u64,
) -> Result<CommandResult> {
    let mut group = crate::resource::supervise_child(&child);
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("command stdout is unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("command stderr is unavailable"))?;
    // Dropping the request must also cancel its pipe readers, not detach them.
    let mut readers = tokio::task::JoinSet::new();
    readers.spawn(async move { (true, read_bounded_stream(stdout).await) });
    readers.spawn(async move { (false, read_bounded_stream(stderr).await) });
    let seconds = timeout_seconds.clamp(1, 1800);
    let waited = timeout(Duration::from_secs(seconds), child.wait()).await;
    let timed_out = waited.is_err();
    let wait_failed = matches!(&waited, Ok(Err(_)));
    let mut status = waited.ok().and_then(std::result::Result::ok);
    if status.is_none() {
        crate::resource::terminate_child(&mut child);
    }
    group.terminate();
    if status.is_none() {
        status = timeout(Duration::from_secs(2), child.wait())
            .await
            .ok()
            .and_then(std::result::Result::ok);
    }
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut truncated = false;
    let mut output_incomplete = false;
    let drained = timeout(Duration::from_secs(2), async {
        while let Some(joined) = readers.join_next().await {
            match joined {
                Ok((is_stdout, Ok((text, cut)))) => {
                    truncated |= cut;
                    if is_stdout {
                        stdout = text;
                    } else {
                        stderr = text;
                    }
                }
                _ => output_incomplete = true,
            }
        }
    })
    .await;
    if drained.is_err() {
        output_incomplete = true;
        readers.abort_all();
    }
    if timed_out {
        stderr.push_str("\n[wcode: command timed out; termination requested. Inspect actual effects before retrying; no rollback or automatic retry was performed.]\n");
    }
    if wait_failed || status.is_none() {
        stderr.push_str("\n[wcode: process completion could not be confirmed.]\n");
    }
    if output_incomplete {
        stderr.push_str("\n[wcode: output capture incomplete; unavailable output is not proof that nothing happened.]\n");
    }
    let success = !timed_out
        && !wait_failed
        && !output_incomplete
        && status.is_some_and(|status| status.success());
    let (stdout, stderr, redacted) = redact_command_streams(stdout, stderr);
    Ok(CommandResult {
        program: program.to_owned(),
        args: args.to_vec(),
        exit_code: status.and_then(|status| status.code()),
        success,
        process_queue_wait_ms,
        stdout,
        stderr,
        truncated: truncated || output_incomplete,
        redacted,
        timed_out,
        output_incomplete,
        retry_guidance: (!success).then_some(
            "Inspect actual effects and diagnostics before retrying. No rollback or automatic retry was performed.",
        ),
    })
}

fn redact_command_streams(stdout: String, stderr: String) -> (String, String, bool) {
    let (stdout, stdout_redacted) = redact_sensitive_text(&stdout);
    let (stderr, stderr_redacted) = redact_sensitive_text(&stderr);
    (stdout, stderr, stdout_redacted || stderr_redacted)
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
