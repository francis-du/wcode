use super::super::tests::{body, headers, inbox, signed_headers, KEY};
use super::*;
use axum::body::Body;
use tower::ServiceExt;

fn request(bytes: Vec<u8>, map: HeaderMap) -> Request {
    let mut request = Request::builder()
        .method("POST")
        .uri("/github/webhook")
        .body(Body::from(bytes))
        .unwrap();
    *request.headers_mut() = map;
    request
}

struct Server(tokio::task::JoinHandle<()>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[tokio::test]
async fn inbox_receiver_real_http_acknowledges_only_authenticated_durable_events() {
    let (_parent, inbox) = inbox();
    let router = inbox.receiver_router(KEY).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let _server = Server(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let client = Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let bytes = body(7);
    let response = client
        .post(format!("{url}/github/webhook"))
        .headers(headers(&bytes))
        .body(bytes.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), HttpStatus::ACCEPTED);
    let value: Value = response.json().await.unwrap();
    assert_eq!(value["accepted"], true);
    assert_eq!(value["duplicate"], false);
    assert_eq!(value["current_acceptance"], false);
    let mut renamed = headers(&bytes);
    renamed.insert(
        "x-github-delivery",
        HeaderValue::from_static("82d3162e-cc78-11e3-81ab-4c9367dc0958"),
    );
    let response = client
        .post(format!("{url}/github/webhook"))
        .headers(renamed)
        .body(bytes)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), HttpStatus::ACCEPTED);
    assert_eq!(response.json::<Value>().await.unwrap()["duplicate"], true);
    assert_eq!(inbox.status().unwrap()["retained"], 1);
    assert_eq!(
        client
            .get(format!("{url}/inbox-status"))
            .send()
            .await
            .unwrap()
            .status(),
        HttpStatus::NOT_FOUND
    );
}

#[tokio::test]
async fn inbox_receiver_real_http_refreshes_only_a_current_authenticated_redelivery() {
    let (_parent, inbox) = inbox();
    let bytes = body(7);
    inbox.enqueue(&headers(&bytes), &bytes, KEY).unwrap();
    let before = fs::read(inbox.root.join("inbox.json")).unwrap();
    let key = b"rotated-http-test-key-not-live";
    let router = inbox.receiver_router(key).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/github/webhook", listener.local_addr().unwrap());
    let _server = Server(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let client = Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let invalid = client
        .post(&url)
        .headers(headers(&bytes))
        .body(bytes.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(invalid.status(), HttpStatus::UNAUTHORIZED);
    assert_eq!(before, fs::read(inbox.root.join("inbox.json")).unwrap());
    let response = client
        .post(&url)
        .headers(signed_headers(&bytes, key))
        .body(bytes.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), HttpStatus::ACCEPTED);
    let ack: Value = response.json().await.unwrap();
    assert_eq!(ack["duplicate"], true);
    assert_eq!(ack["reauthenticated"], true);
    assert_eq!(ack["current_acceptance"], false);
    let reopened = GitHubInbox::open(&inbox.root, inbox.config.clone(), 56).unwrap();
    let state = reopened.read(now_ms().unwrap()).unwrap();
    assert_eq!(state.entries.len(), 1);
    assert_eq!(state.entries[0].attempts, 0);
    VerifiedPullRequestDelivery::verify(
        &inbox.config,
        56,
        &state.entries[0].header_map().unwrap(),
        &state.entries[0].body().unwrap(),
        key,
    )
    .unwrap();
    let stable = fs::read(inbox.root.join("inbox.json")).unwrap();
    let response = client
        .post(&url)
        .headers(signed_headers(&bytes, key))
        .body(bytes)
        .send()
        .await
        .unwrap();
    let duplicate: Value = response.json().await.unwrap();
    assert_eq!(duplicate["reauthenticated"], false);
    assert_eq!(duplicate["generation"], ack["generation"]);
    assert_eq!(stable, fs::read(inbox.root.join("inbox.json")).unwrap());
    assert!(!ack.to_string().contains("PRIVATE"));
    assert!(!ack.to_string().contains(std::str::from_utf8(key).unwrap()));
}

#[tokio::test]
async fn inbox_receiver_rejects_ambiguous_signature_encoding_and_oversized_input() {
    let (_parent, inbox) = inbox();
    let router = inbox.receiver_router(KEY).unwrap();
    let bytes = body(7);
    let mut duplicate = headers(&bytes);
    duplicate.append(
        "x-hub-signature-256",
        duplicate["x-hub-signature-256"].clone(),
    );
    let response = router
        .clone()
        .oneshot(request(bytes.clone(), duplicate))
        .await
        .unwrap();
    assert_eq!(response.status(), HttpStatus::UNAUTHORIZED);
    let response = router
        .clone()
        .oneshot(request(b"PRIVATE invalid body".to_vec(), headers(&bytes)))
        .await
        .unwrap();
    assert_eq!(response.status(), HttpStatus::UNAUTHORIZED);
    let error = to_bytes(response.into_body(), 4096).await.unwrap();
    assert!(!String::from_utf8_lossy(&error).contains("PRIVATE"));
    let response = router
        .clone()
        .oneshot(request(vec![b'x'; MAX_WEBHOOK_BYTES + 1], headers(&bytes)))
        .await
        .unwrap();
    assert_eq!(response.status(), HttpStatus::PAYLOAD_TOO_LARGE);
    let mut compressed = headers(&bytes);
    compressed.insert("content-encoding", HeaderValue::from_static("gzip"));
    let response = router.oneshot(request(bytes, compressed)).await.unwrap();
    assert_eq!(response.status(), HttpStatus::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(inbox.status().unwrap()["retained"], 0);
}

#[tokio::test]
async fn inbox_receiver_slow_body_has_deadline_and_releases_intake_slot() {
    let (_parent, inbox) = inbox();
    let state = ReceiverState {
        inbox,
        key: Arc::new(KEY.to_vec()),
        slots: Arc::new(Semaphore::new(1)),
        receive_timeout: Duration::from_millis(20),
        response_timeout: Duration::from_secs(1),
    };
    let slots = state.slots.clone();
    let router = router(state);
    let pending = Body::from_stream(futures_util::stream::pending::<
        Result<Vec<u8>, std::io::Error>,
    >());
    let request = Request::builder()
        .method("POST")
        .uri("/github/webhook")
        .body(pending)
        .unwrap();
    let response = tokio::time::timeout(Duration::from_secs(2), router.oneshot(request))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), HttpStatus::REQUEST_TIMEOUT);
    assert_eq!(slots.available_permits(), 1);
}

