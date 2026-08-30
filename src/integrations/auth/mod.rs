use crate::auth_origin::PublicEndpoints;
use crate::monitor::TaskMonitor;
use crate::{AUTHOR_HANDLE, AUTHOR_URL, PROJECT_URL};
use anyhow::Result;
use axum::extract::{Form, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Deserialize;
use serde_json::json;
#[cfg(test)]
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use url::Url;
use uuid::Uuid;

mod clients;
mod pages;
mod store;
mod tokens;
use clients::*;
use pages::*;
use store::AuthStore;
use tokens::*;

const AUTHORIZATION_CODE_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_REGISTERED_CLIENTS: usize = 128;
const MAX_PENDING_AUTHORIZATION_CODES: usize = 256;
const MAX_ACCESS_TOKENS: usize = 2_048;
const MAX_REFRESH_TOKENS: usize = 512;
const MAX_REDIRECT_URIS_PER_CLIENT: usize = 8;
const MAX_REDIRECT_URI_CHARS: usize = 2_048;
const MAX_CLIENT_NAME_CHARS: usize = 200;
const MAX_PAIRING_FAILURES_PER_CLIENT: u8 = 5;
const PAIRING_FAILURE_WINDOW: Duration = Duration::from_secs(60);
const PAIRING_LOCKOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Clone)]
pub struct AuthState {
    instance_id: String,
    public_endpoints: PublicEndpoints,
    pairing_code: String,
    ui_token: String,
    pairing_attempts: Arc<Mutex<HashMap<String, PairingAttempt>>>,
    clients: Arc<Mutex<HashMap<String, Client>>>,
    codes: Arc<Mutex<HashMap<String, AuthorizationCode>>>,
    access_tokens: Arc<Mutex<HashMap<String, AccessToken>>>,
    refresh_tokens: Arc<Mutex<HashMap<String, RefreshToken>>>,
    mutation_lock: Arc<Mutex<()>>,
    store: Option<Arc<AuthStore>>,
    monitor: Option<TaskMonitor>,
}

struct PairingAttempt {
    window_started_at: Instant,
    failures: u8,
    blocked_until: Option<Instant>,
}

struct AuthorizationCode {
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    resource: Option<String>,
    expires_at: Instant,
}

#[derive(Deserialize, Clone)]
struct AuthorizeQuery {
    client_id: String,
    redirect_uri: String,
    state: String,
    code_challenge: String,
    #[serde(default)]
    code_challenge_method: Option<String>,
    #[serde(default)]
    response_type: Option<String>,
    #[serde(default)]
    resource: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Deserialize)]
struct AuthorizeForm {
    client_id: String,
    redirect_uri: String,
    state: String,
    code_challenge: String,
    pairing_code: String,
    #[serde(default)]
    resource: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

impl AuthState {
    pub fn new(initial_public_url: String) -> Self {
        Self::from_parts(initial_public_url, None).expect("ephemeral auth state cannot fail")
    }

    fn from_parts(initial_public_url: String, store: Option<AuthStore>) -> Result<Self> {
        let pairing_code = format!("{:06}", Uuid::new_v4().as_u128() % 1_000_000);
        let ui_token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let saved = store
            .as_ref()
            .map(AuthStore::load)
            .transpose()?
            .unwrap_or_default();
        let public_endpoints = PublicEndpoints::new(initial_public_url);
        for resource in saved
            .access_tokens
            .values()
            .filter_map(|token| token.resource.as_deref())
        {
            public_endpoints.trust_resource(resource);
        }
        for resource in saved
            .refresh_tokens
            .values()
            .filter_map(|token| token.resource.as_deref())
        {
            public_endpoints.trust_resource(resource);
        }
        Ok(Self {
            instance_id: Uuid::new_v4().simple().to_string(),
            public_endpoints,
            pairing_code,
            ui_token,
            pairing_attempts: Default::default(),
            clients: Arc::new(Mutex::new(saved.clients)),
            codes: Default::default(),
            access_tokens: Arc::new(Mutex::new(saved.access_tokens)),
            refresh_tokens: Arc::new(Mutex::new(saved.refresh_tokens)),
            mutation_lock: Default::default(),
            store: store.map(Arc::new),
            monitor: None,
        })
    }

