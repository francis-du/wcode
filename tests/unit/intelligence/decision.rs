use crate::decision::{
    agent_context_decision_request, agent_context_decisions, calibration_brier_million,
    compare_decision_batches, probability_calibration_summary, probability_milli, DecisionBatch,
    DecisionMode, DecisionProvider, DecisionRequest, DecisionValue, DeterministicDecisionProvider,
    ProbabilityCalibrationSample,
};
use serde_json::json;

fn ready_pack() -> serde_json::Value {
    json!({
        "targets": [{"id":"symbol:1"}],
        "hot_source": [{"id":"symbol:1"}],
        "tests": [{"target":"tests::target"}],
        "risks": [],
        "repo_map": {"truncated": false},
        "readiness": {
            "edit": "ready",
            "verify": "ready",
            "graph_precision": "syntax",
            "editable_sha_targets": 1
        }
    })
}

#[test]
fn ready_context_prefers_edit_without_claiming_safety_authority() {
    let batch = agent_context_decisions(&ready_pack(), "fix target_feature");
    assert_eq!(batch.policy.authority, "advisory_only");
    assert!(!batch.policy.can_reduce_safety);
    assert!(batch.policy.deterministic_verification_floor);

    let next = batch
        .signals
        .iter()
        .find(|signal| signal.id == "next_action")
        .unwrap();
    assert_eq!(next.mode, DecisionMode::Assist);
    assert_eq!(
        next.value,
        DecisionValue::Choice {
            selected: "edit".into()
        }
    );

    let retrieval = batch
        .signals
        .iter()
        .find(|signal| signal.id == "continue_retrieval")
        .unwrap();
    assert!(matches!(
        retrieval.value,
        DecisionValue::Probability {
            probability_milli: 0..=200
        }
    ));
}

#[test]
fn unresolved_context_recommends_retrieval_and_semantic_escalation() {
    let mut pack = ready_pack();
    pack["targets"] = json!([]);
    pack["hot_source"] = json!([]);
    pack["tests"] = json!([]);
    pack["repo_map"]["truncated"] = json!(true);
    pack["readiness"]["edit"] = json!("needs_source");
    pack["readiness"]["editable_sha_targets"] = json!(0);

    let batch = agent_context_decisions(&pack, "find callers and references");
    let next = batch
        .signals
        .iter()
        .find(|signal| signal.id == "next_action")
        .unwrap();
    assert_eq!(
        next.value,
        DecisionValue::Choice {
            selected: "retrieve".into()
        }
    );

    let semantic = batch
        .signals
        .iter()
        .find(|signal| signal.id == "semantic_navigation_value")
        .unwrap();
    assert!(matches!(
        semantic.value,
        DecisionValue::Probability {
            probability_milli: 900
        }
    ));
}

#[test]
fn repo_map_truncation_does_not_override_edit_ready_context() {
    let mut pack = ready_pack();
    pack["tests"] = json!([]);
    pack["repo_map"]["truncated"] = json!(true);
    let batch = agent_context_decisions(&pack, "fix target_feature");
    assert_eq!(probability_milli(&batch, "context_sufficient"), Some(900));

    pack["readiness"]["edit"] = json!("needs_source");
    let batch = agent_context_decisions(&pack, "fix target_feature");
    assert_eq!(probability_milli(&batch, "context_sufficient"), Some(580));
}

#[test]
fn risk_can_only_raise_or_preserve_verification_work() {
    let mut pack = ready_pack();
    pack["risks"] = json!([{"level":"high"}]);
    let batch = agent_context_decisions(&pack, "change verification runtime");
    let verification = batch
        .signals
        .iter()
        .find(|signal| signal.id == "verification_escalation_value")
        .unwrap();
    assert_eq!(verification.mode, DecisionMode::Shadow);
    assert!(matches!(
        verification.value,
        DecisionValue::Probability {
            probability_milli: 850
        }
    ));
    assert_eq!(
        verification.recommendation,
        "preserve_or_raise_deterministic_verification_floor"
    );
    assert!(!batch.policy.can_reduce_safety);
}

#[test]
fn brier_metric_is_bounded_and_rewards_calibrated_outcomes() {
    assert_eq!(calibration_brier_million(1000, true), 0);
    assert_eq!(calibration_brier_million(0, false), 0);
    assert_eq!(calibration_brier_million(500, true), 250_000);
    assert_eq!(calibration_brier_million(500, false), 250_000);
}