#[tokio::test]
async fn inbox_receiver_response_timeout_keeps_permit_until_actual_storage_work_ends() {
    let slots = Arc::new(Semaphore::new(1));
    let permit = slots.clone().acquire_owned().await.unwrap();
    let (release, wait) = std::sync::mpsc::channel();
    let (started, started_wait) = tokio::sync::oneshot::channel();
    let work = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        started.send(()).unwrap();
        let _ = wait.recv();
        Err(HttpStatus::SERVICE_UNAVAILABLE)
    });
    started_wait.await.unwrap();
    let response = persistence_response(Instant::now() + Duration::from_millis(20), work).await;
    assert_eq!(response.status(), HttpStatus::SERVICE_UNAVAILABLE);
    assert_eq!(slots.available_permits(), 0);
    release.send(()).unwrap();
    let restored = tokio::time::timeout(Duration::from_secs(2), slots.clone().acquire_owned())
        .await
        .unwrap()
        .unwrap();
    drop(restored);
    assert_eq!(slots.available_permits(), 1);
}

#[tokio::test]
async fn inbox_receiver_capacity_and_store_busy_never_acknowledge_success() {
    let (_parent, inbox) = inbox();
    let state = ReceiverState {
        inbox: inbox.clone(),
        key: Arc::new(KEY.to_vec()),
        slots: Arc::new(Semaphore::new(1)),
        receive_timeout: RECEIVE_TIMEOUT,
        response_timeout: RESPONSE_TIMEOUT,
    };
    let permit = state.slots.clone().acquire_owned().await.unwrap();
    let router = router(state);
    let bytes = body(7);
    assert_eq!(
        router
            .clone()
            .oneshot(request(bytes.clone(), headers(&bytes)))
            .await
            .unwrap()
            .status(),
        HttpStatus::SERVICE_UNAVAILABLE
    );
    drop(permit);
    let lock = inbox.lock().unwrap();
    assert_eq!(
        router
            .clone()
            .oneshot(request(bytes.clone(), headers(&bytes)))
            .await
            .unwrap()
            .status(),
        HttpStatus::SERVICE_UNAVAILABLE
    );
    assert_eq!(inbox.status().unwrap()["retained"], 0);
    drop(lock);
    assert_eq!(
        router
            .oneshot(request(bytes.clone(), headers(&bytes)))
            .await
            .unwrap()
            .status(),
        HttpStatus::ACCEPTED
    );
    assert_eq!(inbox.status().unwrap()["retained"], 1);
}
