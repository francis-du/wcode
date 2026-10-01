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
    let scopes = if request.kind == crate::authorization::AuthorizationKind::HumanDecision {
        json!(["deny"])
    } else if request.kind == crate::authorization::AuthorizationKind::DestructiveDelete {
        json!(["exact", "deny"])
    } else {
        json!(["exact", "all_commands", "deny"])
    };
    json!({
        "mode":"form",
        "message":format!("{}\nWorkspace: {}\nRequest: {}", request.summary, request.workspace, request.id),
        "requestedSchema":{
            "type":"object",
            "properties":{
                "scope":{
                    "type":"string",
                    "title":"Authorization scope",
                    "description":"Approve this exact request, authorize all otherwise-allowable commands for this Workspace session, or deny",
                    "enum":scopes
                },
                "approved":{
                    "type":"boolean",
                    "title":"Approve exact request (legacy)",
                    "description":"Backward-compatible exact-request approval"
                }
            }
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
            let scope = response
                .pointer("/content/scope")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    response
                        .pointer("/content/approved")
                        .and_then(Value::as_bool)
                        .map(|approved| if approved { "exact" } else { "deny" }.to_owned())
                })
                .ok_or("accepted authorization response must contain content.scope or legacy content.approved")?;
            if request.kind == crate::authorization::AuthorizationKind::HumanDecision
                && scope != "deny"
            {
                return Err("HumanDecision requires exact approval in the local TUI or protected WebUI; MCP client responses cannot establish human authority".to_owned());
            }
            match scope.as_str() {
                "exact" => {
                    state
                        .workspaces
                        .approve_authorization_session_result(id)
                        .map_err(|error| error.to_string())?;
                    Ok(true)
                }
                "all_commands"
                    if request.kind
                        != crate::authorization::AuthorizationKind::DestructiveDelete =>
                {
                    state
                        .workspaces
                        .set_all_commands_authorized(Some(&request.workspace), true)
                        .map_err(|error| error.to_string())?;
                    Ok(true)
                }
                "deny" => {
                    state.workspaces.deny_authorization(id);
                    Ok(false)
                }
                "all_commands" => {
                    Err("destructive delete cannot use all-commands authorization".to_owned())
                }
                other => Err(format!("unsupported authorization scope: {other}")),
            }
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

pub(super) async fn human_decision_tool(
    state: &AppState,
    args: &Value,
    action: &str,
) -> anyhow::Result<Value> {
    let (workspace_id, workspace) = selected_workspace(state, args).map_err(anyhow::Error::msg)?;
    let plan_id = mcp_tools::required_string(args, "plan_id")
        .map_err(anyhow::Error::msg)?
        .to_owned();
    let label = mcp_tools::required_string(args, "approver")
        .map_err(anyhow::Error::msg)?
        .trim()
        .to_owned();
    let statement = mcp_tools::required_string(args, "statement")
        .map_err(anyhow::Error::msg)?
        .trim()
        .to_owned();
    if args.get("confirmed").and_then(Value::as_bool) != Some(true) {
        anyhow::bail!("confirmed=true acknowledges the requested decision; local operator authorization is still required");
    }
    if label.is_empty()
        || label.len() > 256
        || statement.is_empty()
        || label.chars().any(char::is_control)
        || statement
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        anyhow::bail!("invalid approval display label or statement");
    }
    // Reserve the native summary's bounded space for a server-generated receipt.
    if statement.len() + label.len() > 1_650 {
        anyhow::bail!("approval label and statement exceed 1650 bytes of receipt-bound summary");
    }
    let execution_git_binding = state.harness.execution_git_binding(&workspace).await?;
    let git_guard = execution_git_binding.clone();
    let guard_workspace = workspace.clone();
    let requester = mcp_writer::owner_binding(&mcp_writer::current_owner());
    let harness = state.harness.clone();
    let workspaces = state.workspaces.clone();
    let instance = state.auth.instance_id().to_owned();
    let action = action.to_owned();
    let result = mcp_tools::run_blocking(move || {
        let revision = harness.current_revision(&workspace)?;
        if revision.code.ends_with(":partial")
            || revision.design.as_deref().is_some_and(|value| value.ends_with(":partial")) {
            anyhow::bail!("complete repository revision is required for operator approval");
        }
        let plan = match action.as_str() {
            "verification" => serde_json::to_value(harness.verification_status(&workspace_id, &workspace, &plan_id)?.plan)?,
            "reconciliation" => serde_json::to_value(harness.reconciliation_status(&workspace, &plan_id)?)?,
            _ => anyhow::bail!("unsupported human decision"),
        };
        if action == "verification" && plan.get("require_human_approval").and_then(Value::as_bool) != Some(true) {
            anyhow::bail!("verification plan does not require human approval");
        }
        if plan.get("workspace").and_then(Value::as_str) != Some(workspace_id.as_str()) {
            anyhow::bail!("approval plan does not belong to the selected workspace");
        }
        let plan_revision = if action == "verification" { plan.get("revision") }
            else { plan.pointer("/verification_plan/revision") };
        if plan_revision != Some(&serde_json::to_value(&revision)?) {
            anyhow::bail!("replan_required: operator approval plan revision is stale or missing");
        }
        let policy = if action == "verification" { plan.get("policy") }
            else { plan.pointer("/verification_plan/policy") };
        let digest = format!("sha256:{:x}", Sha256::digest(serde_json::to_vec(&plan)?));
        let binding = json!({
            "domain":"wcode/local-operator-decision/v1", "instance":instance, "requester":requester,
            "root":workspace.root(), "workspace":workspace_id, "action":action,
            "plan_id":plan_id, "plan_digest":digest, "revision":revision,
            "policy":policy, "statement":statement, "display_label":label,
            "execution_git_binding":execution_git_binding,
        });
        let fingerprint = format!("sha256:{:x}", Sha256::digest(serde_json::to_vec(&binding)?));
        let summary = format!(
            "Local operator {action} approval (one use, 2 minute expiry): {plan_id}\nRequester: {requester}\nPlan: {digest}\nCode: {}\nDesign: {}\nPolicy: {}\nDisplay label: {label}\nStatement: {statement}",
            revision.code, revision.design.as_deref().unwrap_or("none"), policy.unwrap_or(&Value::Null)
        );
        // Full access and command session grants do not participate in this decision.
        let receipt = workspaces.require_human_decision(&workspace_id, &summary, &fingerprint)?;
        let approved_statement = format!(
            "Local operator request {} ({fingerprint}); requester={requester}; display label: {label}\n{statement}", receipt.id
        );
        let operator = format!("local_operator:{}", receipt.id);
        // A native stale-revision rejection leaves the one-shot grant consumed.
        let mut value = if action == "verification" {
            serde_json::to_value(harness.verification_approve_authorized_bound(&workspace_id, &workspace, &plan_id, &operator, &approved_statement, execution_git_binding)?)?
        } else {
            harness.reconciliation_approve_authorized(&workspace_id, &workspace, &plan_id, &operator, &approved_statement)?
        };
        value["operator_authorization"] = serde_json::to_value(receipt)?;
        value["display_approver"] = Value::String(label);
        value["requester_binding"] = Value::String(requester);
        Ok(value)
    }).await?;
    state
        .harness
        .ensure_execution_git_binding(&guard_workspace, &git_guard)
        .await?;
    Ok(result)
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
