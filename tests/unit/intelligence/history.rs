use super::*;
use std::time::Instant;

fn reset_reads() {
    REVISION_CALLS.with(|count| count.set(0));
    crate::evidence_store::LOAD_CALLS.with(|count| count.set(0));
}
fn reads() -> (usize, usize) {
    (
        REVISION_CALLS.with(|count| count.get()),
        crate::evidence_store::LOAD_CALLS.with(|count| count.get()),
    )
}

#[test]
fn history_snapshot_reuses_reads_and_matches_individual_statuses() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let plans = (0..16)
        .map(|_| {
            runtime
                .create_plan_for_risk("history", &workspace, RiskLevel::Low)
                .unwrap()
        })
        .collect::<Vec<_>>();
    reset_reads();
    let start = Instant::now();
    let expected = plans
        .iter()
        .map(|plan| {
            let status = runtime
                .verification_status("history", &workspace, &plan.id)
                .unwrap();
            (plan.id.clone(), serde_json::to_value(status).unwrap())
        })
        .collect::<BTreeMap<_, _>>();
    let individual_us = start.elapsed().as_micros();
    let individual_reads = reads();
    assert_eq!(individual_reads, (16, 16));
    reset_reads();
    let start = Instant::now();
    let actual = runtime
        .verification_history("history", &workspace, 100)
        .unwrap();
    let batched_us = start.elapsed().as_micros();
    let batched_reads = reads();
    assert_eq!(batched_reads, (1, 1), "history must share real input reads");
    assert_eq!(actual.len(), 16);
    for status in actual {
        assert_eq!(
            serde_json::to_value(&status).unwrap(),
            expected[&status.plan.id]
        );
    }
    let report = serde_json::json!({
        "round": 21, "plans": 16,
        "individual_revision_scans": individual_reads.0,
        "individual_evidence_loads": individual_reads.1,
        "batch_revision_scans": batched_reads.0,
        "batch_evidence_loads": batched_reads.1,
        "individual_microseconds": individual_us, "batch_microseconds": batched_us,
        "same_statuses": true,
    });
    let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(
        target.join("wcode-history-perf.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}

#[test]
fn empty_history_does_not_scan_source_or_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    reset_reads();
    assert!(runtime
        .verification_history("empty", &workspace, 100)
        .unwrap()
        .is_empty());
    assert_eq!(reads(), (0, 0));
}

#[test]
fn history_rejects_foreign_plan_before_reading_workspace_inputs() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let a = Workspace::new(first.path(), false, false).unwrap();
    let b = Workspace::new(second.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let plan = runtime
        .create_plan_for_risk("A", &a, RiskLevel::Low)
        .unwrap();
    reset_reads();
    let error = runtime.verification_status("B", &b, &plan.id).unwrap_err();
    assert!(error.to_string().contains("selected workspace"));
    assert_eq!(reads(), (0, 0));
}

#[test]
fn history_snapshot_is_not_cached_across_edits_or_new_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let plan = runtime
        .create_plan_for_risk("history", &workspace, RiskLevel::Low)
        .unwrap();
    let revision = runtime.current_revision(&workspace).unwrap();
    for (id, timestamp, result) in [
        ("EV-FAILED", 1, EvidenceResult::Fail),
        ("EV-RETRY", 2, EvidenceResult::Pass),
    ] {
        let mut record = Evidence::new(
            id.into(),
            plan.subject.clone(),
            EvidenceKind::Verification,
            "verify_project".into(),
            revision.clone(),
            result,
            Confidence::Deterministic,
        )
        .unwrap();
        record.timestamp_ms = timestamp;
        record.policy = Some("deterministic/full/v1".into());
        crate::evidence_store::persist(&workspace, &record).unwrap();
        assert_eq!(
            runtime
                .verification_history("history", &workspace, 100)
                .unwrap()[0]
                .deterministic_result,
            Some(result)
        );
    }
    fs::write(dir.path().join("changed.rs"), "fn changed() {}\n").unwrap();
    let changed = runtime
        .verification_history("history", &workspace, 100)
        .unwrap();
    assert!(!changed[0].ready);
    assert!(changed[0]
        .blockers
        .iter()
        .any(|item| item.contains("revision-changed")));
}
