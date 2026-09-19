use crate::risk::{RiskLevel, VerificationProfile};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionEffect {
    Read,
    FileMutation,
    CommandMutation,
}

impl ExecutionEffect {
    pub(crate) fn mutates_workspace(self) -> bool {
        matches!(self, Self::FileMutation | Self::CommandMutation)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VerificationFloor {
    Quick,
    Full,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct ExecutionPolicyInput {
    pub effect: ExecutionEffect,
    pub phase: Option<String>,
    pub risk: RiskLevel,
    #[serde(default)]
    pub product_scopes: Vec<String>,
    #[serde(default)]
    pub writer_lease_active: bool,
    #[serde(default)]
    pub reconciliation_bound: bool,
    #[serde(default)]
    pub existing_review_required: bool,
    #[serde(default)]
    pub existing_verification_floor: Option<VerificationFloor>,
    #[serde(default)]
    pub existing_human_approval_required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ExecutionPolicyDecision {
    pub effect: ExecutionEffect,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    pub risk: RiskLevel,
    pub product_scopes: Vec<String>,
    pub require_writer_lease: bool,
    pub require_review: bool,
    pub verification_floor: VerificationFloor,
    pub require_plan_approval: bool,
    pub require_human_approval: bool,
    pub deterministic_checks: Vec<String>,
    pub independent_reviewers: usize,
    pub require_property: bool,
    pub require_mutation: bool,
    pub require_fuzz: bool,
    pub reasons: Vec<&'static str>,
    pub executable_hooks: bool,
}

pub(crate) fn evaluate(input: ExecutionPolicyInput) -> ExecutionPolicyDecision {
    let profile = VerificationProfile::for_risk(input.risk);
    let risk_floor = if input.risk >= RiskLevel::Medium {
        VerificationFloor::Full
    } else {
        VerificationFloor::Quick
    };
    let verification_floor = input
        .existing_verification_floor
        .map_or(risk_floor, |existing| existing.max(risk_floor));
    let mut product_scopes = crate::scopes::canonicalize(&input.product_scopes);
    product_scopes.sort();
    product_scopes.dedup();

    let mut reasons = Vec::new();
    let mutating = input.effect.mutates_workspace();
    let require_writer_lease = mutating && input.writer_lease_active;
    if require_writer_lease {
        reasons.push("active_writer_lease");
    }
    let require_review = input.existing_review_required || mutating;
    if require_review {
        reasons.push("mutation_requires_review");
    }
    if verification_floor == VerificationFloor::Full {
        reasons.push("risk_requires_full_verification");
    }
    let require_plan_approval = mutating && input.reconciliation_bound;
    if require_plan_approval {
        reasons.push("bound_reconciliation_requires_approved_plan");
    }
    let require_human_approval =
        input.existing_human_approval_required || profile.require_human_approval;
    if require_human_approval {
        reasons.push("human_approval_required");
    }

    ExecutionPolicyDecision {
        effect: input.effect,
        phase: input.phase,
        risk: input.risk,
        product_scopes,
        require_writer_lease,
        require_review,
        verification_floor,
        require_plan_approval,
        require_human_approval,
        deterministic_checks: profile.deterministic_checks,
        independent_reviewers: profile.independent_reviewers,
        require_property: profile.require_property,
        require_mutation: profile.require_mutation,
        require_fuzz: profile.require_fuzz,
        reasons,
        executable_hooks: false,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/runtime/execution_policy.rs"]
mod tests;
