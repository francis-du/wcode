use super::*;

#[test]
fn maintainability_jobs_carry_the_structural_review_rubric() {
    let mut state = VerificationState::default();
    let plan = state
        .create_plan(
            "VP-maintainability".into(),
            "demo".into(),
            "change:maintainability".into(),
            VerificationPlanBinding {
                revision: Revision {
                    design: Some("sha256:design".into()),
                    code: "sha256:maintainability".into(),
                },
                stage_targets: vec![],
                automation_gaps: vec![],
            },
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
fn correctness_jobs_carry_contract_first_rubric() {
    let mut state = VerificationState::default();
    state
        .create_plan(
            "VP-correctness".into(),
            "demo".into(),
            "change:correctness".into(),
            VerificationPlanBinding {
                revision: Revision {
                    design: Some("sha256:design".into()),
                    code: "sha256:correctness".into(),
                },
                stage_targets: vec![],
                automation_gaps: vec![],
            },
            RiskLevel::Low,
            ["VJ-correctness".into()].into_iter(),
        )
        .unwrap();
    let capabilities = BTreeSet::from(["correctness_review".to_owned()]);
    let job = state
        .claim(
            "demo",
            "reviewer-correctness",
            &capabilities,
            Some(ReviewerRole::Correctness),
        )
        .unwrap();
    assert!(job
        .guidance
        .iter()
        .any(|item| item.contains("active Design State")));
    assert!(job
        .guidance
        .iter()
        .any(|item| item.contains("Inconclusive")));
    assert!(job
        .guidance
        .iter()
        .any(|item| item.contains("counterexample")));
    assert!(job
        .guidance
        .iter()
        .any(|item| item.contains("counterfactual")));
}

#[test]
fn blind_jobs_are_claimed_by_capability_and_do_not_expose_other_submissions() {
    let mut state = VerificationState::default();
    let plan = state
        .create_plan(
            "VP-1".into(),
            "demo".into(),
            "change:1".into(),
            VerificationPlanBinding {
                revision: Revision {
                    design: None,
                    code: "sha256:1".into(),
                },
                stage_targets: vec![],
                automation_gaps: vec![],
            },
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
