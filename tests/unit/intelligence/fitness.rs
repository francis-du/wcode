use super::*;

#[test]
fn fitness_complete_recent_window_is_not_confused_with_missing_selected_records() {
    let mut input = journal(
        (1..=64)
            .map(|at| record(&at.to_string(), at, "succeeded", Some("a")))
            .collect(),
    );
    input.retained_records = 512;
    input.truncated = true;
    let view = summarize(&input, revision("a"), 1_000);
    assert!(view.window_limited);
    assert!(!view.partial);
    assert_eq!(view.sampled_records, 64);
    assert_eq!(view.current[0].samples, 64);
    assert!(view.current[0].trend.is_some());
    input.records.pop();
    let partial = summarize(&input, revision("a"), 1_000);
    assert!(partial.window_limited);
    assert!(partial.partial);
    assert!(partial.current[0].trend.is_none());
}

fn revision(code: &str) -> Revision {
    Revision {
        code: code.into(),
        design: Some("design-a".into()),
    }
}

fn record(id: &str, at: u64, outcome: &str, code: Option<&str>) -> Milestone {
    Milestone {
        event_id: Some(id.into()),
        observed_revision: code.map(revision),
        timestamp_ms: at,
        tool: "verify_project".into(),
        stage: "prove".into(),
        outcome: outcome.into(),
        duration_ms: at,
        paths: Vec::new(),
        verification_level: Some("full".into()),
        checks_run: None,
        checks_failed: None,
    }
}

fn journal(records: Vec<Milestone>) -> ProjectEngineeringJournalView {
    ProjectEngineeringJournalView {
        available: true,
        provider: "engineering-milestone-journal".into(),
        stores_prompts_or_chain_of_thought: false,
        retained_records: records.len(),
        truncated: false,
        records,
    }
}

#[test]
fn fitness_empty_and_unavailable_never_become_success() {
    let mut input = journal(Vec::new());
    let empty = summarize(&input, revision("a"), 1_000);
    assert!(empty.available);
    assert!(empty.current.is_empty());
    assert!(empty.history.is_empty());
    assert_eq!(empty.window_start_ms, None);
    assert_eq!(ratio(0, 0), None);
    input.records.push(record("a", 1, "succeeded", Some("a")));
    input.available = false;
    let unavailable = summarize(&input, revision("a"), 1_000);
    assert!(!unavailable.available);
    assert_eq!(unavailable.sampled_records, 0);
    assert!(unavailable.current.is_empty());
}

#[test]
fn fitness_separates_current_stale_design_and_unbound_observations() {
    let mut design_stale = record("design", 3, "succeeded", Some("a"));
    design_stale.observed_revision.as_mut().unwrap().design = Some("design-b".into());
    let input = journal(vec![
        record("current", 1, "failed", Some("a")),
        record("old", 2, "succeeded", Some("old")),
        design_stale,
        record("legacy", 4, "succeeded", None),
    ]);
    let view = summarize(&input, revision("a"), 1_000);
    assert_eq!(view.sampled_records, 4);
    assert_eq!(view.stale_records, 2);
    assert_eq!(view.unbound_records, 1);
    assert_eq!(view.current.len(), 1);
    assert_eq!(view.current[0].samples, 1);
    assert_eq!(view.current[0].success_rate, Some(0.0));
    assert_eq!(view.history.len(), 3);
    assert_eq!(view.history.iter().filter(|row| row.current).count(), 1);
}

#[test]
fn fitness_deduplicates_events_and_rejects_conflicting_copies() {
    let a = record("a", 1, "succeeded", Some("a"));
    let mut conflict = a.clone();
    conflict.outcome = "failed".into();
    let input = journal(vec![
        a.clone(),
        a,
        conflict,
        record("b", 1, "succeeded", Some("a")),
    ]);
    let view = summarize(&input, revision("a"), 1_000);
    assert_eq!(view.duplicate_records, 2);
    assert_eq!(view.conflicting_events, 1);
    assert!(view.partial);
    assert_eq!(view.current[0].samples, 1);
    assert_eq!(view.sampled_records, 1);
}

