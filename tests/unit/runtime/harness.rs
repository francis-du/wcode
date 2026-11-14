use super::*;
use crate::monitor::TaskMonitor;
use std::fs;

#[path = "harness/admission.rs"]
mod admission;
#[path = "harness/agent_context_pack.rs"]
mod agent_context_pack;
#[path = "harness/anchors.rs"]
mod anchors;
#[path = "harness/context_discovery.rs"]
mod context_discovery;
#[path = "harness/context_rank.rs"]
mod context_rank;
#[path = "harness/contracts.rs"]
mod contracts;
#[path = "harness/convention_cache.rs"]
mod convention_cache;
#[path = "harness/cost.rs"]
mod cost;
#[path = "harness/experience.rs"]
mod experience;
#[path = "harness/focus.rs"]
mod focus;
#[path = "harness/parallel_liveness.rs"]
mod parallel_liveness;
#[path = "harness/repo_map.rs"]
mod repo_map;
#[path = "harness/review_generated.rs"]
mod review_generated;
#[path = "harness/selective.rs"]
mod selective;
#[path = "harness/steering.rs"]
mod steering;
#[path = "harness/verification_git.rs"]
mod verification_git;
#[path = "harness/verification_reuse.rs"]
mod verification_reuse;

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
    assert_eq!(status.constraints, 4);
    assert_eq!(status.acceptance_criteria, 0);
    assert_eq!(status.decisions, 0);
    let constraints = fs::read_to_string(root.path().join(".wcode/design/constraints.yaml"))
        .expect("baseline constraints are persisted");
    assert!(constraints.contains("CONSTRAINT-SOURCE-DECOMPOSITION"));
    assert!(constraints.contains("CONSTRAINT-TEST-ROOT"));
    assert!(constraints.contains("CONSTRAINT-DESIGN-SYNC"));
    assert!(constraints.contains("CONSTRAINT-PARALLEL-FIRST"));
    assert!(harness
        .design_init("demo", &workspace, "Other", "")
        .is_err());
}

