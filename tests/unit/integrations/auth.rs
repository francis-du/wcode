use super::*;
use axum::extract::{Form, State};
use axum::http::header::{HOST, LOCATION};

fn host_headers(host: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(HOST, host.parse().unwrap());
    headers
}

async fn response_json(response: Response) -> Value {
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("response body");
    serde_json::from_slice(&body).expect("JSON response")
}

#[test]
fn auth_pages_fit_mobile_viewports_without_disabling_zoom() {
    let simple = simple_auth_html(
        "Ready",
        "A long mobile-safe message",
        Some("https://example.com/mcp"),
    );
    let query = AuthorizeQuery {
        client_id: "client".to_owned(),
        redirect_uri: "https://example.com/callback".to_owned(),
        state: "state".to_owned(),
        code_challenge: "challenge".to_owned(),
        code_challenge_method: Some("S256".to_owned()),
        response_type: Some("code".to_owned()),
        resource: Some("https://example.com/mcp".to_owned()),
        scope: Some("mcp".to_owned()),
    };
    let authorize = authorize_html(&query, None);

    for html in [&simple, &authorize] {
        assert!(html.contains("viewport-fit=cover"));
        assert!(html.contains("min-height:100dvh"));
        assert!(html.contains("safe-area-inset-bottom"));
        assert!(!html.contains("maximum-scale"));
        assert!(!html.contains("user-scalable=no"));
    }
}

#[test]
fn pairing_code_is_always_six_ascii_digits() {
    for _ in 0..64 {
        let state = AuthState::new("https://example.com".to_owned());
        assert_eq!(state.pairing_code().len(), 6);
        assert!(state
            .pairing_code()
            .bytes()
            .all(|byte| byte.is_ascii_digit()));
    }
}

#[test]
fn ui_token_is_high_entropy_and_constant_time_checked() {
    let state = AuthState::new("https://example.com".to_owned());
    assert_eq!(state.ui_token().len(), 64);
    assert!(state
        .ui_token()
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit()));
    let mut headers = HeaderMap::new();
    headers.insert("x-wcode-ui-token", state.ui_token().parse().unwrap());
    assert!(state.ui_authorized(&headers));
    headers.insert(
        "x-wcode-ui-token",
        "0000000000000000000000000000000000000000000000000000000000000000"
            .parse()
            .unwrap(),
    );
    assert!(!state.ui_authorized(&headers));
    assert!(constant_time_text_eq("same", "same"));
    assert!(!constant_time_text_eq("same", "diff"));
    assert!(!constant_time_text_eq("same", "short"));
}

#[test]
fn each_auth_state_has_an_independent_instance_id() {
    let first = AuthState::new("https://first.example".to_owned());
    let second = AuthState::new("https://second.example".to_owned());
    assert_eq!(first.instance_id().len(), 32);
    assert_eq!(second.instance_id().len(), 32);
    assert_ne!(first.instance_id(), second.instance_id());
}

#[test]
fn pairing_failures_lock_out_client_and_success_clears_attempts() {
    let state = AuthState::new("https://example.com".to_owned());
    for _ in 0..MAX_PAIRING_FAILURES_PER_CLIENT - 1 {
        assert_eq!(
            check_pairing_code(&state, "client", "000000"),
            PairingCodeCheck::Rejected
        );
    }
    assert_eq!(
        check_pairing_code(&state, "client", "000000"),
        PairingCodeCheck::LockedOut
    );
    assert_eq!(
        check_pairing_code(&state, "client", state.pairing_code()),
        PairingCodeCheck::LockedOut
    );
    state
        .pairing_attempts
        .lock()
        .unwrap()
        .get_mut("client")
        .unwrap()
        .blocked_until = Some(Instant::now() - Duration::from_secs(1));
    assert_eq!(
        check_pairing_code(&state, "client", state.pairing_code()),
        PairingCodeCheck::Accepted
    );
    assert!(state.pairing_attempts.lock().unwrap().is_empty());
}

