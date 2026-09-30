use super::*;
use crate::evidence::{Confidence, EvidenceKind};
use crate::intelligence_types::ProjectEvidenceView;
use crate::workspace::Workspace;

fn project() -> (tempfile::TempDir, ProjectObservatory) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(root.path().join("src/lib.rs"), "pub fn value() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let project = crate::harness::ToolHarness::new(2)
        .unwrap()
        .project_observatory("demo", &workspace, None)
        .unwrap();
    (root, project)
}

fn failed(subject: &str) -> ProjectEvidenceView {
    ProjectEvidenceView {
        id: format!("evidence:{subject}"),
        subject: subject.into(),
        producer: "compiler".into(),
        policy: Some("full".into()),
        kind: EvidenceKind::Compiler,
        confidence: Confidence::Deterministic,
        result: EvidenceResult::Fail,
        timestamp_ms: 1,
        summary: Some("Compiler reported a failure.".into()),
    }
}

#[test]
fn attention_preserves_revision_and_ranks_current_failure_before_unknowns() {
    let (_root, mut p) = project();
    p.proof.effective.items = vec![failed("check:rust-check")];
    p.proof.effective.failed = 1;
    p.proof.effective.total = 1;
    let view = build(&p, None, &[]);
    assert_eq!(view.revision, p.repository_revision);
    assert_eq!(view.items[0].kind, "evidence_failed");
    assert_eq!(view.items[0].section, "proof");
    assert_eq!(view.items[0].provider, "compiler");
    assert_eq!(view.high, 1);
    assert!(view.partial);
    assert!(view
        .partial_reasons
        .iter()
        .any(|r| r == "change_review_unavailable"));
}

#[test]
fn attention_bounds_rows_without_hiding_raw_severity_counts() {
    let (_root, mut p) = project();
    p.proof.effective.items = (0..100)
        .map(|index| failed(&format!("check:{index}")))
        .collect();
    let view = build(&p, None, &[]);
    assert_eq!(view.items.len(), 64);
    assert_eq!(view.high, 100);
    assert!(view.total >= 100);
    assert!(view.truncated);
    assert_eq!(
        view.total,
        view.critical + view.high + view.medium + view.low + view.info
    );
}

#[test]
fn attention_keeps_disagreement_and_missing_coverage_explicit() {
    let (_root, mut p) = project();
    let mut disagreement = failed("test:fixture");
    disagreement.result = EvidenceResult::Disagree;
    p.proof.effective.items = vec![disagreement];
    p.proof.effective.truncated = true;
    p.structure.truncated = true;
    let view = build(&p, None, &[]);
    assert!(view.items.iter().any(|i| i.kind == "evidence_disagreed"));
    assert!(view.partial_reasons.iter().any(|r| r == "evidence_partial"));
    assert!(view
        .partial_reasons
        .iter()
        .any(|r| r == "source_graph_partial"));
}

#[test]
fn attention_redacts_diagnostics_before_emitting_ids_or_messages() {
    let (_root, mut p) = project();
    let mut failure = failed("Authorization: Bearer secret_fixture_token_12345");
    failure.summary = Some("Authorization: Bearer secret_fixture_token_12345".into());
    p.proof.effective.items = vec![failure];
    let view = serde_json::to_string(&build(&p, None, &[])).unwrap();
    assert!(!view.contains("secret_fixture_token_12345"), "{view}");
}

#[test]
fn attention_preserves_evidence_policy_scope_and_model_precision() {
    let (_root, mut p) = project();
    let full = failed("check:rust-check");
    let mut quick = full.clone();
    quick.policy = Some("quick".into());
    quick.id = "quick-record".into();
    let mut model = full.clone();
    model.id = "model-record".into();
    model.kind = EvidenceKind::ModelReview;
    model.confidence = Confidence::High;
    model.result = EvidenceResult::Disagree;
    p.proof.effective.items = vec![full, quick, model];
    let view = build(&p, None, &[]);
    let failures = view
        .items
        .iter()
        .filter(|item| item.kind == "evidence_failed")
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 2);
    assert_ne!(failures[0].id, failures[1].id);
    assert!(failures.iter().any(|i| i.message.contains("full")));
    assert!(failures.iter().any(|i| i.message.contains("quick")));
    assert_eq!(
        view.items
            .iter()
            .find(|i| i.kind == "evidence_disagreed")
            .unwrap()
            .precision,
        "model_review"
    );
}

