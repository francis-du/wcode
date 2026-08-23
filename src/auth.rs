use crate::monitor::TaskMonitor;
use crate::{AUTHOR_HANDLE, AUTHOR_URL, PROJECT_URL};
use axum::extract::{Form, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use url::Url;
use uuid::Uuid;

const AUTHORIZATION_CODE_TTL: Duration = Duration::from_secs(5 * 60);
const REFRESH_TOKEN_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
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
    public_url: Arc<RwLock<String>>,
    pairing_code: String,
    pairing_attempts: Arc<Mutex<HashMap<String, PairingAttempt>>>,
    clients: Arc<Mutex<HashMap<String, Client>>>,
    codes: Arc<Mutex<HashMap<String, AuthorizationCode>>>,
    access_tokens: Arc<Mutex<HashMap<String, AccessToken>>>,
    refresh_tokens: Arc<Mutex<HashMap<String, RefreshToken>>>,
    monitor: Option<TaskMonitor>,
}

#[derive(Clone)]
struct AccessToken {
    issued_at: Instant,
    client_id: String,
    resource: Option<String>,
}

#[derive(Clone)]
struct RefreshToken {
    issued_at: Instant,
    expires_at: Instant,
    client_id: String,
    resource: Option<String>,
}

#[derive(Clone)]
struct Client {
    redirect_uris: Vec<String>,
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

#[derive(Deserialize)]
struct RegistrationRequest {
    #[serde(default)]
    redirect_uris: Vec<String>,
    #[serde(default)]
    client_name: Option<String>,
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

#[derive(Deserialize)]
struct TokenForm {
    grant_type: String,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    redirect_uri: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    code_verifier: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    resource: Option<String>,
}

impl AuthState {
    pub fn new(initial_public_url: String) -> Self {
        let pairing_code = format!("{:06}", Uuid::new_v4().as_u128() % 1_000_000);
        Self {
            instance_id: Uuid::new_v4().simple().to_string(),
            public_url: Arc::new(RwLock::new(initial_public_url)),
            pairing_code,
            pairing_attempts: Default::default(),
            clients: Default::default(),
            codes: Default::default(),
            access_tokens: Default::default(),
            refresh_tokens: Default::default(),
            monitor: None,
        }
    }

    pub fn new_with_monitor(initial_public_url: String, monitor: TaskMonitor) -> Self {
        let mut state = Self::new(initial_public_url);
        state.monitor = Some(monitor);
        state
    }

    pub fn public_url(&self) -> String {
        self.public_url
            .read()
            .expect("public_url lock poisoned")
            .clone()
    }

    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    pub fn set_public_url(&self, value: String) {
        *self.public_url.write().expect("public_url lock poisoned") = value;
    }

    pub fn pairing_code(&self) -> &str {
        &self.pairing_code
    }

    pub fn authorized(&self, headers: &HeaderMap) -> bool {
        let Some(value) = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
        else {
            return false;
        };
        let Some(token) = value.strip_prefix("Bearer ") else {
            return false;
        };
        let expected_resource = format!("{}/mcp", self.public_url());
        let tokens = self.access_tokens.lock().expect("token lock poisoned");
        tokens.get(token).is_some_and(|saved| {
            !saved.client_id.is_empty()
                && saved
                    .resource
                    .as_deref()
                    .is_none_or(|resource| resource == expected_resource.as_str())
        })
    }

    pub fn unauthorized_response(&self) -> Response {
        let metadata = format!(
            "{}/.well-known/oauth-protected-resource/mcp",
            self.public_url()
        );
        (
            StatusCode::UNAUTHORIZED,
            [(
                "www-authenticate",
                format!("Bearer resource_metadata=\"{metadata}\", scope=\"mcp\""),
            )],
            Json(json!({"error": "unauthorized"})),
        )
            .into_response()
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

async fn protected_resource_metadata(State(state): State<Arc<AuthState>>) -> Json<Value> {
    let base = state.public_url();
    Json(json!({
        "resource": format!("{base}/mcp"),
        "authorization_servers": [base],
        "bearer_methods_supported": ["header"],
        "scopes_supported": ["mcp"],
    }))
}

async fn authorization_server_metadata(State(state): State<Arc<AuthState>>) -> Json<Value> {
    let base = state.public_url();
    Json(json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/authorize"),
        "token_endpoint": format!("{base}/token"),
        "registration_endpoint": format!("{base}/register"),
        "authorization_response_iss_parameter_supported": true,
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
        "scopes_supported": ["mcp"],
    }))
}

async fn register_client(
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
    let client_id = format!("wcode-{}", Uuid::new_v4());
    let mut clients = state.clients.lock().expect("client lock poisoned");
    if clients.len() >= MAX_REGISTERED_CLIENTS {
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
            "token_endpoint_auth_method": "none",
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
        })),
    )
        .into_response()
}

