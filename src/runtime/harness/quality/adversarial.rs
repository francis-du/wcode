use super::*;

const MAX_ADVERSARIAL_QUESTIONS: usize = 12;

macro_rules! push_question {
    ($questions:expr, $review:expr, $id:expr, $category:expr, $claim:expr, $challenge:expr, $signal:expr, $evidence:expr, $tools:expr $(,)?) => {{
        if !$questions.iter().any(|item| item.id == $id) {
            $questions.push(AdversarialQuestion {
                id: $id.to_owned(),
                category: $category.to_owned(),
                claim: $claim.to_owned(),
                challenge: $challenge.to_owned(),
                current_signal: $signal,
                required_evidence: $evidence.iter().map(|item| (*item).to_owned()).collect(),
                suggested_tools: $tools.iter().map(|item| (*item).to_owned()).collect(),
                counterexample_experiment: experiment_for($review, $id),
            });
        }
    }};
}

fn changed_targets(review: &ChangeReviewReport, id: &str) -> (Vec<String>, usize, bool) {
    let finding_code = match id {
        "security-bypass-counterexample" => Some("security-sensitive-change"),
        "dependency-compatibility-counterexample" => Some("manifest-change"),
        "deleted-coverage-counterexample" => Some("deleted-tests"),
        "generated-output-counterexample" => Some("generated-artifact-freshness"),
        _ => None,
    };
    let attributed: BTreeSet<_> = review
        .findings
        .iter()
        .filter(|finding| Some(finding.code.as_str()) == finding_code)
        .flat_map(|finding| finding.paths.iter().map(String::as_str))
        .collect();
    let paths: BTreeSet<_> = review
        .files
        .iter()
        .filter(|file| {
            if finding_code.is_some() {
                attributed.contains(file.path.as_str())
            } else if id == "reproducibility-counterexample" {
                file.untracked
            } else {
                matches!(
                    file.category.as_str(),
                    "source" | "test" | "manifest" | "workflow" | "migration"
                )
            }
        })
        .map(|file| file.path.clone())
        .collect();
    let total = paths.len();
    let truncated = total > 6 || review.truncated || has_finding(review, "review-truncated");
    (paths.into_iter().take(6).collect(), total, truncated)
}

