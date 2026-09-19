use super::*;
use tokio::sync::{
    OwnedRwLockReadGuard, OwnedRwLockWriteGuard, OwnedSemaphorePermit, RwLock, Semaphore,
};

#[cfg(test)]
#[path = "../../../tests/unit/workspace/execution.rs"]
mod tests;

fn process_queue_wait(command_timeout: Duration) -> Duration {
    command_timeout.min(crate::resource::PROCESS_QUEUE_WAIT_CAP)
}

const CARGO_CONTENTION_WAIT_CAP: Duration = Duration::from_secs(30);

fn cargo_contention_wait(command_timeout: Duration) -> Duration {
    command_timeout
        .min(CARGO_CONTENTION_WAIT_CAP)
        .max(crate::resource::PROCESS_QUEUE_WAIT_CAP)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CargoContentionLane {
    Registry,
    WorkspaceExclusive,
    WorkspaceTest,
}

fn cargo_contention_lane(program: &str, args: &[String]) -> Option<CargoContentionLane> {
    if program != "cargo" {
        return None;
    }
    for (index, arg) in args.iter().enumerate() {
        if matches!(arg.as_str(), "info" | "search" | "fetch" | "update") {
            return Some(CargoContentionLane::Registry);
        }
        if arg == "test"
            || (arg == "nextest" && args.get(index + 1).is_some_and(|next| next == "run"))
        {
            return Some(
                if validate_verification_command_shape(program, args).is_ok() {
                    CargoContentionLane::WorkspaceTest
                } else {
                    CargoContentionLane::WorkspaceExclusive
                },
            );
        }
        if matches!(
            arg.as_str(),
            "build" | "check" | "clippy" | "bench" | "doc" | "fix" | "run" | "rustc" | "rustdoc"
        ) {
            return Some(CargoContentionLane::WorkspaceExclusive);
        }
    }
    None
}

fn cargo_registry_gate() -> &'static Arc<Semaphore> {
    static GATE: std::sync::OnceLock<Arc<Semaphore>> = std::sync::OnceLock::new();
    GATE.get_or_init(|| Arc::new(Semaphore::new(1)))
}

struct CargoWorkspaceGate {
    access: Arc<RwLock<()>>,
    test_slots: Arc<Semaphore>,
}

impl CargoWorkspaceGate {
    fn new() -> Self {
        Self {
            access: Arc::new(RwLock::new(())),
            test_slots: Arc::new(Semaphore::new(
                crate::resource::limits().child_processes.max(1),
            )),
        }
    }
}

enum CargoContentionPermit {
    Registry {
        _permit: OwnedSemaphorePermit,
    },
    WorkspaceTest {
        _gate: Arc<CargoWorkspaceGate>,
        _slot: OwnedSemaphorePermit,
        _access: OwnedRwLockReadGuard<()>,
    },
    WorkspaceExclusive {
        _gate: Arc<CargoWorkspaceGate>,
        _access: OwnedRwLockWriteGuard<()>,
    },
}

fn cargo_workspace_gates() -> &'static Mutex<HashMap<PathBuf, Weak<CargoWorkspaceGate>>> {
    static GATES: std::sync::OnceLock<Mutex<HashMap<PathBuf, Weak<CargoWorkspaceGate>>>> =
        std::sync::OnceLock::new();
    GATES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cargo_workspace_gate(root: &Path) -> Arc<CargoWorkspaceGate> {
    let mut gates = cargo_workspace_gates()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    gates.retain(|_, gate| gate.strong_count() > 0);
    if let Some(gate) = gates.get(root).and_then(Weak::upgrade) {
        return gate;
    }
    let gate = Arc::new(CargoWorkspaceGate::new());
    gates.insert(root.to_path_buf(), Arc::downgrade(&gate));
    gate
}

fn cargo_gate_timeout_error() -> anyhow::Error {
    anyhow!("cargo contention gate remained busy for the bounded queue wait")
}

