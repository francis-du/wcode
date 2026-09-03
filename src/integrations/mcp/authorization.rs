use super::*;
use crate::authorization::{AuthorizationRequest, AuthorizationStatus};
use sha2::{Digest, Sha256};

pub(crate) const AUTHORIZATION_INPUT_KEY: &str = "authorization";
const AUTHORIZATION_STATE_PREFIX: &str = "wcode-authorization:";

pub(crate) fn supports_form_elicitation(value: &Value) -> bool {
    let Some(capability) = value.as_object() else {
        return false;
    };
    capability.is_empty() || capability.get("form").is_some_and(Value::is_object)
}

pub(super) fn client_supports_elicitation(message: &Value) -> bool {
    message
        .pointer("/params/_meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("io.modelcontextprotocol/clientCapabilities"))
        .and_then(Value::as_object)
        .and_then(|capabilities| capabilities.get("elicitation"))
        .is_some_and(supports_form_elicitation)
}

pub(crate) fn authorization_request_from_tool_result(
    state: &AppState,
    value: &Value,
) -> Option<AuthorizationRequest> {
    let id = value
        .pointer("/structuredContent/authorization_required/id")
        .and_then(Value::as_str)?;
    state
        .workspaces
        .authorization_request(id)
        .filter(|request| request.status == AuthorizationStatus::Pending)
}

pub(crate) fn authorization_elicitation_params(request: &AuthorizationRequest) -> Value {
    json!({
        "mode":"form",
        "message":format!("{}\nWorkspace: {}\nRequest: {}", request.summary, request.workspace, request.id),
        "requestedSchema":{
            "type":"object",
            "properties":{
                "approved":{
                    "type":"boolean",
                    "title":"Approve",
                    "description":"Approve this exact wcode authorization request"
                }
            },
            "required":["approved"]
        }
    })
}

fn authorization_owner_binding(owner: &str) -> String {
    format!("{:x}", Sha256::digest(owner.as_bytes()))
}

pub(crate) fn authorization_request_state(
    state: &AppState,
    request: &AuthorizationRequest,
    owner: &str,
) -> Result<String, String> {
    let token = state
        .workspaces
        .authorization_interactive_token(&request.id)
        .ok_or_else(|| {
            format!(
                "authorization request {} has no active challenge",
                request.id
            )
        })?;
    Ok(format!(
        "{AUTHORIZATION_STATE_PREFIX}{}:{}:{}",
        request.id,
        token,
        authorization_owner_binding(owner)
    ))
}

pub(super) fn authorization_input_required(
    state: &AppState,
    request: &AuthorizationRequest,
    owner: &str,
) -> Result<Value, String> {
    Ok(modern_result(json!({
        "resultType":"input_required",
        "inputRequests":{
            (AUTHORIZATION_INPUT_KEY):{
                "method":"elicitation/create",
                "params":authorization_elicitation_params(request)
            }
        },
        "requestState":authorization_request_state(state, request, owner)?
    })))
}

pub(crate) fn apply_authorization_response(
    state: &AppState,
    owner: &str,
    request_state: &str,
    response: &Value,
) -> Result<bool, String> {
    let encoded = request_state
        .strip_prefix(AUTHORIZATION_STATE_PREFIX)
        .ok_or("authorization requestState is invalid")?;
    let mut parts = encoded.splitn(3, ':');
    let id = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or("authorization requestState is malformed")?;
    let token = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or("authorization requestState is malformed")?;
    let owner_binding = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or("authorization requestState is malformed")?;
    if owner_binding != authorization_owner_binding(owner) {
        return Err("authorization requestState is bound to a different MCP client".to_owned());
    }
    let request = state
        .workspaces
        .authorization_request(id)
        .ok_or_else(|| format!("authorization request does not exist: {id}"))?;
    if request.status != AuthorizationStatus::Pending {
        return Err(format!("authorization request {id} is no longer pending"));
    }
    if state
        .workspaces
        .authorization_interactive_token(id)
        .as_deref()
        != Some(token)
    {
        return Err("authorization challenge does not match the pending request".to_owned());
    }
    match response.get("action").and_then(Value::as_str) {
        Some("accept") => {
            let approved = response
                .pointer("/content/approved")
                .and_then(Value::as_bool)
                .ok_or("accepted authorization response must contain boolean content.approved")?;
            if !approved {
                state.workspaces.deny_authorization(id);
                return Ok(false);
            }
            state
                .workspaces
                .approve_authorization_session_result(id)
                .map_err(|error| error.to_string())?;
            Ok(true)
        }
        Some("decline" | "cancel") => {
            state.workspaces.deny_authorization(id);
            Ok(false)
        }
        Some(action) => Err(format!(
            "unsupported authorization elicitation action: {action}"
        )),
        None => Err("authorization elicitation response is missing action".to_owned()),
    }
}

pub(super) fn apply_authorization_retry(
    state: &AppState,
    owner: &str,
    message: &Value,
) -> Result<Option<Value>, String> {
    let Some(request_state) = message
        .pointer("/params/requestState")
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };
    if !request_state.starts_with(AUTHORIZATION_STATE_PREFIX) {
        return Ok(None);
    }
    let response = message
        .pointer("/params/inputResponses")
        .and_then(Value::as_object)
        .and_then(|responses| responses.get(AUTHORIZATION_INPUT_KEY))
        .ok_or("authorization MRTR retry is missing inputResponses.authorization")?;
    if apply_authorization_response(state, owner, request_state, response)? {
        Ok(None)
    } else {
        Ok(Some(mcp_tools::tool_result(
            json!({"error":"authorization denied by user"}),
            true,
        )))
    }
}
