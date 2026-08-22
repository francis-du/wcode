use anyhow::{bail, Result};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const MAX_PARALLEL_TOOLS: usize = 64;

#[derive(Clone)]
pub struct ToolHarness {
    slots: Arc<Semaphore>,
    max_parallel: usize,
}

impl ToolHarness {
    pub fn new(max_parallel: usize) -> Result<Self> {
        if !(1..=MAX_PARALLEL_TOOLS).contains(&max_parallel) {
            bail!("max parallel tools must be between 1 and {MAX_PARALLEL_TOOLS}");
        }
        Ok(Self {
            slots: Arc::new(Semaphore::new(max_parallel)),
            max_parallel,
        })
    }

    pub fn max_parallel(&self) -> usize {
        self.max_parallel
    }

    pub async fn acquire(&self) -> Result<OwnedSemaphorePermit, String> {
        self.slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "tool harness is shutting down".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn enforces_parallel_limit() {
        let harness = ToolHarness::new(2).unwrap();
        let first = harness.acquire().await.unwrap();
        let second = harness.acquire().await.unwrap();
        assert_eq!(harness.slots.available_permits(), 0);
        drop(first);
        assert_eq!(harness.slots.available_permits(), 1);
        drop(second);
    }

    #[test]
    fn rejects_unbounded_parallelism() {
        assert!(ToolHarness::new(0).is_err());
        assert!(ToolHarness::new(MAX_PARALLEL_TOOLS + 1).is_err());
    }
}
