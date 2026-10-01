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
            required_checks: None,
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
fn capacity_reclamation_uses_creation_order_not_random_plan_id() {
    let mut state = VerificationState::default();
    let mut oldest_plan = String::new();
    let mut newest_plan = String::new();

    for index in 0..MAX_VERIFICATION_JOBS {
        let suffix = MAX_VERIFICATION_JOBS - index;
        let plan_id = format!("VP-stale-{suffix:03}");
        if index == 0 {
            oldest_plan = plan_id.clone();
        }
        newest_plan = plan_id.clone();
        state
            .create_plan(
                plan_id,
                "demo".into(),
                format!("change:stale-{index}"),
                VerificationPlanBinding {
                    required_checks: None,
                    revision: Revision {
                        design: None,
                        code: format!("sha256:stale-{index}"),
                    },
                    stage_targets: vec![],
                    automation_gaps: vec![],
                },
                RiskLevel::Low,
                [format!("VJ-stale-{index}")].into_iter(),
            )
            .unwrap();
    }

    state
        .create_plan(
            "VP-current".into(),
            "demo".into(),
            "change:current".into(),
            VerificationPlanBinding {
                required_checks: None,
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

    assert!(!state.plans.contains_key(&oldest_plan));
    assert!(state.plans.contains_key(&newest_plan));
    assert!(state.plans.contains_key("VP-current"));
}

#[test]
fn recent_plan_order_is_creation_bound_even_after_more_than_twenty_random_ids() {
    let mut state = VerificationState::default();
    for index in 0..25 {
        let suffix = 99 - index;
        state
            .create_plan(
                format!("VP-{suffix:02}"),
                "demo".into(),
                "change:current".into(),
                VerificationPlanBinding {
                    required_checks: None,
                    revision: Revision {
                        design: Some("sha256:design".into()),
                        code: "sha256:current".into(),
                    },
                    stage_targets: vec![],
                    automation_gaps: vec![],
                },
                RiskLevel::Low,
                [format!("VJ-{suffix:02}")].into_iter(),
            )
            .unwrap();
    }

    let mut recent = state.plans_for_workspace("demo");
    recent.reverse();
    let recent = recent.into_iter().take(20).collect::<Vec<_>>();
    assert_eq!(recent.len(), 20);
    assert_eq!(recent[0].id, "VP-75");
    assert!(recent.iter().any(|plan| plan.id == "VP-75"));

    let snapshot = state.workspace_snapshot("demo");
    let encoded = serde_json::to_vec(&snapshot).unwrap();
    let decoded: VerificationState = serde_json::from_slice(&encoded).unwrap();
    let mut restored = VerificationState::default();
    restored.restore_workspace(decoded).unwrap();
    assert_eq!(
        restored.plans_for_workspace("demo").last().unwrap().id,
        "VP-75"
    );

    let mut legacy = serde_json::to_value(snapshot).unwrap();
    legacy.as_object_mut().unwrap().remove("plan_order");
    let legacy: VerificationState = serde_json::from_value(legacy).unwrap();
    let mut restored_legacy = VerificationState::default();
    restored_legacy.restore_workspace(legacy).unwrap();
    assert_eq!(restored_legacy.plans_for_workspace("demo").len(), 25);
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
                required_checks: None,
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
                required_checks: None,
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
                required_checks: None,
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
                required_checks: None,
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
            required_checks: None,
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
                required_checks: None,
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
                required_checks: None,
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
            required_checks: None,
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
                required_checks: None,
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

fn generation_plan(
    state: &mut VerificationState,
    workspace: &str,
    suffix: &str,
) -> Result<VerificationPlan, VerificationError> {
    state.create_plan(
        format!("VP-{workspace}-{suffix}"),
        workspace.into(),
        format!("change:{suffix}"),
        VerificationPlanBinding {
            required_checks: None,
            revision: Revision {
                code: format!("sha256:{suffix}"),
                design: None,
            },
            stage_targets: vec![],
            automation_gaps: vec![],
        },
        RiskLevel::Low,
        [format!("VJ-{workspace}-{suffix}")].into_iter(),
    )
}

fn generation_submission() -> ReviewSubmission {
    ReviewSubmission {
        verdict: ReviewVerdict::Pass,
        summary: "Independent review completed.".into(),
        claims: vec![],
        risks: vec![],
        model: None,
    }
}

#[test]
fn persistence_generation_increments_only_successful_mutations() {
    let mut state = VerificationState::default();
    assert_eq!(state.persistence_generation(), 0);
    let plan = generation_plan(&mut state, "demo", "first").unwrap();
    assert_eq!(state.persistence_generation(), 1);
    let snapshot = state.workspace_snapshot("demo");
    assert_eq!(snapshot.persistence_generation(), 1);
    assert_eq!(serde_json::to_value(&snapshot).unwrap()["generation"], 1);
    state.status(&plan.id).unwrap();
    state.plans_for_workspace("demo");
    state.latest_plan_for_workspace_revision("demo", plan.revision.as_ref().unwrap());
    state.workspace_snapshot("missing");
    assert_eq!(
        state.persistence_generation(),
        1,
        "read-only views never advance ordering"
    );

    let before = serde_json::to_value(&state).unwrap();
    assert!(matches!(
        generation_plan(&mut state, "demo", "first"),
        Err(VerificationError::DuplicatePlan)
    ));
    assert!(matches!(
        state.claim("demo", "", &BTreeSet::new(), None),
        Err(VerificationError::InvalidReviewer)
    ));
    assert_eq!(serde_json::to_value(&state).unwrap(), before);
    let capabilities = BTreeSet::from(["correctness_review".into()]);
    let job = state
        .claim("demo", "reviewer", &capabilities, None)
        .unwrap();
    assert_eq!(state.persistence_generation(), 2);
    let before = serde_json::to_value(&state).unwrap();
    assert!(matches!(
        state.claim("demo", "reviewer", &capabilities, None),
        Err(VerificationError::NoMatchingJob)
    ));
    assert!(matches!(
        state.submit("demo", &job.id, "other-reviewer", generation_submission()),
        Err(VerificationError::InvalidJobState)
    ));
    let mut invalid = generation_submission();
    invalid.summary.clear();
    assert!(matches!(
        state.submit("demo", &job.id, "reviewer", invalid),
        Err(VerificationError::InvalidSubmission)
    ));
    assert_eq!(serde_json::to_value(&state).unwrap(), before);
    state
        .submit("demo", &job.id, "reviewer", generation_submission())
        .unwrap();
    assert_eq!(state.persistence_generation(), 3);
    let before = serde_json::to_value(&state).unwrap();
    assert!(state
        .submit("demo", &job.id, "reviewer", generation_submission())
        .is_err());
    assert_eq!(serde_json::to_value(&state).unwrap(), before);
}

#[test]
fn persistence_generation_survives_scoped_snapshots_and_legacy_restore() {
    let mut state = VerificationState::default();
    generation_plan(&mut state, "first", "one").unwrap();
    generation_plan(&mut state, "second", "two").unwrap();
    let first = state.workspace_snapshot("first");
    let second = state.workspace_snapshot("second");
    assert_eq!(first.persistence_generation(), 2);
    assert_eq!(second.persistence_generation(), 2);
    assert_eq!(first.plans.len(), 1);
    let mut legacy = serde_json::to_value(first).unwrap();
    legacy.as_object_mut().unwrap().remove("generation");
    let legacy: VerificationState = serde_json::from_value(legacy).unwrap();
    assert_eq!(legacy.persistence_generation(), 0);
    let mut restored = VerificationState::default();
    restored.restore_workspace(second).unwrap();
    restored.restore_workspace(legacy).unwrap();
    assert_eq!(
        restored.persistence_generation(),
        2,
        "legacy restore cannot lower generation"
    );
    assert_eq!(restored.plans.len(), 2);
    generation_plan(&mut restored, "first", "three").unwrap();
    assert_eq!(restored.persistence_generation(), 3);
    let higher = VerificationState {
        generation: 10,
        ..VerificationState::default()
    };
    restored.restore_workspace(higher).unwrap();
    assert_eq!(restored.persistence_generation(), 10);
    let before = serde_json::to_value(&restored).unwrap();
    let invalid = VerificationState {
        generation: 99,
        plan_order: vec!["VP-missing".into()],
        ..VerificationState::default()
    };
    assert!(matches!(
        restored.restore_workspace(invalid),
        Err(VerificationError::InvalidPersistedState)
    ));
    assert_eq!(
        serde_json::to_value(&restored).unwrap(),
        before,
        "invalid restore is atomic"
    );
}

#[test]
fn persistence_generation_overflow_rejects_before_reclamation_or_job_changes() {
    let capabilities = BTreeSet::from(["correctness_review".into()]);
    for mutation in ["create", "claim", "submit"] {
        let mut state = VerificationState::default();
        let job_id = if mutation == "create" {
            seed_verification_jobs(
                &mut state,
                "demo",
                "stale",
                VerificationJobStatus::Queued,
                MAX_VERIFICATION_JOBS,
            );
            String::new()
        } else {
            let plan = generation_plan(&mut state, "demo", "current").unwrap();
            if mutation == "submit" {
                state
                    .claim("demo", "reviewer", &capabilities, None)
                    .unwrap();
            }
            plan.job_ids[0].clone()
        };
        state.generation = u64::MAX;
        let before = serde_json::to_value(&state).unwrap();
        let error = match mutation {
            "create" => generation_plan(&mut state, "demo", "new").unwrap_err(),
            "claim" => state
                .claim("demo", "reviewer", &capabilities, None)
                .unwrap_err(),
            "submit" => state
                .submit("demo", &job_id, "reviewer", generation_submission())
                .unwrap_err(),
            _ => unreachable!(),
        };
        assert_eq!(error, VerificationError::GenerationExhausted, "{mutation}");
        assert_eq!(
            serde_json::to_value(&state).unwrap(),
            before,
            "{mutation} partially mutated"
        );
    }
    let mut state = VerificationState {
        generation: u64::MAX - 1,
        ..VerificationState::default()
    };
    generation_plan(&mut state, "demo", "last").unwrap();
    assert_eq!(state.persistence_generation(), u64::MAX);
    assert!(matches!(
        generation_plan(&mut state, "demo", "overflow"),
        Err(VerificationError::GenerationExhausted)
    ));
}