#[test]
fn pkce_matches_rfc_example() {
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    assert_eq!(
        pkce_challenge(verifier),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}

#[test]
fn parses_chatgpt_resource_and_scope() {
    let query = parse_authorize_query(
        "client_id=chatgpt&redirect_uri=https%3A%2F%2Fchatgpt.com%2Fcallback&state=s&code_challenge=c&code_challenge_method=S256&response_type=code&resource=https%3A%2F%2Fexample.com%2Fmcp&scope=mcp",
    )
    .expect("valid authorization query");
    assert_eq!(query.resource.as_deref(), Some("https://example.com/mcp"));
    assert_eq!(query.scope.as_deref(), Some("mcp"));
}

#[test]
fn repairs_only_backslashes_escaping_authorization_separators() {
    let raw = r"\?client_id\=chatgpt\&redirect_uri\=https%3A%2F%2Fchatgpt.com%2Fcallback\&state\=%E7%8A%B6%E6%80%81\&code_challenge\=challenge\&code_challenge_method\=S256\&response_type\=code";
    let query = parse_authorize_query(raw).expect("escaped separators are repaired");
    assert_eq!(query.client_id, "chatgpt");
    assert_eq!(query.redirect_uri, "https://chatgpt.com/callback");
    assert_eq!(query.state, "状态");
    assert_eq!(query.code_challenge, "challenge");
}

#[test]
fn valid_authorization_query_is_not_rewritten() {
    let raw = "client_id=chatgpt&redirect_uri=https%3A%2F%2Fchatgpt.com%2Fcallback&state=s&code_challenge=c";
    assert_eq!(repair_authorize_query(raw).unwrap(), raw);
}

#[test]
fn rejects_unknown_trailing_and_duplicate_authorization_parameters() {
    for raw in [
        r"client_id\chatgpt&redirect_uri=https%3A%2F%2Fchatgpt.com%2Fcallback&state=s&code_challenge=c",
        r"client_id=chatgpt&redirect_uri=https%3A%2F%2Fchatgpt.com%2Fcallback&state=s&code_challenge=c\",
        "client_id=one&client_id=two&redirect_uri=https%3A%2F%2Fchatgpt.com%2Fcallback&state=s&code_challenge=c",
    ] {
        assert!(parse_authorize_query(raw).is_err(), "unexpectedly accepted {raw}");
    }
}

#[test]
fn percent_encoded_backslash_remains_data() {
    let query = parse_authorize_query(
        "client_id=chat%5Cgpt&redirect_uri=https%3A%2F%2Fchatgpt.com%2Fcallback&state=s&code_challenge=c",
    )
    .expect("encoded backslash is not a raw escape");
    assert_eq!(query.client_id, "chat\\gpt");
}

#[test]
fn rejects_oversized_authorization_query() {
    let raw = "x".repeat(16 * 1024 + 1);
    assert!(parse_authorize_query(&raw).is_err());
}

#[test]
fn unauthorized_challenge_points_to_mcp_resource_metadata_with_scope() {
    let state = AuthState::new("https://example.com".to_owned());
    let response = state.unauthorized_response();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response
            .headers()
            .get("www-authenticate")
            .expect("WWW-Authenticate header")
            .to_str()
            .expect("valid WWW-Authenticate header"),
        "Bearer error=\"invalid_token\", error_description=\"Authentication required\", resource_metadata=\"https://example.com/.well-known/oauth-protected-resource/mcp\", scope=\"mcp\""
    );
}

#[tokio::test]
async fn protected_resource_metadata_matches_mcp_resource_identifier() {
    let state = Arc::new(AuthState::new("https://example.com".to_owned()));
    let metadata =
        response_json(protected_resource_metadata(State(state), host_headers("example.com")).await)
            .await;
    assert_eq!(metadata["resource"], "https://example.com/mcp");
    assert_eq!(metadata["authorization_servers"][0], "https://example.com");
    assert_eq!(metadata["scopes_supported"][0], "mcp");
}

