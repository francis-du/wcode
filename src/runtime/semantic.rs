use crate::harness::ToolHarness;
use crate::monitor::TaskMonitor;
use crate::semantic_provider::{self, SemanticAutoState};
use crate::workspace::{Workspace, Workspaces};
use futures_util::FutureExt;
use std::collections::BTreeSet;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::sleep;

const AUTO_FILES: usize = 128;
const AUTO_SYMBOLS: usize = 1_000;
const AUTO_PROBE_CONCURRENCY: usize = 2;
const CHANGE_POLL: Duration = Duration::from_secs(2);
const IDLE_POLL: Duration = Duration::from_secs(15);
const PRESSURE_POLL: Duration = Duration::from_secs(3);
const DISCOVERY_POLL: Duration = Duration::from_secs(30);
const MIN_RETRY: Duration = Duration::from_secs(10);
const MAX_RETRY: Duration = Duration::from_secs(300);

pub(crate) fn spawn(
    workspaces: Workspaces,
    harness: ToolHarness,
    monitor: TaskMonitor,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut workers = JoinSet::new();
        let mut running = BTreeSet::new();
        let probe_slots = Arc::new(Semaphore::new(AUTO_PROBE_CONCURRENCY));
        loop {
            harness.prune_semantic_sessions();
            for (workspace_id, workspace) in workspaces.semantic_workspaces() {
                if running.insert(workspace_id.clone()) {
                    let harness = harness.clone();
                    let monitor = monitor.clone();
                    let probe_slots = Arc::clone(&probe_slots);
                    workers.spawn(async move {
                        let completed = worker_completed(maintain_workspace(
                            &workspace_id,
                            workspace,
                            harness,
                            monitor,
                            probe_slots,
                        ))
                        .await;
                        if !completed {
                            // A provider or filesystem panic must not permanently remove this
                            // workspace from automatic semantic maintenance.
                            sleep(MIN_RETRY).await;
                        }
                        workspace_id
                    });
                }
            }
            tokio::select! {
                completed = workers.join_next(), if !workers.is_empty() => {
                    if let Some(Ok(workspace_id)) = completed {
                        running.remove(&workspace_id);
                    }
                }
                _ = sleep(DISCOVERY_POLL) => {}
            }
        }
    })
}

async fn worker_completed<F>(worker: F) -> bool
where
    F: Future<Output = ()>,
{
    AssertUnwindSafe(worker).catch_unwind().await.is_ok()
}

async fn maintain_workspace(
    workspace_id: &str,
    workspace: Workspace,
    harness: ToolHarness,
    monitor: TaskMonitor,
    probe_slots: Arc<Semaphore>,
) {
    let mut indexed_fingerprint = None::<String>;
    let mut candidate_fingerprint = None::<String>;
    let mut retry = MIN_RETRY;
    loop {
        if !crate::resource::global().background_ready() {
            sleep(PRESSURE_POLL).await;
            continue;
        }
        let probe_permit = match Arc::clone(&probe_slots).acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => return,
        };
        let probe_workspace = workspace.clone();
        let state = tokio::task::spawn_blocking(move || {
            semantic_provider::automatic_state(&probe_workspace, AUTO_FILES)
        })
        .await;
        drop(probe_permit);
        let state = match state {
            Ok(Ok(state)) => state,
            _ => {
                sleep(retry).await;
                retry = doubled_retry(retry);
                continue;
            }
        };
        if state.providers == 0 || state.files == 0 {
            candidate_fingerprint = None;
            sleep(DISCOVERY_POLL).await;
            continue;
        }
        if indexed_fingerprint.as_deref() == Some(state.fingerprint.as_str()) {
            candidate_fingerprint = None;
            retry = MIN_RETRY;
            sleep(IDLE_POLL).await;
            continue;
        }
        if candidate_fingerprint.as_deref() != Some(state.fingerprint.as_str()) {
            candidate_fingerprint = Some(state.fingerprint.clone());
            sleep(CHANGE_POLL).await;
            continue;
        }

        if !crate::resource::global().background_ready() {
            sleep(PRESSURE_POLL).await;
            continue;
        }
        let success = refresh_workspace(workspace_id, &workspace, &harness, &monitor, &state).await;
        candidate_fingerprint = None;
        if success {
            indexed_fingerprint = Some(state.fingerprint);
            retry = MIN_RETRY;
            sleep(IDLE_POLL).await;
        } else {
            sleep(retry).await;
            retry = doubled_retry(retry);
        }
    }
}

async fn refresh_workspace(
    workspace_id: &str,
    workspace: &Workspace,
    harness: &ToolHarness,
    monitor: &TaskMonitor,
    state: &SemanticAutoState,
) -> bool {
    let mut ticket = monitor.queue(
        workspace_id.to_owned(),
        "semantic_auto",
        format!(
            "auto LSP · {} provider(s) · {} file(s){}",
            state.providers,
            state.files,
            if state.truncated { " · bounded" } else { "" }
        ),
        0,
    );
    let _permit = match harness.acquire().await {
        Ok(permit) => permit,
        Err(_) => {
            ticket.finish(false, 0);
            return false;
        }
    };
    ticket.start();
    let result = harness
        .semantic_provider_refresh_automatic(workspace, ".", AUTO_FILES, AUTO_SYMBOLS)
        .await;
    let (success, response_bytes) = match result {
        Ok(refresh) => {
            let success = !refresh.runs.is_empty() && refresh.failures.is_empty();
            let bytes = serde_json::to_vec(&refresh).map_or(0, |bytes| bytes.len() as u64);
            refresh_monitor_state(workspace_id, workspace, harness, monitor);
            (success, bytes)
        }
        Err(_) => (false, 0),
    };
    ticket.finish(success, response_bytes);
    success
}

fn refresh_monitor_state(
    workspace_id: &str,
    workspace: &Workspace,
    harness: &ToolHarness,
    monitor: &TaskMonitor,
) {
    if let Ok(status) = harness.semantic_provider_status(workspace) {
        if let Ok(value) = serde_json::to_value(status) {
            monitor.record_intelligence_result(workspace_id, "semantic_provider_status", &value);
        }
    }
    if let Ok(value) = serde_json::to_value(harness.semantic_session_status(workspace)) {
        monitor.record_intelligence_result(workspace_id, "semantic_session_status", &value);
    }
    if let Ok(status) = harness.graph_provider_status(workspace) {
        if let Ok(value) = serde_json::to_value(status) {
            monitor.record_intelligence_result(workspace_id, "graph_provider_status", &value);
        }
    }
}

fn doubled_retry(current: Duration) -> Duration {
    current.saturating_mul(2).min(MAX_RETRY)
}

#[cfg(test)]
#[path = "../../tests/unit/runtime/semantic.rs"]
mod tests;
