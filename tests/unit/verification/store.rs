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
            crate::verification::VerificationPlanBinding {
                revision: crate::evidence::Revision {
                    design: Some("sha256:design-fixture".into()),
                    code: "sha256:fixture".into(),
                },
                stage_targets: vec!["language:rust".into()],
                automation_gaps: vec!["property:language:rust".into()],
            },
            RiskLevel::Low,
            ["VJ-persist".into()].into_iter(),
        )
        .unwrap();
    persist(&workspace, &state.workspace_snapshot("demo")).unwrap();
    let loaded = load(&workspace).unwrap().unwrap();
    let status = loaded.status("VP-persist").unwrap();
    assert_eq!(status.plan.workspace, "demo");
    assert_eq!(
        status
            .plan
            .revision
            .as_ref()
            .and_then(|revision| revision.design.as_deref()),
        Some("sha256:design-fixture")
    );
    assert_eq!(status.plan.stage_targets, vec!["language:rust"]);
    assert_eq!(status.plan.automation_gaps, vec!["property:language:rust"]);
    assert_eq!(status.queued, 1);
}
