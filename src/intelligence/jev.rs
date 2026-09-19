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
use std::{env, fs, path::PathBuf, time::Duration};

#[cfg(not(test))]
pub(crate) const JEV_API_KEY_ENV: &str = "JEV_API_KEY";
#[cfg(not(test))]
pub(crate) const JEV_BASE_URL_ENV: &str = "JEV_BASE_URL";
#[cfg(not(test))]
pub(crate) const JEV_DEFAULT_MODEL_ENV: &str = "JEV_DEFAULT_MODEL";
pub(crate) const JEV_DEFAULT_BASE_URL: &str = "https://api.typesafe.ai";
pub(crate) const JEV_DEFAULT_MODEL: &str = "jev-latest";
const AGENT_CONTEXT_QUESTION_SET_ID: &str = "wcode.agent_context";
const AGENT_CONTEXT_QUESTION_SET_VERSION: u16 = 3;
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
    ) -> Result<DecisionBatch, String> {
        let mut state = request.state.clone();
        if let Some(object) = state.as_object_mut() {
            object.insert(
                "_jev_question_set".into(),
                json!({
                    "id": AGENT_CONTEXT_QUESTION_SET_ID,
                    "version": AGENT_CONTEXT_QUESTION_SET_VERSION
                }),
            );
        }
        let response = self
            .client
            .post(endpoint_url(&self.config.base_url)?)
            .bearer_auth(&self.config.api_key)
            .json(&json!({
                "model": self.config.model,
                "state": state,
                "questions": agent_context_questions(),
            }))
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
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Jev returned invalid JSON: {error}"))?;
        let mut batch = response_to_batch(request, &value)?;
        for signal in &mut batch.signals {
            signal.evidence.push(format!("model:{}", self.config.model));
            signal.evidence.push(format!(
                "question_set:{}@{}",
                AGENT_CONTEXT_QUESTION_SET_ID, AGENT_CONTEXT_QUESTION_SET_VERSION
            ));
        }
        Ok(batch)
    }
}

