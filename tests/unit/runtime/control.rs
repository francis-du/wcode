use super::*;

#[test]
fn token_comparison_rejects_prefixes_and_differences() {
    assert!(constant_time_eq(b"same", b"same"));
    assert!(!constant_time_eq(b"same", b"sam"));
    assert!(!constant_time_eq(b"same", b"sane"));
}

#[test]
fn control_actions_have_stable_wire_names() {
    assert_eq!(
        serde_json::to_string(&ControlAction::Stop).unwrap(),
        "\"stop\""
    );
    assert_eq!(
        serde_json::to_string(&ControlAction::Restart).unwrap(),
        "\"restart\""
    );
}
