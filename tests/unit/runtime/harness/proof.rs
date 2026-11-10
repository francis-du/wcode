use super::*;
use crate::evidence::{Confidence, Revision};
use std::fs;

fn fixture() -> (tempfile::TempDir, Workspace, ToolHarness) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    fs::write(
        root.path().join(".wcode/project.yaml"),
        "schema_version: 1\nname: proof-fixture\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".wcode/design/product.yaml"),
        "schema_version: 1\nid: product:demo\nname: Demo\nvision: Initial contract\n",
    )
    .unwrap();
    fs::write(root.path().join(".wcode/design/acceptance.yaml"), "- schema_version: 1\n  id: AC-CHECK\n  title: Check\n  statement: Check the implementation.\n  verification:\n    - kind: check\n      id: rust-test\n").unwrap();
    fs::write(root.path().join("main.rs"), "fn main() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    (root, workspace, ToolHarness::new(2).unwrap())
}
fn record(
    id: &str,
    subject: &str,
    timestamp: u64,
    result: EvidenceResult,
    revision: &Revision,
) -> Evidence {
    let mut record = Evidence::new(
        id.into(),
        subject.into(),
        EvidenceKind::IntegrationTest,
        "deterministic-verification-mesh".into(),
        revision.clone(),
        result,
        Confidence::Deterministic,
    )
    .unwrap();
    record.timestamp_ms = timestamp;
    record.policy = Some("acceptance/full/v2".into());
    let check = crate::evidence::RequiredVerificationCheck::from_command(
        "rust-test",
        "cargo",
        &["test".into(), "--locked".into()],
        ".",
        "workspace",
    );
    record.execution_receipt = Some(crate::evidence::VerificationExecutionReceipt {
        execution_git_binding: None,
        schema_version: 1,
        level: "full".into(),
        required_checks: vec![check.clone()],
        checks: vec![crate::evidence::VerificationCheckReceipt {
            execution: crate::evidence::VerificationCheckExecution::Executed,
            check,
            result,
            reused_from: None,
        }],
        skipped_checks: vec![],
    });
    record
}

#[test]
fn observatory_effective_details_are_bounded_redacted_and_keep_history() {
    let (_root, workspace, harness) = fixture();
    let revision = harness.intelligence.current_revision(&workspace).unwrap();
    for index in 0..30 {
        let mut item = record(
            &format!("EV-{index}"),
            &format!("verification:check-{index}"),
            index + 1,
            EvidenceResult::Fail,
            &revision,
        );
        item.summary = Some(["password", "=fixture-value"].concat());
        crate::evidence_store::persist(&workspace, &item).unwrap();
    }
    crate::evidence_store::persist(
        &workspace,
        &record(
            "EV-retry",
            "verification:check-0",
            100,
            EvidenceResult::Pass,
            &revision,
        ),
    )
    .unwrap();
    let project = harness
        .project_observatory("proof", &workspace, None)
        .unwrap();
    assert_eq!(project.proof.current_evidence, 31);
    assert_eq!(project.proof.current_failed, 30);
    assert_eq!(project.proof.effective.total, 30);
    assert_eq!(project.proof.effective.failed, 29);
    assert_eq!(project.proof.effective.passed, 1);
    assert_eq!(project.proof.effective.items.len(), 24);
    assert!(project.proof.effective.truncated);
    let output = serde_json::to_string(&project.proof.effective).unwrap();
    assert!(!output.contains("fixture-value"));
    assert!(output.contains("REDACTED"));
    assert!(project
        .proof
        .effective
        .items
        .iter()
        .all(|item| item.result == EvidenceResult::Fail));
}

#[test]
fn acceptance_summary_does_not_choose_a_favorable_uuid_or_cross_scope_pass() {
    let (_root, workspace, harness) = fixture();
    let revision = harness.intelligence.current_revision(&workspace).unwrap();
    let failure = record("EV-a", "AC-CHECK", 5, EvidenceResult::Fail, &revision);
    let pass = record("EV-z", "AC-CHECK", 5, EvidenceResult::Pass, &revision);
    crate::evidence_store::persist(&workspace, &failure).unwrap();
    crate::evidence_store::persist(&workspace, &pass).unwrap();
    let project = harness
        .project_observatory("proof", &workspace, None)
        .unwrap();
    assert_eq!(project.proof.acceptance.executed, 1);
    assert_eq!(project.proof.acceptance.passed, 0);
    assert_eq!(project.proof.acceptance.fresh, 1);
    let mut other = record("EV-other", "AC-CHECK", 6, EvidenceResult::Pass, &revision);
    other.producer = "other-runner".into();
    crate::evidence_store::persist(&workspace, &other).unwrap();
    assert_eq!(
        harness
            .project_observatory("proof", &workspace, None)
            .unwrap()
            .proof
            .acceptance
            .passed,
        0
    );
}

fn clean_review() -> ChangeReviewReport {
    ChangeReviewReport {
        workspace: "proof".into(),
        execution: "fixture".into(),
        clean: true,
        files_changed: 0,
        staged_files: 0,
        unstaged_files: 0,
        untracked_files: 0,
        additions: 0,
        deletions: 0,
        binary_files: 0,
        source_changed: false,
        tests_changed: false,
        docs_only: true,
        risk_level: "low".into(),
        recommended_verification: "quick".into(),
        recommended_checks: vec![],
        summary: "fixture".into(),
        files: vec![],
        findings: vec![],
        probes: vec![],
        truncated: false,
    }
}

