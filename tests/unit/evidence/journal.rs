use crate::engineering_journal::{change_fingerprint, load_recent, persist, EngineeringMilestone};

#[test]
fn engineering_journal_access_is_shared_by_canonical_workspace_aliases() {
    let root = tempfile::tempdir().unwrap();
    let first = Workspace::new(root.path(), true, false).unwrap();
    let second = Workspace::new(root.path().join("."), true, false).unwrap();
    let first_gate = super::journal_access(&first).unwrap();
    let second_gate = super::journal_access(&second).unwrap();
    assert!(std::sync::Arc::ptr_eq(&first_gate, &second_gate));
    let _guard = first_gate.lock().unwrap();
    assert!(second_gate.try_lock().is_err());
}
use crate::workspace::Workspace;
use std::fs;

#[test]
fn engineering_journal_observation_identity_is_unique_bounded_and_backward_compatible() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let mut first =
        EngineeringMilestone::new("verify_project", "prove", "succeeded", 1, Vec::new()).unwrap();
    first.observed_revision = Some(crate::evidence::Revision {
        code: format!("sha256:{}", "a".repeat(64)),
        design: Some(format!("sha256:{}", "b".repeat(64))),
    });
    let mut second = first.clone();
    second.event_id = Some(uuid::Uuid::new_v4().to_string());
    persist(&workspace, &first).unwrap();
    persist(&workspace, &second).unwrap();
    let history = load_recent(&workspace, 4).unwrap();
    assert_eq!(history.records.len(), 2);
    assert!(history
        .records
        .iter()
        .all(|event| event.observed_revision == first.observed_revision));
    let mut legacy = serde_json::to_value(&first).unwrap();
    legacy.as_object_mut().unwrap().remove("event_id");
    legacy.as_object_mut().unwrap().remove("observed_revision");
    let legacy: EngineeringMilestone = serde_json::from_value(legacy).unwrap();
    assert!(legacy.event_id.is_none());
    assert!(legacy.observed_revision.is_none());
    persist(&workspace, &legacy).unwrap();
    second.event_id = Some("not-a-uuid".into());
    assert!(persist(&workspace, &second).is_err());
    first.observed_revision.as_mut().unwrap().code = "arbitrary untrusted text".into();
    assert!(persist(&workspace, &first).is_err());
}

#[test]
fn engineering_journal_skipped_corrupt_records_mark_partial_coverage() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let event =
        EngineeringMilestone::new("verify_project", "prove", "succeeded", 1, Vec::new()).unwrap();
    persist(&workspace, &event).unwrap();
    let directory = crate::evidence_store::workspace_state_directory(&workspace)
        .unwrap()
        .join("engineering-journal");
    fs::write(directory.join("invalid.json"), "{broken").unwrap();
    let history = load_recent(&workspace, 4).unwrap();
    assert_eq!(history.retained_records, 2);
    assert_eq!(history.records.len(), 1);
    assert!(history.truncated);
}

#[test]
fn engineering_journal_persists_only_bounded_structured_milestones() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn value() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let mut milestone = EngineeringMilestone::new(
        "apply_edits",
        "change",
        "succeeded",
        12,
        vec!["src/lib.rs".into()],
    )
    .unwrap();
    milestone.verification_level = None;
    persist(&workspace, &milestone).unwrap();

    let history = load_recent(&workspace, 16).unwrap();
    assert_eq!(history.retained_records, 1);
    assert!(!history.truncated);
    assert!(history.chain_valid);
    assert_eq!(history.records.len(), 1);
    let stored = &history.records[0];
    assert_eq!(stored.event_id, milestone.event_id);
    assert_eq!(stored.tool, milestone.tool);
    assert_eq!(stored.stage, milestone.stage);
    assert_eq!(stored.outcome, milestone.outcome);
    assert_eq!(stored.duration_ms, milestone.duration_ms);
    assert_eq!(stored.paths, milestone.paths);
    assert_eq!(stored.actor.as_deref(), Some("unknown"));
    assert!(stored.chain_previous.is_none());
    assert!(stored.record_hash.is_some());

    let encoded = serde_json::to_string(stored).unwrap();
    for forbidden in [
        "prompt",
        "query",
        "chain_of_thought",
        "arguments",
        "source",
        "old_text",
        "new_text",
    ] {
        assert!(!encoded.contains(forbidden));
    }
}

