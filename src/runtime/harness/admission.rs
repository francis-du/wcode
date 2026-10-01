use super::*;
use std::future::Future;
use std::sync::atomic::Ordering;

#[derive(Default)]
pub(super) struct AdmissionWaiters {
    resource: AtomicUsize,
    execution: AtomicUsize,
    slot: AtomicUsize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AdmissionSnapshot {
    pub total_limit: usize,
    pub slots_in_use: usize,
    pub execution_limit: usize,
    pub execution_in_use: usize,
    pub waiting_for_resource: usize,
    pub waiting_for_execution: usize,
    pub waiting_for_slot: usize,
}

struct Waiting<'a>(&'a AtomicUsize);

impl<'a> Waiting<'a> {
    fn new(counter: &'a AtomicUsize) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self(counter)
    }
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

impl ToolHarness {
    pub(super) fn execution_limit_for(max_parallel: usize, _process_capacity: usize) -> usize {
        max_parallel
            .saturating_sub(max_parallel.div_ceil(8).min(4))
            .max(1)
    }

    pub(crate) fn execution_limit(max_parallel: usize) -> usize {
        Self::execution_limit_for(
            max_parallel,
            crate::resource::limits().host_child_process_limit(),
        )
    }

    /// Semaphore reservations, not UI task-start events. A waiter handed a
    /// permit may already be counted before its executor next gets polled.
    pub(crate) fn admission_snapshot(&self) -> AdmissionSnapshot {
        let execution_limit = Self::execution_limit(self.max_parallel);
        AdmissionSnapshot {
            total_limit: self.max_parallel,
            slots_in_use: self
                .max_parallel
                .saturating_sub(self.slots.available_permits()),
            execution_limit,
            execution_in_use: execution_limit
                .saturating_sub(self.execution_slots.available_permits()),
            waiting_for_resource: self.admission_waiters.resource.load(Ordering::Relaxed),
            waiting_for_execution: self.admission_waiters.execution.load(Ordering::Relaxed),
            waiting_for_slot: self.admission_waiters.slot.load(Ordering::Relaxed),
        }
    }

    /// Fixed-size, non-authoritative overload observation. No repository scans,
    /// task launch, credentials, cache locks or changes to admission capacity.
    pub(crate) fn overload_diagnostic(&self) -> Option<Value> {
        let admission = self.admission_snapshot();
        // The bounded status-admission fallback handles resource pressure using
        // the real governor. A stale memory sample must not pin this fast path.
        if admission.slots_in_use < admission.total_limit
            && admission.waiting_for_resource == 0
            && admission.waiting_for_execution == 0
            && admission.waiting_for_slot == 0
        {
            return None;
        }
        Some(self.admission_diagnostic())
    }

    pub(crate) fn admission_diagnostic(&self) -> Value {
        let admission = self.admission_snapshot();
        let resources = crate::resource::snapshot();
        json!({
            "status": "partial",
            "diagnostic_only": true,
            "scope": "current_process_admission_only",
            "runtime": {"version": env!("CARGO_PKG_VERSION"), "process_id": std::process::id()},
            "admission": admission,
            "resources": {
                "memory_pressure": resources.memory_pressure,
                "resident_memory_bytes": resources.resident_memory_bytes,
                "max_memory_bytes": resources.max_memory_bytes,
                "last_sample_ms_ago": resources.last_sample_ms_ago,
                "child_queue": resources.child_queue,
                "probe_queue": resources.probe_queue,
            },
            "guidance": "Normal work remains bounded. No request was launched by this diagnostic; existing blocking workers retain permits until they finish. Workspace details are omitted, not empty."
        })
    }

    async fn wait_admission<T>(
        &self,
        counter: &AtomicUsize,
        stage: &str,
        deadline: tokio::time::Instant,
        wait_for: Duration,
        operation: impl Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        let _waiting = Waiting::new(counter);
        match tokio::time::timeout_at(deadline, operation).await {
            Ok(result) => result.map_err(|error| format!("{stage} admission rejected: {error}")),
            Err(_) => {
                let observed = self.admission_snapshot();
                Err(format!(
                    "tool capacity remained busy for {} ms; request was not started; stage={stage}; slots={}/{}; execution={}/{}; resource_waiters={}",
                    wait_for.as_millis(), observed.slots_in_use, observed.total_limit,
                    observed.execution_in_use, observed.execution_limit, observed.waiting_for_resource,
                ))
            }
        }
    }

    pub(crate) async fn acquire_tool(&self, executes_process: bool) -> Result<ToolPermit, String> {
        self.acquire_tool_with_wait_timeout(executes_process, TOOL_SLOT_WAIT_CAP)
            .await
    }

    pub(crate) async fn acquire_tool_with_wait_timeout(
        &self,
        executes_process: bool,
        wait_for: Duration,
    ) -> Result<ToolPermit, String> {
        self.acquire_after_resources(
            executes_process,
            wait_for,
            crate::resource::global().admit_tool(),
        )
        .await
    }

    // This is also the deterministic test seam for delayed/failed resource
    // admission; production always supplies the real governor above.
    pub(super) async fn acquire_after_resources(
        &self,
        executes_process: bool,
        wait_for: Duration,
        resources: impl Future<Output = Result<(), String>>,
    ) -> Result<ToolPermit, String> {
        let deadline = tokio::time::Instant::now() + wait_for;
        // Resource backpressure holds NO execution or global tool permit.
        self.wait_admission(
            &self.admission_waiters.resource,
            "resource",
            deadline,
            wait_for,
            resources,
        )
        .await?;
        let execution = if executes_process {
            Some(
                self.wait_admission(
                    &self.admission_waiters.execution,
                    "execution",
                    deadline,
                    wait_for,
                    async {
                        self.execution_slots
                            .clone()
                            .acquire_owned()
                            .await
                            .map_err(|_| "execution admission is shutting down".to_owned())
                    },
                )
                .await?,
            )
        } else {
            None
        };
        let slot = self
            .wait_admission(
                &self.admission_waiters.slot,
                "slot",
                deadline,
                wait_for,
                async {
                    self.slots
                        .clone()
                        .acquire_owned()
                        .await
                        .map_err(|_| "tool harness is shutting down".to_owned())
                },
            )
            .await?;
        Ok(ToolPermit {
            _slot: slot,
            _execution: execution,
        })
    }

    // Bare read permits are retained only for legacy test fixtures. Production
    // callers must declare whether their work can start a process.
    #[cfg(test)]
    pub async fn acquire(&self) -> Result<OwnedSemaphorePermit, String> {
        let permit = self.acquire_tool(false).await?;
        Ok(permit._slot)
    }
}
