use super::*;
use crate::evidence::Revision;
use crate::verification::policy::PolicySnapshot;
use crate::verification::policy_store::OperatorReceipt;
use anyhow::{bail, Context};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_POLICY_LIFETIME_MS: u64 = 365 * 24 * 60 * 60 * 1_000;

#[derive(Clone, Copy, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Action {
    #[default]
    Status,
    Preview,
    Activate,
    Revoke,
}

impl Action {
    fn name(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Preview => "preview",
            Self::Activate => "activate",
            Self::Revoke => "revoke",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyArgs {
    #[serde(default)]
    action: Action,
    workspace: Option<String>,
    expected_generation: Option<u64>,
    snapshot_digest: Option<String>,
    expires_at_ms: Option<u64>,
}

impl PolicyArgs {
    fn parse(value: &Value) -> anyhow::Result<Self> {
        let object = value
            .as_object()
            .context("acceptance_policy arguments must be an object")?;
        for key in [
            "workspace",
            "expected_generation",
            "snapshot_digest",
            "expires_at_ms",
        ] {
            if object.get(key).is_some_and(Value::is_null) {
                bail!("{key} must not be null");
            }
        }
        let args: Self = serde_json::from_value(value.clone())?;
        if args.workspace.as_ref().is_some_and(|workspace| {
            workspace.is_empty() || workspace.len() > 300 || workspace.chars().any(char::is_control)
        }) {
            bail!("invalid acceptance_policy workspace");
        }
        match args.action {
            Action::Status | Action::Preview => {
                if args.expected_generation.is_some()
                    || args.snapshot_digest.is_some()
                    || args.expires_at_ms.is_some()
                {
                    bail!("status and preview do not accept mutation arguments");
                }
            }
            Action::Activate => {
                args.expected_generation
                    .context("activate requires expected_generation")?;
                let digest = args
                    .snapshot_digest
                    .as_deref()
                    .context("activate requires snapshot_digest")?;
                if !valid_digest(digest) {
                    bail!("snapshot_digest must be a complete lowercase sha256 identity");
                }
            }
            Action::Revoke => {
                args.expected_generation
                    .context("revoke requires expected_generation")?;
                if args.snapshot_digest.is_some() || args.expires_at_ms.is_some() {
                    bail!("revoke does not accept snapshot_digest or expires_at_ms");
                }
            }
        }
        Ok(args)
    }
}

pub(super) async fn call(state: &AppState, value: &Value) -> AnyResult<Value> {
    let args = PolicyArgs::parse(value)?;
    let (workspace_id, workspace) = selected_workspace(state, value).map_err(anyhow::Error::msg)?;
    // Tokio task-local identity is unavailable inside spawn_blocking.
    let requester = mcp_writer::owner_binding(&mcp_writer::current_owner());
    let harness = state.harness.clone();
    let workspaces = state.workspaces.clone();
    let instance = state.auth.instance_id().to_owned();
    run_blocking(move || {
        match args.action {
            Action::Status => return harness.acceptance_policy_status(&workspace_id, &workspace),
            Action::Preview => {
                let snapshot = harness.acceptance_policy_preview(&workspace_id, &workspace)?;
                return Ok(json!({
                    "workspace":workspace_id, "status":"preview",
                    "snapshot_digest":snapshot.digest()?, "snapshot":snapshot,
                    "authority":"none", "acceptance_record":false,
                }));
            }
            Action::Activate | Action::Revoke => {}
        }
        let expected_generation = args.expected_generation.context("expected_generation is missing")?;
        let status = harness.acceptance_policy_status(&workspace_id, &workspace)?;
        let generation = status.get("generation").and_then(Value::as_u64)
            .context("native Policy status has no valid generation")?;
        if generation != expected_generation {
            bail!("Policy generation changed; expected {expected_generation}, current {generation}");
        }
        let root_digest = status.get("root_digest").and_then(Value::as_str)
            .filter(|digest| valid_digest(digest))
            .context("native Policy status has no valid root identity")?;
        let revision: Revision = serde_json::from_value(
            status.get("revision").context("native Policy status has no revision")?.clone(),
        )?;
        if !valid_digest(&revision.code)
            || revision.design.as_deref().is_some_and(|value| !valid_digest(value)) {
            bail!("complete native revision is required for Policy decisions");
        }
        let record_digest = optional_status_digest(&status, "record_digest")?;
        let historical_snapshot = optional_status_digest(&status, "snapshot_digest")?;
        if (generation == 0) != record_digest.is_none() {
            bail!("native Policy generation and record identity are inconsistent");
        }
        let snapshot = if args.action == Action::Activate {
            let snapshot = harness.acceptance_policy_preview(&workspace_id, &workspace)?;
            let digest = snapshot.digest()?;
            if args.snapshot_digest.as_deref() != Some(digest.as_str()) {
                bail!("Policy snapshot changed; obtain a fresh native preview before requesting activation");
            }
            if snapshot.workspace != workspace_id
                || snapshot.root_digest != root_digest || snapshot.revision != revision {
                bail!("Policy revision or root changed during decision capture; retry");
            }
            Some(snapshot)
        } else {
            // Revocation must remain available without a valid repository draft.
            None
        };
        let snapshot_digest = snapshot.as_ref().map(PolicySnapshot::digest).transpose()?;
        if let Some(expiry) = args.expires_at_ms {
            let lifetime = expiry.checked_sub(now_ms()?);
            if !lifetime.is_some_and(|value| value != 0 && value <= MAX_POLICY_LIFETIME_MS) {
                bail!("expires_at_ms must be in the next 365 days of the native clock");
            }
        }
        let binding = json!({
            "domain":"wcode/acceptance-policy-decision/v1",
            "instance":instance, "requester":requester,
            "root_digest":root_digest, "workspace":workspace_id,
            "action":args.action.name(), "revision":revision,
            "expected_generation":expected_generation,
            "record_digest":record_digest, "historical_snapshot_digest":historical_snapshot,
            "snapshot_digest":snapshot_digest, "expires_at_ms":args.expires_at_ms,
        });
        let fingerprint = format!("sha256:{:x}", Sha256::digest(serde_json::to_vec(&binding)?));
        let mut summary = format!(
            "Local operator Policy {} (one use, 2 minute expiry)\nWorkspace: {workspace_id}\nRequester: {requester}\nGeneration: {expected_generation}\nRoot: {root_digest}\nCode: {}\nDesign: {}\nRecord: {}\nSnapshot: {}\nPolicy expiry: {}\n",
            args.action.name(), revision.code, revision.design.as_deref().unwrap_or("none"),
            record_digest.unwrap_or("none"), snapshot_digest.as_deref().unwrap_or("not applicable"),
            args.expires_at_ms.map_or_else(|| "none".to_owned(), |value| value.to_string()),
        );
        if let Some(snapshot) = &snapshot {
            summary.push_str(&format!(
                "Draft Policy: {} v{}\nDefault matrix: {:?}; {} checks, {} stages, {} reviewers; human approval={}\nFrozen check inventory: {}; rules: {}; docs-only branch: {}\n",
                snapshot.policy.id, snapshot.policy.version,
                snapshot.policy.requirements.minimum_level,
                snapshot.policy.requirements.checks.len(),
                snapshot.policy.requirements.stages.len(),
                snapshot.policy.requirements.reviewers.len(),
                snapshot.policy.requirements.human_approval,
                snapshot.checks.len(), snapshot.policy.rules.len(), snapshot.policy.docs_only.is_some(),
            ));
            for check in snapshot.checks.iter().take(8) {
                summary.push_str(&format!("Check: {} · {}\n", check.binding.id, check.binding.signature));
            }
            if snapshot.checks.len() > 8 {
                summary.push_str("Check list truncated; inspect the full native preview for this snapshot digest.\n");
            }
        }
        // Command grants and Full Access never substitute for a local operator decision.
        let grant = workspaces.require_human_decision(&workspace_id, &summary, &fingerprint)?;
        let decided_at_ms = grant.decided_at_ms
            .filter(|value| *value != 0)
            .context("consumed operator grant has no decision timestamp")?;
        // AUTH numbering is process-local; durable replay identity includes the instance.
        let receipt_id = format!("{instance}:{}", grant.id);
        let receipt = OperatorReceipt::new(&receipt_id, decided_at_ms)?;
        // Native recapture/CAS may reject; a consumed grant is never restored.
        let record = match snapshot {
            Some(snapshot) => harness.acceptance_policy_activate_authorized(
                &workspace_id, &workspace, expected_generation,
                snapshot, receipt, args.expires_at_ms,
            )?,
            None => harness.acceptance_policy_revoke_authorized(
                &workspace_id, &workspace, expected_generation, &revision, receipt,
            )?,
        };
        Ok(json!({
            "workspace":workspace_id, "action":args.action.name(),
            "authority":"local_operator", "acceptance_record":false,
            "record":record, "operator_authorization":grant, "requester_binding":requester,
        }))
    }).await
}

fn optional_status_digest<'a>(status: &'a Value, key: &str) -> anyhow::Result<Option<&'a str>> {
    match status.get(key) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if valid_digest(value) => Ok(Some(value)),
        _ => bail!("native Policy status has invalid {key}"),
    }
}

fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    })
}

fn now_ms() -> anyhow::Result<u64> {
    u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())
        .context("Policy clock overflow")
}

#[cfg(test)]
#[path = "../../../../tests/unit/integrations/mcp/policy.rs"]
mod tests;