    pub fn new_with_monitor(
        initial_public_url: String,
        monitor: TaskMonitor,
        workspace_roots: &[PathBuf],
    ) -> Result<Self> {
        let mut state = Self::from_parts(
            initial_public_url,
            Some(AuthStore::for_workspaces(workspace_roots)?),
        )?;
        state.attach_monitor(monitor);
        Ok(state)
    }

    #[cfg(test)]
    fn new_persistent(initial_public_url: String, path: PathBuf) -> Result<Self> {
        Self::from_parts(initial_public_url, Some(AuthStore::at_path(path)))
    }

    #[cfg(test)]
    fn new_persistent_with_monitor(
        initial_public_url: String,
        path: PathBuf,
        monitor: TaskMonitor,
    ) -> Result<Self> {
        let mut state = Self::new_persistent(initial_public_url, path)?;
        state.attach_monitor(monitor);
        Ok(state)
    }

    fn attach_monitor(&mut self, monitor: TaskMonitor) {
        if !self
            .clients
            .lock()
            .expect("client lock poisoned")
            .is_empty()
        {
            monitor.mark_oauth_client_registered();
        }
        if !self
            .access_tokens
            .lock()
            .expect("token lock poisoned")
            .is_empty()
            || !self
                .refresh_tokens
                .lock()
                .expect("refresh lock poisoned")
                .is_empty()
        {
            monitor.mark_oauth_authorized();
        }
        self.monitor = Some(monitor);
    }

    fn persist(&self) -> Result<()> {
        let Some(store) = &self.store else {
            return Ok(());
        };
        store.save(
            &self.clients.lock().expect("client lock poisoned"),
            &self.access_tokens.lock().expect("token lock poisoned"),
            &self.refresh_tokens.lock().expect("refresh lock poisoned"),
        )
    }

    pub fn public_url(&self) -> String {
        self.public_endpoints.primary()
    }

    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    pub fn set_public_url(&self, value: String) {
        self.public_endpoints.set_primary(value);
    }

    pub fn register_public_url(&self, value: String) {
        self.public_endpoints.register(value);
    }

    pub fn unregister_public_url(&self, value: &str) {
        self.public_endpoints.unregister(value);
    }

    pub fn request_public_url(&self, headers: &HeaderMap) -> Option<String> {
        self.public_endpoints.for_headers(headers)
    }

    pub fn pairing_code(&self) -> &str {
        &self.pairing_code
    }

    pub fn ui_token(&self) -> &str {
        &self.ui_token
    }

    pub fn ui_authorized(&self, headers: &HeaderMap) -> bool {
        headers
            .get("x-wcode-ui-token")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| constant_time_text_eq(value, &self.ui_token))
    }

    #[cfg(test)]
    pub fn authorized(&self, headers: &HeaderMap) -> bool {
        self.authorized_client_fingerprint(headers).is_some()
    }

    #[cfg(test)]
    pub fn authorized_client_fingerprint(&self, headers: &HeaderMap) -> Option<String> {
        let public_url = self.request_public_url(headers)?;
        self.authorized_client_fingerprint_for(headers, &public_url)
    }

    pub fn authorized_client_fingerprint_for(
        &self,
        headers: &HeaderMap,
        public_url: &str,
    ) -> Option<String> {
        let value = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())?;
        let token = value.strip_prefix("Bearer ")?;
        let expected_resource = format!("{public_url}/mcp");
        let tokens = self.access_tokens.lock().expect("token lock poisoned");
        let saved = tokens.get(token)?;
        if saved.client_id.is_empty()
            || saved.resource.as_deref().is_some_and(|resource| {
                !self
                    .public_endpoints
                    .equivalent_mcp_resources(resource, &expected_resource)
            })
        {
            return None;
        }
        Some(format!("{:x}", Sha256::digest(saved.client_id.as_bytes())))
    }

    #[cfg(test)]
    pub fn unauthorized_response(&self) -> Response {
        self.unauthorized_response_for(&self.public_url())
    }

    pub fn unauthorized_response_for(&self, public_url: &str) -> Response {
        let metadata = format!("{}/.well-known/oauth-protected-resource/mcp", public_url);
        (
            StatusCode::UNAUTHORIZED,
            [(
                "www-authenticate",
                format!(
                    "Bearer error=\"invalid_token\", error_description=\"Authentication required\", resource_metadata=\"{metadata}\", scope=\"mcp\""
                ),
            )],
            Json(json!({
                "error": "invalid_token",
                "error_description": "Authentication required"
            })),
        )
            .into_response()
    }

    #[cfg(test)]
    pub(crate) fn insert_test_access_token(&self, token: &str, client_id: &str, resource: &str) {
        self.access_tokens
            .lock()
            .expect("token lock poisoned")
            .insert(
                token.to_owned(),
                AccessToken {
                    issued_at_ms: epoch_ms(),
                    client_id: client_id.to_owned(),
                    resource: Some(resource.to_owned()),
                },
            );
    }
}