#[test]
fn engineering_journal_keeps_prompt_free_context_trajectory_metadata() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn value() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let mut milestone = EngineeringMilestone::new(
        "agent_context",
        "understand",
        "succeeded",
        7,
        vec!["src/lib.rs".into()],
    )
    .unwrap();
    milestone.retrieval_intent = Some("trace_to_code".into());
    persist(&workspace, &milestone).unwrap();
    let history = load_recent(&workspace, 4).unwrap();
    assert_eq!(
        history.records[0].retrieval_intent.as_deref(),
        Some("trace_to_code")
    );
    let encoded = serde_json::to_string(&history.records[0]).unwrap();
    assert!(!encoded.contains("prompt"));
    assert!(!encoded.contains("query"));
    assert!(EngineeringMilestone::new(
        "agent_context",
        "understand",
        "succeeded",
        7,
        vec!["src/lib.rs".into()],
    )
    .is_ok());
}

#[test]
fn engineering_journal_rejects_escaping_paths_and_fingerprints_metadata() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    assert!(EngineeringMilestone::new(
        "apply_edits",
        "change",
        "succeeded",
        1,
        vec!["../outside.rs".into()],
    )
    .is_err());
    let before = change_fingerprint(&workspace).unwrap();
    let milestone =
        EngineeringMilestone::new("verify_project", "prove", "succeeded", 42, Vec::new()).unwrap();
    persist(&workspace, &milestone).unwrap();
    let after = change_fingerprint(&workspace).unwrap();
    assert_ne!(before, after);
}

#[test]
fn engineering_journal_failure_codes_are_unique_bounded_and_legacy_compatible() {
    use crate::engineering_journal::EngineeringFailureCode::*;
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let mut event =
        EngineeringMilestone::new("apply_edits", "change", "failed", 1, Vec::new()).unwrap();
    event.failure_codes = vec![ShaMismatch, AuthorizationRequired];
    persist(&workspace, &event).unwrap();
    let history = load_recent(&workspace, 4).unwrap();
    assert_eq!(history.records[0].failure_codes, event.failure_codes);
    let mut legacy = serde_json::to_value(&event).unwrap();
    legacy.as_object_mut().unwrap().remove("failure_codes");
    assert!(serde_json::from_value::<EngineeringMilestone>(legacy)
        .unwrap()
        .failure_codes
        .is_empty());
    event.failure_codes = vec![ShaMismatch, ShaMismatch];
    assert!(persist(&workspace, &event).is_err());
    event.failure_codes = vec![ShaMismatch];
    event.outcome = "succeeded".into();
    assert!(persist(&workspace, &event).is_err());
    let mut invalid = serde_json::to_value(&history.records[0]).unwrap();
    invalid["failure_codes"] = serde_json::json!(["arbitrary_private_diagnostic"]);
    assert!(serde_json::from_value::<EngineeringMilestone>(invalid).is_err());
}

#[test]
fn engineering_journal_copied_and_renamed_records_do_not_amplify_recurrence() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let mut first =
        EngineeringMilestone::new("apply_edits", "change", "failed", 1, Vec::new()).unwrap();
    first.failure_codes = vec![crate::engineering_journal::EngineeringFailureCode::ShaMismatch];
    persist(&workspace, &first).unwrap();
    let directory = crate::evidence_store::workspace_state_directory(&workspace)
        .unwrap()
        .join("engineering-journal");
    let original = fs::read_dir(&directory)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::copy(
        &original,
        directory.join("99999999999999999999-copied.json"),
    )
    .unwrap();
    fs::copy(
        &original,
        directory.join("99999999999999999998-renamed.json"),
    )
    .unwrap();
    let mut second = first.clone();
    second.event_id = Some(uuid::Uuid::new_v4().to_string());
    persist(&workspace, &second).unwrap();
    let history = load_recent(&workspace, 8).unwrap();
    assert_eq!(history.retained_records, 4);
    assert_eq!(history.records.len(), 2);
    assert_ne!(history.records[0].event_id, history.records[1].event_id);
    assert!(history.truncated);
}