#[test]
fn rust_clippy_gate_matches_ci_for_locked_and_unlocked_projects() {
    let workflow = include_str!("../../../.github/workflows/release.yml");
    for locked in [false, true] {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname = \"clippy-gate\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        if locked {
            fs::write(root.path().join("Cargo.lock"), "# fixture\n").unwrap();
        }
        let workspace = Workspace::new(root.path(), false, false).unwrap();
        let harness = ToolHarness::new(2).unwrap();
        for _ in 0..2 {
            let context = harness.project_context("demo", &workspace).unwrap();
            let check = context
                .recommended_checks
                .iter()
                .find(|check| check.id == "rust-clippy")
                .unwrap();
            let command = format!("{} {}", check.program, check.args.join(" "));
            let expected = format!(
                "cargo clippy{} --all-targets -- -D warnings",
                if locked { " --locked" } else { "" }
            );
            assert_eq!(command, expected);
            assert_eq!(check.level, "full");
            assert_eq!(check.phase, 2);
            if locked {
                assert!(workflow.contains(&format!("run: {command}")));
            }
        }
    }
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
        format!(
            "pub fn large_target() {{}}\n{}",
            "// fixture\n".repeat(2_000)
        ),
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
        .any(|step| step.contains("agent_context") && step.contains("readiness")));
    assert!(first
        .workflow
        .iter()
        .any(|step| step.contains("semantic_navigation") && step.contains("warm")));
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
    let oversized = first
        .conventions
        .findings
        .iter()
        .find(|finding| finding.code == "oversized-source-module")
        .expect("oversized source is a deterministic core-policy violation");
    assert_eq!(
        oversized.severity,
        crate::conventions::ConventionSeverity::Error
    );
    let core_gate = harness_quality::core_policy_check(&first.conventions)
        .expect("core policy gate must fail on deterministic convention errors");
    assert_eq!(core_gate.id, "core-policy");
    assert!(!core_gate.success);
    let agent = harness
        .agent_context(
            "demo",
            &workspace,
            "change large_target behavior",
            4_000,
            &[],
        )
        .unwrap();
    assert!(agent["core_constraints"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| {
            item["id"] == "CONSTRAINT-SOURCE-DECOMPOSITION"
                && item["rule"]
                    .as_str()
                    .is_some_and(|rule| rule.contains("<=1000 lines"))
        })));
    assert!(agent["core_constraints"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| {
            item["id"] == "CONSTRAINT-PARALLEL-FIRST"
                && item["rule"]
                    .as_str()
                    .is_some_and(|rule| rule.contains("bulk tools"))
        })));
    assert_eq!(agent["conventions"]["errors"], 1);
    assert!(agent["conventions"]["findings"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| {
            item["code"] == "oversized-source-module"
                && item["path"] == "src/large_module.rs"
                && item["severity"] == "error"
        })));
    assert_eq!(agent["project"]["source_line_limit"], 1_000);
    let oversized_target = agent["files"]
        .as_array()
        .and_then(|files| {
            files
                .iter()
                .find(|file| file["path"] == "src/large_module.rs")
        })
        .expect("direct oversized source target must remain visible in the edit-ready pack");
    assert_eq!(oversized_target["source_oversized"], true);
    assert!(oversized_target["source_lines"]
        .as_u64()
        .is_some_and(|lines| lines > 1_000));
    assert_eq!(agent["readiness"]["verify"], "blocked_by_core_policy");
    assert_eq!(agent["readiness"]["hard_constraint_violations"], 1);
    assert_eq!(agent["readiness"]["oversized_target_files"], 1);
    assert_eq!(agent["readiness"]["change_strategy"], "localized_refactor");
    assert!(agent["readiness"]["advisories"]
        .as_array()
        .is_some_and(|items| items
            .iter()
            .any(|item| item == "oversized_target_requires_decomposition")));
    assert!(agent["readiness"]["advisories"]
        .as_array()
        .is_some_and(|items| items
            .iter()
            .any(|item| item == "hard_convention_violations")));
    assert!(agent["readiness"]["next_actions"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item == "reconciliation_plan")));
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
    assert!(rust_quality.providers.iter().any(|provider| {
        provider.id == "rust-clippy"
            && provider.command == "cargo clippy --all-targets -- -D warnings"
            && provider.check_only
    }));
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

    let still_oversized = root.path().join("src/runtime/still_large.rs");
    fs::write(&still_oversized, "fn fixture() {}\n".repeat(1_200)).unwrap();
    let mut growth_findings = Vec::new();
    append_maintainability_findings(
        &workspace,
        &[ChangedFileReview {
            path: "src/runtime/still_large.rs".into(),
            status: "modified".into(),
            staged: false,
            unstaged: true,
            untracked: false,
            category: "source".into(),
            additions: Some(25),
            deletions: Some(5),
            binary: false,
            risk_reasons: vec![],
        }],
        &mut growth_findings,
    );
    assert!(growth_findings
        .iter()
        .any(|finding| finding.code == "maintainability-oversized-source-growth"));
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
    // The 5s bound is the deadlock watchdog, not a capacity assertion. Under
    // full-suite parallelism the global probe semaphore can be saturated by real
    // cargo processes long enough to trip PROCESS_QUEUE_WAIT_CAP, so retry once
    // before calling it a deadlock.
    let review = || harness.review_changes(workspace_id.clone(), &workspace, 30, &monitor);
    let report = match tokio::time::timeout(std::time::Duration::from_secs(5), review()).await {
        Ok(result) => result.unwrap(),
        Err(_) => tokio::time::timeout(std::time::Duration::from_secs(5), review())
            .await
            .expect("review probes must not deadlock with one semaphore slot")
            .unwrap(),
    };

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

#[path = "harness/observatory.rs"]
mod observatory;

#[test]
fn output_tail_is_bounded_and_keeps_the_end() {
    let (tail, truncated) = tail_chars("abcdefgh", 5);
    assert!(truncated);
    assert_eq!(tail, "…efgh");
}