#[test]
fn fitness_counts_failures_partials_and_blocks_in_the_denominator() {
    let mut input = journal(vec![
        record("a", 1, "succeeded", Some("a")),
        record("b", 2, "partial", Some("a")),
        record("c", 3, "blocked", Some("a")),
        record("d", 4, "failed", Some("a")),
    ]);
    input.truncated = true;
    input.retained_records = 512;
    let view = summarize(&input, revision("a"), 1_000);
    let row = &view.current[0];
    assert_eq!(row.samples, 4);
    assert_eq!(
        (row.succeeded, row.partial, row.blocked, row.failed),
        (1, 1, 1, 1)
    );
    assert_eq!(row.success_rate, Some(0.25));
    assert_eq!(row.p50_ms, Some(2.5));
    assert_eq!(row.p95_ms, Some(4));
    assert!(row.trend.is_none());
    assert!(view.partial);
    assert_eq!(view.retained_records, 512);
}

#[test]
fn fitness_trends_are_same_tool_level_revision_and_time_ordered() {
    let mut records = (1..=6)
        .map(|at| {
            record(
                &at.to_string(),
                at,
                if at <= 3 { "failed" } else { "succeeded" },
                Some("a"),
            )
        })
        .collect::<Vec<_>>();
    let mut quick = record("quick", 7, "failed", Some("a"));
    quick.verification_level = Some("quick".into());
    records.push(quick);
    records.push(record("old", 8, "failed", Some("old")));
    records.reverse();
    let view = summarize(&journal(records), revision("a"), 1_000);
    let full = view
        .current
        .iter()
        .find(|row| row.verification_level.as_deref() == Some("full"))
        .unwrap();
    let trend = full.trend.as_ref().unwrap();
    assert_eq!((trend.earlier_samples, trend.recent_samples), (3, 3));
    assert_eq!(trend.success_rate_delta_pp, 100.0);
    assert_eq!(trend.p50_delta_ms, 3.0);
    assert_eq!(full.samples, 6);
    let ties = journal(
        (1..=6)
            .map(|id| record(&id.to_string(), 1, "succeeded", Some("a")))
            .collect(),
    );
    assert!(summarize(&ties, revision("a"), 1_000).current[0]
        .trend
        .is_none());
}

#[test]
fn fitness_invalid_and_oversized_inputs_do_not_claim_coverage() {
    let input = journal(vec![
        record("future", 2_000, "succeeded", Some("a")),
        record("bad", 1, "unknown", Some("a")),
    ]);
    let view = summarize(&input, revision("a"), 1_000);
    assert!(view.partial);
    assert_eq!(view.invalid_records, 2);
    assert!(view.current.is_empty());
    let input = journal(
        (1..=65)
            .map(|id| record(&id.to_string(), id, "succeeded", Some("a")))
            .collect(),
    );
    let view = summarize(&input, revision("a"), 1_000);
    assert!(!view.available);
    assert!(view.partial);
    assert!(view.current.is_empty());
}

#[test]
fn fitness_history_is_bounded_and_does_not_disclose_source_or_outcomes_as_proof() {
    let input = journal(
        (1..=20)
            .map(|id| record(&id.to_string(), id, "succeeded", Some(&id.to_string())))
            .collect(),
    );
    let view = summarize(&input, revision("20"), 1_000);
    assert!(view.history_truncated);
    assert_eq!(view.history.len(), 16);
    assert_eq!(view.history[0].revision.code, "20");
    let encoded = serde_json::to_value(&view).unwrap();
    assert!(encoded.get("score").is_none());
    assert!(encoded.get("ready").is_none());
    assert!(encoded.get("paths").is_none());
    assert!(encoded.get("prompt").is_none());
    assert!(!view.not_measured.is_empty());
}