pub fn router(state: Arc<AuthState>) -> Router {
    Router::new()
        .route(
            "/.well-known/oauth-protected-resource",
            get(protected_resource_metadata),
        )
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(protected_resource_metadata),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(authorization_server_metadata),
        )
        .route("/register", post(register_client))
        .route("/authorize", get(authorize_page).post(authorize_submit))
        .route("/authorize/", get(authorize_page).post(authorize_submit))
        .route("/token", post(token))
        .with_state(state)
}

async fn protected_resource_metadata(
    State(state): State<Arc<AuthState>>,
    headers: HeaderMap,
) -> Response {
    let Some(base) = state.request_public_url(&headers) else {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    Json(json!({
        "resource": format!("{base}/mcp"),
        "authorization_servers": [base],
        "bearer_methods_supported": ["header"],
        "scopes_supported": ["mcp"],
    }))
    .into_response()
}

async fn authorization_server_metadata(
    State(state): State<Arc<AuthState>>,
    headers: HeaderMap,
) -> Response {
    let Some(base) = state.request_public_url(&headers) else {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    Json(json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/authorize"),
        "token_endpoint": format!("{base}/token"),
        "registration_endpoint": format!("{base}/register"),
        "client_id_metadata_document_supported": false,
        "authorization_response_iss_parameter_supported": true,
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
        "scopes_supported": ["mcp"],
    }))
    .into_response()
}

async fn authorize_page(
    State(state): State<Arc<AuthState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let Some(base) = state.request_public_url(&headers) else {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    let Some(raw_query) = raw_query else {
        return Html(authorize_landing_html(&base)).into_response();
    };
    let query = match parse_authorize_query(&raw_query) {
        Ok(query) => query,
        Err(error) => {
            return (StatusCode::BAD_REQUEST, Html(authorize_error_html(error))).into_response()
        }
    };
    if let Err((status, error)) = validate_authorize_request(&state, &query, &base) {
        return (status, Html(authorize_error_html(error))).into_response();
    }
    Html(authorize_html(&query, None)).into_response()
}

fn parse_authorize_query(raw_query: &str) -> Result<AuthorizeQuery, &'static str> {
    const MAX_AUTHORIZE_QUERY_BYTES: usize = 16 * 1024;
    if raw_query.len() > MAX_AUTHORIZE_QUERY_BYTES {
        return Err("invalid_request");
    }

    let repaired = repair_authorize_query(raw_query)?;
    let mut values = HashMap::new();
    for (name, value) in url::form_urlencoded::parse(repaired.as_bytes()).into_owned() {
        if values.insert(name, value).is_some() {
            return Err("invalid_request");
        }
    }
    let required = |name: &str| {
        values
            .get(name)
            .filter(|value| !value.is_empty())
            .cloned()
            .ok_or("invalid_request")
    };
    Ok(AuthorizeQuery {
        client_id: required("client_id")?,
        redirect_uri: required("redirect_uri")?,
        state: required("state")?,
        code_challenge: required("code_challenge")?,
        code_challenge_method: values.get("code_challenge_method").cloned(),
        response_type: values.get("response_type").cloned(),
        resource: values.get("resource").cloned(),
        scope: values.get("scope").cloned(),
    })
}

fn repair_authorize_query(raw_query: &str) -> Result<String, &'static str> {
    if !raw_query.contains('\\') {
        return Ok(raw_query.to_owned());
    }

    let mut repaired = String::with_capacity(raw_query.len());
    let mut characters = raw_query.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\\' {
            repaired.push(character);
            continue;
        }
        if !matches!(characters.peek().copied(), Some('?' | '&' | '=')) {
            return Err("invalid_request");
        }
    }
    if repaired.starts_with('?') {
        repaired.remove(0);
    }
    Ok(repaired)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PairingCodeCheck {
    Accepted,
    Rejected,
    LockedOut,
}

