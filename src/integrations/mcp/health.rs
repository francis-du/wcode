use super::*;

pub(super) fn health_diagnostics_snapshot(state: &AppState, headers: &HeaderMap) -> Value {
    let connection = state.monitor.connection_status();
    let public_url = state
        .auth
        .request_public_url(headers)
        .unwrap_or_else(|| state.auth.public_url());
    json!({
        "ok": true,
        "name": "wcode",
        "instance_id": state.auth.instance_id(),
        "version": env!("CARGO_PKG_VERSION"),
        "workspaces": state.workspaces.capabilities(),
        "max_parallel_tools": state.harness.max_parallel(),
        "resources": crate::resource::snapshot(),
        "harness": state.harness.capabilities(),
        "mcp_url": format!("{public_url}/mcp"),
        "legacy_sse_url": format!("{public_url}/sse"),
        "project_url": PROJECT_URL,
        "author_url": AUTHOR_URL,
        "oauth_client_registered": connection.oauth_client_registered,
        "oauth_authorized": connection.oauth_authorized,
        "mcp_connected": connection.chatgpt_initialized,
        // Backward-compatible alias retained for existing health consumers.
        "chatgpt_initialized": connection.chatgpt_initialized,
        "supported_protocol_versions": SUPPORTED_PROTOCOL_VERSIONS,
        "modern_protocol_version": MODERN_PROTOCOL_VERSION,
        "initialize_count": connection.initialize_count,
        "last_initialize_seconds_ago": connection.last_initialize_seconds_ago,
        "last_mcp_seen_seconds_ago": connection.last_mcp_seen_seconds_ago,
        "public_endpoint": connection.public_endpoint,
        "public_url_healthy": connection.public_url_healthy,
        "public_url_last_checked_seconds_ago": connection.public_url_last_checked_seconds_ago,
        "public_url_consecutive_failures": connection.public_url_consecutive_failures,
        "public_url_error": connection.public_url_error,
        "tunnel_running": connection.tunnel_running,
        "tunnel_error": connection.tunnel_error,
        "tunnels": connection
            .tunnels
            .iter()
            .map(|tunnel| {
                json!({
                    "provider": tunnel.provider,
                    "url": tunnel.url,
                    "mcp_url": tunnel.url.as_ref().map(|url| format!("{url}/mcp")),
                    "role": tunnel.role,
                    "state": tunnel.state,
                    "lease_age_seconds": tunnel.lease_age_seconds,
                    "consecutive_failures": tunnel.consecutive_failures,
                    "death_count": tunnel.death_count,
                    "circuit_open": tunnel.circuit_open,
                    "retry_in_seconds": tunnel.retry_in_seconds,
                    "connected_seconds": tunnel.connected_seconds,
                })
            })
            .collect::<Vec<_>>(),
        "active_tasks": connection.active_tasks,
        "queued_tasks": connection.queued_tasks,
        "peak_active_tasks": connection.peak_active_tasks,
    })
}
