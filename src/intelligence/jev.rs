#[cfg(not(test))]
use crate::decision::{
    agent_context_decision_request, agent_context_decisions, compare_decision_batches,
};
use crate::decision::{
    choice_signal_is_concentrated, DecisionBatch, DecisionMode, DecisionPolicy, DecisionPrimitive,
    DecisionRequest, DecisionSignal, DecisionValue, DECISION_SCHEMA_VERSION,
};
#[cfg(not(test))]
use reqwest::Client;
use reqwest::Url;
use serde_json::{json, Value};
use std::net::IpAddr;
#[cfg(not(test))]
use std::{
    env, fs,
    path::PathBuf,
    time::{Duration, Instant},
};

#[cfg(not(test))]
pub(crate) const JEV_API_KEY_ENV: &str = "JEV_API_KEY";
#[cfg(not(test))]
pub(crate) const JEV_BASE_URL_ENV: &str = "JEV_BASE_URL";
#[cfg(not(test))]
pub(crate) const JEV_DEFAULT_MODEL_ENV: &str = "JEV_DEFAULT_MODEL";
pub(crate) const JEV_DEFAULT_BASE_URL: &str = "https://api.typesafe.ai";
pub(crate) const JEV_DEFAULT_MODEL: &str = "jev-latest";
const AGENT_CONTEXT_QUESTION_SET_ID: &str = "wcode.agent_context";
const AGENT_CONTEXT_QUESTION_SET_VERSION: u16 = 5;
#[path = "jev_policy.rs"]
mod policy;
pub(crate) use policy::agent_context_questions;
use policy::{recommendation, score_max_index, signal_mode};
#[path = "jev_checkpoint.rs"]
mod checkpoint;
pub(crate) use checkpoint::{checkpoint_actions, evaluate_checkpoint};
#[path = "jev_metrics.rs"]
mod metrics;
#[cfg(not(test))]
use metrics::{provider_usage, JevCallMetrics, JevEvaluation};
#[cfg(not(test))]
const MAX_RESPONSE_BYTES: u64 = 512 * 1024;
#[cfg(not(test))]
const MAX_SHELL_PROFILE_BYTES: u64 = 512 * 1024;

#[derive(Clone)]
pub(crate) struct JevConfig {
    api_key: String,
    base_url: String,
    model: String,
    timeout_secs: u64,
}

impl JevConfig {
    #[cfg(not(test))]
    pub(crate) fn from_env() -> Result<Option<Self>, String> {
        let Some(api_key) = environment_value(JEV_API_KEY_ENV) else {
            return Ok(None);
        };
        let base_url = environment_value(JEV_BASE_URL_ENV)
            .map(|value| value.trim_end_matches('/').to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| JEV_DEFAULT_BASE_URL.to_owned());
        let model = environment_value(JEV_DEFAULT_MODEL_ENV)
            .unwrap_or_else(|| JEV_DEFAULT_MODEL.to_owned());
        Self::new(api_key, base_url, model, 10).map(Some)
    }

    fn new(
        api_key: String,
        base_url: String,
        model: String,
        timeout_secs: u64,
    ) -> Result<Self, String> {
        let api_key = api_key.trim().to_owned();
        let base_url = base_url.trim().trim_end_matches('/').to_owned();
        let model = model.trim().to_owned();
        if api_key.is_empty() || api_key.len() > 16_384 || api_key.chars().any(char::is_control) {
            return Err("Jev API key is invalid".into());
        }
        if model.is_empty() || model.len() > 256 || model.chars().any(char::is_control) {
            return Err("Jev model is invalid".into());
        }
        if !(1..=120).contains(&timeout_secs) {
            return Err("Jev timeout is invalid".into());
        }
        validate_base_url(&base_url)?;
        Ok(Self {
            api_key,
            base_url,
            model,
            timeout_secs,
        })
    }
}

#[cfg(not(test))]
fn environment_value(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .or_else(|| shell_profile_value(key))
}

