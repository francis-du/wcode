use super::*;
use crate::design::{AcceptancePolicy, PolicyPathMappings};
use crate::verification::policy::{FrozenPolicyCheck, PolicySnapshot};
use crate::verification::policy_store::{self, NativePolicyRecord, OperatorReceipt};
use std::time::{SystemTime, UNIX_EPOCH};

impl ToolHarness {
    /// Capture repository-owned inputs. This never consumes approval or executes checks.
    pub(crate) fn acceptance_policy_preview(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
    ) -> Result<PolicySnapshot> {
        let revision = self.current_revision(workspace)?;
        let load = design::load_design(workspace)?;
        if !load.initialized || load.error_count() != 0 {
            bail!("complete valid Design State is required for Policy activation");
        }
        let policy = load
            .state
            .project
            .as_ref()
            .and_then(|project| project.acceptance_policy.clone())
            .context("project has no Acceptance Policy draft")?;
        let errors = policy.validate(&load.state);
        if !errors.is_empty() {
            bail!("invalid Policy draft: {}", errors.join(", "));
        }
        let profile = harness_profile::capture_policy_profile(workspace)?;
        if !profile.discovery.complete {
            bail!("complete native check discovery is required for Policy activation");
        }
        let mut checks = Vec::new();
        for id in policy_check_ids(&policy) {
            let candidates = profile
                .recommended_checks
                .iter()
                .filter(|check| check.id == id)
                .collect::<Vec<_>>();
            if candidates.len() != 1 {
                bail!("required Policy check is unavailable or ambiguous: {id}");
            }
            let check = candidates[0];
            // Native "workspace" is a scope label, not a directory name.
            for path in std::iter::once(&check.cwd)
                .chain((check.island != "workspace").then_some(&check.island))
            {
                if workspace.path_info(path)?.kind != "directory" {
                    bail!("native Policy check cwd/island is not a directory");
                }
            }
            checks.push(FrozenPolicyCheck {
                binding: verification_check_binding(check),
                level: check.level.clone(),
                phase: check.phase,
                program: check.program.clone(),
                args: check.args.clone(),
                cwd: Some(check.cwd.clone()),
                island: Some(check.island.clone()),
            });
        }
        let snapshot = PolicySnapshot {
            schema_version: 1,
            workspace: workspace_id.to_owned(),
            root_digest: policy_store::workspace_root_digest(workspace)?,
            revision: revision.clone(),
            mappings: policy_mappings(workspace, &load.state, &policy)?,
            policy,
            checks,
            sources: profile.policy_sources.clone(),
        };
        snapshot.validate()?;
        // The store must never bless a mixed before/after capture. These guards
        // detect observed drift; they are not an atomic OS filesystem snapshot.
        if self.current_revision(workspace)? != revision {
            bail!("repository revision changed during Policy capture; retry");
        }
        let confirmed = harness_profile::capture_policy_profile(workspace)?;
        if !confirmed.discovery.complete
            || confirmed.policy_sources != snapshot.sources
            || !policy_checks_current(&confirmed, &snapshot)
        {
            bail!("native check definitions changed during Policy capture; retry");
        }
        Ok(snapshot)
    }