#[tokio::test]
async fn authorization_metadata_prefers_dcr_until_cimd_is_safely_supported() {
    let state = Arc::new(AuthState::new("https://example.com".to_owned()));
    let metadata = response_json(
        authorization_server_metadata(State(state), host_headers("example.com")).await,
    )
    .await;
    assert_eq!(metadata["issuer"], "https://example.com");
    assert_eq!(
        metadata["registration_endpoint"],
        "https://example.com/register"
    );
    assert_eq!(metadata["client_id_metadata_document_supported"], false);
    assert_eq!(metadata["token_endpoint_auth_methods_supported"][0], "none");
    assert_eq!(metadata["code_challenge_methods_supported"][0], "S256");
}

#[test]
fn redirect_uri_policy_allows_https_and_loopback_only() {
    assert!(valid_redirect_uri("https://chatgpt.com/callback"));
    assert!(valid_redirect_uri("http://127.0.0.1:8765/callback"));
    assert!(valid_redirect_uri("http://localhost:8765/callback"));
    assert!(!valid_redirect_uri("http://example.com/callback"));
    assert!(!valid_redirect_uri("file:///tmp/callback"));
    assert!(!valid_redirect_uri("https://user@example.com/callback"));
    assert!(!valid_redirect_uri("https://example.com/callback#fragment"));
}

#[test]
fn modern_mcp_registration_profiles_cover_web_and_native_agents() {
    let web = RegistrationRequest {
        redirect_uris: vec!["https://grok.com/oauth/callback".to_owned()],
        client_name: Some("Grok".to_owned()),
        application_type: Some("web".to_owned()),
        grant_types: vec!["authorization_code".to_owned(), "refresh_token".to_owned()],
        response_types: vec!["code".to_owned()],
        token_endpoint_auth_method: Some("none".to_owned()),
        scope: Some("mcp".to_owned()),
    };
    let web_profile = registration_profile(&web).unwrap();
    assert_eq!(web_profile.application_type, "web");
    assert_eq!(
        web_profile.grant_types,
        ["authorization_code", "refresh_token"]
    );
    assert_eq!(web_profile.response_types, ["code"]);
    assert_eq!(web_profile.scope, "mcp");

    let native = RegistrationRequest {
        redirect_uris: vec!["http://127.0.0.1:43123/callback".to_owned()],
        client_name: Some("Local coding agent".to_owned()),
        ..Default::default()
    };
    let native_profile = registration_profile(&native).unwrap();
    assert_eq!(native_profile.application_type, "native");

    let confidential = RegistrationRequest {
        redirect_uris: vec!["https://agent.example/callback".to_owned()],
        token_endpoint_auth_method: Some("client_secret_basic".to_owned()),
        ..Default::default()
    };
    assert!(registration_profile(&confidential).is_err());
}

