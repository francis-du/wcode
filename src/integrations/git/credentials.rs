//! Opt-in, pinned GitHub App installation credentials. Never execute a helper,
//! follow an API redirect, persist a token, or replay an unconfirmed Check write.
//! Protocol: https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-an-installation-access-token-for-a-github-app
use super::*;
use anyhow::ensure;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use ring::{rand::SystemRandom, signature::RsaKeyPair};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, Semaphore};
use tokio::time::Instant;

const KEY_BYTES: u64 = 32 * 1024;
const REFRESH_MARGIN: u64 = 120;
const REFRESH_TIMEOUT: Duration = Duration::from_secs(35);

pub(super) enum Credentials {
    Static(HeaderValue),
    App(Box<AppCredentials>),
}

pub(super) struct AppCredentials {
    installation_id: u64,
    key_source: AppKeySource,
    signing_slots: Arc<Semaphore>,
    state: Mutex<RefreshState>,
}

#[derive(Clone)]
enum AppKeySource {
    File(PathBuf),
    Memory(Arc<[u8]>),
}

struct RefreshState {
    cached: Option<Cached>,
    retry_after: Instant,
    failures: u32,
}
struct Cached {
    authorization: HeaderValue,
    observed_at: u64,
    expires_at: u64,
    refresh_at: Instant,
}

impl Cached {
    fn usable(&self, wall: u64, monotonic: Instant) -> bool {
        wall >= self.observed_at
            && wall.saturating_add(REFRESH_MARGIN) < self.expires_at
            && monotonic < self.refresh_at
    }
}

impl Credentials {
    pub(super) fn fixed(token: &str) -> Result<Self> {
        Ok(Self::Static(bearer(token)?))
    }

    pub(super) fn app(installation_id: u64, key_path: &Path) -> Result<Self> {
        ensure!(cfg!(unix), "GitHub App-key renewal from a file requires Unix private-file validation; use an in-memory App key or externally managed static token on this platform");
        Self::app_source(installation_id, AppKeySource::File(key_path.to_path_buf()))
    }

    pub(super) fn app_pem(installation_id: u64, private_key: &[u8]) -> Result<Self> {
        ensure!(
            !private_key.is_empty() && private_key.len() <= KEY_BYTES as usize,
            "GitHub App signing key is invalid or unsafe"
        );
        Self::app_source(
            installation_id,
            AppKeySource::Memory(Arc::<[u8]>::from(private_key)),
        )
    }

    fn app_source(installation_id: u64, key_source: AppKeySource) -> Result<Self> {
        ensure!(
            installation_id > 0,
            "GitHub installation identity must be positive"
        );
        match &key_source {
            AppKeySource::File(path) => {
                read_key(path)?;
            }
            AppKeySource::Memory(bytes) => {
                parse_key(bytes)?;
            }
        }
        Ok(Self::App(Box::new(AppCredentials {
            installation_id,
            key_source,
            signing_slots: Arc::new(Semaphore::new(1)),
            state: Mutex::new(RefreshState {
                cached: None,
                retry_after: Instant::now(),
                failures: 0,
            }),
        })))
    }

    pub(super) fn renewable(&self) -> bool {
        matches!(self, Self::App(_))
    }

    pub(super) async fn authorization(
        &self,
        client: &Client,
        api: &Url,
        config: &GitHubConfig,
    ) -> Result<HeaderValue> {
        match self {
            Self::Static(value) => Ok(value.clone()),
            Self::App(app) => {
                tokio::time::timeout(REFRESH_TIMEOUT, app.authorization(client, api, config))
                    .await
                    .map_err(|_| anyhow!("GitHub credential refresh deadline exceeded"))?
            }
        }
    }

    pub(super) async fn reject(&self, used: &HeaderValue) {
        if let Self::App(app) = self {
            let mut state = app.state.lock().await;
            // A late 401 for an older token must not discard its replacement.
            if state
                .cached
                .as_ref()
                .is_some_and(|c| c.authorization == used)
            {
                state.cached = None;
            }
        }
    }
}

impl AppCredentials {
    async fn authorization(
        &self,
        client: &Client,
        api: &Url,
        config: &GitHubConfig,
    ) -> Result<HeaderValue> {
        // One bounded exchange per provider. Waiting callers share its result;
        // cancellation releases the mutex but retains the pre-set retry deadline.
        let mut state = self.state.lock().await;
        let wall = unix_seconds()?;
        if let Some(cached) = state
            .cached
            .as_ref()
            .filter(|c| c.usable(wall, Instant::now()))
        {
            return Ok(cached.authorization.clone());
        }
        state.cached = None;
        ensure!(
            Instant::now() >= state.retry_after,
            "GitHub credential refresh is backing off"
        );
        state.failures = state.failures.saturating_add(1);
        state.retry_after = Instant::now()
            + Duration::from_secs((5u64 << state.failures.saturating_sub(1).min(6)).min(300));
        match self.exchange(client, api, config).await {
            Ok(cached) => {
                let result = cached.authorization.clone();
                state.cached = Some(cached);
                state.failures = 0;
                state.retry_after = Instant::now();
                Ok(result)
            }
            Err(_) => {
                // Delay from completion as well: a slow failure is not permission
                // for queued callers to immediately start another exchange.
                state.retry_after = Instant::now()
                    + Duration::from_secs(
                        (5u64 << state.failures.saturating_sub(1).min(6)).min(300),
                    );
                Err(anyhow!(
                    "GitHub installation credential renewal unavailable; no cached fallback"
                ))
            }
        }
    }

