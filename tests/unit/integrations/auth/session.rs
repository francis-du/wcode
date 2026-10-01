use super::*;
use crate::monitor::TaskMonitor;
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
    let mut first_headers = host_headers("first-tunnel.example");
    first_headers.insert("authorization", format!("Bearer {access}").parse().unwrap());
    let owner_before_restart = first.authorized_client_fingerprint(&first_headers).unwrap();
    drop(first);

    let restarted = persistent_state("https://second-tunnel.example", &path);
    let mut headers = host_headers("second-tunnel.example");
    headers.insert("authorization", format!("Bearer {access}").parse().unwrap());
    assert!(restarted.authorized(&headers));
    assert_eq!(
        restarted.authorized_client_fingerprint(&headers).unwrap(),
        owner_before_restart
    );

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
    let refreshed = response_json(refreshed).await;
    let refreshed_access = refreshed["access_token"].as_str().unwrap();
    let mut refreshed_headers = host_headers("second-tunnel.example");
    refreshed_headers.insert(
        "authorization",
        format!("Bearer {refreshed_access}").parse().unwrap(),
    );
    assert_eq!(
        restarted
            .authorized_client_fingerprint(&refreshed_headers)
            .unwrap(),
        owner_before_restart
    );
}

#[tokio::test]
async fn separate_oauth_grants_for_one_client_get_distinct_writer_owners() {
    let state = Arc::new(AuthState::new("https://example.com".to_owned()));
    let client_id = registered_client(state.clone()).await;
    let first = response_json(issue_tokens(
        state.as_ref(),
        client_id.clone(),
        Some("https://example.com/mcp".to_owned()),
    ))
    .await;
    let second = response_json(issue_tokens(
        state.as_ref(),
        client_id,
        Some("https://example.com/mcp".to_owned()),
    ))
    .await;

    let fingerprint = |access: &str| {
        let mut headers = host_headers("example.com");
        headers.insert("authorization", format!("Bearer {access}").parse().unwrap());
        state.authorized_client_fingerprint(&headers).unwrap()
    };
    let first_owner = fingerprint(first["access_token"].as_str().unwrap());
    let second_owner = fingerprint(second["access_token"].as_str().unwrap());
    assert_ne!(first_owner, second_owner);
}

#[test]
fn legacy_token_records_without_owner_id_keep_compatibility_fallback() {
    let access: AccessToken = serde_json::from_value(json!({
        "issued_at_ms": 1,
        "client_id": "legacy-client",
        "resource": "https://example.com/mcp"
    }))
    .unwrap();
    let refresh: RefreshToken = serde_json::from_value(json!({
        "issued_at_ms": 1,
        "client_id": "legacy-client",
        "resource": "https://example.com/mcp"
    }))
    .unwrap();
    assert!(access.owner_id.is_empty());
    assert!(refresh.owner_id.is_empty());
}

#[tokio::test]
async fn historical_resource_does_not_block_a_custom_host_after_restart() {
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
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response_json(response).await["issuer"],
        "https://old-tunnel.example"
    );
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

#[test]
fn credential_expiry_rejects_boundary_unknown_and_future_timestamps() {
    let now = ACCESS_TOKEN_TTL_MS * 2;
    assert!(credential_current(now, ACCESS_TOKEN_TTL_MS, now));
    assert!(credential_current(
        now - ACCESS_TOKEN_TTL_MS + 1,
        ACCESS_TOKEN_TTL_MS,
        now
    ));
    assert!(!credential_current(
        now - ACCESS_TOKEN_TTL_MS,
        ACCESS_TOKEN_TTL_MS,
        now
    ));
    assert!(!credential_current(0, ACCESS_TOKEN_TTL_MS, now));
    assert!(!credential_current(now + 1, ACCESS_TOKEN_TTL_MS, now));
    assert!(!credential_current(u64::MAX, ACCESS_TOKEN_TTL_MS, now));
}

