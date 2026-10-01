use super::*;
use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::Response,
    routing::any,
    Router,
};
use std::fs;
use std::sync::{Arc, Mutex as StdMutex};

#[path = "credential_fixture.rs"]
mod fixture_keys;
use fixture_keys::*;

fn pem(encoded: &str, pkcs8: bool) -> String {
    let name = if pkcs8 {
        "PRIVATE KEY"
    } else {
        "RSA PRIVATE KEY"
    };
    format!("-----BEGIN {name}-----\n{encoded}\n-----END {name}-----\n")
}

#[test]
fn github_credentials_inline_pem_supports_renewal_without_a_private_file() {
    let private_key = pem(A_PKCS8, true);
    let credentials = Credentials::app_pem(56, private_key.as_bytes()).unwrap();
    assert!(credentials.renewable());
    assert!(Credentials::app_pem(0, private_key.as_bytes()).is_err());
    assert!(Credentials::app_pem(56, b"not-a-private-key").is_err());
    assert!(Credentials::app_pem(56, &vec![b'x'; KEY_BYTES as usize + 1]).is_err());
}
fn key_directory() -> (tempfile::TempDir, PathBuf) {
    use std::io::Write;
    let temp = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let path = temp.path().canonicalize().unwrap().join("fixture.pem");
    inbox::create_file(&path)
        .unwrap()
        .write_all(pem(A_PKCS8, true).as_bytes())
        .unwrap();
    (temp, path)
}
fn config() -> GitHubConfig {
    GitHubConfig::new(
        ProviderRepository::new("owner", "repo").unwrap(),
        12,
        34,
        "gate",
    )
    .unwrap()
}
fn format_time(seconds: u64) -> String {
    let mut days = seconds / 86400;
    let mut year = 1970;
    let leap = |y: u64| y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400));
    while days >= if leap(year) { 366 } else { 365 } {
        days -= if leap(year) { 366 } else { 365 };
        year += 1;
    }
    let mut month = 1;
    for length in [
        31,
        if leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ] {
        if days < length {
            break;
        }
        days -= length;
        month += 1;
    }
    format!(
        "{year:04}-{month:02}-{:02}T{:02}:{:02}:{:02}Z",
        days + 1,
        seconds % 86400 / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}
fn verify_jwt(token: &str, public: &[u8]) {
    let parts: Vec<_> = token.split('.').collect();
    assert_eq!(parts.len(), 3);
    let header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
    let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
    assert_eq!(header["alg"], "RS256");
    assert_eq!(claims["iss"], "34");
    let now = unix_seconds().unwrap();
    assert!(claims["iat"].as_u64().unwrap() <= now - 59);
    assert!(claims["exp"].as_u64().unwrap() > now);
    assert!(claims["exp"].as_u64().unwrap() <= now + 600);
    ring::signature::UnparsedPublicKey::new(&ring::signature::RSA_PKCS1_2048_8192_SHA256, public)
        .verify(
            format!("{}.{}", parts[0], parts[1]).as_bytes(),
            &URL_SAFE_NO_PAD.decode(parts[2]).unwrap(),
        )
        .unwrap();
}

