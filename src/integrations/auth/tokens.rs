use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct AccessToken {
    pub(super) issued_at_ms: u64,
    pub(super) client_id: String,
    #[serde(default)]
    pub(super) owner_id: String,
    pub(super) resource: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct RefreshToken {
    pub(super) issued_at_ms: u64,
    pub(super) client_id: String,
    #[serde(default)]
    pub(super) owner_id: String,
    pub(super) resource: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct TokenForm {
    pub(super) grant_type: String,
    #[serde(default)]
    pub(super) code: Option<String>,
    #[serde(default)]
    pub(super) redirect_uri: Option<String>,
    #[serde(default)]
    pub(super) client_id: Option<String>,
    #[serde(default)]
    pub(super) code_verifier: Option<String>,
    #[serde(default)]
    pub(super) refresh_token: Option<String>,
    #[serde(default)]
    pub(super) resource: Option<String>,
}

pub(super) async fn token(
    State(state): State<Arc<AuthState>>,
    headers: HeaderMap,
    Form(form): Form<TokenForm>,
) -> Response {
    let Some(base) = state.request_public_url(&headers) else {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    let expected_resource = format!("{base}/mcp");
    match form.grant_type.as_str() {
        "authorization_code" => exchange_code(&state, form, &expected_resource),
        "refresh_token" => refresh_access_token(&state, form, &expected_resource),
        _ => oauth_error(StatusCode::BAD_REQUEST, "unsupported_grant_type"),
    }
}

pub(super) fn exchange_code(
    state: &AuthState,
    form: TokenForm,
    expected_resource: &str,
) -> Response {
    let (Some(code), Some(client_id), Some(redirect_uri), Some(verifier)) = (
        form.code,
        form.client_id,
        form.redirect_uri,
        form.code_verifier,
    ) else {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    let Some(saved) = state
        .codes
        .lock()
        .expect("code lock poisoned")
        .remove(&code)
    else {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant");
    };
    if saved.expires_at < Instant::now()
        || saved.client_id != client_id
        || saved.redirect_uri != redirect_uri
        || !valid_pkce_verifier(&verifier)
        || pkce_challenge(&verifier) != saved.code_challenge
        || saved.resource.as_deref().is_none_or(|resource| {
            !state
                .public_endpoints
                .equivalent_mcp_resources(resource, expected_resource)
        })
        || form.resource.as_deref().is_some_and(|resource| {
            !state
                .public_endpoints
                .equivalent_mcp_resources(resource, expected_resource)
        })
    {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant");
    }
    if let Some(monitor) = &state.monitor {
        monitor.mark_oauth_authorized();
    }
    issue_tokens(state, saved.client_id, Some(expected_resource.to_owned()))
}

pub(super) fn refresh_access_token(
    state: &AuthState,
    form: TokenForm,
    expected_resource: &str,
) -> Response {
    let Some(refresh) = form.refresh_token else {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    let _mutation = state
        .mutation_lock
        .lock()
        .expect("auth mutation lock poisoned");
    let saved = {
        let tokens = state.refresh_tokens.lock().expect("refresh lock poisoned");
        let Some(saved) = tokens.get(&refresh) else {
            return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant");
        };
        if !credential_current(saved.issued_at_ms, REFRESH_TOKEN_TTL_MS, epoch_ms())
            || saved.resource.as_deref().is_none_or(|resource| {
                !state
                    .public_endpoints
                    .equivalent_mcp_resources(resource, expected_resource)
            })
            || form
                .client_id
                .as_deref()
                .is_some_and(|client_id| client_id != saved.client_id)
            || form.resource.as_deref().is_some_and(|resource| {
                !state
                    .public_endpoints
                    .equivalent_mcp_resources(resource, expected_resource)
            })
        {
            return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant");
        }
        saved.clone()
    };
    issue_tokens_locked(
        state,
        saved.client_id,
        saved.owner_id,
        Some(expected_resource.to_owned()),
        Some(&refresh),
    )
}

pub(super) fn issue_tokens(
    state: &AuthState,
    client_id: String,
    resource: Option<String>,
) -> Response {
    let _mutation = state
        .mutation_lock
        .lock()
        .expect("auth mutation lock poisoned");
    issue_tokens_locked(state, client_id, random_token("owner"), resource, None)
}

fn issue_tokens_locked(
    state: &AuthState,
    client_id: String,
    owner_id: String,
    resource: Option<String>,
    rotated_refresh: Option<&str>,
) -> Response {
    let previous = state.store.as_ref().map(|_| {
        (
            state
                .access_tokens
                .lock()
                .expect("token lock poisoned")
                .clone(),
            state
                .refresh_tokens
                .lock()
                .expect("refresh lock poisoned")
                .clone(),
        )
    });
    let access = random_token("access");
    let refresh = random_token("refresh");
    let now = epoch_ms();
    {
        let mut tokens = state.access_tokens.lock().expect("token lock poisoned");
        tokens.retain(|_, saved| credential_current(saved.issued_at_ms, ACCESS_TOKEN_TTL_MS, now));
        if rotated_refresh.is_some() {
            tokens.retain(|_, saved| saved.client_id != client_id || saved.owner_id != owner_id);
        }
        if tokens.len() >= MAX_ACCESS_TOKENS {
            if let Some(oldest) = tokens
                .iter()
                .min_by_key(|(_, saved)| saved.issued_at_ms)
                .map(|(token, _)| token.clone())
            {
                tokens.remove(&oldest);
            }
        }
        tokens.insert(
            access.clone(),
            AccessToken {
                issued_at_ms: now,
                client_id: client_id.clone(),
                owner_id: owner_id.clone(),
                resource: resource.clone(),
            },
        );
    }
    {
        let mut tokens = state.refresh_tokens.lock().expect("refresh lock poisoned");
        tokens.retain(|_, saved| credential_current(saved.issued_at_ms, REFRESH_TOKEN_TTL_MS, now));
        if let Some(rotated) = rotated_refresh {
            tokens.remove(rotated);
        }
        if tokens.len() >= MAX_REFRESH_TOKENS {
            if let Some(oldest) = tokens
                .iter()
                .min_by_key(|(_, saved)| saved.issued_at_ms)
                .map(|(token, _)| token.clone())
            {
                tokens.remove(&oldest);
            }
        }
        tokens.insert(
            refresh.clone(),
            RefreshToken {
                issued_at_ms: now,
                client_id,
                owner_id,
                resource,
            },
        );
    }
    if let Err(error) = state.persist() {
        if let Some((access_tokens, refresh_tokens)) = previous {
            *state.access_tokens.lock().expect("token lock poisoned") = access_tokens;
            *state.refresh_tokens.lock().expect("refresh lock poisoned") = refresh_tokens;
        }
        tracing::error!(%error, "cannot persist OAuth tokens");
        return oauth_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error");
    }
    let mut response = Json(json!({
        "access_token": access,
        "token_type": "Bearer",
        "refresh_token": refresh,
        "expires_in": ACCESS_TOKEN_TTL_MS / 1_000,
        "scope": "mcp",
    }))
    .into_response();
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().expect("static header"));
    response
        .headers_mut()
        .insert("pragma", "no-cache".parse().expect("static header"));
    response
}

#[derive(Serialize)]
struct OAuthSession {
    id: String,
    client_id: String,
    resource: Option<String>,
    access_count: usize,
    refresh_count: usize,
    access_expires_at_ms: Option<u64>,
    refresh_expires_at_ms: Option<u64>,
}

fn oauth_session_id(client_id: &str, owner_id: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"wcode-oauth-session-v1\0");
    digest.update(client_id.as_bytes());
    digest.update(b"\0");
    digest.update(owner_id.as_bytes());
    format!("{:x}", digest.finalize())
}

fn operator_request_allowed(state: &AuthState, headers: &HeaderMap) -> bool {
    state.request_public_url(headers).is_some()
        && state.origin_allowed(headers)
        && state.ui_authorized(headers)
}

fn private_json(value: serde_json::Value) -> Response {
    let mut response = Json(value).into_response();
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().expect("static header"));
    response
}

pub(super) async fn oauth_sessions(
    State(state): State<Arc<AuthState>>,
    headers: HeaderMap,
) -> Response {
    if !operator_request_allowed(&state, &headers) {
        return oauth_error(StatusCode::FORBIDDEN, "operator_authorization_required");
    }
    let _mutation = state
        .mutation_lock
        .lock()
        .expect("auth mutation lock poisoned");
    let now = epoch_ms();
    let mut sessions = std::collections::BTreeMap::<String, OAuthSession>::new();
    for saved in state
        .access_tokens
        .lock()
        .expect("token lock poisoned")
        .values()
    {
        if !credential_current(saved.issued_at_ms, ACCESS_TOKEN_TTL_MS, now) {
            continue;
        }
        let id = oauth_session_id(&saved.client_id, &saved.owner_id);
        let session = sessions.entry(id.clone()).or_insert_with(|| OAuthSession {
            id,
            client_id: saved.client_id.clone(),
            resource: saved.resource.clone(),
            access_count: 0,
            refresh_count: 0,
            access_expires_at_ms: None,
            refresh_expires_at_ms: None,
        });
        session.access_count += 1;
        session.access_expires_at_ms = Some(
            session
                .access_expires_at_ms
                .unwrap_or(0)
                .max(saved.issued_at_ms.saturating_add(ACCESS_TOKEN_TTL_MS)),
        );
    }
    for saved in state
        .refresh_tokens
        .lock()
        .expect("refresh lock poisoned")
        .values()
    {
        if !credential_current(saved.issued_at_ms, REFRESH_TOKEN_TTL_MS, now) {
            continue;
        }
        let id = oauth_session_id(&saved.client_id, &saved.owner_id);
        let session = sessions.entry(id.clone()).or_insert_with(|| OAuthSession {
            id,
            client_id: saved.client_id.clone(),
            resource: saved.resource.clone(),
            access_count: 0,
            refresh_count: 0,
            access_expires_at_ms: None,
            refresh_expires_at_ms: None,
        });
        session.refresh_count += 1;
        session.refresh_expires_at_ms = Some(
            session
                .refresh_expires_at_ms
                .unwrap_or(0)
                .max(saved.issued_at_ms.saturating_add(REFRESH_TOKEN_TTL_MS)),
        );
    }
    private_json(
        json!({"sessions": sessions.into_values().collect::<Vec<_>>(), "authority": "local_operator"}),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RevokeSessionInput {
    pub(super) session_id: String,
}

pub(super) async fn oauth_session_revoke(
    State(state): State<Arc<AuthState>>,
    headers: HeaderMap,
    Json(input): Json<RevokeSessionInput>,
) -> Response {
    if !operator_request_allowed(&state, &headers) {
        return oauth_error(StatusCode::FORBIDDEN, "operator_authorization_required");
    }
    if input.session_id.len() != 64
        || !input
            .session_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_session_id");
    }
    let _mutation = state
        .mutation_lock
        .lock()
        .expect("auth mutation lock poisoned");
    let removed_access = {
        let mut saved = state.access_tokens.lock().expect("token lock poisoned");
        let before = saved.len();
        saved.retain(|_, grant| {
            oauth_session_id(&grant.client_id, &grant.owner_id) != input.session_id
        });
        before - saved.len()
    };
    let removed_refresh = {
        let mut saved = state.refresh_tokens.lock().expect("refresh lock poisoned");
        let before = saved.len();
        saved.retain(|_, grant| {
            oauth_session_id(&grant.client_id, &grant.owner_id) != input.session_id
        });
        before - saved.len()
    };
    // Do not restore live credentials after a failed revoke write or acknowledge durability.
    if state.persist().is_err() {
        tracing::error!("cannot persist OAuth session revocation");
        return oauth_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "revocation_not_persisted",
        );
    }
    private_json(
        json!({"revoked": true, "removed_access": removed_access, "removed_refresh": removed_refresh}),
    )
}
