use super::*;
use crate::evidence::{VerificationCheckReceipt, VerificationExecutionReceipt};
use crate::monitor::TaskMonitor;
use std::fs;

use crate::native_acceptance_fixture as native_fixture;

fn recall(
    workspace: &Workspace,
    query: &str,
    paths: &[String],
    limit: usize,
) -> Result<FailureMemoryRecall> {
    super::recall(workspace, query, paths, &HashSet::new(), limit)
}

fn fixture() -> (tempfile::TempDir, Workspace) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::create_dir(root.path().join(".wcode")).unwrap();
    fs::write(root.path().join("src/value.rs"), "pub fn value() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    (root, workspace)
}

fn revision(letter: char) -> Revision {
    Revision {
        code: format!("sha256:{}", letter.to_string().repeat(64)),
        design: Some(format!("sha256:{}", "d".repeat(64))),
    }
}

fn git(head: char) -> ExecutionGitBinding {
    ExecutionGitBinding {
        repository: format!("sha256:{}", "a".repeat(64)),
        head_sha: head.to_string().repeat(40),
        tree_sha: "b".repeat(40),
        dirty: false,
        index_fingerprint: format!("sha256:{}", "c".repeat(64)),
    }
}

fn proof(id: &str, result: EvidenceResult, timestamp: u64) -> Evidence {
    let check = RequiredVerificationCheck::from_command(
        "native-test",
        "node",
        &["--test".into(), "tests/value.js".into()],
        ".",
        ".",
    );
    let mut record = Evidence::new(
        id.into(),
        format!("change:{}", revision('a').code),
        EvidenceKind::Verification,
        "verify_project".into(),
        revision('a'),
        result,
        Confidence::Deterministic,
    )
    .unwrap();
    record.authority = EvidenceAuthority::NativeVerification;
    record.timestamp_ms = timestamp;
    record.execution_git_binding = Some(git('1'));
    record.execution_policy_binding = Some("native-policy:test".into());
    record.policy = Some("deterministic/full/v2".into());
    record.execution_receipt = Some(VerificationExecutionReceipt {
        schema_version: 1,
        level: "full".into(),
        required_checks: vec![check.clone()],
        checks: vec![VerificationCheckReceipt {
            check,
            result,
            execution: VerificationCheckExecution::Executed,
            reused_from: None,
        }],
        skipped_checks: Vec::new(),
        execution_git_binding: Some(git('1')),
    });
    record
}

fn observe(workspace: &Workspace, evidence: Evidence) -> FailureMemoryUpdate {
    observe_native_verification(
        workspace,
        &evidence.revision,
        evidence.execution_git_binding.as_ref(),
        evidence.execution_policy_binding.as_deref(),
        std::slice::from_ref(&evidence),
        &["src/value.rs".into()],
    )
    .unwrap()
}

