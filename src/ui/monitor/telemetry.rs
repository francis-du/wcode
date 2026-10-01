use super::*;
use crate::runtime_telemetry::{TaskTelemetry, TaskTelemetryTicket};

impl AuthTelemetry for TaskMonitor {
    fn mark_oauth_client_registered(&self) {
        TaskMonitor::mark_oauth_client_registered(self);
    }

    fn mark_oauth_authorized(&self) {
        TaskMonitor::mark_oauth_authorized(self);
    }
}

impl TaskTelemetryTicket for TaskTicket {
    fn start(&self) {
        TaskTicket::start(self);
    }

    fn finish(self, success: bool, response_bytes: u64) {
        TaskTicket::finish(self, success, response_bytes);
    }
}

impl TaskTelemetry for TaskMonitor {
    type Ticket = TaskTicket;

    fn queue(
        &self,
        workspace: impl Into<String>,
        tool: impl Into<String>,
        detail: impl Into<String>,
        request_bytes: u64,
    ) -> Self::Ticket {
        TaskMonitor::queue(self, workspace, tool, detail, request_bytes)
    }

    fn record_intelligence_result(&self, workspace: &str, tool: &str, value: &Value) {
        TaskMonitor::record_intelligence_result(self, workspace, tool, value);
    }
}

impl RuntimeTelemetry for TaskMonitor {
    fn operator_message(&self, kind: OperatorMessageKind, label: &str, message: impl Into<String>) {
        TaskMonitor::operator_message(self, kind, label, message);
    }

    fn mark_public_url_check(&self, success: bool, error: Option<String>) {
        TaskMonitor::mark_public_url_check(self, success, error);
    }

    fn mark_tunnel_retry(
        &self,
        provider: &str,
        death_count: u32,
        circuit_open: bool,
        retry_after: Duration,
        retain_endpoint: bool,
    ) {
        TaskMonitor::mark_tunnel_retry(
            self,
            provider,
            death_count,
            circuit_open,
            retry_after,
            retain_endpoint,
        );
    }
}
