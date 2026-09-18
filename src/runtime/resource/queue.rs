use serde::Serialize;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError};

/// Admission occupancy, not a count of OS processes actively using the CPU.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ProcessQueueSnapshot {
    pub limit: usize,
    pub active: usize,
    pub waiting: usize,
    pub waits: u64,
    pub total_wait_ms: u64,
    pub max_wait_ms: u64,
    pub last_wait_ms: u64,
    pub last_wait_age_ms: Option<u64>,
}

pub(super) struct ProcessQueue {
    slots: Arc<Semaphore>,
    limit: usize,
    waiting: AtomicUsize,
    waits: AtomicU64,
    total_wait_us: AtomicU64,
    max_wait_us: AtomicU64,
    last_wait_us: AtomicU64,
    last_wait_at_ms: AtomicU64,
}

impl ProcessQueue {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            slots: Arc::new(Semaphore::new(limit)),
            limit,
            waiting: AtomicUsize::new(0),
            waits: AtomicU64::new(0),
            total_wait_us: AtomicU64::new(0),
            max_wait_us: AtomicU64::new(0),
            last_wait_us: AtomicU64::new(0),
            last_wait_at_ms: AtomicU64::new(0),
        }
    }

    #[cfg(test)]
    pub(super) async fn acquire(&self) -> Result<OwnedSemaphorePermit, String> {
        self.acquire_with_wait().await.map(|(permit, _)| permit)
    }

    pub(super) async fn acquire_with_wait(&self) -> Result<(OwnedSemaphorePermit, u64), String> {
        match self.slots.clone().try_acquire_owned() {
            Ok(permit) => return Ok((permit, 0)),
            Err(TryAcquireError::Closed) => {
                return Err("resource governor is shutting down".to_owned());
            }
            Err(TryAcquireError::NoPermits) => {}
        }
        self.waiting.fetch_add(1, Ordering::Relaxed);
        self.waits.fetch_add(1, Ordering::Relaxed);
        // Cancellation must remove both the semaphore waiter and its telemetry.
        let wait = QueueWait {
            queue: self,
            started: Instant::now(),
        };
        let permit = self
            .slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "resource governor is shutting down".to_owned())?;
        let wait_ms = u64::try_from(wait.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        Ok((permit, wait_ms))
    }

    pub(super) fn snapshot(&self) -> ProcessQueueSnapshot {
        let last_wait_at_ms = self.last_wait_at_ms.load(Ordering::Relaxed);
        ProcessQueueSnapshot {
            limit: self.limit,
            active: self.limit.saturating_sub(self.slots.available_permits()),
            waiting: self.waiting.load(Ordering::Relaxed),
            waits: self.waits.load(Ordering::Relaxed),
            total_wait_ms: self.total_wait_us.load(Ordering::Relaxed) / 1_000,
            max_wait_ms: self.max_wait_us.load(Ordering::Relaxed) / 1_000,
            last_wait_ms: self.last_wait_us.load(Ordering::Relaxed) / 1_000,
            last_wait_age_ms: (last_wait_at_ms != 0)
                .then(|| unix_time_ms().saturating_sub(last_wait_at_ms)),
        }
    }
}

struct QueueWait<'a> {
    queue: &'a ProcessQueue,
    started: Instant,
}

impl Drop for QueueWait<'_> {
    fn drop(&mut self) {
        let micros = u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX);
        self.queue.waiting.fetch_sub(1, Ordering::Relaxed);
        let _ =
            self.queue
                .total_wait_us
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |total| {
                    Some(total.saturating_add(micros))
                });
        self.queue.max_wait_us.fetch_max(micros, Ordering::Relaxed);
        self.queue.last_wait_us.store(micros, Ordering::Relaxed);
        self.queue
            .last_wait_at_ms
            .store(unix_time_ms(), Ordering::Relaxed);
    }
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "../../../tests/unit/runtime/process_admission.rs"]
mod admission_tests;
#[cfg(test)]
#[path = "../../../tests/unit/runtime/process_queue.rs"]
mod tests;
