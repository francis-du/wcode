use super::*;

#[test]
fn verification_targets_follow_changed_source_languages() {
    let review = ChangeReviewReport {
        workspace: "demo".into(),
        execution: "fixture".into(),
        clean: false,
        files_changed: 3,
        staged_files: 0,
        unstaged_files: 3,
        untracked_files: 0,
        additions: 3,
        deletions: 0,
        binary_files: 0,
        source_changed: true,
        tests_changed: false,
        docs_only: false,
        risk_level: "moderate".into(),
        recommended_verification: "full".into(),
        recommended_checks: vec![],
        summary: "fixture".into(),
        files: vec![
            crate::harness::ChangedFileReview {
                path: "src/lib.rs".into(),
                status: "modified".into(),
                staged: false,
                unstaged: true,
                untracked: false,
                binary: false,
                category: "source".into(),
                additions: Some(1),
                deletions: Some(0),
                risk_reasons: vec![],
            },
            crate::harness::ChangedFileReview {
                path: "src/ui/app.js".into(),
                status: "modified".into(),
                staged: false,
                unstaged: true,
                untracked: false,
                binary: false,
                category: "source".into(),
                additions: Some(1),
                deletions: Some(0),
                risk_reasons: vec![],
            },
            crate::harness::ChangedFileReview {
                path: "README.md".into(),
                status: "modified".into(),
                staged: false,
                unstaged: true,
                untracked: false,
                binary: false,
                category: "docs".into(),
                additions: Some(1),
                deletions: Some(0),
                risk_reasons: vec![],
            },
        ],
        findings: vec![],
        probes: vec![],
        truncated: false,
    };
    let registry = StageExecutorRegistry {
        configured: false,
        config_path: ".wcode/executors.yaml",
        detected_languages: vec![
            crate::semantic_provider::SemanticLanguage::Css,
            crate::semantic_provider::SemanticLanguage::Html,
            crate::semantic_provider::SemanticLanguage::Rust,
        ],
        executors: vec![],
        coverage: BTreeMap::new(),
        universal_config: true,
    };

    assert_eq!(
        verification_targets_for_review(&review, &registry),
        vec!["language:javascript", "language:rust"]
    );
}

#[test]
fn required_stage_without_local_executor_is_an_explicit_verification_gap() {
    let profile = VerificationProfile::for_risk(RiskLevel::Medium);
    let registry = StageExecutorRegistry {
        configured: false,
        config_path: ".wcode/executors.yaml",
        detected_languages: vec![crate::semantic_provider::SemanticLanguage::Rust],
        executors: Vec::new(),
        coverage: BTreeMap::new(),
        universal_config: true,
    };
    let targets = vec![crate::stage_executor::language_target(
        crate::semantic_provider::SemanticLanguage::Rust,
    )];
    let mut risks = Vec::new();
    append_verification_automation_gap("demo", &profile, &registry, &targets, &mut risks);
    assert_eq!(risks.len(), 1);
    assert_eq!(risks[0].category, RiskCategory::VerificationGap);
    assert_eq!(risks[0].level, RiskLevel::Medium);
    assert!(risks[0]
        .signals
        .contains(&"missing-local-stage-target:property:language:rust".to_owned()));
    assert!(risks[0]
        .signals
        .contains(&"missing-local-stage-target:mutation:language:rust".to_owned()));
    assert!(risks[0]
        .summary
        .contains("External stage evidence remains admissible"));
}

