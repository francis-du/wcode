//! Verify the original GitHub request bytes before using any event identity.
//! Signature protocol: https://docs.github.com/en/webhooks/using-webhooks/validating-webhook-deliveries
//! This authenticates a shared-secret delivery, not its freshness or engineering proof.
use super::*;
use ring::hmac;
use sha2::{Digest, Sha256};

pub const MAX_WEBHOOK_BYTES: usize = 1024 * 1024;

/// No Deserialize or public fields: caller JSON cannot construct authenticated input.
/// The body digest supports a trusted receiver's durable deduplication; this value
/// is not itself a durable inbox, replay check, human approval or Acceptance.
#[derive(Clone, Debug, Serialize)]
pub struct VerifiedPullRequestDelivery {
    schema_version: u32,
    target: ProviderTarget,
    repository_id: u64,
    expected_app_id: u64,
    installation_id: u64,
    delivery_id: String,
    action: String,
    body_digest: String,
    authentication: &'static str,
    freshness_verified: bool,
    replay_protection_verified: bool,
    native_verification: bool,
    human_approval: bool,
    remote_mutations: bool,
}

impl VerifiedPullRequestDelivery {
    pub fn target(&self) -> &ProviderTarget {
        &self.target
    }

    pub fn body_digest(&self) -> &str {
        &self.body_digest
    }

    /// Use a separately provisioned webhook secret, never an installation token.
    /// Raw JSON must be unchanged; duplicate routing/signature headers fail closed.
    /// Delivery IDs are unsigned transport metadata and never establish freshness.
    pub fn verify(
        config: &GitHubConfig,
        expected_installation_id: u64,
        headers: &HeaderMap,
        raw_body: &[u8],
        secret: &[u8],
    ) -> Result<Self> {
        if raw_body.is_empty() || raw_body.len() > MAX_WEBHOOK_BYTES {
            bail!("GitHub webhook body is empty or exceeds its byte bound");
        }
        verify_signature(
            raw_body,
            single_header(headers, "x-hub-signature-256")?,
            secret,
        )?;
        if single_header(headers, "x-github-event")? != "pull_request" {
            bail!("GitHub webhook event type is unsupported");
        }
        let content_type = single_header(headers, "content-type")?;
        let mut parts = content_type.split(';').map(str::trim);
        if !parts
            .next()
            .is_some_and(|kind| kind.eq_ignore_ascii_case("application/json"))
            || parts.any(|parameter| !parameter.eq_ignore_ascii_case("charset=utf-8"))
        {
            bail!("GitHub webhook requires an unmodified JSON body");
        }
        let delivery_id = single_header(headers, "x-github-delivery")?;
        if delivery_id.len() != 36
            || !uuid::Uuid::parse_str(delivery_id).is_ok_and(|id| !id.is_nil())
        {
            bail!("GitHub webhook delivery identity is invalid");
        }
        let payload: PullRequestPayload = serde_json::from_slice(raw_body)
            .map_err(|_| anyhow!("GitHub webhook payload is invalid or incomplete"))?;
        if expected_installation_id == 0
            || payload.installation.id != expected_installation_id
            || payload.repository.id != config.repository_id
            || payload.repository.full_name.to_ascii_lowercase() != config.repository.key()
            || payload.pull_request.base.repo.id != config.repository_id
            || payload
                .pull_request
                .base
                .repo
                .full_name
                .to_ascii_lowercase()
                != config.repository.key()
        {
            bail!("GitHub webhook installation or repository does not match enrollment");
        }
        if !matches!(
            payload.action.as_str(),
            "opened" | "reopened" | "synchronize" | "ready_for_review" | "edited"
        ) || payload.number == 0
            || payload.pull_request.number != payload.number
            || payload.pull_request.state != "open"
            || payload.pull_request.draft
            || payload.pull_request.merged
            || !payload
                .pull_request
                .head
                .repo
                .as_ref()
                .is_some_and(|repo| repo.id > 0)
        {
            bail!("GitHub webhook does not describe a supported open candidate");
        }
        let target = ProviderTarget {
            repository: config.repository.clone(),
            change: payload.number,
            base_sha: payload.pull_request.base.sha.to_ascii_lowercase(),
            head_sha: payload.pull_request.head.sha.to_ascii_lowercase(),
        };
        target.validate()?;
        Ok(Self {
            schema_version: 1,
            target,
            repository_id: config.repository_id,
            expected_app_id: config.app_id,
            installation_id: expected_installation_id,
            delivery_id: delivery_id.to_owned(),
            action: payload.action,
            body_digest: format!("sha256:{:x}", Sha256::digest(raw_body)),
            authentication: "shared_secret_hmac_sha256",
            freshness_verified: false,
            replay_protection_verified: false,
            native_verification: false,
            human_approval: false,
            remote_mutations: false,
        })
    }
}

