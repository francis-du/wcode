//! GitHub Check Runs transport for a separately deployed trusted publisher.
//! No PR checkout, command execution, branch-protection mutation or JSON CAR import.
//! Protocol: https://docs.github.com/en/rest/checks/runs
use super::*;
use anyhow::{anyhow, bail, Result};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION};
use reqwest::{Client, Method, StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
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

    /// List the check runs bound to one exact commit. Read-only operator
    /// inspection: the response is never a verification or Acceptance claim.
    pub async fn list_check_runs(&self, commit: &str) -> Result<Vec<CheckObservation>> {
        if commit.len() != 64 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("invalid commit identity");
        }
        let suffix = ["commits", commit, "check-runs"];
        let value = self
            .request(Method::GET, &suffix, None, StatusCode::OK)
            .await?;
        let runs = match value.get("check_runs").and_then(Value::as_array) {
            Some(runs) => runs,
            None => bail!("GitHub returned no check_runs array"),
        };
        if runs.len() > MAX_CHECK_RUNS {
            bail!("GitHub returned too many check runs");
        }
        runs.iter()
            .map(|run| check_observation(run.clone()))
            .collect()
    }

    /// Read the bounded annotations of one check run. Read-only inspection;
    /// annotations are diagnostics, never an Acceptance or verification claim.
    pub async fn read_check_annotations(&self, id: u64) -> Result<Vec<CheckAnnotation>> {
        if id == 0 {
            bail!("invalid check identity");
        }
        let id_text = id.to_string();
        let value = self
            .request(
                Method::GET,
                &["check-runs", &id_text, "annotations"],
                None,
                StatusCode::OK,
            )
            .await?;
        let annotations = match value.get("annotations").and_then(Value::as_array) {
            Some(annotations) => annotations,
            None => bail!("GitHub returned no annotations array"),
        };
        if annotations.len() > MAX_CHECK_ANNOTATIONS {
            bail!("GitHub returned too many check annotations");
        }
        annotations
            .iter()
            .map(|value| check_annotation(value.clone()))
            .collect()
    }

    /// Read one authoritative CI receipt: the check run plus its bounded
    /// annotations, the repository binding it was captured from, and a digest
    /// over every field. The digest only evidences this captured receipt; it is
    /// not a verification or Acceptance claim.
    pub async fn read_check_receipt(&self, id: u64) -> Result<CheckReceipt> {
        let check = self.read_check(id).await?;
        let annotations = self.read_check_annotations(id).await?;
        let digest = check_receipt_digest(&check, &annotations);
        Ok(CheckReceipt {
            schema_version: CHECK_RECEIPT_SCHEMA_VERSION,
            provider: "github".to_owned(),
            repository: self.config.repository.key(),
            repository_id: self.config.repository_id,
            app_id: app_id_from_source(&check.source_id),
            check,
            annotations,
            digest,
        })
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
/// Receipt files carry CI metadata plus annotation text, so they are created
/// private to the operator instead of with the process default mode.
#[cfg(unix)]
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|error| anyhow!("cannot write {}: {error}", path.display()))
}

#[cfg(not(unix))]
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).map_err(|error| anyhow!("cannot write {}: {error}", path.display()))
}

/// The store is replaced through a temporary file in the same directory.
/// `O_CREAT|O_EXCL|O_NOFOLLOW` keeps a pre-planted symlink or a colliding name
/// from redirecting the write somewhere the operator did not choose.
#[cfg(unix)]
fn write_private_exclusive(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(0o600)
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|error| anyhow!("cannot write {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn write_private_exclusive(path: &Path, bytes: &[u8]) -> Result<()> {
    write_private(path, bytes)
}

fn unique_temporary_path(directory: &Path, name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
    directory.join(format!(".{name}.tmp-{}-{sequence}", std::process::id()))
}

/// Bounded local retention of captured authoritative CI receipts, one JSON
/// object per line. Receipts are keyed by check-run ID, newest last, and the
/// store is capped so an operator can retain receipts without unbounded growth.
const MAX_RECEIPTS: usize = 128;
const MAX_RECEIPT_STORE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_RECEIPT_BYTES: u64 = 64 * 1024;
const MAX_CHECK_RUNS: usize = 1000;
const MAX_CHECK_ANNOTATIONS: usize = 1000;

pub(crate) fn load_receipts(path: &Path) -> Result<Vec<CheckReceipt>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes =
        std::fs::read(path).map_err(|error| anyhow!("cannot read {}: {error}", path.display()))?;
    if bytes.len() as u64 > MAX_RECEIPT_STORE_BYTES {
        bail!("receipt store exceeds its size bound");
    }
    let mut receipts = Vec::new();
    for line in bytes.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        if line.len() as u64 > MAX_RECEIPT_BYTES {
            bail!("receipt store entry exceeds its size bound");
        }
        let receipt: CheckReceipt = serde_json::from_slice(line)
            .map_err(|error| anyhow!("invalid receipt store line: {error}"))?;
        if receipt.schema_version != CHECK_RECEIPT_SCHEMA_VERSION {
            bail!("unsupported receipt store schema");
        }
        if !receipt.verify_digest() {
            bail!("receipt store entry digest does not match its contents");
        }
        receipts.push(receipt);
    }
    if receipts.len() > MAX_RECEIPTS {
        bail!("receipt store exceeds its entry bound");
    }
    Ok(receipts)
}

