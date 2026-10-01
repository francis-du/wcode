use super::*;

#[test]
fn github_preflight_link_headers_cannot_hide_later_pages() {
    use crate::git_provider::github::preflight::has_next;
    use reqwest::header::{HeaderMap, HeaderValue, LINK};
    let mut headers = HeaderMap::new();
    assert!(!has_next(&headers).unwrap());
    headers.append(
        LINK,
        HeaderValue::from_static("<https://api.github.com/page1>; rel=\"prev\""),
    );
    assert!(!has_next(&headers).unwrap());
    headers.append(
        LINK,
        HeaderValue::from_static("<https://example.invalid/not-followed>; rel = next"),
    );
    assert!(has_next(&headers).unwrap());
    headers.insert(
        LINK,
        HeaderValue::from_static("<https://api.github.com/page2>; rel=\"next last\""),
    );
    assert!(has_next(&headers).unwrap());
}

#[test]
fn github_preflight_malformed_link_metadata_is_not_complete_coverage() {
    use crate::git_provider::github::preflight::has_next;
    use reqwest::header::{HeaderMap, HeaderValue, LINK};
    for invalid in [
        "",
        "broken",
        "<url>",
        "<url>; rel=",
        "<url>; rel=\" \"",
        "<url>; rel=unknown",
        "<url>; rel=\"next",
        "<url>; rel=prev; rel=next",
    ] {
        let mut headers = HeaderMap::new();
        headers.insert(LINK, HeaderValue::from_str(invalid).unwrap());
        assert!(has_next(&headers).is_err(), "{invalid}");
    }
}

fn signed_delivery(target: Value) -> VerifiedPullRequestDelivery {
    let bytes = serde_json::to_vec(&json!({
        "action": "synchronize", "number": 7, "installation": {"id": 789},
        "repository": {"id": 42, "full_name": "francis-du/wcode"},
        "pull_request": target,
        "ready": true, "native_verification": true, "human_approval": true
    }))
    .unwrap();
    let secret = b"separate-receiver-test-key-never-a-live-credential";
    let tag = ring::hmac::sign(
        &ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret),
        &bytes,
    );
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
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    VerifiedPullRequestDelivery::verify(&config(), 789, &headers, &bytes, secret).unwrap()
}

#[tokio::test]
async fn github_inbox_drains_real_native_acceptance_and_never_replays_a_green_receipt() {
    let native = trusted_native_fixture::NativeFixture::new();
    native.activate();
    let ready = native.verify_and_review().await;
    let mut target = native_pull(&native.base, &native.head);
    target["base"]["ref"] = json!("main");
    let body = serde_json::to_vec(&json!({"action":"synchronize","number":7,
        "installation":{"id":789},"repository":{"id":42,"full_name":"francis-du/wcode"},
        "pull_request":target}))
    .unwrap();
    let key = b"inbox-native-fixture-key-not-live";
    let tag = ring::hmac::sign(&ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key), &body);
    let signature = format!(
        "sha256={}",
        tag.as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
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
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().canonicalize().unwrap().join("inbox");
    let inbox = GitHubInbox::initialize(&root, config(), 789).unwrap();
    inbox.enqueue(&headers, &body, key).unwrap();
    let mut success = native_check(&native.head, Some(ready.record().digest()));
    success["conclusion"] = json!("success");
    let fixture = fixture(vec![
        reply(target.clone()),
        reply(classic()),
        reply(json!([])),
        reply(target.clone()),
        reply(target.clone()),
        created(native_check(&native.head, None)),
        reply(target.clone()),
        reply(success.clone()),
        reply(target),
        reply(success),
    ])
    .await;
    let result = inbox
        .publish_next(
            &fixture.provider,
            native.root.path(),
            trusted_native_fixture::ID,
            key,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.verdict(), GateVerdict::Ready);
    assert_eq!(result.record_digest(), Some(ready.record().digest()));
    assert_eq!(
        fixture.requests.lock().unwrap()[5].body["conclusion"],
        "failure"
    );
    assert_eq!(
        fixture.requests.lock().unwrap()[7].body["conclusion"],
        "success"
    );
    let reopened = GitHubInbox::open(&root, config(), 789).unwrap();
    assert_eq!(
        reopened.status().unwrap()["entries"][0]["state"],
        "completed"
    );
    assert_eq!(reopened.status().unwrap()["current_acceptance"], false);
    assert!(reopened.enqueue(&headers, &body, key).unwrap().duplicate);
    assert!(reopened
        .publish_next(
            &fixture.provider,
            native.root.path(),
            trusted_native_fixture::ID,
            key
        )
        .await
        .unwrap()
        .is_none());
    assert_eq!(fixture.requests.lock().unwrap().len(), 10);
}

