use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OperatorMessageKind {
    Info,
    Success,
    Warning,
}

/// Neutral runtime observation port.
///
/// Implementations may render, persist or ignore these signals, but callers
/// cannot obtain authorization, verification authority or task ownership from
/// this interface.
pub(crate) trait AuthTelemetry: Send + Sync {
    fn mark_oauth_client_registered(&self);
    fn mark_oauth_authorized(&self);
}

pub(crate) trait TaskTelemetryTicket: Send + 'static {
    fn start(&self);
    fn finish(self, success: bool, response_bytes: u64);
}

pub(crate) trait TaskTelemetry: Clone + Send + Sync + 'static {
    type Ticket: TaskTelemetryTicket;

    fn queue(
        &self,
        workspace: impl Into<String>,
        tool: impl Into<String>,
        detail: impl Into<String>,
        request_bytes: u64,
    ) -> Self::Ticket;

    fn record_intelligence_result(&self, workspace: &str, tool: &str, value: &Value);
}

pub(crate) trait RuntimeTelemetry: Clone + Send + Sync + 'static {
    fn operator_message(&self, kind: OperatorMessageKind, label: &str, message: impl Into<String>);

    fn mark_public_url_check(&self, success: bool, error: Option<String>);
    fn mark_tunnel_retry(
        &self,
        provider: &str,
        death_count: u32,
        circuit_open: bool,
        retry_after: Duration,
        retain_endpoint: bool,
    );
}
