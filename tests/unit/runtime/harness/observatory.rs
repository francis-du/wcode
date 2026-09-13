use super::*;

#[tokio::test]
async fn observatory_revision_signal_detects_repeated_edits_to_same_modified_file() {
    use std::process::Command;

    let root = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(root.path())
            .status()
            .expect("git must be available for observatory revision tests")
    };
    assert!(git(&["init", "-q"]).success());
    assert!(git(&["config", "user.email", "wcode@example.test"]).success());
    assert!(git(&["config", "user.name", "wcode test"]).success());
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn value() -> i32 { 1 }\n",
    )
    .unwrap();
    assert!(git(&["add", "."]).success());
    assert!(git(&["-c", "commit.gpgsign=false", "commit", "-qm", "initial"]).success());

    let workspace = Workspace::new(root.path(), false, true).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    let clean = harness
        .observatory_revision_signal(&workspace)
        .await
        .unwrap();
    assert_eq!(clean.changed_files, 0);

    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn value() -> i32 { 2 }\n",
    )
    .unwrap();
    let first = harness
        .observatory_revision_signal(&workspace)
        .await
        .unwrap();
    assert_eq!(first.changed_files, 1);

    std::thread::sleep(std::time::Duration::from_millis(2));
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn value() -> i32 { 3 }\n",
    )
    .unwrap();
    let second = harness
        .observatory_revision_signal(&workspace)
        .await
        .unwrap();
    assert_eq!(second.changed_files, 1);
    assert_ne!(first.fingerprint, second.fingerprint);
}

#[tokio::test]
async fn observatory_revision_signal_uses_metadata_without_git_or_execution() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn value() -> u8 { 1 }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let harness = ToolHarness::new(2).unwrap();

    let first = harness
        .observatory_revision_signal(&workspace)
        .await
        .unwrap();
    assert!(first.fingerprint.is_some());
    assert!(!first.truncated);
    assert!(!first.full_refresh_required);

    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn value() -> u16 { 1000 }\n",
    )
    .unwrap();
    let second = harness
        .observatory_revision_signal(&workspace)
        .await
        .unwrap();
    assert_ne!(first.fingerprint, second.fingerprint);
    assert!(!second.full_refresh_required);

    fs::write(root.path().join("src/new.rs"), "pub fn added() {}\n").unwrap();
    let third = harness
        .observatory_revision_signal(&workspace)
        .await
        .unwrap();
    assert_ne!(second.fingerprint, third.fingerprint);
    assert!(!third.full_refresh_required);
}

#[test]
fn product_scope_status_maps_source_domains_and_surfaces_unmapped_files() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src/runtime")).unwrap();
    fs::create_dir_all(root.path().join("src/integrations")).unwrap();
    fs::write(
        root.path().join("src/runtime/control.rs"),
        "pub fn control() {}\n",
    )
    .unwrap();
    fs::write(
        root.path().join("src/integrations/mcp.rs"),
        "pub fn serve() {}\n",
    )
    .unwrap();
    fs::write(root.path().join("src/orphan.rs"), "pub fn orphan() {}\n").unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let status = harness.product_scope_status(&workspace).unwrap();

    assert_eq!(status.source_files, 3);
    assert_eq!(status.mapped_files, 2);
    assert_eq!(status.counts.get("runtime"), Some(&1));
    assert_eq!(status.counts.get("integrations"), Some(&1));
    assert_eq!(status.unmapped_files, vec!["src/orphan.rs"]);
}

#[test]
fn project_observatory_builds_requirement_architecture_from_current_code() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join(".wcode/project.yaml"),
        "schema_version: 1\nname: demo\ndescription: observatory fixture\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".wcode/design/requirements.yaml"),
        r#"- schema_version: 1
  id: REQ-FEATURE-001
  title: Feature flow
  intent: Route feature work through the helper boundary.
  priority: high
  implemented_by:
    - component:feature
    - component:helper
  acceptance:
    - AC-FEATURE-001
  constraints: []
  risk: {}