#[cfg(not(test))]
fn shell_profile_value(key: &str) -> Option<String> {
    let home = env::var_os("HOME").map(PathBuf::from)?;
    let mut resolved = None;
    for name in [".profile", ".zshenv", ".zprofile", ".zshrc"] {
        let path = home.join(name);
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > MAX_SHELL_PROFILE_BYTES {
            continue;
        }
        let Ok(contents) = fs::read_to_string(path) else {
            continue;
        };
        if let Some(value) = parse_shell_profile_value(&contents, key) {
            resolved = Some(value);
        }
    }
    resolved
}

fn parse_shell_profile_value(contents: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
            let raw = line.strip_prefix(&prefix)?.trim();
            shell_literal(raw)
        })
        .next_back()
}

fn shell_literal(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.len() >= 2 {
        if let Some(value) = raw
            .strip_prefix('\'')
            .and_then(|value| value.strip_suffix('\''))
        {
            return (!value.is_empty()).then(|| value.to_owned());
        }
        if let Some(value) = raw
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
        {
            if value.contains('$') || value.contains('`') {
                return None;
            }
            return (!value.is_empty()).then(|| value.to_owned());
        }
    }
    let value = raw.split_whitespace().next()?;
    if value.is_empty() || value.contains('$') || value.contains('`') {
        return None;
    }
    Some(value.to_owned())
}

#[cfg(not(test))]
pub(crate) struct JevDecisionProvider {
    config: JevConfig,
    client: Client,
}

#[cfg(not(test))]
impl JevDecisionProvider {
    fn model(&self) -> &str {
        &self.config.model
    }

    pub(crate) fn from_env() -> Result<Option<Self>, String> {
        JevConfig::from_env()?.map(Self::new).transpose()
    }

