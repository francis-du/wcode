use super::*;
use std::fs;

#[tokio::test]
async fn enforces_parallel_limit() {
    let harness = ToolHarness::new(2).unwrap();
    let first = harness.acquire().await.unwrap();
    let second = harness.acquire().await.unwrap();
    assert_eq!(harness.slots.available_permits(), 0);
    drop(first);
    assert_eq!(harness.slots.available_permits(), 1);
    drop(second);
}

#[test]
fn rejects_unbounded_parallelism() {
    assert!(ToolHarness::new(0).is_err());
    assert_eq!(
        ToolHarness::new(MAX_PARALLEL_TOOLS)
            .expect("documented maximum should be accepted")
            .max_parallel(),
        MAX_PARALLEL_TOOLS
    );
    assert!(ToolHarness::new(MAX_PARALLEL_TOOLS + 1).is_err());
}

#[test]
fn design_init_bootstraps_minimal_valid_state_without_overwrite() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let status = harness
        .design_init("demo", &workspace, "Demo Service", "runtime fixture")
        .unwrap();
    assert!(status.initialized);
    assert!(status.valid, "{:?}", status.diagnostics);
    assert_eq!(status.project.as_deref(), Some("Demo Service"));
    assert!(root.path().join(".wcode/design/product.yaml").is_file());
    for collection in [
        "requirements.yaml",
        "components.yaml",
        "constraints.yaml",
        "acceptance.yaml",
        "decisions.yaml",
    ] {
        assert!(
            !root.path().join(".wcode/design").join(collection).exists(),
            "design_init must not materialize empty {collection} placeholders"
        );
    }
    assert_eq!(status.requirements, 0);
    assert_eq!(status.components, 0);
    assert_eq!(status.constraints, 0);
    assert_eq!(status.acceptance_criteria, 0);
    assert_eq!(status.decisions, 0);
    assert!(harness
        .design_init("demo", &workspace, "Other", "")
        .is_err());
}

#[test]
fn project_context_detects_guidance_and_quality_checks() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.path().join("Cargo.lock"), "# lock\n").unwrap();
    fs::write(
        root.path().join("AGENTS.md"),
        "# Instructions\nKeep changes small and run tests.\n",
    )
    .unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/large_module.rs"),
        "// fixture\n".repeat(2_001),
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();

    let first = harness.project_context("demo", &workspace).unwrap();
    assert!(!first.cache_hit);
    assert!(first.project_types.contains(&"rust".to_owned()));
    assert!(first
        .guidance
        .iter()
        .any(|document| document.path == "AGENTS.md"));
    assert!(first
        .recommended_checks
        .iter()
        .any(|check| check.id == "rust-check" && check.args.contains(&"--locked".to_owned())));
    assert!(first
        .workflow
        .iter()
        .any(|step| step.contains("scope_status") && step.contains("architecture debt")));
    assert!(first.recommended_checks.iter().any(|check| {
        check.id == "rust-release-build"
            && check.args
                == [
                    "build".to_owned(),
                    "--release".to_owned(),
                    "--locked".to_owned(),
                ]
            && check.phase == 3
    }));
    assert!(first
        .conventions
        .findings
        .iter()
        .any(|finding| finding.code == "oversized-source-module"));
    assert_eq!(first.language_quality.languages.len(), 22);
    let rust_quality = first
        .language_quality
        .languages
        .iter()
        .find(|language| language.language == crate::semantic_provider::SemanticLanguage::Rust)
        .unwrap();
    assert_eq!(rust_quality.detected_files, 1);
    assert!(rust_quality
        .providers
        .iter()
        .any(|provider| provider.id == "rustfmt" && provider.declared));
    assert!(first
        .product_scopes
        .iter()
        .any(|scope| scope.id == "graph" && scope.title == "Software Graph"));
    assert!(first
        .product_scopes
        .iter()
        .any(|scope| scope.id == "reconciliation"));

    let second = harness.project_context("demo", &workspace).unwrap();
    assert!(second.cache_hit);
}

