use super::*;

fn seed_verification_jobs(
    state: &mut VerificationState,
    workspace: &str,
    revision: &str,
    status: VerificationJobStatus,
    count: usize,
) -> String {
    let plan_id = format!("VP-seed-{workspace}-{revision}");
    let job_ids = (0..count)
        .map(|index| format!("VJ-seed-{workspace}-{revision}-{index}"))
        .collect::<Vec<_>>();
    state.plans.insert(
        plan_id.clone(),
        VerificationPlan {
            id: plan_id.clone(),
            workspace: workspace.to_owned(),
            subject: format!("change:{revision}"),
            revision: Some(Revision {
                design: None,
                code: revision.to_owned(),
            }),
            risk_level: RiskLevel::Low,
            policy: "test".into(),
            deterministic_level: "quick".into(),
            deterministic_checks: vec![],
            reviewer_roles: vec![ReviewerRole::Correctness],
            require_property: false,
            require_mutation: false,
            require_fuzz: false,
            require_human_approval: false,
            stage_targets: vec![],
            automation_gaps: vec![],
            job_ids: job_ids.clone(),
        },
    );
    for id in job_ids {
        state.jobs.insert(
            id.clone(),
            VerificationJob {
                id,
                plan_id: plan_id.clone(),
                workspace: workspace.to_owned(),
                subject: format!("change:{revision}"),
                role: ReviewerRole::Correctness,
                required_capabilities: vec!["correctness_review".into()],
                guidance: vec![],
                blind: true,
                status,
                claimed_by: (status == VerificationJobStatus::Claimed)
                    .then(|| "reviewer-busy".to_owned()),
                submission: None,
            },
        );
    }
    plan_id
}

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
fn adversarial_jobs_carry_falsification_first_rubric() {
    let mut state = VerificationState::default();
    state
        .create_plan(
            "VP-adversarial".into(),
            "demo".into(),
            "change:adversarial".into(),
            VerificationPlanBinding {
                revision: Revision {
                    design: Some("sha256:design".into()),
                    code: "sha256:adversarial".into(),
                },
                stage_targets: vec!["language:rust".into()],
                automation_gaps: vec![],
            },
            RiskLevel::High,
            [
                "VJ-correctness".into(),
                "VJ-maintainability".into(),
                "VJ-architecture".into(),
                "VJ-security".into(),
                "VJ-adversarial".into(),
            ]
            .into_iter(),
        )
        .unwrap();
    let capabilities = BTreeSet::from(["adversarial_review".to_owned()]);
    let job = state
        .claim(
            "demo",
            "reviewer-adversarial",
            &capabilities,
            Some(ReviewerRole::Adversarial),
        )
        .unwrap();
    assert!(job.guidance.iter().any(|item| item.contains("falsify")));
    assert!(job
        .guidance
        .iter()
        .any(|item| item.contains("majority-vote")));
    assert!(job
        .guidance
        .iter()
        .any(|item| item.contains("revision-bound evidence")));
    assert!(job
        .guidance
        .iter()
        .any(|item| item.contains("counterfactual")));
}

#[test]
fn create_plan_reclaims_superseded_unclaimed_jobs_only_under_capacity_pressure() {
    let mut state = VerificationState::default();
    let stale_plan = seed_verification_jobs(
        &mut state,
        "demo",
        "sha256:stale",
        VerificationJobStatus::Queued,
        MAX_VERIFICATION_JOBS,
    );
    let plan = state
        .create_plan(
            "VP-current".into(),
            "demo".into(),
            "change:current".into(),
            VerificationPlanBinding {
                revision: Revision {
                    design: None,
                    code: "sha256:current".into(),
                },
                stage_targets: vec![],
                automation_gaps: vec![],
            },
            RiskLevel::Low,
            ["VJ-current".into()].into_iter(),
        )
        .unwrap();
    assert_eq!(plan.job_ids, vec!["VJ-current"]);
    assert!(!state.plans.contains_key(&stale_plan));
    assert_eq!(state.jobs.len(), 1);
}