    fn new(config: JevConfig) -> Result<Self, String> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(config.timeout_secs.min(5)))
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|error| format!("Jev HTTP client unavailable: {error}"))?;
        Ok(Self { config, client })
    }

    pub(crate) async fn evaluate(
        &self,
        request: &DecisionRequest,
    ) -> Result<JevEvaluation, String> {
        self.evaluate_with_questions(
            request,
            AGENT_CONTEXT_QUESTION_SET_ID,
            AGENT_CONTEXT_QUESTION_SET_VERSION,
            agent_context_questions(),
        )
        .await
    }

    pub(super) async fn evaluate_with_questions(
        &self,
        request: &DecisionRequest,
        question_set_id: &str,
        question_set_version: u16,
        questions: Value,
    ) -> Result<JevEvaluation, String> {
        let mut state = request.state.clone();
        if let Some(object) = state.as_object_mut() {
            object.insert(
                "_jev_question_set".into(),
                json!({
                    "id": question_set_id,
                    "version": question_set_version
                }),
            );
        }
        let payload = json!({
            "model": self.config.model,
            "state": state,
            "questions": questions,
        });
        let payload = serde_json::to_vec(&payload)
            .map_err(|error| format!("Jev request serialization failed: {error}"))?;
        let request_bytes = u64::try_from(payload.len()).unwrap_or(u64::MAX);
        let started = Instant::now();
        let response = self
            .client
            .post(endpoint_url(&self.config.base_url)?)
            .bearer_auth(&self.config.api_key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(payload)
            .send()
            .await
            .map_err(|error| format!("Jev request failed: {error}"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("Jev returned HTTP {}", status.as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES)
        {
            return Err("Jev response exceeded the size budget".into());
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| format!("Jev response read failed: {error}"))?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_RESPONSE_BYTES {
            return Err("Jev response exceeded the size budget".into());
        }
        let response_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Jev returned invalid JSON: {error}"))?;
        let usage = provider_usage(&value);
        let mut batch = response_to_batch(request, &value)?;
        for signal in &mut batch.signals {
            signal.evidence.push(format!("model:{}", self.config.model));
            signal.evidence.push(format!(
                "question_set:{}@{}",
                question_set_id, question_set_version
            ));
        }
        Ok(JevEvaluation {
            batch,
            metrics: JevCallMetrics {
                request_bytes,
                response_bytes,
                elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                total_tokens: usage.total_tokens,
            },
        })
    }
}

pub(crate) async fn augment_agent_context(
    context: &mut Value,
    query: &str,
) -> Result<Value, String> {
    #[cfg(test)]
    {
        let _ = (context, query);
        Ok(Value::Null)
    }

    #[cfg(not(test))]
    {
        let baseline = context
            .get("decision_plane")
            .cloned()
            .and_then(|value| serde_json::from_value::<DecisionBatch>(value).ok())
            .unwrap_or_else(|| agent_context_decisions(context, query));
        let baseline_next_action = choice(&baseline, "next_action").unwrap_or("unknown");
        let provider = match JevDecisionProvider::from_env() {
            Ok(Some(provider)) => provider,
            Ok(None) => {
                let telemetry = json!({
                    "provider": "jev",
                    "status": "disabled",
                    "reason": "not_configured",
                    "authority": "increase_only_assist",
                    "baseline_next_action": baseline_next_action,
                    "question_set": {
                        "id": AGENT_CONTEXT_QUESTION_SET_ID,
                        "version": AGENT_CONTEXT_QUESTION_SET_VERSION
                    },
                    "fallback": "deterministic"
                });
                attach_jev_if_budget_allows(context, telemetry.clone());
                return Ok(telemetry);
            }
            Err(error) => {
                tracing::warn!(error = %error, "Jev configuration rejected; deterministic decision plane retained");
                let telemetry = json!({
                    "provider": "jev",
                    "status": "invalid_configuration",
                    "authority": "increase_only_assist",
                    "baseline_next_action": baseline_next_action,
                    "question_set": {
                        "id": AGENT_CONTEXT_QUESTION_SET_ID,
                        "version": AGENT_CONTEXT_QUESTION_SET_VERSION
                    },
                    "fallback": "deterministic"
                });
                attach_jev_if_budget_allows(context, telemetry.clone());
                return Ok(telemetry);
            }
        };
        let request = agent_context_decision_request(context, query);
        match provider.evaluate(&request).await {
            Ok(evaluation) => {
                let candidate = evaluation.batch;
                let comparison = compare_decision_batches(&baseline, &candidate);
                let guidance = advisory_guidance(&baseline, &candidate);
                let mut telemetry = json!({
                    "provider": "jev",
                    "model": provider.model(),
                    "status": "active",
                    "authority": "increase_only_assist",
                    "baseline_next_action": baseline_next_action,
                    "candidate_next_action": choice(&candidate, "next_action"),
                    "candidate_signal_count": candidate.signals.len(),
                    "question_set": {
                        "id": AGENT_CONTEXT_QUESTION_SET_ID,
                        "version": AGENT_CONTEXT_QUESTION_SET_VERSION
                    },
                    "comparison": {
                        "shared_signals": comparison.shared_signals,
                        "missing_from_baseline": comparison.missing_from_baseline,
                        "missing_from_candidate": comparison.missing_from_candidate,
                        "probability_pairs": comparison.probability_pairs,
                        "score_pairs": comparison.score_pairs,
                        "choice_pairs": comparison.choice_pairs,
                        "choice_disagreements": comparison.choice_disagreement_count,
                        "safety_policy_violations": comparison.safety_policy_violation_count,
                        "shape_mismatches": comparison.primitive_mismatch_count
                            + comparison.mode_mismatch_count
                            + comparison.baseline_duplicate_signal_ids
                            + comparison.candidate_duplicate_signal_ids
                            + usize::from(comparison.scope_mismatch)
                            + usize::from(comparison.schema_mismatch)
                    },
                    "guidance": guidance,
                    "call": evaluation.metrics.as_json(),
                    "fallback": "deterministic"
                });
                let routing = apply_agent_context_guidance(context, &telemetry);
                telemetry["routing"] = routing;
                attach_jev_if_budget_allows(context, telemetry.clone());
                Ok(telemetry)
            }
            Err(error) => {
                tracing::warn!(error = %error, "Jev request failed; deterministic decision plane retained");
                let telemetry = json!({
                    "provider": "jev",
                    "model": provider.model(),
                    "status": "unavailable",
                    "authority": "increase_only_assist",
                    "baseline_next_action": baseline_next_action,
                    "question_set": {
                        "id": AGENT_CONTEXT_QUESTION_SET_ID,
                        "version": AGENT_CONTEXT_QUESTION_SET_VERSION
                    },
                    "fallback": "deterministic"
                });
                attach_jev_if_budget_allows(context, telemetry.clone());
                Ok(telemetry)
            }
        }
    }
}

fn apply_agent_context_guidance(context: &mut Value, telemetry: &Value) -> Value {
    if telemetry.get("status").and_then(Value::as_str) != Some("active") {
        return json!({"applied":false,"actions":[]});
    }
    let guidance = telemetry
        .get("guidance")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let candidate_next_action = telemetry
        .get("candidate_next_action")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let target_count = context
        .get("targets")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let hot_source_count = context
        .get("hot_source")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let retrieval_action = if target_count == 0 {
        "find_symbol"
    } else if hot_source_count < target_count {
        "symbol_context"
    } else {
        "software_context"
    };

    let needs_semantic = guidance.iter().any(|item| {
        item.as_str() == Some("jev:prefer_semantic_navigation")
            || item.as_str() == Some("jev:next_action:semantic_navigation")
    }) || candidate_next_action == "semantic_navigation";
    let needs_review = guidance
        .iter()
        .any(|item| item.as_str() == Some("jev:next_action:review_worktree"))
        || candidate_next_action == "review_worktree";
    let needs_retrieval = guidance.iter().any(|item| {
        matches!(
            item.as_str(),
            Some("jev:retrieve_more_evidence")
                | Some("jev:choice_uncertain_collect_evidence")
                | Some("jev:next_action:retrieve")
        )
    }) || candidate_next_action == "retrieve";
    let verification_escalation = guidance
        .iter()
        .any(|item| item.as_str() == Some("jev:preserve_or_raise_verification"));
    let capability_group = guidance.iter().find_map(|item| {
        item.as_str()
            .and_then(|value| value.strip_prefix("jev:capability_group:"))
            .filter(|group| !crate::harness::model_tools_for_group(group).is_empty())
    });
    let original_capabilities = context.get("capabilities").cloned();
    let lsp_install_required = context
        .pointer("/readiness/advisories")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.as_str() == Some("lsp_install_required"))
        });

    let Some(readiness) = context.get_mut("readiness").and_then(Value::as_object_mut) else {
        return json!({"applied":false,"actions":[]});
    };
    let original = readiness.clone();
    let actions = readiness
        .entry("next_actions")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut();
    let Some(actions) = actions else {
        return json!({"applied":false,"actions":[]});
    };

    let mut applied = Vec::<String>::new();
    if needs_retrieval {
        promote_before_edit(actions, retrieval_action);
        applied.push(retrieval_action.to_owned());
    }
    if needs_review {
        promote_before_edit(actions, "review_changes");
        applied.push("review_changes".to_owned());
    }
    if needs_semantic {
        if lsp_install_required {
            promote_before_edit(actions, "semantic_provider_install");
            promote_before_edit(actions, "semantic_provider_refresh");
            applied.push("semantic_provider_install".to_owned());
            applied.push("semantic_provider_refresh".to_owned());
        }
        promote_before_edit(actions, "semantic_navigation");
        applied.push("semantic_navigation".to_owned());
    }
    if verification_escalation {
        promote_before_action(actions, "verification_plan", "verify_project");
        applied.push("verification_plan".to_owned());
    }

    let advisories = readiness
        .entry("advisories")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut();
    if let Some(advisories) = advisories {
        for advisory in [
            needs_retrieval.then_some("jev_retrieval_review"),
            needs_review.then_some("jev_worktree_review"),
            needs_semantic.then_some("jev_semantic_navigation"),
            verification_escalation.then_some("jev_verification_escalation"),
        ]
        .into_iter()
        .flatten()
        {
            if !advisories
                .iter()
                .any(|item| item.as_str() == Some(advisory))
            {
                advisories.push(Value::String(advisory.to_owned()));
            }
        }
    }
    let mut promoted_tools = applied.clone();
    if let Some(group) = capability_group {
        for tool in crate::harness::model_tools_for_group(group) {
            if !promoted_tools.iter().any(|candidate| candidate == tool) {
                promoted_tools.push((*tool).to_owned());
            }
        }
    }
    if !applied.is_empty() || capability_group.is_some() {
        readiness.insert(
            "decision_assist".to_owned(),
            json!({
                "provider":"jev",
                "authority":"increase_only",
                "candidate_next_action": candidate_next_action,
                "capability_group": capability_group,
                "applied_actions": applied,
                "promoted_tools": promoted_tools,
            }),
        );
    }
    if let Some(capabilities) = context.get_mut("capabilities") {
        crate::harness::promote_model_tools(capabilities, &promoted_tools);
    }

    if !context_fits_budget(context) {
        if let Some(readiness) = context.get_mut("readiness").and_then(Value::as_object_mut) {
            *readiness = original;
        }
        if let Some(original_capabilities) = original_capabilities {
            context["capabilities"] = original_capabilities;
        }
        return json!({
            "applied":false,
            "actions":[],
            "reason":"context_budget"
        });
    }

    json!({
        "applied":!applied.is_empty() || capability_group.is_some(),
        "actions":applied,
        "capability_group":capability_group,
        "promoted_tools":promoted_tools,
        "verification_escalation":verification_escalation
    })
}

