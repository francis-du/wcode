use super::*;

#[test]
fn engineering_fitness_challenge_budget_cannot_be_raised() {
    let case = super::super::corpus::base_case("rust");
    // The response fits its self-declared limit but violates the experiment.
    let pack = json!({"budget":4_000,"padding":"x".repeat(6_000)});
    assert!(
        score(&pack, &case).within_budget,
        "control must fit the claimed limit"
    );
    let measured = score_for_request(&pack, &case, 1_000);
    assert!(
        !measured.within_budget,
        "the tool must not choose its own grading budget"
    );
    for declared in [json!(2_000), json!("1000"), Value::Null] {
        let pack = json!({"budget":declared});
        assert!(!score_for_request(&pack, &case, 1_000).within_budget);
    }
    assert!(score_for_request(&json!({"budget":1_000}), &case, 1_000).within_budget);
}

#[test]
fn engineering_fitness_challenge_failed_ranking_stays_in_denominator() {
    let mut case = super::super::corpus::base_case("rust");
    case.useful.clear();
    let observed = score(
        &json!({"budget":4_000,"targets":[
        {"path":"src/session.rs","qualified_name":"cleanup_if_owner"},
        {"path":"src/session.rs","qualified_name":"refresh_session"}]}),
        &case,
    );
    assert_eq!(observed.ndcg_at_10, Some(1.0));
    let row = Row {
        case_id: "failed-bug-control".into(),
        language: "rust".into(),
        category: "bug-relevant-evidence".into(),
        fixture_sha256: "fixture".into(),
        fixture_files: 1,
        required_count: 2,
        writable: true,
        no_answer: false,
        budget: 4_000,
        phase: "cold".into(),
        warmup_us: None,
        warmup_error: None,
        samples: vec![
            Sample {
                elapsed_us: 1,
                score: Some(observed),
                error: None,
            },
            Sample {
                elapsed_us: 2,
                score: None,
                error: Some("tool unavailable".into()),
            },
        ],
        latency: distribution(&[1, 2]),
    };
    let summary = summarize(std::slice::from_ref(&row));
    let grouped = breakdown::build(std::slice::from_ref(&row));
    assert_eq!(grouped["language"]["rust"][0]["attempts"], 2);
    assert_eq!(grouped["language"]["rust"][0]["errors"], 1);
    assert_eq!(
        grouped["category"]["bug-relevant-evidence"][0]["required_recall"],
        json!(0.5)
    );
    assert_eq!(summary[0]["bug_relevant_required_recall"], json!(0.5));
    assert_eq!(
        summary[0]["mean_ndcg_at_10"],
        json!(0.5),
        "failed queries must not improve average ranking"
    );
}

#[test]
fn engineering_fitness_challenge_unknown_is_not_zero() {
    let report = Report {
        schema_version: 2,
        metadata: json!({"profile":"test","case_count":1}),
        summary: vec![json!({"budget":1_000,"phase":"cold","attempts":1,"errors":1})],
        breakdown: json!({}),
        controls: vec![],
        rows: vec![],
    };
    let text = markdown(&report);
    assert!(
        text.contains("N/A"),
        "undefined summary metrics must remain unavailable: {text}"
    );
    assert!(
        !text.contains("0.0%"),
        "unknown values were presented as measurements"
    );
}