#[test]
fn maintainability_review_flags_threshold_crossing_and_cross_scope_churn() {
    let root = tempfile::tempdir().unwrap();
    for (path, lines) in [
        ("src/runtime/growth.rs", 1_005usize),
        ("src/graph/growth.rs", 40usize),
        ("src/ui/growth.rs", 40usize),
    ] {
        let full = root.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, "fn fixture() {}\n".repeat(lines)).unwrap();
    }
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let files = vec![
        ChangedFileReview {
            path: "src/runtime/growth.rs".into(),
            status: "modified".into(),
            staged: false,
            unstaged: true,
            untracked: false,
            category: "source".into(),
            additions: Some(505),
            deletions: Some(495),
            binary: false,
            risk_reasons: vec![],
        },
        ChangedFileReview {
            path: "src/graph/growth.rs".into(),
            status: "modified".into(),
            staged: false,
            unstaged: true,
            untracked: false,
            category: "source".into(),
            additions: Some(500),
            deletions: Some(0),
            binary: false,
            risk_reasons: vec![],
        },
        ChangedFileReview {
            path: "src/ui/growth.rs".into(),
            status: "modified".into(),
            staged: false,
            unstaged: true,
            untracked: false,
            category: "source".into(),
            additions: Some(500),
            deletions: Some(0),
            binary: false,
            risk_reasons: vec![],
        },
    ];
    let mut findings = Vec::new();
    append_maintainability_findings(&workspace, &files, &mut findings);
    assert!(findings
        .iter()
        .any(|finding| finding.code == "maintainability-file-crossed-1k"));
    assert!(findings
        .iter()
        .any(|finding| finding.code == "maintainability-concentrated-growth"));
    assert!(findings
        .iter()
        .any(|finding| finding.code == "maintainability-cross-scope-churn"));

    let untracked = root.path().join("src/runtime/new_large.rs");
    fs::write(&untracked, "fn fixture() {}\n".repeat(1_100)).unwrap();
    let mut untracked_findings = Vec::new();
    append_maintainability_findings(
        &workspace,
        &[ChangedFileReview {
            path: "src/runtime/new_large.rs".into(),
            status: "untracked".into(),
            staged: false,
            unstaged: false,
            untracked: true,
            category: "source".into(),
            additions: None,
            deletions: None,
            binary: false,
            risk_reasons: vec![],
        }],
        &mut untracked_findings,
    );
    assert!(untracked_findings
        .iter()
        .any(|finding| finding.code == "maintainability-file-crossed-1k"));
}

#[test]
fn node_context_uses_repository_package_manager_and_scripts() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("package.json"),
        r#"{"scripts":{"lint":"eslint .","test":"vitest run","build":"vite build"}}"#,
    )
    .unwrap();
    fs::write(root.path().join("pnpm-lock.yaml"), "lockfileVersion: 9\n").unwrap();
    let workspace = Workspace::new(root.path(), false, true).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    let context = harness.project_context("web", &workspace).unwrap();

    assert!(context.project_types.contains(&"node".to_owned()));
    assert!(context
        .recommended_checks
        .iter()
        .any(|check| check.program == "pnpm" && check.args == ["run", "lint"]));
}

#[test]
fn change_review_parsers_classify_status_metrics_and_risk() {
    let (mut files, truncated) =
        parse_git_status(" M src/auth.rs\nA  Cargo.lock\n?? tests/auth_test.rs\n D README.md\n");
    assert!(!truncated);
    assert_eq!(files["src/auth.rs"].status, "modified");
    assert!(files["src/auth.rs"].unstaged);
    assert!(files["Cargo.lock"].staged);
    assert!(files["tests/auth_test.rs"].untracked);
    assert_eq!(files["README.md"].status, "deleted");

    assert!(!merge_numstat(
        &mut files,
        "12\t3\tsrc/auth.rs\n2\t0\ttests/auth_test.rs\n-\t-\tassets/logo.png\n",
    ));
    assert_eq!(files["src/auth.rs"].additions, 12);
    assert_eq!(files["src/auth.rs"].deletions, 3);
    assert!(files["assets/logo.png"].binary);
    assert_eq!(file_category("src/auth.rs"), "source");
    assert_eq!(file_category("web/page.html"), "source");
    assert_eq!(file_category("web/styles.css"), "source");
    assert_eq!(file_category("lib/worker.ex"), "source");
    assert_eq!(file_category("tests/auth_test.rs"), "test");
    assert_eq!(file_category("src/runtime/harness_tests.rs"), "test");
    assert_eq!(file_category("src/intelligence/tests.rs"), "test");
    assert_eq!(file_category("lib/service_spec.rb"), "test");
    assert_eq!(file_category("README.md"), "docs");
    assert_eq!(file_category("Cargo.lock"), "manifest");
    assert!(security_sensitive_path("src/auth.rs"));
    assert!(!security_sensitive_path("src/author.rs"));
    assert_eq!(
        normalize_numstat_path("src/{old.rs => new.rs}"),
        "src/new.rs"
    );
    assert_eq!(verification_phase("rust-format"), 0);
    assert_eq!(verification_phase("rust-test"), 1);
    assert_eq!(verification_phase("rust-clippy"), 2);
    assert_eq!(verification_phase("rust-release-build"), 3);
}