#[test]
fn reconciliation_and_verification_bind_identical_required_checks() {
    let (root, workspace, harness) = fixture();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn entry() {}\n").unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"proof-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    for level in ["low", "high"] {
        let mut review = clean_review();
        review.risk_level = level.into();
        let standalone = harness
            .verification_plan("proof", &workspace, &review)
            .unwrap();
        let reconciliation = harness
            .reconciliation_plan("proof", &workspace, &review)
            .unwrap();
        assert_eq!(
            standalone.revision,
            reconciliation.verification_plan.revision
        );
        assert!(!standalone
            .required_checks
            .as_ref()
            .expect("profile has checks")
            .is_empty());
        assert_eq!(
            standalone.required_checks, reconciliation.verification_plan.required_checks,
            "both entry points must bind the same exact required commands at {level} risk"
        );
    }
}

#[test]
fn observatory_reports_the_reconciliation_plan_that_claims_drift() {
    let (root, workspace, harness) = fixture();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn entry() {}\n").unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"proof-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    // A requirement whose only owner does not exist leaves the trace partial,
    // which is the drift a reconciliation plan has to claim.
    fs::write(
        root.path().join(".wcode/design/requirements.yaml"),
        r#"- schema_version: 1
  id: REQ-DRIFT
  title: Drifted requirement
  intent: Exercise an incomplete implementation trace.
  priority: high
  implemented_by:
    - component:missing
  acceptance:
    - AC-CHECK
  constraints: []
  risk: {}
"#,
    )
    .unwrap();
    fs::write(
        root.path().join(".wcode/design/components.yaml"),
        r#"- schema_version: 1
  id: component:feature
  name: Feature entry
  responsibilities:
    - expose the feature entry point
  depends_on: []
  constraints: []
  implementation: []
"#,
    )
    .unwrap();
    let mut review = clean_review();
    review.risk_level = "high".into();
    let plan = harness
        .reconciliation_plan("proof", &workspace, &review)
        .unwrap();
    assert!(
        !plan.drift_ids.is_empty(),
        "the fixture must produce a drift finding for the plan to claim"
    );
    let project = harness
        .project_observatory("proof", &workspace, Some(&review))
        .unwrap();
    let view = project
        .reconciliation
        .iter()
        .find(|item| item.id == plan.id)
        .expect("the declared plan must be observable");
    assert_eq!(view.drift_ids, plan.drift_ids);
    assert_eq!(
        view.implementation_tasks.len(),
        plan.implementation_tasks.len()
    );
    assert_ne!(view.implementation_tasks.len(), 0);
    let execution = project
        .reconciliation_execution
        .iter()
        .find(|status| status.execution.plan_id == plan.id)
        .expect("a plan that claims a current drift must carry its live execution state");
    assert_eq!(execution.execution.plan_id, plan.id);
}

#[test]
fn observatory_acceptance_checks_trace_each_required_check_to_its_evidence() {
    let (root, workspace, harness) = fixture();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn entry() {}\n").unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"proof-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    let review = clean_review();
    let plan = harness
        .verification_plan("proof", &workspace, &review)
        .unwrap();
    let required = plan
        .required_checks
        .as_ref()
        .expect("the profile declares checks")
        .clone();
    assert!(!required.is_empty());
    let revision = harness.intelligence.current_revision(&workspace).unwrap();
    let item = record(
        "EV-1",
        "verification:check-rust-test",
        1,
        EvidenceResult::Pass,
        &revision,
    );
    crate::evidence_store::persist(&workspace, &item).unwrap();
    let project = harness
        .project_observatory("proof", &workspace, None)
        .unwrap();
    let checks = &project.proof.acceptance.checks;
    assert_eq!(
        checks.len(),
        required.len(),
        "every required check stays observable: {checks:?}"
    );
    let view = checks
        .iter()
        .find(|check| check.id == "rust-test")
        .expect("the declared check must be observable");
    assert!(view.mapped, "the design declares this check");
    assert_eq!(view.execution, "executed");
    assert_eq!(view.outcome, "pass");
    assert_eq!(view.freshness, "current");
    assert_eq!(view.level.as_deref(), Some("full"));
    assert!(view.required_level.is_some());
    let unobserved = required
        .iter()
        .filter(|check| check.id != "rust-test")
        .flat_map(|check| checks.iter().find(|view| view.id == check.id))
        .next()
        .expect("a second required check must stay visible without evidence");
    assert_eq!(unobserved.execution, "unknown");
    assert_eq!(unobserved.outcome, "unknown");
    assert_eq!(unobserved.freshness, "missing");
}

#[test]
fn current_observatory_plans_require_both_code_and_design_revision() {
    let (root, workspace, harness) = fixture();
    let review = clean_review();
    harness
        .verification_plan("proof", &workspace, &review)
        .unwrap();
    let before = harness
        .project_observatory("proof", &workspace, None)
        .unwrap();
    assert_eq!(before.proof.current_verification_plans, 1);
    fs::write(
        root.path().join(".wcode/design/product.yaml"),
        "schema_version: 1\nid: product:demo\nname: Demo\nvision: Updated contract\n",
    )
    .unwrap();
    let after = harness
        .project_observatory("proof", &workspace, None)
        .unwrap();
    assert_eq!(after.proof.revision_code, before.proof.revision_code);
    assert_ne!(after.proof.revision_design, before.proof.revision_design);
    assert_eq!(after.proof.current_verification_plans, 0);
    assert_eq!(
        harness
            .verification_history("proof", &workspace, 100)
            .unwrap()
            .len(),
        1,
        "old plan remains in history"
    );
}
