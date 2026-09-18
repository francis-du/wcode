use super::*;

fn report() -> ChangeReviewReport {
    ChangeReviewReport {
        workspace: "demo".into(),
        execution: "fixture".into(),
        clean: false,
        files_changed: 3,
        staged_files: 0,
        unstaged_files: 3,
        untracked_files: 0,
        additions: 42,
        deletions: 7,
        binary_files: 0,
        source_changed: true,
        tests_changed: true,
        docs_only: false,
        risk_level: "moderate".into(),
        recommended_verification: "full".into(),
        recommended_checks: vec!["rust-test".into()],
        summary: "fixture".into(),
        files: vec![
            ChangedFileReview {
                path: "src/target.rs".into(),
                status: "modified".into(),
                staged: false,
                unstaged: true,
                untracked: false,
                category: "source".into(),
                additions: Some(20),
                deletions: Some(2),
                binary: false,
                risk_reasons: vec![],
            },
            ChangedFileReview {
                path: "tests/target.rs".into(),
                status: "modified".into(),
                staged: false,
                unstaged: true,
                untracked: false,
                category: "test".into(),
                additions: Some(20),
                deletions: Some(5),
                binary: false,
                risk_reasons: vec![],
            },
            ChangedFileReview {
                path: "Cargo.toml".into(),
                status: "modified".into(),
                staged: false,
                unstaged: true,
                untracked: false,
                category: "manifest".into(),
                additions: Some(2),
                deletions: Some(0),
                binary: false,
                risk_reasons: vec![],
            },
        ],
        findings: Vec::new(),
        probes: Vec::new(),
        truncated: false,
    }
}

fn finding(code: &str, severity: &str) -> ReviewFinding {
    ReviewFinding {
        severity: severity.into(),
        code: code.into(),
        message: format!("fixture {code}"),
        paths: vec!["src/target.rs".into()],
    }
}

fn source_file(path: &str) -> ChangedFileReview {
    ChangedFileReview {
        path: path.into(),
        status: "modified".into(),
        staged: false,
        unstaged: true,
        untracked: false,
        category: "source".into(),
        additions: Some(1),
        deletions: Some(0),
        binary: false,
        risk_reasons: vec![],
    }
}

#[test]
fn adversarial_qa_partial_review_is_never_complete() {
    let mut review = report();
    review.truncated = true;
    let packet = build(&review);
    assert!(packet.questions.len() < MAX_ADVERSARIAL_QUESTIONS);
    assert!(
        packet.truncated,
        "partial input must stay partial even below the question limit"
    );
}

#[test]
fn adversarial_qa_security_targets_do_not_follow_alphabetical_fillers() {
    let mut review = report();
    review.files = (0..8)
        .map(|n| source_file(&format!("src/a{n}.rs")))
        .collect();
    review.files.push(source_file("src/target.rs"));
    review
        .findings
        .push(finding("security-sensitive-change", "high"));
    let packet = build(&review);
    let security = packet
        .questions
        .iter()
        .find(|q| q.id == "security-bypass-counterexample")
        .unwrap();
    assert_eq!(
        security.counterexample_experiment.targets,
        vec!["src/target.rs"]
    );
}

#[test]
fn adversarial_qa_targets_are_unique_and_order_independent() {
    let mut review = report();
    review.files = vec![
        source_file("src/z.rs"),
        source_file("src/a.rs"),
        source_file("src/a.rs"),
    ];
    let first = build(&review)
        .questions
        .remove(0)
        .counterexample_experiment
        .targets;
    review.files.reverse();
    let second = build(&review)
        .questions
        .remove(0)
        .counterexample_experiment
        .targets;
    assert_eq!(first, second);
    assert_eq!(first, vec!["src/a.rs", "src/z.rs"]);
}

#[test]
fn adversarial_qa_large_line_counts_do_not_overflow() {
    let mut review = report();
    review.files_changed = 40;
    review.additions = u64::MAX;
    review.deletions = 1;
    assert!(build(&review)
        .questions
        .iter()
        .any(|q| q.id == "minimality-counterexample"));
}

