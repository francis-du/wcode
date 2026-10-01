use super::{reviewer_roles, ReviewerRole, VerificationError, VerificationStage};
use crate::design::{PolicyLevel, PolicyRequirements};
use crate::evidence::RequiredVerificationCheck;
use crate::risk::{RiskLevel, VerificationProfile};

#[derive(Clone, Debug)]
pub(crate) struct PolicyPlanRequirements {
    pub risk_level: RiskLevel,
    pub requirements: PolicyRequirements,
    pub policy_binding: String,
}

pub(super) struct PlanConfiguration {
    pub risk_level: RiskLevel,
    pub policy: Option<PolicyPlanRequirements>,
}
pub(super) struct PlanRequirements {
    pub profile: VerificationProfile,
    pub roles: Vec<ReviewerRole>,
    pub deterministic_level: String,
    pub policy_binding: String,
}

pub(super) fn resolve(
    configuration: PlanConfiguration,
    required_checks: Option<&[RequiredVerificationCheck]>,
) -> Result<PlanRequirements, VerificationError> {
    let risk = configuration.risk_level;
    let mut profile = VerificationProfile::for_risk(risk);
    let mut roles = reviewer_roles(&profile);
    let mut full = risk >= RiskLevel::Medium;
    let policy_binding = if let Some(policy) = configuration.policy {
        let requirements = policy.requirements;
        if policy.risk_level != risk
            || policy.policy_binding.trim().is_empty()
            || policy.policy_binding.len() > 256
            || policy.policy_binding.chars().any(char::is_control)
            || requirements.checks.len() > 32
            || requirements.reviewers.len() > 9
            || requirements.stages.len() > 4
            || requirements.checks.iter().any(|id| {
                required_checks.is_none_or(|checks| !checks.iter().any(|check| check.id == *id))
            })
        {
            return Err(VerificationError::InvalidPlan);
        }
        full |= requirements.minimum_level == PolicyLevel::Full;
        for role in requirements.reviewers {
            if !roles.contains(&role) {
                roles.push(role);
            }
        }
        profile.require_property |= requirements.stages.contains(&VerificationStage::Property);
        profile.require_mutation |= requirements.stages.contains(&VerificationStage::Mutation);
        profile.require_fuzz |= requirements.stages.contains(&VerificationStage::Fuzz);
        profile.require_human_approval |= requirements.human_approval
            || requirements
                .human_approval_min_risk
                .is_some_and(|threshold| risk >= threshold);
        if requirements
            .stages
            .contains(&VerificationStage::RuntimeCanary)
            && !profile
                .deterministic_checks
                .iter()
                .any(|check| check == "runtime-gate")
        {
            profile.deterministic_checks.push("runtime-gate".into());
        }
        for role in reviewer_roles(&profile) {
            if !roles.contains(&role) {
                roles.push(role);
            }
        }
        for id in requirements.checks {
            if !profile.deterministic_checks.contains(&id) {
                profile.deterministic_checks.push(id);
            }
        }
        policy.policy_binding
    } else {
        format!("risk-adaptive/v2/{risk:?}").to_ascii_lowercase()
    };
    if roles.len() > 9 || profile.deterministic_checks.len() > 32 {
        return Err(VerificationError::InvalidPlan);
    }
    Ok(PlanRequirements {
        profile,
        roles,
        deterministic_level: if full { "full" } else { "quick" }.into(),
        policy_binding,
    })
}