struct Remote {
    installs: usize,
    exchanges: usize,
    reads: usize,
    writes: usize,
    public: Vec<u8>,
    variant: &'static str,
    deny_write: bool,
    pause: Option<Arc<tokio::sync::Notify>>,
}
struct Fixture {
    provider: Arc<GitHubProvider>,
    remote: Arc<StdMutex<Remote>>,
    key: PathBuf,
    _temp: tempfile::TempDir,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
async fn remote() -> Fixture {
    async fn handler(
        State(shared): State<Arc<StdMutex<Remote>>>,
        request: Request,
    ) -> Response<Body> {
        let path = request.uri().path().to_owned();
        let method = request.method().clone();
        let authorization = request.headers()[AUTHORIZATION]
            .to_str()
            .unwrap()
            .to_owned();
        let bytes = to_bytes(request.into_body(), 65536).await.unwrap();
        let (status, bytes, pause) = {
            let mut state = shared.lock().unwrap();
            let mut status = 200u16;
            let mut pause = None;
            let value = if path.ends_with("/installation") {
                assert_eq!(path, "/repos/owner/repo/installation");
                assert_eq!(method, Method::GET);
                verify_jwt(
                    authorization.strip_prefix("Bearer ").unwrap(),
                    &state.public,
                );
                state.installs += 1;
                let mut value = json!({"id":56,"app_id":34,"suspended_at":null});
                match state.variant {
                    "wrong_installation" => value["id"] = json!(57),
                    "wrong_app" => value["app_id"] = json!(35),
                    "suspended" => value["suspended_at"] = json!("2026-01-01T00:00:00Z"),
                    "missing_suspension" => {
                        value.as_object_mut().unwrap().remove("suspended_at");
                    }
                    _ => {}
                }
                value
            } else if path == "/app/installations/56/access_tokens" {
                status = 201;
                assert_eq!(method, Method::POST);
                verify_jwt(
                    authorization.strip_prefix("Bearer ").unwrap(),
                    &state.public,
                );
                let body: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(body["repository_ids"], json!([12]));
                assert_eq!(
                    body["permissions"],
                    json!({"checks":"write","pull_requests":"read",
                    "contents":"read","administration":"read","metadata":"read"})
                );
                state.exchanges += 1;
                pause = state.pause.clone();
                let mut value = json!({
                    "token":format!("ghs_APPID_offline_fixture_{}",state.exchanges),
                    "expires_at":format_time(unix_seconds().unwrap()+3600),
                    "permissions":body["permissions"],
                    "repositories":[{"id":12,"full_name":"owner/repo"}],
                });
                match state.variant {
                    "outage" => {
                        status = 503;
                        value = json!({"private":"PRIVATE-REMOTE-BODY"});
                    }
                    "redirect" => status = 302,
                    "expired" => {
                        value["expires_at"] = json!(format_time(unix_seconds().unwrap() - 1))
                    }
                    "too_long" => {
                        value["expires_at"] = json!(format_time(unix_seconds().unwrap() + 7200))
                    }
                    "near_expiry" => {
                        value["expires_at"] = json!(format_time(unix_seconds().unwrap() + 120))
                    }
                    "bad_expiry" => value["expires_at"] = json!("2026-02-30T01:00:00Z"),
                    "wrong_repo" => value["repositories"][0]["id"] = json!(13),
                    "wrong_name" => value["repositories"][0]["full_name"] = json!("other/repo"),
                    "extra_repo" => value["repositories"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!({"id":13,"full_name":"other/repo"})),
                    "missing_repo" => {
                        value.as_object_mut().unwrap().remove("repositories");
                    }
                    "broad_permission" => value["permissions"]["contents"] = json!("write"),
                    "extra_permission" => value["permissions"]["issues"] = json!("read"),
                    "missing_permission" => {
                        value["permissions"]
                            .as_object_mut()
                            .unwrap()
                            .remove("checks");
                    }
                    "bad_token" => value["token"] = json!("PRIVATE-TOKEN\nnot-a-header"),
                    "oversized" => value["oversized"] = json!("X".repeat(65537)),
                    _ => {}
                }
                value
            } else {
                assert_eq!(
                    authorization,
                    format!("Bearer ghs_APPID_offline_fixture_{}", state.exchanges)
                );
                state.reads += usize::from(method == Method::GET);
                if path == "/repos/owner/repo/pulls/7" {
                    json!({"number":7,"state":"open","draft":false,"merged":false,
                        "base":{"sha":"a".repeat(40),"ref":"main","repo":{"id":12,"full_name":"owner/repo"}},
                        "head":{"sha":"b".repeat(40),"repo":{"id":13,"full_name":"fork/repo"}}})
                } else if path.ends_with("/branches/main/protection") {
                    json!({"enforce_admins":{"enabled":true},"required_status_checks":{
                        "strict":true,"checks":[{"context":"gate","app_id":34}]}})
                } else if path.ends_with("/rules/branches/main") {
                    json!([])
                } else if path.ends_with("/check-runs") {
                    assert_eq!(method, Method::POST);
                    state.writes += 1;
                    if state.deny_write {
                        status = 401;
                        state.deny_write = false;
                    } else {
                        status = 201;
                    }
                    let mut body: Value = serde_json::from_slice(&bytes).unwrap();
                    body["id"] = json!(9);
                    body["app"] = json!({"id":34});
                    body
                } else {
                    panic!("unexpected credential fixture route");
                }
            };
            let bytes = if state.variant == "malformed" && path.ends_with("/access_tokens") {
                b"PRIVATE-NOT-JSON".to_vec()
            } else {
                serde_json::to_vec(&value).unwrap()
            };
            (status, bytes, pause)
        };
        if let Some(pause) = pause {
            pause.notified().await;
        }
        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .header("location", "https://example.invalid/never-follow")
            .body(Body::from(bytes))
            .unwrap()
    }
    let (temp, key) = key_directory();
    let state = Arc::new(StdMutex::new(Remote {
        installs: 0,
        exchanges: 0,
        reads: 0,
        writes: 0,
        public: read_key(&key).unwrap().public().as_ref().to_vec(),
        variant: "valid",
        deny_write: false,
        pause: None,
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let app = Router::new()
        .fallback(any(handler))
        .with_state(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Fixture {
        provider: Arc::new(GitHubProvider::app_client(config(), 56, &key, api, false).unwrap()),
        remote: state,
        key,
        _temp: temp,
        server,
    }
}
async fn expire(fixture: &Fixture) {
    let Credentials::App(app) = &fixture.provider.credentials else {
        panic!()
    };
    let mut state = app.state.lock().await;
    state.cached.as_mut().unwrap().refresh_at = Instant::now();
}

#[test]
fn github_credentials_sign_pkcs1_and_pkcs8_with_real_rsa_and_bounded_claims() {
    for (encoded, kind) in [(A_PKCS8, true), (A_PKCS1, false)] {
        let key = parse_key(pem(encoded, kind).as_bytes()).unwrap();
        let jwt = sign_jwt(&key, 34, unix_seconds().unwrap()).unwrap();
        verify_jwt(&jwt, key.public().as_ref());
        assert!(!jwt.contains(encoded));
        assert!(sign_jwt(&key, 0, 100).is_err());
        assert!(sign_jwt(&key, 34, 59).is_err());
        assert!(sign_jwt(&key, 34, u64::MAX).is_err());
    }
    for bad in [
        "",
        "PRIVATE-NOT-A-KEY",
        "-----BEGIN ENCRYPTED PRIVATE KEY-----\nAAAA\n-----END ENCRYPTED PRIVATE KEY-----",
    ] {
        assert!(parse_key(bad.as_bytes()).is_err());
    }
}

#[test]
fn github_credentials_expiry_and_dual_clock_boundaries_are_strict() {
    for (time, seconds) in [
        ("1970-01-01T00:00:00Z", 0),
        ("2000-01-01T00:00:00Z", 946684800),
        ("2000-02-29T00:00:00Z", 951782400),
        ("2038-01-19T03:14:07Z", 2147483647),
    ] {
        assert_eq!(parse_expiry(time).unwrap(), seconds);
    }
    for bad in [
        "1969-12-31T23:59:59Z",
        "2100-02-29T00:00:00Z",
        "2024-02-30T00:00:00Z",
        "2026-01-01T24:00:00Z",
        "2026-01-01T00:60:00Z",
        "2026-01-01T00:00:60Z",
        "2026-13-01T00:00:00Z",
        "2026-00-01T00:00:00Z",
        "2026-01-00T00:00:00Z",
        "2026-01-01T00:00:00+00:00",
        "2026-01-01T00:00:00.1Z",
        "🦀🦀🦀🦀🦀",
    ] {
        assert!(parse_expiry(bad).is_err(), "{bad}");
    }
    let now = Instant::now();
    let cached = Cached {
        authorization: bearer("offline").unwrap(),
        observed_at: 1000,
        expires_at: 4600,
        refresh_at: now + Duration::from_secs(3480),
    };
    assert!(cached.usable(1000, now));
    assert!(!cached.usable(999, now));
    assert!(!cached.usable(4480, now));
    assert!(!cached.usable(1000, now + Duration::from_secs(3480)));
}

#[cfg(unix)]
#[test]
fn github_credentials_private_key_paths_reject_links_permissions_and_missing_without_writes() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let (temp, key) = key_directory();
    assert!(Credentials::app(56, &key).is_ok());
    assert!(Credentials::app(0, &key).is_err());
    let alias = temp.path().join("alias.pem");
    symlink(&key, &alias).unwrap();
    assert!(Credentials::app(56, &alias).is_err());
    let hard = temp.path().join("hard.pem");
    fs::hard_link(&key, &hard).unwrap();
    assert!(Credentials::app(56, &key).is_err());
    fs::remove_file(hard).unwrap();
    fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(Credentials::app(56, &key).is_err());
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Credentials::app(56, &key).is_err());
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let missing = temp.path().join("missing.pem");
    let error = Credentials::app(56, &missing).err().unwrap();
    assert!(!format!("{error:#}").contains(temp.path().to_str().unwrap()));
    assert!(!missing.exists());
}

#[tokio::test]
async fn github_credentials_concurrent_requests_share_one_scoped_exchange() {
    let fixture = remote().await;
    let results =
        futures_util::future::join_all((0..16).map(|_| fixture.provider.read_target(7))).await;
    assert!(results.iter().all(Result::is_ok));
    let state = fixture.remote.lock().unwrap();
    assert_eq!((state.installs, state.exchanges, state.reads), (1, 1, 16));
    assert!(!format!("{:?}", fixture.provider).contains("fixture.pem"));
}

#[tokio::test]
async fn github_credentials_expiry_refreshes_and_late_401_cannot_drop_replacement() {
    let fixture = remote().await;
    fixture.provider.read_target(7).await.unwrap();
    let old = fixture
        .provider
        .credentials
        .authorization(
            &fixture.provider.client,
            &fixture.provider.api,
            &fixture.provider.config,
        )
        .await
        .unwrap();
    assert!(old.is_sensitive());
    expire(&fixture).await;
    fixture.provider.read_target(7).await.unwrap();
    fixture.provider.credentials.reject(&old).await;
    fixture.provider.read_target(7).await.unwrap();
    assert_eq!(fixture.remote.lock().unwrap().exchanges, 2);
}

#[tokio::test]
async fn github_credentials_failed_renewal_never_reuses_expired_token_or_hammers_exchange() {
    let fixture = remote().await;
    fixture.provider.read_target(7).await.unwrap();
    expire(&fixture).await;
    fixture.remote.lock().unwrap().variant = "outage";
    for _ in 0..8 {
        let error = fixture.provider.read_target(7).await.unwrap_err();
        assert!(!format!("{error:#}").contains("PRIVATE-REMOTE"));
    }
    {
        let state = fixture.remote.lock().unwrap();
        assert_eq!((state.exchanges, state.reads), (2, 1));
    }
    fixture.remote.lock().unwrap().variant = "valid";
    // Simulate arrival at the already-tested deadline, not a sleep-dependent test.
    let Credentials::App(app) = &fixture.provider.credentials else {
        panic!()
    };
    app.state.lock().await.retry_after = Instant::now();
    fixture.provider.read_target(7).await.unwrap();
    fixture.provider.read_target(7).await.unwrap();
    assert_eq!(app.state.lock().await.failures, 0);
    let state = fixture.remote.lock().unwrap();
    assert_eq!((state.exchanges, state.reads), (3, 3));
}

#[tokio::test]
async fn github_credentials_wrong_installation_app_or_suspension_cannot_mint() {
    for variant in [
        "wrong_installation",
        "wrong_app",
        "suspended",
        "missing_suspension",
    ] {
        let fixture = remote().await;
        fixture.remote.lock().unwrap().variant = variant;
        assert!(fixture.provider.read_target(7).await.is_err(), "{variant}");
        let state = fixture.remote.lock().unwrap();
        assert_eq!((state.exchanges, state.reads), (0, 0));
    }
}

#[tokio::test]
async fn github_credentials_invalid_scope_expiry_and_responses_never_reach_repository() {
    for variant in [
        "expired",
        "too_long",
        "near_expiry",
        "bad_expiry",
        "wrong_repo",
        "wrong_name",
        "extra_repo",
        "missing_repo",
        "broad_permission",
        "extra_permission",
        "missing_permission",
        "bad_token",
        "oversized",
        "malformed",
        "redirect",
    ] {
        let fixture = remote().await;
        fixture.remote.lock().unwrap().variant = variant;
        let error = fixture.provider.read_target(7).await.unwrap_err();
        assert!(!format!("{error:#}").contains("PRIVATE-"));
        let state = fixture.remote.lock().unwrap();
        assert_eq!((state.exchanges, state.reads), (1, 0), "{variant}");
    }
}

#[tokio::test]
async fn github_credentials_401_invalidates_but_never_replays_check_write() {
    let fixture = remote().await;
    fixture.remote.lock().unwrap().deny_write = true;
    assert!(fixture.provider.publish_unavailable(7).await.is_err());
    {
        let state = fixture.remote.lock().unwrap();
        assert_eq!((state.exchanges, state.writes), (1, 1));
    }
    assert_eq!(
        fixture
            .provider
            .publish_unavailable(7)
            .await
            .unwrap()
            .verdict(),
        GateVerdict::Blocked
    );
    let state = fixture.remote.lock().unwrap();
    assert_eq!((state.exchanges, state.writes), (2, 2));
}

#[tokio::test]
async fn github_credentials_key_rotation_uses_new_rsa_material_and_missing_key_fails_closed() {
    let fixture = remote().await;
    fixture.provider.read_target(7).await.unwrap();
    expire(&fixture).await;
    fs::write(&fixture.key, pem(B_PKCS8, true)).unwrap();
    fixture.remote.lock().unwrap().public =
        read_key(&fixture.key).unwrap().public().as_ref().to_vec();
    fixture.provider.read_target(7).await.unwrap();
    expire(&fixture).await;
    fs::remove_file(&fixture.key).unwrap();
    assert!(fixture.provider.read_target(7).await.is_err());
    let state = fixture.remote.lock().unwrap();
    assert_eq!((state.exchanges, state.reads), (2, 2));
}

#[tokio::test]
async fn github_credentials_cancelled_signing_retains_slot_until_actual_work_ends() {
    let slots = Arc::new(Semaphore::new(1));
    let (started, running) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let first = tokio::spawn(sign_blocking(slots.clone(), move || {
        started.send(()).unwrap();
        released.recv_timeout(Duration::from_secs(3)).unwrap();
        Ok(())
    }));
    tokio::time::timeout(Duration::from_secs(2), running)
        .await
        .unwrap()
        .unwrap();
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    assert_eq!(
        slots.available_permits(),
        0,
        "caller cancellation released a live signer"
    );
    let entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = entered.clone();
    assert!(tokio::time::timeout(
        Duration::from_millis(20),
        sign_blocking(slots.clone(), move || {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        })
    )
    .await
    .is_err());
    assert!(!entered.load(std::sync::atomic::Ordering::SeqCst));
    release.send(()).unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        sign_blocking(slots.clone(), || Ok(())),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(slots.available_permits(), 1);
    assert!(
        sign_blocking(slots.clone(), || Err::<(), _>(anyhow!("fixture failure")))
            .await
            .is_err()
    );
    assert_eq!(slots.available_permits(), 1);
}

#[tokio::test]
async fn github_credentials_cancelled_exchange_preserves_backoff_and_does_not_lock_callers() {
    let fixture = remote().await;
    let pause = Arc::new(tokio::sync::Notify::new());
    fixture.remote.lock().unwrap().pause = Some(pause.clone());
    let provider = fixture.provider.clone();
    let task = tokio::spawn(async move { provider.read_target(7).await });
    tokio::time::timeout(Duration::from_secs(3), async {
        while fixture.remote.lock().unwrap().exchanges == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    pause.notify_one();
    let result = tokio::time::timeout(Duration::from_secs(1), fixture.provider.read_target(7))
        .await
        .unwrap();
    assert!(result.unwrap_err().to_string().contains("backing off"));
    let state = fixture.remote.lock().unwrap();
    assert_eq!((state.exchanges, state.reads), (1, 0));
}

#[tokio::test]
async fn github_credentials_preflight_uses_renewal_without_claiming_no_auth_mutations() {
    let fixture = remote().await;
    let report = fixture.provider.preflight(7).await.unwrap();
    assert!(report.configuration_verified);
    assert!(report.credential_renewal_enabled);
    assert!(report.remote_mutations);
    assert!(!report.repository_mutations);
    assert!(!report.acceptance_evaluated);
    assert_eq!(fixture.remote.lock().unwrap().exchanges, 1);
}

#[tokio::test]
async fn github_credentials_static_mode_retains_opaque_tokens_and_sensitive_headers() {
    let token = format!("ghs_APPID_{}", "x".repeat(4000));
    let creds = Credentials::fixed(&token).unwrap();
    let value = creds
        .authorization(
            &Client::new(),
            &Url::parse("https://api.github.com/").unwrap(),
            &config(),
        )
        .await
        .unwrap();
    assert!(value.is_sensitive());
    assert!(!creds.renewable());
    assert_eq!(value.to_str().unwrap(), format!("Bearer {token}"));
    for invalid in [
        "".into(),
        "x".repeat(8193),
        "token\nprivate".into(),
        "non ascii 🦀".into(),
    ] {
        assert!(Credentials::fixed(&invalid).is_err());
    }
}
