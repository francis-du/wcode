use super::*;
use crate::evidence::{Confidence, EvidenceKind, EvidenceResult, Revision};

#[test]
fn evidence_signal_detects_late_records_and_removal_without_loading_json() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let empty = change_fingerprint(&workspace).unwrap();
    let mut record = Evidence::new(
        "EV-newer".into(),
        "check".into(),
        EvidenceKind::UnitTest,
        "test-runner".into(),
        Revision {
            code: "code:1".into(),
            design: None,
        },
        EvidenceResult::Pass,
        Confidence::Deterministic,
    )
    .unwrap();
    record.timestamp_ms = 100;
    persist(&workspace, &record).unwrap();
    LOAD_CALLS.with(|count| count.set(0));
    let before = change_fingerprint(&workspace).unwrap();
    assert_ne!(empty, before);
    assert_eq!(before, change_fingerprint(&workspace).unwrap());
    record.id = "EV-late".into();
    record.timestamp_ms = 1;
    persist(&workspace, &record).unwrap();
    let late = change_fingerprint(&workspace).unwrap();
    assert_ne!(before, late, "a backdated arrival still changes the set");
    assert_eq!(LOAD_CALLS.with(|count| count.get()), 0);
    let paths = evidence_paths(&evidence_directory(&workspace).unwrap()).unwrap();
    fs::remove_file(&paths[0]).unwrap();
    assert_ne!(late, change_fingerprint(&workspace).unwrap());
}

#[cfg(unix)]
#[test]
fn evidence_signal_rejects_symlink_records_without_following_them() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let directory = evidence_directory(&workspace).unwrap();
    fs::create_dir_all(&directory).unwrap();
    let destination = root.path().join("fixture.txt");
    fs::write(&destination, "not evidence").unwrap();
    std::os::unix::fs::symlink(&destination, directory.join("linked.json")).unwrap();
    assert!(change_fingerprint(&workspace).is_err());
    assert_eq!(fs::read_to_string(destination).unwrap(), "not evidence");
}

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

fn fixture_evidence(id: &str, code: &str, result: EvidenceResult) -> Evidence {
    let mut record = Evidence::new(
        id.into(),
        "check".into(),
        EvidenceKind::UnitTest,
        "native-test".into(),
        Revision {
            code: code.into(),
            design: None,
        },
        result,
        Confidence::Deterministic,
    )
    .unwrap();
    record.timestamp_ms = 1;
    record
}

#[test]
fn authoritative_evidence_load_rejects_bad_records_and_recovers_after_repair() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let baseline = fixture_evidence("EV-before", "code:1", EvidenceResult::Pass);
    persist(&workspace, &baseline).unwrap();
    let directory = evidence_directory(&workspace).unwrap();
    let bad = directory.join("00000000000000000002-broken.json");
    let mut unknown = serde_json::to_value(&baseline).unwrap();
    unknown["unexpected_approval"] = serde_json::json!(true);
    let mut invalid = serde_json::to_value(&baseline).unwrap();
    invalid["producer"] = serde_json::json!("");
    for bytes in [
        b"{".to_vec(),
        serde_json::to_vec(&unknown).unwrap(),
        serde_json::to_vec(&invalid).unwrap(),
        vec![b' '; MAX_EVIDENCE_BYTES as usize + 1],
    ] {
        fs::write(&bad, bytes).unwrap();
        assert!(
            load(&workspace).is_err(),
            "must not hide a bad record behind prior Pass"
        );
        assert!(load_recent(&workspace, 1).is_err());
    }
    fs::remove_file(&bad).unwrap();
    let mut failure = fixture_evidence("EV-after", "code:2", EvidenceResult::Fail);
    failure.timestamp_ms = 3;
    persist(&workspace, &failure).unwrap();
    let restarted = Workspace::new(root.path(), false, false).unwrap();
    assert_eq!(load(&restarted).unwrap(), vec![baseline, failure]);
    // An older broken record also blocks the complete ledger, despite a valid latest result.
    fs::write(directory.join("00000000000000000000-broken.json"), "{").unwrap();
    assert!(load(&restarted).is_err());
}

