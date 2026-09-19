use super::*;

fn input(risk: RiskLevel, effect: ExecutionEffect) -> ExecutionPolicyInput {
    ExecutionPolicyInput {
        effect,
        phase: Some("executing".into()),
        risk,
        product_scopes: vec!["Workspace".into(), "verification".into()],
        writer_lease_active: false,
        reconciliation_bound: false,
        existing_review_required: false,
        existing_verification_floor: None,
        existing_human_approval_required: false,
    }
}

#[test]
fn policy_is_deterministic_and_monotonic_in_risk() {
    let low = evaluate(input(RiskLevel::Low, ExecutionEffect::FileMutation));
    let low_again = evaluate(input(RiskLevel::Low, ExecutionEffect::FileMutation));
    assert_eq!(low, low_again);

    let medium = evaluate(input(RiskLevel::Medium, ExecutionEffect::FileMutation));
    let high = evaluate(input(RiskLevel::High, ExecutionEffect::FileMutation));
    let critical = evaluate(input(RiskLevel::Critical, ExecutionEffect::FileMutation));

    assert_eq!(low.verification_floor, VerificationFloor::Quick);
    assert_eq!(medium.verification_floor, VerificationFloor::Full);
    assert_eq!(high.verification_floor, VerificationFloor::Full);
    assert_eq!(critical.verification_floor, VerificationFloor::Full);
    assert!(medium.independent_reviewers >= low.independent_reviewers);
    assert!(high.independent_reviewers >= medium.independent_reviewers);
    assert!(critical.independent_reviewers >= high.independent_reviewers);
    assert!(!low.require_human_approval);
    assert!(critical.require_human_approval);

    let mut preserved = input(RiskLevel::Low, ExecutionEffect::Read);
    preserved.existing_verification_floor = Some(VerificationFloor::Full);
    preserved.existing_review_required = true;
    preserved.existing_human_approval_required = true;
    let preserved = evaluate(preserved);
    assert_eq!(preserved.verification_floor, VerificationFloor::Full);
    assert!(preserved.require_review);
    assert!(preserved.require_human_approval);
}

#[test]
fn policy_requires_writer_lease_only_for_mutating_effects_when_active() {
    let mut read = input(RiskLevel::Low, ExecutionEffect::Read);
    read.writer_lease_active = true;
    let read = evaluate(read);
    assert!(!read.require_writer_lease);
    assert!(!read.require_review);

    for effect in [
        ExecutionEffect::FileMutation,
        ExecutionEffect::CommandMutation,
    ] {
        let mut mutation = input(RiskLevel::Low, effect);
        mutation.writer_lease_active = true;
        mutation.reconciliation_bound = true;
        let mutation = evaluate(mutation);
        assert!(mutation.require_writer_lease);
        assert!(mutation.require_review);
        assert!(mutation.require_plan_approval);
    }
}

#[test]
fn policy_has_no_repository_executable_hook_surface() {
    let decision = evaluate(input(RiskLevel::Critical, ExecutionEffect::CommandMutation));
    let encoded = serde_json::to_string(&decision).unwrap();
    assert!(encoded.contains("\"executable_hooks\":false"));
    assert!(!encoded.contains("shell_hook"));
    assert!(!encoded.contains("hook_command"));
    assert!(!encoded.contains("repository_script"));
    assert_eq!(
        decision.product_scopes,
        vec!["verification".to_owned(), "workspace".to_owned()]
    );
}
