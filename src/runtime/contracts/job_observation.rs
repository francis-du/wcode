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
    pub(crate) success: Option<bool>,
    pub(crate) error: Option<String>,
}

/// Implementations enforce exact runtime owner and Workspace/job identity.
/// Observation never grants command launch or verification authority.
pub(crate) trait MonitorJobAccess: Send + Sync {
    fn observe(&self, workspace: &str, job_id: &str) -> anyhow::Result<MonitorJobSnapshot>;
    fn cancel(&self, workspace: &str, job_id: &str) -> anyhow::Result<()>;
}