#[tokio::test]
async fn failure_memory_links_real_native_check_recovery_across_restart_without_causal_claim() {
    let fixture = native_fixture::NativeFixture::new();
    let monitor = TaskMonitor::new([native_fixture::ID.to_owned()]);
    fs::write(
        fixture.root.path().join("src/value.js"),
        "exports.value = function value() { return 2; };\n",
    )
    .unwrap();
    native_fixture::git(fixture.root.path(), &["add", "src/value.js"]);
    native_fixture::commit(fixture.root.path(), "real failing check");
    let failed = fixture
        .harness
        .verify_project_mode(
            native_fixture::ID,
            &fixture.workspace,
            ("full", false),
            60,
            &monitor,
        )
        .await
        .unwrap();
    assert!(!failed.passed);
    assert!(failed
        .checks
        .iter()
        .any(|check| check.id == "node-test" && !check.success && check.exit_code.is_some()));
    let failed_revision = fixture
        .harness
        .current_revision(&fixture.workspace)
        .unwrap();
    let failed_records = crate::evidence_store::load(&fixture.workspace)
        .unwrap()
        .into_iter()
        .filter(|record| record.revision == failed_revision)
        .collect::<Vec<_>>();
    let failed_policy = failed_records
        .iter()
        .find(|record| record.kind == EvidenceKind::Verification)
        .unwrap()
        .execution_policy_binding
        .clone();
    observe_native_verification(
        &fixture.workspace,
        &failed_revision,
        failed.execution_git_binding.as_ref(),
        failed_policy.as_deref(),
        &failed_records,
        &["src/value.js".into()],
    )
    .unwrap();

    fs::write(
        fixture.root.path().join("src/value.js"),
        "exports.value = function value() { return 1; };\n",
    )
    .unwrap();
    native_fixture::git(fixture.root.path(), &["add", "src/value.js"]);
    native_fixture::commit(fixture.root.path(), "real repaired check");
    let passed = fixture
        .harness
        .verify_project_mode(
            native_fixture::ID,
            &fixture.workspace,
            ("full", false),
            60,
            &monitor,
        )
        .await
        .unwrap();
    assert!(passed.passed, "{passed:?}");
    let passed_revision = fixture
        .harness
        .current_revision(&fixture.workspace)
        .unwrap();
    let passed_records = crate::evidence_store::load(&fixture.workspace)
        .unwrap()
        .into_iter()
        .filter(|record| record.revision == passed_revision)
        .collect::<Vec<_>>();
    let passed_policy = passed_records
        .iter()
        .find(|record| record.kind == EvidenceKind::Verification)
        .unwrap()
        .execution_policy_binding
        .clone();
    assert_eq!(
        failed_policy, passed_policy,
        "this fixture does not activate a different Policy"
    );
    observe_native_verification(
        &fixture.workspace,
        &passed_revision,
        passed.execution_git_binding.as_ref(),
        passed_policy.as_deref(),
        &passed_records,
        &["src/value.js".into()],
    )
    .unwrap();
    let restarted = Workspace::new(fixture.root.path(), true, false).unwrap();
    let lessons = recall(&restarted, "node-test", &["src/value.js".into()], 8).unwrap();
    let recovered = lessons
        .items
        .iter()
        .find(|lesson| {
            lesson.status == "verified_same_check_recovery"
                && lesson
                    .check
                    .as_ref()
                    .is_some_and(|check| check.id == "node-test")
        })
        .unwrap();
    assert!(!recovered.cause_proven);
    assert!(failed_records
        .iter()
        .any(|record| Some(&record.id) == recovered.failure_evidence_id.as_ref()));
    assert!(passed_records
        .iter()
        .any(|record| Some(&record.id) == recovered.pass_evidence_id.as_ref()));
    assert_ne!(failed.execution_git_binding, passed.execution_git_binding);
    assert!(recovered.guidance.contains("does not prove the cause"));
    let history = load_records(&restarted).unwrap();
    let serialized = serde_json::to_string(&history).unwrap();
    assert!(
        !serialized.contains("exports.value")
            && !serialized.contains("stdout")
            && !serialized.contains("stderr")
    );
}