#[tokio::test]
async fn github_signed_delivery_uses_native_verification_and_red_first_publication() {
    let native = trusted_native_fixture::NativeFixture::new();
    native.activate();
    let ready = native.verify_and_review().await;
    let mut target = native_pull(&native.base, &native.head);
    target["base"]["ref"] = json!("main");
    let delivery = signed_delivery(target.clone());
    let mut success = native_check(&native.head, Some(ready.record().digest()));
    success["conclusion"] = json!("success");
    let fixture = fixture(vec![
        reply(target.clone()),
        reply(classic()),
        reply(json!([])),
        reply(target.clone()),
        reply(target.clone()),
        created(native_check(&native.head, None)),
        reply(target.clone()),
        reply(success.clone()),
        reply(target),
        reply(success),
    ])
    .await;
    let receipt = fixture
        .provider
        .publish_delivery(&delivery, native.root.path(), trusted_native_fixture::ID)
        .await
        .unwrap();
    assert_eq!(receipt.verdict(), GateVerdict::Ready);
    assert_eq!(receipt.record_digest(), Some(ready.record().digest()));
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 10);
    assert_eq!(requests[5].body["conclusion"], "failure");
    assert_eq!(requests[7].body["conclusion"], "success");
    assert_eq!(
        requests[7].body["external_id"],
        format!("wcode:{}", ready.record().digest())
    );
    assert!(requests[..5].iter().all(|request| request.method == "GET"));
}

#[tokio::test]
async fn github_signed_delivery_stale_event_cannot_retarget_or_write_a_check() {
    let delivery = signed_delivery(candidate());
    let mut newer = candidate();
    newer["head"]["sha"] = json!("c".repeat(40));
    let fixture = fixture(vec![
        reply(newer.clone()),
        reply(classic()),
        reply(json!([])),
        reply(newer),
    ])
    .await;
    let root = tempfile::tempdir().unwrap();
    let error = fixture
        .provider
        .publish_delivery(&delivery, root.path(), "project")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("explicitly requested candidate"));
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    assert!(requests.iter().all(|request| request.method == "GET"));
}

fn candidate() -> Value {
    let mut value = pull();
    value["base"]["ref"] = json!("release/stable");
    value
}
fn classic() -> Value {
    json!({"enforce_admins":{"enabled":true},"required_status_checks":{
        "strict":true,"contexts":["wcode/change-acceptance"],
        "checks":[{"context":"wcode/change-acceptance","app_id":99}]}})
}
fn rule() -> Value {
    json!({"type":"required_status_checks", "ruleset_id":3,
        "ruleset_source":"francis-du", "ruleset_source_type":"Organization",
        "parameters":{"strict_required_status_checks_policy":true,
            "required_status_checks":[{"context":"wcode/change-acceptance","integration_id":99}]}})
}
fn ruleset() -> Value {
    json!({"id":3,"enforcement":"active","target":"branch", "source":"francis-du",
        "source_type":"Organization", "bypass_actors":[],
        "rules":[{"type":"required_status_checks","parameters":rule()["parameters"]}]})
}
fn unavailable(status: u16) -> Reply {
    Reply {
        status,
        body: b"PRIVATE-REMOTE-RESPONSE".to_vec(),
        location: None,
    }
}