fn experiment_for(review: &ChangeReviewReport, id: &str) -> CounterexampleExperiment {
    let (targets, targets_total, targets_truncated) = changed_targets(review, id);
    let (kind, hypothesis, falsifying_condition, execution, closes_with): (&str, &str, &str, &[&str], &[&str]) = match id {
        "acceptance-counterexample" => (
            "regression",
            "A user-visible acceptance behavior still fails despite the inferred green checks.",
            "A focused test or runtime reproduction fails on the current revision for a mapped acceptance behavior.",
            &["traceability_status", "verify_project"],
            &["current-revision focused regression pass", "mapped acceptance evidence"],
        ),
        "impact-counterexample" => (
            "compatibility",
            "An unedited caller, consumer, contract, or generated artifact observes a breaking change.",
            "Impact or semantic navigation finds a reachable consumer whose existing contract is violated.",
            &["impact_analysis", "semantic_navigation"],
            &["bounded impact evidence", "consumer regression or compatibility pass"],
        ),
        "revision-freshness" => (
            "runtime",
            "A green result belongs to an older code or Design State revision.",
            "The newest passing evidence revision differs from the active revision.",
            &["evidence_status", "verify_project"],
            &["current-revision deterministic verification evidence"],
        ),
        "negative-path-counterexample" => (
            "property",
            "A failure-path invariant is violated for malformed, stale, cancelled, concurrent, or denied inputs.",
            "A generated boundary case violates the stated invariant or produces an unintended success/state transition.",
            &["verification_plan", "verification_executor_status", "verify_project"],
            &["property/regression evidence on the active revision"],
        ),
        "test-overfit-counterexample" => (
            "mutation",
            "The changed tests accept a trivial or pre-fix implementation and therefore mirror the same mistaken assumption.",
            "A targeted mutation or pre-fix behavior survives the changed tests.",
            &["verification_plan", "verification_executor_status"],
            &["mutation kill or demonstrated pre-fix regression failure"],
        ),
        "missing-regression-counterexample" | "deleted-coverage-counterexample" => (
            "regression",
            "The claimed behavior has no independent regression oracle.",
            "No named test fails against the old/removed behavior while passing on the intended behavior.",
            &["traceability_status", "verify_project"],
            &["named regression with pre-fix failure and current pass"],
        ),
        "security-bypass-counterexample" => (
            "authorization",
            "An alternate entry point bypasses the intended trust or authorization boundary.",
            "A denied identity/input/path reaches the protected operation or receives protected data.",
            &["risk_status", "verification_plan", "verify_project"],
            &["negative authorization/security regression evidence"],
        ),
        "dependency-compatibility-counterexample" => (
            "compatibility",
            "A platform, feature, lockfile, or downstream consumer is incompatible with the metadata change.",
            "A relevant locked/platform/feature build or compatibility check fails.",
            &["impact_analysis", "verify_project"],
            &["relevant locked build/test or compatibility evidence"],
        ),
        "generated-output-counterexample" => (
            "runtime",
            "Tracked generated output differs from what the changed contract/config would regenerate.",
            "The native freshness check or regeneration produces a diff.",
            &["verify_project", "run_command"],
            &["generator-native freshness pass or zero regeneration diff"],
        ),
        "minimality-counterexample" => (
            "regression",
            "Some changed file or abstraction is unnecessary to satisfy the acceptance behavior.",
            "Removing one independent change preserves all required behavior and verification.",
            &["traceability_status", "verification_plan"],
            &["change-to-requirement mapping", "independent maintainability review"],
        ),
        "reproducibility-counterexample" => (
            "runtime",
            "An untracked input changes the result without being part of the intended deliverable.",
            "Removing or excluding a relevant untracked input changes verification or runtime behavior.",
            &["review_changes", "verify_project"],
            &["reproducible verification from the intended tracked inputs"],
        ),
        "bounded-review-counterexample" => (
            "compatibility",
            "A relevant changed file lies outside the bounded review window.",
            "Partitioned review reveals an additional risk-bearing file or dependency omitted from the original packet.",
            &["review_changes", "impact_analysis"],
            &["complete or partitioned review coverage"],
        ),
        _ => (
            "regression",
            "The challenged claim has a concrete observable counterexample.",
            "A current-revision deterministic or runtime check reproduces the counterexample.",
            &["verify_project"],
            &["current-revision deterministic evidence"],
        ),
    };
    CounterexampleExperiment {
        kind: kind.to_owned(),
        targets,
        targets_total,
        targets_truncated,
        hypothesis: hypothesis.to_owned(),
        falsifying_condition: falsifying_condition.to_owned(),
        execution: execution.iter().map(|item| (*item).to_owned()).collect(),
        closes_with: closes_with.iter().map(|item| (*item).to_owned()).collect(),
    }
}

fn has_finding(review: &ChangeReviewReport, code: &str) -> bool {
    review.findings.iter().any(|finding| finding.code == code)
}

