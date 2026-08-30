use super::*;
use crate::risk::RiskLevel;

#[test]
fn verification_state_round_trips_across_runtime_instances() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let mut state = VerificationState::default();
    state
        .create_plan(
            "VP-persist".into(),
            "demo".into(),
            "change:fixture".into(),
            RiskLevel::Low,
            ["VJ-persist".into()].into_iter(),
        )
        .unwrap();
    persist(&workspace, &state.workspace_snapshot("demo")).unwrap();
    let loaded = load(&workspace).unwrap().unwrap();
    let status = loaded.status("VP-persist").unwrap();
    assert_eq!(status.plan.workspace, "demo");
    assert_eq!(status.queued, 1);
}