#[test]
fn low_risk_verification_becomes_ready_after_deterministic_and_blind_review_pass() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let plan = runtime
        .create_plan_for_risk("demo", &workspace, RiskLevel::Low)
        .unwrap();
    assert!(plan.automation_gaps.is_empty());
    let job = runtime
        .verification_claim(
            "demo",
            &workspace,
            "reviewer-a",
            &["correctness_review".to_owned()],
            Some(ReviewerRole::Correctness),
        )
        .unwrap();
    runtime
        .verification_submit(
            "demo",
            &workspace,
            &job.id,
            "reviewer-a",
            ReviewSubmission {
                verdict: crate::verification::ReviewVerdict::Pass,
                summary: "Correctness review passed.".into(),
                claims: vec![],
                risks: vec![],
                model: Some("provider/model".into()),
            },
        )
        .unwrap();
    runtime
        .record_verification_report(
            "demo",
            &workspace,
            &VerificationReport {
                workspace: "demo".into(),
                level: "quick".into(),
                execution: "fixture".into(),
                phases_run: 1,
                passed: true,
                checks_run: 1,
                checks_failed: 0,
                elapsed_ms: 1,
                summary: "fixture passed".into(),
                checks: vec![crate::harness::VerificationCheck {
                    id: "rust-check".into(),
                    phase: 0,
                    command: "cargo check --locked".into(),
                    reason: "fixture".into(),
                    success: true,
                    exit_code: Some(0),
                    elapsed_ms: 1,
                    stdout_tail: String::new(),
                    stderr_tail: String::new(),
                    output_truncated: false,
                }],
            },
        )
        .unwrap();
    let status = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert_eq!(status.deterministic_result, Some(EvidenceResult::Pass));
    assert!(status.ready, "{:?}", status.blockers);
    assert!(status.blockers.is_empty());
}

#[test]
fn design_revision_change_invalidates_plan_and_evidence() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".wcode/design")).unwrap();
    fs::write(
        dir.path().join(".wcode/design/product.yaml"),
        "schema_version: 1\nid: product:demo\nname: Demo\nvision: Initial contract\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let plan = runtime
        .create_plan_for_risk("demo", &workspace, RiskLevel::Low)
        .unwrap();
    let captured_revision = plan.revision.as_ref().expect("new plans bind a revision");
    assert!(captured_revision.design.is_some());

    let job = runtime
        .verification_claim(
            "demo",
            &workspace,
            "reviewer-contract",
            &["correctness_review".to_owned()],
            Some(ReviewerRole::Correctness),
        )
        .unwrap();
    runtime
        .verification_submit(
            "demo",
            &workspace,
            &job.id,
            "reviewer-contract",
            ReviewSubmission {
                verdict: crate::verification::ReviewVerdict::Pass,
                summary: "Active contract is satisfied.".into(),
                claims: vec![],
                risks: vec![],
                model: Some("provider/model".into()),
            },
        )
        .unwrap();
    runtime
        .record_verification_report(
            "demo",
            &workspace,
            &VerificationReport {
                workspace: "demo".into(),
                level: "quick".into(),
                execution: "fixture".into(),
                phases_run: 1,
                passed: true,
                checks_run: 1,
                checks_failed: 0,
                elapsed_ms: 1,
                summary: "fixture passed".into(),
                checks: vec![crate::harness::VerificationCheck {
                    id: "rust-check".into(),
                    phase: 0,
                    command: "cargo check --locked".into(),
                    reason: "fixture".into(),
                    success: true,
                    exit_code: Some(0),
                    elapsed_ms: 1,
                    stdout_tail: String::new(),
                    stderr_tail: String::new(),
                    output_truncated: false,
                }],
            },
        )
        .unwrap();
    let before = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert!(before.ready, "{:?}", before.blockers);

    fs::write(
        dir.path().join(".wcode/design/product.yaml"),
        "schema_version: 1\nid: product:demo\nname: Demo\nvision: Revised active contract\n",
    )
    .unwrap();
    let after = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert_eq!(
        after.plan.subject, plan.subject,
        "code revision did not change"
    );
    assert!(after
        .blockers
        .contains(&"design-revision-changed-since-plan".to_owned()));
    assert_eq!(
        after.deterministic_result,
        Some(EvidenceResult::Pass),
        "historical proof remains attributable to the old plan"
    );
    assert!(!after.ready);

    runtime
        .record_verification_report(
            "demo",
            &workspace,
            &VerificationReport {
                workspace: "demo".into(),
                level: "quick".into(),
                execution: "revised-contract-fixture".into(),
                phases_run: 1,
                passed: false,
                checks_run: 1,
                checks_failed: 1,
                elapsed_ms: 1,
                summary: "revised contract fixture failed".into(),
                checks: vec![crate::harness::VerificationCheck {
                    id: "rust-check".into(),
                    phase: 0,
                    command: "cargo check --locked".into(),
                    reason: "revised fixture".into(),
                    success: false,
                    exit_code: Some(1),
                    elapsed_ms: 1,
                    stdout_tail: String::new(),
                    stderr_tail: "fixture failure".into(),
                    output_truncated: false,
                }],
            },
        )
        .unwrap();
    let old_plan = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert_eq!(
        old_plan.deterministic_result,
        Some(EvidenceResult::Pass),
        "new-contract evidence must not contaminate the historical plan"
    );
    let current_plan = runtime
        .create_plan_for_risk("demo", &workspace, RiskLevel::Low)
        .unwrap();
    let current = runtime
        .verification_status("demo", &workspace, &current_plan.id)
        .unwrap();
    assert_eq!(current.deterministic_result, Some(EvidenceResult::Fail));
}

