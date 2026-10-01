//! GitHub Check Runs transport for a separately deployed trusted publisher.
//! No PR checkout, command execution, branch-protection mutation or JSON CAR import.
//! Protocol: https://docs.github.com/en/rest/checks/runs
use super::*;
use anyhow::{anyhow, bail, Result};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION};
use reqwest::{Client, Method, StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

#[path = "credentials.rs"]
mod credentials;
use credentials::Credentials;
#[path = "inbox.rs"]
mod inbox;
pub use inbox::{GitHubInbox, InboxArchiveCheckpoint, InboxEnqueueResult};
#[path = "publish_lock.rs"]
mod publish_lock;
use publish_lock::{PublicationBusy, PublicationGuard};

const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const API_VERSION: &str = "2022-11-28";

#[path = "preflight.rs"]
mod preflight;
pub use preflight::{GitHubPreflight, PreflightFinding};
#[path = "watch.rs"]
mod watch;
pub use watch::GitHubCandidateWatch;
#[path = "webhook.rs"]
mod webhook;
pub use webhook::{VerifiedPullRequestDelivery, MAX_WEBHOOK_BYTES};

#[derive(Clone, Debug)]
pub struct GitHubConfig {
    repository: ProviderRepository,
    repository_id: u64,
    app_id: u64,
    check_name: String,
}

impl GitHubConfig {
    pub fn new(
        repository: ProviderRepository,
        repository_id: u64,
        app_id: u64,
        check_name: &str,
    ) -> Result<Self> {
        if repository_id == 0
            || app_id == 0
            || check_name.trim().is_empty()
            || check_name.len() > 100
            || check_name.chars().any(char::is_control)
        {
            bail!(
                "GitHub publisher requires repository ID, expected App ID and bounded check name"
            );
        }
        Ok(Self {
            repository,
            repository_id,
            app_id,
            check_name: check_name.into(),
        })
    }
}

/// Credentials remain in this client; never give the publisher token to workers.
pub struct GitHubProvider {
    client: Client,
    api: Url,
    config: GitHubConfig,
    credentials: Credentials,
}

impl std::fmt::Debug for GitHubProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitHubProvider")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Serialize)]
pub struct GitHubGateReceipt {
    target: ProviderTarget,
    verdict: GateVerdict,
    record_digest: Option<String>,
    check: CheckObservation,
}
impl GitHubGateReceipt {
    pub fn target(&self) -> &ProviderTarget {
        &self.target
    }
    pub fn verdict(&self) -> GateVerdict {
        self.verdict
    }
    pub fn check(&self) -> &CheckObservation {
        &self.check
    }
    pub fn record_digest(&self) -> Option<&str> {
        self.record_digest.as_deref()
    }
}

impl GitHubProvider {
    pub fn new(config: GitHubConfig, installation_token: &str) -> Result<Self> {
        Self::client(
            config,
            installation_token,
            Url::parse("https://api.github.com/")?,
            true,
        )
    }
    fn client(config: GitHubConfig, token: &str, api: Url, https_only: bool) -> Result<Self> {
        Ok(Self {
            credentials: Credentials::fixed(token)?,
            client: Self::http_client(https_only)?,
            api,
            config,
        })
    }

    /// Opt in to renewal using a dedicated App key supplied by the trusted host.
    /// The key is more privileged than an installation token: isolate it from
    /// workers, use a dedicated App, and do not give this mode to an untrusted host.
    /// Construction validates local key material; exchange is lazy on API use.
    pub fn with_app_key(
        config: GitHubConfig,
        installation_id: u64,
        key_path: impl AsRef<std::path::Path>,
    ) -> Result<Self> {
        Self::app_client(
            config,
            installation_id,
            key_path.as_ref(),
            Url::parse("https://api.github.com/")?,
            true,
        )
    }

    /// Cross-platform App-key renewal for trusted launchers that already hold
    /// the PEM in protected process memory. The key is never written by wcode.
    pub fn with_app_key_pem(
        config: GitHubConfig,
        installation_id: u64,
        private_key: impl AsRef<[u8]>,
    ) -> Result<Self> {
        let api = Url::parse("https://api.github.com/")?;
        Ok(Self {
            credentials: Credentials::app_pem(installation_id, private_key.as_ref())?,
            client: Self::http_client(true)?,
            api,
            config,
        })
    }

    fn app_client(
        config: GitHubConfig,
        installation_id: u64,
        key_path: &std::path::Path,
        api: Url,
        https_only: bool,
    ) -> Result<Self> {
        Ok(Self {
            credentials: Credentials::app(installation_id, key_path)?,
            client: Self::http_client(https_only)?,
            api,
            config,
        })
    }

    fn http_client(https_only: bool) -> Result<Client> {
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            "X-GitHub-Api-Version",
            HeaderValue::from_static(API_VERSION),
        );
        let client = Client::builder()
            .default_headers(headers)
            .user_agent("wcode-native-gate")
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .https_only(https_only)
            .build()?;
        Ok(client)
    }

    async fn send_authenticated(
        &self,
        method: Method,
        url: Url,
        body: Option<Value>,
    ) -> Result<reqwest::Response> {
        let authorization = self
            .credentials
            .authorization(&self.client, &self.api, &self.config)
            .await?;
        let mut request = self
            .client
            .request(method, url)
            .header(AUTHORIZATION, authorization.clone());
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .map_err(|_| anyhow!("GitHub transport unavailable"))?;
        if response.status() == StatusCode::UNAUTHORIZED {
            self.credentials.reject(&authorization).await;
        }
        // Never replay this request. Especially for writes, its remote effect
        // may be ambiguous. A later guarded operation may obtain a new token.
        Ok(response)
    }

    fn endpoint(&self, suffix: &[&str]) -> Result<Url> {
        let mut url = self.api.clone();
        url.path_segments_mut()
            .map_err(|_| anyhow!("invalid GitHub API root"))?
            .pop_if_empty()
            .extend([
                "repos",
                self.config.repository.namespace(),
                self.config.repository.name(),
            ])
            .extend(suffix.iter().copied());
        Ok(url)
    }
    async fn request(
        &self,
        method: Method,
        suffix: &[&str],
        body: Option<Value>,
        expected: StatusCode,
    ) -> Result<Value> {
        let mut response = self
            .send_authenticated(method, self.endpoint(suffix)?, body)
            .await?;
        if response.status() != expected {
            bail!(
                "GitHub request failed (HTTP {})",
                response.status().as_u16()
            );
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            bail!("GitHub response exceeds metadata bound");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow!("GitHub response unavailable"))?
        {
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                bail!("GitHub response exceeds metadata bound");
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| anyhow!("GitHub returned invalid metadata"))
    }
    pub async fn read_target(&self, change: u64) -> Result<ProviderTarget> {
        if change == 0 {
            bail!("invalid pull request identity");
        }
        let number = change.to_string();
        let value = self
            .request(Method::GET, &["pulls", &number], None, StatusCode::OK)
            .await?;
        let pull: Pull = serde_json::from_value(value)
            .map_err(|_| anyhow!("GitHub pull metadata incomplete"))?;
        self.target_from_pull(change, &pull)
    }
    fn target_from_pull(&self, change: u64, pull: &Pull) -> Result<ProviderTarget> {
        if pull.number != change
            || pull.state != "open"
            || pull.draft
            || pull.merged
            || pull.base.repo.id != self.config.repository_id
            || pull.base.repo.full_name.to_ascii_lowercase() != self.config.repository.key()
            || !pull.head.repo.as_ref().is_some_and(|repo| repo.id > 0)
        {
            bail!("GitHub pull target is unavailable or outside publisher binding");
        }
        let target = ProviderTarget {
            repository: self.config.repository.clone(),
            change,
            base_sha: pull.base.sha.to_ascii_lowercase(),
            head_sha: pull.head.sha.to_ascii_lowercase(),
        };
        target.validate()?;
        Ok(target)
    }
    pub async fn read_check(&self, id: u64) -> Result<CheckObservation> {
        if id == 0 {
            bail!("invalid check identity");
        }
        let id_text = id.to_string();
        let value = self
            .request(Method::GET, &["check-runs", &id_text], None, StatusCode::OK)
            .await?;
        let check = check_observation(value)?;
        if check.id != id {
            bail!("GitHub returned another check identity");
        }
        Ok(check)
    }
    fn check_body(&self, publication: &NativePublication, create: bool) -> Value {
        let ready = publication.verdict() == GateVerdict::Ready;
        let mut value = json!({
            "name": self.config.check_name, "status": "completed",
            "conclusion": if ready { "success" } else { "failure" },
            "external_id": external_id(publication),
            "output": { "title": "wcode change acceptance",
                "summary": if ready { "Current native acceptance is ready for this exact candidate." }
                    else { "Native acceptance is unavailable, blocked, incomplete or stale. Merge is not approved." } }
        });
        if create {
            value["head_sha"] = json!(publication.target().head_sha);
        }
        value
    }
    fn validate_check(
        &self,
        check: &CheckObservation,
        publication: &NativePublication,
    ) -> Result<()> {
        let expected = if publication.verdict() == GateVerdict::Ready {
            "success"
        } else {
            "failure"
        };
        if publication.target().repository != self.config.repository
            || check.id == 0
            || check.head_sha != publication.target().head_sha
            || check.name != self.config.check_name
            || check.source_id != format!("github-app:{}", self.config.app_id)
            || check.external_id != external_id(publication)
            || check.status != "completed"
            || check.conclusion.as_deref() != Some(expected)
        {
            bail!("GitHub check does not match native gate, exact SHA and expected App");
        }
        Ok(())
    }
    async fn read_owned_check(&self, target: &ProviderTarget, id: u64) -> Result<CheckObservation> {
        let check = self.read_check(id).await?;
        if target.repository != self.config.repository
            || check.head_sha != target.head_sha
            || check.name != self.config.check_name
            || check.source_id != format!("github-app:{}", self.config.app_id)
        {
            bail!("watched Check no longer has the expected repository, SHA, name and App");
        }
        Ok(check)
    }

    async fn create_check(
        &self,
        publication: &NativePublication,
        guard: &PublicationGuard,
    ) -> Result<CheckObservation> {
        guard.check(self, publication.target())?;
        publication.target().validate()?;
        if publication.target().repository != self.config.repository {
            bail!("cross-repository publication refused");
        }
        let value = self
            .request(
                Method::POST,
                &["check-runs"],
                Some(self.check_body(publication, true)),
                StatusCode::CREATED,
            )
            .await?;
        let check = check_observation(value)?;
        self.validate_check(&check, publication)?;
        Ok(check)
    }
    async fn update_check(
        &self,
        id: u64,
        publication: &NativePublication,
        guard: &PublicationGuard,
    ) -> Result<CheckObservation> {
        guard.check(self, publication.target())?;
        let number = id.to_string();
        let value = self
            .request(
                Method::PATCH,
                &["check-runs", &number],
                Some(self.check_body(publication, false)),
                StatusCode::OK,
            )
            .await?;
        let check = check_observation(value)?;
        if check.id != id {
            bail!("GitHub returned another check identity");
        }
        self.validate_check(&check, publication)?;
        Ok(check)
    }
    /// A denial needs no fabricated CAR. This only emits failure, never skipped
    /// or neutral. HTTP failure is unavailable, never permission to merge.
    pub async fn publish_unavailable(&self, change: u64) -> Result<GitHubGateReceipt> {
        let target = self.read_target(change).await?;
        let publication = NativePublication::unavailable(target.clone())?;
        let guard = PublicationGuard::acquire(self, &target)?;
        let check = self.create_check(&publication, &guard).await?;
        Ok(receipt(publication, check))
    }
    /// Public publication always captures fresh native state itself. Deploy the
    /// fixed publisher outside the worker identity; this never executes PR code.
    /// Sequential observations do not form an atomic GitHub merge lock.
    pub async fn publish_local(
        &self,
        change: u64,
        root: impl AsRef<std::path::Path>,
        workspace_id: &str,
    ) -> Result<GitHubGateReceipt> {
        let root = root.as_ref().to_path_buf();
        let workspace_id = workspace_id.to_owned();
        self.publish_current(change, move |target| {
            let root = root.clone();
            let workspace_id = workspace_id.clone();
            async move {
                crate::verification::acceptance_native::capture_local(
                    root,
                    &workspace_id,
                    &target.base_sha,
                    &target.head_sha,
                )
                .await
            }
        })
        .await
    }

    // Injectable only inside the crate for protocol tests, never worker JSON.
    pub(crate) async fn publish_current<F, Fut>(
        &self,
        change: u64,
        capture: F,
    ) -> Result<GitHubGateReceipt>
    where
        F: FnMut(ProviderTarget) -> Fut,
        Fut: Future<Output = Result<NativeAcceptanceRecord>>,
    {
        self.publish_bound(change, None, capture).await
    }

    async fn publish_bound<F, Fut>(
        &self,
        change: u64,
        expected: Option<ProviderTarget>,
        capture: F,
    ) -> Result<GitHubGateReceipt>
    where
        F: FnMut(ProviderTarget) -> Fut,
        Fut: Future<Output = Result<NativeAcceptanceRecord>>,
    {
        self.publish_bound_with_guard(change, expected, capture, None)
            .await
    }

    async fn publish_bound_with_guard<F, Fut>(
        &self,
        change: u64,
        expected: Option<ProviderTarget>,
        capture: F,
        mut guard: Option<Arc<PublicationGuard>>,
    ) -> Result<GitHubGateReceipt>
    where
        F: FnMut(ProviderTarget) -> Fut,
        Fut: Future<Output = Result<NativeAcceptanceRecord>>,
    {
        self.publish_bound_with_check(change, expected, capture, &mut None, &mut guard)
            .await
    }

    // The retained ID is set from our red creation acknowledgement before any
    // possible green write. It is never supplied by a webhook or imported JSON.
    async fn publish_bound_with_check<F, Fut>(
        &self,
        change: u64,
        expected: Option<ProviderTarget>,
        mut capture: F,
        owned: &mut Option<CheckObservation>,
        guard: &mut Option<Arc<PublicationGuard>>,
    ) -> Result<GitHubGateReceipt>
    where
        F: FnMut(ProviderTarget) -> Fut,
        Fut: Future<Output = Result<NativeAcceptanceRecord>>,
    {
        let target = self.read_target(change).await?;
        if expected
            .as_ref()
            .is_some_and(|expected| expected != &target)
        {
            bail!("GitHub candidate changed before any Check mutation");
        }
        if guard.is_none() {
            *guard = Some(Arc::new(PublicationGuard::acquire(self, &target)?));
        }
        let guard = guard.as_deref().expect("publication guard acquired");
        guard.check(self, &target)?;
        // Begin red. Missing capture, outage or guard failure cannot publish green.
        let blocked = NativePublication::unavailable(target.clone())?;
        let initial = if let Some(known) = owned.as_ref() {
            let current = self.read_owned_check(&target, known.id).await?;
            let native = capture(target.clone())
                .await
                .map_err(|_| anyhow!("native acceptance refresh unavailable"))?;
            let publication = NativePublication::from_native(&native, target.clone())?;
            // Avoid both new Check creation and red/green flicker for unchanged
            // facts. A previous receipt alone never reaches this branch.
            if self.validate_check(&current, &publication).is_ok() {
                let repeated = capture(target.clone())
                    .await
                    .map_err(|_| anyhow!("native acceptance recapture unavailable"))?;
                let remote = self.read_target(change).await?;
                let observed = self.read_owned_check(&target, known.id).await?;
                if remote != target
                    || NativePublication::from_native(&repeated, target.clone())? != publication
                    || self.validate_check(&observed, &publication).is_err()
                {
                    bail!("candidate, native acceptance or Check changed during refresh");
                }
                return Ok(receipt(publication, observed));
            }
            self.update_check(known.id, &blocked, guard).await?
        } else {
            let created = self.create_check(&blocked, guard).await?;
            *owned = Some(created.clone());
            created
        };
        let native = capture(target.clone())
            .await
            .map_err(|_| anyhow!("native acceptance capture unavailable"))?;
        let publication = NativePublication::from_native(&native, target.clone())?;
        if publication.verdict() == GateVerdict::Blocked {
            let check = self.update_check(initial.id, &publication, guard).await?;
            return Ok(receipt(publication, check));
        }
        let before = capture(target.clone())
            .await
            .map_err(|_| anyhow!("native acceptance recapture unavailable"))?;
        if self.read_target(change).await? != target
            || NativePublication::from_native(&before, target.clone())? != publication
        {
            bail!("candidate or native acceptance changed before publication");
        }
        let published = match self.update_check(initial.id, &publication, guard).await {
            Ok(check) => check,
            Err(_) => {
                // An unconfirmed response may still have applied remotely.
                let _ = self.update_check(initial.id, &blocked, guard).await;
                bail!("GitHub success publication could not be confirmed; gate unavailable");
            }
        };
        // If a later guard fails, attempt to revoke green. An outage may prevent
        // that update; report unavailable, never claim the remote check expired.
        let after = capture(target.clone()).await;
        let remote = self.read_target(change).await;
        let current = match after {
            Ok(native) => NativePublication::from_native(&native, target.clone()),
            Err(error) => Err(error),
        };
        if remote.as_ref().ok() != Some(&target) || current.as_ref().ok() != Some(&publication) {
            self.update_check(initial.id, &blocked, guard).await?;
            bail!("candidate or native acceptance changed after publication");
        }
        let observed = match self.read_check(published.id).await {
            Ok(check) => check,
            Err(_) => {
                self.update_check(initial.id, &blocked, guard).await?;
                bail!("published check could not be observed");
            }
        };
        if self.validate_check(&observed, &publication).is_err() {
            self.update_check(initial.id, &blocked, guard).await?;
            bail!("published check no longer matches native acceptance");
        }
        Ok(receipt(publication, observed))
    }
}

