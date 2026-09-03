use crate::auth::AuthState;
use crate::harness::ToolHarness;
use crate::mcp::{
    apply_authorization_response, authorization_elicitation_params,
    authorization_request_from_tool_result, authorization_request_state, dispatch_mcp_payload,
    supports_form_elicitation, validate_modern_payload, AppState, TaskRuntime,
    LEGACY_PROTOCOL_VERSIONS, MODERN_PROTOCOL_VERSION,
};
use crate::monitor::TaskMonitor;
use crate::workspace::Workspaces;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::io::{self, AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};

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
    let mut legacy_elicitation = false;

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
            legacy_elicitation = legacy_client_supports_elicitation(&message);
        }

        let original = message.clone();
        let response = dispatch_mcp_payload(state.clone(), message, &protocol, &owner).await;
        let response = if protocol != MODERN_PROTOCOL_VERSION {
            drive_legacy_authorization(
                state.clone(),
                &mut lines,
                &mut stdout,
                LegacyAuthorizationTurn {
                    original,
                    protocol: protocol.clone(),
                    owner: owner.clone(),
                    response,
                    elicitation_supported: legacy_elicitation,
                },
            )
            .await?
        } else {
            response
        };
        if let Some(response) = response {
            write_response(&mut stdout, &response).await?;
        }
    }
    Ok(())
}

fn legacy_client_supports_elicitation(message: &Value) -> bool {
    message
        .pointer("/params/capabilities/elicitation")
        .is_some_and(supports_form_elicitation)
}

struct LegacyAuthorizationTurn {
    original: Value,
    protocol: String,
    owner: String,
    response: Option<Value>,
    elicitation_supported: bool,
}

async fn drive_legacy_authorization<R, W>(
    state: Arc<AppState>,
    lines: &mut tokio::io::Lines<R>,
    stdout: &mut W,
    mut turn: LegacyAuthorizationTurn,
) -> Result<Option<Value>>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWriteExt + Unpin,
{
    const MAX_AUTHORIZATION_ROUNDS: usize = 4;
    for _ in 0..MAX_AUTHORIZATION_ROUNDS {
        let Some(current) = turn.response.as_ref() else {
            return Ok(None);
        };
        let Some(request) = current
            .get("result")
            .and_then(|result| authorization_request_from_tool_result(&state, result))
        else {
            return Ok(turn.response);
        };
        if !turn.elicitation_supported {
            return Ok(turn.response);
        }

        let request_state = authorization_request_state(&state, &request, &turn.owner)
            .map_err(anyhow::Error::msg)?;
        let elicitation = json!({
            "jsonrpc":"2.0",
            "id":request_state,
            "method":"elicitation/create",
            "params":authorization_elicitation_params(&request),
        });
        write_response(stdout, &elicitation).await?;
        let answer = read_legacy_elicitation_response(lines, &request_state).await?;
        let accepted = apply_authorization_response(&state, &turn.owner, &request_state, &answer)
            .map_err(anyhow::Error::msg)?;
        if !accepted {
            let id = turn.original.get("id").cloned().unwrap_or(Value::Null);
            return Ok(Some(json!({
                "jsonrpc":"2.0",
                "id":id,
                "result":{
                    "content":[{"type":"text","text":"{\"error\":\"authorization denied by user\"}"}],
                    "structuredContent":{"error":"authorization denied by user"},
                    "isError":true
                }
            })));
        }
        turn.response = dispatch_mcp_payload(
            state.clone(),
            turn.original.clone(),
            &turn.protocol,
            &turn.owner,
        )
        .await;
    }
    let id = turn.original.get("id").cloned().unwrap_or(Value::Null);
    Ok(Some(jsonrpc_error(
        id,
        -32603,
        "authorization interaction exceeded bounded round limit",
    )))
}

async fn read_legacy_elicitation_response<R: AsyncBufRead + Unpin>(
    lines: &mut tokio::io::Lines<R>,
    expected_id: &str,
) -> Result<Value> {
    loop {
        let line = lines
            .next_line()
            .await
            .context("cannot read MCP stdio elicitation response")?
            .ok_or_else(|| anyhow::anyhow!("MCP stdio closed during authorization elicitation"))?;
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > MAX_STDIO_MESSAGE_BYTES {
            anyhow::bail!("MCP stdio elicitation response exceeds 8 MiB bound");
        }
        let message: Value =
            serde_json::from_str(&line).context("cannot parse MCP stdio elicitation response")?;
        if message.get("id").and_then(Value::as_str) != Some(expected_id) {
            anyhow::bail!("unexpected MCP stdio message while waiting for authorization response");
        }
        if let Some(error) = message.get("error") {
            anyhow::bail!("client rejected authorization elicitation: {error}");
        }
        return message.get("result").cloned().ok_or_else(|| {
            anyhow::anyhow!("authorization elicitation response is missing result")
        });
    }
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

async fn write_response<W: AsyncWriteExt + Unpin>(stdout: &mut W, response: &Value) -> Result<()> {
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