#[test]
fn verification_jobs_resume_in_a_fresh_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let first = SoftwareIntelligenceRuntime::default();
    let plan = first
        .create_plan_for_risk("demo", &workspace, RiskLevel::Low)
        .unwrap();
    drop(first);

    let second = SoftwareIntelligenceRuntime::default();
    let job = second
        .verification_claim(
            "demo",
            &workspace,
            "reviewer-after-restart",
            &["correctness_review".to_owned()],
            Some(ReviewerRole::Correctness),
        )
        .unwrap();
    assert_eq!(job.plan_id, plan.id);
    let status = second
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert_eq!(status.claimed, 1);
}

#[test]
fn required_stage_evidence_replaces_automation_gap_blockers() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let plan = runtime
        .create_plan_for_risk("demo", &workspace, RiskLevel::Medium)
        .unwrap();
    let before = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert!(before
        .blockers
        .iter()
        .any(|blocker| blocker == "property-evidence-missing"));
    assert!(before
        .blockers
        .iter()
        .any(|blocker| blocker == "mutation-evidence-missing"));

    for stage in [VerificationStage::Property, VerificationStage::Mutation] {
        runtime
            .verification_stage_submit(
                "demo",
                &workspace,
                &plan.id,
                StageSubmission {
                    stage,
                    producer: "external-test-runner".into(),
                    verdict: crate::verification::ReviewVerdict::Pass,
                    summary: format!("{stage:?} stage passed."),
                    artifact_digest: format!("sha256:{stage:?}"),
                    targets: plan.stage_targets.clone(),
                    model: None,
                },
            )
            .unwrap();
    }
    let after = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert_eq!(
        after.stage_results.get("property"),
        Some(&EvidenceResult::Pass)
    );
    assert_eq!(
        after.stage_results.get("mutation"),
        Some(&EvidenceResult::Pass)
    );
    assert!(!after
        .blockers
        .iter()
        .any(|blocker| blocker.contains("property-evidence")));
    assert!(!after
        .blockers
        .iter()
        .any(|blocker| blocker.contains("mutation-evidence")));
    assert!(
        !after.ready,
        "review and deterministic verification are still required"
    );
}