fn single_header<'a>(headers: &'a HeaderMap, name: &'static str) -> Result<&'a str> {
    let mut values = headers.get_all(name).iter();
    let value = values
        .next()
        .ok_or_else(|| anyhow!("GitHub webhook header is missing"))?;
    if values.next().is_some() {
        bail!("GitHub webhook header is ambiguous");
    }
    value
        .to_str()
        .ok()
        .filter(|value| !value.is_empty() && value.len() <= 256)
        .ok_or_else(|| anyhow!("GitHub webhook header is invalid"))
}

fn verify_signature(body: &[u8], signature: &str, secret: &[u8]) -> Result<()> {
    if !(16..=4096).contains(&secret.len()) {
        bail!("GitHub webhook secret must be separately provisioned within bounds");
    }
    let digest = signature
        .strip_prefix("sha256=")
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| anyhow!("GitHub webhook signature is invalid"))?;
    let mut tag = [0u8; 32];
    for (index, output) in tag.iter_mut().enumerate() {
        *output = u8::from_str_radix(&digest[index * 2..index * 2 + 2], 16)
            .map_err(|_| anyhow!("GitHub webhook signature is invalid"))?;
    }
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    hmac::verify(&key, body, &tag)
        .map_err(|_| anyhow!("GitHub webhook signature verification failed"))
}

#[derive(Deserialize)]
struct Installation {
    id: u64,
}

#[derive(Deserialize)]
struct PullRequestPayload {
    action: String,
    number: u64,
    installation: Installation,
    repository: Repository,
    pull_request: Pull,
}

impl GitHubProvider {
    /// Only an authenticated event may supply candidate SHAs to this entrypoint.
    /// Every attempt still re-reads remote policy/candidate and native Acceptance;
    /// a valid signature is not a reason to execute code, trust a sender or replay green.
    pub async fn publish_delivery(
        &self,
        delivery: &VerifiedPullRequestDelivery,
        root: impl AsRef<std::path::Path>,
        workspace_id: &str,
    ) -> Result<GitHubGateReceipt> {
        self.publish_delivery_with_guard(delivery, root, workspace_id, None)
            .await
    }

    pub(super) async fn publish_delivery_with_guard(
        &self,
        delivery: &VerifiedPullRequestDelivery,
        root: impl AsRef<std::path::Path>,
        workspace_id: &str,
        admission: Option<Arc<PublicationGuard>>,
    ) -> Result<GitHubGateReceipt> {
        if delivery.repository_id != self.config.repository_id
            || delivery.expected_app_id != self.config.app_id
            || delivery.target.repository != self.config.repository
        {
            bail!("GitHub webhook belongs to another publisher enrollment");
        }
        let target = delivery.target();
        self.publish_preflighted_with_guard(
            target.change,
            (&target.base_sha, &target.head_sha),
            root,
            workspace_id,
            admission,
        )
        .await
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/git/webhook.rs"]
mod tests;