    async fn exchange(&self, client: &Client, api: &Url, config: &GitHubConfig) -> Result<Cached> {
        let key_source = self.key_source.clone();
        let app_id = config.app_id;
        let jwt = sign_blocking(self.signing_slots.clone(), move || {
            let key = match key_source {
                AppKeySource::File(path) => read_key(&path)?,
                AppKeySource::Memory(bytes) => parse_key(&bytes)?,
            };
            sign_jwt(&key, app_id, unix_seconds()?)
        })
        .await?;
        let jwt = bearer(&jwt)?;
        let mut installation_url = api.clone();
        installation_url
            .path_segments_mut()
            .map_err(|_| anyhow!("invalid API root"))?
            .pop_if_empty()
            .extend([
                "repos",
                config.repository.namespace(),
                config.repository.name(),
                "installation",
            ]);
        let installation: InstallationBinding = bounded_json(
            client
                .get(installation_url)
                .header(AUTHORIZATION, jwt.clone())
                .send()
                .await?,
            StatusCode::OK,
        )
        .await?;
        ensure!(
            installation.id == self.installation_id
                && installation.app_id == config.app_id
                && installation.suspended_at == Value::Null,
            "GitHub App installation binding unavailable"
        );
        let mut token_url = api.clone();
        token_url
            .path_segments_mut()
            .map_err(|_| anyhow!("invalid API root"))?
            .pop_if_empty()
            .extend([
                "app",
                "installations",
                &self.installation_id.to_string(),
                "access_tokens",
            ]);
        let issued: IssuedToken = bounded_json(
            client
                .post(token_url)
                .header(AUTHORIZATION, jwt)
                .json(
                    &json!({"repository_ids":[config.repository_id], "permissions":permissions()}),
                )
                .send()
                .await?,
            StatusCode::CREATED,
        )
        .await?;
        let now = unix_seconds()?;
        let expires_at = parse_expiry(&issued.expires_at)?;
        let lifetime = expires_at
            .checked_sub(now)
            .ok_or_else(|| anyhow!("expired credential"))?;
        ensure!(
            lifetime > REFRESH_MARGIN
                && lifetime <= 3660
                && issued.permissions == permissions()
                && issued.repositories.len() == 1
                && issued.repositories[0].id == config.repository_id
                && issued.repositories[0].full_name.to_ascii_lowercase() == config.repository.key(),
            "GitHub issued credential scope or lifetime differs from the requested binding"
        );
        Ok(Cached {
            authorization: bearer(&issued.token)?,
            observed_at: now,
            expires_at,
            refresh_at: Instant::now() + Duration::from_secs(lifetime - REFRESH_MARGIN),
        })
    }
}

// Cancellation can detach a blocking file read/signature from its caller. The
// actual worker, not that caller, must retain capacity until it truly finishes.
async fn sign_blocking<T, F>(slots: Arc<Semaphore>, work: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    let slot = slots
        .acquire_owned()
        .await
        .map_err(|_| anyhow!("GitHub App signing capacity unavailable"))?;
    tokio::task::spawn_blocking(move || {
        let _slot = slot;
        work()
    })
    .await
    .map_err(|_| anyhow!("GitHub App signing unavailable"))?
}

#[derive(Deserialize)]
struct InstallationBinding {
    id: u64,
    app_id: u64,
    suspended_at: Value,
}
#[derive(Deserialize)]
struct IssuedToken {
    token: String,
    expires_at: String,
    permissions: Permissions,
    repositories: Vec<Repository>,
}
#[derive(Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Permissions {
    checks: String,
    pull_requests: String,
    contents: String,
    administration: String,
    metadata: String,
}
fn permissions() -> Permissions {
    Permissions {
        checks: "write".into(),
        pull_requests: "read".into(),
        contents: "read".into(),
        administration: "read".into(),
        metadata: "read".into(),
    }
}

async fn bounded_json<T: serde::de::DeserializeOwned>(
    mut response: reqwest::Response,
    expected: StatusCode,
) -> Result<T> {
    ensure!(
        response.status() == expected,
        "GitHub credential endpoint unavailable"
    );
    ensure!(
        response
            .content_length()
            .is_none_or(|n| n <= MAX_RESPONSE_BYTES as u64),
        "GitHub credential response exceeds bound"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len().saturating_add(chunk.len()) <= MAX_RESPONSE_BYTES,
            "GitHub credential response exceeds bound"
        );
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| anyhow!("GitHub credential metadata invalid"))
}

