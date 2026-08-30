use super::*;
use crate::evidence::{Confidence, EvidenceKind, EvidenceResult, Revision};

#[test]
fn evidence_survives_a_fresh_load_and_is_workspace_scoped() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let first_workspace = Workspace::new(first.path(), false, false).unwrap();
    let second_workspace = Workspace::new(second.path(), false, false).unwrap();
    let evidence = Evidence::new(
        "EV-PERSIST-1".into(),
        "REQ-1".into(),
        EvidenceKind::UnitTest,
        "cargo-test".into(),
        Revision {
            design: None,
            code: "sha256:fixture".into(),
        },
        EvidenceResult::Pass,
        Confidence::Deterministic,
    )
    .unwrap();

    persist(&first_workspace, &evidence).unwrap();
    assert_eq!(load(&first_workspace).unwrap(), vec![evidence]);
    assert!(load(&second_workspace).unwrap().is_empty());
}
