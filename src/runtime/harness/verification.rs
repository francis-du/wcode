use super::*;

pub(super) async fn run_verification_check(
    harness: ToolHarness,
    monitor: TaskMonitor,
    workspace_id: String,
    workspace: Workspace,
    check: CheckSpec,
    timeout_seconds: u64,
) -> VerificationCheck {
    let command = command_text(&check.program, &check.args);
    let request_bytes = command.len() as u64;
    let mut task = monitor.queue(
        workspace_id,
        format!("verify:{}", check.id),
        format!("phase {} · {command}", check.phase),
        request_bytes,
    );
    let _permit = match harness.acquire().await {
        Ok(permit) => permit,
        Err(error) => return verification_error(check, error, 0),
    };
    task.start();
    let started = Instant::now();

    match workspace
        .run_verification_command(
            &check.program,
            &check.args,
            ".",
            timeout_seconds.clamp(1, 300),
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

fn verification_error(check: CheckSpec, error: String, elapsed_ms: u128) -> VerificationCheck {
    VerificationCheck {
        id: check.id,
        phase: check.phase,
        command: command_text(&check.program, &check.args),
        reason: check.reason,
        success: false,
        exit_code: None,
        elapsed_ms,
        stdout_tail: String::new(),
        stderr_tail: error,
        output_truncated: false,
    }
}