fn valid_redirect_uri(value: &str) -> bool {
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

async fn authorize_page(
    State(state): State<Arc<AuthState>>,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let Some(raw_query) = raw_query else {
        return Html(authorize_landing_html(&state)).into_response();
    };
    let query = match parse_authorize_query(&raw_query) {
        Ok(query) => query,
        Err(error) => {
            return (StatusCode::BAD_REQUEST, Html(authorize_error_html(error))).into_response()
        }
    };
    if let Err((status, error)) = validate_authorize_request(&state, &query) {
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

fn authorize_landing_html(state: &AuthState) -> String {
    let mcp_url = format!("{}/mcp", state.public_url());
    simple_auth_html(
        "OAuth is ready",
        "Add the MCP endpoint below to a compatible AI client and choose OAuth. The client will return here to complete the authorization flow.",
        Some(&mcp_url),
    )
}

fn authorize_error_html(error: &str) -> String {
    simple_auth_html(
        "Authorization request rejected",
        &format!("The MCP client sent an invalid OAuth request: {}. Remove the connection and add the MCP endpoint again.", html_escape(error)),
        None,
    )
}

fn simple_auth_html(title: &str, message: &str, endpoint: Option<&str>) -> String {
    let endpoint_html = endpoint
        .map(|value| format!(r#"<div class="endpoint">{}</div>"#, html_escape(value)))
        .unwrap_or_default();
    format!(
        r##"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="dark"><title>{title} · wcode</title><style>*{{box-sizing:border-box}}body{{margin:0;min-height:100vh;display:grid;place-items:center;background:#09090b;color:#f5f5f6;font:15px/1.55 ui-sans-serif,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;padding:24px}}main{{width:min(100%,520px)}}.card{{padding:28px;border:1px solid #29292f;border-radius:18px;background:#111114;box-shadow:0 28px 80px #0008}}h1{{margin:0 0 8px;font-size:24px}}p{{margin:0;color:#a1a1aa}}.endpoint{{margin-top:20px;padding:13px 15px;border:1px solid #323239;border-radius:12px;background:#09090b;color:#fafafa;font:12px ui-monospace,SFMono-Regular,Menlo,monospace;overflow:auto}}.links{{display:flex;gap:16px;flex-wrap:wrap;margin-top:18px}}a{{display:inline-block;color:#fff}}</style></head><body><main><section class="card"><h1>{title}</h1><p>{message}</p>{endpoint_html}<div class="links"><a href="{project_url}" target="_blank" rel="noreferrer">Project ↗</a><a href="{author_url}" target="_blank" rel="noreferrer">{author_handle} ↗</a></div></section></main></body></html>"##,
        title = html_escape(title),
        message = message,
        project_url = PROJECT_URL,
        author_url = AUTHOR_URL,
        author_handle = AUTHOR_HANDLE,
    )
}

fn authorize_html(query: &AuthorizeQuery, error: Option<&str>) -> String {
    let error_html = error
        .map(|message| {
            format!(
                r#"<div class="error"><span>!</span>{}</div>"#,
                html_escape(message)
            )
        })
        .unwrap_or_default();
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="color-scheme" content="dark">
<title>Authorize · wcode</title>
<style>
:root{{--bg:#09090b;--panel:#111114;--panel2:#17171b;--line:#29292f;--text:#f5f5f6;--muted:#9b9ba7;--accent:#ffffff;--danger:#ff6b6b}}
*{{box-sizing:border-box}}body{{margin:0;min-height:100vh;display:grid;place-items:center;background:radial-gradient(900px 500px at 50% -15%,#25252d 0%,var(--bg) 62%);color:var(--text);font:15px/1.55 ui-sans-serif,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;padding:24px}}
.shell{{width:min(100%,460px)}}.brand{{display:flex;align-items:center;gap:12px;margin:0 0 18px 4px}}.mark{{width:34px;height:34px;border:1px solid #3a3a42;border-radius:10px;display:grid;place-items:center;background:linear-gradient(145deg,#24242a,#101012);font:700 15px ui-monospace,SFMono-Regular,Menlo,monospace;box-shadow:0 8px 30px #0008}}.brand strong{{font-size:15px;letter-spacing:.1px}}.brand span{{display:block;color:var(--muted);font-size:12px}}
.card{{background:linear-gradient(180deg,#151519,#101013);border:1px solid var(--line);border-radius:18px;padding:28px;box-shadow:0 28px 80px #0009}}h1{{font-size:24px;line-height:1.2;margin:0 0 8px;letter-spacing:-.45px}}p{{margin:0;color:var(--muted)}}.scope{{display:flex;gap:10px;align-items:flex-start;margin:22px 0;padding:13px 14px;border:1px solid #27272d;background:#0c0c0f;border-radius:12px}}.scope svg{{flex:0 0 auto;margin-top:2px}}.scope b{{display:block;font-size:13px;margin-bottom:2px}}.scope span{{font-size:12px;color:var(--muted)}}
label{{display:block;font-size:12px;font-weight:650;color:#d7d7dc;margin:0 0 7px}}input.code{{width:100%;height:54px;border-radius:12px;border:1px solid #323239;background:#09090b;color:#fff;outline:none;padding:0 16px;font:600 22px/1 ui-monospace,SFMono-Regular,Menlo,monospace;letter-spacing:8px;text-align:center;transition:.15s border,.15s box-shadow}}input.code:focus{{border-color:#74747f;box-shadow:0 0 0 3px #ffffff12}}input.code::placeholder{{font:400 14px ui-sans-serif,-apple-system,sans-serif;letter-spacing:0;color:#666670}}
button{{width:100%;height:48px;margin-top:12px;border:0;border-radius:12px;background:var(--accent);color:#09090b;font-weight:750;font-size:14px;cursor:pointer;transition:.15s transform,.15s opacity}}button:hover{{opacity:.9}}button:active{{transform:translateY(1px)}}.error{{display:flex;align-items:center;gap:9px;margin:0 0 14px;padding:10px 12px;border:1px solid #5a2929;background:#251313;color:#ffb4b4;border-radius:10px;font-size:12px}}.error span{{display:grid;place-items:center;width:18px;height:18px;border-radius:50%;background:#ff6b6b;color:#160606;font-weight:900}}
.foot{{display:flex;justify-content:space-between;align-items:center;margin-top:16px;padding:0 4px;font-size:12px;color:#73737d}}a{{color:#b8b8c0;text-decoration:none}}a:hover{{color:#fff}}.dot{{width:7px;height:7px;border-radius:50%;background:#5ee28a;box-shadow:0 0 12px #5ee28a99;display:inline-block;margin-right:7px}}
</style>
</head>
<body><main class="shell">
<div class="brand"><div class="mark">WC</div><div><strong>wcode</strong><span>Local MCP bridge</span></div></div>
<section class="card"><h1>Authorize MCP client</h1><p>Confirm access to the local workspaces exposed by this wcode instance.</p>
<div class="scope"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#bdbdc6" stroke-width="1.7"><path d="M12 3l8 4v5c0 5-3.4 8.3-8 9-4.6-.7-8-4-8-9V7l8-4z"/><path d="M9 12l2 2 4-4"/></svg><div><b>Workspace-scoped access</b><span>Paths remain limited to the configured roots. Write and command permissions follow the CLI flags.</span></div></div>
{error_html}
<form method="post" action="/authorize">
<input type="hidden" name="client_id" value="{}"><input type="hidden" name="redirect_uri" value="{}"><input type="hidden" name="state" value="{}"><input type="hidden" name="code_challenge" value="{}">
<input type="hidden" name="resource" value="{}"><input type="hidden" name="scope" value="{}">
<label for="pairing_code">6-digit pairing code</label><input class="code" id="pairing_code" name="pairing_code" inputmode="numeric" pattern="[0-9]{{6}}" maxlength="6" autocomplete="one-time-code" placeholder="Enter code" required autofocus spellcheck="false">
<button type="submit">Authorize connection</button></form></section>
<div class="foot"><span><i class="dot"></i>OAuth 2.1 · PKCE</span><span><a href="{project_url}" target="_blank" rel="noreferrer">Project ↗</a> · <a href="{author_url}" target="_blank" rel="noreferrer">{author_handle}</a></span></div>
</main></body></html>"##,
        html_escape(&query.client_id),
        html_escape(&query.redirect_uri),
        html_escape(&query.state),
        html_escape(&query.code_challenge),
        html_escape(query.resource.as_deref().unwrap_or_default()),
        html_escape(query.scope.as_deref().unwrap_or_default()),
        project_url = PROJECT_URL,
        author_url = AUTHOR_URL,
        author_handle = AUTHOR_HANDLE,
    )
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
    Form(form): Form<AuthorizeForm>,
) -> Response {
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
    if let Err((status, error)) = validate_authorize_request(&state, &query) {
        return oauth_error(status, error);
    }
    match check_pairing_code(&state, &form.client_id, &form.pairing_code) {
        PairingCodeCheck::Accepted => {}
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
            resource: form.resource.filter(|value| !value.is_empty()),
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
        .append_pair("iss", &state.public_url());
    Redirect::to(redirect.as_str()).into_response()
}

async fn token(State(state): State<Arc<AuthState>>, Form(form): Form<TokenForm>) -> Response {
    match form.grant_type.as_str() {
        "authorization_code" => exchange_code(&state, form),
        "refresh_token" => refresh_access_token(&state, form),
        _ => oauth_error(StatusCode::BAD_REQUEST, "unsupported_grant_type"),
    }
}

fn exchange_code(state: &AuthState, form: TokenForm) -> Response {
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
        || pkce_challenge(&verifier) != saved.code_challenge
        || saved
            .resource
            .as_deref()
            .is_some_and(|resource| form.resource.as_deref() != Some(resource))
    {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant");
    }
    if let Some(monitor) = &state.monitor {
        monitor.mark_oauth_authorized();
    }
    issue_tokens(state, saved.client_id, saved.resource)
}

fn refresh_access_token(state: &AuthState, form: TokenForm) -> Response {
    let Some(refresh) = form.refresh_token else {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    let now = Instant::now();
    let saved = {
        let mut tokens = state.refresh_tokens.lock().expect("refresh lock poisoned");
        tokens.retain(|_, saved| saved.issued_at <= now && saved.expires_at > now);
        let Some(saved) = tokens.get(&refresh) else {
            return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant");
        };
        if form
            .client_id
            .as_deref()
            .is_some_and(|client_id| client_id != saved.client_id)
            || form
                .resource
                .as_deref()
                .is_some_and(|resource| Some(resource) != saved.resource.as_deref())
        {
            return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant");
        }
        tokens
            .remove(&refresh)
            .expect("validated refresh token disappeared")
    };
    issue_tokens(state, saved.client_id, saved.resource)
}

fn issue_tokens(state: &AuthState, client_id: String, resource: Option<String>) -> Response {
    let access = random_token("access");
    let refresh = random_token("refresh");
    let now = Instant::now();
    {
        let mut tokens = state.access_tokens.lock().expect("token lock poisoned");
        if tokens.len() >= MAX_ACCESS_TOKENS {
            if let Some(oldest) = tokens
                .iter()
                .min_by_key(|(_, saved)| saved.issued_at)
                .map(|(token, _)| token.clone())
            {
                tokens.remove(&oldest);
            }
        }
        tokens.insert(
            access.clone(),
            AccessToken {
                issued_at: now,
                client_id: client_id.clone(),
                resource: resource.clone(),
            },
        );
    }
    {
        let mut tokens = state.refresh_tokens.lock().expect("refresh lock poisoned");
        tokens.retain(|_, saved| saved.issued_at <= now && saved.expires_at > now);
        if tokens.len() >= MAX_REFRESH_TOKENS {
            if let Some(oldest) = tokens
                .iter()
                .min_by_key(|(_, saved)| saved.issued_at)
                .map(|(token, _)| token.clone())
            {
                tokens.remove(&oldest);
            }
        }
        tokens.insert(
            refresh.clone(),
            RefreshToken {
                issued_at: now,
                expires_at: now + REFRESH_TOKEN_TTL,
                client_id,
                resource,
            },
        );
    }
    Json(json!({
        "access_token": access,
        "token_type": "Bearer",
        "refresh_token": refresh,
        "scope": "mcp",
    }))
    .into_response()
}

fn validate_authorize_request(
    state: &AuthState,
    query: &AuthorizeQuery,
) -> Result<(), (StatusCode, &'static str)> {
    if query.response_type.as_deref().unwrap_or("code") != "code"
        || query.code_challenge_method.as_deref().unwrap_or("S256") != "S256"
        || query
            .resource
            .as_deref()
            .is_some_and(|resource| resource != format!("{}/mcp", state.public_url()))
    {
        return Err((StatusCode::BAD_REQUEST, "invalid_request"));
    }
    let clients = state.clients.lock().expect("client lock poisoned");
    let Some(client) = clients.get(&query.client_id) else {
        return Err((StatusCode::BAD_REQUEST, "invalid_client"));
    };
    if !client.redirect_uris.contains(&query.redirect_uri) {
        return Err((StatusCode::BAD_REQUEST, "invalid_redirect_uri"));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{Form, State};
    use axum::http::header::LOCATION;

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
            "Bearer resource_metadata=\"https://example.com/.well-known/oauth-protected-resource/mcp\", scope=\"mcp\""
        );
    }

    #[tokio::test]
    async fn protected_resource_metadata_matches_mcp_resource_identifier() {
        let state = Arc::new(AuthState::new("https://example.com".to_owned()));
        let Json(metadata) = protected_resource_metadata(State(state)).await;
        assert_eq!(metadata["resource"], "https://example.com/mcp");
        assert_eq!(metadata["authorization_servers"][0], "https://example.com");
        assert_eq!(metadata["scopes_supported"][0], "mcp");
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
        let verifier = "test-verifier";
        let resource = "https://example.com/mcp".to_owned();
        let response = authorize_submit(
            State(state.clone()),
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
    fn expired_authorization_code_is_rejected_and_removed() {
        let state = AuthState::new("https://example.com".to_owned());
        let verifier = "expired-verifier";
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
        );
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(state.codes.lock().unwrap().is_empty());
    }

    #[test]
    fn expired_refresh_token_is_rejected_and_removed() {
        let state = AuthState::new("https://example.com".to_owned());
        state.refresh_tokens.lock().unwrap().insert(
            "expired-refresh".to_owned(),
            RefreshToken {
                issued_at: Instant::now() - Duration::from_secs(10),
                expires_at: Instant::now() - Duration::from_secs(1),
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
                refresh_token: Some("expired-refresh".to_owned()),
                resource: None,
            },
        );
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(state.refresh_tokens.lock().unwrap().is_empty());
    }

    #[test]
    fn old_access_tokens_remain_authorized_without_expiry() {
        let state = AuthState::new("https://example.com".to_owned());
        let token = "access_expired".to_owned();
        state.access_tokens.lock().unwrap().insert(
            token.clone(),
            AccessToken {
                issued_at: Instant::now() - Duration::from_secs(10 * 365 * 24 * 60 * 60),
                client_id: "client".to_owned(),
                resource: Some("https://example.com/mcp".to_owned()),
            },
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {token}")
                .parse()
                .expect("valid bearer header"),
        );
        assert!(state.authorized(&headers));

        state.set_public_url("https://other.example.com".to_owned());
        assert!(!state.authorized(&headers));
        assert_eq!(state.access_tokens.lock().unwrap().len(), 1);
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
        );
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
        assert!(state.refresh_tokens.lock().unwrap().contains_key(&refresh));
    }
}
