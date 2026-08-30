use super::*;
use std::path::Path;

fn registration() -> RegistrationRequest {
    RegistrationRequest {
        redirect_uris: vec!["https://chatgpt.com/connector_platform_oauth_redirect".to_owned()],
        client_name: Some("ChatGPT".to_owned()),
        application_type: Some("web".to_owned()),
        grant_types: vec!["authorization_code".to_owned(), "refresh_token".to_owned()],
        response_types: vec!["code".to_owned()],
        token_endpoint_auth_method: Some("none".to_owned()),
        scope: Some("mcp".to_owned()),
    }
}

async fn registered_client(state: Arc<AuthState>) -> String {
    let response = register_client(State(state), Json(registration())).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    response_json(response).await["client_id"]
        .as_str()
        .expect("registered client id")
        .to_owned()
}

fn persistent_state(public_url: &str, path: &Path) -> AuthState {
    AuthState::new_persistent(public_url.to_owned(), path.to_owned())
        .expect("persistent auth state")
}

#[tokio::test]
async fn registered_client_can_authorize_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("oauth.json");
    let client_id = registered_client(Arc::new(persistent_state(
        "https://franciss-air.tail.example",
        &path,
    )))
    .await;

    let restarted = Arc::new(persistent_state("https://franciss-air.tail.example", &path));
    let mut authorize_url = Url::parse("https://franciss-air.tail.example/authorize").unwrap();
    authorize_url
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &client_id)
        .append_pair(
            "redirect_uri",
            "https://chatgpt.com/connector_platform_oauth_redirect",
        )
        .append_pair("scope", "mcp")
        .append_pair(
            "code_challenge",
            "o12ib6vBYRsGVNAuLapTalKC-vby4IVFsCwOa9P4mpM",
        )
        .append_pair("code_challenge_method", "S256")
        .append_pair("resource", "https://franciss-air.tail.example/mcp")
        .append_pair("state", "oauth-state");
    let response = authorize_page(
        State(restarted),
        host_headers("franciss-air.tail.example"),
        RawQuery(authorize_url.query().map(str::to_owned)),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn legacy_server_client_id_is_recovered_once_after_upgrade() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("oauth.json");
    let client_id = "wcode-782eef5c-7845-483c-b5de-7a1864d9ff65";
    let public_url = "https://franciss-air.taild4af1f.ts.net";
    let mut authorize_url = Url::parse(&format!("{public_url}/authorize")).unwrap();
    authorize_url
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair(
            "redirect_uri",
            "https://chatgpt.com/connector_platform_oauth_redirect",
        )
        .append_pair("scope", "mcp")
        .append_pair(
            "code_challenge",
            "o12ib6vBYRsGVNAuLapTalKC-vby4IVFsCwOa9P4mpM",
        )
        .append_pair("code_challenge_method", "S256")
        .append_pair("resource", &format!("{public_url}/mcp"))
        .append_pair("state", "oauth-state");
    let raw_query = authorize_url.query().map(str::to_owned);

    let upgraded = Arc::new(persistent_state(public_url, &path));
    let first = authorize_page(
        State(upgraded.clone()),
        host_headers("franciss-air.taild4af1f.ts.net"),
        RawQuery(raw_query.clone()),
    )
    .await;
    assert_eq!(first.status(), StatusCode::OK);
    let approved = authorize_submit(
        State(upgraded.clone()),
        host_headers("franciss-air.taild4af1f.ts.net"),
        Form(AuthorizeForm {
            client_id: client_id.to_owned(),
            redirect_uri: "https://chatgpt.com/connector_platform_oauth_redirect".to_owned(),
            state: "oauth-state".to_owned(),
            code_challenge: "o12ib6vBYRsGVNAuLapTalKC-vby4IVFsCwOa9P4mpM".to_owned(),
            pairing_code: upgraded.pairing_code().to_owned(),
            resource: Some(format!("{public_url}/mcp")),
            scope: Some("mcp".to_owned()),
        }),
    )
    .await;
    assert_eq!(approved.status(), StatusCode::SEE_OTHER);
    drop(upgraded);

    let restarted = Arc::new(persistent_state(public_url, &path));
    let second = authorize_page(
        State(restarted),
        host_headers("franciss-air.taild4af1f.ts.net"),
        RawQuery(raw_query),
    )
    .await;
    assert_eq!(second.status(), StatusCode::OK);
}

