//! Foreground delivery polling only; the inbox still owns claims and retries.
//! Shutdown stops new polls and lets the current publication/persistence finish.
use crate::git_provider::github::GitHubGateReceipt;
use crate::git_provider::GateVerdict;
use anyhow::Result;
use serde::Serialize;
use serde_json::{json, Value};
use std::future::Future;
use std::time::Duration;
use tokio::time::{sleep_until, Instant};

const IDLE_DELAY: Duration = Duration::from_secs(1);
const ERROR_DELAY: Duration = Duration::from_secs(5);

#[derive(Debug, Default, Serialize)]
pub(super) struct WorkerSummary {
    polls: u64,
    publications: u64,
    blocked_publications: u64,
    unavailable_polls: u64,
    current_acceptance: bool,
}

pub(super) async fn run<F, Fut, S, E>(attempt: F, shutdown: S, emit: E) -> Result<WorkerSummary>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Option<GitHubGateReceipt>>>,
    S: Future<Output = Result<()>>,
    E: FnMut(Value) -> Result<()>,
{
    drive(attempt, shutdown, emit, IDLE_DELAY, ERROR_DELAY).await
}

async fn drive<F, Fut, S, E>(
    mut attempt: F,
    shutdown: S,
    mut emit: E,
    idle_delay: Duration,
    error_delay: Duration,
) -> Result<WorkerSummary>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Option<GitHubGateReceipt>>>,
    S: Future<Output = Result<()>>,
    E: FnMut(Value) -> Result<()>,
{
    let mut summary = WorkerSummary::default();
    tokio::pin!(shutdown);
    emit(json!({"event":"worker_started", "current_acceptance":false,
        "shutdown":"finish_current_attempt_then_stop", "poll_interval_ms":idle_delay.as_millis(),
        "error_interval_ms":error_delay.as_millis()}))?;
    let mut next = Instant::now();
    loop {
        // Prefer an already requested shutdown over starting more work. Signals
        // stay registered while attempt() is awaited; we never abort its locks.
        tokio::select! {
            biased;
            signal = &mut shutdown => {
                signal?;
                emit(json!({"event":"worker_stopped", "summary":&summary}))?;
                return Ok(summary);
            }
            _ = sleep_until(next) => {}
        }
        let result = attempt().await;
        summary.polls = summary.polls.saturating_add(1);
        let delay = match result {
            Ok(Some(receipt)) => {
                summary.publications = summary.publications.saturating_add(1);
                if receipt.verdict() != GateVerdict::Ready {
                    summary.blocked_publications = summary.blocked_publications.saturating_add(1);
                }
                // A fresh receipt is reported only after publish_next has durably
                // finished. Counters never substitute for current native proof.
                emit(json!({"event":"publication_finished", "receipt":receipt,
                    "summary":&summary}))?;
                idle_delay
            }
            Ok(None) => idle_delay,
            Err(_) => {
                summary.unavailable_polls = summary.unavailable_polls.saturating_add(1);
                emit(
                    json!({"event":"publication_unavailable", "summary":&summary,
                    "action":"inspect inbox-status; persisted retry deadlines and attempt limits remain in force"}),
                )?;
                error_delay
            }
        };
        // Schedule from completion, not from a missed interval: slow work must
        // never produce a catch-up burst or overlapping publication attempts.
        next = Instant::now() + delay;
    }
}

pub(in crate::app::github) use crate::app::shutdown::signal as shutdown_signal;

#[cfg(test)]
#[path = "../../tests/unit/app/github_worker.rs"]
mod tests;