#[test]
fn stage_readiness_requires_every_changed_language_target() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let registry = StageExecutorRegistry {
        configured: false,
        config_path: ".wcode/executors.yaml",
        detected_languages: vec![],
        executors: vec![],
        coverage: BTreeMap::new(),
        universal_config: true,
    };
    let plan = runtime
        .create_plan_for_risk_with_targets(
            "demo",
            &workspace,
            RiskLevel::Medium,
            vec!["language:javascript".into(), "language:rust".into()],
            &registry,
        )
        .unwrap();
    assert_eq!(
        plan.stage_targets,
        vec!["language:javascript", "language:rust"]
    );

    runtime
        .verification_stage_submit(
            "demo",
            &workspace,
            &plan.id,
            StageSubmission {
                stage: VerificationStage::Property,
                producer: "rust-property".into(),
                verdict: crate::verification::ReviewVerdict::Pass,
                summary: "Rust property suite passed.".into(),
                artifact_digest: "sha256:rust-property".into(),
                targets: vec!["language:rust".into()],
                model: None,
            },
        )
        .unwrap();
    let partial = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert_eq!(
        partial.stage_target_results["property"]["language:rust"],
        EvidenceResult::Pass
    );
    assert!(!partial.stage_target_results["property"].contains_key("language:javascript"));
    assert!(partial
        .blockers
        .contains(&"property-target-missing:language:javascript".to_owned()));
    assert!(partial
        .blockers
        .contains(&"property-evidence-missing".to_owned()));
    assert!(!partial.stage_results.contains_key("property"));

    runtime
        .verification_stage_submit(
            "demo",
            &workspace,
            &plan.id,
            StageSubmission {
                stage: VerificationStage::Property,
                producer: "js-property".into(),
                verdict: crate::verification::ReviewVerdict::Pass,
                summary: "JavaScript property suite passed.".into(),
                artifact_digest: "sha256:js-property".into(),
                targets: vec!["language:javascript".into()],
                model: None,
            },
        )
        .unwrap();
    let covered = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert_eq!(covered.stage_results["property"], EvidenceResult::Pass);
    assert!(!covered.blockers.iter().any(|blocker| {
        blocker.starts_with("property-target-") || blocker == "property-evidence-missing"
    }));
}

#[test]
fn stage_readiness_aggregates_latest_result_per_producer_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let plan = runtime
        .create_plan_for_risk("demo", &workspace, RiskLevel::Medium)
        .unwrap();

    for (producer, verdict) in [
        (
            "property-runner-a",
            crate::verification::ReviewVerdict::Fail,
        ),
        (
            "property-runner-b",
            crate::verification::ReviewVerdict::Pass,
        ),
    ] {
        runtime
            .verification_stage_submit(
                "demo",
                &workspace,
                &plan.id,
                StageSubmission {
                    stage: VerificationStage::Property,
                    producer: producer.into(),
                    verdict,
                    summary: format!("{producer} returned {verdict:?}"),
                    artifact_digest: format!("sha256:{producer}-{verdict:?}"),
                    targets: plan.stage_targets.clone(),
                    model: None,
                },
            )
            .unwrap();
    }

    let failed = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert_eq!(
        failed.stage_results.get("property"),
        Some(&EvidenceResult::Fail),
        "one producer failure must not be hidden by another producer pass"
    );
    assert_eq!(
        failed.stage_producer_results["property"]["property-runner-a"],
        EvidenceResult::Fail
    );
    assert_eq!(
        failed.stage_producer_results["property"]["property-runner-b"],
        EvidenceResult::Pass
    );
    assert!(failed
        .blockers
        .contains(&"property-evidence-failed".to_owned()));

    runtime
        .verification_stage_submit(
            "demo",
            &workspace,
            &plan.id,
            StageSubmission {
                stage: VerificationStage::Property,
                producer: "property-runner-a".into(),
                verdict: crate::verification::ReviewVerdict::Pass,
                summary: "property-runner-a passed after remediation".into(),
                artifact_digest: "sha256:property-runner-a-remediated".into(),
                targets: plan.stage_targets.clone(),
                model: None,
            },
        )
        .unwrap();
    let remediated = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert_eq!(
        remediated.stage_results.get("property"),
        Some(&EvidenceResult::Pass)
    );
    assert!(!remediated
        .blockers
        .iter()
        .any(|blocker| blocker.starts_with("property-evidence-")));
}

