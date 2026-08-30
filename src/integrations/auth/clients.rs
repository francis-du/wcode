use super::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Client {
    pub(super) redirect_uris: Vec<String>,
}

#[derive(Deserialize, Default)]
pub(super) struct RegistrationRequest {
    #[serde(default)]
    pub(super) redirect_uris: Vec<String>,
    #[serde(default)]
    pub(super) client_name: Option<String>,
    #[serde(default)]
    pub(super) application_type: Option<String>,
    #[serde(default)]
    pub(super) grant_types: Vec<String>,
    #[serde(default)]
    pub(super) response_types: Vec<String>,
    #[serde(default)]
    pub(super) token_endpoint_auth_method: Option<String>,
    #[serde(default)]
    pub(super) scope: Option<String>,
}

pub(super) struct RegistrationProfile {
    pub(super) application_type: String,
    pub(super) grant_types: Vec<String>,
    pub(super) response_types: Vec<String>,
    pub(super) scope: String,
}

pub(super) fn registration_profile(
    request: &RegistrationRequest,
) -> Result<RegistrationProfile, &'static str> {
    let application_type = request.application_type.as_deref().unwrap_or_else(|| {
        if request.redirect_uris.iter().any(|uri| {
            Url::parse(uri).ok().is_some_and(|url| {
                url.scheme() == "http"
                    && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
            })
        }) {
            "native"
        } else {
            "web"
        }
    });
    if !matches!(application_type, "native" | "web") {
        return Err("invalid_client_metadata");
    }
    if request
        .token_endpoint_auth_method
        .as_deref()
        .is_some_and(|method| method != "none")
    {
        return Err("invalid_client_metadata");
    }
    if request
        .grant_types
        .iter()
        .any(|grant| !matches!(grant.as_str(), "authorization_code" | "refresh_token"))
        || (!request.grant_types.is_empty()
            && !request
                .grant_types
                .iter()
                .any(|grant| grant == "authorization_code"))
    {
        return Err("invalid_client_metadata");
    }
    if request
        .response_types
        .iter()
        .any(|response| response != "code")
    {
        return Err("invalid_client_metadata");
    }
    if request.scope.as_deref().is_some_and(|scope| {
        let scopes = scope.split_ascii_whitespace().collect::<Vec<_>>();
        scopes.is_empty() || scopes.iter().any(|scope| *scope != "mcp")
    }) {
        return Err("invalid_client_metadata");
    }
    let grant_types = if request.grant_types.is_empty() {
        vec!["authorization_code".to_owned(), "refresh_token".to_owned()]
    } else {
        let mut grants = request.grant_types.clone();
        grants.sort();
        grants.dedup();
        grants
    };
    let response_types = if request.response_types.is_empty() {
        vec!["code".to_owned()]
    } else {
        let mut responses = request.response_types.clone();
        responses.dedup();
        responses
    };
    Ok(RegistrationProfile {
        application_type: application_type.to_owned(),
        grant_types,
        response_types,
        scope: "mcp".to_owned(),
    })
}

pub(super) async fn register_client(
    State(state): State<Arc<AuthState>>,
    Json(request): Json<RegistrationRequest>,
) -> Response {
    if request.redirect_uris.is_empty()
        || request.redirect_uris.len() > MAX_REDIRECT_URIS_PER_CLIENT
        || request
            .redirect_uris
            .iter()
            .any(|uri| uri.chars().count() > MAX_REDIRECT_URI_CHARS || !valid_redirect_uri(uri))
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_redirect_uri"})),
        )
            .into_response();
    }
    if request
        .client_name
        .as_deref()
        .is_some_and(|name| name.chars().count() > MAX_CLIENT_NAME_CHARS)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_client_metadata"})),
        )
            .into_response();
    }
    let profile = match registration_profile(&request) {
        Ok(profile) => profile,
        Err(error) => {
            return (StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response()
        }
    };
    let client_id = format!("wcode-{}", Uuid::new_v4());
    let _mutation = state
        .mutation_lock
        .lock()
        .expect("auth mutation lock poisoned");
    let mut clients = state.clients.lock().expect("client lock poisoned");
    let previous = state.store.as_ref().map(|_| clients.clone());
    if !make_client_room(&state, &mut clients) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error": "server_error"})),
        )
            .into_response();
    }
    clients.insert(
        client_id.clone(),
        Client {
            redirect_uris: request.redirect_uris.clone(),
        },
    );
    drop(clients);
    if let Err(error) = state.persist() {
        let mut clients = state.clients.lock().expect("client lock poisoned");
        if let Some(previous) = previous {
            *clients = previous;
        } else {
            clients.remove(&client_id);
        }
        tracing::error!(%error, "cannot persist OAuth client registration");
        return oauth_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error");
    }
    if let Some(monitor) = &state.monitor {
        monitor.mark_oauth_client_registered();
    }
    let issued_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    (
        StatusCode::CREATED,
        Json(json!({
            "client_id": client_id,
            "client_id_issued_at": issued_at,
            "client_name": request.client_name.unwrap_or_else(|| "MCP client".into()),
            "redirect_uris": request.redirect_uris,
            "application_type": profile.application_type,
            "scope": profile.scope,
            "token_endpoint_auth_method": "none",
            "grant_types": profile.grant_types,
            "response_types": profile.response_types,
        })),
    )
        .into_response()
}

