use super::*;

#[test]
fn streaming_json_size_matches_real_serialization() {
    let value = json!({
        "text": "unicode-你好-🚀",
        "nested": [{"a": 1, "b": true}, null, [1, 2, 3]],
        "escaped": "quote: \" slash: \\ newline: \n"
    });
    assert_eq!(
        context_budget::serialized_json_bytes(&value).unwrap(),
        serde_json::to_vec(&value).unwrap().len()
    );
    assert_eq!(
        context_budget::estimated_json_tokens(&value).unwrap(),
        serde_json::to_vec(&value).unwrap().len().div_ceil(4)
    );
}

#[test]
fn streaming_json_size_matches_real_serialization_across_200_shapes() {
    for round in 0..200usize {
        let value = json!({
            "round": round,
            "ascii": "x".repeat(round % 113),
            "unicode": format!("你好🚀-{}-{}", round, "λ".repeat(round % 19)),
            "flags": [round % 2 == 0, round % 3 == 0, round % 5 == 0],
            "nested": {"left": round.saturating_mul(17), "right": null},
        });
        let encoded = serde_json::to_vec(&value).unwrap();
        assert_eq!(
            context_budget::serialized_json_bytes(&value).unwrap(),
            encoded.len(),
            "round {round}"
        );
        assert_eq!(
            context_budget::estimated_json_tokens(&value).unwrap(),
            encoded.len().div_ceil(4),
            "round {round}"
        );
    }
}

#[test]
fn trimming_drops_secondary_hot_source_before_shrinking_primary() {
    let primary = "primary-body-".repeat(64);
    let secondary = "secondary-body-".repeat(64);
    let mut value = json!({
        "truncated": false,
        "hot_source": [
            {"body": {"content": primary, "truncated": false}},
            {"body": {"content": secondary, "truncated": false}}
        ]
    });
    let mut one_item = value.clone();
    one_item["hot_source"].as_array_mut().unwrap().pop();
    let budget = context_budget::estimated_json_tokens(&one_item).unwrap();

    context_budget::trim_agent_context(&mut value, budget).unwrap();

    let hot_source = value["hot_source"].as_array().unwrap();
    assert_eq!(hot_source.len(), 1);
    assert_eq!(
        hot_source[0]["body"]["content"].as_str().unwrap(),
        "primary-body-".repeat(64)
    );
    assert_eq!(value["truncated"], true);
}

#[test]
fn task_aware_retrieval_specializes_only_when_one_intent_is_clear() {
    use crate::harness::harness_retrieval::{classify_repo_map_intent, RepoMapIntent};

    let trace = classify_repo_map_intent("find the implementation for REQ-AUTH-001");
    assert_eq!(trace.intent, RepoMapIntent::TraceToCode);
    assert!(trace.specialized);

    let tests = classify_repo_map_intent("which tests verify token rotation");
    assert_eq!(tests.intent, RepoMapIntent::CodeToTest);
    assert!(tests.specialized);

    let comment = classify_repo_map_intent(
        "review comment: should this stay consistent with the existing implementation elsewhere?",
    );
    assert_eq!(comment.intent, RepoMapIntent::CommentToContext);
    assert!(comment.specialized);

    let failure = classify_repo_map_intent(
        "test worker failed: thread 'worker' panicked at src/runtime/worker.rs:120 assertion failed",
    );
    assert_eq!(failure.intent, RepoMapIntent::FailureTraceToCode);
    assert!(failure.specialized);
    assert_eq!(failure.reason, "reproduced_failure_trace_signal");

    let ripple = classify_repo_map_intent("show callers and rename impact");
    assert_eq!(ripple.intent, RepoMapIntent::EditToRipple);
    assert!(ripple.specialized);

    let generic = classify_repo_map_intent("optimize token rotation");
    assert_eq!(generic.intent, RepoMapIntent::Context);
    assert!(!generic.specialized);
    assert_eq!(generic.reason, "no_specific_retrieval_signal");

    let ambiguous =
        classify_repo_map_intent("review comment: which tests verify the same behavior elsewhere?");
    assert_eq!(ambiguous.intent, RepoMapIntent::Context);
    assert!(!ambiguous.specialized);
    assert_eq!(ambiguous.reason, "ambiguous_retrieval_signals");
}

#[test]
fn readiness_precision_requires_relationship_coverage_for_every_direct_target() {
    let targets = ["direct:a".to_owned(), "direct:b".to_owned()]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let mut relations = std::collections::HashMap::<String, Vec<Value>>::from([
        (
            "related:a".to_owned(),
            vec![json!({"direct": "direct:a", "precision": "semantic"})],
        ),
        (
            "broad-seed".to_owned(),
            vec![json!({"direct": "direct:broad-seed", "precision": "runtime"})],
        ),
    ]);
    assert_eq!(
        crate::harness::harness_repo_map::repo_map_covered_precision(&targets, &relations),
        "syntax",
        "one semantic target and one uncovered target must stay syntax"
    );

    relations.insert(
        "related:b".to_owned(),
        vec![json!({"direct": "direct:b", "precision": "semantic"})],
    );
    let semantic =
        crate::harness::harness_repo_map::repo_map_covered_precision(&targets, &relations);
    assert_eq!(semantic, "semantic");
    let value = json!({"repo_map": {"precision": semantic}});
    assert_eq!(covered_repo_map_precision(&value), "semantic");

    relations.insert(
        "runtime:a".to_owned(),
        vec![json!({"direct": "direct:a", "precision": "runtime"})],
    );
    assert_eq!(
        crate::harness::harness_repo_map::repo_map_covered_precision(&targets, &relations),
        "semantic",
        "one runtime-covered target must not upgrade a second semantic-only target"
    );
}
