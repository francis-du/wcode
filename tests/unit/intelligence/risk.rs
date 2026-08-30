use super::*;

#[test]
fn verification_profiles_scale_with_risk() {
    let low = VerificationProfile::for_risk(RiskLevel::Low);
    let critical = VerificationProfile::for_risk(RiskLevel::Critical);
    assert!(critical.independent_reviewers > low.independent_reviewers);
    assert!(!low.require_fuzz);
    assert!(critical.require_fuzz);
    assert!(critical.require_human_approval);
}

#[test]
fn bounded_risk_metadata_validates() {
    let risk = Risk {
        id: "RISK-SEC-001".into(),
        subject: "component:workspace-security".into(),
        category: RiskCategory::Security,
        level: RiskLevel::Critical,
        summary: "Workspace escape would cross the primary trust boundary.".into(),
        signals: vec!["filesystem-boundary".into()],
        guards: vec!["CONSTRAINT-ROOT-ISOLATION".into()],
    };
    assert_eq!(risk.validate(), Ok(()));
}