pub(super) fn valid_redirect_uri(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    if url.fragment().is_some() || !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    match url.scheme() {
        "https" => url.host_str().is_some(),
        "http" => matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1")),
        _ => false,
    }
}

fn make_client_room(state: &AuthState, clients: &mut HashMap<String, Client>) -> bool {
    if clients.len() < MAX_REGISTERED_CLIENTS {
        return true;
    }
    if state.store.is_none() {
        return false;
    }
    let mut bound = HashSet::new();
    bound.extend(
        state
            .access_tokens
            .lock()
            .expect("token lock poisoned")
            .values()
            .map(|token| token.client_id.clone()),
    );
    bound.extend(
        state
            .refresh_tokens
            .lock()
            .expect("refresh lock poisoned")
            .values()
            .map(|token| token.client_id.clone()),
    );
    let reclaimable = clients
        .keys()
        .find(|client_id| !bound.contains(*client_id))
        .cloned();
    reclaimable.is_some_and(|client_id| clients.remove(&client_id).is_some())
}

pub(super) fn validate_authorize_request(
    state: &AuthState,
    query: &AuthorizeQuery,
    public_url: &str,
) -> Result<(), (StatusCode, &'static str)> {
    if query.response_type.as_deref().unwrap_or("code") != "code"
        || query.code_challenge_method.as_deref().unwrap_or("S256") != "S256"
        || !valid_pkce_challenge(&query.code_challenge)
        || query
            .resource
            .as_deref()
            .is_some_and(|resource| resource != format!("{public_url}/mcp"))
    {
        return Err((StatusCode::BAD_REQUEST, "invalid_request"));
    }
    if query.scope.as_deref().is_some_and(|scope| {
        let scopes = scope.split_ascii_whitespace().collect::<Vec<_>>();
        scopes.is_empty() || scopes.iter().any(|scope| *scope != "mcp")
    }) {
        return Err((StatusCode::BAD_REQUEST, "invalid_scope"));
    }
    {
        let clients = state.clients.lock().expect("client lock poisoned");
        if let Some(client) = clients.get(&query.client_id) {
            return if client.redirect_uris.contains(&query.redirect_uri) {
                Ok(())
            } else {
                Err((StatusCode::BAD_REQUEST, "invalid_redirect_uri"))
            };
        }
    }
    validate_legacy_client(&query.client_id, &query.redirect_uri)
}

fn validate_legacy_client(
    client_id: &str,
    redirect_uri: &str,
) -> Result<(), (StatusCode, &'static str)> {
    let legacy_id = client_id
        .strip_prefix("wcode-")
        .and_then(|value| Uuid::parse_str(value).ok())
        .is_some_and(|uuid| uuid.get_version_num() == 4);
    if !legacy_id {
        return Err((StatusCode::BAD_REQUEST, "invalid_client"));
    }
    if redirect_uri.chars().count() > MAX_REDIRECT_URI_CHARS || !valid_redirect_uri(redirect_uri) {
        return Err((StatusCode::BAD_REQUEST, "invalid_redirect_uri"));
    }
    Ok(())
}

pub(super) fn persist_recovered_client(
    state: &AuthState,
    client_id: &str,
    redirect_uri: &str,
) -> Result<(), (StatusCode, &'static str)> {
    let _mutation = state
        .mutation_lock
        .lock()
        .expect("auth mutation lock poisoned");
    let mut clients = state.clients.lock().expect("client lock poisoned");
    if let Some(client) = clients.get(client_id) {
        return if client.redirect_uris.iter().any(|uri| uri == redirect_uri) {
            Ok(())
        } else {
            Err((StatusCode::BAD_REQUEST, "invalid_redirect_uri"))
        };
    }
    validate_legacy_client(client_id, redirect_uri)?;
    let previous = state.store.as_ref().map(|_| clients.clone());
    if !make_client_room(state, &mut clients) {
        return Err((StatusCode::TOO_MANY_REQUESTS, "server_error"));
    }
    clients.insert(
        client_id.to_owned(),
        Client {
            redirect_uris: vec![redirect_uri.to_owned()],
        },
    );
    drop(clients);
    if let Err(error) = state.persist() {
        let mut clients = state.clients.lock().expect("client lock poisoned");
        if let Some(previous) = previous {
            *clients = previous;
        } else {
            clients.remove(client_id);
        }
        tracing::error!(%error, "cannot persist recovered OAuth client");
        return Err((StatusCode::INTERNAL_SERVER_ERROR, "server_error"));
    }
    Ok(())
}