#[tokio::test]
async fn change_review_runs_all_probes_without_parent_slot_deadlock() {
    use std::process::Command;

    let root = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(root.path())
            .status()
            .expect("git must be available for repository review tests")
    };
    assert!(git(&["init", "-q"]).success());
    assert!(git(&["config", "user.email", "wcode@example.test"]).success());
    assert!(git(&["config", "user.name", "wcode test"]).success());
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::create_dir_all(root.path().join("tests")).unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"review-demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join("src/auth.rs"),
        "pub fn enabled() -> bool { false }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("tests/auth_test.rs"),
        "// existing coverage\n",
    )
    .unwrap();
    assert!(git(&["add", "."]).success());
    assert!(git(&["-c", "commit.gpgsign=false", "commit", "-qm", "initial"]).success());

    fs::write(
        root.path().join("src/auth.rs"),
        "pub fn enabled() -> bool { true }\n",
    )
    .unwrap();
    fs::create_dir_all(root.path().join("docs")).unwrap();
    fs::write(root.path().join("docs/note.md"), "review note\n").unwrap();

    let workspace = Workspace::new(root.path(), false, true).unwrap();
    let workspace_id = "review-demo".to_owned();
    let harness = ToolHarness::new(1).unwrap();
    let monitor = TaskMonitor::new([workspace_id.clone()]);
    let report = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        harness.review_changes(workspace_id, &workspace, 30, &monitor),
    )
    .await
    .expect("review probes must not deadlock with one semaphore slot")
    .unwrap();

    assert_eq!(report.execution, "parallel-git-probes");
    assert_eq!(report.probes.len(), 5);
    assert_eq!(report.files_changed, 2);
    assert!(report.source_changed);
    assert!(!report.tests_changed);
    assert_eq!(report.risk_level, "high");
    assert_eq!(report.recommended_verification, "full");
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.code == "source-without-test-change"));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.code == "security-sensitive-change"));
}

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
fn agent_context_compiles_edit_ready_pack_with_real_budget_and_sha() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(root.path().join("Cargo.lock"), "# fixture lock\n").unwrap();
    fs::write(
        root.path().join("AGENTS.md"),
        format!(
            "# Rules\nReuse existing helpers. {}\n",
            "bounded guidance ".repeat(80)
        ),
    )
    .unwrap();
    fs::write(
        root.path().join(".wcode/project.yaml"),
        "schema_version: 1\nname: demo\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".wcode/design/requirements.yaml"),
        r#"- schema_version: 1
  id: REQ-CONTEXT-001
  title: Compact feature context
  intent: Route feature work through the existing helper.
  priority: high
  implemented_by:
    - component:feature
  acceptance:
    - AC-CONTEXT-001
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
    - reuse the helper boundary
  depends_on: []
  constraints: []
  implementation:
    - kind: symbol
      path: src/lib.rs
      symbol: feature_entry
"#,
    )
    .unwrap();
    fs::write(
        root.path().join(".wcode/design/acceptance.yaml"),
        r#"- schema_version: 1
  id: AC-CONTEXT-001
  title: Feature flow stays verified
  statement: The feature entry continues to call the helper.
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

fn helper() {}

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

    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let pack = harness
        .agent_context("demo", &workspace, "feature entry", 1_000, &[])
        .unwrap();

    assert!(pack["estimated_tokens"].as_u64().unwrap() <= 1_000);
    assert!(pack["serialized_bytes"].as_u64().unwrap() <= 4_000);
    assert_eq!(pack["budget_mode"], "explicit");
    assert_eq!(pack["budget"], 1_000);
    assert_eq!(pack["requested_budget"], 1_000);
    assert!(pack["targets"].as_array().unwrap().iter().any(|target| {
        target["qualified_name"] == "feature_entry" && target["path"] == "src/lib.rs"
    }));
    let file = pack["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "src/lib.rs")
        .unwrap();
    assert_eq!(file["sha256"].as_str().unwrap().len(), 64);
    assert!(pack["tests"].as_array().unwrap().iter().any(|test| {
        test["target"] == "src/lib.rs::tests::feature_flow" && test["resolved"] == true
    }));
    assert!(pack["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["id"] == "rust-check"));
    let repo_items = pack["repo_map"]["items"].as_array().unwrap();
    assert!(repo_items
        .iter()
        .any(|item| item["qualified_name"] == "feature_entry"));
    assert_eq!(
        pack["provenance_defaults"]["repo_map_symbols"]["precision"],
        "syntax"
    );
    assert!(repo_items
        .iter()
        .all(|item| item.get("precision").is_none()));
    let hot_source = pack["hot_source"].as_array().unwrap();
    assert_eq!(hot_source.len(), 1);
    assert_eq!(hot_source[0]["qualified_name"], "feature_entry");
    assert!(hot_source[0]["body"]["content"]
        .as_str()
        .unwrap()
        .contains("helper();"));
    assert_eq!(
        pack["provenance_defaults"]["hot_source"]["precision"],
        "syntax"
    );
    assert!(hot_source[0].get("precision").is_none());
    assert_eq!(pack["readiness"]["edit"], "ready");
    assert_eq!(pack["readiness"]["verify"], "ready");
    assert_eq!(pack["readiness"]["graph_precision"], "syntax");
    assert_eq!(pack["readiness"]["recommended_edit_tool"], "apply_edits");
    assert_eq!(
        pack["readiness"]["next_actions"],
        json!(["apply_edits", "review_changes", "verify_project"])
    );
    assert!(pack["timing"]["build_ms"].as_u64().is_some());
    assert!(pack["timing"]["software_context_ms"].as_u64().is_some());
    assert!(pack["readiness"]["advisories"]
        .as_array()
        .unwrap()
        .iter()
        .any(|advisory| advisory == "syntax_only_relationships"));
    assert_eq!(pack["repo_map"]["cache_hit"], false);
    assert_eq!(pack["repo_map"]["scope_path"], "src");
    assert_eq!(pack["repo_map"]["files_indexed"], 1);
    assert!(pack["context_bytes_avoided"].as_u64().unwrap() > 0);
    assert!(pack["context_reduction_percent"].as_f64().unwrap() > 20.0);

    let cached = harness
        .agent_context("demo", &workspace, "feature entry", 2_000, &[])
        .unwrap();
    assert_eq!(cached["repo_map"]["cache_hit"], true);
    assert!(cached["estimated_tokens"].as_u64().unwrap() <= 2_000);
    let cached_items = cached["repo_map"]["items"].as_array().unwrap();
    let helper = cached_items
        .iter()
        .find(|item| item["qualified_name"] == "helper")
        .unwrap();
    assert_eq!(helper["reason"], "callee_of_direct");
    assert!(helper["signature"].as_str().unwrap().contains("fn helper"));
    assert!(helper["relationships"]
        .as_array()
        .unwrap()
        .iter()
        .any(|relation| {
            relation["relation"] == "callee_of_direct" && relation["precision"] == "syntax"
        }));

    let adaptive = harness
        .agent_context("demo", &workspace, "feature entry", 0, &[])
        .unwrap();
    assert_eq!(adaptive["budget_mode"], "adaptive");
    assert!(adaptive["requested_budget"].is_null());
    let adaptive_budget = adaptive["budget"].as_u64().unwrap();
    assert!((1_200..=1_800).contains(&adaptive_budget));
    assert!(adaptive["estimated_tokens"].as_u64().unwrap() <= adaptive_budget);
    assert_eq!(adaptive["readiness"]["edit"], "ready");

    let complex_scopes = vec!["customer-domain".to_owned(), "platform-domain".to_owned()];
    let complex = harness
        .agent_context(
            "demo",
            &workspace,
            "feature entry refactor architecture helper compatibility verification cross module behavior reusable boundary performance safety reliability migration",
            0,
            &complex_scopes,
        )
        .unwrap();
    assert_eq!(complex["budget_mode"], "adaptive");
    let complex_budget = complex["budget"].as_u64().unwrap();
    assert!(complex_budget > adaptive_budget);
    assert!(complex_budget <= 4_000);
    assert!(complex["estimated_tokens"].as_u64().unwrap() <= complex_budget);

    let source_sha256 = workspace.read_file("src/lib.rs", 1, None).unwrap().sha256;
    harness
        .graph_provider_import(
            &workspace,
            crate::graph::GraphProviderImport {
                provider: "lsp:fixture".into(),
                precision: GraphPrecision::Semantic,
                revision: "semantic-fixture-1".into(),
                nodes: vec![
                    crate::graph::GraphImportNode {
                        id: "semantic:feature-entry".into(),
                        kind: NodeKind::Function,
                        label: "feature_entry".into(),
                        attributes: BTreeMap::from([
                            ("path".into(), json!("src/lib.rs")),
                            ("source_sha256".into(), json!(source_sha256.clone())),
                            ("name".into(), json!("feature_entry")),
                            ("qualified_name".into(), json!("feature_entry")),
                        ]),
                    },
                    crate::graph::GraphImportNode {
                        id: "semantic:helper".into(),
                        kind: NodeKind::Function,
                        label: "helper".into(),
                        attributes: BTreeMap::from([
                            ("path".into(), json!("src/lib.rs")),
                            ("source_sha256".into(), json!(source_sha256)),
                            ("name".into(), json!("helper")),
                            ("qualified_name".into(), json!("helper")),
                        ]),
                    },
                ],
                edges: vec![crate::graph::GraphImportEdge {
                    from: "semantic:feature-entry".into(),
                    to: "semantic:helper".into(),
                    kind: EdgeKind::Calls,
                }],
            },
        )
        .unwrap();
    let semantic = harness
        .agent_context("demo", &workspace, "feature entry", 2_000, &[])
        .unwrap();
    assert_eq!(semantic["repo_map"]["provider"], "wcode-composite");
    assert_eq!(semantic["repo_map"]["precision"], "semantic");
    assert_eq!(semantic["readiness"]["graph_precision"], "semantic");
    let semantic_helper = semantic["repo_map"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["qualified_name"] == "helper")
        .unwrap();
    assert_eq!(semantic_helper["reason"], "callee_of_direct");
    assert!(semantic_helper["relationships"]
        .as_array()
        .unwrap()
        .iter()
        .any(|relation| {
            relation["provider"] == "lsp:fixture" && relation["precision"] == "semantic"
        }));

    let changed_source = format!(
        "{}\n// semantic provider should become stale after this edit\n",
        fs::read_to_string(root.path().join("src/lib.rs")).unwrap()
    );
    fs::write(root.path().join("src/lib.rs"), changed_source).unwrap();
    let stale_semantic = harness
        .agent_context("demo", &workspace, "feature entry", 2_000, &[])
        .unwrap();
    assert_eq!(stale_semantic["repo_map"]["provider"], "tree-sitter");
    assert_eq!(stale_semantic["repo_map"]["precision"], "syntax");
    assert_eq!(stale_semantic["readiness"]["graph_precision"], "syntax");

    let read_only_workspace = Workspace::new(root.path(), false, true).unwrap();
    let read_only = harness
        .agent_context("demo", &read_only_workspace, "feature entry", 2_000, &[])
        .unwrap();
    assert_eq!(read_only["project"]["write_enabled"], false);
    assert_eq!(read_only["readiness"]["edit"], "read_only_workspace");
    assert_eq!(read_only["readiness"]["next_actions"], json!([]));
    assert!(read_only["readiness"]["advisories"]
        .as_array()
        .unwrap()
        .iter()
        .any(|advisory| advisory == "workspace_write_disabled"));
}

#[test]
fn output_tail_is_bounded_and_keeps_the_end() {
    let (tail, truncated) = tail_chars("abcdefgh", 5);
    assert!(truncated);
    assert_eq!(tail, "…efgh");
}
