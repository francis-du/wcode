use super::{ProcessQueue, ResourceGovernor};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::sync::OwnedSemaphorePermit;

pub(super) struct SubspaceProcessQueues {
    queues: Mutex<HashMap<PathBuf, Weak<ProcessQueue>>>,
}

impl Default for SubspaceProcessQueues {
    fn default() -> Self {
        Self {
            queues: Mutex::new(HashMap::new()),
        }
    }
}

impl SubspaceProcessQueues {
    fn queue(&self, root: &Path, limit: usize) -> Arc<ProcessQueue> {
        let mut queues = self
            .queues
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        queues.retain(|_, queue| queue.strong_count() > 0);
        if let Some(queue) = queues.get(root).and_then(Weak::upgrade) {
            return queue;
        }
        let queue = Arc::new(ProcessQueue::new(limit.max(1)));
        queues.insert(root.to_path_buf(), Arc::downgrade(&queue));
        queue
    }
}

pub(crate) struct SubspaceChildPermit {
    _queue: Arc<ProcessQueue>,
    _local: OwnedSemaphorePermit,
    _host: OwnedSemaphorePermit,
}

impl ResourceGovernor {
    async fn acquire_child_for_workspace_with_wait(
        &self,
        workspace_root: &Path,
    ) -> Result<(SubspaceChildPermit, u64), String> {
        self.admit_tool().await?;
        let queue = self
            .subspace_child_slots
            .queue(workspace_root, self.limits.child_processes);
        let (local, local_wait_ms) = queue.acquire_with_wait().await?;
        let (host, host_wait_ms) = self.child_slot.acquire_with_wait().await?;
        Ok((
            SubspaceChildPermit {
                _queue: queue,
                _local: local,
                _host: host,
            },
            local_wait_ms.saturating_add(host_wait_ms),
        ))
    }

    pub(crate) async fn acquire_child_for_workspace_with_wait_timeout(
        &self,
        workspace_root: &Path,
        wait_for: Duration,
    ) -> Result<(SubspaceChildPermit, u64), String> {
        let wait_ms = u64::try_from(wait_for.as_millis()).unwrap_or(u64::MAX);
        match tokio::time::timeout(
            wait_for,
            self.acquire_child_for_workspace_with_wait(workspace_root),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(format!(
                "subspace/host process capacity remained busy for {wait_ms} ms; command was not started"
            )),
        }
    }

    #[cfg(test)]
    pub async fn acquire_child(&self) -> Result<OwnedSemaphorePermit, String> {
        self.admit_tool().await?;
        self.child_slot.acquire().await
    }

    #[cfg(test)]
    pub(crate) async fn acquire_child_for_workspace(
        &self,
        workspace_root: &Path,
    ) -> Result<SubspaceChildPermit, String> {
        self.acquire_child_for_workspace_with_wait(workspace_root)
            .await
            .map(|(permit, _)| permit)
    }
}