#[tokio::test]
async fn registration_is_bounded() {
    let state = Arc::new(AuthState::new("https://example.com".to_owned()));
    {
        let mut clients = state.clients.lock().unwrap();
        for index in 0..MAX_REGISTERED_CLIENTS {
            clients.insert(
                format!("client-{index}"),
                Client {
                    redirect_uris: vec!["https://chatgpt.com/callback".to_owned()],
                },
            );
        }
    }
    let response = register_client(
        State(state),
        Json(RegistrationRequest {
            redirect_uris: vec!["https://chatgpt.com/callback".to_owned()],
            client_name: Some("ChatGPT".to_owned()),
            application_type: Some("web".to_owned()),
            grant_types: vec!["authorization_code".to_owned(), "refresh_token".to_owned()],
            response_types: vec!["code".to_owned()],
            token_endpoint_auth_method: Some("none".to_owned()),
            scope: Some("mcp".to_owned()),
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn chatgpt_oauth_callback_includes_issuer_and_exchanges_resource_bound_code() {
    let state = Arc::new(AuthState::new("https://example.com".to_owned()));
    let client_id = "chatgpt-test".to_owned();
    let redirect_uri = "https://chatgpt.com/connector_platform_oauth_redirect".to_owned();
    state.clients.lock().expect("client lock poisoned").insert(
        client_id.clone(),
        Client {
            redirect_uris: vec![redirect_uri.clone()],
        },
    );
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    let resource = "https://example.com/mcp".to_owned();
    let response = authorize_submit(
        State(state.clone()),
        host_headers("example.com"),
        Form(AuthorizeForm {
            client_id: client_id.clone(),
            redirect_uri: redirect_uri.clone(),
            state: "chatgpt-state".to_owned(),
            code_challenge: pkce_challenge(verifier),
            pairing_code: state.pairing_code().to_owned(),
            resource: Some(resource.clone()),
            scope: Some("mcp".to_owned()),
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let callback = Url::parse(
        response
            .headers()
            .get(LOCATION)
            .expect("redirect location")
            .to_str()
            .expect("valid redirect location"),
    )
    .expect("valid callback URL");
    let params = callback
        .query_pairs()
        .into_owned()
        .collect::<HashMap<_, _>>();
    assert_eq!(
        params.get("state").map(String::as_str),
        Some("chatgpt-state")
    );
    assert_eq!(
        params.get("iss").map(String::as_str),
        Some("https://example.com")
    );

    let token_response = token(
        State(state.clone()),
        host_headers("example.com"),
        Form(TokenForm {
            grant_type: "authorization_code".to_owned(),
            code: params.get("code").cloned(),
            redirect_uri: Some(redirect_uri.clone()),
            client_id: Some(client_id.clone()),
            code_verifier: Some(verifier.to_owned()),
            refresh_token: None,
            resource: Some(resource.clone()),
        }),
    )
    .await;
    assert_eq!(token_response.status(), StatusCode::OK);

    let replay = token(
        State(state.clone()),
        host_headers("example.com"),
        Form(TokenForm {
            grant_type: "authorization_code".to_owned(),
            code: params.get("code").cloned(),
            redirect_uri: Some(redirect_uri),
            client_id: Some(client_id),
            code_verifier: Some(verifier.to_owned()),
            refresh_token: None,
            resource: Some(resource),
        }),
    )
    .await;
    assert_eq!(replay.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn resource_bound_authorization_code_allows_legacy_token_request_to_inherit_resource() {
    let state = AuthState::new("https://example.com".to_owned());
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    state.codes.lock().unwrap().insert(
        "legacy-resource-code".to_owned(),
        AuthorizationCode {
            client_id: "client".to_owned(),
            redirect_uri: "https://agent.example/callback".to_owned(),
            code_challenge: pkce_challenge(verifier),
            resource: Some("https://example.com/mcp".to_owned()),
            expires_at: Instant::now() + Duration::from_secs(60),
        },
    );
    let response = exchange_code(
        &state,
        TokenForm {
            grant_type: "authorization_code".to_owned(),
            code: Some("legacy-resource-code".to_owned()),
            redirect_uri: Some("https://agent.example/callback".to_owned()),
            client_id: Some("client".to_owned()),
            code_verifier: Some(verifier.to_owned()),
            refresh_token: None,
            resource: None,
        },
        "https://example.com/mcp",
    );
    assert_eq!(response.status(), StatusCode::OK);
    assert!(state
        .access_tokens
        .lock()
        .unwrap()
        .values()
        .all(|token| token.resource.as_deref() == Some("https://example.com/mcp")));
}

#[test]
fn expired_authorization_code_is_rejected_and_removed() {
    let state = AuthState::new("https://example.com".to_owned());
    let verifier = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
    state.codes.lock().unwrap().insert(
        "expired-code".to_owned(),
        AuthorizationCode {
            client_id: "client".to_owned(),
            redirect_uri: "https://chatgpt.com/callback".to_owned(),
            code_challenge: pkce_challenge(verifier),
            resource: Some("https://example.com/mcp".to_owned()),
            expires_at: Instant::now() - Duration::from_secs(1),
        },
    );
    let response = exchange_code(
        &state,
        TokenForm {
            grant_type: "authorization_code".to_owned(),
            code: Some("expired-code".to_owned()),
            redirect_uri: Some("https://chatgpt.com/callback".to_owned()),
            client_id: Some("client".to_owned()),
            code_verifier: Some(verifier.to_owned()),
            refresh_token: None,
            resource: Some("https://example.com/mcp".to_owned()),
        },
        "https://example.com/mcp",
    );
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(state.codes.lock().unwrap().is_empty());
}

#[test]
fn old_refresh_token_remains_valid_for_runtime_lifetime() {
    let state = AuthState::new("https://example.com".to_owned());
    state.refresh_tokens.lock().unwrap().insert(
        "old-refresh".to_owned(),
        RefreshToken {
            issued_at_ms: 1,
            client_id: "client".to_owned(),
            resource: Some("https://example.com/mcp".to_owned()),
        },
    );
    let response = refresh_access_token(
        &state,
        TokenForm {
            grant_type: "refresh_token".to_owned(),
            code: None,
            redirect_uri: None,
            client_id: None,
            code_verifier: None,
            refresh_token: Some("old-refresh".to_owned()),
            resource: None,
        },
        "https://example.com/mcp",
    );
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!state.refresh_tokens.lock().unwrap().is_empty());
}

#[test]
fn old_access_tokens_follow_verified_tunnel_aliases_without_expiry() {
    let state = AuthState::new("https://example.com".to_owned());
    let token = "access_expired".to_owned();
    state.access_tokens.lock().unwrap().insert(
        token.clone(),
        AccessToken {
            issued_at_ms: 1,
            client_id: "client".to_owned(),
            resource: Some("https://example.com/mcp".to_owned()),
        },
    );
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "example.com".parse().unwrap());
    headers.insert(
        "authorization",
        format!("Bearer {token}")
            .parse()
            .expect("valid bearer header"),
    );
    assert!(state.authorized(&headers));

    state.set_public_url("https://other.example.com".to_owned());
    assert!(state.authorized(&headers));

    headers.insert(HOST, "other.example.com".parse().unwrap());
    assert!(state.authorized(&headers));
    headers.insert(HOST, "unknown.example.com".parse().unwrap());
    assert!(!state.authorized(&headers));
    assert_eq!(state.access_tokens.lock().unwrap().len(), 1);
}

#[test]
fn refresh_token_moves_to_a_verified_reconnected_tunnel() {
    let state = AuthState::new("https://one.example".to_owned());
    state.set_public_url("https://two.example".to_owned());
    state.refresh_tokens.lock().unwrap().insert(
        "old-refresh".to_owned(),
        RefreshToken {
            issued_at_ms: 1,
            client_id: "client".to_owned(),
            resource: Some("https://one.example/mcp".to_owned()),
        },
    );

    let response = refresh_access_token(
        &state,
        TokenForm {
            grant_type: "refresh_token".to_owned(),
            code: None,
            redirect_uri: None,
            client_id: Some("client".to_owned()),
            code_verifier: None,
            refresh_token: Some("old-refresh".to_owned()),
            resource: None,
        },
        "https://two.example/mcp",
    );
    assert_eq!(response.status(), StatusCode::OK);
    assert!(state
        .refresh_tokens
        .lock()
        .unwrap()
        .values()
        .all(|token| token.resource.as_deref() == Some("https://two.example/mcp")));
}

#[tokio::test]
async fn oauth_metadata_uses_the_tunnel_that_received_the_request() {
    let state = Arc::new(AuthState::new("https://one.example".to_owned()));
    state.set_public_url("https://two.example".to_owned());

    let first = response_json(
        authorization_server_metadata(State(state.clone()), host_headers("one.example")).await,
    )
    .await;
    let second = response_json(
        authorization_server_metadata(State(state), host_headers("two.example")).await,
    )
    .await;

    assert_eq!(first["issuer"], "https://one.example");
    assert_eq!(
        first["authorization_endpoint"],
        "https://one.example/authorize"
    );
    assert_eq!(second["issuer"], "https://two.example");
}

#[tokio::test]
async fn token_response_does_not_advertise_access_token_expiry() {
    let state = AuthState::new("https://example.com".to_owned());
    let response = issue_tokens(
        &state,
        "client".to_owned(),
        Some("https://example.com/mcp".to_owned()),
    );
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("token response body");
    let value: Value = serde_json::from_slice(&body).expect("valid token response JSON");
    assert!(value.get("access_token").is_some());
    assert!(value.get("refresh_token").is_some());
    assert!(value.get("expires_in").is_none());
}

#[test]
fn refresh_tokens_rotate_and_preserve_binding() {
    let state = AuthState::new("https://example.com".to_owned());
    let client_id = "client".to_owned();
    let resource = Some("https://example.com/mcp".to_owned());
    let response = issue_tokens(&state, client_id.clone(), resource.clone());
    assert_eq!(response.status(), StatusCode::OK);
    let old_refresh = state
        .refresh_tokens
        .lock()
        .unwrap()
        .keys()
        .next()
        .expect("refresh token")
        .clone();

    let refreshed = refresh_access_token(
        &state,
        TokenForm {
            grant_type: "refresh_token".to_owned(),
            code: None,
            redirect_uri: None,
            client_id: Some(client_id.clone()),
            code_verifier: None,
            refresh_token: Some(old_refresh.clone()),
            resource: resource.clone(),
        },
        "https://example.com/mcp",
    );
    assert_eq!(refreshed.status(), StatusCode::OK);
    let tokens = state.refresh_tokens.lock().unwrap();
    assert!(!tokens.contains_key(&old_refresh));
    assert_eq!(tokens.len(), 1);
    let new_refresh = tokens.keys().next().unwrap().clone();
    drop(tokens);

    let replay = refresh_access_token(
        &state,
        TokenForm {
            grant_type: "refresh_token".to_owned(),
            code: None,
            redirect_uri: None,
            client_id: Some(client_id.clone()),
            code_verifier: None,
            refresh_token: Some(old_refresh),
            resource: resource.clone(),
        },
        "https://example.com/mcp",
    );
    assert_eq!(replay.status(), StatusCode::BAD_REQUEST);

    let second = refresh_access_token(
        &state,
        TokenForm {
            grant_type: "refresh_token".to_owned(),
            code: None,
            redirect_uri: None,
            client_id: Some(client_id),
            code_verifier: None,
            refresh_token: Some(new_refresh),
            resource,
        },
        "https://example.com/mcp",
    );
    assert_eq!(second.status(), StatusCode::OK);
}

#[test]
fn issued_token_state_remains_bounded() {
    let state = AuthState::new("https://example.com".to_owned());
    for _ in 0..MAX_ACCESS_TOKENS + 16 {
        let response = issue_tokens(
            &state,
            "client".to_owned(),
            Some("https://example.com/mcp".to_owned()),
        );
        assert_eq!(response.status(), StatusCode::OK);
    }
    assert!(state.access_tokens.lock().unwrap().len() <= MAX_ACCESS_TOKENS);
    assert!(state.refresh_tokens.lock().unwrap().len() <= MAX_REFRESH_TOKENS);
}

#[test]
fn refresh_token_binding_mismatch_does_not_rotate() {
    let state = AuthState::new("https://example.com".to_owned());
    issue_tokens(
        &state,
        "client".to_owned(),
        Some("https://example.com/mcp".to_owned()),
    );
    let refresh = state
        .refresh_tokens
        .lock()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    let rejected = refresh_access_token(
        &state,
        TokenForm {
            grant_type: "refresh_token".to_owned(),
            code: None,
            redirect_uri: None,
            client_id: Some("other-client".to_owned()),
            code_verifier: None,
            refresh_token: Some(refresh.clone()),
            resource: None,
        },
        "https://example.com/mcp",
    );
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    assert!(state.refresh_tokens.lock().unwrap().contains_key(&refresh));
}

#[path = "auth/session.rs"]
mod session;