#[test]
fn calibration_summary_tracks_false_stops_false_continues_and_mean_brier() {
    let summary = probability_calibration_summary(
        &[
            ProbabilityCalibrationSample::new(900, false),
            ProbabilityCalibrationSample::new(800, true),
            ProbabilityCalibrationSample::new(200, true),
            ProbabilityCalibrationSample::new(100, false),
        ],
        700,
    );
    assert_eq!(summary.samples, 4);
    assert_eq!(summary.threshold_milli, 700);
    assert_eq!(summary.mean_brier_million, Some(375_000));
    assert_eq!(summary.false_stop, 1);
    assert_eq!(summary.false_continue, 1);
    assert_eq!(summary.correct_stop, 1);
    assert_eq!(summary.correct_continue, 1);
}

#[test]
fn calibration_summary_is_bounded_and_empty_safe() {
    let empty = probability_calibration_summary(&[], 1_500);
    assert_eq!(empty.samples, 0);
    assert_eq!(empty.threshold_milli, 1_000);
    assert_eq!(empty.mean_brier_million, None);
    assert_eq!(empty.false_stop, 0);
    assert_eq!(empty.false_continue, 0);

    let bounded =
        probability_calibration_summary(&[ProbabilityCalibrationSample::new(1_500, true)], 1_500);
    assert_eq!(bounded.threshold_milli, 1_000);
    assert_eq!(bounded.mean_brier_million, Some(0));
    assert_eq!(bounded.correct_stop, 1);
}

#[test]
fn probability_lookup_is_typed_and_does_not_coerce_choice_or_score() {
    let batch = agent_context_decisions(&ready_pack(), "fix target_feature");
    assert_eq!(probability_milli(&batch, "context_sufficient"), Some(940));
    assert_eq!(probability_milli(&batch, "next_action"), None);
    assert_eq!(probability_milli(&batch, "evidence_density"), None);
    assert_eq!(probability_milli(&batch, "missing"), None);
}

struct UnsafeShadowCandidate;

impl DecisionProvider for UnsafeShadowCandidate {
    fn provider_id(&self) -> &'static str {
        "fixture-unsafe-shadow"
    }

    fn evaluate(&self, request: &DecisionRequest) -> DecisionBatch {
        let mut batch = DeterministicDecisionProvider.evaluate(request);
        batch.provider = self.provider_id().into();
        batch.policy.can_reduce_safety = true;
        batch.policy.deterministic_verification_floor = false;
        batch
            .signals
            .retain(|signal| signal.id != "evidence_density");
        if let Some(signal) = batch
            .signals
            .iter_mut()
            .find(|signal| signal.id == "context_sufficient")
        {
            signal.value = DecisionValue::Probability {
                probability_milli: 700,
            };
        }
        if let Some(signal) = batch
            .signals
            .iter_mut()
            .find(|signal| signal.id == "next_action")
        {
            signal.mode = DecisionMode::Shadow;
            signal.value = DecisionValue::Choice {
                selected: "retrieve".into(),
            };
        }
        batch
    }
}

#[test]
fn identical_shadow_batches_have_zero_delta_and_no_policy_violation() {
    let request = agent_context_decision_request(&ready_pack(), "fix target_feature");
    let baseline = DeterministicDecisionProvider.evaluate(&request);
    let comparison = compare_decision_batches(&baseline, &baseline);

    assert_eq!(comparison.shared_signals, baseline.signals.len());
    assert_eq!(comparison.missing_from_baseline, 0);
    assert_eq!(comparison.missing_from_candidate, 0);
    assert_eq!(comparison.probability_abs_delta_milli_sum, 0);
    assert_eq!(comparison.score_abs_delta_milli_sum, 0);
    assert_eq!(comparison.choice_disagreement_count, 0);
    assert_eq!(comparison.primitive_mismatch_count, 0);
    assert_eq!(comparison.mode_mismatch_count, 0);
    assert_eq!(comparison.safety_policy_violation_count, 0);
}

#[test]
fn unsafe_shadow_candidate_is_measured_but_never_granted_authority() {
    let request = agent_context_decision_request(&ready_pack(), "fix target_feature");
    let baseline = DeterministicDecisionProvider.evaluate(&request);
    let candidate = UnsafeShadowCandidate.evaluate(&request);
    let comparison = compare_decision_batches(&baseline, &candidate);

    assert_eq!(comparison.candidate_provider, "fixture-unsafe-shadow");
    assert_eq!(comparison.missing_from_candidate, 1);
    assert_eq!(comparison.probability_pairs, 4);
    assert_eq!(comparison.probability_abs_delta_milli_sum, 240);
    assert_eq!(comparison.choice_pairs, 1);
    assert_eq!(comparison.choice_disagreement_count, 1);
    assert_eq!(comparison.mode_mismatch_count, 1);
    assert_eq!(comparison.safety_policy_violation_count, 2);
    assert!(!candidate.policy.deterministic_verification_floor);
    assert!(candidate.policy.can_reduce_safety);
    assert!(baseline.policy.deterministic_verification_floor);
    assert!(!baseline.policy.can_reduce_safety);
}