#[test]
fn attention_keeps_unlisted_oversized_paths_visible_and_diagnostic_ids_stable() {
    let (_root, mut p) = project();
    p.structure.oversized_files = 5;
    p.structure.largest_files.clear();
    p.coverage
        .diagnostics
        .push(crate::design::DesignDiagnostic {
            severity: crate::design::DiagnosticSeverity::Error,
            code: "missing-reference".into(),
            path: ".wcode/design/requirements.yaml".into(),
            message: "Missing component.".into(),
        });
    let before = build(&p, None, &[]);
    let diagnostic = before
        .items
        .iter()
        .find(|i| i.kind == "design_diagnostic")
        .unwrap()
        .id
        .clone();
    p.proof.effective.items = vec![failed("check:extra")];
    let after = build(&p, None, &[]);
    assert_eq!(
        after
            .items
            .iter()
            .find(|i| i.kind == "design_diagnostic")
            .unwrap()
            .id,
        diagnostic
    );
    let remainder = after
        .items
        .iter()
        .find(|i| i.kind == "oversized_remaining")
        .unwrap();
    assert_eq!(remainder.count, 5);
    assert_eq!(remainder.severity, AttentionSeverity::High);
    assert!(after
        .partial_reasons
        .contains(&"oversized_paths_partial".into()));
}

#[test]
fn attention_latest_plan_does_not_reintroduce_older_blockers() {
    let (_root, p) = project();
    let make = |id: &str, ready: bool| crate::verification::VerificationStatus {
        plan: crate::verification::VerificationPlan {
            id: id.into(),
            workspace: p.workspace.clone(),
            subject: format!("change:{}", p.repository_revision.code),
            revision: Some(p.repository_revision.clone()),
            risk_level: crate::risk::RiskLevel::Low,
            policy: "test".into(),
            deterministic_level: "full".into(),
            deterministic_checks: vec![],
            reviewer_roles: vec![],
            require_property: false,
            require_mutation: false,
            require_fuzz: false,
            require_human_approval: false,
            stage_targets: vec![],
            automation_gaps: vec![],
            job_ids: vec![],
        },
        queued: 0,
        claimed: 0,
        submitted: 0,
        reviewer_failures: 0,
        reviewer_inconclusive: 0,
        disagreements: 0,
        deterministic_result: Some(EvidenceResult::Pass),
        stage_results: Default::default(),
        stage_producer_results: Default::default(),
        stage_target_results: Default::default(),
        human_approval: false,
        ready,
        blockers: if ready {
            vec![]
        } else {
            vec!["old-blocker".into()]
        },
        jobs: vec![],
    };
    // Persisted history is newest-first, not sorted by UUID.
    let view = build(&p, None, &[make("a-new", true), make("z-old", false)]);
    assert!(!view.items.iter().any(|i| i.kind == "verification_blocked"));
}

#[test]
fn attention_includes_high_risk_without_drift_and_routes_provider_gaps() {
    let (_root, mut p) = project();
    p.risk = Some(crate::intelligence_types::RiskStatus {
        workspace: p.workspace.clone(),
        revision: p.repository_revision.clone(),
        level: crate::risk::RiskLevel::High,
        profile: crate::risk::VerificationProfile::for_risk(crate::risk::RiskLevel::High),
        risks: vec![crate::risk::Risk {
            id: "security-risk".into(),
            subject: "Changed dependency".into(),
            category: crate::risk::RiskCategory::Security,
            level: crate::risk::RiskLevel::High,
            summary: "Inspect dependency trust.".into(),
            signals: vec![],
            guards: vec![],
        }],
        bug_patterns: crate::risk::BugPatternStatus {
            precision: "heuristic",
            patterns_scanned: 0,
            matches: 0,
            files: 0,
            findings: vec![],
            truncated: false,
        },
        drift: crate::intelligence_types::DriftStatus {
            workspace: p.workspace.clone(),
            design_changed: false,
            implementation_changed: false,
            implementation_drift: 0,
            design_drift: 0,
            runtime_drift: 0,
            findings: vec![],
            truncated: false,
        },
        traceability: p.coverage.clone(),
    });
    if let Some(language) = p.language_quality.languages.first_mut() {
        language.gaps.push("type checker unavailable".into());
    }
    let view = build(&p, None, &[]);
    assert!(view.items.iter().any(|i| i.kind == "risk"
        && i.severity == AttentionSeverity::High
        && i.precision == "heuristic"));
    assert!(view
        .items
        .iter()
        .filter(|i| i.kind == "provider_gap")
        .all(|i| i.section == "quality"));
}

#[test]
fn attention_selection_identity_survives_unrelated_evidence_insertion() {
    let (_root, mut p) = project();
    p.proof.effective.items = vec![failed("check:selected")];
    let before = build(&p, None, &[]);
    let selected_id = before
        .items
        .iter()
        .find(|i| i.subject == "check:selected")
        .unwrap()
        .id
        .clone();
    p.proof.effective.items.insert(0, failed("check:unrelated"));
    let after = build(&p, None, &[]);
    assert_eq!(
        after
            .items
            .iter()
            .find(|i| i.subject == "check:selected")
            .unwrap()
            .id,
        selected_id
    );
}
