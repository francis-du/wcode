use super::*;

#[test]
fn maintainability_jobs_carry_the_structural_review_rubric() {
    let mut state = VerificationState::default();
    let plan = state
        .create_plan(
            "VP-maintainability".into(),
            "demo".into(),
            "change:maintainability".into(),
            RiskLevel::Medium,
            ["VJ-correctness".into(), "VJ-maintainability".into()].into_iter(),
        )
        .unwrap();
    assert!(plan.reviewer_roles.contains(&ReviewerRole::Maintainability));
    let capabilities = BTreeSet::from(["maintainability_review".to_owned()]);
    let job = state
        .claim(
            "demo",
            "reviewer-maintainability",
            &capabilities,
            Some(ReviewerRole::Maintainability),
        )
        .unwrap();
    assert!(job.guidance.iter().any(|item| item.contains("code-judo")));
    assert!(job.guidance.iter().any(|item| item.contains("1,000")));
    assert!(job
        .guidance
        .iter()
        .any(|item| item.contains("canonical layer")));

    let mut invalid = state.workspace_snapshot("demo");
    invalid.jobs.values_mut().next().unwrap().guidance =
        vec!["x".repeat(MAX_REVIEW_GUIDANCE_CHARS + 1)];
    assert!(matches!(
        VerificationState::default().restore_workspace(invalid),
        Err(VerificationError::InvalidPersistedState)
    ));
}

#[test]
fn blind_jobs_are_claimed_by_capability_and_do_not_expose_other_submissions() {
    let mut state = VerificationState::default();
    let plan = state
        .create_plan(
            "VP-1".into(),
            "demo".into(),
            "change:1".into(),
            RiskLevel::Medium,
            ["VJ-1".into(), "VJ-2".into()].into_iter(),
        )
        .unwrap();
    assert_eq!(plan.job_ids.len(), 2);
    assert_eq!(
        plan.reviewer_roles,
        vec![ReviewerRole::Correctness, ReviewerRole::Maintainability]
    );
    let capabilities = BTreeSet::from(["correctness_review".to_owned()]);
    let job = state
        .claim("demo", "reviewer-a", &capabilities, None)
        .unwrap();
    assert!(job.blind);
    assert!(job.submission.is_none());
    state
        .submit(
            "demo",
            &job.id,
            "reviewer-a",
            ReviewSubmission {
                verdict: ReviewVerdict::Pass,
                summary: "No correctness issue found.".into(),
                claims: vec![],
                risks: vec![],
                model: Some("provider/model".into()),
            },
        )
        .unwrap();
    let status = state.status("VP-1").unwrap();
    assert_eq!(status.submitted, 1);
    assert_eq!(status.queued, 1);
}
