use super::*;

pub(super) const PUBLIC_HEALTH_TIMEOUT: Duration = Duration::from_secs(15);
pub(super) const PUBLIC_HEALTH_PARALLELISM: usize = 4;
static PUBLIC_HEALTH_SLOTS: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
    std::sync::OnceLock::new();

async fn acquire_public_health_slot() -> tokio::sync::OwnedSemaphorePermit {
    PUBLIC_HEALTH_SLOTS
        .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(PUBLIC_HEALTH_PARALLELISM)))
        .clone()
        .acquire_owned()
        .await
        .expect("public health probe semaphore closed")
}

const PUBLIC_TRANSIENT_RECHECKS: usize = 2;
const PUBLIC_TRANSIENT_RECHECK_DELAY: Duration = Duration::from_millis(250);
const PUBLIC_STARTUP_HEALTH_ATTEMPTS: usize = 4;

pub(crate) async fn wait_for_public_endpoint(
    public_url: &str,
    instance_id: &str,
    monitor: &TaskMonitor,
) -> Result<(), String> {
    monitor.operator_message(
        OperatorMessageKind::Info,
        "endpoint",
        "verifying this wcode instance",
    );
    let mut last_error = String::new();
    for attempt in 1..=PUBLIC_STARTUP_HEALTH_ATTEMPTS {
        match check_public_endpoint(public_url, instance_id).await {
            Ok(()) => {
                monitor.mark_public_url_check(true, None);
                monitor.operator_message(
                    OperatorMessageKind::Success,
                    "endpoint",
                    "reachable and instance-matched",
                );
                return Ok(());
            }
            Err(error) => {
                last_error = error;
                monitor.mark_public_url_check(false, Some(last_error.clone()));
                monitor.operator_message(
                    OperatorMessageKind::Warning,
                    "endpoint",
                    format!(
                        "attempt {attempt}/{PUBLIC_STARTUP_HEALTH_ATTEMPTS} failed · {}",
                        truncate_diagnostic(&last_error, 180)
                    ),
                );
                if attempt < PUBLIC_STARTUP_HEALTH_ATTEMPTS {
                    sleep(Duration::from_secs(attempt.min(3) as u64)).await;
                }
            }
        }
    }
    Err(last_error)
}

pub(crate) async fn check_public_endpoint_resilient(
    public_url: &str,
    expected_instance_id: &str,
) -> Result<(), String> {
    match check_public_endpoint(public_url, expected_instance_id).await {
        Ok(()) => Ok(()),
        Err(mut last_error) => {
            for _ in 0..PUBLIC_TRANSIENT_RECHECKS {
                sleep(PUBLIC_TRANSIENT_RECHECK_DELAY).await;
                match check_public_endpoint(public_url, expected_instance_id).await {
                    Ok(()) => return Ok(()),
                    Err(error) => last_error = error,
                }
            }
            Err(last_error)
        }
    }
}

pub(crate) async fn check_public_endpoint(
    public_url: &str,
    expected_instance_id: &str,
) -> Result<(), String> {
    // Health checks are runtime infrastructure, not model-facing commands. Keep
    // them independently bounded so retained aliases cannot bypass the normal
    // child-process budget or create a curl storm during a provider outage.
    let _probe_slot = acquire_public_health_slot().await;
    let health_url = format!("{public_url}/healthz/probe");
    let mut command = Command::new("curl");
    command
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--connect-timeout",
            "10",
            "--max-time",
            "15",
            &health_url,
        ])
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let output = timeout(
        PUBLIC_HEALTH_TIMEOUT + Duration::from_secs(1),
        command.output(),
    )
    .await
    .map_err(|_| {
        format!(
            "health check timed out after {}s",
            PUBLIC_HEALTH_TIMEOUT.as_secs()
        )
    })?
    .map_err(|error| format!("curl could not run: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "curl exited with {}{}",
            output.status,
            if stderr.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", truncate_diagnostic(stderr.trim(), 180))
            }
        ));
    }
    validate_health_response(&output.stdout, expected_instance_id)
}

pub(crate) fn validate_health_response(
    body: &[u8],
    expected_instance_id: &str,
) -> Result<(), String> {
    let payload: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| format!("health endpoint returned invalid JSON: {error}"))?;
    let actual = payload
        .get("instance_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "health response is missing instance_id".to_owned())?;
    if actual != expected_instance_id {
        return Err(format!(
            "health response belongs to a different wcode instance ({})",
            truncate_diagnostic(actual, 12)
        ));
    }
    if payload.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err("health response did not report ok=true".to_owned());
    }
    Ok(())
}