pub(crate) async fn augment_agent_context(context: &mut Value, query: &str) -> Result<(), String> {
    #[cfg(test)]
    {
        let _ = (context, query);
        Ok(())
    }

    #[cfg(not(test))]
    {
        let baseline = context
            .get("decision_plane")
            .cloned()
            .and_then(|value| serde_json::from_value::<DecisionBatch>(value).ok())
            .unwrap_or_else(|| agent_context_decisions(context, query));
        let provider = match JevDecisionProvider::from_env() {
            Ok(Some(provider)) => provider,
            Ok(None) => return Ok(()),
            Err(error) => {
                tracing::warn!(error = %error, "Jev configuration rejected; deterministic decision plane retained");
                attach_jev_if_budget_allows(
                    context,
                    json!({
                        "provider": "jev",
                        "status": "invalid_configuration",
                        "fallback": "deterministic"
                    }),
                );
                return Ok(());
            }
        };
        let request = agent_context_decision_request(context, query);
        match provider.evaluate(&request).await {
            Ok(candidate) => {
                let comparison = compare_decision_batches(&baseline, &candidate);
                let guidance = advisory_guidance(&baseline, &candidate);
                attach_jev_if_budget_allows(
                    context,
                    json!({
                        "provider": "jev",
                        "model": provider.model(),
                        "status": "active",
                        "authority": "increase_only_assist",
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
                        "fallback": "deterministic"
                    }),
                );
            }
            Err(error) => {
                tracing::warn!(error = %error, "Jev request failed; deterministic decision plane retained");
                attach_jev_if_budget_allows(
                    context,
                    json!({
                        "provider": "jev",
                        "model": provider.model(),
                        "status": "unavailable",
                        "fallback": "deterministic"
                    }),
                );
            }
        }
        Ok(())
    }
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

pub(crate) fn agent_context_questions() -> Value {
    json!({
        "context_sufficient": {
            "type": "noul",
            "instructions": "Using only the bounded repository state supplied here, is there enough precise evidence to start the intended localized edit without broadening retrieval?",
            "criteria": {
                "true": "Targets, source context, current edit preconditions and verification references are specific enough that no missing repository fact could materially change the target, safety boundary or verification plan.",
                "false": "An exact source, worktree, design, semantic or verification fact is still missing or ambiguous enough to change the target, safety boundary or verification plan."
            }
        },
        "continue_retrieval": {
            "type": "noul",
            "instructions": "Is additional bounded repository retrieval necessary before a safe localized edit?",
            "criteria": {
                "true": "Another exact source, worktree, design, semantic or verification fact is required because it could materially change the edit or its safety.",
                "false": "The target, current source, edit preconditions and verification route are already sufficiently established; unrelated or merely nice-to-have context does not count."
            }
        },
        "semantic_navigation_required": {
            "type": "noul",
            "instructions": "Is semantic navigation such as callers, callees, references, implementations or hover necessary evidence before a safe localized edit?",
            "criteria": {
                "true": "Relationship information is required to understand impact, resolve the intended symbol or locate the correct implementation before editing.",
                "false": "Ordinary source or test retrieval, worktree review, or the already-localized target is sufficient; semantic navigation would only be optional extra context."
            }
        },
        "verification_escalation_value": {
            "type": "noul",
            "instructions": "Must verification depth be preserved or increased because of semantic risk?",
            "criteria": {
                "true": "Observed risk, uncertainty or cross-boundary impact justifies preserving or increasing deterministic verification work.",
                "false": "No additional semantic risk in this bounded state justifies verification beyond the deterministic plan already selected."
            }
        },
        "next_action": {
            "type": "choice",
            "instructions": "Which bounded next action best fits this coding-agent state? This is advisory only and must not reduce deterministic safety, worktree review or verification.",
            "criteria": {
                "retrieve": {
                    "use_when": "Exact source, tests, design, schema or other repository evidence required for a safe edit is still missing.",
                    "do_not_use_when": "The primary missing evidence is a caller/reference/implementation relationship or the worktree must be reviewed first."
                },
                "semantic_navigation": {
                    "use_when": "Caller, callee, reference, implementation or hover relationships are necessary evidence before editing.",
                    "do_not_use_when": "Ordinary retrieval or worktree review is the actual missing step, or the edit is already localized."
                },
                "review_worktree": {
                    "use_when": "Existing target changes, ownership, staging or merge state must be understood before editing.",
                    "do_not_use_when": "The relevant worktree state is already known safe for the localized edit."
                },
                "edit_then_verify": {
                    "use_when": "The exact target, current source, edit preconditions and verification route are sufficiently established for a localized edit.",
                    "do_not_use_when": "Any required retrieval, semantic relationship or worktree review remains."
                },
                "other_review": {
                    "use_when": "The bounded state is too underspecified or none of the other actions safely fits.",
                    "do_not_use_when": "One of the other bounded actions is clearly supported."
                }
            }
        },
        "risk_surface": {
            "type": "choice",
            "instructions": "Which semantic risk surface is most worth checking before or immediately after the localized change? This is a review-priority signal only and never authorizes an edit.",
            "criteria": {
                "stale_state": "Async or cached state may no longer belong to the current request or workspace.",
                "response_contract": "A structurally valid response may not semantically match the request or contract.",
                "workspace_isolation": "State, authorization, or evidence may cross workspace boundaries.",
                "graph_semantics": "Graph relationships, direction, precision, roots, or provenance may be semantically misleading.",
                "verification_gap": "Existing tests may not exercise the real production behavior or failure path.",
                "ui_truthfulness": "The UI may display success, freshness, health, or evidence more strongly than the facts justify.",
                "none": "No specific additional semantic risk surface stands out from the supplied state."
            }
        },
        "evidence_density": {
            "type": "score",
            "instructions": "How dense and edit-ready is the supplied context evidence?",
            "criteria": [
                "Little or no relevant evidence",
                "Some relevant evidence but important gaps remain",
                "Enough localized evidence for a cautious edit",
                "Strong localized evidence plus verification references",
                "Highly edit-ready context with precise targets, source, and checks"
            ]
        }
    })
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

fn signal_mode(id: &str) -> DecisionMode {
    match id {
        "continue_retrieval" | "semantic_navigation_required" | "next_action" => {
            DecisionMode::Assist
        }
        _ => DecisionMode::Shadow,
    }
}

fn recommendation(id: &str) -> &'static str {
    match id {
        "context_sufficient" => "observe_semantic_context_sufficiency",
        "continue_retrieval" => "semantic_retrieval_advice",
        "semantic_navigation_required" => "semantic_navigation_required_before_edit",
        "verification_escalation_value" => "increase_only_verification_advice",
        "next_action" => "typed_jev_routing_without_bypassing_gates",
        "risk_surface" => "semantic_adversarial_review_priority",
        "evidence_density" => "semantic_context_quality_score",
        _ => "jev_advisory",
    }
}

fn score_max_index(id: &str) -> f64 {
    match id {
        "evidence_density" => 4.0,
        _ => 1.0,
    }
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
