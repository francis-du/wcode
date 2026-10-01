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
fn receipt_record(
    id: &str,
    timestamp: u64,
    level: &str,
    checks: &[(&str, EvidenceResult)],
) -> Evidence {
    let checks = checks
        .iter()
        .map(|(id, result)| VerificationCheckReceipt {
            execution: crate::evidence::VerificationCheckExecution::Executed,
            check: RequiredVerificationCheck::from_command(id, "cargo", &[], ".", "workspace"),
            result: *result,
            reused_from: None,
        })
        .collect::<Vec<_>>();
    let receipt = VerificationExecutionReceipt {
        execution_git_binding: None,
        schema_version: 1,
        level: level.into(),
        required_checks: checks.iter().map(|item| item.check.clone()).collect(),
        checks,
        skipped_checks: vec![],
    };
    let mut record = scoped_record(id, timestamp, receipt.result());
    record.kind = EvidenceKind::Verification;
    record.subject = "change:code:1".into();
    record.policy = Some(format!("deterministic/{level}/v2"));
    record.execution_receipt = Some(receipt);
    record
}

#[test]
fn effective_receipt_narrow_pass_cannot_hide_broad_failure() {
    let failure = receipt_record(
        "EV-wide-fail",
        1,
        "full",
        &[("a", EvidenceResult::Pass), ("b", EvidenceResult::Fail)],
    );
    let narrow = receipt_record("EV-narrow-pass", 2, "full", &[("a", EvidenceResult::Pass)]);
    let records = [failure.clone(), narrow];
    let latest = latest_current(&records, &failure.revision);
    assert_eq!(latest.len(), 2);
    assert!(latest
        .iter()
        .any(|record| record.result == EvidenceResult::Fail));

    let wider = receipt_record(
        "EV-wide-pass",
        3,
        "full",
        &[("b", EvidenceResult::Pass), ("a", EvidenceResult::Pass)],
    );
    let records = [failure.clone(), wider];
    assert_eq!(latest_current(&records, &failure.revision).len(), 1);
}

#[test]
fn effective_receipt_changed_signature_is_a_distinct_scope() {
    let failure = receipt_record("EV-fail", 1, "full", &[("a", EvidenceResult::Fail)]);
    let mut changed = receipt_record("EV-changed", 2, "full", &[("a", EvidenceResult::Pass)]);
    let receipt = changed.execution_receipt.as_mut().unwrap();
    let binding =
        RequiredVerificationCheck::from_command("a", "cargo", &["test".into()], ".", "workspace");
    receipt.required_checks = vec![binding.clone()];
    receipt.checks[0].check = binding;
    let records = [failure.clone(), changed];
    let latest = latest_current(&records, &failure.revision);
    assert_eq!(latest.len(), 2);
    assert!(latest
        .iter()
        .any(|record| record.result == EvidenceResult::Fail));
}

#[test]
fn effective_receipt_full_supersedes_only_covered_quick() {
    for family in ["deterministic", "acceptance"] {
        let mut quick = receipt_record("EV-quick", 1, "quick", &[("a", EvidenceResult::Fail)]);
        quick.policy = Some(format!("{family}/quick/v2"));
        let mut full = receipt_record(
            "EV-full",
            2,
            "full",
            &[("a", EvidenceResult::Pass), ("b", EvidenceResult::Pass)],
        );
        full.policy = Some(format!("{family}/full/v2"));
        let records = [quick.clone(), full.clone()];
        assert_eq!(
            latest_current(&records, &quick.revision).len(),
            1,
            "{family}"
        );

        let mut narrower = receipt_record("EV-other", 3, "full", &[("b", EvidenceResult::Pass)]);
        narrower.policy = Some(format!("{family}/full/v2"));
        let records = [quick.clone(), narrower];
        assert_eq!(
            latest_current(&records, &quick.revision).len(),
            2,
            "{family}"
        );

        let mut skipped = full.clone();
        let receipt = skipped.execution_receipt.as_mut().unwrap();
        receipt.checks.retain(|item| item.check.id != "a");
        receipt.skipped_checks = vec!["a".into()];
        skipped.result = receipt.result();
        let records = [quick.clone(), skipped];
        assert_eq!(
            latest_current(&records, &quick.revision).len(),
            2,
            "{family}"
        );

        full.timestamp_ms = quick.timestamp_ms;
        let records = [quick.clone(), full];
        assert_eq!(
            latest_current(&records, &quick.revision).len(),
            2,
            "simultaneous full pass cannot resolve quick failure: {family}",
        );
    }
    let full = receipt_record("EV-full-fail", 1, "full", &[("a", EvidenceResult::Fail)]);
    let quick = receipt_record("EV-quick-pass", 2, "quick", &[("a", EvidenceResult::Pass)]);
    let records = [full.clone(), quick];
    assert_eq!(latest_current(&records, &full.revision).len(), 2);
}