fn promote_before_action(actions: &mut Vec<Value>, action: &str, before: &str) {
    let existing = actions
        .iter()
        .position(|item| item.as_str() == Some(action));
    let mut before_index = actions
        .iter()
        .position(|item| item.as_str() == Some(before))
        .unwrap_or(actions.len());
    if let Some(existing) = existing {
        if existing < before_index {
            return;
        }
        let value = actions.remove(existing);
        before_index = actions
            .iter()
            .position(|item| item.as_str() == Some(before))
            .unwrap_or(actions.len());
        actions.insert(before_index, value);
        return;
    }
    actions.insert(before_index, Value::String(action.to_owned()));
}

fn promote_before_edit(actions: &mut Vec<Value>, action: &str) {
    let edit_action = |value: &Value| {
        matches!(
            value.as_str(),
            Some("apply_edits")
                | Some("apply_file_edits")
                | Some("write_file")
                | Some("create_file")
                | Some("create_files")
        )
    };
    let existing = actions
        .iter()
        .position(|item| item.as_str() == Some(action));
    let mut edit_index = actions
        .iter()
        .position(edit_action)
        .unwrap_or(actions.len());
    if let Some(existing) = existing {
        if existing < edit_index {
            return;
        }
        let value = actions.remove(existing);
        edit_index = actions
            .iter()
            .position(edit_action)
            .unwrap_or(actions.len());
        actions.insert(edit_index, value);
        return;
    }
    actions.insert(edit_index, Value::String(action.to_owned()));
}