"#,
    )
    .unwrap();
    fs::write(
        root.path().join(".wcode/design/components.yaml"),
        r#"- schema_version: 1
  id: component:feature
  name: Feature entry
  responsibilities:
    - expose the feature entry point
  depends_on:
    - component:helper
  constraints: []
  implementation:
    - kind: symbol
      path: src/lib.rs
      symbol: feature_entry
- schema_version: 1
  id: component:helper
  name: Helper boundary
  responsibilities:
    - implement the helper behavior
  depends_on: []
  constraints: []
  implementation:
    - kind: symbol
      path: src/lib.rs
      symbol: helper
"#,
    )
    .unwrap();
    fs::write(
        root.path().join(".wcode/design/acceptance.yaml"),
        r#"- schema_version: 1
  id: AC-FEATURE-001
  title: Feature calls helper
  statement: The feature entry is backed by the helper implementation.
  verification:
    - kind: test
      path: src/lib.rs
      symbol: tests::feature_flow
"#,
    )
    .unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        r#"pub fn feature_entry() {
    helper();
}

pub fn helper() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_flow() {
        feature_entry();
    }
}
"#,
    )
    .unwrap();

    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let project = harness
        .project_observatory("demo", &workspace, None)
        .unwrap();

    assert!(project.design_valid);
    assert_eq!(project.code.source_files, 1);
    assert!(project.code.source_lines >= 10);
    assert!(project
        .code
        .languages
        .iter()
        .any(|language| language.name == "rust" && language.files == 1));
    let feature = project
        .requirements
        .iter()
        .find(|requirement| requirement.id == "REQ-FEATURE-001")
        .unwrap();
    assert!(feature.aligned, "{:#?}", feature.dependency_alignment);
    assert_eq!(
        feature.convergence,
        crate::intelligence::FeatureConvergenceState::Stable
    );
    assert!(feature.convergence_blockers.is_empty());
    assert_eq!(feature.components.len(), 2);
    assert_eq!(feature.implementation_symbols, 2);
    assert_eq!(project.convergence.stable_requirements, 1);
    assert_eq!(project.convergence.needs_convergence_requirements, 0);
    assert_eq!(project.proof.current_evidence, 0);
    assert_eq!(project.adaptive_verification.mode, "static");
    assert_eq!(
        project.adaptive_verification.fallback_reason.as_deref(),
        Some("review_unavailable")
    );
    assert!(project.adaptive_verification.full_coverage_unchanged);
    assert!(project.verified_learning.available);
    assert_eq!(project.verified_learning.records, 0);
    assert_eq!(
        project.verified_learning.evaluation_method,
        "global-temporal-ab-v2"
    );
    assert_eq!(
        project.verified_learning.retrieval_model,
        "verified-context-cochange-v3"
    );
    assert_eq!(project.verified_learning.baseline_model, "raw-count-v1");
    assert_eq!(project.verified_learning.retrieval_precision, "heuristic");
    assert!(!project.verified_learning.stores_prompts_or_chain_of_thought);
    assert_eq!(project.proof.acceptance.total, 1);
    assert_eq!(project.proof.acceptance.mapped, 1);
    assert_eq!(project.proof.acceptance.executed, 0);
    assert_eq!(project.proof.acceptance.passed, 0);
    assert_eq!(project.proof.acceptance.fresh, 0);
    assert_eq!(project.architecture.components.len(), 2);
    assert_eq!(project.architecture.subsystems.len(), 1);
    let subsystem = &project.architecture.subsystems[0];
    assert_eq!(subsystem.id, "runtime");
    assert_eq!(subsystem.components, 2);
    assert_eq!(subsystem.requirements, 1);
    assert_eq!(subsystem.implementation_files, 1);
    assert_eq!(subsystem.changed_components, 0);
    assert!(subsystem.depends_on.is_empty());
    assert!(subsystem.depended_on_by.is_empty());
    assert!(subsystem.designed_depends_on.is_empty());
    assert!(subsystem.observed_depends_on.is_empty());
    assert!(subsystem.unobserved_designed_depends_on.is_empty());
    assert!(subsystem.undeclared_observed_depends_on.is_empty());
    assert_eq!(project.architecture.desired_edges, 1);
    assert_eq!(project.architecture.aligned_edges, 1);
    assert_eq!(project.architecture.blocking_drift_edges, 0);
    assert_eq!(project.architecture.observed_drift_percent, 0.0);
    assert_eq!(project.architecture.evidence_coverage_percent, 100.0);
    assert_eq!(project.architecture.implementation_coverage_percent, 100.0);
    assert!(project.architecture.dependencies.iter().any(|dependency| {
        dependency.from == "component:feature"
            && dependency.to == "component:helper"
            && dependency.desired
            && dependency.actual
            && dependency.status == "aligned"
    }));
    assert!(feature.dependency_alignment.iter().any(|dependency| {
        dependency.from == "component:feature"
            && dependency.to == "component:helper"
            && dependency.desired
            && dependency.actual
            && dependency.status == "aligned"
    }));

    let review = ChangeReviewReport {
        workspace: "demo".into(),
        execution: "fixture".into(),
        clean: false,
        files_changed: 1,
        staged_files: 0,
        unstaged_files: 1,
        untracked_files: 0,
        additions: 1,
        deletions: 0,
        binary_files: 0,
        source_changed: true,
        tests_changed: false,
        docs_only: false,
        risk_level: "moderate".into(),
        recommended_verification: "full".into(),
        recommended_checks: Vec::new(),
        summary: "fixture review".into(),
        files: vec![ChangedFileReview {
            path: "src/lib.rs".into(),
            status: "modified".into(),
            staged: false,
            unstaged: true,
            untracked: false,
            category: "source".into(),
            additions: Some(1),
            deletions: Some(0),
            binary: false,
            risk_reasons: Vec::new(),
        }],
        findings: Vec::new(),
        probes: vec![ReviewProbeSummary {
            id: "status".into(),
            success: true,
            elapsed_ms: 1,
            queue_wait_ms: 0,
            execution_ms: 1,
            error: None,
        }],
        truncated: false,
    };
    crate::design::LOAD_DESIGN_CALLS.with(|count| count.set(0));
    crate::stage_executor::REGISTRY_CALLS.with(|count| count.set(0));
    let reviewed = harness
        .project_observatory("demo", &workspace, Some(&review))
        .unwrap();
    assert_eq!(
        crate::design::LOAD_DESIGN_CALLS.with(|count| count.get()),
        0,
        "a warm Observatory snapshot should reuse the already-loaded Design State"
    );
    assert_eq!(
        crate::stage_executor::REGISTRY_CALLS.with(|count| count.get()),
        0,
        "a warm Observatory snapshot should reuse the already-discovered advanced executor registry"
    );
    harness.design_status("demo", &workspace).unwrap();
    assert_eq!(
        crate::design::LOAD_DESIGN_CALLS.with(|count| count.get()),
        0,
        "unchanged Design State should continue to reuse the warm runtime cache"
    );
    let project_path = workspace.root().join(crate::design::PROJECT_FILE);
    let mut project = std::fs::read_to_string(&project_path).unwrap();
    project.push('\n');
    std::fs::write(&project_path, project).unwrap();
    harness.design_status("demo", &workspace).unwrap();
    assert_eq!(
        crate::design::LOAD_DESIGN_CALLS.with(|count| count.get()),
        1,
        "Design cache must invalidate exactly once when external file metadata changes"
    );
    assert_eq!(
        reviewed.impact.as_ref().unwrap().risk_level,
        reviewed.risk.as_ref().unwrap().level,
        "impact must reuse the risk level from the same Observatory snapshot"
    );
    let verification_impact = reviewed.verification_impact.as_ref().unwrap();
    assert!(verification_impact.selective);
    assert_eq!(verification_impact.affected_islands, vec!["."]);
    assert!(verification_impact.reasons.iter().any(|reason| {
        reason.kind == "direct_change"
            && reason.source == "src/lib.rs"
            && reason.relationship == "manifest_ownership"
            && reason.provider == "manifest-discovery"
            && reason.precision == "structural"
    }));

    harness
        .intelligence
        .record_verification_report(
            "demo",
            &workspace,
            &harness.intelligence.current_revision(&workspace).unwrap(),
            &crate::harness::VerificationReport {
                workspace: "demo".into(),
                level: "quick".into(),
                execution: "fixture".into(),
                phases_run: 1,
                passed: true,
                checks_run: 1,
                checks_reused: 0,
                checks_failed: 0,
                skipped_checks: Vec::new(),
                elapsed_ms: 1,
                summary: "fixture passed".into(),
                impact: None,
                cost_model: None,
                checks: vec![crate::harness::VerificationCheck {
                    id: "rust-test".into(),
                    phase: 0,
                    command: "cargo test --locked".into(),
                    reason: "fixture".into(),
                    success: true,
                    reused: false,
                    exit_code: Some(0),
                    elapsed_ms: 1,
                    queue_wait_ms: 0,
                    execution_ms: 1,
                    stdout_tail: String::new(),
                    stderr_tail: String::new(),
                    output_truncated: false,
                }],
            },
        )
        .unwrap();
    let proved = harness
        .project_observatory("demo", &workspace, None)
        .unwrap();
    assert_eq!(proved.proof.acceptance.mapped, 1);
    assert_eq!(proved.proof.acceptance.executed, 1);
    assert_eq!(proved.proof.acceptance.passed, 1);
    assert_eq!(proved.proof.acceptance.fresh, 1);

    fs::write(
        root.path().join("src/lib.rs"),
        r#"pub fn feature_entry() {
    let _revision = 2;
}

