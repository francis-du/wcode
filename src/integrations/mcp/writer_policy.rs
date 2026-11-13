use super::*;

pub(super) struct RuntimePolicyContext {
    pub(super) decision: ExecutionPolicyDecision,
    pub(super) writer: WriterLeaseStatus,
    pub(super) plan_id: Option<String>,
    pub(super) workspace: Workspace,
}

pub(super) fn runtime_policy_context(
    state: &AppState,
    tool_name: &str,
    tool_args: &Value,
    selected_args: &Value,
) -> Result<RuntimePolicyContext, String> {
    let (workspace_id, workspace) = super::mcp_tools::selected_workspace(state, selected_args)?;
    let execution = crate::execution::refresh(&state.harness, &workspace_id, &workspace, false)
        .map_err(|error| error.to_string())?;
    let phase = execution
        .get("phase")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let plan_id = execution
        .pointer("/checkpoint/reconciliation_plan_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let plan = match plan_id.as_deref() {
        Some(plan_id) => crate::reconciliation_store::load(&workspace, plan_id)
            .map_err(|error| error.to_string())?,
        None => None,
    };
    let risk = plan
        .as_ref()
        .map(|plan| plan.risk_level)
        .unwrap_or(crate::risk::RiskLevel::Low);
    let writer = runtime().status(&state.workspaces, &workspace)?;
    let product_scopes = crate::scopes::tool_scopes(tool_name)
        .into_iter()
        .map(|scope| scope.as_str().to_owned())
        .collect::<Vec<_>>();
    let existing_floor = plan.as_ref().map(|plan| {
        if plan.verification_plan.deterministic_level == "full" {
            VerificationFloor::Full
        } else {
            VerificationFloor::Quick
        }
    });
    let decision = crate::execution_policy::evaluate(ExecutionPolicyInput {
        effect: tool_effect(tool_name, tool_args),
        phase,
        risk,
        product_scopes,
        writer_lease_active: writer.active || writer.recovery_required,
        reconciliation_bound: plan.is_some(),
        existing_review_required: plan.is_some(),
        existing_verification_floor: existing_floor,
        existing_human_approval_required: plan
            .as_ref()
            .is_some_and(|plan| plan.verification_plan.require_human_approval),
    });
    Ok(RuntimePolicyContext {
        decision,
        writer,
        plan_id,
        workspace,
    })
}

pub(crate) fn policy_status(state: &AppState, args: &Value) -> Result<Value, String> {
    let tool_name = args
        .get("tool_name")
        .and_then(Value::as_str)
        .ok_or_else(|| "execution_policy_status requires tool_name".to_owned())?;
    let tool_args = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
    if !tool_args.is_object() {
        return Err("execution_policy_status arguments must be an object".to_owned());
    }
    let context = runtime_policy_context(state, tool_name, &tool_args, args)?;
    Ok(json!({
        "policy": context.decision,
        "writer": context.writer,
        "sandbox": crate::workspace::execution_sandbox_status(),
        "reconciliation_plan_id": context.plan_id,
        "source": "runtime_state"
    }))
}

pub(super) fn enforce_plan_approval(context: &RuntimePolicyContext) -> Result<(), String> {
    if !context.decision.require_plan_approval {
        return Ok(());
    }
    let plan_id = context
        .plan_id
        .as_deref()
        .ok_or_else(|| "plan_approval_required: no bound reconciliation plan".to_owned())?;
    let live_plan = crate::reconciliation_store::load(&context.workspace, plan_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("reconciliation plan does not exist: {plan_id}"))?;
    crate::reconciliation_approval::require(&context.workspace, plan_id)
        .map_err(|error| error.to_string())?;
    let current_revision = crate::intelligence::SoftwareIntelligenceRuntime::default()
        .current_revision(&context.workspace)
        .map_err(|error| error.to_string())?;
    let execution = crate::reconciliation_execution_store::load(&context.workspace, plan_id)
        .map_err(|error| error.to_string())?;
    let status = crate::reconciliation_approval::status(
        &context.workspace,
        &live_plan,
        &current_revision,
        execution.as_ref(),
    )
    .map_err(|error| error.to_string())?;
    if status["replan_required"].as_bool() == Some(true) {
        return Err(format!(
            "replan_required: approved plan no longer matches runtime premises: {}",
            status["reasons"]
        ));
    }
    Ok(())
}