fn attach_jev_if_budget_allows(context: &mut Value, jev: Value) {
    let previous = context.get("jev").cloned();
    context["jev"] = jev;
    if !context_fits_budget(context) {
        if let Some(previous) = previous {
            context["jev"] = previous;
        } else if let Some(object) = context.as_object_mut() {
            object.remove("jev");
        }
    }
}

fn context_fits_budget(context: &Value) -> bool {
    let budget = context.get("budget").and_then(Value::as_u64).unwrap_or(0);
    if budget == 0 {
        return true;
    }
    serde_json::to_vec(context)
        .map(|bytes| (bytes.len() as u64).div_ceil(4) <= budget)
        .unwrap_or(false)
}

pub(crate) fn response_to_batch(
    request: &DecisionRequest,
    value: &Value,
) -> Result<DecisionBatch, String> {
    let answers = value
        .get("answers")
        .and_then(Value::as_object)
        .ok_or_else(|| "Jev response is missing answers".to_owned())?;
    let mut signals = Vec::with_capacity(answers.len());
    for (id, answer) in answers {
        signals.push(answer_to_signal(id, answer)?);
    }
    if signals.is_empty() {
        return Err("Jev response contains no answers".into());
    }
    Ok(DecisionBatch {
        schema_version: DECISION_SCHEMA_VERSION.into(),
        provider: "jev-v1".into(),
        scope: request.scope.clone(),
        policy: DecisionPolicy {
            authority: "advisory_only".into(),
            can_increase_work: true,
            can_reduce_safety: false,
            deterministic_verification_floor: true,
        },
        signals,
    })
}

