use super::*;
use trusted_native_fixture::{NativeFixture, ID};

struct Remote {
    target: Value,
    check: Option<Value>,
    writes: Vec<Value>,
    creates: usize,
    offline: bool,
    protection: bool,
    unconfirmed_green: bool,
    wrong_app: bool,
}
struct WatchFixture {
    provider: GitHubProvider,
    state: Arc<Mutex<Remote>>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for WatchFixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
async fn remote(native: &NativeFixture) -> WatchFixture {
    async fn handler(State(state): State<Arc<Mutex<Remote>>>, request: Request) -> Response<Body> {
        let path = request.uri().path().to_owned();
        let method = request.method().clone();
        let bytes = to_bytes(request.into_body(), 64 * 1024).await.unwrap();
        let mut state = state.lock().unwrap();
        let mut status = HttpStatus::OK;
        let value = if state.offline {
            status = HttpStatus::SERVICE_UNAVAILABLE;
            json!({"error":"PRIVATE-REMOTE-ERROR"})
        } else if path.ends_with("/pulls/7") && method == Method::GET {
            state.target.clone()
        } else if path.ends_with("/branches/main/protection") {
            json!({"enforce_admins":{"enabled":true}, "required_status_checks":{
                "strict":state.protection, "checks":[{"context":"wcode/change-acceptance","app_id":99}]}})
        } else if path.ends_with("/rules/branches/main") {
            json!([])
        } else if path.ends_with("/check-runs") && method == Method::POST {
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            state.writes.push(body.clone());
            state.creates += 1;
            let mut check = body;
            check["id"] = json!(9);
            check["app"] = json!({"id":99});
            state.check = Some(check.clone());
            status = HttpStatus::CREATED;
            check
        } else if path.ends_with("/check-runs/9") {
            if method == Method::PATCH {
                let body: Value = serde_json::from_slice(&bytes).unwrap();
                state.writes.push(body.clone());
                let check = state.check.as_mut().unwrap();
                for (name, value) in body.as_object().unwrap() {
                    assert_ne!(name, "head_sha", "an existing Check cannot be retargeted");
                    check[name] = value.clone();
                }
                if state.unconfirmed_green && body["conclusion"] == "success" {
                    state.unconfirmed_green = false;
                    status = HttpStatus::SERVICE_UNAVAILABLE;
                }
            } else {
                assert_eq!(method, Method::GET);
            }
            let mut check = state.check.clone().unwrap();
            if state.wrong_app {
                check["app"]["id"] = json!(100);
            }
            check
        } else {
            panic!("unexpected watcher request {method} {path}");
        };
        Response::builder()
            .status(status)
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&value).unwrap()))
            .unwrap()
    }
    let mut target = native_pull(&native.base, &native.head);
    target["base"]["ref"] = json!("main");
    let state = Arc::new(Mutex::new(Remote {
        target,
        check: None,
        writes: Vec::new(),
        creates: 0,
        offline: false,
        protection: true,
        unconfirmed_green: false,
        wrong_app: false,
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let router = Router::new()
        .fallback(any(handler))
        .with_state(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    WatchFixture {
        provider: GitHubProvider::client(config(), "NOT-LIVE-WATCH-TOKEN", api, false).unwrap(),
        state,
        server,
    }
}
fn revoke_policy(native: &NativeFixture) {
    let revision = native.harness.current_revision(&native.workspace).unwrap();
    native
        .harness
        .acceptance_policy_revoke_authorized(
            ID,
            &native.workspace,
            1,
            &revision,
            trusted_native_fixture::operator_receipt(),
        )
        .unwrap();
}
fn assert_red(fixture: &WatchFixture) {
    assert_eq!(
        fixture.state.lock().unwrap().check.as_ref().unwrap()["conclusion"],
        "failure"
    );
}

#[tokio::test]
async fn github_watch_rejects_competing_watchers_without_replacing_live_check() {
    let native = NativeFixture::new();
    native.activate();
    native.verify_and_review().await;
    let fixture = remote(&native).await;
    let other = GitHubProvider::client(
        config(),
        "ANOTHER-NOT-LIVE-TOKEN",
        fixture.provider.api.clone(),
        false,
    )
    .unwrap();
    let mut first = fixture
        .provider
        .watch_candidate(7, (&native.base, &native.head), native.root.path(), ID)
        .unwrap();
    assert_eq!(first.refresh().await.unwrap().verdict(), GateVerdict::Ready);
    let writes = fixture.state.lock().unwrap().writes.len();
    let mut second = other
        .watch_candidate(7, (&native.base, &native.head), native.root.path(), ID)
        .unwrap();
    assert!(
        second.refresh().await.is_err(),
        "a second publisher replaced a live Check"
    );
    assert_eq!(fixture.state.lock().unwrap().writes.len(), writes);
    assert_eq!(fixture.state.lock().unwrap().creates, 1);
    drop(first);
    assert!(
        second.refresh().await.is_ok(),
        "dropping the owner did not release local admission"
    );
    assert_eq!(fixture.state.lock().unwrap().creates, 2);
}

#[tokio::test]
async fn github_watch_rejects_one_shot_and_trait_publication_while_owned() {
    let native = NativeFixture::new();
    native.activate();
    native.verify_and_review().await;
    let fixture = remote(&native).await;
    let mut watch = fixture
        .provider
        .watch_candidate(7, (&native.base, &native.head), native.root.path(), ID)
        .unwrap();
    watch.refresh().await.unwrap();
    let writes = fixture.state.lock().unwrap().writes.len();
    assert!(
        fixture.provider.publish_unavailable(7).await.is_err(),
        "denial bypassed the live owner"
    );
    assert!(fixture
        .provider
        .publish_local(7, native.root.path(), ID)
        .await
        .is_err());
    assert!(fixture
        .provider
        .publish_preflighted(7, (&native.base, &native.head), native.root.path(), ID,)
        .await
        .is_err());
    let target = fixture.provider.read_target(7).await.unwrap();
    let denied = NativePublication::unavailable(target).unwrap();
    assert!(
        fixture.provider.write_check(&denied).await.is_err(),
        "trait path bypassed admission"
    );
    assert_eq!(fixture.state.lock().unwrap().writes.len(), writes);
    assert!(
        fixture
            .provider
            .preflight(7)
            .await
            .unwrap()
            .configuration_verified,
        "read-only diagnostics must not need a publication slot"
    );
    assert!(watch.revoke().await.unwrap());
    assert_red(&fixture);
}

#[tokio::test]
async fn github_watch_busy_inbox_keeps_attempts_and_backoff_unchanged() {
    let native = NativeFixture::new();
    native.activate();
    native.verify_and_review().await;
    let fixture = remote(&native).await;
    let mut watch = fixture
        .provider
        .watch_candidate(7, (&native.base, &native.head), native.root.path(), ID)
        .unwrap();
    watch.refresh().await.unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let inbox = GitHubInbox::initialize(
        &temporary.path().canonicalize().unwrap().join("inbox"),
        config(),
        56,
    )
    .unwrap();
    let key = b"watch-inbox-local-fixture-key";
    let body = serde_json::to_vec(&json!({
        "action":"synchronize", "number":7, "installation":{"id":56},
        "repository":{"id":42,"full_name":"francis-du/wcode"},
        "pull_request":fixture.state.lock().unwrap().target.clone(),
    }))
    .unwrap();
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
    inbox.enqueue(&headers, &body, key).unwrap();
    let before = inbox.status().unwrap();
    let writes = fixture.state.lock().unwrap().writes.len();
    for _ in 0..7 {
        assert!(inbox
            .publish_next(&fixture.provider, native.root.path(), ID, key)
            .await
            .is_err());
        assert_eq!(
            inbox.status().unwrap(),
            before,
            "local contention consumed a delivery attempt"
        );
    }
    assert!(inbox.enqueue(&headers, &body, key).unwrap().duplicate);
    assert_eq!(fixture.state.lock().unwrap().writes.len(), writes);
    drop(watch);
    assert_eq!(
        inbox
            .publish_next(&fixture.provider, native.root.path(), ID, key)
            .await
            .unwrap()
            .unwrap()
            .verdict(),
        GateVerdict::Ready
    );
    assert_eq!(inbox.status().unwrap()["entries"][0]["attempts"], 1);
    assert_eq!(inbox.status().unwrap()["entries"][0]["state"], "completed");
}

#[tokio::test]
async fn github_watch_native_verification_and_policy_changes_reuse_one_check() {
    let native = NativeFixture::new();
    native.activate();
    let fixture = remote(&native).await;
    let mut watch = fixture
        .provider
        .watch_candidate(7, (&native.base, &native.head), native.root.path(), ID)
        .unwrap();
    assert_eq!(
        watch.refresh().await.unwrap().verdict(),
        GateVerdict::Blocked
    );
    assert_red(&fixture);
    let ready = native.verify_and_review().await;
    let receipt = watch.refresh().await.unwrap();
    assert_eq!(receipt.verdict(), GateVerdict::Ready);
    assert_eq!(receipt.record_digest(), Some(ready.record().digest()));
    let count = fixture.state.lock().unwrap().writes.len();
    assert_eq!(watch.refresh().await.unwrap().verdict(), GateVerdict::Ready);
    assert_eq!(
        fixture.state.lock().unwrap().writes.len(),
        count,
        "unchanged proof caused remote churn"
    );
    revoke_policy(&native);
    let result = watch.refresh().await;
    assert!(!result
        .as_ref()
        .is_ok_and(|r| r.verdict() == GateVerdict::Ready));
    assert_red(&fixture);
    assert_eq!(fixture.state.lock().unwrap().creates, 1);
    // Restoring Policy alone must not resurrect the old native verification.
    let snapshot = native
        .harness
        .acceptance_policy_preview(ID, &native.workspace)
        .unwrap();
    native
        .harness
        .acceptance_policy_activate_authorized(
            ID,
            &native.workspace,
            2,
            snapshot,
            trusted_native_fixture::operator_receipt(),
            None,
        )
        .unwrap();
    assert_eq!(
        watch.refresh().await.unwrap().verdict(),
        GateVerdict::Blocked
    );
    native.verify_and_review().await;
    assert_eq!(watch.refresh().await.unwrap().verdict(), GateVerdict::Ready);
    assert!(watch.revoke().await.unwrap());
    assert_red(&fixture);
    assert_eq!(fixture.state.lock().unwrap().creates, 1);
}

#[tokio::test]
async fn github_watch_stale_target_or_protection_revokes_only_its_original_check() {
    for drift_target in [false, true] {
        let native = NativeFixture::new();
        native.activate();
        native.verify_and_review().await;
        let fixture = remote(&native).await;
        let mut watch = fixture
            .provider
            .watch_candidate(7, (&native.base, &native.head), native.root.path(), ID)
            .unwrap();
        assert_eq!(watch.refresh().await.unwrap().verdict(), GateVerdict::Ready);
        {
            let mut state = fixture.state.lock().unwrap();
            if drift_target {
                state.target["head"]["sha"] = json!("c".repeat(40));
            } else {
                state.protection = false;
            }
        }
        let error = watch.refresh().await.unwrap_err();
        assert!(error.to_string().contains("denial confirmed"));
        assert_red(&fixture);
        let state = fixture.state.lock().unwrap();
        assert_eq!(state.creates, 1);
        assert_eq!(state.check.as_ref().unwrap()["head_sha"], native.head);
    }
}

#[tokio::test]
async fn github_watch_outage_does_not_return_cached_green_or_claim_revocation() {
    let native = NativeFixture::new();
    native.activate();
    native.verify_and_review().await;
    let fixture = remote(&native).await;
    let mut watch = fixture
        .provider
        .watch_candidate(7, (&native.base, &native.head), native.root.path(), ID)
        .unwrap();
    watch.refresh().await.unwrap();
    fixture.state.lock().unwrap().offline = true;
    let error = watch.refresh().await.unwrap_err().to_string();
    assert!(error.contains("denial unconfirmed"));
    assert!(!error.contains("PRIVATE-REMOTE"));
    // A genuine outage may leave the old success remotely; no false rollback.
    assert_eq!(
        fixture.state.lock().unwrap().check.as_ref().unwrap()["conclusion"],
        "success"
    );
    revoke_policy(&native);
    fixture.state.lock().unwrap().offline = false;
    assert!(!watch
        .refresh()
        .await
        .as_ref()
        .is_ok_and(|r| r.verdict() == GateVerdict::Ready));
    assert_red(&fixture);
    assert_eq!(fixture.state.lock().unwrap().creates, 1);
}

#[tokio::test]
async fn github_watch_unconfirmed_green_retains_check_identity_for_denial_and_retry() {
    let native = NativeFixture::new();
    native.activate();
    native.verify_and_review().await;
    let fixture = remote(&native).await;
    fixture.state.lock().unwrap().unconfirmed_green = true;
    let mut watch = fixture
        .provider
        .watch_candidate(7, (&native.base, &native.head), native.root.path(), ID)
        .unwrap();
    assert!(watch.refresh().await.is_err());
    assert_red(&fixture);
    assert_eq!(fixture.state.lock().unwrap().creates, 1);
    assert_eq!(watch.refresh().await.unwrap().verdict(), GateVerdict::Ready);
    assert_eq!(fixture.state.lock().unwrap().creates, 1);
}

#[tokio::test]
async fn github_watch_refuses_foreign_check_metadata_without_mutating_it() {
    let native = NativeFixture::new();
    native.activate();
    native.verify_and_review().await;
    let fixture = remote(&native).await;
    let mut watch = fixture
        .provider
        .watch_candidate(7, (&native.base, &native.head), native.root.path(), ID)
        .unwrap();
    watch.refresh().await.unwrap();
    let count = fixture.state.lock().unwrap().writes.len();
    fixture.state.lock().unwrap().wrong_app = true;
    assert!(watch
        .refresh()
        .await
        .unwrap_err()
        .to_string()
        .contains("denial unconfirmed"));
    assert_eq!(fixture.state.lock().unwrap().writes.len(), count);
}

#[tokio::test]
async fn github_watch_foreground_rechecks_after_proof_change_and_denies_on_shutdown() {
    use tokio::sync::oneshot;
    let native = NativeFixture::new();
    native.activate();
    native.verify_and_review().await;
    let fixture = remote(&native).await;
    let mut watch = fixture
        .provider
        .watch_candidate(7, (&native.base, &native.head), native.root.path(), ID)
        .unwrap();
    let (stop, stopped) = oneshot::channel();
    let mut stop = Some(stop);
    let mut events = Vec::new();
    let mut observations = 0;
    tokio::time::timeout(
        Duration::from_secs(15),
        watch.run_with_delay(
            async {
                stopped
                    .await
                    .map_err(|_| anyhow!("test stop channel closed"))
            },
            |event| {
                if event["event"] == "candidate_observed"
                    || event["event"] == "candidate_unavailable"
                {
                    observations += 1;
                    if observations == 1 {
                        assert_eq!(event["receipt"]["verdict"], "ready");
                        revoke_policy(&native);
                    } else {
                        assert_ne!(event["receipt"]["verdict"], "ready");
                        stop.take().unwrap().send(()).unwrap();
                    }
                }
                events.push(event);
                Ok(())
            },
            Duration::from_millis(20),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(observations, 2);
    assert_eq!(events.last().unwrap()["event"], "candidate_watch_stopped");
    assert_eq!(events.last().unwrap()["owned_check_denied"], true);
    assert_red(&fixture);
    assert_eq!(fixture.state.lock().unwrap().creates, 1);
    // A completed run releases admission even while its controller object still
    // exists. It must not retain a stale Check ID for a later run.
    let receipt = fixture.provider.publish_unavailable(7).await.unwrap();
    assert_eq!(receipt.verdict(), GateVerdict::Blocked);
    assert_eq!(fixture.state.lock().unwrap().creates, 2);
}

#[tokio::test]
async fn github_watch_ready_shutdown_or_invalid_input_never_starts_work() {
    let native = NativeFixture::new();
    let fixture = remote(&native).await;
    for (base, head) in [
        ("short".to_owned(), native.head.clone()),
        (native.base.clone(), "b".repeat(64)),
    ] {
        assert!(fixture
            .provider
            .watch_candidate(7, (&base, &head), native.root.path(), ID)
            .is_err());
    }
    assert!(fixture
        .provider
        .watch_candidate(0, (&native.base, &native.head), native.root.path(), ID)
        .is_err());
    assert!(fixture
        .provider
        .watch_candidate(7, (&native.base, &native.head), native.root.path(), " ")
        .is_err());
    let mut watch = fixture
        .provider
        .watch_candidate(7, (&native.base, &native.head), native.root.path(), ID)
        .unwrap();
    let mut events = Vec::new();
    watch
        .run(std::future::ready(Ok(())), |event| {
            events.push(event);
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[1]["owned_check_denied"], false);
    assert_eq!(fixture.state.lock().unwrap().creates, 0);
}

#[tokio::test]
async fn github_watch_shutdown_outage_is_not_reported_as_safe_stop() {
    let native = NativeFixture::new();
    native.activate();
    native.verify_and_review().await;
    let fixture = remote(&native).await;
    let mut watch = fixture
        .provider
        .watch_candidate(7, (&native.base, &native.head), native.root.path(), ID)
        .unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let mut stop = Some(stop);
    let mut events = Vec::new();
    let error = watch
        .run(
            async { stopped.await.map_err(|_| anyhow!("test stop lost")) },
            |event| {
                if event["event"] == "candidate_observed" {
                    fixture.state.lock().unwrap().offline = true;
                    stop.take().unwrap().send(()).unwrap();
                }
                events.push(event);
                Ok(())
            },
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("shutdown denial unconfirmed"));
    assert!(events
        .iter()
        .all(|event| event["event"] != "candidate_watch_stopped"));
    assert_eq!(
        fixture.state.lock().unwrap().check.as_ref().unwrap()["conclusion"],
        "success"
    );
}

#[tokio::test]
async fn github_watch_output_failure_still_denies_owned_green() {
    let native = NativeFixture::new();
    native.activate();
    native.verify_and_review().await;
    let fixture = remote(&native).await;
    let mut watch = fixture
        .provider
        .watch_candidate(7, (&native.base, &native.head), native.root.path(), ID)
        .unwrap();
    let error = watch
        .run(std::future::pending(), |event| {
            if event["event"] == "candidate_observed" {
                Err(anyhow!("test sink unavailable"))
            } else {
                Ok(())
            }
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("sink unavailable"));
    assert_red(&fixture);
    assert_eq!(fixture.state.lock().unwrap().creates, 1);
}
