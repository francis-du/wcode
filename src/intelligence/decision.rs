use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const DECISION_SCHEMA_VERSION: &str = "0.1.0";
pub const CONTEXT_SUFFICIENT_STOP_THRESHOLD_MILLI: u16 = 800;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionPrimitive {
    Probability,
    Choice,
    Score,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionMode {
    Shadow,
    Assist,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DecisionValue {
    Probability { probability_milli: u16 },
    Choice { selected: String },
    Score { score_milli: u16 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecisionSignal {
    pub id: String,
    pub primitive: DecisionPrimitive,
    pub mode: DecisionMode,
    pub value: DecisionValue,
    pub confidence_milli: u16,
    pub recommendation: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecisionPolicy {
    pub authority: String,
    pub can_increase_work: bool,
    pub can_reduce_safety: bool,
    pub deterministic_verification_floor: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecisionBatch {
    pub schema_version: String,
    pub provider: String,
    pub scope: String,
    pub policy: DecisionPolicy,
    pub signals: Vec<DecisionSignal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRequest {
    pub scope: String,
    pub state: Value,
}

pub trait DecisionProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;
    fn evaluate(&self, request: &DecisionRequest) -> DecisionBatch;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecisionShadowComparison {
    pub baseline_provider: String,
    pub candidate_provider: String,
    pub shared_signals: usize,
    pub missing_from_baseline: usize,
    pub missing_from_candidate: usize,
    pub baseline_duplicate_signal_ids: usize,
    pub candidate_duplicate_signal_ids: usize,
    pub probability_pairs: usize,
    pub probability_abs_delta_milli_sum: u64,
    pub score_pairs: usize,
    pub score_abs_delta_milli_sum: u64,
    pub choice_pairs: usize,
    pub choice_disagreement_count: usize,
    pub primitive_mismatch_count: usize,
    pub mode_mismatch_count: usize,
    pub safety_policy_violation_count: usize,
    pub scope_mismatch: bool,
    pub schema_mismatch: bool,
}

#[must_use]
pub fn compare_decision_batches(
    baseline: &DecisionBatch,
    candidate: &DecisionBatch,
) -> DecisionShadowComparison {
    let (baseline_signals, baseline_duplicate_signal_ids) = signal_index(&baseline.signals);
    let (candidate_signals, candidate_duplicate_signal_ids) = signal_index(&candidate.signals);
    let mut comparison = DecisionShadowComparison {
        baseline_provider: baseline.provider.clone(),
        candidate_provider: candidate.provider.clone(),
        shared_signals: 0,
        missing_from_baseline: 0,
        missing_from_candidate: 0,
        baseline_duplicate_signal_ids,
        candidate_duplicate_signal_ids,
        probability_pairs: 0,
        probability_abs_delta_milli_sum: 0,
        score_pairs: 0,
        score_abs_delta_milli_sum: 0,
        choice_pairs: 0,
        choice_disagreement_count: 0,
        primitive_mismatch_count: 0,
        mode_mismatch_count: 0,
        safety_policy_violation_count: decision_policy_violations(candidate),
        scope_mismatch: baseline.scope != candidate.scope,
        schema_mismatch: baseline.schema_version != candidate.schema_version,
    };

    for (id, baseline_signal) in &baseline_signals {
        let Some(candidate_signal) = candidate_signals.get(id) else {
            comparison.missing_from_candidate += 1;
            continue;
        };
        comparison.shared_signals += 1;
        comparison.mode_mismatch_count +=
            usize::from(baseline_signal.mode != candidate_signal.mode);
        let baseline_primitive = value_primitive(&baseline_signal.value);
        let candidate_primitive = value_primitive(&candidate_signal.value);
        if baseline_signal.primitive != baseline_primitive
            || candidate_signal.primitive != candidate_primitive
            || baseline_primitive != candidate_primitive
        {
            comparison.primitive_mismatch_count += 1;
            continue;
        }
        match (&baseline_signal.value, &candidate_signal.value) {
            (
                DecisionValue::Probability {
                    probability_milli: baseline_value,
                },
                DecisionValue::Probability {
                    probability_milli: candidate_value,
                },
            ) => {
                comparison.probability_pairs += 1;
                comparison.probability_abs_delta_milli_sum = comparison
                    .probability_abs_delta_milli_sum
                    .saturating_add(u64::from(baseline_value.abs_diff(*candidate_value)));
            }
            (
                DecisionValue::Score {
                    score_milli: baseline_value,
                },
                DecisionValue::Score {
                    score_milli: candidate_value,
                },
            ) => {
                comparison.score_pairs += 1;
                comparison.score_abs_delta_milli_sum = comparison
                    .score_abs_delta_milli_sum
                    .saturating_add(u64::from(baseline_value.abs_diff(*candidate_value)));
            }
            (
                DecisionValue::Choice {
                    selected: baseline_value,
                },
                DecisionValue::Choice {
                    selected: candidate_value,
                },
            ) => {
                comparison.choice_pairs += 1;
                comparison.choice_disagreement_count +=
                    usize::from(baseline_value != candidate_value);
            }
            _ => comparison.primitive_mismatch_count += 1,
        }
    }
    comparison.missing_from_baseline = candidate_signals
        .keys()
        .filter(|id| !baseline_signals.contains_key(*id))
        .count();
    comparison
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DeterministicDecisionProvider;

impl DecisionProvider for DeterministicDecisionProvider {
    fn provider_id(&self) -> &'static str {
        "wcode-deterministic-v1"
    }

    fn evaluate(&self, request: &DecisionRequest) -> DecisionBatch {
        let direct_targets = state_usize(&request.state, "direct_targets");
        let hot_source_items = state_usize(&request.state, "hot_source_items");
        let test_refs = state_usize(&request.state, "test_refs");
        let risk_count = state_usize(&request.state, "risk_count");
        let editable_sha_targets = state_usize(&request.state, "editable_sha_targets");
        let repo_map_truncated = state_bool(&request.state, "repo_map_truncated");
        let semantic_requested = state_bool(&request.state, "semantic_requested");
        let edit = state_str(&request.state, "edit");
        let verify = state_str(&request.state, "verify");
        let graph_precision = state_str(&request.state, "graph_precision");

        let mut sufficient = if direct_targets == 0 {
            180_i32
        } else if edit == "ready" && hot_source_items > 0 && editable_sha_targets > 0 {
            900
        } else if editable_sha_targets > 0 {
            720
        } else {
            460
        };
        if test_refs > 0 {
            sufficient += 40;
        }
        if repo_map_truncated && edit != "ready" {
            sufficient -= 140;
        }
        sufficient = sufficient.clamp(50, 960);
        let sufficient = u16::try_from(sufficient).unwrap_or(500);

        let semantic_value = if semantic_requested && graph_precision == "syntax" {
            900
        } else if semantic_requested {
            700
        } else if repo_map_truncated {
            420
        } else {
            120
        };

        let verification_escalation = if edit == "worktree_conflict" {
            950
        } else if risk_count > 0 {
            850
        } else if verify != "ready" {
            700
        } else {
            250
        };

        let next_action = if direct_targets == 0 || matches!(edit, "needs_source" | "blocked") {
            "retrieve"
        } else if edit == "worktree_conflict" {
            "review_worktree"
        } else if risk_count > 0 {
            "edit_then_verify"
        } else {
            "edit"
        };

        let evidence_density = (hot_source_items.saturating_mul(500)
            + test_refs.saturating_mul(250)
            + editable_sha_targets.saturating_mul(250))
        .checked_div(direct_targets)
        .unwrap_or(0)
        .min(1000);

        let signals = vec![
            probability(
                "context_sufficient",
                sufficient,
                920,
                DecisionMode::Shadow,
                "observe_context_sufficiency",
                evidence_codes(
                    direct_targets,
                    hot_source_items,
                    test_refs,
                    editable_sha_targets,
                    repo_map_truncated,
                ),
            ),
            probability(
                "continue_retrieval",
                1000_u16.saturating_sub(sufficient),
                900,
                DecisionMode::Assist,
                if sufficient >= CONTEXT_SUFFICIENT_STOP_THRESHOLD_MILLI {
                    "prefer_edit_over_more_retrieval"
                } else {
                    "retrieve_more_evidence"
                },
                vec![format!("context_sufficient_milli:{sufficient}")],
            ),
            probability(
                "semantic_navigation_value",
                semantic_value,
                880,
                DecisionMode::Assist,
                if semantic_value >= 700 {
                    "prefer_semantic_navigation"
                } else {
                    "semantic_navigation_optional"
                },
                vec![
                    format!("semantic_requested:{semantic_requested}"),
                    format!("graph_precision:{graph_precision}"),
                ],
            ),
            probability(
                "verification_escalation_value",
                verification_escalation,
                980,
                DecisionMode::Shadow,
                "preserve_or_raise_deterministic_verification_floor",
                vec![
                    format!("risk_count:{risk_count}"),
                    format!("verify:{verify}"),
                    format!("edit:{edit}"),
                ],
            ),
            DecisionSignal {
                id: "next_action".into(),
                primitive: DecisionPrimitive::Choice,
                mode: DecisionMode::Assist,
                value: DecisionValue::Choice {
                    selected: next_action.into(),
                },
                confidence_milli: 950,
                recommendation: "advisory_next_action".into(),
                evidence: vec![
                    format!("direct_targets:{direct_targets}"),
                    format!("edit:{edit}"),
                    format!("risk_count:{risk_count}"),
                ],
            },
            DecisionSignal {
                id: "evidence_density".into(),
                primitive: DecisionPrimitive::Score,
                mode: DecisionMode::Shadow,
                value: DecisionValue::Score {
                    score_milli: u16::try_from(evidence_density).unwrap_or(1000),
                },
                confidence_milli: 1000,
                recommendation: "track_context_quality_without_overriding_readiness".into(),
                evidence: vec![
                    format!("hot_source_items:{hot_source_items}"),
                    format!("test_refs:{test_refs}"),
                    format!("editable_sha_targets:{editable_sha_targets}"),
                ],
            },
        ];

        DecisionBatch {
            schema_version: DECISION_SCHEMA_VERSION.into(),
            provider: self.provider_id().into(),
            scope: request.scope.clone(),
            policy: DecisionPolicy {
                authority: "advisory_only".into(),
                can_increase_work: true,
                can_reduce_safety: false,
                deterministic_verification_floor: true,
            },
            signals,
        }
    }
}

pub fn agent_context_decision_request(pack: &Value, query: &str) -> DecisionRequest {
    DecisionRequest {
        scope: "agent_context".into(),
        state: json!({
            "direct_targets": array_len(pack, "targets"),
            "hot_source_items": array_len(pack, "hot_source"),
            "test_refs": array_len(pack, "tests"),
            "risk_count": array_len(pack, "risks"),
            "editable_sha_targets": pack.pointer("/readiness/editable_sha_targets").and_then(Value::as_u64).unwrap_or(0),
            "repo_map_truncated": pack.pointer("/repo_map/truncated").and_then(Value::as_bool).unwrap_or(false),
            "graph_precision": pack.pointer("/readiness/graph_precision").and_then(Value::as_str).unwrap_or("unknown"),
            "edit": pack.pointer("/readiness/edit").and_then(Value::as_str).unwrap_or("unknown"),
            "verify": pack.pointer("/readiness/verify").and_then(Value::as_str).unwrap_or("unknown"),
            "semantic_requested": query_has_relationship_intent(query),
        }),
    }
}

pub fn agent_context_decisions(pack: &Value, query: &str) -> DecisionBatch {
    DeterministicDecisionProvider.evaluate(&agent_context_decision_request(pack, query))
}

#[must_use]
pub fn evaluate_agent_context_provider(
    provider: &dyn DecisionProvider,
    pack: &Value,
    query: &str,
) -> DecisionBatch {
    provider.evaluate(&agent_context_decision_request(pack, query))
}

#[must_use]
pub fn probability_milli(batch: &DecisionBatch, id: &str) -> Option<u16> {
    batch.signals.iter().find_map(|signal| {
        (signal.id == id)
            .then_some(&signal.value)
            .and_then(|value| match value {
                DecisionValue::Probability { probability_milli } => Some(*probability_milli),
                DecisionValue::Choice { .. } | DecisionValue::Score { .. } => None,
            })
    })
}

#[must_use]
pub fn calibration_brier_million(probability_milli: u16, outcome: bool) -> u32 {
    let probability = i32::from(probability_milli.min(1000));
    let target = if outcome { 1000 } else { 0 };
    u32::try_from((probability - target).pow(2)).unwrap_or(1_000_000)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbabilityCalibrationSample {
    pub probability_milli: u16,
    pub outcome: bool,
}

impl ProbabilityCalibrationSample {
    #[must_use]
    pub fn new(probability_milli: u16, outcome: bool) -> Self {
        Self {
            probability_milli: probability_milli.min(1000),
            outcome,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbabilityCalibrationSummary {
    pub samples: usize,
    pub threshold_milli: u16,
    pub mean_brier_million: Option<u32>,
    pub false_stop: usize,
    pub false_continue: usize,
    pub correct_stop: usize,
    pub correct_continue: usize,
}

#[must_use]
pub fn probability_calibration_summary(
    samples: &[ProbabilityCalibrationSample],
    threshold_milli: u16,
) -> ProbabilityCalibrationSummary {
    let threshold_milli = threshold_milli.min(1000);
    let mut brier_total = 0_u64;
    let (mut false_stop, mut false_continue, mut correct_stop, mut correct_continue) = (0, 0, 0, 0);

    for sample in samples {
        let probability_milli = sample.probability_milli.min(1000);
        brier_total = brier_total.saturating_add(u64::from(calibration_brier_million(
            probability_milli,
            sample.outcome,
        )));
        match (probability_milli >= threshold_milli, sample.outcome) {
            (true, true) => correct_stop += 1,
            (true, false) => false_stop += 1,
            (false, true) => false_continue += 1,
            (false, false) => correct_continue += 1,
        }
    }

    let mean_brier_million = (!samples.is_empty())
        .then(|| u32::try_from(brier_total / samples.len() as u64).unwrap_or(1_000_000));
    ProbabilityCalibrationSummary {
        samples: samples.len(),
        threshold_milli,
        mean_brier_million,
        false_stop,
        false_continue,
        correct_stop,
        correct_continue,
    }
}

fn signal_index(signals: &[DecisionSignal]) -> (BTreeMap<&str, &DecisionSignal>, usize) {
    let mut index = BTreeMap::new();
    let mut duplicates = 0;
    for signal in signals {
        if index.insert(signal.id.as_str(), signal).is_some() {
            duplicates += 1;
        }
    }
    (index, duplicates)
}

const fn value_primitive(value: &DecisionValue) -> DecisionPrimitive {
    match value {
        DecisionValue::Probability { .. } => DecisionPrimitive::Probability,
        DecisionValue::Choice { .. } => DecisionPrimitive::Choice,
        DecisionValue::Score { .. } => DecisionPrimitive::Score,
    }
}

fn decision_policy_violations(batch: &DecisionBatch) -> usize {
    usize::from(batch.policy.authority != "advisory_only")
        + usize::from(batch.policy.can_reduce_safety)
        + usize::from(!batch.policy.deterministic_verification_floor)
}

fn probability(
    id: &str,
    probability_milli: u16,
    confidence_milli: u16,
    mode: DecisionMode,
    recommendation: &str,
    evidence: Vec<String>,
) -> DecisionSignal {
    DecisionSignal {
        id: id.into(),
        primitive: DecisionPrimitive::Probability,
        mode,
        value: DecisionValue::Probability {
            probability_milli: probability_milli.min(1000),
        },
        confidence_milli: confidence_milli.min(1000),
        recommendation: recommendation.into(),
        evidence,
    }
}

fn state_usize(state: &Value, key: &str) -> usize {
    state
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0)
}

fn state_bool(state: &Value, key: &str) -> bool {
    state.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn state_str<'a>(state: &'a Value, key: &str) -> &'a str {
    state.get(key).and_then(Value::as_str).unwrap_or("unknown")
}

fn array_len(value: &Value, key: &str) -> usize {
    value.get(key).and_then(Value::as_array).map_or(0, Vec::len)
}

fn query_has_relationship_intent(query: &str) -> bool {
    let query = query.to_ascii_lowercase();
    [
        "caller",
        "callee",
        "call graph",
        "reference",
        "relationship",
        "dependency",
        "semantic",
        "lsp",
        "调用",
        "引用",
        "依赖",
        "关系",
    ]
    .iter()
    .any(|marker| query.contains(marker))
}

fn evidence_codes(
    direct_targets: usize,
    hot_source_items: usize,
    test_refs: usize,
    editable_sha_targets: usize,
    repo_map_truncated: bool,
) -> Vec<String> {
    vec![
        format!("direct_targets:{direct_targets}"),
        format!("hot_source_items:{hot_source_items}"),
        format!("test_refs:{test_refs}"),
        format!("editable_sha_targets:{editable_sha_targets}"),
        format!("repo_map_truncated:{repo_map_truncated}"),
    ]
}
