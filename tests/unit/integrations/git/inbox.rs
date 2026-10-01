use super::*;
use ring::hmac;

pub(super) const KEY: &[u8] = b"inbox-test-webhook-key-not-live";
fn config() -> GitHubConfig {
    GitHubConfig::new(
        ProviderRepository::new("owner", "repo").unwrap(),
        12,
        34,
        "gate",
    )
    .unwrap()
}
pub(super) fn body(number: u64) -> Vec<u8> {
    serde_json::to_vec(&json!({"action":"synchronize", "number":number,
        "installation":{"id":56}, "repository":{"id":12,"full_name":"owner/repo"},
        "pull_request":{"number":number,"state":"open","draft":false,"merged":false,
            "base":{"sha":"a".repeat(40),"repo":{"id":12,"full_name":"owner/repo"}},
            "head":{"sha":"b".repeat(40),"repo":{"id":13,"full_name":"fork/repo"}},
            "body":"PRIVATE-PAYLOAD-DO-NOT-PRINT"}}))
    .unwrap()
}
pub(super) fn headers(body: &[u8]) -> HeaderMap {
    signed_headers(body, KEY)
}

pub(super) fn signed_headers(body: &[u8], key: &[u8]) -> HeaderMap {
    let tag = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), body);
    let signature = format!(
        "sha256={}",
        tag.as_ref()
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect::<String>()
    );
    let mut headers = HeaderMap::new();
    for (name, value) in HEADER_NAMES.into_iter().zip([
        signature,
        "pull_request".into(),
        "72d3162e-cc78-11e3-81ab-4c9367dc0958".into(),
        "application/json".into(),
    ]) {
        headers.insert(
            reqwest::header::HeaderName::from_static(name),
            HeaderValue::from_str(&value).unwrap(),
        );
    }
    headers
}
pub(super) fn inbox() -> (tempfile::TempDir, GitHubInbox) {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().canonicalize().unwrap().join("inbox");
    let inbox = GitHubInbox::initialize(&root, config(), 56).unwrap();
    (parent, inbox)
}
fn deny(delivery: &VerifiedPullRequestDelivery) -> GitHubGateReceipt {
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

#[tokio::test]
async fn inbox_busy_candidate_does_not_starve_unrelated_due_delivery() {
    let (_parent, inbox) = inbox();
    let first = body(7);
    let mut second: Value = serde_json::from_slice(&body(8)).unwrap();
    second["pull_request"]["head"]["sha"] = json!("c".repeat(40));
    let second = serde_json::to_vec(&second).unwrap();
    inbox.enqueue(&headers(&first), &first, KEY).unwrap();
    inbox.enqueue(&headers(&second), &second, KEY).unwrap();
    let provider = GitHubProvider::client(
        config(),
        "NOT-LIVE-LOCK-TEST",
        Url::parse(&format!("https://{}.invalid/", uuid::Uuid::new_v4())).unwrap(),
        true,
    )
    .unwrap();
    let delivery =
        VerifiedPullRequestDelivery::verify(&config(), 56, &headers(&first), &first, KEY).unwrap();
    let held = PublicationGuard::acquire(&provider, delivery.target()).unwrap();
    let receipt = inbox
        .process_admitted_at(
            KEY,
            now_ms().unwrap(),
            |delivery| PublicationGuard::acquire(&provider, delivery.target()).map(Arc::new),
            |delivery, _guard| async move { Ok(deny(&delivery)) },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt.target().change, 8);
    let status = inbox.status().unwrap();
    assert_eq!(status["entries"][0]["state"], "queued");
    assert_eq!(status["entries"][0]["attempts"], 0);
    assert_eq!(status["entries"][1]["state"], "completed");
    assert_eq!(status["entries"][1]["attempts"], 1);
    drop(held);
}

#[test]
fn inbox_deduplicates_body_across_delivery_ids_and_restart_without_exposing_payload() {
    let (_parent, inbox) = inbox();
    let bytes = body(7);
    let mut headers = headers(&bytes);
    let first = inbox.enqueue(&headers, &bytes, KEY).unwrap();
    assert!(!first.duplicate);
    headers.insert(
        "x-github-delivery",
        HeaderValue::from_static("82d3162e-cc78-11e3-81ab-4c9367dc0958"),
    );
    let reopened = GitHubInbox::open(&inbox.root, config(), 56).unwrap();
    let second = reopened.enqueue(&headers, &bytes, KEY).unwrap();
    assert!(second.duplicate);
    assert_eq!(first.generation, second.generation);
    assert_eq!(first.body_digest, second.body_digest);
    let text = reopened.status().unwrap().to_string();
    assert!(!text.contains("PRIVATE-PAYLOAD"));
    assert!(!text.contains("sha256="));
    assert!(!text.contains(std::str::from_utf8(KEY).unwrap()));
    assert_eq!(reopened.status().unwrap()["current_acceptance"], false);
}

#[test]
fn inbox_bad_authentication_or_foreign_binding_has_no_write_effect() {
    let (_parent, inbox) = inbox();
    let before = fs::read(inbox.root.join("inbox.json")).unwrap();
    let bytes = body(7);
    assert!(inbox
        .enqueue(&headers(&bytes), b"other content", KEY)
        .is_err());
    assert!(GitHubInbox::open(&inbox.root, config(), 999).is_err());
    let other = GitHubConfig::new(
        ProviderRepository::new("owner", "repo").unwrap(),
        12,
        34,
        "other-gate",
    )
    .unwrap();
    assert!(GitHubInbox::open(&inbox.root, other, 56).is_err());
    assert_eq!(before, fs::read(inbox.root.join("inbox.json")).unwrap());
}

#[test]
fn inbox_missing_corrupt_or_partial_state_is_never_rebootstrapped() {
    let (_parent, inbox) = inbox();
    assert!(GitHubInbox::initialize(&inbox.root, config(), 56).is_err());
    fs::write(inbox.root.join("inbox.json"), b"broken").unwrap();
    assert!(inbox.status().is_err());
    fs::remove_file(inbox.root.join("inbox.json")).unwrap();
    assert!(GitHubInbox::open(&inbox.root, config(), 56).is_err());
    assert!(GitHubInbox::initialize(&inbox.root, config(), 56).is_err());
    assert!(!inbox.root.join("inbox.json").exists());
}

#[test]
fn inbox_released_guard_does_not_leave_an_inherited_descriptor_lock() {
    let (_parent, inbox) = inbox();
    for name in ["inbox.lock", "worker.lock"] {
        let guard = inbox.lock_file(name).unwrap();
        // File::try_clone shares the open-file description, as an inherited
        // descriptor can. Merely closing the original is not explicit unlock.
        let inherited = guard.try_clone().unwrap();
        assert!(inbox.lock_file(name).is_err());
        drop(guard);
        let replacement = inbox.lock_file(name);
        assert!(
            replacement.is_ok(),
            "completed operation left {name} held by its duplicate"
        );
        drop(replacement);
        drop(inherited);
    }
}

#[test]
fn inbox_attempt_bounds_clock_and_duplicate_entries_are_validated() {
    let (_parent, inbox) = inbox();
    let bytes = body(7);
    inbox.enqueue(&headers(&bytes), &bytes, KEY).unwrap();
    let now = now_ms().unwrap();
    let state = inbox.read(now).unwrap();
    let mut duplicate = state.clone();
    duplicate.entries.push(duplicate.entries[0].clone());
    assert!(inbox.validate(&duplicate, now).is_err());
    let mut invalid = state.clone();
    invalid.entries[0].attempts = MAX_ATTEMPTS + 1;
    assert!(inbox.validate(&invalid, now).is_err());
    assert!(inbox.validate(&state, state.updated_at_ms - 1).is_err());
}

#[tokio::test]
async fn inbox_retry_backoff_and_exhaustion_are_durable() {
    for _ in 0..16 {
        exercise_retry_history().await;
    }
}

async fn exercise_retry_history() {
    let (_parent, inbox) = inbox();
    let bytes = body(7);
    inbox.enqueue(&headers(&bytes), &bytes, KEY).unwrap();
    let mut now = now_ms().unwrap();
    for attempt in 1..=MAX_ATTEMPTS {
        let error = inbox
            .process_at(KEY, now, |_| async { bail!("PRIVATE FAILURE") })
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("inbox publication unavailable"),
            "attempt {attempt}: {error:#}"
        );
        let state = inbox.read(u64::MAX).unwrap();
        assert_eq!(state.entries[0].attempts, attempt);
        assert!(inbox
            .process_at(KEY, state.updated_at_ms, |_| async {
                panic!("backoff bypass")
            })
            .await
            .unwrap()
            .is_none());
        now = state.entries[0].retry_at_ms;
    }
    assert_eq!(
        inbox.read(u64::MAX).unwrap().entries[0].state,
        DeliveryState::Exhausted
    );
    assert!(inbox
        .process_at(KEY, now + 1, |_| async { panic!("attempt limit bypass") })
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn inbox_publication_keeps_receiver_and_status_available_and_preserves_new_entries() {
    let (_parent, inbox) = inbox();
    let first = body(7);
    inbox.enqueue(&headers(&first), &first, KEY).unwrap();
    let other = inbox.clone();
    let result = inbox
        .process_at(KEY, now_ms().unwrap(), |delivery| async move {
            assert_eq!(other.status().unwrap()["entries"][0]["state"], "publishing");
            let second = body(8);
            other.enqueue(&headers(&second), &second, KEY).unwrap();
            assert!(other
                .process_at(KEY, now_ms().unwrap(), |_| async {
                    panic!("overlapping worker")
                })
                .await
                .is_err());
            Ok(deny(&delivery))
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.verdict(), GateVerdict::Blocked);
    let state = inbox.read(now_ms().unwrap()).unwrap();
    assert_eq!(state.entries.len(), 2);
    assert_eq!(state.entries[0].state, DeliveryState::Completed);
    assert_eq!(state.entries[1].state, DeliveryState::Queued);
    assert!(
        inbox
            .enqueue(&headers(&first), &first, KEY)
            .unwrap()
            .duplicate
    );
}

#[tokio::test]
async fn inbox_cancelled_worker_releases_os_lock_without_replaying_a_success_receipt() {
    let (_parent, inbox) = inbox();
    let bytes = body(7);
    inbox.enqueue(&headers(&bytes), &bytes, KEY).unwrap();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let cloned = inbox.clone();
    let worker = tokio::spawn(async move {
        cloned
            .process_at(KEY, now_ms().unwrap(), |_| async move {
                sender.send(()).unwrap();
                std::future::pending::<Result<GitHubGateReceipt>>().await
            })
            .await
    });
    receiver.await.unwrap();
    worker.abort();
    assert!(worker.await.unwrap_err().is_cancelled());
    let state = inbox.read(now_ms().unwrap()).unwrap();
    assert_eq!(state.entries[0].state, DeliveryState::Publishing);
    let due = state.entries[0].retry_at_ms;
    assert!(inbox
        .process_at(KEY, due, |delivery| async move { Ok(deny(&delivery)) })
        .await
        .unwrap()
        .is_some());
    assert_eq!(inbox.read(u64::MAX).unwrap().entries[0].attempts, 2);
    assert!(inbox
        .process_at(KEY, due + 1, |_| async { panic!("completed replay") })
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn inbox_key_rotation_does_not_starve_later_authenticated_deliveries() {
    let (_parent, inbox) = inbox();
    let old_body = body(7);
    inbox.enqueue(&headers(&old_body), &old_body, KEY).unwrap();
    let rotated = b"rotated-inbox-test-key-not-live";
    let new_body = body(8);
    let mut new_headers = headers(&new_body);
    let tag = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, rotated), &new_body);
    let signature = format!(
        "sha256={}",
        tag.as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    new_headers.insert(
        "x-hub-signature-256",
        HeaderValue::from_str(&signature).unwrap(),
    );
    inbox.enqueue(&new_headers, &new_body, rotated).unwrap();

    let reopened = GitHubInbox::open(&inbox.root, config(), 56).unwrap();
    let result = reopened
        .process_at(rotated, now_ms().unwrap(), |delivery| async move {
            assert_eq!(
                delivery.target().change,
                8,
                "unverifiable event reached publication"
            );
            Ok(deny(&delivery))
        })
        .await
        .expect("old-key entry starved a valid later delivery")
        .unwrap();
    assert_eq!(result.verdict(), GateVerdict::Blocked);
    let state = reopened.read(now_ms().unwrap()).unwrap();
    assert_eq!(state.entries[0].state, DeliveryState::Queued);
    assert_eq!(state.entries[0].attempts, 0);
    assert_eq!(state.entries[1].state, DeliveryState::Completed);
    assert_eq!(state.entries[1].attempts, 1);

    let before = fs::read(inbox.root.join("inbox.json")).unwrap();
    let error = reopened
        .process_at(rotated, now_ms().unwrap(), |_| async {
            panic!("unverifiable or completed entry reached publication")
        })
        .await
        .unwrap_err();
    assert!(!error
        .to_string()
        .contains(std::str::from_utf8(rotated).unwrap()));
    assert_eq!(before, fs::read(inbox.root.join("inbox.json")).unwrap());
    // Retention is not silent rejection: a deliberately restored trusted key can
    // authenticate the original event; no attempts or tombstones were discarded.
    reopened
        .process_at(KEY, now_ms().unwrap(), |delivery| async move {
            assert_eq!(delivery.target().change, 7);
            Ok(deny(&delivery))
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reopened.status().unwrap()["retained"], 2);
    assert_eq!(reopened.status().unwrap()["current_acceptance"], false);
}

#[tokio::test]
async fn inbox_authenticated_redelivery_refreshes_pending_signature_without_resetting_attempts() {
    for retry in [false, true] {
        let (_parent, inbox) = inbox();
        let bytes = body(7);
        inbox.enqueue(&headers(&bytes), &bytes, KEY).unwrap();
        if retry {
            assert!(inbox
                .process_at(KEY, now_ms().unwrap(), |_| async {
                    bail!("previous publication unavailable")
                })
                .await
                .is_err());
        }
        let before = inbox.read(now_ms().unwrap()).unwrap();
        let key = b"newly-provisioned-inbox-test-key";
        let signed = signed_headers(&bytes, key);
        let ack = inbox.enqueue(&signed, &bytes, key).unwrap();
        assert!(ack.duplicate);
        assert!(
            ack.generation > before.generation,
            "current signature was not durably refreshed"
        );
        assert_eq!(serde_json::to_value(&ack).unwrap()["reauthenticated"], true);
        let reopened = GitHubInbox::open(&inbox.root, config(), 56).unwrap();
        let after = reopened.read(now_ms().unwrap()).unwrap();
        assert_eq!(after.entries.len(), 1);
        assert_eq!(after.entries[0].state, before.entries[0].state);
        assert_eq!(after.entries[0].attempts, before.entries[0].attempts);
        assert_eq!(after.entries[0].body_digest, before.entries[0].body_digest);
        assert_eq!(after.entries[0].body_base64, before.entries[0].body_base64);
        assert_eq!(
            after.entries[0].received_at_ms,
            before.entries[0].received_at_ms
        );
        assert!(after.entries[0].retry_at_ms >= before.entries[0].retry_at_ms);
        if retry {
            assert_eq!(after.entries[0].retry_at_ms, before.entries[0].retry_at_ms);
        }
        let stable = fs::read(inbox.root.join("inbox.json")).unwrap();
        let duplicate = reopened.enqueue(&signed, &bytes, key).unwrap();
        assert_eq!(duplicate.generation, after.generation);
        assert_eq!(
            serde_json::to_value(&duplicate).unwrap()["reauthenticated"],
            false
        );
        assert_eq!(stable, fs::read(inbox.root.join("inbox.json")).unwrap());
        reopened
            .process_at(
                key,
                after.entries[0].retry_at_ms.max(now_ms().unwrap()),
                |delivery| async move { Ok(deny(&delivery)) },
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            reopened.read(u64::MAX).unwrap().entries[0].attempts,
            before.entries[0].attempts + 1
        );
    }
}

#[test]
fn inbox_rotated_redelivery_cannot_resurrect_terminal_or_inflight_work() {
    for terminal in [
        DeliveryState::Completed,
        DeliveryState::Exhausted,
        DeliveryState::Publishing,
    ] {
        let (_parent, inbox) = inbox();
        let bytes = body(7);
        inbox.enqueue(&headers(&bytes), &bytes, KEY).unwrap();
        let mut state = inbox.read(now_ms().unwrap()).unwrap();
        state.entries[0].state = terminal;
        state.entries[0].attempts = if terminal == DeliveryState::Exhausted {
            MAX_ATTEMPTS
        } else {
            1
        };
        state.entries[0].retry_at_ms = state.updated_at_ms;
        inbox.write(&state, false).unwrap();
        let before = fs::read(inbox.root.join("inbox.json")).unwrap();
        let key = b"newly-provisioned-inbox-test-key";
        let ack = inbox
            .enqueue(&signed_headers(&bytes, key), &bytes, key)
            .unwrap();
        assert!(ack.duplicate);
        assert_eq!(ack.generation, state.generation);
        assert_eq!(
            serde_json::to_value(&ack).unwrap()["reauthenticated"],
            false
        );
        assert_eq!(before, fs::read(inbox.root.join("inbox.json")).unwrap());
    }
}

#[tokio::test]
async fn inbox_invalid_worker_key_does_not_mutate_interrupted_attempts() {
    let (_parent, inbox) = inbox();
    let bytes = body(7);
    inbox.enqueue(&headers(&bytes), &bytes, KEY).unwrap();
    let mut state = inbox.read(now_ms().unwrap()).unwrap();
    state.entries[0].state = DeliveryState::Publishing;
    state.entries[0].attempts = MAX_ATTEMPTS;
    state.entries[0].retry_at_ms = state.updated_at_ms;
    inbox.write(&state, false).unwrap();
    let before = fs::read(inbox.root.join("inbox.json")).unwrap();
    for key in [Vec::new(), vec![b'x'; 15], vec![b'x'; 4097]] {
        assert!(inbox
            .process_at(&key, now_ms().unwrap(), |_| async {
                panic!("invalid key reached publication")
            })
            .await
            .is_err());
        assert_eq!(before, fs::read(inbox.root.join("inbox.json")).unwrap());
    }
}

#[tokio::test]
async fn inbox_reauthenticates_persisted_bodies_before_any_publication() {
    let (_parent, inbox) = inbox();
    let bytes = body(7);
    inbox.enqueue(&headers(&bytes), &bytes, KEY).unwrap();
    assert!(inbox
        .process_at(
            b"different-key-after-rotation",
            now_ms().unwrap(),
            |_| async { panic!("bad key reached network") }
        )
        .await
        .is_err());
    assert_eq!(inbox.status().unwrap()["entries"][0]["attempts"], 0);
    let mut state = inbox.read(now_ms().unwrap()).unwrap();
    let changed = body(9);
    state.entries[0].body_base64 = STANDARD.encode(&changed);
    state.entries[0].body_digest = digest(&changed);
    inbox.write(&state, false).unwrap(); // Valid outer checksum cannot forge the original signature.
    assert!(inbox
        .process_at(KEY, now_ms().unwrap(), |_| async {
            panic!("tampered body reached network")
        })
        .await
        .is_err());
}

#[cfg(unix)]
#[test]
fn inbox_rejects_private_path_aliases_permissions_and_hardlinks() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let (parent, inbox) = inbox();
    let alias = parent.path().canonicalize().unwrap().join("alias");
    symlink(&inbox.root, &alias).unwrap();
    assert!(GitHubInbox::open(&alias, config(), 56).is_err());
    let path = inbox.root.join("inbox.json");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(inbox.status().is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let copy = parent.path().join("hardlink");
    fs::hard_link(&path, copy).unwrap();
    assert!(inbox.status().is_err());
}

#[test]
fn inbox_capacity_never_evicts_completed_replay_tombstones() {
    let (_parent, inbox) = inbox();
    let original = body(1);
    inbox.enqueue(&headers(&original), &original, KEY).unwrap();
    let mut state = inbox.read(now_ms().unwrap()).unwrap();
    for number in 2..=MAX_EVENTS as u64 {
        let bytes = body(number);
        let mut entry = state.entries[0].clone();
        entry.body_base64 = STANDARD.encode(&bytes);
        entry.body_digest = digest(&bytes);
        entry.state = DeliveryState::Completed;
        entry.attempts = 1;
        entry.retry_at_ms = entry.updated_at_ms + backoff_ms(1);
        let signed = headers(&bytes);
        for (index, name) in HEADER_NAMES.iter().enumerate() {
            entry.headers[index] = signed[*name].to_str().unwrap().into();
        }
        state.entries.push(entry);
    }
    inbox.write(&state, false).unwrap();
    let before = fs::read(inbox.root.join("inbox.json")).unwrap();
    let bytes = body(999);
    assert!(inbox.enqueue(&headers(&bytes), &bytes, KEY).is_err());
    assert!(
        inbox
            .enqueue(&headers(&original), &original, KEY)
            .unwrap()
            .duplicate
    );
    assert_eq!(before, fs::read(inbox.root.join("inbox.json")).unwrap());
}