async fn acquire_cargo_contention_gate(
    program: &str,
    args: &[String],
    workspace_root: &Path,
    wait_timeout: Duration,
) -> Result<Option<CargoContentionPermit>> {
    let lane = match cargo_contention_lane(program, args) {
        Some(lane) => lane,
        None => return Ok(None),
    };
    let deadline = tokio::time::Instant::now() + wait_timeout;
    match lane {
        CargoContentionLane::Registry => {
            let permit = tokio::time::timeout(
                wait_timeout,
                Arc::clone(cargo_registry_gate()).acquire_owned(),
            )
            .await
            .map_err(|_| cargo_gate_timeout_error())?
            .map_err(|_| anyhow!("cargo contention gate is shutting down"))?;
            Ok(Some(CargoContentionPermit::Registry { _permit: permit }))
        }
        CargoContentionLane::WorkspaceTest => {
            let gate = cargo_workspace_gate(workspace_root);
            let slot = tokio::time::timeout(
                deadline.saturating_duration_since(tokio::time::Instant::now()),
                Arc::clone(&gate.test_slots).acquire_owned(),
            )
            .await
            .map_err(|_| cargo_gate_timeout_error())?
            .map_err(|_| anyhow!("cargo test contention gate is shutting down"))?;
            let access = tokio::time::timeout(
                deadline.saturating_duration_since(tokio::time::Instant::now()),
                Arc::clone(&gate.access).read_owned(),
            )
            .await
            .map_err(|_| cargo_gate_timeout_error())?;
            Ok(Some(CargoContentionPermit::WorkspaceTest {
                _gate: gate,
                _slot: slot,
                _access: access,
            }))
        }
        CargoContentionLane::WorkspaceExclusive => {
            let gate = cargo_workspace_gate(workspace_root);
            let access = tokio::time::timeout(
                deadline.saturating_duration_since(tokio::time::Instant::now()),
                Arc::clone(&gate.access).write_owned(),
            )
            .await
            .map_err(|_| cargo_gate_timeout_error())?;
            Ok(Some(CargoContentionPermit::WorkspaceExclusive {
                _gate: gate,
                _access: access,
            }))
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct VerificationCommandFlightKey {
    workspace_root: PathBuf,
    cwd: PathBuf,
    program: String,
    args: Vec<String>,
    revision: String,
    timeout_seconds: u64,
    lane: &'static str,
}

struct VerificationCommandFlight {
    result: Mutex<Option<std::result::Result<CommandResult, String>>>,
    notify: tokio::sync::Notify,
}

enum VerificationCommandFlightClaim {
    Leader(VerificationCommandFlightLeader),
    Follower(Arc<VerificationCommandFlight>),
}

struct VerificationCommandFlightLeader {
    flight: Arc<VerificationCommandFlight>,
    completed: bool,
}

impl VerificationCommandFlight {
    fn new() -> Self {
        Self {
            result: Mutex::new(None),
            notify: tokio::sync::Notify::new(),
        }
    }

    fn publish(&self, result: std::result::Result<CommandResult, String>) {
        let mut slot = self
            .result
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if slot.is_none() {
            *slot = Some(result);
            drop(slot);
            self.notify.notify_waiters();
        }
    }

    async fn wait(&self) -> Result<CommandResult> {
        loop {
            let notified = self.notify.notified();
            if let Some(result) = self
                .result
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
            {
                return result.map_err(anyhow::Error::msg);
            }
            notified.await;
        }
    }
}

impl VerificationCommandFlightLeader {
    fn complete(&mut self, result: &Result<CommandResult>) {
        self.flight.publish(match result {
            Ok(result) => Ok(result.clone()),
            Err(error) => Err(error.to_string()),
        });
        self.completed = true;
    }
}

impl Drop for VerificationCommandFlightLeader {
    fn drop(&mut self) {
        if !self.completed {
            self.flight.publish(Err(
                "in-flight verification command leader ended before publishing a result".to_owned(),
            ));
        }
    }
}

fn verification_command_flights(
) -> &'static Mutex<HashMap<VerificationCommandFlightKey, Weak<VerificationCommandFlight>>> {
    static FLIGHTS: std::sync::OnceLock<
        Mutex<HashMap<VerificationCommandFlightKey, Weak<VerificationCommandFlight>>>,
    > = std::sync::OnceLock::new();
    FLIGHTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn claim_verification_command_flight(
    key: VerificationCommandFlightKey,
) -> VerificationCommandFlightClaim {
    let mut flights = verification_command_flights()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    flights.retain(|_, flight| flight.strong_count() > 0);
    if let Some(flight) = flights.get(&key).and_then(Weak::upgrade) {
        return VerificationCommandFlightClaim::Follower(flight);
    }
    let flight = Arc::new(VerificationCommandFlight::new());
    flights.insert(key, Arc::downgrade(&flight));
    VerificationCommandFlightClaim::Leader(VerificationCommandFlightLeader {
        flight,
        completed: false,
    })
}

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
        let unrestricted_commands =
            self.security.allow_unrestricted_commands || self.workspace_commands_granted();
        if !self.allow_exec && !unrestricted_commands {
            bail!("command execution is disabled; restart without --no-exec or explicitly authorize all commands for this Workspace");
        }
        if !unrestricted_commands {
            validate_authorizable_program(program)?;
            // Reject malformed/unavailable operations before creating an approval
            // request, so the operator never approves something that cannot run.
            let mut admissible = self.security;
            admissible.allow_risky_exec = true;
            validate_command_policy(program, args, admissible)?;
        }
        let development_program = LANGUAGE_DEVELOPMENT_COMMANDS.contains(&program);
        let broad_execution = sandbox::command_requires_sandbox(
            unrestricted_commands,
            self.allow_exec,
            self.allow_write,
            program,
            args,
        );
        let mut safe_development = self.security;
        safe_development.allow_unrestricted_commands = false;
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
        if !unrestricted_commands
            && !self.allow_write
            && command_requires_workspace_write(program, args)
            && validate_verification_command_shape(program, args).is_err()
        {
            bail!("command modifies repository state and is blocked in a read-only workspace");
        }
        if !unrestricted_commands
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
        if unrestricted_commands || (autonomous_development && development_program) {
            effective_security.allow_risky_exec = true;
        }
        if !unrestricted_commands
            && !effective_security.allow_risky_exec
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
        if !unrestricted_commands {
            validate_command_policy(program, args, effective_security)?;
        }
        let cwd = self.existing_path(cwd)?;
        if !cwd.is_dir() {
            bail!("cwd is not a directory");
        }
        let command_timeout = Duration::from_secs(timeout_seconds.clamp(1, 1800));
        let _cargo_contention_permit = acquire_cargo_contention_gate(
            program,
            args,
            &self.root,
            cargo_contention_wait(command_timeout),
        )
        .await?;
        let deadline = tokio::time::Instant::now() + command_timeout;
        let queue_wait = process_queue_wait(command_timeout);
        let governor = crate::resource::global();
        let (_probe_permit, _child_permit, process_queue_wait_ms) =
            if is_inspection_probe(program, args) {
                let (permit, wait_ms) = governor
                    .acquire_probe_with_wait_timeout(queue_wait)
                    .await
                    .map_err(anyhow::Error::msg)?;
                (Some(permit), None, wait_ms)
            } else {
                let (permit, wait_ms) = governor
                    .acquire_child_for_workspace_with_wait_timeout(&self.root, queue_wait)
                    .await
                    .map_err(anyhow::Error::msg)?;
                (None, Some(permit), wait_ms)
            };
        let effective_args = if broad_execution {
            args.to_vec()
        } else {
            hardened_command_args(program, args)
        };
        let executable = if Path::new(program).is_absolute() {
            PathBuf::from(program)
        } else if program.contains(['/', '\\']) {
            let executable = self.existing_path(program)?;
            ensure_workspace_executable(&executable)?;
            executable
        } else {
            PathBuf::from(program)
        };
        let mut sandbox_guard = None;
        let mut command = if broad_execution {
            let (command, guard) =
                sandbox::prepare(&self.root, &cwd, &executable, &effective_args)?;
            sandbox_guard = Some(guard);
            command
        } else {
            let mut command = Command::new(executable);
            command.args(&effective_args).current_dir(&cwd);
            command
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        scrub_sensitive_environment(
            &mut command,
            program,
            args,
            !broad_execution
                && (effective_security.allow_risky_exec
                    || (program == "git" && is_git_push_command(args))),
        );
        if program == "git" {
            command
                .env("GIT_CEILING_DIRECTORIES", &self.root)
                .env("GIT_DISCOVERY_ACROSS_FILESYSTEM", "0");
        }
        crate::resource::apply_child_limits(&mut command);

        let child = command.spawn().context("failed to start command")?;
        let result =
            collect_command_result(child, program, args, deadline, process_queue_wait_ms).await;
        drop(sandbox_guard);
        result
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
        if program.contains(['/', '\\']) {
            return self
                .run_workspace_verification_executable(program, args, cwd, timeout_seconds)
                .await;
        }
        validate_verification_command_shape(program, args)?;
        let mut verification_workspace = self.clone();
        verification_workspace.security.allow_risky_exec = true;
        verification_workspace
            .run_command(program, args, cwd, timeout_seconds)
            .await
    }

    fn verification_command_flight_key(
        &self,
        lane: &'static str,
        program: &str,
        args: &[String],
        cwd: &str,
        timeout_seconds: u64,
        revision: &str,
    ) -> Result<VerificationCommandFlightKey> {
        if revision.trim().is_empty() || revision.ends_with(":partial") {
            bail!("verification command coalescing requires a complete revision");
        }
        let cwd = self.existing_path(cwd)?;
        if !cwd.is_dir() {
            bail!("cwd is not a directory");
        }
        Ok(VerificationCommandFlightKey {
            workspace_root: self.root().to_path_buf(),
            cwd,
            program: program.to_owned(),
            args: args.to_vec(),
            revision: revision.to_owned(),
            timeout_seconds: timeout_seconds.clamp(1, 1800),
            lane,
        })
    }

    pub(crate) async fn run_command_at_revision(
        &self,
        program: &str,
        args: &[String],
        cwd: &str,
        timeout_seconds: u64,
        revision: &str,
    ) -> Result<CommandResult> {
        validate_verification_command_shape(program, args)?;
        let key = self.verification_command_flight_key(
            "run-command",
            program,
            args,
            cwd,
            timeout_seconds,
            revision,
        )?;
        match claim_verification_command_flight(key) {
            VerificationCommandFlightClaim::Follower(flight) => flight.wait().await,
            VerificationCommandFlightClaim::Leader(mut leader) => {
                let result = self.run_command(program, args, cwd, timeout_seconds).await;
                leader.complete(&result);
                result
            }
        }
    }

    pub(crate) async fn run_verification_command_at_revision(
        &self,
        program: &str,
        args: &[String],
        cwd: &str,
        timeout_seconds: u64,
        revision: &str,
    ) -> Result<CommandResult> {
        let key = self.verification_command_flight_key(
            "verification",
            program,
            args,
            cwd,
            timeout_seconds,
            revision,
        )?;
        match claim_verification_command_flight(key) {
            VerificationCommandFlightClaim::Follower(flight) => flight.wait().await,
            VerificationCommandFlightClaim::Leader(mut leader) => {
                let result = self
                    .run_verification_command(program, args, cwd, timeout_seconds)
                    .await;
                leader.complete(&result);
                result
            }
        }
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
        let command_timeout = Duration::from_secs(timeout_seconds.clamp(1, 1800));
        let deadline = tokio::time::Instant::now() + command_timeout;
        let (_child_permit, process_queue_wait_ms) = crate::resource::global()
            .acquire_child_for_workspace_with_wait_timeout(
                &self.root,
                process_queue_wait(command_timeout),
            )
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
        collect_command_result(child, program, args, deadline, process_queue_wait_ms).await
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
        let command_timeout = Duration::from_secs(timeout_seconds.clamp(1, 1800));
        let _cargo_contention_permit = acquire_cargo_contention_gate(
            program,
            args,
            &self.root,
            cargo_contention_wait(command_timeout),
        )
        .await?;
        let deadline = tokio::time::Instant::now() + command_timeout;
        let (_child_permit, process_queue_wait_ms) = crate::resource::global()
            .acquire_child_for_workspace_with_wait_timeout(
                &self.root,
                process_queue_wait(command_timeout),
            )
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
        collect_command_result(child, program, args, deadline, process_queue_wait_ms).await
    }
}

// Both execution lanes share cancellation ownership, bounded cleanup and the
// same failed-result contract. A timeout does not undo already-applied effects.
async fn collect_command_result(
    mut child: tokio::process::Child,
    program: &str,
    args: &[String],
    deadline: tokio::time::Instant,
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
    let waited = tokio::time::timeout_at(deadline, child.wait()).await;
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
