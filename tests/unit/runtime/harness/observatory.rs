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
    assert_eq!(project.architecture.components.len(), 2);
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