fn check_pairing_code(state: &AuthState, client_id: &str, pairing_code: &str) -> PairingCodeCheck {
    let now = Instant::now();
    let mut attempts = state
        .pairing_attempts
        .lock()
        .expect("pairing attempt lock poisoned");

    if let Some(attempt) = attempts.get_mut(client_id) {
        if attempt.blocked_until.is_some_and(|until| until > now) {
            return PairingCodeCheck::LockedOut;
        }
        if attempt.blocked_until.is_some()
            || now.duration_since(attempt.window_started_at) >= PAIRING_FAILURE_WINDOW
        {
            attempt.window_started_at = now;
            attempt.failures = 0;
            attempt.blocked_until = None;
        }
    }

    if pairing_code == state.pairing_code {
        attempts.remove(client_id);
        return PairingCodeCheck::Accepted;
    }

    let attempt = attempts
        .entry(client_id.to_owned())
        .or_insert(PairingAttempt {
            window_started_at: now,
            failures: 0,
            blocked_until: None,
        });
    attempt.failures = attempt.failures.saturating_add(1);
    if attempt.failures >= MAX_PAIRING_FAILURES_PER_CLIENT {
        attempt.blocked_until = Some(now + PAIRING_LOCKOUT);
        PairingCodeCheck::LockedOut
    } else {
        PairingCodeCheck::Rejected
    }
}

async fn authorize_submit(
    State(state): State<Arc<AuthState>>,
    headers: HeaderMap,
    Form(form): Form<AuthorizeForm>,
) -> Response {
    let Some(base) = state.request_public_url(&headers) else {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    let query = AuthorizeQuery {
        client_id: form.client_id.clone(),
        redirect_uri: form.redirect_uri.clone(),
        state: form.state.clone(),
        code_challenge: form.code_challenge.clone(),
        code_challenge_method: Some("S256".into()),
        response_type: Some("code".into()),
        resource: form.resource.clone().filter(|value| !value.is_empty()),
        scope: form.scope.clone().filter(|value| !value.is_empty()),
    };
    if let Err((status, error)) = validate_authorize_request(&state, &query, &base) {
        return oauth_error(status, error);
    }
    match check_pairing_code(&state, &form.client_id, &form.pairing_code) {
        PairingCodeCheck::Accepted => {
            if let Err((status, error)) =
                persist_recovered_client(&state, &form.client_id, &form.redirect_uri)
            {
                return oauth_error(status, error);
            }
        }
        PairingCodeCheck::Rejected => {
            return (
                StatusCode::FORBIDDEN,
                Html(authorize_html(
                    &query,
                    Some(
                        "That pairing code is not valid. Check the code shown in your wcode terminal.",
                    ),
                )),
            )
                .into_response();
        }
        PairingCodeCheck::LockedOut => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Html(authorize_html(
                    &query,
                    Some("Too many invalid pairing attempts. Try again in a few minutes."),
                )),
            )
                .into_response();
        }
    }
    let code = random_token("code");
    let now = Instant::now();
    let mut codes = state.codes.lock().expect("code lock poisoned");
    codes.retain(|_, saved| saved.expires_at > now);
    if codes.len() >= MAX_PENDING_AUTHORIZATION_CODES {
        return oauth_error(StatusCode::TOO_MANY_REQUESTS, "temporarily_unavailable");
    }
    codes.insert(
        code.clone(),
        AuthorizationCode {
            client_id: form.client_id,
            redirect_uri: form.redirect_uri.clone(),
            code_challenge: form.code_challenge,
            resource: Some(format!("{base}/mcp")),
            expires_at: now + AUTHORIZATION_CODE_TTL,
        },
    );
    drop(codes);
    let mut redirect = match Url::parse(&form.redirect_uri) {
        Ok(url) => url,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    redirect
        .query_pairs_mut()
        .append_pair("code", &code)
        .append_pair("state", &form.state)
        .append_pair("iss", &base);
    Redirect::to(redirect.as_str()).into_response()
}

fn valid_pkce_challenge(challenge: &str) -> bool {
    challenge.len() == 43
        && challenge
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_pkce_verifier(verifier: &str) -> bool {
    (43..=128).contains(&verifier.len())
        && verifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
}

fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn random_token(prefix: &str) -> String {
    format!(
        "{prefix}_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn oauth_error(status: StatusCode, error: &str) -> Response {
    (status, Json(json!({"error": error}))).into_response()
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn constant_time_text_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.as_bytes()
        .iter()
        .zip(right.as_bytes())
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/auth.rs"]
mod tests;
