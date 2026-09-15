use super::*;

pub(super) async fn run_verification_check(
    harness: ToolHarness,
    monitor: TaskMonitor,
    workspace_id: String,
    workspace: Workspace,
    check: CheckSpec,
    timeout_seconds: u64,
) -> VerificationCheck {
    let command = verification_command_text(&check);
    let request_bytes = command.len() as u64;
    let mut task = monitor.queue(
        workspace_id,
        format!("verify:{}", check.id),
        format!("phase {} · {command}", check.phase),
        request_bytes,
    );
    let _permit = match harness.acquire_tool(true).await {
        Ok(permit) => permit,
        Err(error) => return verification_error(check, error, 0),
    };
    task.start();
    let started = Instant::now();

    match workspace
        .run_verification_command(
            &check.program,
            &check.args,
            &check.cwd,
            timeout_seconds.clamp(1, 1800),
        )
        .await
    {
        Ok(result) => {
            let success = result.success;
            let response_bytes = result.stdout.len().saturating_add(result.stderr.len()) as u64;
            let report = verification_check(check, result, started.elapsed().as_millis());
            task.finish(success, response_bytes);
            report
        }
        Err(error) => {
            let message = error.to_string();
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

fn verification_error(check: CheckSpec, error: String, elapsed_ms: u128) -> VerificationCheck {
    let command = verification_command_text(&check);
    VerificationCheck {
        id: check.id,
        phase: check.phase,
        command,
        reason: check.reason,
        success: false,
        reused: false,
        exit_code: None,
        elapsed_ms,
        queue_wait_ms: 0,
        execution_ms: elapsed_ms,
        stdout_tail: String::new(),
        stderr_tail: error,
        output_truncated: false,
    }
}