#[test]
fn failure_memory_native_recovery_requires_exact_signature_level_policy_and_current_bindings() {
    let variants: &[fn(&mut Evidence)] = &[
        |source| source.authority = EvidenceAuthority::SelfReported,
        |source| source.authority = EvidenceAuthority::LegacyUnknown,
        |source| source.producer = "self-reported:mcp:owner".into(),
        |source| source.execution_git_binding = None,
        |source| source.execution_policy_binding = Some("different-policy".into()),
        |source| {
            source.execution_receipt.as_mut().unwrap().level = "quick".into();
            source.policy = Some("deterministic/quick/v2".into());
        },
        |source| {
            let receipt = source.execution_receipt.as_mut().unwrap();
            let changed = RequiredVerificationCheck::from_command(
                "native-test",
                "node",
                &["--test".into(), "different.js".into()],
                ".",
                ".",
            );
            receipt.required_checks = vec![changed.clone()];
            receipt.checks[0].check = changed;
        },
        |source| {
            source.execution_receipt.as_mut().unwrap().checks[0].reused_from =
                Some("old-pass".into())
        },
        |source| {
            source.execution_receipt.as_mut().unwrap().checks[0].execution =
                VerificationCheckExecution::Unknown
        },
        |source| {
            source.kind = EvidenceKind::UnitTest;
            source.subject = "verification:native-test".into();
            source.producer = "node --test tests/value.js".into();
        },
    ];
    for mutate in variants {
        let (_root, workspace) = fixture();
        let timestamp = now_ms().saturating_sub(10);
        observe(&workspace, proof("failed", EvidenceResult::Fail, timestamp));
        let mut passed = proof("passed", EvidenceResult::Pass, timestamp + 1);
        mutate(&mut passed);
        let update = observe(&workspace, passed);
        assert_eq!(update.recoveries_written, 0);
        assert!(!recall(&workspace, "", &[], 8)
            .unwrap()
            .items
            .iter()
            .any(|lesson| lesson.status == "verified_same_check_recovery"));
    }
    let (_root, workspace) = fixture();
    let time = now_ms().saturating_sub(10);
    let failed = proof("failed", EvidenceResult::Fail, time);
    observe(&workspace, failed.clone());
    let pass = proof("passed", EvidenceResult::Pass, time + 1);
    assert_eq!(
        observe_native_verification(
            &workspace,
            &revision('b'),
            Some(&git('1')),
            pass.execution_policy_binding.as_deref(),
            std::slice::from_ref(&pass),
            &[]
        )
        .unwrap()
        .recoveries_written,
        0
    );
    assert_eq!(
        observe_native_verification(
            &workspace,
            &pass.revision,
            Some(&git('2')),
            pass.execution_policy_binding.as_deref(),
            std::slice::from_ref(&pass),
            &[]
        )
        .unwrap()
        .recoveries_written,
        0
    );
    assert_eq!(
        observe(&workspace, proof("same-time", EvidenceResult::Pass, time)).recoveries_written,
        0
    );
    let mut foreign = proof("foreign", EvidenceResult::Pass, time + 1);
    foreign.execution_git_binding.as_mut().unwrap().repository =
        format!("sha256:{}", "f".repeat(64));
    foreign
        .execution_receipt
        .as_mut()
        .unwrap()
        .execution_git_binding = foreign.execution_git_binding.clone();
    assert_eq!(observe(&workspace, foreign).recoveries_written, 0);
}

#[test]
fn failure_memory_unavailable_and_partial_checks_remain_unverified_candidates() {
    for execution in [
        VerificationCheckExecution::Unknown,
        VerificationCheckExecution::Unavailable,
        VerificationCheckExecution::TimedOut,
    ] {
        let (_root, workspace) = fixture();
        let time = now_ms().saturating_sub(10);
        let mut failed = proof("unknown", EvidenceResult::Inconclusive, time);
        failed.execution_receipt.as_mut().unwrap().checks[0].execution = execution;
        observe(&workspace, failed);
        assert_eq!(
            observe(&workspace, proof("passed", EvidenceResult::Pass, time + 1)).recoveries_written,
            0
        );
        let lessons = recall(&workspace, "", &[], 8).unwrap();
        assert_eq!(lessons.items[0].status, "unverified");
    }
    let (_root, workspace) = fixture();
    let time = now_ms().saturating_sub(10);
    observe(&workspace, proof("failed", EvidenceResult::Fail, time));
    let mut pass = proof("partial-pass", EvidenceResult::Pass, time + 1);
    let extra = RequiredVerificationCheck::from_command("not-executed", "node", &[], ".", ".");
    pass.execution_receipt
        .as_mut()
        .unwrap()
        .required_checks
        .push(extra);
    assert_eq!(observe(&workspace, pass).recoveries_written, 0);
}

