use super::*;

#[path = "preflight.rs"]
mod preflight;
#[path = "watch.rs"]
mod watch;
use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::{Response, StatusCode as HttpStatus};
use axum::routing::any;
use axum::Router;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct FixtureState {
    replies: Arc<Mutex<VecDeque<Reply>>>,
    requests: Arc<Mutex<Vec<Captured>>>,
}
struct Reply {
    status: u16,
    body: Vec<u8>,
    location: Option<&'static str>,
}
struct Captured {
    method: String,
    path: String,
    body: Value,
    authenticated: bool,
    version: String,
}
struct Fixture {
    provider: GitHubProvider,
    requests: Arc<Mutex<Vec<Captured>>>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
fn reply(value: Value) -> Reply {
    Reply {
        status: 200,
        body: serde_json::to_vec(&value).unwrap(),
        location: None,
    }
}
fn config() -> GitHubConfig {
    GitHubConfig::new(
        ProviderRepository::new("francis-du", "wcode").unwrap(),
        42,
        99,
        "wcode/change-acceptance",
    )
    .unwrap()
}
async fn fixture(replies: Vec<Reply>) -> Fixture {
    async fn handler(State(state): State<FixtureState>, request: Request) -> Response<Body> {
        let authenticated = request.headers().contains_key(AUTHORIZATION);
        let version = request
            .headers()
            .get("X-GitHub-Api-Version")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let method = request.method().to_string();
        let path = request.uri().path().to_owned();
        let bytes = to_bytes(request.into_body(), 64 * 1024).await.unwrap();
        state.requests.lock().unwrap().push(Captured {
            method,
            path,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            authenticated,
            version,
        });
        let response = state.replies.lock().unwrap().pop_front().unwrap_or(Reply {
            status: 500,
            body: b"{}".to_vec(),
            location: None,
        });
        let mut builder = Response::builder()
            .status(HttpStatus::from_u16(response.status).unwrap())
            .header("Content-Type", "application/json")
            .header("Content-Length", response.body.len().to_string());
        if let Some(location) = response.location {
            builder = builder.header("Location", location);
        }
        builder.body(Body::from(response.body)).unwrap()
    }
    let state = FixtureState {
        replies: Arc::new(Mutex::new(replies.into())),
        requests: Arc::new(Mutex::new(Vec::new())),
    };
    let requests = state.requests.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().fallback(any(handler)).with_state(state),
        )
        .await
        .unwrap();
    });
    Fixture {
        provider: GitHubProvider::client(config(), "TEST-PUBLISHER-TOKEN", api, false).unwrap(),
        requests,
        server,
    }
}
fn pull() -> Value {
    json!({"number":7,"state":"open","draft":false,"merged":false,
        "base":{"sha":"a".repeat(40),"repo":{"id":42,"full_name":"francis-du/wcode"}},
        "head":{"sha":"b".repeat(40),"repo":{"id":42,"full_name":"francis-du/wcode"}}})
}
fn check() -> Value {
    json!({"id":9,"head_sha":"b".repeat(40),"name":"wcode/change-acceptance","app":{"id":99},
        "external_id":"wcode:unavailable","status":"completed","conclusion":"failure"})
}
fn created(value: Value) -> Reply {
    let mut value = reply(value);
    value.status = 201;
    value
}

#[tokio::test]
async fn github_gate_http_denial_posts_exact_sha_and_never_neutral() {
    let fixture = fixture(vec![reply(pull()), created(check())]).await;
    let result = fixture.provider.publish_unavailable(7).await.unwrap();
    assert_eq!(result.verdict(), GateVerdict::Blocked);
    assert!(!result.check().strictly_successful());
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/repos/francis-du/wcode/pulls/7");
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/repos/francis-du/wcode/check-runs");
    assert_eq!(requests[1].body["head_sha"], "b".repeat(40));
    assert_eq!(requests[1].body["status"], "completed");
    assert_eq!(requests[1].body["conclusion"], "failure");
    assert!(requests
        .iter()
        .all(|request| request.authenticated && request.version == API_VERSION));
    assert!(!format!("{:?}", fixture.provider).contains("TEST-PUBLISHER-TOKEN"));
}

