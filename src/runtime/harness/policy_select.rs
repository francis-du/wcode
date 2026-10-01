//! Native approved Policy selection shared by planning, execution and Acceptance.
use super::harness_core::verification_check_plans;
use super::*;
use crate::design::{PolicyChangeSet, PolicyLevel, PolicyRequirements, PolicySelection};
use crate::risk::{RiskLevel, VerificationProfile};
use crate::verification::change::GitChangeSnapshot;
use crate::verification::policy::PolicySnapshot;
use crate::verification::policy_store;

pub(super) struct NativePolicySelection {
    pub record: policy_store::NativePolicyRecord,
    pub snapshot: PolicySnapshot,
    pub selection: PolicySelection,
    pub risk_level: RiskLevel,
    pub risk: crate::intelligence_types::RiskStatus,
    pub required_checks: Vec<crate::evidence::RequiredVerificationCheck>,
}

impl NativePolicySelection {
    pub fn plan_binding(&self) -> Result<String> {
        Ok(format!(
            "project-policy/v1/{}/{}",
            self.record.generation(),
            self.snapshot.digest()?
        ))
    }
}

impl ToolHarness {
    pub(super) fn select_native_policy(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        git: &GitChangeSnapshot,
        review: &ChangeReviewReport,
        profile: &ProjectProfile,
    ) -> Result<Option<NativePolicySelection>> {
        let Some(record) = policy_store::load(workspace, workspace_id)? else {
            return Ok(None);
        };
        let snapshot = record
            .snapshot()
            .context("Acceptance Policy is revoked")?
            .clone();
        let now = u64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis(),
        )?;
        if !record.is_active_at(now) {
            bail!("Acceptance Policy is expired or its clock is unavailable");
        }
        if !self.policy_definitions_current(workspace, &snapshot)? {
            bail!("Acceptance Policy definitions are stale; explicit operator reactivation is required");
        }
        if !profile.discovery.complete {
            bail!("Acceptance Policy requires complete native check discovery");
        }
        let known = harness_profile::known_checks_from_profile(profile);
        let risk = self.intelligence.risk_status(
            workspace_id,
            workspace,
            &self.code_index,
            &known,
            review,
        )?;
        let risk_level = crate::execution::verification_risk_floor(workspace, risk.level)?;
        let risk_profile = VerificationProfile::for_risk(risk_level);
        let plans = verification_check_plans(profile, review);
        let minimum_level = if risk_level >= RiskLevel::Medium {
            PolicyLevel::Full
        } else {
            PolicyLevel::Quick
        };
        let floor_checks = if minimum_level == PolicyLevel::Full {
            &plans.full
        } else {
            &plans.quick
        };
        let mut stages = Vec::new();
        if risk_profile.require_property {
            stages.push(crate::verification::VerificationStage::Property);
        }
        if risk_profile.require_mutation {
            stages.push(crate::verification::VerificationStage::Mutation);
        }
        if risk_profile.require_fuzz {
            stages.push(crate::verification::VerificationStage::Fuzz);
        }
        if risk_profile
            .deterministic_checks
            .iter()
            .any(|check| check == "runtime-gate")
        {
            stages.push(crate::verification::VerificationStage::RuntimeCanary);
        }
        let floor = PolicyRequirements {
            minimum_level,
            checks: floor_checks.iter().map(|check| check.id.clone()).collect(),
            stages,
            reviewers: Vec::new(),
            human_approval: risk_profile.require_human_approval,
            human_approval_min_risk: None,
        };
        let paths = git
            .changes
            .iter()
            .flat_map(|change| change.old_path.iter().chain(change.new_path.iter()))
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut selection = snapshot
            .policy
            .select(
                &PolicyChangeSet {
                    paths,
                    complete: git.complete,
                    executable_changes: git.executable_changes,
                    regular_file_changes: git.regular_file_changes,
                },
                &snapshot.mappings,
                &floor,
                risk_level,
            )
            .map_err(|errors| {
                anyhow::anyhow!(
                    "Acceptance Policy selection is incomplete: {}",
                    errors.join(", ")
                )
            })?;
        // A Policy can require full coverage even when the native risk floor is quick.
        // Freeze the full native matrix rather than relabeling quick results as full.
        if selection.requirements.minimum_level == PolicyLevel::Full {
            selection
                .requirements
                .checks
                .extend(plans.full.iter().map(|check| check.id.clone()));
            selection.requirements.checks.sort();
            selection.requirements.checks.dedup();
        }
        let required_checks = selection
            .requirements
            .checks
            .iter()
            .map(|id| {
                let mut matches = profile
                    .recommended_checks
                    .iter()
                    .filter(|check| &check.id == id);
                let check = matches
                    .next()
                    .context("selected Policy check is unavailable")?;
                if matches.next().is_some() {
                    bail!("selected Policy check is ambiguous");
                }
                let binding = verification_check_binding(check);
                if let Some(frozen) = snapshot.checks.iter().find(|check| &check.binding.id == id) {
                    if frozen.binding != binding {
                        bail!("selected Policy check definition has changed");
                    }
                }
                Ok(binding)
            })
            .collect::<Result<Vec<_>>>()?;
        if required_checks.is_empty() || required_checks.len() > MAX_VERIFICATION_CHECKS {
            bail!("selected Policy check matrix is empty or exceeds its bound");
        }
        Ok(Some(NativePolicySelection {
            record,
            snapshot,
            selection,
            risk_level,
            risk,
            required_checks,
        }))
    }
}

