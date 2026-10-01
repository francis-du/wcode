use super::super::tests::{body, headers, inbox, KEY};
use super::*;

fn private_parent(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[cfg(not(unix))]
    let _ = path;
}
fn blocked(delivery: &VerifiedPullRequestDelivery) -> GitHubGateReceipt {
    receipt(
        NativePublication::unavailable(delivery.target().clone()).unwrap(),
        CheckObservation {
            id: 1,
            head_sha: delivery.target().head_sha.clone(),
            name: "gate".into(),
            source_id: "github-app:34".into(),
            external_id: "wcode:unavailable".into(),
            status: "completed".into(),
            conclusion: Some("failure".into()),
        },
    )
}
async fn complete_one(inbox: &GitHubInbox) {
    let done = inbox
        .process_at(KEY, now_ms().unwrap(), |delivery| async move {
            Ok(blocked(&delivery))
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(done.verdict(), GateVerdict::Blocked);
}

#[tokio::test]
async fn inbox_archive_preserves_raw_payload_and_replay_identity_across_reopen() {
    let (parent, inbox) = inbox();
    private_parent(parent.path());
    let bytes = body(7);
    let original = inbox.enqueue(&headers(&bytes), &bytes, KEY).unwrap();
    complete_one(&inbox).await;
    let before = inbox.read(now_ms().unwrap()).unwrap();
    let output = parent.path().canonicalize().unwrap().join("archive.json");
    let pin = inbox.archive_terminal(before.generation, &output).unwrap();
    assert_eq!(pin.source_generation, before.generation);
    assert_eq!(pin.compacted_generation, before.generation + 1);
    assert_eq!(pin.archived_entries, 1);
    assert!(!pin.current_acceptance);
    let archive_bytes = fs::read(&output).unwrap();
    assert_eq!(digest(&archive_bytes), pin.archive_digest);
    let archive: DeliveryArchive = serde_json::from_slice(&archive_bytes).unwrap();
    assert_eq!(archive.entries[0].body().unwrap(), bytes);
    assert_eq!(archive.entries[0].headers, before.entries[0].headers);
    let reopened = GitHubInbox::open(&inbox.root, inbox.config.clone(), 56).unwrap();
    let compacted = fs::read(inbox.root.join("inbox.json")).unwrap();
    let duplicate = reopened.enqueue(&headers(&bytes), &bytes, KEY).unwrap();
    assert!(duplicate.duplicate && duplicate.archived && !duplicate.reauthenticated);
    assert_eq!(duplicate.body_digest, original.body_digest);
    assert_eq!(duplicate.generation, pin.compacted_generation);
    assert_eq!(fs::read(inbox.root.join("inbox.json")).unwrap(), compacted);
    assert!(reopened
        .process_at(KEY, now_ms().unwrap(), |_| async {
            panic!("archived replay executed")
        })
        .await
        .unwrap()
        .is_none());
    let report = reopened
        .inspect_archive(&output, &pin.archive_digest)
        .unwrap();
    assert_eq!(report["archived_entries"], 1);
    assert_eq!(report["restored"], false);
    assert!(!report.to_string().contains("PRIVATE-PAYLOAD"));
    assert!(!report.to_string().contains("sha256="));
    assert!(reopened
        .inspect_archive(&output, &digest(b"wrong external pin"))
        .is_err());
    let other = super::super::tests::inbox();
    assert!(other
        .1
        .inspect_archive(&output, &pin.archive_digest)
        .is_err());
    fs::remove_file(&output).unwrap();
    assert!(
        reopened
            .enqueue(&headers(&bytes), &bytes, KEY)
            .unwrap()
            .archived,
        "archive storage loss must never erase the online replay tombstone"
    );
}

#[tokio::test]
async fn inbox_archive_frees_payload_slots_without_evicting_completed_digests() {
    let (parent, inbox) = inbox();
    private_parent(parent.path());
    for n in 1..=MAX_EVENTS as u64 {
        let bytes = body(n);
        inbox.enqueue(&headers(&bytes), &bytes, KEY).unwrap();
    }
    let fresh = body(MAX_EVENTS as u64 + 1);
    assert!(inbox.enqueue(&headers(&fresh), &fresh, KEY).is_err());
    complete_one(&inbox).await;
    complete_one(&inbox).await;
    let generation = inbox.status().unwrap()["generation"].as_u64().unwrap();
    let output = parent.path().canonicalize().unwrap().join("archive.json");
    let pin = inbox.archive_terminal(generation, &output).unwrap();
    assert_eq!(pin.archived_entries, 2);
    assert_eq!(inbox.status().unwrap()["retained"], MAX_EVENTS - 2);
    assert_eq!(inbox.status().unwrap()["archived_replay_tombstones"], 2);
    assert!(
        !inbox
            .enqueue(&headers(&fresh), &fresh, KEY)
            .unwrap()
            .duplicate
    );
    let old = body(1);
    assert!(inbox.enqueue(&headers(&old), &old, KEY).unwrap().archived);
    assert_eq!(
        inbox.status().unwrap()["protected_signed_body_digests"],
        MAX_EVENTS + 1
    );
}

#[tokio::test]
async fn inbox_archive_rejects_stale_busy_existing_and_unsafe_destinations_without_pruning() {
    let (parent, inbox) = inbox();
    private_parent(parent.path());
    let bytes = body(7);
    inbox.enqueue(&headers(&bytes), &bytes, KEY).unwrap();
    complete_one(&inbox).await;
    let generation = inbox.status().unwrap()["generation"].as_u64().unwrap();
    let output = parent.path().canonicalize().unwrap().join("archive.json");
    let before = fs::read(inbox.root.join("inbox.json")).unwrap();
    assert!(inbox.archive_terminal(generation - 1, &output).is_err());
    assert!(!output.exists());
    let worker = inbox.lock_file("worker.lock").unwrap();
    assert!(inbox.archive_terminal(generation, &output).is_err());
    assert!(!output.exists());
    drop(worker);
    fs::write(&output, b"existing-confidential-file").unwrap();
    assert!(inbox.archive_terminal(generation, &output).is_err());
    assert_eq!(fs::read(&output).unwrap(), b"existing-confidential-file");
    assert!(inbox
        .archive_terminal(generation, &inbox.root.join("archive.json"))
        .is_err());
    assert!(inbox
        .archive_terminal(generation, Path::new("relative.json"))
        .is_err());
    assert_eq!(before, fs::read(inbox.root.join("inbox.json")).unwrap());
}

#[tokio::test]
async fn inbox_archive_retains_pending_and_interrupted_work_but_never_replays_exhausted() {
    let (parent, inbox) = inbox();
    private_parent(parent.path());
    for n in 1..=4 {
        let b = body(n);
        inbox.enqueue(&headers(&b), &b, KEY).unwrap();
    }
    complete_one(&inbox).await;
    let mut state = inbox.read(now_ms().unwrap()).unwrap();
    // Explicit interrupted/exhausted storage fixtures, not native execution proof.
    for (index, status, attempts) in [
        (1, DeliveryState::Publishing, 1),
        (2, DeliveryState::Exhausted, MAX_ATTEMPTS),
    ] {
        let entry = &mut state.entries[index];
        entry.state = status;
        entry.attempts = attempts;
        entry.retry_at_ms = entry.updated_at_ms + 10_000;
    }
    inbox.write(&state, false).unwrap();
    let output = parent.path().canonicalize().unwrap().join("archive.json");
    inbox.archive_terminal(state.generation, &output).unwrap();
    let after = inbox.read(now_ms().unwrap()).unwrap();
    assert_eq!(after.entries.len(), 2);
    assert_eq!(after.entries[0].state, DeliveryState::Publishing);
    assert_eq!(after.entries[0].attempts, 1);
    assert_eq!(after.entries[1].state, DeliveryState::Queued);
    assert_eq!(after.archived.len(), 2);
    let exhausted = body(3);
    assert!(
        inbox
            .enqueue(&headers(&exhausted), &exhausted, KEY)
            .unwrap()
            .archived
    );
}

#[tokio::test]
async fn inbox_archive_legacy_checksum_survives_and_corrupt_replay_index_fails_closed() {
    let (parent, inbox) = inbox();
    private_parent(parent.path());
    let legacy = fs::read(inbox.root.join("inbox.json")).unwrap();
    assert!(!String::from_utf8_lossy(&legacy).contains("archived"));
    GitHubInbox::open(&inbox.root, inbox.config.clone(), 56).unwrap();
    assert_eq!(legacy, fs::read(inbox.root.join("inbox.json")).unwrap());
    let bytes = body(7);
    inbox.enqueue(&headers(&bytes), &bytes, KEY).unwrap();
    complete_one(&inbox).await;
    let current = inbox.read(now_ms().unwrap()).unwrap();
    inbox
        .archive_terminal(
            current.generation,
            &parent.path().canonicalize().unwrap().join("archive.json"),
        )
        .unwrap();
    let clean = inbox.read(now_ms().unwrap()).unwrap();
    assert_eq!(clean.schema_version, 2);
    let mut old_reader = clean.clone();
    old_reader.schema_version = 1;
    assert!(inbox.validate(&old_reader, now_ms().unwrap()).is_err());
    let mut duplicate = clean.clone();
    duplicate.archived.push(duplicate.archived[0].clone());
    assert!(inbox.validate(&duplicate, now_ms().unwrap()).is_err());
    let mut cross = clean.clone();
    cross.entries.push(current.entries[0].clone());
    assert!(inbox.validate(&cross, now_ms().unwrap()).is_err());
    let mut bad = clean.clone();
    bad.archived[0].state = DeliveryState::Queued;
    assert!(inbox.validate(&bad, now_ms().unwrap()).is_err());
    let corrupt = Envelope {
        checksum: digest(&serde_json::to_vec(&bad).unwrap()),
        state: bad,
    };
    fs::write(
        inbox.root.join("inbox.json"),
        serde_json::to_vec(&corrupt).unwrap(),
    )
    .unwrap();
    assert!(inbox.status().is_err());
    assert!(inbox.enqueue(&headers(&bytes), &bytes, KEY).is_err());
}

#[tokio::test]
async fn inbox_archive_full_replay_index_refuses_without_deleting_old_tombstones() {
    let (parent, inbox) = inbox();
    private_parent(parent.path());
    let bytes = body(7);
    inbox.enqueue(&headers(&bytes), &bytes, KEY).unwrap();
    complete_one(&inbox).await;
    let mut state = inbox.read(now_ms().unwrap()).unwrap();
    state.schema_version = 2;
    for n in 0..MAX_REPLAY_TOMBSTONES {
        state.archived.push(ReplayTombstone {
            body_digest: digest(&n.to_le_bytes()),
            archive_digest: digest(b"fixture-archive"),
            state: DeliveryState::Completed,
            attempts: 1,
            received_at_ms: state.updated_at_ms,
            updated_at_ms: state.updated_at_ms,
        });
    }
    inbox.write(&state, false).unwrap();
    let before = fs::read(inbox.root.join("inbox.json")).unwrap();
    let output = parent.path().canonicalize().unwrap().join("archive.json");
    assert!(inbox.archive_terminal(state.generation, &output).is_err());
    assert!(!output.exists());
    assert_eq!(before, fs::read(inbox.root.join("inbox.json")).unwrap());
}

#[cfg(unix)]
#[tokio::test]
async fn inbox_archive_rejects_parent_aliases_and_nonprivate_exports() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let (parent, inbox) = inbox();
    private_parent(parent.path());
    let bytes = body(7);
    inbox.enqueue(&headers(&bytes), &bytes, KEY).unwrap();
    complete_one(&inbox).await;
    let generation = inbox.status().unwrap()["generation"].as_u64().unwrap();
    let root = parent.path().canonicalize().unwrap();
    let alias = root.join("alias");
    symlink(&root, &alias).unwrap();
    assert!(inbox
        .archive_terminal(generation, &alias.join("alias-output.json"))
        .is_err());
    assert!(!root.join("alias-output.json").exists());
    let output = root.join("archive.json");
    let pin = inbox.archive_terminal(generation, &output).unwrap();
    assert_eq!(
        fs::metadata(&output).unwrap().permissions().mode() & 0o077,
        0
    );
    fs::set_permissions(&output, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(inbox.inspect_archive(&output, &pin.archive_digest).is_err());
    assert!(
        inbox
            .enqueue(&headers(&bytes), &bytes, KEY)
            .unwrap()
            .archived
    );
}