#[test]
fn engineering_journal_payload_digest_rejects_modified_record_under_original_name() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let event =
        EngineeringMilestone::new("run_command", "understand", "failed", 1, Vec::new()).unwrap();
    persist(&workspace, &event).unwrap();
    let directory = crate::evidence_store::workspace_state_directory(&workspace)
        .unwrap()
        .join("engineering-journal");
    let path = fs::read_dir(directory)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut bytes = fs::read(&path).unwrap();
    bytes.push(b' ');
    fs::write(path, bytes).unwrap();
    let history = load_recent(&workspace, 4).unwrap();
    assert_eq!(history.retained_records, 1);
    assert!(history.records.is_empty());
    assert!(history.truncated);
}

#[cfg(unix)]
#[test]
fn engineering_journal_hardlinks_are_rejected_and_mark_partial() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let event =
        EngineeringMilestone::new("run_command", "understand", "failed", 1, Vec::new()).unwrap();
    persist(&workspace, &event).unwrap();
    let directory = crate::evidence_store::workspace_state_directory(&workspace)
        .unwrap()
        .join("engineering-journal");
    let path = fs::read_dir(&directory)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::hard_link(path, directory.join("hardlinked.json")).unwrap();
    let history = load_recent(&workspace, 4).unwrap();
    assert_eq!(history.retained_records, 2);
    assert!(history.records.is_empty());
    assert!(history.truncated);
}