#[test]
fn adversarial_qa_challenges_success_claims_without_becoming_evidence() {
    let packet = build(&report());
    assert_eq!(packet.provider, "wcode-adversarial-qa");
    assert_eq!(packet.precision, "deterministic");
    assert_eq!(packet.policy, "challenge-packet-not-evidence");
    assert_eq!(packet.reviewer_role, "adversarial");
    assert!(packet.reviewer_bridge.contains("not Evidence"));
    for id in [
        "acceptance-counterexample",
        "impact-counterexample",
        "revision-freshness",
        "negative-path-counterexample",
        "test-overfit-counterexample",
    ] {
        assert!(
            packet.questions.iter().any(|question| question.id == id),
            "missing {id}"
        );
    }
    assert!(packet
        .questions
        .iter()
        .all(|question| !question.required_evidence.is_empty()));
    assert!(packet
        .questions
        .iter()
        .all(|question| !question.suggested_tools.is_empty()));
    assert!(packet.questions.iter().all(|question| {
        !question.counterexample_experiment.kind.is_empty()
            && !question
                .counterexample_experiment
                .falsifying_condition
                .is_empty()
            && !question.counterexample_experiment.execution.is_empty()
            && !question.counterexample_experiment.closes_with.is_empty()
            && question
                .counterexample_experiment
                .targets
                .contains(&"src/target.rs".to_owned())
    }));
    let overfit = packet
        .questions
        .iter()
        .find(|question| question.id == "test-overfit-counterexample")
        .unwrap();
    assert_eq!(overfit.counterexample_experiment.kind, "mutation");
    assert!(overfit
        .counterexample_experiment
        .execution
        .contains(&"verification_executor_status".to_owned()));
    assert!(overfit
        .counterexample_experiment
        .closes_with
        .iter()
        .any(|evidence| evidence.contains("mutation kill")));
    assert!(!serde_json::to_string(&packet)
        .unwrap()
        .contains("stage_passed"));
    assert!(packet
        .recommended_next_actions
        .contains(&"verification_plan".to_owned()));
}

#[test]
fn adversarial_qa_turns_deterministic_review_findings_into_targeted_questions() {
    let mut review = report();
    review.tests_changed = false;
    review.untracked_files = 2;
    review.files_changed = 40;
    review.additions = 900;
    review.deletions = 300;
    review.findings = vec![
        finding("source-without-test-change", "warning"),
        finding("security-sensitive-change", "high"),
        finding("manifest-change", "warning"),
        finding("deleted-tests", "high"),
        finding("large-change-set", "warning"),
    ];
    let packet = build(&review);
    for id in [
        "missing-regression-counterexample",
        "security-bypass-counterexample",
        "dependency-compatibility-counterexample",
        "deleted-coverage-counterexample",
        "minimality-counterexample",
        "reproducibility-counterexample",
    ] {
        assert!(
            packet.questions.iter().any(|question| question.id == id),
            "missing {id}"
        );
    }
    assert!(!packet
        .questions
        .iter()
        .any(|question| question.id == "test-overfit-counterexample"));
}

#[test]
fn adversarial_qa_is_bounded_unique_and_quiet_for_a_clean_tree() {
    let mut review = report();
    review.clean = true;
    review.files_changed = 0;
    review.source_changed = false;
    review.tests_changed = false;
    review.risk_level = "low".into();
    let clean = build(&review);
    assert!(clean.questions.is_empty());

    let mut noisy = report();
    noisy.untracked_files = 3;
    noisy.files_changed = 90;
    noisy.truncated = true;
    noisy.findings = (0..40)
        .map(|index| {
            finding(
                if index % 2 == 0 {
                    "manifest-change"
                } else {
                    "large-change-set"
                },
                "warning",
            )
        })
        .collect();
    let packet = build(&noisy);
    assert!(packet.questions.len() <= MAX_ADVERSARIAL_QUESTIONS);
    let unique = packet
        .questions
        .iter()
        .map(|question| question.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique.len(), packet.questions.len());
}