pub(super) fn build(review: &ChangeReviewReport) -> AdversarialReviewReport {
    let mut questions: Vec<AdversarialQuestion> = Vec::new();
    if !review.clean {
        push_question!(
            &mut questions,
            review,
            "acceptance-counterexample",
            "correctness",
            "Passing the inferred checks is sufficient to prove the requested behavior.",
            "Which acceptance criterion or user-visible behavior could still fail even if every currently inferred check passes?",
            format!(
                "The working tree changes {} file(s); review_changes does not itself prove product acceptance.",
                review.files_changed
            ),
            &["mapped acceptance criterion", "focused regression or runtime observation"],
            &["traceability_status", "verify_project"],
        );
        push_question!(
            &mut questions,
            review,
            "impact-counterexample",
            "scope",
            "The changed files are the complete impact surface.",
            "Which caller, consumer, generated artifact, configuration, or contract can break without appearing in the edited file list?",
            format!("{} changed file(s) are visible in the bounded Git review.", review.files_changed),
            &["bounded impact chain", "cross-file relationship evidence"],
            &["impact_analysis", "semantic_navigation"],
        );
        push_question!(
            &mut questions,
            review,
            "revision-freshness",
            "evidence",
            "Any green verification still proves the current code and Design State.",
            "What changed after the newest green result, and is every claimed pass bound to the exact current revision?",
            "This QA packet is generated from the current review but does not attest verification freshness.".to_owned(),
            &["current-revision deterministic verification evidence"],
            &["verify_project", "evidence_status"],
        );
    }

    if review.source_changed {
        push_question!(
            &mut questions,
            review,
            "negative-path-counterexample",
            "correctness",
            "The implementation is correct because the main path works.",
            "Which stale, empty, malformed, permission-denied, timeout, cancellation, or concurrency case can falsify the implementation?",
            "Production source changed; happy-path checks alone cannot establish failure-path behavior.".to_owned(),
            &["focused negative-path regression", "runtime or deterministic failure observation"],
            &["verify_project", "verification_plan"],
        );
    }

    if review.source_changed && review.tests_changed {
        push_question!(
            &mut questions,
            review,
            "test-overfit-counterexample",
            "tests",
            "The changed tests independently prove the changed implementation.",
            "Would these tests fail against the pre-change behavior or a trivial/stub implementation, or are implementation and tests encoding the same mistaken assumption?",
            "Source and tests changed together.".to_owned(),
            &["pre-fix failure", "mutation/property evidence or an independent regression oracle"],
            &["verification_plan", "verification_executor_status"],
        );
    } else if has_finding(review, "source-without-test-change") {
        push_question!(
            &mut questions,
            review,
            "missing-regression-counterexample",
            "tests",
            "Existing tests already cover the changed behavior.",
            "Which existing test fails on the old behavior and passes on this change? If none does, what regression test is missing?",
            "review_changes observed source edits without a test-file change.".to_owned(),
            &["named existing regression with pre-fix failure", "or a new focused regression"],
            &["traceability_status", "verify_project"],
        );
    }

    if has_finding(review, "security-sensitive-change") {
        push_question!(
            &mut questions,
            review,
            "security-bypass-counterexample",
            "security",
            "The security control still enforces the intended property on every path.",
            "What alternate entry point, stale identity, confused-deputy path, missing denial, or malformed input bypasses the control while the happy path still passes?",
            "Security-sensitive files changed.".to_owned(),
            &["explicit security invariant", "negative authorization/authentication test or runtime proof"],
            &["risk_status", "verification_plan"],
        );
    }
    if has_finding(review, "manifest-change") {
        push_question!(
            &mut questions,
            review,
            "dependency-compatibility-counterexample",
            "compatibility",
            "The dependency/build metadata change is behaviorally compatible.",
            "Which lockfile, platform, feature combination, generated output, or downstream consumer can fail despite the local build succeeding?",
            "Dependency or build metadata changed.".to_owned(),
            &["locked build/test evidence", "relevant compatibility or platform evidence"],
            &["verify_project", "impact_analysis"],
        );
    }
    if has_finding(review, "deleted-tests") {
        push_question!(
            &mut questions,
            review,
            "deleted-coverage-counterexample",
            "tests",
            "Deleted tests were redundant and coverage was preserved.",
            "Which exact behavior formerly guarded by the deleted test is still exercised, and where is the replacement evidence?",
            "Test deletion is present in the current review.".to_owned(),
            &["replacement test mapping or explicit removal rationale plus equivalent evidence"],
            &["traceability_status", "verify_project"],
        );
    }
    if has_finding(review, "generated-artifact-freshness") {
        push_question!(
            &mut questions,
            review,
            "generated-output-counterexample",
            "generated-code",
            "Tracked generated output is still synchronized with its changed input contract.",
            "Can regeneration or the native freshness check reproduce the checked-in output exactly?",
            "A contract/config changed without a matching generated-output change.".to_owned(),
            &["generator-native freshness result or regenerated diff"],
            &["verify_project", "run_command"],
        );
    }
    if has_finding(review, "large-change-set") || review.files_changed > 25 {
        push_question!(
            &mut questions,
            review,
            "minimality-counterexample",
            "maintainability",
            "Every changed file is necessary for the requested outcome.",
            "Which changed file or abstraction can be removed while keeping the acceptance criterion true?",
            format!("The review spans {} files and {} changed lines.", review.files_changed, review.additions.saturating_add(review.deletions)),
            &["change-to-requirement mapping", "independent maintainability review"],
            &["traceability_status", "verification_plan"],
        );
    }
    if review.untracked_files > 0 {
        push_question!(
            &mut questions,
            review,
            "reproducibility-counterexample",
            "reproducibility",
            "The reviewed result is reproducible from the intended change set.",
            "Do any untracked inputs, fixtures, generated files, or tests affect the result without being part of the intended deliverable?",
            format!("{} untracked file(s) are present.", review.untracked_files),
            &["explicit disposition for every relevant untracked file"],
            &["review_changes"],
        );
    }
    if review.truncated || has_finding(review, "review-truncated") {
        push_question!(
            &mut questions,
            review,
            "bounded-review-counterexample",
            "coverage",
            "The bounded review saw every relevant changed file.",
            "What could be hidden beyond the review bound, and has the remaining change set been inspected separately?",
            "The bounded change review is truncated.".to_owned(),
            &["complete change-set coverage or a narrower partitioned review"],
            &["review_changes", "impact_analysis"],
        );
    }

    let truncated = review.truncated
        || has_finding(review, "review-truncated")
        || questions.len() > MAX_ADVERSARIAL_QUESTIONS
        || questions
            .iter()
            .any(|q| q.counterexample_experiment.targets_truncated);
    questions.truncate(MAX_ADVERSARIAL_QUESTIONS);
    let mut next_actions = vec!["review_changes".to_owned()];
    if review.source_changed {
        next_actions.extend([
            "traceability_status".to_owned(),
            "impact_analysis".to_owned(),
            "risk_status".to_owned(),
        ]);
    }
    if matches!(review.risk_level.as_str(), "moderate" | "high") || review.tests_changed {
        next_actions.push("verification_plan".to_owned());
    }
    next_actions.push("verify_project".to_owned());

    AdversarialReviewReport {
        workspace: review.workspace.clone(),
        provider: "wcode-adversarial-qa",
        precision: "deterministic",
        policy: "challenge-packet-not-evidence",
        review_risk_level: review.risk_level.clone(),
        questions,
        truncated,
        recommended_next_actions: next_actions,
        reviewer_role: "adversarial",
        candidate_search: None,
        reviewer_bridge: "Use verification_plan and claim an independent adversarial reviewer only when that role is queued by the risk policy. Other plans retain their assigned roles. This packet itself is not Evidence; self-review is not independent proof, and only actual review or deterministic/stage/human evidence enters the Verification Mesh.",
    }
}

impl ToolHarness {
    pub fn adversarial_review(&self, review: &ChangeReviewReport) -> AdversarialReviewReport {
        build(review)
    }

    pub fn adversarial_review_with_candidates(
        &self,
        workspace: &Workspace,
        review: &ChangeReviewReport,
    ) -> AdversarialReviewReport {
        let mut packet = self.adversarial_review(review);
        let search = super::counterexamples::build(self, workspace, review);
        packet.truncated |= search.truncated;
        packet.candidate_search = Some(search);
        packet
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/runtime/harness/adversarial.rs"]
mod tests;