#[test]
fn engineering_journal_failure_anchors_survive_513_future_success_events() {
    use crate::engineering_journal::EngineeringFailureCode::*;
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let mut expected = std::collections::BTreeSet::new();
    let mut oldest_sha = None;
    for (index, code) in [
        ShaMismatch,
        ShaMismatch,
        ShaMismatch,
        AuthorizationRequired,
        AuthorizationRequired,
    ]
    .into_iter()
    .enumerate()
    {
        let mut event =
            EngineeringMilestone::new("apply_edits", "change", "failed", 1, Vec::new()).unwrap();
        event.timestamp_ms = 100 + index as u64;
        event.failure_codes = vec![code];
        if index == 0 {
            oldest_sha = event.event_id.clone();
        } else {
            expected.insert(event.event_id.clone().unwrap());
        }
        persist(&workspace, &event).unwrap();
    }
    for index in 0..513 {
        let mut event =
            EngineeringMilestone::new("agent_context", "understand", "succeeded", 1, Vec::new())
                .unwrap();
        event.timestamp_ms = 1_000_000 + index;
        persist(&workspace, &event).unwrap();
    }
    let history = load_recent(&workspace, 512).unwrap();
    assert_eq!(history.retained_records, 512);
    assert_eq!(history.records.len(), 512);
    assert!(!history.truncated);
    let anchors = history
        .records
        .iter()
        .filter(|event| !event.failure_codes.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(anchors.len(), 4);
    assert_eq!(
        anchors
            .iter()
            .map(|event| event.event_id.clone().unwrap())
            .collect::<std::collections::BTreeSet<_>>(),
        expected
    );
    assert!(!history
        .records
        .iter()
        .any(|event| event.event_id == oldest_sha));
    assert_eq!(
        anchors
            .iter()
            .filter(|event| event.failure_codes == [ShaMismatch])
            .count(),
        2
    );
    assert_eq!(
        anchors
            .iter()
            .filter(|event| event.failure_codes == [AuthorizationRequired])
            .count(),
        2
    );
}

#[test]
fn engineering_journal_future_records_are_partial_without_rejecting_append() {
    use crate::engineering_journal::EngineeringFailureCode::*;
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let mut current =
        EngineeringMilestone::new("apply_edits", "change", "failed", 1, Vec::new()).unwrap();
    current.failure_codes = vec![ShaMismatch];
    let mut near = current.clone();
    near.event_id = Some(uuid::Uuid::new_v4().to_string());
    near.timestamp_ms = super::now_ms().saturating_add(60_000);
    near.failure_codes = vec![AuthorizationRequired];
    let mut distant = current.clone();
    distant.event_id = Some(uuid::Uuid::new_v4().to_string());
    distant.timestamp_ms = u64::MAX;
    distant.failure_codes = vec![ProtectedPath];
    for event in [&current, &near, &distant] {
        persist(&workspace, event).unwrap();
    }
    let history = load_recent(&workspace, 4).unwrap();
    assert_eq!(history.retained_records, 3);
    assert_eq!(history.records.len(), 2);
    assert!(history.truncated);
    assert!(history
        .records
        .iter()
        .any(|event| event.event_id == current.event_id));
    assert!(history
        .records
        .iter()
        .any(|event| event.event_id == near.event_id));
    assert!(!history
        .records
        .iter()
        .any(|event| event.event_id == distant.event_id));
}

#[test]
fn engineering_journal_entry_errors_propagate_and_directory_scan_is_bounded() {
    let error = std::iter::once(Err::<fs::DirEntry, _>(std::io::Error::other(
        "fixture entry IO",
    )));
    assert!(super::collect_journal_paths(error, 4).is_err());
    let root = tempfile::tempdir().unwrap();
    for index in 0..4 {
        fs::write(root.path().join(format!("{index}.json")), b"{}").unwrap();
    }
    let seen = std::cell::Cell::new(0);
    let entries = fs::read_dir(root.path())
        .unwrap()
        .inspect(|_| seen.set(seen.get() + 1));
    assert!(super::collect_journal_paths(entries, 2).is_err());
    assert_eq!(
        seen.get(),
        3,
        "stop at the first entry beyond the explicit bound"
    );
}

#[test]
fn engineering_journal_concurrent_writers_at_capacity_keep_both_failures() {
    use crate::engineering_journal::EngineeringFailureCode::*;
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    for index in 0..512 {
        let mut event =
            EngineeringMilestone::new("agent_context", "understand", "succeeded", 1, Vec::new())
                .unwrap();
        event.timestamp_ms = 100 + index;
        persist(&workspace, &event).unwrap();
    }
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let handles = [ShaMismatch, AuthorizationRequired]
        .into_iter()
        .map(|code| {
            let path = root.path().to_owned();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let workspace = Workspace::new(path, true, false).unwrap();
                let mut event =
                    EngineeringMilestone::new("apply_edits", "change", "failed", 1, Vec::new())
                        .unwrap();
                event.failure_codes = vec![code];
                let id = event.event_id.clone().unwrap();
                barrier.wait();
                persist(&workspace, &event).unwrap();
                id
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let ids = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    let history = load_recent(&workspace, 512).unwrap();
    assert_eq!(history.retained_records, 512);
    assert_eq!(history.records.len(), 512);
    assert!(!history.truncated);
    let failures = history
        .records
        .iter()
        .filter(|event| !event.failure_codes.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 2);
    assert_eq!(
        failures
            .into_iter()
            .map(|event| event.event_id.clone().unwrap())
            .collect::<std::collections::BTreeSet<_>>(),
        ids
    );
}

#[test]
fn engineering_journal_bounded_old_overflow_is_recovered_before_append() {
    let root = tempfile::tempdir().unwrap();
    let seed_root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let seed = Workspace::new(seed_root.path(), true, false).unwrap();
    for index in 0..512 {
        let mut event =
            EngineeringMilestone::new("agent_context", "understand", "succeeded", 1, Vec::new())
                .unwrap();
        event.timestamp_ms = 100 + index;
        persist(&workspace, &event).unwrap();
    }
    for index in 0..2 {
        let mut event =
            EngineeringMilestone::new("agent_context", "understand", "succeeded", 1, Vec::new())
                .unwrap();
        event.timestamp_ms = 1_000_000 + index;
        persist(&seed, &event).unwrap();
    }
    let directory = crate::evidence_store::workspace_state_directory(&workspace)
        .unwrap()
        .join("engineering-journal");
    let seed_directory = crate::evidence_store::workspace_state_directory(&seed)
        .unwrap()
        .join("engineering-journal");
    // Reproduce a pre-existing overflow with canonical files emitted by the
    // actual writer, without duplicating filename or serialization logic.
    for entry in fs::read_dir(seed_directory).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), directory.join(entry.file_name())).unwrap();
    }
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 514);
    assert!(load_recent(&workspace, 512).is_err());
    let mut event =
        EngineeringMilestone::new("run_command", "understand", "failed", 1, Vec::new()).unwrap();
    event.failure_codes = vec![crate::engineering_journal::EngineeringFailureCode::Timeout];
    persist(&workspace, &event).unwrap();
    let history = load_recent(&workspace, 512).unwrap();
    assert_eq!(history.retained_records, 512);
    assert_eq!(history.records.len(), 512);
    assert!(!history.truncated);
    assert!(history
        .records
        .iter()
        .any(|record| record.event_id == event.event_id));
}

