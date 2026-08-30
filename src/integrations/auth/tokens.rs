use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct AccessToken {
    pub(super) issued_at_ms: u64,
    pub(super) client_id: String,
    pub(super) resource: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct RefreshToken {
    pub(super) issued_at_ms: u64,
    pub(super) client_id: String,
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
        if saved.resource.as_deref().is_none_or(|resource| {
            !state
                .public_endpoints
                .equivalent_mcp_resources(resource, expected_resource)
        }) || form
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
    issue_tokens_locked(state, client_id, resource, None)
}

fn issue_tokens_locked(
    state: &AuthState,
    client_id: String,
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
                resource: resource.clone(),
            },
        );
    }
    {
        let mut tokens = state.refresh_tokens.lock().expect("refresh lock poisoned");
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
    Json(json!({
        "access_token": access,
        "token_type": "Bearer",
        "refresh_token": refresh,
        "scope": "mcp",
    }))
    .into_response()
}
