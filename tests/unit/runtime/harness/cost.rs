use super::*;
use crate::evidence::{Confidence, Evidence, EvidenceKind, EvidenceResult, Revision};
use crate::harness::harness_cost::VerificationCostEvaluation;

fn check(id: &str, phase: u8) -> CheckSpec {
    CheckSpec {
        id: id.to_owned(),
        level: if phase == 0 { "quick" } else { "full" }.to_owned(),
        phase,
        program: "fixture".to_owned(),
        args: vec![id.to_owned()],
        cwd: ".".to_owned(),
        island: "workspace".to_owned(),
        languages: vec!["fixture".to_owned()],
        reason: "fixture".to_owned(),
    }
}

fn metric_evidence(check: &CheckSpec, elapsed_ms: u128, failed: bool, index: usize) -> Evidence {
    metric_evidence_for_revision(check, elapsed_ms, failed, index, index)
}

fn metric_evidence_for_revision(
    check: &CheckSpec,
    elapsed_ms: u128,
    failed: bool,
    evidence_index: usize,
    revision_index: usize,
) -> Evidence {
    let mut evidence = Evidence::new(
        format!("EV-cost-{evidence_index}"),
        format!("verification:{}", check.id),
        EvidenceKind::Verification,
        verification_command_text(check),
        Revision {
            code: format!("sha256:{revision_index}"),
            design: None,
        },
        if failed {
            EvidenceResult::Fail
        } else {
            EvidenceResult::Pass
        },
        Confidence::Deterministic,
    )
    .unwrap();
    evidence.summary = Some(harness_cost::metrics_summary(elapsed_ms, check.phase));
    evidence
}

#[test]
fn cost_model_promotes_only_a_proven_high_value_phase_zero_sentinel() {
    let format = check("format", 0);
    let compile = check("compile", 0);
    let test = check("test", 1);
    let plan = vec![format.clone(), compile.clone(), test.clone()];
    let mut evidence = Vec::new();
    for index in 0..4 {
        evidence.push(metric_evidence(&format, 100, index < 2, index));
        evidence.push(metric_evidence(&compile, 1_000, false, index + 10));
        evidence.push(metric_evidence(&test, 5_000, false, index + 20));
    }

    let (adapted, decision) = harness_cost::apply_cost_model_from_evidence(&plan, &evidence);
    let decision = decision.expect("format should save expected fail-fast time");
    assert_eq!(decision.sentinel_check, "format");
    assert_eq!(decision.samples, 4);
    assert_eq!(decision.failures, 2);
    assert_eq!(decision.median_elapsed_ms, 100);
    assert!(decision.estimated_savings_ms > 0);
    assert_eq!(adapted.len(), plan.len());
    assert_eq!(adapted[0].id, "format");
    assert_eq!(adapted[0].phase, 0);
    assert_eq!(adapted[1].phase, 1);
    assert_eq!(adapted[2].phase, 2);
}

