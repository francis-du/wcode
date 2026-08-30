use crate::auth::AuthState;
use crate::harness::ToolHarness;
use crate::mcp::{
    dispatch_mcp_payload, validate_modern_payload, AppState, TaskRuntime, LEGACY_PROTOCOL_VERSIONS,
    MODERN_PROTOCOL_VERSION,
};
use crate::monitor::TaskMonitor;
use crate::workspace::Workspaces;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

const DEFAULT_LEGACY_PROTOCOL: &str = "2025-11-25";
const MAX_STDIO_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

pub(crate) async fn serve(
    workspaces: Workspaces,
    harness: ToolHarness,
    monitor: TaskMonitor,
) -> Result<()> {
    let auth = Arc::new(AuthState::new("http://127.0.0.1".to_owned()));
    let owner = format!(
        "{:x}",
        Sha256::digest(format!("wcode-stdio:{}", auth.instance_id()).as_bytes())
    );
    let state = Arc::new(AppState {
        auth,
        workspaces,
        harness,
        monitor,
        tasks: TaskRuntime::default(),
    });

    let stdin = io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut stdout = io::stdout();
    let mut legacy_protocol = DEFAULT_LEGACY_PROTOCOL.to_owned();

    while let Some(line) = lines
        .next_line()
        .await
        .context("cannot read MCP stdio request")?
    {
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > MAX_STDIO_MESSAGE_BYTES {
            write_response(
                &mut stdout,
                &jsonrpc_error(Value::Null, -32600, "MCP stdio request exceeds 8 MiB bound"),
            )
            .await?;
            continue;
        }
        let message: Value = match serde_json::from_str(&line) {
            Ok(message) => message,
            Err(error) => {
                write_response(
                    &mut stdout,
                    &jsonrpc_error(Value::Null, -32700, format!("Parse error: {error}")),
                )
                .await?;
                continue;
            }
        };
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let protocol = protocol_for_message(&message, &legacy_protocol);
        if protocol == MODERN_PROTOCOL_VERSION {
            if let Err(error) = validate_modern_payload(&message) {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                write_response(&mut stdout, &jsonrpc_error(id, -32602, error)).await?;
                continue;
            }
        } else if !LEGACY_PROTOCOL_VERSIONS.contains(&protocol.as_str()) {
            let id = message.get("id").cloned().unwrap_or(Value::Null);
            write_response(
                &mut stdout,
                &json!({
                    "jsonrpc":"2.0",
                    "id":id,
                    "error":{
                        "code":-32022,
                        "message":"Unsupported protocol version",
                        "data":{"supported":[MODERN_PROTOCOL_VERSION, "2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"], "requested":protocol}
                    }
                }),
            )
            .await?;
            continue;
        } else if method == "initialize" {
            if let Some(requested) = message
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str)
                .filter(|version| LEGACY_PROTOCOL_VERSIONS.contains(version))
            {
                legacy_protocol = requested.to_owned();
            }
        }

        if let Some(response) =
            dispatch_mcp_payload(state.clone(), message, &protocol, &owner).await
        {
            write_response(&mut stdout, &response).await?;
        }
    }
    Ok(())
}

fn protocol_for_message(message: &Value, legacy_protocol: &str) -> String {
    if message.get("method").and_then(Value::as_str) == Some("server/discover") {
        return MODERN_PROTOCOL_VERSION.to_owned();
    }
    if let Some(version) = message
        .get("params")
        .and_then(Value::as_object)
        .and_then(|params| params.get("_meta"))
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("io.modelcontextprotocol/protocolVersion"))
        .and_then(Value::as_str)
    {
        return version.to_owned();
    }
    if message.get("method").and_then(Value::as_str) == Some("initialize") {
        return message
            .pointer("/params/protocolVersion")
            .and_then(Value::as_str)
            .filter(|version| LEGACY_PROTOCOL_VERSIONS.contains(version))
            .unwrap_or(legacy_protocol)
            .to_owned();
    }
    legacy_protocol.to_owned()
}

async fn write_response(stdout: &mut io::Stdout, response: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(response).context("cannot encode MCP stdio response")?;
    stdout
        .write_all(&bytes)
        .await
        .context("cannot write MCP stdio response")?;
    stdout
        .write_all(b"\n")
        .await
        .context("cannot terminate MCP stdio response")?;
    stdout
        .flush()
        .await
        .context("cannot flush MCP stdio response")?;
    Ok(())
}

fn jsonrpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message.into()}})
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/mcp/stdio.rs"]
mod tests;
