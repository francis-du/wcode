use super::*;

pub(super) fn endpoint_runtime_key(public_url: &str) -> String {
    format!("endpoint:{public_url}")
}

pub(super) fn provider_runtime_key(provider: &str) -> String {
    format!("provider:{provider}")
}

pub(super) fn apply_public_url_check(
    state: &mut MonitorState,
    success: bool,
    error: Option<String>,
) {
    state.public_url_last_checked = Some(Instant::now());
    if success {
        state.public_url_consecutive_failures = 0;
        state.public_url_error = None;
        if state.public_url_healthy == Some(false) {
            state.public_url_consecutive_successes =
                state.public_url_consecutive_successes.saturating_add(1);
            if state.public_url_consecutive_successes >= 2 {
                state.public_url_healthy = Some(true);
                state.public_url_consecutive_successes = 0;
            }
        } else {
            state.public_url_healthy = Some(true);
            state.public_url_consecutive_successes = 0;
        }
    } else {
        state.public_url_consecutive_successes = 0;
        state.public_url_consecutive_failures =
            state.public_url_consecutive_failures.saturating_add(1);
        state.public_url_error = error;
        if state.public_url_consecutive_failures >= 3 {
            state.public_url_healthy = Some(false);
        }
    }
}

pub(super) fn tunnel_runtime_statuses(
    state: &MonitorState,
    now: Instant,
) -> Vec<MonitorTunnelRuntimeStatus> {
    state
        .tunnel_runtime
        .values()
        .take(MAX_TUNNEL_RUNTIME_STATUSES)
        .map(|runtime| MonitorTunnelRuntimeStatus {
            provider: runtime.provider.clone(),
            url: runtime.url.clone(),
            role: runtime.role.clone(),
            state: runtime.state.clone(),
            lease_age_seconds: runtime
                .lease_verified_at
                .map(|verified| now.saturating_duration_since(verified).as_secs()),
            consecutive_failures: runtime.consecutive_failures,
            death_count: runtime.death_count,
            circuit_open: runtime.circuit_open,
            retry_in_seconds: runtime
                .retry_at
                .map(|due| due.saturating_duration_since(now).as_secs()),
            connected_seconds: runtime
                .connected_at
                .map(|connected| now.saturating_duration_since(connected).as_secs()),
        })
        .collect()
}

/// Keep the operator-facing task target bounded and single-line so a long or
/// multi-line path cannot stretch the activity row or smuggle markup. The value
/// is stored verbatim: trimming would make a rejected call look like a real
/// changed file, and would hide a legitimate path that merely has surrounding
/// whitespace.
pub(super) fn bounded_target(target: Option<String>) -> Option<String> {
    let target = target?;
    if target.trim().is_empty() || target.len() > 256 || target.contains(['\n', '\r']) {
        return None;
    }
    Some(target)
}

pub(super) fn trim_history(state: &mut MonitorState, now: Instant) {
    while state
        .traffic
        .front()
        .is_some_and(|event| now.saturating_duration_since(event.at) > TRAFFIC_WINDOW)
    {
        state.traffic.pop_front();
    }
    while state.traffic.len() > MAX_TRAFFIC_EVENTS {
        state.traffic.pop_front();
    }
    while state.tasks.len() > MAX_RECENT_TASKS {
        let removable = state
            .tasks
            .iter()
            .position(|task| matches!(task.status, TaskStatus::Completed | TaskStatus::Failed));
        if let Some(index) = removable {
            state.tasks.remove(index);
        } else {
            break;
        }
    }
}

pub(super) struct TerminalClaimGuard(pub(super) TaskMonitor);

impl Drop for TerminalClaimGuard {
    fn drop(&mut self) {
        self.0.release_terminal();
    }
}

impl TaskTicket {
    pub fn start(&self) {
        self.inner.monitor.start(self.inner.id);
    }

    pub fn finish(self, success: bool, response_bytes: u64) {
        self.finish_once(success, response_bytes, 0);
    }

    pub fn finish_with_context_savings(
        self,
        success: bool,
        response_bytes: u64,
        context_bytes_avoided: u64,
    ) {
        self.finish_once(success, response_bytes, context_bytes_avoided);
    }

    fn finish_once(&self, success: bool, response_bytes: u64, context_bytes_avoided: u64) {
        if !self.inner.finished.swap(true, Ordering::AcqRel) {
            self.inner.monitor.finish(
                self.inner.id,
                success,
                response_bytes,
                context_bytes_avoided,
            );
        }
    }
}

impl Drop for TaskTicketInner {
    fn drop(&mut self) {
        if !self.finished.swap(true, Ordering::AcqRel) {
            self.monitor.finish(self.id, false, 0, 0);
        }
    }
}

impl MonitorRenderer {
    pub fn interrupt_receiver(&self) -> watch::Receiver<bool> {
        self.interrupted.clone()
    }

    pub async fn stop(self) {
        let _ = self.stop.send(true);
        let _ = self.join.await;
    }
}
