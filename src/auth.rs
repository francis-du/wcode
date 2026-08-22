use axum::extract::{Form, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::Rng;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use url::Url;
use uuid::Uuid;

#[derive(Clone)]
pub struct AuthState {
    public_url: Arc<RwLock<String>>,
    pairing_code: String,
    clients: Arc<Mutex<HashMap<String, Client>>>,
    codes: Arc<Mutex<HashMap<String, AuthorizationCode>>>,
    access_tokens: Arc<Mutex<HashSet<String>>>,
    refresh_tokens: Arc<Mutex<HashSet<String>>>,
}

#[derive(Clone)]
struct Client {
    redirect_uris: Vec<String>,
}

struct AuthorizationCode {
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
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
}

#[derive(Deserialize)]
struct AuthorizeForm {
    client_id: String,
    redirect_uri: String,
    state: String,
    code_challenge: String,
    pairing_code: String,
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
}

impl AuthState {
    pub fn new(initial_public_url: String) -> Self {
        let pairing_code = format!("{:06}", rand::thread_rng().gen_range(0..1_000_000));
        Self {
            public_url: Arc::new(RwLock::new(initial_public_url)),
            pairing_code,
            clients: Default::default(),
            codes: Default::default(),
            access_tokens: Default::default(),
            refresh_tokens: Default::default(),
        }
    }

    pub fn public_url(&self) -> String {
        self.public_url
            .read()
            .expect("public_url lock poisoned")
            .clone()
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
        self.access_tokens
            .lock()
            .expect("token lock poisoned")
            .contains(token)
    }

    pub fn unauthorized_response(&self) -> Response {
        let metadata = format!("{}/.well-known/oauth-protected-resource", self.public_url());
        (
            StatusCode::UNAUTHORIZED,
            [(
                "www-authenticate",
                format!("Bearer resource_metadata=\"{metadata}\""),
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
            "/.well-known/oauth-authorization-server",
            get(authorization_server_metadata),
        )
        .route("/register", post(register_client))
        .route("/authorize", get(authorize_page).post(authorize_submit))
        .route("/token", post(token))
        .with_state(state)
}

async fn protected_resource_metadata(State(state): State<Arc<AuthState>>) -> Json<Value> {
    let base = state.public_url();
    Json(json!({
        "resource": format!("{base}/mcp"),
        "authorization_servers": [base],
        "bearer_methods_supported": ["header"],
    }))
}

async fn authorization_server_metadata(State(state): State<Arc<AuthState>>) -> Json<Value> {
    let base = state.public_url();
    Json(json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/authorize"),
        "token_endpoint": format!("{base}/token"),
        "registration_endpoint": format!("{base}/register"),
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
        || request
            .redirect_uris
            .iter()
            .any(|uri| Url::parse(uri).is_err())
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_redirect_uri"})),
        )
            .into_response();
    }
    let client_id = format!("wcode-{}", Uuid::new_v4());
    state.clients.lock().expect("client lock poisoned").insert(
        client_id.clone(),
        Client {
            redirect_uris: request.redirect_uris.clone(),
        },
    );
    let issued_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    (
        StatusCode::CREATED,
        Json(json!({
            "client_id": client_id,
            "client_id_issued_at": issued_at,
            "client_name": request.client_name.unwrap_or_else(|| "ChatGPT".into()),
            "redirect_uris": request.redirect_uris,
            "token_endpoint_auth_method": "none",
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
        })),
    )
        .into_response()
}

async fn authorize_page(
    State(state): State<Arc<AuthState>>,
    Query(query): Query<AuthorizeQuery>,
) -> Response {
    if let Err((status, error)) = validate_authorize_request(&state, &query) {
        return oauth_error(status, error);
    }
    Html(authorize_html(&query, None)).into_response()
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
<section class="card"><h1>Authorize ChatGPT</h1><p>Confirm access to the local workspaces exposed by this wcode instance.</p>
<div class="scope"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#bdbdc6" stroke-width="1.7"><path d="M12 3l8 4v5c0 5-3.4 8.3-8 9-4.6-.7-8-4-8-9V7l8-4z"/><path d="M9 12l2 2 4-4"/></svg><div><b>Workspace-scoped access</b><span>Paths remain limited to the configured roots. Write and command permissions follow the CLI flags.</span></div></div>
{error_html}
<form method="post" action="/authorize">
<input type="hidden" name="client_id" value="{}"><input type="hidden" name="redirect_uri" value="{}"><input type="hidden" name="state" value="{}"><input type="hidden" name="code_challenge" value="{}">
<label for="pairing_code">6-digit pairing code</label><input class="code" id="pairing_code" name="pairing_code" inputmode="numeric" pattern="[0-9]{{6}}" maxlength="6" autocomplete="one-time-code" placeholder="Enter code" required autofocus spellcheck="false">
<button type="submit">Authorize connection</button></form></section>
<div class="foot"><span><i class="dot"></i>OAuth 2.1 · PKCE</span><a href="https://github.com/francis-du/wcode" target="_blank" rel="noreferrer">github.com/francis-du/wcode ↗</a></div>
</main></body></html>"##,
        html_escape(&query.client_id),
        html_escape(&query.redirect_uri),
        html_escape(&query.state),
        html_escape(&query.code_challenge),
    )
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
    };
    if let Err((status, error)) = validate_authorize_request(&state, &query) {
        return oauth_error(status, error);
    }
    if form.pairing_code != state.pairing_code {
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
    let code = random_token("code");
    state.codes.lock().expect("code lock poisoned").insert(
        code.clone(),
        AuthorizationCode {
            client_id: form.client_id,
            redirect_uri: form.redirect_uri.clone(),
            code_challenge: form.code_challenge,
            expires_at: Instant::now() + Duration::from_secs(300),
        },
    );
    let mut redirect = match Url::parse(&form.redirect_uri) {
        Ok(url) => url,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    redirect
        .query_pairs_mut()
        .append_pair("code", &code)
        .append_pair("state", &form.state);
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
    {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant");
    }
    issue_tokens(state)
}

fn refresh_access_token(state: &AuthState, form: TokenForm) -> Response {
    let Some(refresh) = form.refresh_token else {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    if !state
        .refresh_tokens
        .lock()
        .expect("refresh lock poisoned")
        .contains(&refresh)
    {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant");
    }
    issue_tokens(state)
}

fn issue_tokens(state: &AuthState) -> Response {
    let access = random_token("access");
    let refresh = random_token("refresh");
    state
        .access_tokens
        .lock()
        .expect("token lock poisoned")
        .insert(access.clone());
    state
        .refresh_tokens
        .lock()
        .expect("refresh lock poisoned")
        .insert(refresh.clone());
    Json(json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": 3600,
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

    #[test]
    fn pkce_matches_rfc_example() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            pkce_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
}