#[tokio::test]
async fn github_gate_outage_malformed_and_redirect_are_unavailable() {
    for response in [
        Reply {
            status: 503,
            body: b"PRIVATE-RESPONSE".to_vec(),
            location: None,
        },
        Reply {
            status: 200,
            body: b"PRIVATE-NOT-JSON".to_vec(),
            location: None,
        },
        Reply {
            status: 302,
            body: Vec::new(),
            location: Some("https://example.invalid/credential-sink"),
        },
    ] {
        let fixture = fixture(vec![response]).await;
        let error = fixture.provider.publish_unavailable(7).await.unwrap_err();
        assert!(!format!("{error:#}").contains("PRIVATE-"));
        assert_eq!(fixture.requests.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn github_gate_rejects_unknown_draft_closed_and_repository_drift() {
    let mut variants = Vec::new();
    let mut value = pull();
    value["draft"] = json!(true);
    variants.push(value);
    let mut value = pull();
    value["state"] = json!("closed");
    variants.push(value);
    let mut value = pull();
    value["merged"] = json!(true);
    variants.push(value);
    let mut value = pull();
    value["base"]["repo"]["id"] = json!(43);
    variants.push(value);
    let mut value = pull();
    value["head"]["repo"] = Value::Null;
    variants.push(value);
    let mut value = pull();
    value["head"]["sha"] = json!("short");
    variants.push(value);
    let mut value = pull();
    value.as_object_mut().unwrap().remove("draft");
    variants.push(value);
    for value in variants {
        let fixture = fixture(vec![reply(value)]).await;
        assert!(fixture.provider.publish_unavailable(7).await.is_err());
        assert_eq!(fixture.requests.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn github_gate_check_ack_requires_exact_sha_app_and_failure() {
    let mut variants = Vec::new();
    let mut value = check();
    value["app"]["id"] = json!(100);
    variants.push(value);
    let mut value = check();
    value["head_sha"] = json!("c".repeat(40));
    variants.push(value);
    let mut value = check();
    value["name"] = json!("other-check");
    variants.push(value);
    let mut value = check();
    value["external_id"] = json!("forged-native-car");
    variants.push(value);
    for conclusion in ["success", "neutral", "skipped", "cancelled", "timed_out"] {
        let mut value = check();
        value["conclusion"] = json!(conclusion);
        variants.push(value);
    }
    for value in variants {
        let fixture = fixture(vec![reply(pull()), created(value)]).await;
        assert!(fixture.provider.publish_unavailable(7).await.is_err());
    }
}

#[tokio::test]
async fn github_gate_missing_native_authority_never_promotes_worker_json() {
    let fixture = fixture(vec![reply(pull()), created(check())]).await;
    let result = fixture
        .provider
        .publish_current(7, |_| async {
            Err::<NativeAcceptanceRecord, _>(anyhow!(
                "worker JSON says ready=true; PRIVATE-WORKER-OUTPUT"
            ))
        })
        .await;
    let error = result.unwrap_err();
    assert!(!format!("{error:#}").contains("PRIVATE-WORKER-OUTPUT"));
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].body["conclusion"], "failure");
    assert!(!requests
        .iter()
        .any(|request| request.body["conclusion"] == "success"));
}

#[tokio::test]
async fn github_gate_response_and_configuration_bounds_fail_closed() {
    let fixture = fixture(vec![Reply {
        status: 200,
        body: vec![b' '; MAX_RESPONSE_BYTES + 1],
        location: None,
    }])
    .await;
    assert!(fixture.provider.read_target(7).await.is_err());
    assert!(ProviderRepository::new("owner/token", "repo").is_err());
    assert!(ProviderRepository::new("https://token@host", "repo").is_err());
    assert!(GitHubConfig::new(
        ProviderRepository::new("owner", "repo").unwrap(),
        0,
        99,
        "check"
    )
    .is_err());
    assert!(GitHubConfig::new(
        ProviderRepository::new("owner", "repo").unwrap(),
        42,
        0,
        "check"
    )
    .is_err());
    assert!(GitHubProvider::new(config(), "token\nprivate").is_err());
}

#[tokio::test]
async fn github_gate_remote_skipped_neutral_or_pending_is_not_success() {
    for (status, conclusion) in [
        ("completed", Some("neutral")),
        ("completed", Some("skipped")),
        ("queued", None),
        ("in_progress", None),
        ("completed", Some("failure")),
    ] {
        let mut value = check();
        value["status"] = json!(status);
        value["conclusion"] = json!(conclusion);
        let fixture = fixture(vec![reply(value)]).await;
        let observation = fixture.provider.read_check(9).await.unwrap();
        assert!(!observation.strictly_successful());
    }
}

fn native_git(root: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
fn native_git_fixture() -> (tempfile::TempDir, String, String) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("src/app.js"),
        "export function value() { return 1; }\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("package.json"),
        r#"{"name":"native-publisher-fixture","scripts":{"lint":"node --check src/app.js"}}"#,
    )
    .unwrap();
    std::fs::write(root.path().join("README.md"), "# Before\n").unwrap();
    native_git(root.path(), &["init", "-q"]);
    native_git(root.path(), &["add", "."]);
    let commit = [
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "-c",
        "commit.gpgSign=false",
        "commit",
        "-qm",
        "fixture",
    ];
    native_git(root.path(), &commit);
    let base = native_git(root.path(), &["rev-parse", "HEAD"]);
    std::fs::write(root.path().join("README.md"), "# Candidate\n").unwrap();
    native_git(root.path(), &["add", "README.md"]);
    native_git(root.path(), &commit);
    let head = native_git(root.path(), &["rev-parse", "HEAD"]);
    (root, base, head)
}
fn native_pull(base: &str, head: &str) -> Value {
    let mut value = pull();
    value["base"]["sha"] = json!(base);
    value["head"]["sha"] = json!(head);
    value
}
fn native_check(head: &str, digest: Option<&str>) -> Value {
    let mut value = check();
    value["head_sha"] = json!(head);
    value["external_id"] = json!(format!("wcode:{}", digest.unwrap_or("unavailable")));
    value
}

#[tokio::test]
async fn github_gate_real_native_capture_without_policy_or_with_dirty_commit_stays_failure() {
    for dirty in [false, true] {
        let (root, base, head) = native_git_fixture();
        if dirty {
            std::fs::write(
                root.path().join("src/app.js"),
                "export function value() { return 2; }\n",
            )
            .unwrap();
        }
        let native = crate::verification::acceptance_native::capture_local(
            root.path(),
            "provider-native-fixture",
            &base,
            &head,
        )
        .await
        .unwrap();
        assert_ne!(
            native.record().state,
            crate::verification::acceptance::AcceptanceState::Ready
        );
        assert!(native.record().policy.is_none());
        assert_eq!(native.record().git.complete, !dirty);
        let fixture = fixture(vec![
            reply(native_pull(&base, &head)),
            created(native_check(&head, None)),
            reply(native_check(&head, Some(native.record().digest()))),
        ])
        .await;
        let path = root.path().to_path_buf();
        let receipt = fixture
            .provider
            .publish_current(7, |target| {
                let path = path.clone();
                async move {
                    crate::verification::acceptance_native::capture_local(
                        path,
                        "provider-native-fixture",
                        &target.base_sha,
                        &target.head_sha,
                    )
                    .await
                }
            })
            .await
            .unwrap();
        assert_eq!(receipt.verdict(), GateVerdict::Blocked);
        assert_eq!(receipt.record_digest(), Some(native.record().digest()));
        let requests = fixture.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[1].body["head_sha"], head);
        assert_eq!(requests[2].method, "PATCH");
        assert!(requests
            .iter()
            .filter(|request| request.method != "GET")
            .all(|request| request.body["conclusion"] == "failure"));
    }
}

#[tokio::test]
async fn github_gate_real_native_capture_for_another_head_is_incomplete_and_denied() {
    let (root, base, _) = native_git_fixture();
    let native = crate::verification::acceptance_native::capture_local(
        root.path(),
        "provider-native-fixture",
        &base,
        &base,
    )
    .await
    .unwrap();
    assert!(
        !native.record().git.complete,
        "an older commit cannot be the current clean HEAD"
    );
    assert_ne!(
        native.record().state,
        crate::verification::acceptance::AcceptanceState::Ready
    );
    let fixture = fixture(vec![
        reply(native_pull(&base, &base)),
        created(native_check(&base, None)),
        reply(native_check(&base, Some(native.record().digest()))),
    ])
    .await;
    let path = root.path().to_path_buf();
    let receipt = fixture
        .provider
        .publish_current(7, |target| {
            let path = path.clone();
            async move {
                crate::verification::acceptance_native::capture_local(
                    path,
                    "provider-native-fixture",
                    &target.base_sha,
                    &target.head_sha,
                )
                .await
            }
        })
        .await
        .unwrap();
    assert_eq!(receipt.verdict(), GateVerdict::Blocked);
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests
        .iter()
        .filter(|request| request.method != "GET")
        .all(|request| request.body["conclusion"] == "failure"));
}

use crate::native_acceptance_fixture as trusted_native_fixture;

#[tokio::test]
async fn github_gate_public_local_capture_publishes_only_real_native_ready() {
    let native = trusted_native_fixture::NativeFixture::new();
    native.activate();
    let ready = native.verify_and_review().await;
    let mut success = native_check(&native.head, Some(ready.record().digest()));
    success["conclusion"] = json!("success");
    let fixture = fixture(vec![
        reply(native_pull(&native.base, &native.head)),
        created(native_check(&native.head, None)),
        reply(native_pull(&native.base, &native.head)),
        reply(success.clone()),
        reply(native_pull(&native.base, &native.head)),
        reply(success),
    ])
    .await;
    let receipt = fixture
        .provider
        .publish_local(7, native.root.path(), trusted_native_fixture::ID)
        .await
        .unwrap();
    assert_eq!(receipt.verdict(), GateVerdict::Ready);
    assert_eq!(receipt.record_digest(), Some(ready.record().digest()));
    assert!(receipt.check().strictly_successful());
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(
        requests
            .iter()
            .map(|request| request.method.as_str())
            .collect::<Vec<_>>(),
        ["GET", "POST", "GET", "PATCH", "GET", "GET"]
    );
    assert_eq!(requests[1].body["conclusion"], "failure");
    assert_eq!(requests[3].body["conclusion"], "success");
    assert_eq!(
        requests[3].body["external_id"],
        format!("wcode:{}", ready.record().digest())
    );
}

#[tokio::test]
async fn github_gate_real_native_drift_before_or_after_success_never_returns_ready() {
    for drift_after_success in [false, true] {
        let native = trusted_native_fixture::NativeFixture::new();
        native.activate();
        let ready = native.verify_and_review().await;
        let mut success = native_check(&native.head, Some(ready.record().digest()));
        success["conclusion"] = json!("success");
        let mut replies = vec![
            reply(native_pull(&native.base, &native.head)),
            created(native_check(&native.head, None)),
            reply(native_pull(&native.base, &native.head)),
        ];
        if drift_after_success {
            replies.extend([
                reply(success),
                reply(native_pull(&native.base, &native.head)),
                reply(native_check(&native.head, None)),
            ]);
        }
        let fixture = fixture(replies).await;
        let count = std::sync::atomic::AtomicUsize::new(0);
        let result = fixture
            .provider
            .publish_current(7, |target| {
                let count = count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let root = native.root.path().to_path_buf();
                async move {
                    if count == if drift_after_success { 2 } else { 1 } {
                        std::fs::write(root.join("README.md"), "# Drift after prior capture\n")
                            .unwrap();
                    }
                    crate::verification::acceptance_native::capture_local(
                        root,
                        trusted_native_fixture::ID,
                        &target.base_sha,
                        &target.head_sha,
                    )
                    .await
                }
            })
            .await;
        assert!(result.is_err());
        let requests = fixture.requests.lock().unwrap();
        let conclusions = requests
            .iter()
            .filter_map(|request| request.body["conclusion"].as_str())
            .collect::<Vec<_>>();
        if drift_after_success {
            assert_eq!(conclusions, ["failure", "success", "failure"]);
            assert_eq!(requests.last().unwrap().method, "PATCH");
        } else {
            assert_eq!(conclusions, ["failure"]);
        }
    }
}

#[tokio::test]
async fn github_gate_unconfirmed_success_response_attempts_failure_replacement() {
    let native = trusted_native_fixture::NativeFixture::new();
    native.activate();
    native.verify_and_review().await;
    let fixture = fixture(vec![
        reply(native_pull(&native.base, &native.head)),
        created(native_check(&native.head, None)),
        reply(native_pull(&native.base, &native.head)),
        reply(json!({"unexpected":"PRIVATE-SUCCESS-ACK"})),
        reply(native_check(&native.head, None)),
    ])
    .await;
    let result = fixture
        .provider
        .publish_local(7, native.root.path(), trusted_native_fixture::ID)
        .await;
    assert!(result.is_err());
    assert!(!result
        .unwrap_err()
        .to_string()
        .contains("PRIVATE-SUCCESS-ACK"));
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(
        requests
            .iter()
            .filter_map(|request| request.body["conclusion"].as_str())
            .collect::<Vec<_>>(),
        ["failure", "success", "failure"]
    );
}
