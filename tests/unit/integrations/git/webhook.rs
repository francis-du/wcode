use super::*;

const KEY: &[u8] = b"webhook-only-test-key-not-a-publisher-token";

fn config() -> GitHubConfig {
    GitHubConfig::new(
        ProviderRepository::new("owner", "repo").unwrap(),
        123,
        456,
        "gate",
    )
    .unwrap()
}

fn payload() -> Value {
    json!({
        "action": "synchronize", "number": 7,
        "installation": {"id": 789},
        "repository": {"id": 123, "full_name": "owner/repo"},
        "pull_request": {
            "number": 7, "state": "open", "draft": false, "merged": false,
            "base": {"sha": "a".repeat(40), "ref": "main", "repo": {"id": 123, "full_name": "owner/repo"}},
            "head": {"sha": "b".repeat(40), "repo": {"id": 987, "full_name": "fork/repo"}},
            "title": "用户可控标题", "body": "do not echo this or accept its PASS"
        }
    })
}

fn headers(body: &[u8]) -> HeaderMap {
    let key = hmac::Key::new(hmac::HMAC_SHA256, KEY);
    let tag = hmac::sign(&key, body);
    let signature = format!(
        "sha256={}",
        tag.as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-hub-signature-256",
        HeaderValue::from_str(&signature).unwrap(),
    );
    headers.insert("x-github-event", HeaderValue::from_static("pull_request"));
    headers.insert(
        "x-github-delivery",
        HeaderValue::from_static("72d3162e-cc78-11e3-81ab-4c9367dc0958"),
    );
    headers.insert(
        "content-type",
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    headers
}

fn verify(value: &Value) -> Result<VerifiedPullRequestDelivery> {
    let body = serde_json::to_vec(value).unwrap();
    VerifiedPullRequestDelivery::verify(&config(), 789, &headers(&body), &body, KEY)
}

#[test]
fn github_webhook_signature_matches_official_vector_and_rejects_byte_changes() {
    // GitHub's published independent test vector, not an in-test self-signing oracle.
    let signature = "sha256=757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17";
    assert!(verify_signature(b"Hello, World!", signature, b"It's a Secret to Everybody").is_ok());
    assert!(
        verify_signature(b"Hello, World!\n", signature, b"It's a Secret to Everybody").is_err()
    );
    assert!(verify_signature(b"Hello, World!", signature, b"a different webhook secret").is_err());
}

#[test]
fn github_webhook_exact_candidate_preserves_unicode_and_ignores_authority_claims() {
    for action in [
        "opened",
        "reopened",
        "synchronize",
        "ready_for_review",
        "edited",
    ] {
        let mut value = payload();
        value["action"] = json!(action);
        value["ready"] = json!(true);
        value["human_approval"] = json!(true);
        value["native_verification"] = json!(true);
        value["sender"] = json!({"login": "admin", "role": "owner"});
        let delivery = verify(&value).unwrap();
        assert_eq!(delivery.target().change, 7);
        assert_eq!(delivery.target().base_sha, "a".repeat(40));
        assert_eq!(delivery.target().head_sha, "b".repeat(40));
        let exported = serde_json::to_value(&delivery).unwrap();
        for field in [
            "freshness_verified",
            "replay_protection_verified",
            "native_verification",
            "human_approval",
            "remote_mutations",
        ] {
            assert_eq!(exported[field], false);
        }
        let text = serde_json::to_string(&delivery).unwrap();
        for forbidden in [
            "用户可控标题",
            "do not echo",
            "admin",
            "webhook-only-test-key",
        ] {
            assert!(!text.contains(forbidden));
        }
    }
}

#[test]
fn github_webhook_rejects_tampering_missing_duplicate_headers_and_legacy_signatures() {
    let mut body = serde_json::to_vec(&payload()).unwrap();
    let valid_headers = headers(&body);
    for name in [
        "x-hub-signature-256",
        "x-github-event",
        "x-github-delivery",
        "content-type",
    ] {
        let mut missing = valid_headers.clone();
        missing.remove(name);
        assert!(VerifiedPullRequestDelivery::verify(&config(), 789, &missing, &body, KEY).is_err());
        let mut duplicate = valid_headers.clone();
        duplicate.append(name, valid_headers[name].clone());
        assert!(
            VerifiedPullRequestDelivery::verify(&config(), 789, &duplicate, &body, KEY).is_err()
        );
    }
    for signature in ["sha1=abc", "sha256=", "sha256=xyz", " sha256=abcdef"] {
        let mut bad = valid_headers.clone();
        bad.insert(
            "x-hub-signature-256",
            HeaderValue::from_str(signature).unwrap(),
        );
        assert!(VerifiedPullRequestDelivery::verify(&config(), 789, &bad, &body, KEY).is_err());
    }
    body.push(b'\n');
    assert!(
        VerifiedPullRequestDelivery::verify(&config(), 789, &valid_headers, &body, KEY).is_err()
    );
}

#[test]
fn github_webhook_scopes_repository_installation_and_pr_identity() {
    for (pointer, replacement) in [
        ("/installation/id", json!(999)),
        ("/repository/id", json!(999)),
        ("/repository/full_name", json!("other/repo")),
        ("/pull_request/base/repo/id", json!(999)),
        ("/pull_request/base/repo/full_name", json!("other/repo")),
        ("/number", json!(0)),
        ("/pull_request/number", json!(8)),
    ] {
        let mut value = payload();
        *value.pointer_mut(pointer).unwrap() = replacement;
        assert!(verify(&value).is_err(), "{pointer}");
    }
    let body = serde_json::to_vec(&payload()).unwrap();
    assert!(
        VerifiedPullRequestDelivery::verify(&config(), 0, &headers(&body), &body, KEY).is_err()
    );
}

#[test]
fn github_webhook_rejects_unsupported_closed_draft_and_incomplete_targets() {
    for (pointer, replacement) in [
        ("/action", json!("closed")),
        ("/action", json!("labeled")),
        ("/pull_request/state", json!("closed")),
        ("/pull_request/draft", json!(true)),
        ("/pull_request/merged", json!(true)),
        ("/pull_request/head/sha", json!("main")),
        ("/pull_request/head/sha", json!("b".repeat(64))),
        ("/pull_request/head/repo", Value::Null),
    ] {
        let mut value = payload();
        *value.pointer_mut(pointer).unwrap() = replacement;
        assert!(verify(&value).is_err(), "{pointer}");
    }
    let body = serde_json::to_vec(&payload()).unwrap();
    for (name, value) in [
        ("x-github-event", "push"),
        ("x-github-event", "pull_request_target"),
        ("content-type", "application/x-www-form-urlencoded"),
        ("x-github-delivery", "../arbitrary-path"),
        ("x-github-delivery", "00000000-0000-0000-0000-000000000000"),
    ] {
        let mut bad = headers(&body);
        bad.insert(name, HeaderValue::from_static(value));
        assert!(VerifiedPullRequestDelivery::verify(&config(), 789, &bad, &body, KEY).is_err());
    }
}

#[test]
fn github_webhook_limits_untrusted_input_and_never_echoes_payload_or_key() {
    let huge = vec![b'x'; MAX_WEBHOOK_BYTES + 1];
    assert!(
        VerifiedPullRequestDelivery::verify(&config(), 789, &HeaderMap::new(), &huge, KEY).is_err()
    );
    let body = b"secret private source: not JSON";
    let error =
        VerifiedPullRequestDelivery::verify(&config(), 789, &headers(body), body, KEY).unwrap_err();
    assert!(!error.to_string().contains("private source"));
    let normal = serde_json::to_vec(&payload()).unwrap();
    for bad_key in [b"".as_slice(), b"short".as_slice()] {
        assert!(VerifiedPullRequestDelivery::verify(
            &config(),
            789,
            &headers(&normal),
            &normal,
            bad_key
        )
        .is_err());
    }
    let mut bad = headers(body);
    bad.insert(
        "x-hub-signature-256",
        HeaderValue::from_str(&format!("sha256={}", "0".repeat(64))).unwrap(),
    );
    let error = VerifiedPullRequestDelivery::verify(&config(), 789, &bad, body, KEY).unwrap_err();
    assert!(error.to_string().contains("signature verification failed"));
}

#[test]
fn github_webhook_changed_delivery_id_does_not_change_signed_body_identity() {
    let body = serde_json::to_vec(&payload()).unwrap();
    let original = headers(&body);
    let first = VerifiedPullRequestDelivery::verify(&config(), 789, &original, &body, KEY).unwrap();
    let mut renamed = original;
    renamed.insert(
        "x-github-delivery",
        HeaderValue::from_static("82d3162e-cc78-11e3-81ab-4c9367dc0958"),
    );
    let second = VerifiedPullRequestDelivery::verify(&config(), 789, &renamed, &body, KEY).unwrap();
    assert_eq!(first.body_digest(), second.body_digest());
    assert!(!first.replay_protection_verified);
    assert!(!second.freshness_verified);
}

#[test]
fn github_webhook_duplicate_identity_fields_fail_after_valid_signature() {
    let original = serde_json::to_string(&payload()).unwrap();
    let duplicate = original.replacen("\"number\":7", "\"number\":7,\"number\":8", 1);
    assert_ne!(duplicate, original);
    assert!(VerifiedPullRequestDelivery::verify(
        &config(),
        789,
        &headers(duplicate.as_bytes()),
        duplicate.as_bytes(),
        KEY
    )
    .is_err());
}

#[tokio::test]
async fn github_webhook_cross_publisher_is_rejected_before_network() {
    let delivery = verify(&payload()).unwrap();
    let other = GitHubConfig::new(
        ProviderRepository::new("owner", "repo").unwrap(),
        123,
        999,
        "gate",
    )
    .unwrap();
    let provider = GitHubProvider::client(
        other,
        "test-not-live-token",
        Url::parse("http://127.0.0.1:1/").unwrap(),
        false,
    )
    .unwrap();
    let error = provider
        .publish_delivery(&delivery, "does-not-exist", "project")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("another publisher enrollment"));
}
