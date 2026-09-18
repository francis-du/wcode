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

#[test]
fn bug_pattern_scan_propagates_a_discovered_pattern_across_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("one.go"),
        "package demo\nvar a = *DriverBizInfo().IsAutoGrab\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("two.go"),
        "package demo\nvar b = *svc.DriverBizInfo().State\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();

    let status = scan_bug_patterns(&workspace).unwrap();
    assert_eq!(status.precision, "heuristic-regex-candidate");
    assert_eq!(status.matches, 2);
    assert_eq!(status.files, 2);
    assert!(status
        .findings
        .iter()
        .all(|finding| finding.pattern == "deref-call-result"));
}