#[test]
fn failure_memory_none_policy_is_exact_and_recovery_does_not_grant_acceptance() {
    let (_root, workspace) = fixture();
    let time = now_ms().saturating_sub(10);
    let mut failed = proof("failed", EvidenceResult::Fail, time);
    failed.execution_policy_binding = None;
    observe(&workspace, failed);
    let mut pass = proof("pass", EvidenceResult::Pass, time + 1);
    pass.execution_policy_binding = None;
    assert_eq!(observe(&workspace, pass.clone()).recoveries_written, 1);
    assert_eq!(observe(&workspace, pass).recoveries_written, 0);
    let lesson = recall(&workspace, "", &[], 8).unwrap().items.remove(0);
    assert_eq!(lesson.status, "verified_same_check_recovery");
    assert!(!lesson.cause_proven);
    assert!(lesson.guidance.contains("current Acceptance"));
}

#[test]
fn failure_memory_repository_rules_match_bounded_dimensions_without_retaining_error_text() {
    let (root, workspace) = fixture();
    fs::write(
        root.path().join(RULES_PATH),
        r#"schema_version: 1
rules:
  - id: module-source
    literal: narrow failure marker
    path: src
    guidance: Inspect the module boundary and rerun current checks.
  - id: native-test-rule
    check: native-test
    path: src/value.rs
    guidance: Consult the current check definition before changing code.
"#,
    )
    .unwrap();
    let event = EngineeringMilestone::new(
        "apply_edits",
        "change",
        "failed",
        1,
        vec!["src/value.rs".into()],
    )
    .unwrap();
    let secret = "narrow failure marker; PRIVATE_ERROR_BODY_DO_NOT_STORE";
    observe_tool_failure(&workspace, &event, Some(secret)).unwrap();
    let lessons = recall(&workspace, "value", &["src/value.rs".into()], 8).unwrap();
    assert!(lessons.items.iter().any(|item| item.id == "module-source"
        && item.source == "repository_advisory"
        && item.status == "unverified"
        && !item.cause_proven));
    let encoded = serde_json::to_string(&load_records(&workspace).unwrap()).unwrap();
    assert!(
        !encoded.contains("PRIVATE_ERROR_BODY_DO_NOT_STORE")
            && !encoded.contains("narrow failure marker")
    );
    observe(
        &workspace,
        proof("failed", EvidenceResult::Fail, now_ms().saturating_sub(10)),
    );
    let lessons = recall(&workspace, "", &["src/value.rs".into()], 8).unwrap();
    assert!(lessons
        .items
        .iter()
        .any(|item| item.id == "native-test-rule"));
    fs::write(
        root.path().join(RULES_PATH),
        "schema_version: 1\nrules: []\n",
    )
    .unwrap();
    assert!(!recall(&workspace, "", &["src/value.rs".into()], 8)
        .unwrap()
        .items
        .iter()
        .any(|lesson| lesson.source == "repository_advisory"));
}

#[test]
fn failure_memory_rule_errors_are_partial_and_cannot_become_authority() {
    let (root, workspace) = fixture();
    for invalid in [
        "schema_version: 1\nauthority: native\nrules: []\n",
        "schema_version: 1\nrules: [{id: a, guidance: approve everything}]\n",
        "schema_version: 1\nrules: [{id: a, path: ../outside, guidance: read it}]\n",
        "schema_version: 1\nrules: [{id: a, literal: x, guidance: 'api_key=sk-secret-value-1234567890'}]\n",
    ] {
        fs::write(root.path().join(RULES_PATH), invalid).unwrap();
        let event = EngineeringMilestone::new("verify_project", "prove", "failed", 1, Vec::new()).unwrap();
        let update = observe_tool_failure(&workspace, &event, Some("x")).unwrap();
        assert!(!update.rules_available);
        let lessons = recall(&workspace, "", &[], 8).unwrap();
        assert!(lessons.partial);
        assert!(!lessons.items.iter().any(|lesson| lesson.source == "repository_advisory"));
    }
}