#[tokio::test]
async fn github_preflighted_publication_uses_real_native_ready_and_exact_requested_candidate() {
    let native = trusted_native_fixture::NativeFixture::new();
    native.activate();
    let ready = native.verify_and_review().await;
    let mut target = native_pull(&native.base, &native.head);
    target["base"]["ref"] = json!("main");
    let mut success = native_check(&native.head, Some(ready.record().digest()));
    success["conclusion"] = json!("success");
    let fixture = fixture(vec![
        reply(target.clone()),
        reply(classic()),
        reply(json!([])),
        reply(target.clone()),
        reply(target.clone()),
        created(native_check(&native.head, None)),
        reply(target.clone()),
        reply(success.clone()),
        reply(target),
        reply(success),
    ])
    .await;
    let receipt = fixture
        .provider
        .publish_preflighted(
            7,
            (&native.base, &native.head),
            native.root.path(),
            trusted_native_fixture::ID,
        )
        .await
        .unwrap();
    assert_eq!(receipt.verdict(), GateVerdict::Ready);
    assert_eq!(receipt.record_digest(), Some(ready.record().digest()));
    assert!(receipt.check().strictly_successful());
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 10);
    assert!(requests[..4].iter().all(|request| request.method == "GET"));
    assert_eq!(requests[5].body["conclusion"], "failure");
    assert_eq!(requests[7].body["conclusion"], "success");
    assert_eq!(
        requests[7].body["external_id"],
        format!("wcode:{}", ready.record().digest())
    );
}

#[tokio::test]
async fn github_preflighted_publication_rejects_stale_event_before_writes() {
    let fixture = fixture(vec![
        reply(candidate()),
        reply(classic()),
        reply(json!([])),
        reply(candidate()),
    ])
    .await;
    let root = tempfile::tempdir().unwrap();
    let error = fixture
        .provider
        .publish_preflighted(
            7,
            (&"a".repeat(40), &"c".repeat(40)),
            root.path(),
            "project",
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("explicitly requested candidate"));
    assert_eq!(fixture.requests.lock().unwrap().len(), 4);
    assert!(fixture
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|request| request.method == "GET"));
}

#[tokio::test]
async fn github_preflighted_publication_candidate_drift_cannot_retarget_green() {
    let mut changed = candidate();
    changed["head"]["sha"] = json!("c".repeat(40));
    let mut denied = check();
    denied["head_sha"] = json!("c".repeat(40));
    let fixture = fixture(vec![
        reply(candidate()),
        reply(classic()),
        reply(json!([])),
        reply(candidate()),
        reply(changed),
        created(denied),
    ])
    .await;
    let root = tempfile::tempdir().unwrap();
    assert!(fixture
        .provider
        .publish_preflighted(
            7,
            (&"a".repeat(40), &"b".repeat(40)),
            root.path(),
            "project",
        )
        .await
        .is_err());
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 5);
    assert!(requests.iter().all(|request| request.method == "GET"));
}

#[tokio::test]
async fn github_preflight_observes_classic_binding_without_writes_or_authority() {
    let fixture = fixture(vec![
        reply(candidate()),
        reply(classic()),
        reply(json!([])),
        reply(candidate()),
    ])
    .await;
    let report = fixture.provider.preflight(7).await.unwrap();
    assert!(report.configuration_verified);
    assert_eq!(report.protection_source, Some("classic_branch_protection"));
    assert!(!report.remote_mutations);
    assert!(!report.acceptance_evaluated);
    assert!(!report.credential_write_permission_verified);
    assert!(!report.deployment_isolation_verified);
    let text = serde_json::to_string(&report).unwrap();
    assert!(!text.contains("TEST-PUBLISHER-TOKEN"));
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    assert!(requests
        .iter()
        .all(|request| request.method == "GET" && request.body.is_null()));
    assert!(requests[1].path.contains("release%2Fstable"));
}