#[tokio::test]
async fn configured_stage_executor_produces_real_persistent_stage_evidence() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn value() -> u8 { 1 }\n").unwrap();
    let workspace = Workspace::new_with_security(
        dir.path(),
        false,
        true,
        WorkspaceSecurity {
            allow_risky_exec: true,
            ..WorkspaceSecurity::default()
        },
    )
    .unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let plan = runtime
        .create_plan_for_risk("demo", &workspace, RiskLevel::Medium)
        .unwrap();
    let execution = execute_stage(
        &workspace,
        &StageExecutorSpec {
            id: "fixture-property".into(),
            stage: VerificationStage::Property,
            languages: vec![crate::semantic_provider::SemanticLanguage::Rust],
            program: "rustc".into(),
            args: vec!["--version".into()],
            cwd: ".".into(),
            timeout_seconds: 10,
            builtin: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(execution.verdict, crate::verification::ReviewVerdict::Pass);
    runtime
        .verification_stage_submit(
            "demo",
            &workspace,
            &plan.id,
            StageSubmission {
                stage: execution.stage,
                producer: format!("executor:{}", execution.executor_id),
                verdict: execution.verdict,
                summary: execution.summary,
                artifact_digest: execution.artifact_digest,
                targets: plan.stage_targets.clone(),
                model: None,
            },
        )
        .unwrap();
    let status = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert_eq!(
        status.stage_results.get("property"),
        Some(&EvidenceResult::Pass)
    );
    let fresh = SoftwareIntelligenceRuntime::default();
    let status = fresh
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert_eq!(
        status.stage_results.get("property"),
        Some(&EvidenceResult::Pass),
        "stage evidence must survive runtime restart"
    );
}

#[test]
fn explicit_human_approval_clears_only_the_human_blocker() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let plan = runtime
        .create_plan_for_risk("demo", &workspace, RiskLevel::Critical)
        .unwrap();
    let before = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert!(before
        .blockers
        .contains(&"human-approval-required".to_owned()));
    runtime
        .verification_approve(
            "demo",
            &workspace,
            &plan.id,
            "operator-a",
            "I reviewed the critical-risk plan and approve proceeding.",
        )
        .unwrap();
    let after = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert!(after.human_approval);
    assert!(!after
        .blockers
        .contains(&"human-approval-required".to_owned()));
    assert!(after
        .blockers
        .iter()
        .any(|blocker| blocker.ends_with("-evidence-missing")));
    assert!(!after.ready);
}

#[test]
fn reviewer_disagreement_is_persisted_as_evidence_once() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let runtime = SoftwareIntelligenceRuntime::default();
    let plan = runtime
        .create_plan_for_risk("demo", &workspace, RiskLevel::Medium)
        .unwrap();
    assert_eq!(plan.job_ids.len(), 2);

    let correctness = runtime
        .verification_claim(
            "demo",
            &workspace,
            "reviewer-a",
            &["correctness_review".to_owned()],
            Some(ReviewerRole::Correctness),
        )
        .unwrap();
    runtime
        .verification_submit(
            "demo",
            &workspace,
            &correctness.id,
            "reviewer-a",
            ReviewSubmission {
                verdict: crate::verification::ReviewVerdict::Pass,
                summary: "No correctness issue found.".into(),
                claims: vec![],
                risks: vec![],
                model: Some("provider/model-a".into()),
            },
        )
        .unwrap();

    let maintainability = runtime
        .verification_claim(
            "demo",
            &workspace,
            "reviewer-b",
            &["maintainability_review".to_owned()],
            Some(ReviewerRole::Maintainability),
        )
        .unwrap();
    runtime
        .verification_submit(
            "demo",
            &workspace,
            &maintainability.id,
            "reviewer-b",
            ReviewSubmission {
                verdict: crate::verification::ReviewVerdict::Fail,
                summary:
                    "The change preserves behavior but adds an avoidable structural regression."
                        .into(),
                claims: vec![],
                risks: vec!["canonical boundary and duplicated branching".into()],
                model: Some("provider/model-b".into()),
            },
        )
        .unwrap();

    let status = runtime
        .verification_status("demo", &workspace, &plan.id)
        .unwrap();
    assert_eq!(status.disagreements, 1);
    let evidence = runtime
        .evidence_status("demo", &workspace, Some(&plan.subject), 100)
        .unwrap();
    assert_eq!(evidence.disagreed, 1);
    assert_eq!(
        evidence
            .evidence
            .iter()
            .filter(|record| record.result == EvidenceResult::Disagree)
            .count(),
        1
    );
}