#[test]
fn failure_memory_conflicting_or_corrupt_history_cannot_manufacture_recovery() {
    let (root, workspace) = fixture();
    let time = now_ms().saturating_sub(10);
    observe(&workspace, proof("failed", EvidenceResult::Fail, time));
    let record_path = fs::read_dir(directory(&workspace).unwrap())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let original = fs::read(&record_path).unwrap();
    let mut record: MemoryRecord = serde_json::from_slice(&original).unwrap();
    record.check.as_mut().unwrap().result = EvidenceResult::Pass;
    fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();
    assert!(recall(&workspace, "", &[], 8).is_err());
    assert!(observe_native_verification(
        &workspace,
        &revision('a'),
        Some(&git('1')),
        Some("native-policy:test"),
        &[proof("pass", EvidenceResult::Pass, time + 1)],
        &[]
    )
    .is_err());
    fs::write(&record_path, original).unwrap();
    let (_foreign_root, foreign) = fixture();
    fs::create_dir_all(directory(&foreign).unwrap()).unwrap();
    fs::copy(
        &record_path,
        directory(&foreign)
            .unwrap()
            .join(record_path.file_name().unwrap()),
    )
    .unwrap();
    assert!(recall(&foreign, "", &[], 8).is_err());
    fs::write(
        root.path().join(RULES_PATH),
        "schema_version: 1\nrules: []\n",
    )
    .unwrap();
}

#[test]
fn failure_memory_input_identity_conflict_and_future_time_do_not_teach_a_fix() {
    let (_root, workspace) = fixture();
    let time = now_ms().saturating_sub(10);
    let first = proof("same-id", EvidenceResult::Fail, time);
    let other = proof("same-id", EvidenceResult::Pass, time + 1);
    assert!(observe_native_verification(
        &workspace,
        &revision('a'),
        Some(&git('1')),
        Some("native-policy:test"),
        &[first, other],
        &[]
    )
    .is_err());
    let future = proof(
        "future",
        EvidenceResult::Fail,
        // Leave headroom for actual filesystem work before the production clock
        // is sampled; a +1ms fixture can become valid while the test is running.
        now_ms() + 2 * super::super::MAX_CLOCK_SKEW_MS,
    );
    assert_eq!(observe(&workspace, future).observations_written, 0);
    assert!(recall(&workspace, "", &[], 8).unwrap().items.is_empty());
}

#[test]
fn failure_memory_retained_history_is_bounded_and_omissions_are_explicit() {
    let (_root, workspace) = fixture();
    let _access = ACCESS.lock().unwrap();
    let mut history = Vec::new();
    let time = now_ms().saturating_sub(1000);
    for index in 0..MAX_RECORDS + 1 {
        let mut observation = proof(
            &format!("failure-{index}"),
            EvidenceResult::Fail,
            time + index as u64,
        );
        let check = RequiredVerificationCheck::from_command(
            &format!("check-{index}"),
            "node",
            &[],
            ".",
            ".",
        );
        observation.execution_receipt.as_mut().unwrap().checks[0].check = check.clone();
        observation
            .execution_receipt
            .as_mut()
            .unwrap()
            .required_checks = vec![check.clone()];
        let record = MemoryRecord {
            schema_version: VERSION,
            id: String::new(),
            root_digest: root_digest(&workspace).unwrap(),
            timestamp_ms: observation.timestamp_ms,
            event_id: None,
            tool: None,
            paths: Vec::new(),
            rules: Vec::new(),
            check: Some(CheckObservation {
                evidence_id: observation.id,
                check,
                level: "full".into(),
                revision: observation.revision,
                git: observation.execution_git_binding,
                policy_binding: observation.execution_policy_binding,
                result: EvidenceResult::Fail,
                execution: VerificationCheckExecution::Executed,
                native_complete: true,
            }),
            recovery: None,
        };
        append(&workspace, &mut history, record).unwrap();
    }
    assert_eq!(history.len(), MAX_RECORDS);
    assert_eq!(load_records(&workspace).unwrap().len(), MAX_RECORDS);
    drop(_access);
    let recall = recall(&workspace, "", &[], 2).unwrap();
    assert_eq!(recall.items.len(), 2);
    assert_eq!(recall.omitted, MAX_RECORDS - 2);
    assert!(recall.partial);
}