#[test]
fn evidence_enumeration_errors_and_bounds_are_not_complete_proof() {
    let errors = std::iter::once(Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "entry denied",
    )));
    assert!(collect_record_paths(errors).is_err());
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let directory = evidence_directory(&workspace).unwrap();
    fs::create_dir_all(&directory).unwrap();
    let record = fixture_evidence("EV-retained", "code:1", EvidenceResult::Pass);
    let bytes = serde_json::to_vec(&record).unwrap();
    for index in 0..MAX_STORED_EVIDENCE {
        fs::write(
            directory.join(format!("00000000000000000000-{index:05}.json")),
            &bytes,
        )
        .unwrap();
    }
    let mut next = fixture_evidence("EV-new-revision", "code:2", EvidenceResult::Fail);
    next.timestamp_ms = 2;
    persist(&workspace, &next).unwrap();
    assert_eq!(
        evidence_paths(&directory).unwrap().len(),
        MAX_STORED_EVIDENCE
    );
    assert!(
        load(&workspace).unwrap().contains(&next),
        "append then prune must work at capacity"
    );
    let overflow = directory.join("00000000000000000003-overflow.json");
    fs::write(&overflow, &bytes).unwrap();
    assert!(
        load(&workspace).is_err(),
        "a truncated ledger must not appear complete"
    );
    fs::write(directory.join("00000000000000000004-overflow.json"), &bytes).unwrap();
    assert!(
        evidence_paths(&directory).is_err(),
        "enumeration itself must remain bounded"
    );
}

#[cfg(unix)]
#[test]
fn authoritative_evidence_load_rejects_links_and_dangling_store_directory() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let record = fixture_evidence("EV-link", "code:1", EvidenceResult::Fail);
    persist(&workspace, &record).unwrap();
    let directory = evidence_directory(&workspace).unwrap();
    let target = root.path().join("record.json");
    fs::write(&target, serde_json::to_vec(&record).unwrap()).unwrap();
    let linked = directory.join("00000000000000000002-linked.json");
    std::os::unix::fs::symlink(&target, &linked).unwrap();
    assert!(load(&workspace).is_err());
    fs::remove_file(&target).unwrap();
    assert!(
        load(&workspace).is_err(),
        "dangling record must not be silently omitted"
    );
    fs::remove_dir_all(&directory).unwrap();
    std::os::unix::fs::symlink(root.path().join("missing"), &directory).unwrap();
    assert!(
        load(&workspace).is_err(),
        "dangling directory is not an empty ledger"
    );
}

