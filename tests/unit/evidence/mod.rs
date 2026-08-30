use super::*;

#[test]
fn evidence_requires_revision_and_producer_provenance() {
    let evidence = Evidence::new(
        "EV-SEC-001".into(),
        "REQ-SEC-001".into(),
        EvidenceKind::UnitTest,
        "cargo-test".into(),
        Revision {
            design: Some("design:1".into()),
            code: "git:abc123".into(),
        },
        EvidenceResult::Pass,
        Confidence::Deterministic,
    )
    .unwrap();
    assert_eq!(evidence.validate(), Ok(()));
    assert_eq!(evidence.result, EvidenceResult::Pass);
}

#[test]
fn model_consensus_cannot_be_labeled_deterministic_by_kind() {
    let evidence = Evidence::new(
        "EV-REVIEW-001".into(),
        "component:auth".into(),
        EvidenceKind::ModelReview,
        "reviewer".into(),
        Revision {
            design: None,
            code: "git:abc123".into(),
        },
        EvidenceResult::Pass,
        Confidence::High,
    )
    .unwrap();
    assert_eq!(evidence.kind, EvidenceKind::ModelReview);
    assert_ne!(evidence.confidence, Confidence::Deterministic);
}
