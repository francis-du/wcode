use super::*;

#[test]
fn acceptance_arguments_reject_forged_proof_and_approval() {
    for value in [
        json!({"evidence":[]}),
        json!({"ready":true}),
        json!({"approved":true}),
        json!({"action":"approve"}),
        json!({"actor":"owner"}),
        json!({"base_revision":"main"}),
        json!({"target_revision":"HEAD~1"}),
        json!({"base_revision":"worktree"}),
        json!({"action":"inspect","timeout_seconds":120}),
        json!({"action":"history","base_revision":"HEAD"}),
        json!({"action":"verify","timeout_seconds":1801}),
        json!({"workspace":null}),
    ] {
        assert!(Args::parse(&value).is_err(), "{value}");
    }
    for action in ["inspect", "plan", "record", "verify", "history"] {
        assert!(Args::parse(&json!({"action":action})).is_ok());
    }
}
