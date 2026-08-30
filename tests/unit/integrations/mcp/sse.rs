use super::*;
use crate::auth::AuthState;
use crate::harness::ToolHarness;
use crate::mcp::TaskRuntime;
use crate::monitor::TaskMonitor;
use crate::workspace::Workspaces;
use axum::body::Body;
use futures_util::StreamExt;
use serde_json::json;
use std::fs;

fn request_headers(token: &str, host: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("host", host.parse().unwrap());
    headers.insert(
        "origin",
        format!("http://{host}").parse().expect("valid origin"),
    );
    headers.insert(
        "authorization",
        format!("Bearer {token}").parse().expect("valid bearer"),
    );
    headers
}

fn test_state() -> (Arc<AppState>, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("note.txt"), "hello\n").unwrap();
    let workspaces = Workspaces::new([root.path()], false, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let auth = Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned()));
    auth.insert_test_access_token("client-one", "client-one", "http://127.0.0.1:8765/mcp");
    (
        Arc::new(AppState {
            auth,
            workspaces,
            harness: ToolHarness::new(2).unwrap(),
            monitor: TaskMonitor::new([workspace_id]),
            tasks: TaskRuntime::default(),
        }),
        root,
    )
}

#[tokio::test]
async fn legacy_sse_routes_share_dispatch_and_cleanup_closed_sessions() {
    let (state, _root) = test_state();
    let baseline = active_session_count();
    let response = open_session(
        State(state.clone()),
        request_headers("client-one", "127.0.0.1:8765"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(active_session_count(), baseline + 1);

    let mut body = Body::into_data_stream(response.into_body());
    let endpoint = body.next().await.unwrap().unwrap();
    let endpoint = String::from_utf8(endpoint.to_vec()).unwrap();
    assert!(endpoint.contains("event: endpoint"));
    assert!(endpoint.contains("http://127.0.0.1:8765/message?sessionId="));
    let session_id = endpoint
        .split("sessionId=")
        .nth(1)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .trim()
        .to_owned();

    let notification = post_message(
        State(state.clone()),
        Query(MessageQuery {
            session_id: session_id.clone(),
        }),
        request_headers("client-one", "127.0.0.1:8765"),
        Json(json!({"jsonrpc":"2.0","method":"notifications/initialized"})),
    )
    .await;
    assert_eq!(notification.status(), StatusCode::ACCEPTED);

    let batch = post_message(
        State(state.clone()),
        Query(MessageQuery {
            session_id: session_id.clone(),
        }),
        request_headers("client-one", "127.0.0.1:8765"),
        Json(json!([
            {"jsonrpc":"2.0","id":1,"method":"ping"},
            {"jsonrpc":"2.0","id":2,"method":"tools/list"}
        ])),
    )
    .await;
    assert_eq!(batch.status(), StatusCode::ACCEPTED);
    let message = String::from_utf8(body.next().await.unwrap().unwrap().to_vec()).unwrap();
    assert!(message.contains("event: message"));
    assert!(message.contains("\"id\":1"));
    assert!(message.contains("\"id\":2"));

    state
        .auth
        .set_public_url("http://127.0.0.1:9876".to_owned());
    state
        .auth
        .insert_test_access_token("client-two", "client-one", "http://127.0.0.1:9876/mcp");
    let wrong_tunnel = post_message(
        State(state),
        Query(MessageQuery { session_id }),
        request_headers("client-two", "127.0.0.1:9876"),
        Json(json!({"jsonrpc":"2.0","id":3,"method":"ping"})),
    )
    .await;
    assert_eq!(wrong_tunnel.status(), StatusCode::NOT_FOUND);

    drop(body);
    assert_eq!(active_session_count(), baseline);
}

#[tokio::test]
async fn legacy_sse_rejects_missing_auth_and_cross_origin_requests() {
    let (state, _root) = test_state();
    let mut missing_auth = request_headers("client-one", "127.0.0.1:8765");
    missing_auth.remove("authorization");
    assert_eq!(
        open_session(State(state.clone()), missing_auth)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );

    let mut wrong_origin = request_headers("client-one", "127.0.0.1:8765");
    wrong_origin.insert("origin", "https://attacker.example".parse().unwrap());
    assert_eq!(
        open_session(State(state), wrong_origin).await.status(),
        StatusCode::FORBIDDEN
    );
}