#[test]
fn expired_access_is_rejected_even_for_a_trusted_resource() {
    let state = AuthState::new("https://example.com".to_owned());
    state.insert_test_access_token("expires", "client", "https://example.com/mcp");
    let mut headers = host_headers("example.com");
    headers.insert("authorization", "Bearer expires".parse().unwrap());
    assert!(state.authorized(&headers));
    state
        .access_tokens
        .lock()
        .unwrap()
        .get_mut("expires")
        .unwrap()
        .issued_at_ms = epoch_ms() - ACCESS_TOKEN_TTL_MS;
    assert!(!state.authorized(&headers));
    assert_eq!(state.authorized_client_fingerprint(&headers), None);
}

fn operator_headers(state: &AuthState) -> HeaderMap {
    let mut headers = host_headers("example.com");
    headers.insert("x-wcode-ui-token", state.ui_token().parse().unwrap());
    headers.insert("origin", "https://example.com".parse().unwrap());
    headers
}

#[tokio::test]
async fn oauth_rotation_expires_old_access_and_sessions_never_expose_credentials() {
    let state = Arc::new(AuthState::new("https://example.com".to_owned()));
    let response = issue_tokens(
        &state,
        "client".to_owned(),
        Some("https://example.com/mcp".to_owned()),
    );
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["pragma"], "no-cache");
    let first = response_json(response).await;
    assert_eq!(first["expires_in"], 3600);
    let access = first["access_token"].as_str().unwrap();
    let refresh = first["refresh_token"].as_str().unwrap();
    let mut agent = host_headers("example.com");
    agent.insert("authorization", format!("Bearer {access}").parse().unwrap());
    assert!(state.authorized(&agent));
    let owner = state.authorized_client_fingerprint(&agent).unwrap();
    let denied = tokens::oauth_sessions(State(state.clone()), agent.clone()).await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let headers = operator_headers(&state);
    let list =
        response_json(tokens::oauth_sessions(State(state.clone()), headers.clone()).await).await;
    let encoded = list.to_string();
    assert!(!encoded.contains(access));
    assert!(!encoded.contains(refresh));
    assert!(!encoded.contains("owner_"));
    assert_eq!(list["sessions"].as_array().unwrap().len(), 1);
    let session_id = list["sessions"][0]["id"].as_str().unwrap().to_owned();
    let next = response_json(refresh_access_token(
        &state,
        TokenForm {
            grant_type: "refresh_token".to_owned(),
            code: None,
            redirect_uri: None,
            client_id: Some("client".to_owned()),
            code_verifier: None,
            refresh_token: Some(refresh.to_owned()),
            resource: None,
        },
        "https://example.com/mcp",
    ))
    .await;
    assert!(!state.authorized(&agent));
    agent.insert(
        "authorization",
        format!("Bearer {}", next["access_token"].as_str().unwrap())
            .parse()
            .unwrap(),
    );
    assert_eq!(state.authorized_client_fingerprint(&agent).unwrap(), owner);
    let list =
        response_json(tokens::oauth_sessions(State(state.clone()), headers.clone()).await).await;
    assert_eq!(list["sessions"][0]["id"], session_id);
    assert_eq!(list["sessions"][0]["access_count"], 1);
    assert!(!state.refresh_tokens.lock().unwrap().contains_key(refresh));
    let denied = tokens::oauth_session_revoke(
        State(state.clone()),
        agent.clone(),
        Json(tokens::RevokeSessionInput {
            session_id: session_id.clone(),
        }),
    )
    .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let mut cross_origin = headers.clone();
    cross_origin.insert("origin", "https://attacker.example".parse().unwrap());
    assert_eq!(
        tokens::oauth_session_revoke(
            State(state.clone()),
            cross_origin,
            Json(tokens::RevokeSessionInput {
                session_id: session_id.clone()
            })
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let other = response_json(issue_tokens(
        &state,
        "client".to_owned(),
        Some("https://example.com/mcp".to_owned()),
    ))
    .await;
    let revoked = tokens::oauth_session_revoke(
        State(state.clone()),
        headers,
        Json(tokens::RevokeSessionInput { session_id }),
    )
    .await;
    assert_eq!(revoked.status(), StatusCode::OK);
    assert!(!state.authorized(&agent));
    agent.insert(
        "authorization",
        format!("Bearer {}", other["access_token"].as_str().unwrap())
            .parse()
            .unwrap(),
    );
    assert!(state.authorized(&agent));
    assert!(!state
        .refresh_tokens
        .lock()
        .unwrap()
        .contains_key(next["refresh_token"].as_str().unwrap()));
}

#[tokio::test]
async fn session_revocation_is_persisted_and_recovery_does_not_restore_grants() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("oauth.json");
    let state = Arc::new(
        AuthState::new_persistent("https://example.com".to_owned(), path.clone()).unwrap(),
    );
    let client = "wcode-782eef5c-7845-483c-b5de-7a1864d9ff65";
    state.clients.lock().unwrap().insert(
        client.to_owned(),
        Client {
            redirect_uris: vec!["https://agent.example/callback".to_owned()],
        },
    );
    let first = response_json(issue_tokens(
        &state,
        client.to_owned(),
        Some("https://example.com/mcp".to_owned()),
    ))
    .await;
    let headers = operator_headers(&state);
    let list =
        response_json(tokens::oauth_sessions(State(state.clone()), headers.clone()).await).await;
    let session_id = list["sessions"][0]["id"].as_str().unwrap().to_owned();
    let response = tokens::oauth_session_revoke(
        State(state),
        headers,
        Json(tokens::RevokeSessionInput { session_id }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let restored = AuthState::new_persistent("https://example.com".to_owned(), path).unwrap();
    let mut agent = host_headers("example.com");
    agent.insert(
        "authorization",
        format!("Bearer {}", first["access_token"].as_str().unwrap())
            .parse()
            .unwrap(),
    );
    assert!(!restored.authorized(&agent));
    assert!(restored.refresh_tokens.lock().unwrap().is_empty());
}

#[tokio::test]
async fn failed_revoke_storage_never_reports_success_or_restores_live_access() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = AuthState::new("https://example.com".to_owned());
    let first = response_json(issue_tokens(
        &state,
        "client".to_owned(),
        Some("https://example.com/mcp".to_owned()),
    ))
    .await;
    state.store = Some(Arc::new(AuthStore::at_path(directory.path().to_owned())));
    let state = Arc::new(state);
    let headers = operator_headers(&state);
    let list =
        response_json(tokens::oauth_sessions(State(state.clone()), headers.clone()).await).await;
    let session_id = list["sessions"][0]["id"].as_str().unwrap().to_owned();
    let response = tokens::oauth_session_revoke(
        State(state.clone()),
        headers,
        Json(tokens::RevokeSessionInput { session_id }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let mut agent = host_headers("example.com");
    agent.insert(
        "authorization",
        format!("Bearer {}", first["access_token"].as_str().unwrap())
            .parse()
            .unwrap(),
    );
    assert!(!state.authorized(&agent));
    assert!(state.refresh_tokens.lock().unwrap().is_empty());
}

#[tokio::test]
async fn expired_persisted_grants_do_not_pin_registration_capacity() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("oauth.json");
    let state = Arc::new(persistent_state("https://example.com", &path));
    for index in 0..MAX_REGISTERED_CLIENTS {
        let client_id = format!("wcode-{}", Uuid::new_v4());
        state.clients.lock().unwrap().insert(
            client_id.clone(),
            Client {
                redirect_uris: vec!["https://chatgpt.com/callback".to_owned()],
            },
        );
        state.access_tokens.lock().unwrap().insert(
            format!("access_expired_{index}"),
            AccessToken {
                issued_at_ms: 1,
                client_id: client_id.clone(),
                owner_id: String::new(),
                resource: Some("https://example.com/mcp".to_owned()),
            },
        );
        state.refresh_tokens.lock().unwrap().insert(
            format!("refresh_expired_{index}"),
            RefreshToken {
                issued_at_ms: 1,
                client_id,
                owner_id: String::new(),
                resource: Some("https://example.com/mcp".to_owned()),
            },
        );
    }
    state.persist().unwrap();
    let response = register_client(State(state.clone()), Json(registration())).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(state.access_tokens.lock().unwrap().is_empty());
    assert!(state.refresh_tokens.lock().unwrap().is_empty());
    assert_eq!(state.clients.lock().unwrap().len(), MAX_REGISTERED_CLIENTS);
    let restored = persistent_state("https://example.com", &path);
    assert!(restored.access_tokens.lock().unwrap().is_empty());
    assert_eq!(
        restored.clients.lock().unwrap().len(),
        MAX_REGISTERED_CLIENTS
    );
}

#[tokio::test]
async fn expired_and_future_persisted_grants_are_not_renewed_on_load() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("oauth.json");
    let state = Arc::new(persistent_state("https://example.com", &path));
    let client = registered_client(state.clone()).await;
    let issued = response_json(issue_tokens(
        &state,
        client,
        Some("https://example.com/mcp".to_owned()),
    ))
    .await;
    state
        .access_tokens
        .lock()
        .unwrap()
        .values_mut()
        .for_each(|grant| grant.issued_at_ms = 1);
    state
        .refresh_tokens
        .lock()
        .unwrap()
        .values_mut()
        .for_each(|grant| grant.issued_at_ms = u64::MAX);
    state.persist().unwrap();
    let restored = persistent_state("https://example.com", &path);
    assert!(restored.access_tokens.lock().unwrap().is_empty());
    assert!(restored.refresh_tokens.lock().unwrap().is_empty());
    let mut agent = host_headers("example.com");
    agent.insert(
        "authorization",
        format!("Bearer {}", issued["access_token"].as_str().unwrap())
            .parse()
            .unwrap(),
    );
    assert!(!restored.authorized(&agent));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_refresh_and_revoke_cannot_resurrect_the_same_grant() {
    let state = Arc::new(AuthState::new("https://example.com".to_owned()));
    let first = response_json(issue_tokens(
        &state,
        "client".to_owned(),
        Some("https://example.com/mcp".to_owned()),
    ))
    .await;
    let headers = operator_headers(&state);
    let list =
        response_json(tokens::oauth_sessions(State(state.clone()), headers.clone()).await).await;
    let session_id = list["sessions"][0]["id"].as_str().unwrap().to_owned();
    let refresh = first["refresh_token"].as_str().unwrap().to_owned();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let state_a = state.clone();
    let barrier_a = barrier.clone();
    let rotate = tokio::spawn(async move {
        barrier_a.wait();
        refresh_access_token(
            &state_a,
            TokenForm {
                grant_type: "refresh_token".to_owned(),
                code: None,
                redirect_uri: None,
                client_id: Some("client".to_owned()),
                code_verifier: None,
                refresh_token: Some(refresh),
                resource: None,
            },
            "https://example.com/mcp",
        )
        .status()
    });
    let state_b = state.clone();
    let revoke = tokio::spawn(async move {
        barrier.wait();
        tokens::oauth_session_revoke(
            State(state_b),
            headers,
            Json(tokens::RevokeSessionInput { session_id }),
        )
        .await
        .status()
    });
    let result = rotate.await.unwrap();
    assert!(matches!(result, StatusCode::OK | StatusCode::BAD_REQUEST));
    assert_eq!(revoke.await.unwrap(), StatusCode::OK);
    assert!(state.access_tokens.lock().unwrap().is_empty());
    assert!(state.refresh_tokens.lock().unwrap().is_empty());
}

#[tokio::test]
async fn session_routes_enforce_host_operator_and_strict_revoke_payload() {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;
    let state = Arc::new(AuthState::new("https://example.com".to_owned()));
    let app = router(state.clone());
    let request = Request::builder()
        .uri("/oauth/sessions")
        .header("host", "example.com")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let request = Request::builder()
        .uri("/oauth/sessions")
        .header("host", "https://attacker.example")
        .header("x-wcode-ui-token", state.ui_token())
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    // A syntactically valid custom reverse-proxy Host remains compatible;
    // the operator token and browser Origin provide the protected UI boundary.
    let request = Request::builder()
        .uri("/oauth/sessions")
        .header("host", "custom.example")
        .header("x-wcode-ui-token", state.ui_token())
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::OK
    );
    let request = Request::builder()
        .method("POST")
        .uri("/oauth/sessions/revoke")
        .header("host", "example.com")
        .header("x-wcode-ui-token", state.ui_token())
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"session_id":"{}","unexpected":true}}"#,
            "0".repeat(64)
        )))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let request = Request::builder()
        .uri("/oauth/sessions")
        .header("host", "example.com")
        .header("x-wcode-ui-token", state.ui_token())
        .header("origin", "https://attacker.example")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
}
