use super::*;

const DEFAULT_VERIFICATION_TIMEOUT_SECONDS: u64 = 120;
const COLD_RUST_GATE_TIMEOUT_FLOOR_SECONDS: u64 = 300;

fn verification_check_timeout_seconds(check: &CheckSpec, requested: u64) -> u64 {
    let requested = requested.clamp(1, 1800);
    let canonical_full_rust_gate = check.level == "full"
        && ((check.id == "rust-test" && check.phase == 1)
            || (check.id == "rust-release-build" && check.phase == 3));
    if canonical_full_rust_gate && requested == DEFAULT_VERIFICATION_TIMEOUT_SECONDS {
        COLD_RUST_GATE_TIMEOUT_FLOOR_SECONDS
    } else {
        requested
    }
}

/// Discovery is a precondition of matrix coverage, never a skipped command.
/// Include the same binding in execution and frozen plans so a known subset
/// cannot replace an unavailable project inventory.
pub(super) fn discovery_completeness_check(
    profile: &ProjectProfile,
    level: &str,
) -> Option<CheckSpec> {
    if profile.discovery.complete {
        return None;
    }
    let mut reasons = profile.discovery.reasons.clone();
    reasons.sort();
    reasons.dedup();
    if reasons.is_empty() {
        reasons.push("discovery_incomplete".to_owned());
    }
    Some(CheckSpec {
        id: "profile-discovery-completeness".to_owned(),
        level: level.to_owned(),
        phase: 0,
        program: "wcode-discovery".to_owned(),
        args: reasons,
        cwd: ".".to_owned(),
        island: "workspace".to_owned(),
        languages: Vec::new(),
        reason: "Project discovery is incomplete; observed checks remain valid, but the complete verification matrix is unavailable.".to_owned(),
    })
}

impl ToolHarness {
    pub(crate) fn current_workspace_revision_key(
        &self,
        workspace: &Workspace,
    ) -> Result<Option<String>> {
        let revision = self.intelligence.current_revision(workspace)?;
        if revision.code.ends_with(":partial")
            || revision
                .design
                .as_deref()
                .is_some_and(|value| value.ends_with(":partial"))
        {
            return Ok(None);
        }
        Ok(Some(format!(
            "code={};design={}",
            revision.code,
            revision.design.as_deref().unwrap_or("none")
        )))
    }
}

pub(super) async fn run_verification_check<T: TaskTelemetry>(
    harness: ToolHarness,
    monitor: T,
    workspace_id: String,
    workspace: Workspace,
    check: CheckSpec,
    revision_key: Option<String>,
    timeout_seconds: u64,
) -> VerificationCheck {
    let command = verification_command_text(&check);
    let request_bytes = command.len() as u64;
    let task = monitor.queue(
        workspace_id,
        format!("verify:{}", check.id),
        format!("phase {} · {command}", check.phase),
        request_bytes,
    );
    if check.id == "profile-discovery-completeness" && check.program == "wcode-discovery" {
        task.start();
        let message = format!(
            "{} Discovery reasons: {}.",
            check.reason,
            check.args.join(", ")
        );
        task.finish(false, message.len() as u64);
        let mut evaluated = verification_error(check, message, 0);
        evaluated.execution = crate::evidence::VerificationCheckExecution::Executed;
        return evaluated;
    }
    let _permit = match harness.acquire_tool(true).await {
        Ok(permit) => permit,
        Err(error) => {
            task.finish(false, error.len() as u64);
            return verification_error(check, error, 0);
        }
    };
    task.start();
    let started = Instant::now();

    let check_timeout_seconds = verification_check_timeout_seconds(&check, timeout_seconds);
    let result = match revision_key.as_deref() {
        Some(revision) => {
            workspace
                .run_verification_command_at_revision(
                    &check.program,
                    &check.args,
                    &check.cwd,
                    check_timeout_seconds,
                    revision,
                )
                .await
        }
        None => {
            workspace
                .run_verification_command(
                    &check.program,
                    &check.args,
                    &check.cwd,
                    check_timeout_seconds,
                )
                .await
        }
    };

    match result {
        Ok(result) => {
            let success = result.success;
            let response_bytes = result.stdout.len().saturating_add(result.stderr.len()) as u64;
            let report = verification_check(check, result, started.elapsed().as_millis());
            task.finish(success, response_bytes);
            report
        }
        Err(error) => {
            let message = verification_command_error(&check, &error);
            let report = verification_error(check, message.clone(), started.elapsed().as_millis());
            task.finish(false, message.len() as u64);
            report
        }
    }
}

/// Deterministic observation reduction: retain test totals and the log tail on
/// success. Failed diagnostics retain the existing, larger output allowance.
pub(super) fn verification_output(text: &str, success: bool) -> (String, bool) {
    if !success || text.chars().nth(2_048).is_none() {
        return tail_chars(text, MAX_CHECK_OUTPUT_CHARS);
    }
    let mut summaries = String::new();
    let mut summary_chars = 0usize;
    for line in text
        .lines()
        .filter(|line| line.trim_start().starts_with("test result:"))
    {
        let line_chars = line.chars().count();
        if summary_chars.saturating_add(line_chars).saturating_add(1) > 1_024 {
            break;
        }
        summaries.push_str(line);
        summaries.push('\n');
        summary_chars = summary_chars.saturating_add(line_chars).saturating_add(1);
    }
    let (tail, _) = tail_chars(text, 1_000);
    (
        format!("{summaries}[successful log compacted]\n{tail}"),
        true,
    )
}

fn verification_command_error(check: &CheckSpec, error: &anyhow::Error) -> String {
    let details = format!("{error:#}");
    let missing = error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    });
    if missing {
        format!(
            "verification executable `{}` is unavailable; install the project toolchain or declare a runnable verification provider, then retry verify_project. {details}",
            check.program
        )
    } else {
        details
    }
}

fn verification_error(check: CheckSpec, error: String, elapsed_ms: u128) -> VerificationCheck {
    let signature = verification_check_binding(&check).signature;
    let command = verification_command_text(&check);
    VerificationCheck {
        id: check.id,
        phase: check.phase,
        command,
        reason: check.reason,
        success: false,
        reused: false,
        execution: crate::evidence::VerificationCheckExecution::Unavailable,
        exit_code: None,
        elapsed_ms,
        queue_wait_ms: 0,
        execution_ms: elapsed_ms,
        stdout_tail: String::new(),
        stderr_tail: error,
        output_truncated: false,
        signature: Some(signature),
        evidence_id: None,
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/runtime/harness/verification_errors.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../tests/unit/runtime/harness/discovery_gate.rs"]
mod discovery_gate;