#[test]
fn effective_receipt_same_timestamp_keeps_failure_in_any_order() {
    let failure = receipt_record("EV-a", 5, "full", &[("a", EvidenceResult::Fail)]);
    let pass = receipt_record("EV-z", 5, "full", &[("a", EvidenceResult::Pass)]);
    for records in [[failure.clone(), pass.clone()], [pass, failure.clone()]] {
        let latest = latest_current(&records, &failure.revision);
        assert_eq!(latest.len(), 1);
        assert_eq!(latest[0].result, EvidenceResult::Fail);
    }
}

#[test]
fn effective_receipt_invalid_or_inconsistent_pass_cannot_hide_failure() {
    let failure = receipt_record("EV-fail", 1, "full", &[("a", EvidenceResult::Fail)]);
    let mut inconsistent =
        receipt_record("EV-false-pass", 2, "full", &[("a", EvidenceResult::Fail)]);
    inconsistent.result = EvidenceResult::Pass;
    let mut invalid = receipt_record("EV-invalid", 3, "full", &[("a", EvidenceResult::Pass)]);
    invalid.execution_receipt.as_mut().unwrap().schema_version = 99;
    let mut legacy = scoped_record("EV-legacy", 4, EvidenceResult::Pass);
    legacy.subject = failure.subject.clone();
    legacy.kind = failure.kind;
    let records = [failure.clone(), inconsistent, invalid, legacy];
    let latest = latest_current(&records, &failure.revision);
    assert!(latest.iter().any(|record| record.id == failure.id));
}
#[test]
fn legacy_authority_migrates_only_strong_native_verification_receipts() {
    let mut native = receipt_record("EV-typed-legacy", 1, "full", &[("a", EvidenceResult::Pass)]);
    native.producer = "verify_project".into();
    let mut json = serde_json::to_value(&native).unwrap();
    json.as_object_mut().unwrap().remove("authority");
    let legacy: Evidence = serde_json::from_value(json).unwrap();
    assert_eq!(legacy.authority, EvidenceAuthority::LegacyUnknown);
    assert_eq!(
        legacy.effective_authority(),
        EvidenceAuthority::NativeVerification
    );
    for changed in ["stage", "producer", "policy", "verdict", "self_reported"] {
        let mut untrusted = legacy.clone();
        match changed {
            "stage" => untrusted.kind = EvidenceKind::Property,
            "producer" => untrusted.producer = "executor:guessed-native".into(),
            "policy" => untrusted.policy = Some("deterministic/full/v1".into()),
            "verdict" => untrusted.result = EvidenceResult::Fail,
            "self_reported" => untrusted.authority = EvidenceAuthority::SelfReported,
            _ => unreachable!(),
        }
        assert_ne!(
            untrusted.effective_authority(),
            EvidenceAuthority::NativeVerification,
            "{changed}"
        );
    }
}

#[test]
fn advisory_authority_cannot_replace_native_negative_proof() {
    let mut failure = scoped_record("EV-native-failure", 1, EvidenceResult::Fail);
    failure.authority = EvidenceAuthority::NativeVerification;
    let mut forged = failure.clone();
    forged.id = "EV-forged-pass".into();
    forged.result = EvidenceResult::Pass;
    forged.timestamp_ms = 2;
    forged.authority = EvidenceAuthority::SelfReported;
    let records = [failure.clone(), forged];
    let selected = latest_current(&records, &failure.revision);
    assert_eq!(selected.len(), 2);
    assert!(selected.iter().any(|record| record.id == failure.id));
    let mut native_retry = failure.clone();
    native_retry.id = "EV-native-retry".into();
    native_retry.result = EvidenceResult::Pass;
    native_retry.timestamp_ms = 3;
    let retried = [failure.clone(), native_retry];
    let selected = latest_current(&retried, &failure.revision);
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].result, EvidenceResult::Pass);
}
