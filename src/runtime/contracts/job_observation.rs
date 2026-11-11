#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MonitorJobOrigin {
    Ui,
    Mcp,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct MonitorJobStream {
    pub(crate) text: String,
    pub(crate) total_bytes: u64,
    pub(crate) truncated: bool,
    pub(crate) redacted: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct MonitorJobSnapshot {
    pub(crate) job_id: String,
    pub(crate) workspace: String,
    pub(crate) status: String,
    pub(crate) origin: MonitorJobOrigin,
    pub(crate) can_cancel: bool,
    pub(crate) stdout: MonitorJobStream,
    pub(crate) stderr: MonitorJobStream,
    pub(crate) exit_code: Option<i32>,
    /// The process was terminated by a signal, so `exit_code` is absent.
    pub(crate) signaled: Option<bool>,
    pub(crate) success: Option<bool>,
    pub(crate) error: Option<String>,
    /// Deterministic runner metadata retained with the completed command
    /// result. Live output has none, so these stay unknown until completion.
    pub(crate) queue_wait_ms: Option<u64>,
    pub(crate) timed_out: Option<bool>,
    pub(crate) output_incomplete: Option<bool>,
    /// The lane the completed command actually ran on (`direct`, `sandbox`, or
    /// `sandbox_unavailable` when a requested sandbox had no usable backend).
    /// The executable, argument vector, cwd and environment stay out of the
    /// observation payload by contract, so they remain unknown rather than
    /// being inferred from the workspace root.
    pub(crate) sandbox: Option<String>,
}

/// Implementations enforce exact runtime owner and Workspace/job identity.
/// Observation never grants command launch or verification authority.
pub(crate) trait MonitorJobAccess: Send + Sync {
    fn observe(&self, workspace: &str, job_id: &str) -> anyhow::Result<MonitorJobSnapshot>;
    fn cancel(&self, workspace: &str, job_id: &str) -> anyhow::Result<()>;
}