pub fn helper() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_flow() {
        feature_entry();
    }
}
"#,
    )
    .unwrap();
    let changed = harness
        .project_observatory("demo", &workspace, None)
        .unwrap();
    assert_eq!(changed.proof.acceptance.executed, 1);
    assert_eq!(changed.proof.acceptance.passed, 1);
    assert_eq!(changed.proof.acceptance.fresh, 0);
    assert!(changed.history.len() >= 2);
    let changed_feature = changed
        .requirements
        .iter()
        .find(|requirement| requirement.id == "REQ-FEATURE-001")
        .unwrap();
    assert_eq!(
        changed_feature.convergence,
        crate::intelligence::FeatureConvergenceState::Stable
    );
    assert!(changed_feature.aligned);
    assert!(changed_feature.convergence_blockers.is_empty());
    assert!(changed_feature
        .dependency_alignment
        .iter()
        .any(|dependency| {
            dependency.from == "component:feature"
                && dependency.to == "component:helper"
                && dependency.desired
                && !dependency.actual
                && !dependency.blocking
                && dependency.status == "unverified_actual"
        }));
    assert_eq!(changed.convergence.needs_convergence_requirements, 0);
    let delta = changed
        .latest_delta
        .expect("second code revision should produce a graph delta");
    assert!(delta.changed_paths.iter().any(|path| path == "src/lib.rs"));
}

#[test]
fn project_observatory_exposes_bounded_file_structure_and_oversized_files() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src/nested")).unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        "pub mod nested;\npub fn small() {}\n",
    )
    .unwrap();
    fs::write(
        root.path().join("src/nested/large.rs"),
        "pub fn line() {}\n".repeat(1_001),
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let project = ToolHarness::new(4)
        .unwrap()
        .project_observatory("demo", &workspace, None)
        .unwrap();

    assert_eq!(project.structure.line_limit, 1_000);
    assert_eq!(project.structure.entries.len(), 2);
    assert!(project.structure.directory_count >= 2);
    assert!(project.structure.max_depth >= 3);
    assert_eq!(project.structure.oversized_files, 1);
    assert_eq!(
        project.structure.largest_files[0].path,
        "src/nested/large.rs"
    );
    assert!(project.structure.largest_files[0].over_limit);
    assert!(!project.structure.truncated);
}