#[test]
fn engineering_journal_excessive_recovery_scan_fails_before_new_append() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let directory = crate::evidence_store::workspace_state_directory(&workspace)
        .unwrap()
        .join("engineering-journal");
    fs::create_dir_all(&directory).unwrap();
    for index in 0..=super::MAX_RECOVERY_SCAN_ENTRIES {
        fs::write(directory.join(format!("invalid-{index:04}.json")), b"{}").unwrap();
    }
    let before = fs::read_dir(&directory).unwrap().count();
    let event =
        EngineeringMilestone::new("agent_context", "understand", "succeeded", 1, Vec::new())
            .unwrap();
    assert!(persist(&workspace, &event).is_err());
    assert_eq!(fs::read_dir(directory).unwrap().count(), before);
}

#[test]
fn engineering_journal_failed_postappend_cleanup_removes_new_record() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    for index in 0..511 {
        let mut event =
            EngineeringMilestone::new("agent_context", "understand", "succeeded", 1, Vec::new())
                .unwrap();
        event.timestamp_ms = 100 + index;
        persist(&workspace, &event).unwrap();
    }
    let directory = crate::evidence_store::workspace_state_directory(&workspace)
        .unwrap()
        .join("engineering-journal");
    fs::create_dir(directory.join("00000000000000000000-bad.json")).unwrap();
    let before = fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        before.len(),
        512,
        "511 records plus one corrupt directory fill the physical capacity"
    );
    let mut event =
        EngineeringMilestone::new("apply_edits", "change", "failed", 1, Vec::new()).unwrap();
    event.failure_codes = vec![crate::engineering_journal::EngineeringFailureCode::ShaMismatch];
    let error = persist(&workspace, &event).unwrap_err();
    assert!(format!("{error:#}").contains("cannot prune engineering milestone"));
    let after = fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        after, before,
        "failed cleanup must close and remove this append, not report success"
    );
    let history = load_recent(&workspace, 512).unwrap();
    assert_eq!(history.retained_records, 512);
    assert_eq!(history.records.len(), 511);
    assert!(history.truncated);
    assert!(!history
        .records
        .iter()
        .any(|record| record.event_id == event.event_id));
}