#[test]
fn failure_memory_journal_callback_and_transient_matching_deduplicate_one_observation() {
    let (root, workspace) = fixture();
    fs::write(
        root.path().join(RULES_PATH),
        r#"schema_version: 1
rules:
  - id: scoped-module
    path: src
    guidance: Inspect the selected source and current diagnostics.
  - id: literal-module
    literal: module limit
    path: src
    guidance: Keep the responsibility bounded and rerun verification.
"#,
    )
    .unwrap();
    let event = EngineeringMilestone::new(
        "apply_edits",
        "change",
        "failed",
        1,
        vec!["src/value.rs".into()],
    )
    .unwrap();
    for _ in 0..2 {
        crate::engineering_journal::persist_with_error(
            &workspace,
            &event,
            Some("module limit PRIVATE_ERROR"),
        )
        .unwrap();
    }
    let lessons = recall(&workspace, "", &["src/value.rs".into()], 8).unwrap();
    assert_eq!(
        lessons
            .items
            .iter()
            .find(|item| item.id == "scoped-module")
            .unwrap()
            .observations,
        1
    );
    assert_eq!(
        lessons
            .items
            .iter()
            .find(|item| item.id == "literal-module")
            .unwrap()
            .observations,
        1
    );
    assert!(lessons
        .items
        .iter()
        .all(|lesson| lesson.status == "unverified" && !lesson.cause_proven));
    let unrelated =
        EngineeringMilestone::new("apply_edits", "change", "failed", 1, Vec::new()).unwrap();
    observe_tool_failure(&workspace, &unrelated, Some("module limit")).unwrap();
    assert_eq!(
        recall(&workspace, "", &["src/value.rs".into()], 8)
            .unwrap()
            .items
            .iter()
            .find(|item| item.id == "literal-module")
            .unwrap()
            .observations,
        1
    );
}

#[test]
fn failure_memory_independent_executed_failure_survives_an_incomplete_project_report() {
    let (_root, workspace) = fixture();
    let time = now_ms().saturating_sub(10);
    let mut independent = proof("independent-fail", EvidenceResult::Fail, time);
    independent.kind = EvidenceKind::UnitTest;
    independent.subject = "verification:native-test".into();
    independent.producer = "node --test tests/value.js".into();
    observe(&workspace, independent);
    let mut incomplete = proof("partial-project", EvidenceResult::Inconclusive, time + 1);
    incomplete.execution_receipt.as_mut().unwrap().checks[0].result = EvidenceResult::Inconclusive;
    incomplete.execution_receipt.as_mut().unwrap().checks[0].execution =
        VerificationCheckExecution::Unavailable;
    observe(&workspace, incomplete);
    let mut pass = proof("full-pass", EvidenceResult::Pass, time + 2);
    pass.revision = revision('b');
    pass.subject = format!("change:{}", pass.revision.code);
    pass.execution_git_binding = Some(git('2'));
    pass.execution_receipt
        .as_mut()
        .unwrap()
        .execution_git_binding = pass.execution_git_binding.clone();
    assert_eq!(observe(&workspace, pass).recoveries_written, 1);
    let lesson = recall(&workspace, "", &[], 8).unwrap().items.remove(0);
    assert_eq!(
        lesson.failure_evidence_id.as_deref(),
        Some("independent-fail")
    );
    assert_eq!(lesson.pass_evidence_id.as_deref(), Some("full-pass"));
}

#[cfg(unix)]
#[test]
fn failure_memory_symlink_or_hardlink_history_is_unavailable() {
    use std::os::unix::fs::symlink;
    let (root, workspace) = fixture();
    let memory = directory(&workspace).unwrap();
    fs::create_dir_all(&memory).unwrap();
    let outside = root.path().join("outside");
    fs::write(&outside, "{}").unwrap();
    symlink(&outside, memory.join("record.json")).unwrap();
    assert!(recall(&workspace, "", &[], 8).is_err());
    fs::remove_file(memory.join("record.json")).unwrap();
    fs::hard_link(&outside, memory.join("record.json")).unwrap();
    assert!(recall(&workspace, "", &[], 8).is_err());
}