#[tokio::test]
async fn bearer_and_refresh_sessions_survive_restart_and_tunnel_change() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("oauth.json");
    let first = persistent_state("https://first-tunnel.example", &path);
    let client_id = registered_client(Arc::new(first.clone())).await;
    let issued = response_json(issue_tokens(
        &first,
        client_id.clone(),
        Some("https://first-tunnel.example/mcp".to_owned()),
    ))
    .await;
    let access = issued["access_token"].as_str().unwrap().to_owned();
    let refresh = issued["refresh_token"].as_str().unwrap().to_owned();
    drop(first);

    let restarted = persistent_state("https://second-tunnel.example", &path);
    let mut headers = host_headers("second-tunnel.example");
    headers.insert("authorization", format!("Bearer {access}").parse().unwrap());
    assert!(restarted.authorized(&headers));

    let refreshed = refresh_access_token(
        &restarted,
        TokenForm {
            grant_type: "refresh_token".to_owned(),
            code: None,
            redirect_uri: None,
            client_id: Some(client_id),
            code_verifier: None,
            refresh_token: Some(refresh),
            resource: Some("https://second-tunnel.example/mcp".to_owned()),
        },
        "https://second-tunnel.example/mcp",
    );
    assert_eq!(refreshed.status(), StatusCode::OK);
}

#[tokio::test]
async fn old_tunnel_resource_does_not_make_old_host_active_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("oauth.json");
    let first = persistent_state("https://old-tunnel.example", &path);
    let client_id = registered_client(Arc::new(first.clone())).await;
    assert_eq!(
        issue_tokens(
            &first,
            client_id,
            Some("https://old-tunnel.example/mcp".to_owned()),
        )
        .status(),
        StatusCode::OK
    );
    drop(first);

    let restarted = Arc::new(persistent_state("https://current-tunnel.example", &path));
    let response =
        authorization_server_metadata(State(restarted), host_headers("old-tunnel.example")).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn malformed_persistent_state_fails_closed() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("oauth.json");
    std::fs::write(&path, b"not-json").unwrap();

    assert!(AuthState::new_persistent("https://example.com".to_owned(), path).is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn persistent_oauth_state_is_owner_only_and_rejects_symlinks() {
    use std::os::unix::fs::{symlink, MetadataExt};

    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("oauth.json");
    registered_client(Arc::new(persistent_state("https://example.com", &target))).await;
    assert_eq!(std::fs::metadata(&target).unwrap().mode() & 0o077, 0);

    let alias = directory.path().join("oauth-link.json");
    symlink(&target, &alias).unwrap();
    assert!(AuthState::new_persistent("https://example.com".to_owned(), alias).is_err());
}

#[tokio::test]
async fn restored_session_is_visible_in_runtime_status() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("oauth.json");
    let first = persistent_state("https://example.com", &path);
    let client_id = registered_client(Arc::new(first.clone())).await;
    assert_eq!(
        issue_tokens(
            &first,
            client_id,
            Some("https://example.com/mcp".to_owned()),
        )
        .status(),
        StatusCode::OK
    );
    drop(first);

    let monitor = TaskMonitor::new(["workspace".to_owned()]);
    let _restarted = AuthState::new_persistent_with_monitor(
        "https://example.com".to_owned(),
        path,
        monitor.clone(),
    )
    .unwrap();
    let status = monitor.connection_status();
    assert!(status.oauth_client_registered);
    assert!(status.oauth_authorized);
}

#[tokio::test]
async fn persistent_client_capacity_reclaims_only_an_unbound_registration() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("oauth.json");
    let state = Arc::new(persistent_state("https://example.com", &path));
    {
        let mut clients = state.clients.lock().unwrap();
        for _ in 0..MAX_REGISTERED_CLIENTS {
            clients.insert(
                format!("wcode-{}", Uuid::new_v4()),
                Client {
                    redirect_uris: vec!["https://chatgpt.com/callback".to_owned()],
                },
            );
        }
    }
    state.persist().unwrap();

    let response = register_client(State(state.clone()), Json(registration())).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(state.clients.lock().unwrap().len(), MAX_REGISTERED_CLIENTS);
    drop(state);
    assert_eq!(
        persistent_state("https://example.com", &path)
            .clients
            .lock()
            .unwrap()
            .len(),
        MAX_REGISTERED_CLIENTS
    );
}