#[test]
fn capacity_failure_preserves_all_existing_verification_state() {
    let mut state = VerificationState::default();
    seed_verification_jobs(
        &mut state,
        "demo",
        "sha256:stale-claimed",
        VerificationJobStatus::Claimed,
        MAX_VERIFICATION_JOBS - 1,
    );
    seed_verification_jobs(
        &mut state,
        "demo",
        "sha256:stale-queued",
        VerificationJobStatus::Queued,
        1,
    );
    let before = serde_json::to_value(&state).unwrap();
    let result = state.create_plan(
        "VP-blocked".into(),
        "demo".into(),
        "change:blocked".into(),
        VerificationPlanBinding {
            revision: Revision {
                design: None,
                code: "sha256:current".into(),
            },
            stage_targets: vec![],
            automation_gaps: vec![],
        },
        RiskLevel::Medium,
        ["VJ-correctness".into(), "VJ-maintainability".into()].into_iter(),
    );
    assert!(matches!(result, Err(VerificationError::CapacityExceeded)));
    assert!(serde_json::to_value(&state).unwrap() == before);
}

#[test]
fn capacity_reclamation_preserves_current_revision_and_unpressured_history() {
    for (revision, count, succeeds) in [
        ("sha256:current", MAX_VERIFICATION_JOBS, false),
        ("sha256:stale", MAX_VERIFICATION_JOBS - 1, true),
    ] {
        let mut state = VerificationState::default();
        let retained_plan = seed_verification_jobs(
            &mut state,
            "demo",
            revision,
            VerificationJobStatus::Queued,
            count,
        );
        let result = state.create_plan(
            "VP-current".into(),
            "demo".into(),
            "change:current".into(),
            VerificationPlanBinding {
                revision: Revision {
                    design: None,
                    code: "sha256:current".into(),
                },
                stage_targets: vec![],
                automation_gaps: vec![],
            },
            RiskLevel::Low,
            ["VJ-current".into()].into_iter(),
        );
        assert_eq!(result.is_ok(), succeeds);
        assert!(state.plans.contains_key(&retained_plan));
        assert_eq!(state.jobs.len(), count + usize::from(succeeds));
    }
}

#[test]
fn verification_capacity_is_scoped_per_workspace() {
    let mut state = VerificationState::default();
    seed_verification_jobs(
        &mut state,
        "other",
        "sha256:other",
        VerificationJobStatus::Queued,
        MAX_VERIFICATION_JOBS,
    );
    state
        .create_plan(
            "VP-demo".into(),
            "demo".into(),
            "change:demo".into(),
            VerificationPlanBinding {
                revision: Revision {
                    design: None,
                    code: "sha256:demo".into(),
                },
                stage_targets: vec![],
                automation_gaps: vec![],
            },
            RiskLevel::Low,
            ["VJ-demo".into()].into_iter(),
        )
        .unwrap();
    assert_eq!(
        state.workspace_snapshot("other").jobs.len(),
        MAX_VERIFICATION_JOBS
    );
    assert_eq!(state.workspace_snapshot("demo").jobs.len(), 1);
}

#[test]
fn create_plan_never_reclaims_claimed_superseded_jobs() {
    let mut state = VerificationState::default();
    let stale_plan = seed_verification_jobs(
        &mut state,
        "demo",
        "sha256:stale-claimed",
        VerificationJobStatus::Claimed,
        MAX_VERIFICATION_JOBS,
    );
    let result = state.create_plan(
        "VP-blocked".into(),
        "demo".into(),
        "change:blocked".into(),
        VerificationPlanBinding {
            revision: Revision {
                design: None,
                code: "sha256:current".into(),
            },
            stage_targets: vec![],
            automation_gaps: vec![],
        },
        RiskLevel::Low,
        ["VJ-blocked".into()].into_iter(),
    );
    assert!(matches!(result, Err(VerificationError::CapacityExceeded)));
    assert!(state.plans.contains_key(&stale_plan));
    assert_eq!(state.jobs.len(), MAX_VERIFICATION_JOBS);
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