#[test]
fn cost_frontier_adds_a_second_sentinel_only_for_independent_failures() {
    let format = check("format", 0);
    let lint = check("lint", 0);
    let compile = check("compile", 0);
    let test = check("test", 1);
    let plan = vec![format.clone(), lint.clone(), compile.clone(), test.clone()];
    let mut evidence = Vec::new();
    for revision in 0..6 {
        let base = revision * 10;
        evidence.push(metric_evidence_for_revision(
            &format,
            100,
            revision < 2,
            base,
            revision,
        ));
        evidence.push(metric_evidence_for_revision(
            &lint,
            200,
            matches!(revision, 2 | 3),
            base + 1,
            revision,
        ));
        evidence.push(metric_evidence_for_revision(
            &compile,
            1_000,
            false,
            base + 2,
            revision,
        ));
        evidence.push(metric_evidence_for_revision(
            &test,
            5_000,
            false,
            base + 3,
            revision,
        ));
    }

    let (adapted, decision) = harness_cost::apply_cost_model_from_evidence(&plan, &evidence);
    let decision = decision.expect("two independent fail-fast sentinels should be worthwhile");
    assert_eq!(decision.model, "verification-cost-frontier-v3");
    assert_eq!(decision.sentinel_check, "format");
    assert_eq!(decision.frontier.len(), 2);
    assert_eq!(decision.frontier[0].check_id, "format");
    assert_eq!(decision.frontier[1].check_id, "lint");
    assert_eq!(decision.frontier[1].marginal_samples, 4);
    assert_eq!(decision.frontier[1].marginal_failures, 2);
    assert!(decision.frontier[1].estimated_incremental_savings_ms > 0);
    assert!(decision.estimated_total_savings_ms > decision.estimated_savings_ms);
    assert_eq!(
        adapted.iter().map(|check| check.phase).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    let mut original_ids = plan
        .iter()
        .map(|check| check.id.clone())
        .collect::<Vec<_>>();
    let mut adapted_ids = adapted
        .iter()
        .map(|check| check.id.clone())
        .collect::<Vec<_>>();
    original_ids.sort();
    adapted_ids.sort();
    assert_eq!(
        adapted_ids, original_ids,
        "frontier may only reorder checks"
    );
}

#[test]
fn cost_frontier_rejects_a_correlated_second_sentinel() {
    let format = check("format", 0);
    let lint = check("lint", 0);
    let compile = check("compile", 0);
    let test = check("test", 1);
    let plan = vec![format.clone(), lint.clone(), compile.clone(), test.clone()];
    let mut evidence = Vec::new();
    for revision in 0..6 {
        let base = revision * 10;
        let correlated_failure = revision < 2;
        evidence.push(metric_evidence_for_revision(
            &format,
            100,
            correlated_failure,
            base,
            revision,
        ));
        evidence.push(metric_evidence_for_revision(
            &lint,
            200,
            correlated_failure,
            base + 1,
            revision,
        ));
        evidence.push(metric_evidence_for_revision(
            &compile,
            1_000,
            false,
            base + 2,
            revision,
        ));
        evidence.push(metric_evidence_for_revision(
            &test,
            5_000,
            false,
            base + 3,
            revision,
        ));
    }

    let (adapted, decision) = harness_cost::apply_cost_model_from_evidence(&plan, &evidence);
    let decision = decision.expect("first sentinel should still be valuable");
    assert_eq!(decision.sentinel_check, "format");
    assert_eq!(decision.frontier.len(), 1);
    assert_eq!(
        adapted.iter().map(|check| check.phase).collect::<Vec<_>>(),
        [0, 1, 1, 2],
        "correlated failures must not create a redundant second frontier stage"
    );
}

#[test]
fn duplicate_retry_evidence_cannot_manufacture_revision_sample_confidence() {
    let format = check("format", 0);
    let compile = check("compile", 0);
    let test = check("test", 1);
    let plan = vec![format.clone(), compile.clone(), test.clone()];
    let mut evidence = Vec::new();

    for revision in 0..4 {
        let mut compile_record =
            metric_evidence_for_revision(&compile, 1_000, false, 100 + revision, revision);
        compile_record.timestamp_ms = 1_000 + revision as u64;
        evidence.push(compile_record);
        let mut test_record =
            metric_evidence_for_revision(&test, 5_000, false, 200 + revision, revision);
        test_record.timestamp_ms = 1_000 + revision as u64;
        evidence.push(test_record);
    }
    for retry in 0..4 {
        let mut record = metric_evidence_for_revision(&format, 100, true, retry, 0);
        record.timestamp_ms = 2_000 + retry as u64;
        evidence.push(record);
    }

    let (unchanged, decision) = harness_cost::apply_cost_model_from_evidence(&plan, &evidence);
    assert!(
        decision.is_none(),
        "one revision must remain one sample despite retries"
    );
    assert_eq!(
        unchanged
            .iter()
            .map(|check| check.phase)
            .collect::<Vec<_>>(),
        [0, 0, 1]
    );
}

#[test]
fn latest_revision_observation_wins_and_equal_timestamp_conflicts_fail_closed() {
    let format = check("format", 0);
    let compile = check("compile", 0);
    let test = check("test", 1);
    let plan = vec![format.clone(), compile.clone(), test.clone()];
    let mut evidence = Vec::new();

    for revision in 0..4 {
        let base = revision * 10;
        let timestamp = 10_000 + revision as u64 * 10;
        let mut stale_fail = metric_evidence_for_revision(&format, 100, true, base, revision);
        stale_fail.timestamp_ms = timestamp;
        evidence.push(stale_fail);
        let mut latest_pass = metric_evidence_for_revision(&format, 110, false, base + 1, revision);
        latest_pass.timestamp_ms = timestamp + 1;
        evidence.push(latest_pass);
        let mut compile_record =
            metric_evidence_for_revision(&compile, 1_000, false, base + 2, revision);
        compile_record.timestamp_ms = timestamp + 1;
        evidence.push(compile_record);
        let mut test_record = metric_evidence_for_revision(&test, 5_000, false, base + 3, revision);
        test_record.timestamp_ms = timestamp + 1;
        evidence.push(test_record);
    }

    let (_, decision) = harness_cost::apply_cost_model_from_evidence(&plan, &evidence);
    assert!(
        decision.is_none(),
        "newer passes must replace stale failures per revision"
    );

    for revision in 0..4 {
        let mut tie_fail =
            metric_evidence_for_revision(&format, 120, true, 100 + revision, revision);
        tie_fail.timestamp_ms = 10_001 + revision as u64 * 10;
        evidence.push(tie_fail);
    }
    let (_, decision) = harness_cost::apply_cost_model_from_evidence(&plan, &evidence);
    let decision = decision.expect("equal-timestamp pass/fail conflicts must stay fail-closed");
    assert_eq!(decision.samples, 4);
    assert_eq!(decision.failures, 4);
}

#[test]
fn cost_backtest_circuit_breaker_stops_proven_regressions_but_allows_cold_start() {
    let cold = VerificationCostEvaluation {
        evaluable_revisions: 3,
        regret_ms: 1_000,
        ..VerificationCostEvaluation::default()
    };
    assert!(harness_cost::cost_backtest_allows_candidate(&cold));

    let mismatch = VerificationCostEvaluation {
        outcome_mismatches: 1,
        ..VerificationCostEvaluation::default()
    };
    assert!(!harness_cost::cost_backtest_allows_candidate(&mismatch));

    let mature_loss = VerificationCostEvaluation {
        evaluable_revisions: 4,
        gross_savings_ms: 100,
        regret_ms: 101,
        ..VerificationCostEvaluation::default()
    };
    assert!(!harness_cost::cost_backtest_allows_candidate(&mature_loss));

    let mature_tie = VerificationCostEvaluation {
        evaluable_revisions: 4,
        gross_savings_ms: 100,
        regret_ms: 100,
        ..VerificationCostEvaluation::default()
    };
    assert!(!harness_cost::cost_backtest_allows_candidate(&mature_tie));

    let mature_win = VerificationCostEvaluation {
        evaluable_revisions: 4,
        gross_savings_ms: 101,
        regret_ms: 100,
        ..VerificationCostEvaluation::default()
    };
    assert!(harness_cost::cost_backtest_allows_candidate(&mature_win));
}

#[test]
fn cost_model_cold_start_and_low_failure_history_preserve_static_phases() {
    let format = check("format", 0);
    let compile = check("compile", 0);
    let test = check("test", 1);
    let plan = vec![format.clone(), compile.clone(), test.clone()];
    let sparse = vec![
        metric_evidence(&format, 100, true, 0),
        metric_evidence(&compile, 1_000, false, 1),
        metric_evidence(&test, 5_000, false, 2),
    ];
    let (unchanged, decision) = harness_cost::apply_cost_model_from_evidence(&plan, &sparse);
    assert!(decision.is_none());
    assert_eq!(
        unchanged
            .iter()
            .map(|check| check.phase)
            .collect::<Vec<_>>(),
        [0, 0, 1]
    );

    let mut low_failure = Vec::new();
    for index in 0..6 {
        low_failure.push(metric_evidence(&format, 100, index == 0, index));
        low_failure.push(metric_evidence(&compile, 1_000, false, index + 10));
        low_failure.push(metric_evidence(&test, 5_000, false, index + 20));
    }
    let (unchanged, decision) = harness_cost::apply_cost_model_from_evidence(&plan, &low_failure);
    assert!(decision.is_none());
    assert_eq!(
        unchanged
            .iter()
            .map(|check| check.phase)
            .collect::<Vec<_>>(),
        [0, 0, 1]
    );
}
