//! Native plan construction consumes the approved Policy without weakening risk floors.
use super::*;
use crate::design::PolicyRequirements;

pub(crate) struct NativePolicyPlan<'a> {
    pub risk_level: RiskLevel,
    pub requirements: &'a PolicyRequirements,
    pub required_checks: Vec<RequiredVerificationCheck>,
    pub policy_binding: &'a str,
}

impl SoftwareIntelligenceRuntime {
    pub(crate) fn create_policy_verification_plan(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        review: &ChangeReviewReport,
        input: NativePolicyPlan<'_>,
    ) -> Result<VerificationPlan> {
        let NativePolicyPlan {
            risk_level,
            requirements,
            required_checks,
            policy_binding,
        } = input;
        self.ensure_verification_loaded(workspace_id, workspace)?;
        let revision = self.current_revision(workspace)?;
        let registry = stage_executor::registry(workspace)?;
        let stage_targets = verification_targets_for_review(review, &registry, risk_level);
        let mut profile = VerificationProfile::for_risk(risk_level);
        profile.require_property |= requirements.stages.contains(&VerificationStage::Property);
        profile.require_mutation |= requirements.stages.contains(&VerificationStage::Mutation);
        profile.require_fuzz |= requirements.stages.contains(&VerificationStage::Fuzz);
        profile.require_human_approval |= requirements.human_approval;
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
        let automation_gaps = verification_automation_gaps(&profile, &registry, &stage_targets);
        let (plan, snapshot) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            let plan = state.verification.create_plan_with_policy(
                self.next_id("VP"),
                workspace_id.to_owned(),
                format!("change:{}", revision.code),
                VerificationPlanBinding {
                    revision,
                    stage_targets,
                    automation_gaps,
                    required_checks: Some(required_checks),
                },
                crate::verification::PolicyPlanRequirements {
                    risk_level,
                    requirements: requirements.clone(),
                    policy_binding: policy_binding.into(),
                },
                std::iter::repeat_with(|| self.next_id("VJ")),
            )?;
            (plan, state.verification.workspace_snapshot(workspace_id))
        };
        verification_store::persist(workspace, &snapshot)?;
        Ok(plan)
    }
}
