//! Collect a finished child process into a bounded, redacted `CommandResult`.
use super::execution::redact_command_streams;
use super::*;

pub(super) async fn collect_command_result(
    mut child: tokio::process::Child,
    program: &str,
    args: &[String],
    deadline: tokio::time::Instant,
    process_queue_wait_ms: u64,
    sandbox: &'static str,
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
    // Task-mode callers may install a bounded progress sink so a long-running
    // application remains observable without changing normal command behavior.
    let progress = command_output_progress_sender();
    let mut readers = tokio::task::JoinSet::new();
    let stdout_progress = progress.clone();
    readers.spawn(async move {
        (
            true,
            read_bounded_stream(stdout, stdout_progress, false).await,
        )
    });
    readers.spawn(async move { (false, read_bounded_stream(stderr, progress, true).await) });
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
    let raw_stdout = stdout.clone();
    let (stdout, stderr, redacted) = redact_command_streams(stdout, stderr);
    #[cfg(unix)]
    let signaled = status
        .as_ref()
        .is_some_and(|status| std::os::unix::process::ExitStatusExt::signal(status).is_some());
    #[cfg(not(unix))]
    let signaled = false;
    Ok(CommandResult {
        program: program.to_owned(),
        args: args.to_vec(),
        exit_code: status.and_then(|status| status.code()),
        signaled,
        success,
        process_queue_wait_ms,
        stdout,
        raw_stdout: (!redacted).then_some(raw_stdout),
        stderr,
        truncated: truncated || output_incomplete,
        redacted,
        timed_out,
        output_incomplete,
        sandbox,
        retry_guidance: (!success).then_some(
            "Inspect actual effects and diagnostics before retrying. No rollback or automatic retry was performed.",
        ),
    })
}