pub(super) fn candidate_review(workspace_id: &str, git: &GitChangeSnapshot) -> ChangeReviewReport {
    let paths = git
        .changes
        .iter()
        .flat_map(|change| change.old_path.iter().chain(change.new_path.iter()))
        .cloned()
        .collect::<BTreeSet<_>>();
    let files = paths
        .into_iter()
        .map(|path| {
            let documentation =
                path.ends_with(".md") || path.ends_with(".rst") || path.starts_with("docs/");
            let test =
                path.starts_with("tests/") || path.contains("/tests/") || path.contains("test");
            ChangedFileReview {
                path,
                status: "changed".into(),
                staged: false,
                unstaged: false,
                untracked: false,
                category: if documentation {
                    "documentation"
                } else if test {
                    "test"
                } else {
                    "source"
                }
                .into(),
                additions: None,
                deletions: None,
                binary: false,
                risk_reasons: Vec::new(),
            }
        })
        .collect::<Vec<_>>();
    let docs_only = !files.is_empty()
        && files.iter().all(|file| file.category == "documentation")
        && git.executable_changes == Some(false)
        && git.regular_file_changes == Some(true);
    ChangeReviewReport {
        workspace: workspace_id.into(),
        execution: "git-candidate-metadata".into(),
        clean: git.binding.as_ref().is_some_and(|binding| !binding.dirty),
        files_changed: files.len(),
        staged_files: 0,
        unstaged_files: 0,
        untracked_files: 0,
        additions: 0,
        deletions: 0,
        binary_files: 0,
        source_changed: files.iter().any(|file| file.category == "source"),
        tests_changed: files.iter().any(|file| file.category == "test"),
        docs_only,
        risk_level: "unknown".into(),
        recommended_verification: "quick".into(),
        recommended_checks: Vec::new(),
        summary: "Native Git candidate paths; line counts and source diffs are not captured here."
            .into(),
        files,
        findings: Vec::new(),
        probes: Vec::new(),
        truncated: !git.complete,
    }
}
pub(super) fn policy_authority_fingerprint(
    workspace: &Workspace,
    workspace_id: &str,
) -> Result<String> {
    let Some(record) = policy_store::load(workspace, workspace_id)? else {
        return Ok("none".into());
    };
    let now = u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis(),
    )?;
    Ok(format!(
        "{}:active={}",
        record.digest()?,
        record.is_active_at(now)
    ))
}

pub(super) fn ensure_policy_authority(
    workspace: &Workspace,
    workspace_id: &str,
    expected: &str,
) -> Result<()> {
    if policy_authority_fingerprint(workspace, workspace_id)? != expected {
        bail!("Acceptance Policy authority changed during verification; results are stale, no evidence was recorded");
    }
    Ok(())
}