fn receipt(publication: NativePublication, check: CheckObservation) -> GitHubGateReceipt {
    GitHubGateReceipt {
        target: publication.target,
        verdict: publication.verdict,
        record_digest: publication.record_digest,
        check,
    }
}
fn external_id(publication: &NativePublication) -> String {
    format!(
        "wcode:{}",
        publication.record_digest().unwrap_or("unavailable")
    )
}

#[derive(Deserialize)]
struct Repository {
    id: u64,
    full_name: String,
}
#[derive(Deserialize)]
struct Side {
    sha: String,
    #[serde(default, rename = "ref")]
    reference: Option<String>,
    repo: Repository,
}
#[derive(Deserialize)]
struct Head {
    sha: String,
    repo: Option<Repository>,
}
#[derive(Deserialize)]
struct Pull {
    number: u64,
    state: String,
    draft: bool,
    merged: bool,
    base: Side,
    head: Head,
}
#[derive(Deserialize)]
struct App {
    id: u64,
}
#[derive(Deserialize)]
struct Check {
    id: u64,
    head_sha: String,
    name: String,
    app: App,
    external_id: String,
    status: String,
    conclusion: Option<String>,
}
fn check_observation(value: Value) -> Result<CheckObservation> {
    let check: Check =
        serde_json::from_value(value).map_err(|_| anyhow!("GitHub check metadata incomplete"))?;
    if check.id == 0 || check.app.id == 0 || !full_oid(&check.head_sha) {
        bail!("GitHub check identity incomplete");
    }
    Ok(CheckObservation {
        id: check.id,
        head_sha: check.head_sha.to_ascii_lowercase(),
        name: check.name,
        source_id: format!("github-app:{}", check.app.id),
        external_id: check.external_id,
        status: check.status,
        conclusion: check.conclusion,
    })
}
impl GitProvider for GitHubProvider {
    fn observe<'a>(&'a self, change: u64) -> ProviderFuture<'a, ProviderTarget> {
        Box::pin(self.read_target(change))
    }
    fn write_check<'a>(
        &'a self,
        publication: &'a NativePublication,
    ) -> ProviderFuture<'a, CheckObservation> {
        Box::pin(async move {
            let guard = PublicationGuard::acquire(self, publication.target())?;
            self.create_check(publication, &guard).await
        })
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/git/github.rs"]
mod tests;