    pub(crate) fn acceptance_policy_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
    ) -> Result<Value> {
        let revision = self.current_revision(workspace)?;
        let root_digest = policy_store::workspace_root_digest(workspace)?;
        let record = policy_store::load(workspace, workspace_id)?;
        let Some(record) = record else {
            return Ok(json!({
                "workspace":workspace_id, "root_digest":root_digest,
                "revision":revision, "generation":0, "status":"inactive",
                "record_digest":null, "snapshot_digest":null,
                "authority":"none", "acceptance_record":false,
            }));
        };
        let snapshot = record.snapshot();
        let status = match snapshot {
            None => "revoked",
            Some(_) if !record.is_active_at(policy_now_ms()?) => "expired",
            Some(snapshot)
                if !self
                    .policy_definitions_current(workspace, snapshot)
                    .unwrap_or(false) =>
            {
                "stale_definition"
            }
            Some(_) => "active",
        };
        Ok(json!({
            "workspace":workspace_id, "root_digest":root_digest,
            "revision":revision, "generation":record.generation(), "status":status,
            "record_digest":record.digest()?,
            "snapshot_digest":snapshot.map(PolicySnapshot::digest).transpose()?,
            "source_seal_digest":snapshot.map(PolicySnapshot::source_seal_digest).transpose()?,
            "policy":snapshot.map(|snapshot| json!({
                "id":snapshot.policy.id, "version":snapshot.policy.version,
                "digest":snapshot.policy.digest(), "activation_revision":snapshot.revision,
            })),
            "created_at_ms":record.created_at_ms(),
            "authority":"local_operator", "acceptance_record":false,
            "definition_precision":"captured native configuration files; not all transitive executable inputs",
        }))
    }

    pub(crate) fn acceptance_policy_activate_authorized(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        expected_generation: u64,
        snapshot: PolicySnapshot,
        receipt: OperatorReceipt,
        expires_at_ms: Option<u64>,
    ) -> Result<NativePolicyRecord> {
        let confirmed = self.acceptance_policy_preview(workspace_id, workspace)?;
        if confirmed.digest()? != snapshot.digest()? {
            bail!("Policy inputs changed after operator approval; consumed grant cannot be reused");
        }
        policy_store::activate(
            workspace,
            workspace_id,
            expected_generation,
            confirmed,
            receipt,
            expires_at_ms,
        )
    }

    pub(crate) fn acceptance_policy_revoke_authorized(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        expected_generation: u64,
        expected_revision: &crate::evidence::Revision,
        receipt: OperatorReceipt,
    ) -> Result<NativePolicyRecord> {
        if self.current_revision(workspace)? != *expected_revision {
            bail!("revision changed after Policy revocation approval");
        }
        policy_store::revoke(workspace, workspace_id, expected_generation, receipt)
    }

    pub(super) fn policy_definitions_current(
        &self,
        workspace: &Workspace,
        snapshot: &PolicySnapshot,
    ) -> Result<bool> {
        let profile = harness_profile::capture_policy_profile(workspace)?;
        if !profile.discovery.complete || profile.policy_sources != snapshot.sources {
            return Ok(false);
        }
        Ok(policy_checks_current(&profile, snapshot))
    }
}

fn policy_checks_current(profile: &ProjectProfile, snapshot: &PolicySnapshot) -> bool {
    snapshot.checks.iter().all(|frozen| {
        let mut checks = profile
            .recommended_checks
            .iter()
            .filter(|check| check.id == frozen.binding.id);
        let Some(check) = checks.next() else {
            return false;
        };
        checks.next().is_none()
            && verification_check_binding(check) == frozen.binding
            && check.level == frozen.level
            && check.phase == frozen.phase
    })
}

fn policy_check_ids(policy: &AcceptancePolicy) -> BTreeSet<&str> {
    std::iter::once(&policy.requirements)
        .chain(policy.docs_only.iter().map(|docs| &docs.require))
        .chain(policy.rules.iter().map(|rule| &rule.require))
        .flat_map(|requirements| requirements.checks.iter().map(String::as_str))
        .collect()
}

fn policy_mappings(
    workspace: &Workspace,
    state: &design::DesignState,
    policy: &AcceptancePolicy,
) -> Result<PolicyPathMappings> {
    let mut mappings = PolicyPathMappings {
        complete: true,
        ..Default::default()
    };
    let component_paths = |id: &str| -> Result<Vec<String>> {
        let component = state
            .components
            .get(id)
            .context("Policy mapping references an unknown component")?;
        if component.implementation.len() > 4096 {
            bail!("Policy component mapping exceeds its path bound");
        }
        let paths = component
            .implementation
            .iter()
            .map(|reference| reference.path().to_owned())
            .collect::<BTreeSet<_>>();
        if paths.is_empty() {
            bail!("Policy component has no declared file mapping: {id}");
        }
        for path in &paths {
            // Metadata only: validate ordinary contained files without hashing an
            // unbounded source body. Symbol references use conservative file scope.
            workspace.source_metadata_stamp(path)?;
        }
        Ok(paths.into_iter().collect())
    };
    for id in policy.rules.iter().flat_map(|rule| &rule.when.components) {
        mappings.components.insert(id.clone(), component_paths(id)?);
    }
    for id in policy.rules.iter().flat_map(|rule| &rule.when.requirements) {
        let requirement = state
            .requirements
            .get(id)
            .context("Policy mapping references an unknown requirement")?;
        if requirement.implemented_by.is_empty() {
            bail!("Policy requirement has no component ownership: {id}");
        }
        let mut paths = BTreeSet::new();
        for component in &requirement.implemented_by {
            paths.extend(component_paths(component)?);
            if paths.len() > 4096 {
                bail!("Policy requirement mapping exceeds its path bound");
            }
        }
        mappings
            .requirements
            .insert(id.clone(), paths.into_iter().collect());
    }
    Ok(mappings)
}

fn policy_now_ms() -> Result<u64> {
    u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())
        .context("Policy clock overflow")
}

#[cfg(test)]
#[path = "../../../tests/unit/runtime/harness/policy.rs"]
mod tests;