fn answer_to_signal(id: &str, answer: &Value) -> Result<DecisionSignal, String> {
    let answer_type = answer
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Jev answer {id} is missing type"))?;
    let probabilities_milli = answer_probability_distribution(answer, id)?;
    let (primitive, value, confidence_milli, native_confidence_milli) = match answer_type {
        "noul" => {
            let probability_milli =
                probability_to_milli(bounded_probability(answer.get("noul"), id)?);
            (
                DecisionPrimitive::Probability,
                DecisionValue::Probability { probability_milli },
                0,
                None,
            )
        }
        "choice" => {
            let selected = answer
                .get("choice")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty() && value.len() <= 256)
                .ok_or_else(|| format!("Jev choice answer {id} is invalid"))?
                .to_owned();
            let confidence = bounded_probability(answer.get("confidence"), id)?;
            (
                DecisionPrimitive::Choice,
                DecisionValue::Choice { selected },
                probability_to_milli(confidence),
                Some(probability_to_milli(confidence)),
            )
        }
        "score" => {
            let score = answer
                .get("score")
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite() && *value >= 0.0)
                .ok_or_else(|| format!("Jev score answer {id} is invalid"))?;
            let normalized = (score / score_max_index(id)).clamp(0.0, 1.0);
            let confidence = bounded_probability(answer.get("confidence"), id)?;
            (
                DecisionPrimitive::Score,
                DecisionValue::Score {
                    score_milli: probability_to_milli(normalized),
                },
                probability_to_milli(confidence),
                Some(probability_to_milli(confidence)),
            )
        }
        other => return Err(format!("Jev answer {id} has unsupported type {other}")),
    };
    Ok(DecisionSignal {
        id: id.to_owned(),
        primitive,
        mode: signal_mode(id),
        value,
        confidence_milli,
        native_confidence_milli,
        probabilities_milli,
        recommendation: recommendation(id).to_owned(),
        evidence: vec![
            "provider:jev".into(),
            if answer_type == "noul" {
                "confidence:not_applicable;use_noul_probability_directly".into()
            } else {
                "confidence:native".into()
            },
        ],
    })
}

