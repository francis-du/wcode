use super::*;

#[test]
fn checkpoint_actions_only_increase_deterministic_work() {
    let telemetry = json!({
        "status":"active",
        "candidate_next_action":"semantic_navigation",
        "guidance":[
            "jev:choice_uncertain_collect_evidence",
            "jev:prefer_semantic_navigation",
            "jev:preserve_or_raise_verification"
        ]
    });
    assert_eq!(
        checkpoint_actions("post_edit_review", &telemetry),
        vec![
            "review_changes(adversarial=true)",
            "semantic_navigation(relevant changed/failed symbol)",
            "verification_plan",
            "verify_project(level=full)"
        ]
    );
}

#[test]
fn verification_failure_keeps_a_deterministic_repair_retry_floor() {
    let telemetry = json!({
        "status":"active",
        "candidate_next_action":"edit_then_verify",
        "guidance":[]
    });
    assert_eq!(
        checkpoint_actions("verification_failure", &telemetry),
        vec![
            "repair_from_failed_check_evidence",
            "verify_project(retry_same_level)"
        ]
    );
}

#[test]
fn disabled_checkpoint_never_adds_work() {
    assert!(checkpoint_actions("post_edit_review", &json!({"status":"disabled"})).is_empty());
}

#[test]
fn verification_failure_keeps_the_repair_floor_when_jev_is_disabled() {
    assert_eq!(
        checkpoint_actions("verification_failure", &json!({"status":"disabled"})),
        vec![
            "repair_from_failed_check_evidence",
            "verify_project(retry_same_level)"
        ]
    );
}

#[tokio::test]
async fn test_evaluate_checkpoint_stays_disabled_and_deterministic() {
    let telemetry = evaluate_checkpoint("verification_failure", &json!({}), "fix_then_retry")
        .await
        .unwrap();
    assert_eq!(telemetry["status"], "disabled");
    assert_eq!(telemetry["authority"], "increase_only_assist");
    assert_eq!(telemetry["fallback"], "deterministic");
    assert_eq!(
        checkpoint_actions("verification_failure", &telemetry),
        vec![
            "repair_from_failed_check_evidence",
            "verify_project(retry_same_level)"
        ]
    );
}
