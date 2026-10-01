//! Bounded immutable inputs for a local OSS Policy activation.
//!
//! Validation proves structure and content identity, not source authority.
//! Only a native capture and protected operator/store boundary may establish
//! trust. Imported JSON, matching hashes and command signatures do not approve
//! a Policy or prove that a check ran. The historical Revision is an audit
//! anchor; later candidate revisions are evaluated separately.
use crate::design::{
    AcceptancePolicy, PolicyChangeSet, PolicyLevel, PolicyPathMappings, PolicyRequirements,
};
use crate::evidence::{RequiredVerificationCheck, Revision};
use crate::risk::RiskLevel;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::{self, Write};

const MAX_SNAPSHOT_BYTES: usize = 2 * 1024 * 1024;
// Default + docs-only + 32 rules, each with at most 32 check references.
const MAX_FROZEN_CHECKS: usize = 34 * 32;
const MAX_SOURCE_PATHS: usize = 4096;
const MAX_MAPPING_PATHS: usize = 4096;
const MAX_MAPPING_IDS: usize = 2 * 32 * 32;
const SNAPSHOT_DOMAIN: &[u8] = b"wcode/policy-snapshot/v1";
const SOURCE_DOMAIN: &[u8] = b"wcode/policy-source-seal/v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PolicySnapshot {
    pub(crate) schema_version: u32,
    pub(crate) workspace: String,
    pub(crate) root_digest: String,
    pub(crate) revision: Revision,
    pub(crate) policy: AcceptancePolicy,
    pub(crate) mappings: PolicyPathMappings,
    pub(crate) checks: Vec<FrozenPolicyCheck>,
    pub(crate) sources: Vec<PolicySourceDigest>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FrozenPolicyCheck {
    pub(crate) binding: RequiredVerificationCheck,
    pub(crate) level: String,
    pub(crate) phase: u8,
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    pub(crate) cwd: Option<String>,
    pub(crate) island: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PolicySourceDigest {
    pub(crate) path: String,
    /// Native SourceDocument SHA256: 64 lowercase hexadecimal characters.
    /// None explicitly records a missing input, not a skipped read.
    pub(crate) sha256: Option<String>,
}

impl PolicySnapshot {
    pub(crate) fn validate(&self) -> Result<()> {
        self.checked_digest().map(|_| ())
    }

    pub(crate) fn digest(&self) -> Result<String> {
        self.checked_digest()
    }

    pub(crate) fn source_seal_digest(&self) -> Result<String> {
        self.validate()?;
        let mut sources = self.sources.iter().collect::<Vec<_>>();
        sources.sort_by(|left, right| left.path.cmp(&right.path));
        bounded_digest(SOURCE_DOMAIN, &(&self.root_digest, sources))
    }

    fn checked_digest(&self) -> Result<String> {
        if self.checks.is_empty() || self.checks.len() > MAX_FROZEN_CHECKS {
            bail!("frozen Policy check inventory is empty or exceeds its count bound");
        }
        if self.sources.is_empty() || self.sources.len() > MAX_SOURCE_PATHS {
            bail!("Policy source inventory is empty or exceeds its count bound");
        }
        let mapping_ids = self
            .mappings
            .components
            .len()
            .checked_add(self.mappings.requirements.len())
            .context("Policy mapping count overflow")?;
        let mapping_paths = self
            .mappings
            .components
            .values()
            .chain(self.mappings.requirements.values())
            .try_fold(0usize, |total, paths| total.checked_add(paths.len()))
            .context("Policy mapping path count overflow")?;
        if mapping_ids > MAX_MAPPING_IDS || mapping_paths > MAX_MAPPING_PATHS {
            bail!("Policy mapping inventory exceeds its count bound");
        }
        // Count/hash streaming JSON before command-signature work or allocation.
        let digest = bounded_digest(SNAPSHOT_DOMAIN, self)?;
        if self.schema_version != 1
            || !bounded_text(&self.workspace, 300)
            || !sha256_identity(&self.root_digest)
            || !sha256_identity(&self.revision.code)
            || !self.revision.design.as_deref().is_some_and(sha256_identity)
        {
            bail!("Policy snapshot requires version 1 and complete root/code/Design identities");
        }
        if !self.mappings.complete {
            bail!("Policy snapshot Design mappings are incomplete");
        }
        for (id, paths) in self
            .mappings
            .components
            .iter()
            .chain(self.mappings.requirements.iter())
        {
            if !bounded_id(id) || paths.is_empty() {
                bail!("Policy snapshot mapping identity or file scope is missing");
            }
            let mut seen = BTreeSet::new();
            for path in paths {
                if !relative_path(path) || !seen.insert(path) {
                    bail!("Policy snapshot mapping contains an invalid or duplicate file path");
                }
            }
        }
        let mut source_paths = BTreeSet::new();
        let mut present_source = false;
        for source in &self.sources {
            if !relative_path(&source.path) || !source_paths.insert(&source.path) {
                bail!("Policy snapshot source contains an invalid or duplicate path");
            }
            if let Some(hash) = &source.sha256 {
                if !full_sha256(hash) {
                    bail!("Policy source digest is not a complete native SHA256");
                }
                present_source = true;
            }
        }
        if !present_source {
            bail!("Policy snapshot has no captured source definition");
        }
        let mut check_ids = BTreeSet::new();
        for check in &self.checks {
            validate_check(check)?;
            if !check_ids.insert(check.binding.id.as_str()) {
                bail!("Policy snapshot contains duplicate frozen check IDs");
            }
        }
        // Reuse the draft's sole structural/mapping validation and union engine.
        // Empty change paths here validate the snapshot, never accept a change.
        self.policy
            .select(
                &PolicyChangeSet {
                    paths: Vec::new(),
                    complete: true,
                    executable_changes: Some(false),
                    regular_file_changes: Some(true),
                },
                &self.mappings,
                &PolicyRequirements {
                    minimum_level: PolicyLevel::Quick,
                    checks: Vec::new(),
                    stages: Vec::new(),
                    reviewers: Vec::new(),
                    human_approval: false,
                    human_approval_min_risk: None,
                },
                RiskLevel::Low,
            )
            .map_err(|errors| {
                anyhow::anyhow!("invalid frozen Policy draft: {}", errors.join(", "))
            })?;
        let requirements = std::iter::once(&self.policy.requirements)
            .chain(self.policy.docs_only.iter().map(|docs| &docs.require))
            .chain(self.policy.rules.iter().map(|rule| &rule.require));
        let required_ids = requirements
            .flat_map(|requirements| requirements.checks.iter().map(String::as_str))
            .collect::<BTreeSet<_>>();
        if required_ids.iter().any(|id| !check_ids.contains(id)) {
            bail!("Policy branch references an unavailable frozen check");
        }
        if check_ids != required_ids {
            bail!("Policy snapshot contains an unreferenced frozen check");
        }
        Ok(digest)
    }
}

fn validate_check(check: &FrozenPolicyCheck) -> Result<()> {
    if !check.binding.valid()
        || !matches!(check.level.as_str(), "quick" | "full")
        || check.phase > 3
        || !bounded_text(&check.program, 256)
        || check.program.chars().any(char::is_whitespace)
        || check.args.len() > 128
        || check
            .args
            .iter()
            .any(|arg| arg.len() > 4096 || arg.chars().any(char::is_control))
        || check.args.iter().map(String::len).sum::<usize>() > 64 * 1024
    {
        bail!("invalid frozen Policy check definition");
    }
    // These are lexical identities only. Native capture and Workspace checks
    // remain responsible for containment, symlinks and execution authorization.
    let cwd = check
        .cwd
        .as_deref()
        .context("frozen Policy check cwd is missing")?;
    let island = check
        .island
        .as_deref()
        .context("frozen Policy check island is missing")?;
    if (cwd != "." && !relative_path(cwd)) || (island != "." && !relative_path(island)) {
        bail!("frozen Policy check cwd/island is not canonical");
    }
    let expected = RequiredVerificationCheck::from_command(
        &check.binding.id,
        &check.program,
        &check.args,
        cwd,
        island,
    );
    if check.binding != expected {
        bail!("frozen Policy check signature does not match its exact command");
    }
    Ok(())
}

fn bounded_text(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn bounded_id(value: &str) -> bool {
    bounded_text(value, 160) && !value.chars().any(char::is_whitespace)
}

fn relative_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && !path
            .chars()
            .any(|character| character.is_control() || matches!(character, '*' | '?' | '[' | ']'))
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn full_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn sha256_identity(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(full_sha256)
}

struct BoundedHashWriter {
    bytes: usize,
    hash: Sha256,
}

impl Write for BoundedHashWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let total = self.bytes.checked_add(bytes.len()).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Policy snapshot byte overflow")
        })?;
        if total > MAX_SNAPSHOT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Policy snapshot exceeds 2 MiB",
            ));
        }
        self.bytes = total;
        self.hash.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn bounded_digest(domain: &[u8], value: &impl Serialize) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update([0]);
    let mut writer = BoundedHashWriter { bytes: 0, hash };
    serde_json::to_writer(&mut writer, value).context("cannot encode a bounded Policy snapshot")?;
    Ok(format!("sha256:{:x}", writer.hash.finalize()))
}

#[cfg(test)]
#[path = "../../tests/unit/verification/policy.rs"]
mod tests;