pub(crate) fn advisory_guidance(
    baseline: &DecisionBatch,
    candidate: &DecisionBatch,
) -> Vec<String> {
    let mut guidance = Vec::new();
    if probability(candidate, "continue_retrieval").is_some_and(|value| value >= 650) {
        guidance.push("jev:retrieve_more_evidence".into());
    }
    if probability(candidate, "semantic_navigation_required").is_some_and(|value| value >= 650) {
        guidance.push("jev:prefer_semantic_navigation".into());
    }
    if probability(candidate, "verification_escalation_value").is_some_and(|value| value >= 650) {
        guidance.push("jev:preserve_or_raise_verification".into());
    }
    if let Some(signal) = candidate
        .signals
        .iter()
        .find(|signal| signal.id == "next_action")
    {
        if !choice_signal_is_concentrated(signal) {
            guidance.push("jev:choice_uncertain_collect_evidence".into());
        } else if let DecisionValue::Choice { selected } = &signal.value {
            let baseline_action = choice(baseline, "next_action");
            let increase_only = matches!(
                selected.as_str(),
                "retrieve" | "semantic_navigation" | "review_worktree"
            );
            if increase_only || baseline_action == Some(selected.as_str()) {
                guidance.push(format!("jev:next_action:{selected}"));
            }
        }
    }
    if let Some(signal) = candidate
        .signals
        .iter()
        .find(|signal| signal.id == "capability_group")
    {
        if choice_signal_is_concentrated(signal) {
            if let DecisionValue::Choice { selected } = &signal.value {
                if selected != "none" && !crate::harness::model_tools_for_group(selected).is_empty()
                {
                    guidance.push(format!("jev:capability_group:{selected}"));
                }
            }
        }
    }
    if let Some(signal) = candidate
        .signals
        .iter()
        .find(|signal| signal.id == "risk_surface")
    {
        if choice_signal_is_concentrated(signal) {
            if let DecisionValue::Choice { selected } = &signal.value {
                if selected != "none" {
                    guidance.push(format!("jev:risk_surface:{selected}"));
                }
            }
        }
    }
    guidance
}

fn choice<'a>(batch: &'a DecisionBatch, id: &str) -> Option<&'a str> {
    batch.signals.iter().find_map(|signal| {
        (signal.id == id)
            .then_some(&signal.value)
            .and_then(|value| match value {
                DecisionValue::Choice { selected } => Some(selected.as_str()),
                DecisionValue::Probability { .. } | DecisionValue::Score { .. } => None,
            })
    })
}

fn probability(batch: &DecisionBatch, id: &str) -> Option<u16> {
    batch.signals.iter().find_map(|signal| {
        (signal.id == id)
            .then_some(&signal.value)
            .and_then(|value| match value {
                DecisionValue::Probability { probability_milli } => Some(*probability_milli),
                DecisionValue::Choice { .. } | DecisionValue::Score { .. } => None,
            })
    })
}

fn answer_probability_distribution(
    answer: &Value,
    id: &str,
) -> Result<std::collections::BTreeMap<String, u16>, String> {
    let Some(probabilities) = answer.get("probabilities") else {
        return Ok(std::collections::BTreeMap::new());
    };
    let probabilities = probabilities
        .as_object()
        .ok_or_else(|| format!("Jev answer {id} contains invalid probabilities"))?;
    probabilities
        .iter()
        .map(|(label, value)| {
            bounded_probability(Some(value), id)
                .map(|probability| (label.clone(), probability_to_milli(probability)))
        })
        .collect()
}

fn bounded_probability(value: Option<&Value>, id: &str) -> Result<f64, String> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
        .ok_or_else(|| format!("Jev answer {id} contains an invalid probability"))
}

fn probability_to_milli(value: f64) -> u16 {
    (value.clamp(0.0, 1.0) * 1000.0).round() as u16
}

#[cfg(not(test))]
fn endpoint_url(base_url: &str) -> Result<Url, String> {
    let base = Url::parse(&format!("{}/", base_url.trim_end_matches('/')))
        .map_err(|_| "Jev base URL is invalid".to_owned())?;
    base.join("v1/systemone")
        .map_err(|_| "Jev endpoint URL is invalid".to_owned())
}

fn validate_base_url(base_url: &str) -> Result<(), String> {
    let url = Url::parse(base_url).map_err(|_| "Jev base URL is invalid".to_owned())?;
    if url.query().is_some()
        || url.fragment().is_some()
        || url.username() != ""
        || url.password().is_some()
    {
        return Err("Jev base URL must not contain credentials, query, or fragment".into());
    }
    if url.scheme() == "https" {
        return Ok(());
    }
    if url.scheme() == "http" && host_is_loopback(&url) {
        return Ok(());
    }
    Err("Jev base URL must use HTTPS unless it targets loopback".into())
}

fn host_is_loopback(url: &Url) -> bool {
    url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}

#[cfg(test)]
#[path = "../../tests/unit/intelligence/jev.rs"]
mod tests;