#[tokio::test]
async fn github_preflight_context_only_wrong_app_loose_base_and_admin_bypass_fail_closed() {
    let mut variants = Vec::new();
    for id in [Value::Null, json!(-1), json!(0), json!(100), json!("99")] {
        let mut value = classic();
        value["required_status_checks"]["checks"][0]["app_id"] = id;
        variants.push(value);
    }
    let mut loose = classic();
    loose["required_status_checks"]["strict"] = json!(false);
    variants.push(loose);
    let mut bypass = classic();
    bypass["enforce_admins"]["enabled"] = json!(false);
    variants.push(bypass);
    let mut legacy = classic();
    legacy["required_status_checks"]
        .as_object_mut()
        .unwrap()
        .remove("checks");
    variants.push(legacy);
    let mut conflict = classic();
    conflict["required_status_checks"]["checks"]
        .as_array_mut()
        .unwrap()
        .push(json!({"context":"wcode/change-acceptance","app_id":1}));
    variants.push(conflict);
    for value in variants {
        let fixture = fixture(vec![reply(candidate()), reply(value), reply(json!([]))]).await;
        let report = fixture.provider.preflight(7).await.unwrap();
        assert!(!report.configuration_verified);
        assert!(fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|request| request.method == "GET"));
    }
}

#[tokio::test]
async fn github_preflight_accepts_active_inherited_ruleset_only_with_visible_empty_bypass() {
    let fixture = fixture(vec![
        reply(candidate()),
        unavailable(404),
        reply(json!([rule()])),
        reply(ruleset()),
        reply(candidate()),
    ])
    .await;
    let report = fixture.provider.preflight(7).await.unwrap();
    assert!(report.configuration_verified);
    assert_eq!(report.protection_source, Some("active_ruleset"));
    assert_eq!(report.ruleset_id, Some(3));
    assert!(fixture
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|request| request.method == "GET"));
}

#[tokio::test]
async fn github_preflight_hidden_bypass_disabled_or_changed_ruleset_is_not_proof() {
    let mut variants = Vec::new();
    for bypass in [
        Value::Null,
        json!([{"actor_type":"OrganizationAdmin","bypass_mode":"always"}]),
    ] {
        let mut value = ruleset();
        value["bypass_actors"] = bypass;
        variants.push(value);
    }
    for (key, value) in [
        ("id", json!(4)),
        ("enforcement", json!("evaluate")),
        ("target", json!("tag")),
        ("source", json!("another-org")),
        ("source_type", json!("Repository")),
    ] {
        let mut detail = ruleset();
        detail[key] = value;
        variants.push(detail);
    }
    let mut changed = ruleset();
    changed["rules"][0]["parameters"]["required_status_checks"][0]["integration_id"] = json!(100);
    variants.push(changed);
    for detail in variants {
        let fixture = fixture(vec![
            reply(candidate()),
            unavailable(404),
            reply(json!([rule()])),
            reply(detail),
        ])
        .await;
        assert!(
            !fixture
                .provider
                .preflight(7)
                .await
                .unwrap()
                .configuration_verified
        );
        assert_eq!(fixture.requests.lock().unwrap().len(), 4);
    }
}

#[tokio::test]
async fn github_preflight_rejects_merge_queue_partial_rules_and_candidate_drift() {
    for rules in [
        json!([{"type":"merge_queue","ruleset_id":8}]),
        json!({}),
        json!([rule(), rule()]),
        json!([{"type":"required_status_checks"}]),
    ] {
        let fixture = fixture(vec![reply(candidate()), reply(classic()), reply(rules)]).await;
        assert!(
            !fixture
                .provider
                .preflight(7)
                .await
                .unwrap()
                .configuration_verified
        );
    }
    for field in ["sha", "ref"] {
        let mut changed = candidate();
        changed["base"][field] = if field == "sha" {
            json!("c".repeat(40))
        } else {
            json!("different")
        };
        let fixture = fixture(vec![
            reply(candidate()),
            reply(classic()),
            reply(json!([])),
            reply(changed),
        ])
        .await;
        assert!(
            !fixture
                .provider
                .preflight(7)
                .await
                .unwrap()
                .configuration_verified
        );
    }
}