fn bearer(token: &str) -> Result<HeaderValue> {
    ensure!(
        !token.is_empty() && token.len() <= 8192 && token.bytes().all(|b| b.is_ascii_graphic()),
        "invalid GitHub publisher credential"
    );
    let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|_| anyhow!("invalid GitHub publisher credential"))?;
    value.set_sensitive(true);
    Ok(value)
}

fn read_key(path: &Path) -> Result<RsaKeyPair> {
    let result = (|| -> Result<RsaKeyPair> {
        let parent = path.parent().ok_or_else(|| anyhow!("missing key parent"))?;
        inbox::validate_root(parent, false)?;
        inbox::private_metadata(parent, true)?;
        let file = inbox::open_regular(path, false, KEY_BYTES)?;
        let mut bytes = Vec::new();
        file.take(KEY_BYTES + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() as u64 <= KEY_BYTES, "key exceeds bound");
        parse_key(&bytes)
    })();
    result.map_err(|_| anyhow!("GitHub App signing key is invalid or unsafe"))
}
fn parse_key(bytes: &[u8]) -> Result<RsaKeyPair> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| anyhow!("invalid PEM"))?
        .trim();
    for (begin, end, pkcs8) in [
        (
            "-----BEGIN PRIVATE KEY-----",
            "-----END PRIVATE KEY-----",
            true,
        ),
        (
            "-----BEGIN RSA PRIVATE KEY-----",
            "-----END RSA PRIVATE KEY-----",
            false,
        ),
    ] {
        if let Some(body) = text.strip_prefix(begin).and_then(|s| s.strip_suffix(end)) {
            let encoded = body
                .bytes()
                .filter(|b| !b.is_ascii_whitespace())
                .collect::<Vec<_>>();
            let der = STANDARD
                .decode(encoded)
                .map_err(|_| anyhow!("invalid PEM"))?;
            return if pkcs8 {
                RsaKeyPair::from_pkcs8(&der)
            } else {
                RsaKeyPair::from_der(&der)
            }
            .map_err(|_| anyhow!("invalid RSA key"));
        }
    }
    bail!("unsupported signing key encoding")
}
fn sign_jwt(key: &RsaKeyPair, app_id: u64, now: u64) -> Result<String> {
    ensure!(app_id > 0 && now >= 60, "invalid App identity or clock");
    let exp = now
        .checked_add(540)
        .ok_or_else(|| anyhow!("clock overflow"))?;
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({
        "iat":now-60, "exp":exp, "iss":app_id.to_string(),
    }))?);
    let signed = format!("{header}.{payload}");
    let mut signature = vec![0; key.public().modulus_len()];
    key.sign(
        &ring::signature::RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        signed.as_bytes(),
        &mut signature,
    )
    .map_err(|_| anyhow!("RSA signing unavailable"))?;
    Ok(format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature)))
}
fn unix_seconds() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_| anyhow!("GitHub credential clock unavailable"))
}

// GitHub's canonical UTC expiry, not a permissive general-purpose date parser.
fn parse_expiry(text: &str) -> Result<u64> {
    let bytes = text.as_bytes();
    ensure!(
        bytes.len() == 20
            && bytes[4] == b'-'
            && bytes[7] == b'-'
            && bytes[10] == b'T'
            && bytes[13] == b':'
            && bytes[16] == b':'
            && bytes[19] == b'Z',
        "invalid GitHub credential expiry"
    );
    let number = |start: usize, end: usize| -> Result<u64> {
        ensure!(
            bytes[start..end].iter().all(u8::is_ascii_digit),
            "invalid expiry digits"
        );
        Ok(bytes[start..end]
            .iter()
            .fold(0u64, |n, b| n * 10 + (b - b'0') as u64))
    };
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    ensure!(
        year >= 1970 && (1..=12).contains(&month) && hour < 24 && minute < 60 && second < 60,
        "invalid expiry calendar"
    );
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    ensure!(
        (1..=months[month as usize - 1]).contains(&day),
        "invalid expiry day"
    );
    let prior = year - 1;
    let days = 365 * (year - 1970) + prior / 4 - 1969 / 4 - (prior / 100 - 1969 / 100)
        + prior / 400
        - 1969 / 400
        + months[..month as usize - 1].iter().sum::<u64>()
        + day
        - 1;
    Ok(days * 86400 + hour * 3600 + minute * 60 + second)
}

#[cfg(all(test, not(unix)))]
#[path = "../../../tests/unit/integrations/git/credentials_portability.rs"]
mod portability_tests;
#[cfg(all(test, unix))]
#[path = "../../../tests/unit/integrations/git/credentials.rs"]
mod tests;
