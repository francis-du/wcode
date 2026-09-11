use super::*;

fn scoped_record(id: &str, timestamp: u64, result: EvidenceResult) -> Evidence {
    let mut record = Evidence::new(
        id.into(),
        "verification:rust-test".into(),
        EvidenceKind::IntegrationTest,
        "cargo test".into(),
        Revision {
            code: "code:1".into(),
            design: Some("design:1".into()),
        },
        result,
        Confidence::Deterministic,
    )
    .unwrap();
    record.timestamp_ms = timestamp;
    record.policy = Some("deterministic/full/v1".into());
    record
}

#[test]
fn effective_evidence_retry_replaces_only_its_exact_scope() {
    let failure = scoped_record("EV-fail", 1, EvidenceResult::Fail);
    let retry = scoped_record("EV-retry", 2, EvidenceResult::Pass);
    let records = vec![failure.clone(), retry];
    let latest = latest_current(&records, &failure.revision);
    assert_eq!(latest.len(), 1);
    assert_eq!(latest[0].result, EvidenceResult::Pass);
    assert_eq!(
        records[0].result,
        EvidenceResult::Fail,
        "audit history is immutable"
    );
    for change in ["producer", "policy", "target", "model"] {
        let mut other = scoped_record("EV-other", 3, EvidenceResult::Pass);
        match change {
            "producer" => other.producer = "other-runner".into(),
            "policy" => other.policy = Some("deterministic/quick/v1".into()),
            "target" => other.targets = vec!["language:python".into()],
            "model" => other.model = Some("other-model".into()),
            _ => unreachable!(),
        }
        let records = [failure.clone(), other];
        let latest = latest_current(&records, &failure.revision);
        assert_eq!(latest.len(), 2, "{change}");
        assert!(latest
            .iter()
            .any(|item| item.result == EvidenceResult::Fail));
    }
}

#[test]
fn effective_evidence_ties_keep_failure_and_revisions_stay_isolated() {
    let failure = scoped_record("EV-a", 5, EvidenceResult::Fail);
    let pass = scoped_record("EV-z", 5, EvidenceResult::Pass);
    for records in [[failure.clone(), pass.clone()], [pass, failure.clone()]] {
        let latest = latest_current(&records, &failure.revision);
        assert_eq!(latest.len(), 1);
        assert_eq!(latest[0].result, EvidenceResult::Fail);
    }
    let mut stale = scoped_record("EV-old-design", 9, EvidenceResult::Pass);
    stale.revision.design = Some("design:old".into());
    let records = [failure.clone(), stale];
    assert_eq!(latest_current(&records, &failure.revision).len(), 1);
    let unknown = Revision {
        code: "other".into(),
        design: None,
    };
    assert!(latest_current(&records, &unknown).is_empty());
}

#[test]
fn effective_evidence_full_can_supersede_quick_but_never_in_reverse() {
    let mut quick = scoped_record("EV-quick", 1, EvidenceResult::Fail);
    quick.policy = Some("deterministic/quick/v1".into());
    let full = scoped_record("EV-full", 2, EvidenceResult::Pass);
    let records = [quick.clone(), full];
    assert_eq!(latest_current(&records, &quick.revision).len(), 1);
    let mut full = scoped_record("EV-full-fail", 1, EvidenceResult::Fail);
    full.targets = vec!["b".into(), "a".into()];
    let mut later = scoped_record("EV-full-retry", 2, EvidenceResult::Pass);
    later.targets = vec!["a".into(), "b".into(), "a".into()];
    let records = [full.clone(), later];
    assert_eq!(
        latest_current(&records, &full.revision).len(),
        1,
        "target order is not a new scope"
    );
    let mut human = scoped_record("EV-human", 4, EvidenceResult::Pass);
    human.kind = EvidenceKind::HumanApproval;
    assert!(
        latest_current([&human], &human.revision).is_empty(),
        "approval is not test proof"
    );
}

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