#[test]
fn engineering_journal_failed_existing_overflow_cleanup_does_not_append() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    for index in 0..512 {
        let mut event =
            EngineeringMilestone::new("agent_context", "understand", "succeeded", 1, Vec::new())
                .unwrap();
        event.timestamp_ms = 100 + index;
        persist(&workspace, &event).unwrap();
    }
    let directory = crate::evidence_store::workspace_state_directory(&workspace)
        .unwrap()
        .join("engineering-journal");
    fs::create_dir(directory.join("00000000000000000000-bad.json")).unwrap();
    let before = fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(before.len(), 513);
    let event =
        EngineeringMilestone::new("apply_edits", "change", "failed", 1, Vec::new()).unwrap();
    let error = persist(&workspace, &event).unwrap_err();
    assert!(format!("{error:#}").contains("cannot prune engineering milestone"));
    let after = fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        after, before,
        "unrecoverable old corruption must fail before creating a new record"
    );
    let history = load_recent(&workspace, 512).unwrap();
    assert_eq!(
        history.retained_records, 513,
        "an error does not claim pre-existing corruption was repaired"
    );
    assert!(history.truncated);
    assert!(!history
        .records
        .iter()
        .any(|record| record.event_id == event.event_id));
}

#[test]
fn engineering_journal_actor_defaults_to_unknown_and_is_bounded() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let event =
        EngineeringMilestone::new("verify_project", "prove", "succeeded", 1, Vec::new()).unwrap();
    persist(&workspace, &event).unwrap();
    let history = load_recent(&workspace, 4).unwrap();
    assert_eq!(history.records[0].actor.as_deref(), Some("unknown"));
    assert!(history.chain_valid);

    let mut long =
        EngineeringMilestone::new("verify_project", "prove", "succeeded", 1, Vec::new()).unwrap();
    long.actor = Some("a".repeat(81));
    assert!(persist(&workspace, &long).is_err());
}

#[test]
fn engineering_journal_chain_links_records_and_detects_tampering() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    for index in 0..3 {
        let event = EngineeringMilestone::new(
            "verify_project",
            "prove",
            "succeeded",
            index + 1,
            Vec::new(),
        )
        .unwrap();
        persist(&workspace, &event).unwrap();
    }
    let history = load_recent(&workspace, 8).unwrap();
    assert!(history.chain_valid);
    let records = history.records;
    let hashes = records
        .iter()
        .filter_map(|record| record.record_hash.as_deref().map(|hash| (hash, record)))
        .collect::<std::collections::BTreeMap<&str, &EngineeringMilestone>>();
    let mut links = 0;
    for record in &records {
        if let Some(previous) = record.chain_previous.as_deref() {
            assert!(
                hashes.contains_key(previous),
                "every chain link must resolve inside the retained window"
            );
            links += 1;
        }
    }
    assert_eq!(links + 1, hashes.len());
    let mut tampered = records.clone();
    tampered[0].duration_ms += 1;
    assert!(crate::engineering_journal::verify_chain(&tampered).is_err());
    let mut gap = records.clone();
    gap[0].chain_previous = Some("0".repeat(64));
    assert!(crate::engineering_journal::verify_chain(&gap).is_err());
}

#[test]
fn engineering_journal_export_is_bounded_and_json_lines() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    for index in 0..3 {
        let event = EngineeringMilestone::new(
            "verify_project",
            "prove",
            "succeeded",
            index + 1,
            Vec::new(),
        )
        .unwrap();
        persist(&workspace, &event).unwrap();
    }
    let history = load_recent(&workspace, 8).unwrap();
    let exported = crate::engineering_journal::export_records(&history.records, 2).unwrap();
    let lines = exported.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    for line in lines {
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(value.get("record_hash").is_some());
        assert_eq!(
            value.get("actor").and_then(|actor| actor.as_str()),
            Some("unknown")
        );
    }
}

#[test]
fn engineering_journal_export_clamps_the_requested_window() {
    let records = (0..600)
        .map(|index| {
            EngineeringMilestone::new("verify_project", "prove", "succeeded", index, Vec::new())
                .unwrap()
        })
        .collect::<Vec<_>>();
    let exported = crate::engineering_journal::export_records(&records, 1_000_000).unwrap();
    assert_eq!(exported.lines().count(), 512);
    let exported = crate::engineering_journal::export_records(&records, 0).unwrap();
    assert_eq!(exported.lines().count(), 1);
}