#[tokio::test]
async fn github_preflight_pages_all_effective_rules_and_refuses_unbounded_scan() {
    let batch = |start| {
        json!((start..start + 100)
            .map(|id| json!({
        "type":"commit_message_pattern","ruleset_id":id}))
            .collect::<Vec<_>>())
    };
    let first = fixture(vec![
        reply(candidate()),
        reply(classic()),
        reply(batch(1)),
        reply(json!([])),
        reply(candidate()),
    ])
    .await;
    assert!(
        first
            .provider
            .preflight(7)
            .await
            .unwrap()
            .configuration_verified
    );
    assert_eq!(first.requests.lock().unwrap().len(), 5);
    let mut replies = vec![reply(candidate()), reply(classic())];
    replies.extend((0..5).map(|page| reply(batch(1 + page * 100))));
    let fixture = fixture(replies).await;
    assert!(
        !fixture
            .provider
            .preflight(7)
            .await
            .unwrap()
            .configuration_verified
    );
    assert_eq!(fixture.requests.lock().unwrap().len(), 7);
}

#[tokio::test]
async fn github_preflight_transport_errors_and_oversized_bodies_never_leak_or_verify() {
    for response in [
        unavailable(401),
        unavailable(403),
        unavailable(429),
        unavailable(503),
        Reply {
            status: 302,
            body: Vec::new(),
            location: Some("https://example.invalid/sink"),
        },
        Reply {
            status: 200,
            body: vec![b'x'; MAX_RESPONSE_BYTES + 1],
            location: None,
        },
    ] {
        let fixture = fixture(vec![reply(candidate()), reply(classic()), response]).await;
        let report = fixture.provider.preflight(7).await.unwrap();
        assert!(!report.configuration_verified);
        let text = serde_json::to_string(&report).unwrap();
        assert!(!text.contains("PRIVATE-REMOTE"));
        assert!(!text.contains("TEST-PUBLISHER-TOKEN"));
        assert_eq!(fixture.requests.lock().unwrap().len(), 3);
    }
}

#[tokio::test]
async fn github_preflight_missing_branch_and_other_repository_are_rejected() {
    let mut foreign = candidate();
    foreign["base"]["repo"]["id"] = json!(999);
    let mut traversal = candidate();
    traversal["base"]["ref"] = json!("../admin");
    for value in [pull(), foreign, traversal] {
        let fixture = fixture(vec![reply(value)]).await;
        assert!(fixture.provider.preflight(7).await.is_err());
        assert_eq!(fixture.requests.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn github_preflight_publication_rejects_bad_configuration_before_any_write() {
    let fixture = fixture(vec![reply(candidate()), reply(json!({})), reply(json!([]))]).await;
    let root = tempfile::tempdir().unwrap();
    assert!(fixture
        .provider
        .publish_preflighted(
            7,
            (&"a".repeat(40), &"b".repeat(40)),
            root.path(),
            "project"
        )
        .await
        .is_err());
    assert!(fixture
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|request| request.method == "GET"));
}

#[tokio::test]
async fn github_preflight_matching_configuration_does_not_replace_native_acceptance() {
    let fixture = fixture(vec![
        reply(candidate()),
        reply(classic()),
        reply(json!([])),
        reply(candidate()),
        reply(candidate()),
        created(check()),
        unavailable(503),
    ])
    .await;
    let root = tempfile::tempdir().unwrap();
    assert!(fixture
        .provider
        .publish_preflighted(
            7,
            (&"a".repeat(40), &"b".repeat(40)),
            root.path(),
            "project"
        )
        .await
        .is_err());
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 7);
    assert_eq!(requests[5].body["conclusion"], "failure");
    assert!(requests
        .iter()
        .all(|request| request.body["conclusion"] != "success"));
}