pub(crate) fn append_receipt(path: &Path, receipt: &CheckReceipt) -> Result<()> {
    if receipt.schema_version != CHECK_RECEIPT_SCHEMA_VERSION {
        bail!("cannot retain a receipt with an unsupported schema");
    }
    if !receipt.verify_digest() {
        bail!("cannot retain a receipt whose digest does not match its contents");
    }
    let mut receipts = load_receipts(path)?;
    receipts.retain(|existing| existing.check.id != receipt.check.id);
    receipts.push(receipt.clone());
    let overflow = receipts.len().saturating_sub(MAX_RECEIPTS);
    receipts.drain(..overflow);
    let mut encoded = receipts
        .iter()
        .map(|entry| {
            serde_json::to_string(entry).map_err(|error| anyhow!("cannot encode receipt: {error}"))
        })
        .collect::<Result<Vec<_>>>()?
        .join("\n");
    encoded.push('\n');
    // Replace the store atomically so an interrupted write can never leave a
    // truncated receipt behind for the next reader.
    let directory = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => Path::new(".").to_path_buf(),
    };
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "receipts.jsonl".to_owned());
    let temporary = unique_temporary_path(&directory, &name);
    write_private_exclusive(&temporary, encoded.as_bytes())?;
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(anyhow!("cannot replace {}: {error}", path.display()));
    }
    Ok(())
}

fn hash_text(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

fn hash_optional_text(hasher: &mut Sha256, value: Option<&str>) {
    match value {
        Some(value) => {
            hasher.update([1u8]);
            hash_text(hasher, value);
        }
        None => hasher.update([0u8]),
    }
}

fn hash_optional_u64(hasher: &mut Sha256, value: Option<u64>) {
    match value {
        Some(value) => {
            hasher.update([1u8]);
            hasher.update(value.to_le_bytes());
        }
        None => hasher.update([0u8]),
    }
}

/// Digest over every captured field, length-prefixed so no two field layouts
/// can collide. It evidences capture integrity only; it is not an authentication
/// of the producer.
pub(crate) fn check_receipt_digest(
    check: &CheckObservation,
    annotations: &[CheckAnnotation],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(check.id.to_le_bytes());
    hash_text(&mut hasher, &check.head_sha);
    hash_text(&mut hasher, &check.name);
    hash_text(&mut hasher, &check.source_id);
    hash_text(&mut hasher, &check.external_id);
    hash_text(&mut hasher, &check.status);
    hash_optional_text(&mut hasher, check.conclusion.as_deref());
    hasher.update((annotations.len() as u64).to_le_bytes());
    for annotation in annotations {
        hash_text(&mut hasher, &annotation.path);
        hash_optional_u64(&mut hasher, annotation.start_line);
        hash_optional_u64(&mut hasher, annotation.end_line);
        hash_text(&mut hasher, &annotation.annotation_level);
        hash_text(&mut hasher, &annotation.title);
        hash_text(&mut hasher, &annotation.message);
    }
    format!("{:x}", hasher.finalize())
}

fn check_annotation(value: Value) -> Result<CheckAnnotation> {
    let path = value
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let level = value
        .get("annotation_level")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let start_line = value.get("start_line").and_then(Value::as_u64);
    let end_line = value.get("end_line").and_then(Value::as_u64);
    if path.is_empty()
        || path.len() > 512
        || level.is_empty()
        || level.len() > 16
        || title.len() > 256
        || message.len() > 4096
    {
        bail!("check annotation exceeds its bounds");
    }
    if start_line.is_some_and(|start| end_line.is_some_and(|end| start > end)) {
        bail!("check annotation line range is reversed");
    }
    Ok(CheckAnnotation {
        path: path.to_owned(),
        start_line,
        end_line,
        annotation_level: level.to_owned(),
        title: title.to_owned(),
        message: message.to_owned(),
    })
}

fn app_id_from_source(source_id: &str) -> u64 {
    source_id
        .strip_prefix("github-app:")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
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