#[test]
fn evidence_identity_conflicts_fail_closed_but_exact_duplicates_are_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let record = fixture_evidence("EV-one", "code:1", EvidenceResult::Pass);
    persist(&workspace, &record).unwrap();
    let directory = evidence_directory(&workspace).unwrap();
    fs::write(
        directory.join("exact-duplicate.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    assert_eq!(load(&workspace).unwrap(), vec![record.clone()]);
    let conflicting = directory.join("identity-conflict.json");
    for field in ["result", "revision", "producer"] {
        let mut changed = record.clone();
        match field {
            "result" => changed.result = EvidenceResult::Fail,
            "revision" => changed.revision.code = "code:2".into(),
            _ => changed.producer = "another-native-runner".into(),
        }
        fs::write(&conflicting, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(
            load(&workspace).is_err(),
            "same ID with different {field} cannot overwrite proof"
        );
    }
    fs::remove_file(conflicting).unwrap();
    assert_eq!(load(&workspace).unwrap(), vec![record]);
}

#[test]
fn advisory_append_cannot_evict_current_native_failure_across_restart() {
    use crate::verification::{ReviewVerdict, StageSubmission, VerificationPlanBinding};
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let runtime = crate::intelligence::SoftwareIntelligenceRuntime::default();
    let revision = runtime.current_revision(&workspace).unwrap();
    let mut state = crate::verification::VerificationState::default();
    let plan = state
        .create_plan(
            "VP-retention".into(),
            "demo".into(),
            format!("change:{}", revision.code),
            VerificationPlanBinding {
                revision: revision.clone(),
                required_checks: None,
                stage_targets: vec!["language:rust".into()],
                automation_gaps: vec![],
            },
            crate::risk::RiskLevel::Medium,
            ["VJ-retention-a".into(), "VJ-retention-b".into()].into_iter(),
        )
        .unwrap();
    crate::verification_store::persist(&workspace, &state).unwrap();
    let mut failure = fixture_evidence("EV-current-failure", &revision.code, EvidenceResult::Fail);
    failure.revision = revision.clone();
    failure.subject = plan.subject.clone();
    failure.kind = EvidenceKind::Property;
    failure.authority = crate::evidence::EvidenceAuthority::NativeStage;
    failure.producer = "executor:native-a".into();
    failure.targets = plan.stage_targets.clone();
    persist(&workspace, &failure).unwrap();
    let mut pass = failure.clone();
    pass.id = "EV-independent-pass".into();
    pass.result = EvidenceResult::Pass;
    pass.producer = "executor:native-b".into();
    pass.timestamp_ms = 2;
    persist(&workspace, &pass).unwrap();
    let directory = evidence_directory(&workspace).unwrap();
    let mut advisory = failure.clone();
    advisory.authority = crate::evidence::EvidenceAuthority::SelfReported;
    advisory.producer = "self-reported:mcp:owner-a".into();
    advisory.result = EvidenceResult::Inconclusive;
    advisory.confidence = Confidence::Low;
    for index in 0..MAX_STORED_EVIDENCE - 2 {
        advisory.id = format!("EV-advisory-{index}");
        advisory.timestamp_ms = index as u64 + 3;
        fs::write(
            directory.join(format!("{:020}-advisory.json", advisory.timestamp_ms)),
            serde_json::to_vec(&advisory).unwrap(),
        )
        .unwrap();
    }
    for index in 0..2 {
        runtime
            .verification_stage_submit(
                "demo",
                &workspace,
                &plan.id,
                StageSubmission {
                    stage: crate::verification::VerificationStage::Property,
                    producer: "self-reported:mcp:owner-a".into(),
                    verdict: ReviewVerdict::Inconclusive,
                    summary: format!("advisory flood {index}"),
                    artifact_digest: "sha256:advisory".into(),
                    targets: plan.stage_targets.clone(),
                    model: None,
                },
            )
            .unwrap();
    }
    assert_eq!(
        evidence_paths(&directory).unwrap().len(),
        MAX_STORED_EVIDENCE
    );
    let restarted = crate::intelligence::SoftwareIntelligenceRuntime::default();
    let status = restarted
        .verification_status_for_revision("demo", &workspace, &revision)
        .unwrap()
        .unwrap();
    assert_eq!(
        status.stage_producer_results["property"]["executor:native-a"],
        EvidenceResult::Fail
    );
    assert_eq!(
        status.stage_producer_results["property"]["executor:native-b"],
        EvidenceResult::Pass
    );
    assert_eq!(
        status.stage_target_results["property"]["language:rust"],
        EvidenceResult::Fail
    );
    assert!(status
        .blockers
        .iter()
        .any(|blocker| blocker == "property-evidence-failed"));
    assert!(!status.ready);
    // A genuine new native revision makes prior negatives eligible for retention pruning.
    fs::write(root.path().join("new_revision.txt"), "changed").unwrap();
    let next_revision = restarted.current_revision(&workspace).unwrap();
    assert_ne!(revision, next_revision);
    let mut next = advisory.clone();
    next.id = "EV-new-native-revision".into();
    next.timestamp_ms = u64::MAX - 1;
    next.revision = next_revision;
    persist(&workspace, &next).unwrap();
    assert!(!load(&workspace)
        .unwrap()
        .iter()
        .any(|record| record.id == failure.id));
}

#[test]
fn protected_capacity_rejects_append_without_losing_current_negative_proof() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let directory = evidence_directory(&workspace).unwrap();
    fs::create_dir_all(&directory).unwrap();
    let mut record = fixture_evidence("EV-negative", "code:1", EvidenceResult::Fail);
    for index in 0..MAX_STORED_EVIDENCE {
        record.id = format!("EV-native-negative-{index}");
        record.subject = format!("check:{index}");
        record.policy = None;
        record.execution_receipt = None;
        record.timestamp_ms = index as u64 + 1;
        record.result = match index % 3 {
            0 => EvidenceResult::Fail,
            1 => EvidenceResult::Disagree,
            _ => EvidenceResult::Inconclusive,
        };
        if index == 0 {
            use crate::evidence::{
                RequiredVerificationCheck, VerificationCheckReceipt, VerificationExecutionReceipt,
            };
            let checks = ["a", "b"]
                .into_iter()
                .map(|id| VerificationCheckReceipt {
                    execution: crate::evidence::VerificationCheckExecution::Executed,
                    check: RequiredVerificationCheck::from_command(
                        id,
                        "cargo",
                        &[],
                        ".",
                        "workspace",
                    ),
                    result: if id == "a" {
                        EvidenceResult::Pass
                    } else {
                        EvidenceResult::Fail
                    },
                    reused_from: None,
                })
                .collect::<Vec<_>>();
            record.policy = Some("deterministic/full/v2".into());
            record.execution_receipt = Some(VerificationExecutionReceipt {
                execution_git_binding: None,
                schema_version: 1,
                level: "full".into(),
                required_checks: checks.iter().map(|check| check.check.clone()).collect(),
                checks,
                skipped_checks: vec![],
            });
        }
        fs::write(
            directory.join(format!("{:020}-negative.json", record.timestamp_ms)),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
    }
    let mut incoming = record.clone();
    incoming.id = "EV-rejected-advisory".into();
    incoming.producer = "self-reported:mcp:owner-a".into();
    incoming.timestamp_ms = u64::MAX;
    assert!(persist(&workspace, &incoming).is_err());
    let retained = load(&workspace).unwrap();
    assert_eq!(retained.len(), MAX_STORED_EVIDENCE);
    assert!(!retained.iter().any(|record| record.id == incoming.id));
    assert!(retained
        .iter()
        .all(|record| !record.producer.starts_with("self-reported:mcp:")));
    let broad = retained
        .iter()
        .find(|record| record.subject == "check:0")
        .unwrap();
    let mut narrow = broad.clone();
    narrow.id = "EV-native-narrow-pass".into();
    narrow.timestamp_ms = u64::MAX - 2;
    narrow.result = EvidenceResult::Pass;
    let receipt = narrow.execution_receipt.as_mut().unwrap();
    receipt.required_checks.retain(|check| check.id == "a");
    receipt.checks.retain(|check| check.check.id == "a");
    assert!(
        persist(&workspace, &narrow).is_err(),
        "a narrow Pass cannot release broad negative proof"
    );
    assert!(load(&workspace)
        .unwrap()
        .iter()
        .any(|record| record.id == broad.id));
    let mut full = broad.clone();
    full.id = "EV-native-full-retry".into();
    full.timestamp_ms = u64::MAX - 1;
    full.result = EvidenceResult::Pass;
    for check in &mut full.execution_receipt.as_mut().unwrap().checks {
        check.result = EvidenceResult::Pass;
    }
    persist(&workspace, &full).unwrap();
    let repaired = load(&workspace).unwrap();
    assert_eq!(repaired.len(), MAX_STORED_EVIDENCE);
    assert!(!repaired.iter().any(|record| record.id == broad.id));
    assert!(repaired.iter().any(|record| record.id == full.id));
}

#[test]
fn utf8_workspace_state_identity_preserves_legacy_evidence_and_journal() {
    use crate::engineering_journal::{load_recent as load_journal, EngineeringMilestone};
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let legacy_key = format!(
        "{:x}",
        Sha256::digest(workspace.root().to_str().unwrap().as_bytes())
    );
    let legacy_directory = state_root().unwrap().join(STORE_VERSION).join(legacy_key);
    assert_eq!(
        workspace_state_directory(&workspace).unwrap(),
        legacy_directory
    );

    let record = fixture_evidence("EV-legacy-utf8", "code:legacy", EvidenceResult::Fail);
    fs::create_dir_all(legacy_directory.join("evidence")).unwrap();
    fs::write(
        legacy_directory.join("evidence/legacy.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    assert_eq!(load(&workspace).unwrap(), vec![record]);

    let mut event =
        EngineeringMilestone::new("apply_edits", "change", "failed", 1, Vec::new()).unwrap();
    event.event_id = None;
    event.timestamp_ms = 123;
    let bytes = serde_json::to_vec(&event).unwrap();
    let legacy_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(legacy_json.get("failure_codes").is_none());
    let filename = format!(
        "{:020}-{}.json",
        event.timestamp_ms,
        &digest_bytes(&bytes)[..24]
    );
    fs::create_dir_all(legacy_directory.join("engineering-journal")).unwrap();
    fs::write(
        legacy_directory.join("engineering-journal").join(filename),
        bytes,
    )
    .unwrap();
    assert_eq!(load_journal(&workspace, 4).unwrap().records, vec![event]);
}

#[cfg(unix)]
#[test]
fn non_utf8_workspace_root_keys_do_not_use_lossy_identity() {
    use std::os::unix::ffi::OsStringExt;
    let first = PathBuf::from(std::ffi::OsString::from_vec(
        b"/tmp/wcode-root-\xff".to_vec(),
    ));
    let second = PathBuf::from(std::ffi::OsString::from_vec(
        b"/tmp/wcode-root-\xfe".to_vec(),
    ));
    assert_ne!(first, second);
    assert!(first.to_str().is_none());
    assert_eq!(first.to_string_lossy(), second.to_string_lossy());
    assert_ne!(
        workspace_root_key(&first).unwrap(),
        workspace_root_key(&second).unwrap()
    );
    let legacy_key = format!("{:x}", Sha256::digest(first.to_string_lossy().as_bytes()));
    assert_ne!(workspace_root_key(&first).unwrap(), legacy_key);
    assert_ne!(workspace_root_key(&second).unwrap(), legacy_key);
}

#[cfg(windows)]
#[test]
fn non_unicode_windows_workspace_root_keys_do_not_use_lossy_identity() {
    use std::os::windows::ffi::OsStringExt;
    let prefix = r"C:\wcode-root-".encode_utf16().collect::<Vec<_>>();
    let first = PathBuf::from(std::ffi::OsString::from_wide(
        &[prefix.as_slice(), &[0xd800]].concat(),
    ));
    let second = PathBuf::from(std::ffi::OsString::from_wide(
        &[prefix.as_slice(), &[0xd801]].concat(),
    ));
    assert_ne!(first, second);
    assert!(first.to_str().is_none());
    assert_eq!(first.to_string_lossy(), second.to_string_lossy());
    assert_ne!(
        workspace_root_key(&first).unwrap(),
        workspace_root_key(&second).unwrap()
    );
    let legacy_key = format!("{:x}", Sha256::digest(first.to_string_lossy().as_bytes()));
    assert_ne!(workspace_root_key(&first).unwrap(), legacy_key);
    assert_ne!(workspace_root_key(&second).unwrap(), legacy_key);
}

#[cfg(unix)]
#[test]
fn non_utf8_real_workspace_stores_isolate_evidence_and_journal_without_legacy_fallback() {
    use crate::engineering_journal::{
        load_recent as load_journal, persist as persist_journal, EngineeringFailureCode,
        EngineeringMilestone,
    };
    use std::os::unix::ffi::OsStringExt;
    let parent = tempfile::tempdir().unwrap();
    let mut workspaces = Vec::new();
    for byte in [0xff, 0xfe] {
        let path = parent
            .path()
            .join(std::ffi::OsString::from_vec(vec![b'r', b'-', byte]));
        match fs::create_dir(&path) {
            Ok(()) => {}
            #[cfg(target_os = "macos")]
            Err(error) if error.raw_os_error() == Some(libc::EILSEQ) => {
                eprintln!(
                    "not exercised: this macOS filesystem rejects non-UTF-8 directories (EILSEQ)"
                );
                return;
            }
            Err(error) => panic!("cannot create non-UTF-8 Workspace fixture: {error}"),
        }
        workspaces.push(Workspace::new(&path, false, false).unwrap());
    }
    let second = workspaces.pop().unwrap();
    let first = workspaces.pop().unwrap();
    assert_ne!(first.root(), second.root());
    assert!(first.root().to_str().is_none());
    assert_eq!(
        first.root().to_string_lossy(),
        second.root().to_string_lossy()
    );
    assert_ne!(
        workspace_state_directory(&first).unwrap(),
        workspace_state_directory(&second).unwrap()
    );

    let legacy_key = format!(
        "{:x}",
        Sha256::digest(first.root().to_string_lossy().as_bytes())
    );
    let legacy_directory = state_root().unwrap().join(STORE_VERSION).join(legacy_key);
    assert_ne!(workspace_state_directory(&first).unwrap(), legacy_directory);
    assert_ne!(
        workspace_state_directory(&second).unwrap(),
        legacy_directory
    );
    let legacy_record = fixture_evidence("EV-lossy-shared", "code:legacy", EvidenceResult::Fail);
    fs::create_dir_all(legacy_directory.join("evidence")).unwrap();
    fs::write(
        legacy_directory.join("evidence/shared.json"),
        serde_json::to_vec(&legacy_record).unwrap(),
    )
    .unwrap();
    let mut legacy_event =
        EngineeringMilestone::new("apply_edits", "change", "failed", 1, Vec::new()).unwrap();
    legacy_event.failure_codes = vec![EngineeringFailureCode::ProtectedPath];
    let bytes = serde_json::to_vec(&legacy_event).unwrap();
    let filename = format!(
        "{:020}-{}.json",
        legacy_event.timestamp_ms,
        &digest_bytes(&bytes)[..24]
    );
    fs::create_dir_all(legacy_directory.join("engineering-journal")).unwrap();
    fs::write(
        legacy_directory.join("engineering-journal").join(filename),
        bytes,
    )
    .unwrap();
    assert!(load(&first).unwrap().is_empty());
    assert!(load(&second).unwrap().is_empty());
    assert!(load_journal(&first, 4).unwrap().records.is_empty());
    assert!(load_journal(&second, 4).unwrap().records.is_empty());

    let first_record = fixture_evidence("EV-first-root", "code:first", EvidenceResult::Pass);
    let second_record = fixture_evidence("EV-second-root", "code:second", EvidenceResult::Fail);
    persist(&first, &first_record).unwrap();
    assert!(load(&second).unwrap().is_empty());
    persist(&second, &second_record).unwrap();
    assert_eq!(load(&first).unwrap(), vec![first_record]);
    assert_eq!(load(&second).unwrap(), vec![second_record]);
    let mut first_event =
        EngineeringMilestone::new("apply_edits", "change", "failed", 1, Vec::new()).unwrap();
    first_event.failure_codes = vec![EngineeringFailureCode::ShaMismatch];
    persist_journal(&first, &first_event).unwrap();
    assert!(load_journal(&second, 4).unwrap().records.is_empty());
    let mut second_event = first_event.clone();
    second_event.event_id = Some(uuid::Uuid::new_v4().to_string());
    second_event.failure_codes = vec![EngineeringFailureCode::AuthorizationRequired];
    persist_journal(&second, &second_event).unwrap();
    assert_eq!(load_journal(&first, 4).unwrap().records, vec![first_event]);
    assert_eq!(
        load_journal(&second, 4).unwrap().records,
        vec![second_event]
    );
}
