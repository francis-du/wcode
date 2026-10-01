use super::*;
use crate::monitor::TaskMonitor;
use crate::verification::change::ExecutionGitBinding;
use std::process::Command;

fn git(root: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn git_fixture() -> (tempfile::TempDir, Workspace, ToolHarness) {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("README.md"), "# Acceptance fixture\n").unwrap();
    fs::write(root.path().join(".gitignore"), "target/\n.wcode/\n").unwrap();
    git(root.path(), &["init", "--quiet"]);
    git(root.path(), &["config", "user.name", "Acceptance Test"]);
    git(
        root.path(),
        &["config", "user.email", "acceptance@example.invalid"],
    );
    git(root.path(), &["add", "."]);
    git(
        root.path(),
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "--no-verify",
            "-m",
            "initial",
        ],
    );
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    (root, workspace, harness)
}

#[tokio::test]
async fn native_verification_receipts_bind_execution_commit_and_preserve_legacy_unknown() {
    let (root, workspace, harness) = git_fixture();
    let monitor = TaskMonitor::new(["demo".into()]);
    let initial_revision = harness.intelligence.current_revision(&workspace).unwrap();
    let first = harness
        .verify_project_mode("demo", &workspace, ("quick", false), 30, &monitor)
        .await
        .unwrap();
    assert!(first.passed, "{first:?}");
    let first_binding = first.execution_git_binding.clone().unwrap();
    assert_eq!(
        first_binding.head_sha,
        git(root.path(), &["rev-parse", "HEAD"])
    );
    let proof = crate::evidence_store::load(&workspace).unwrap();
    assert!(proof
        .iter()
        .any(|record| record.subject.starts_with("verification:")));
    for record in proof
        .iter()
        .filter(|record| record.execution_receipt.is_some())
    {
        assert_eq!(record.execution_git_binding.as_ref(), Some(&first_binding));
        assert_eq!(
            record
                .execution_receipt
                .as_ref()
                .unwrap()
                .execution_git_binding
                .as_ref(),
            Some(&first_binding)
        );
    }

    git(
        root.path(),
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "--no-verify",
            "--allow-empty",
            "-m",
            "same bytes new revision",
        ],
    );
    assert_eq!(
        harness.intelligence.current_revision(&workspace).unwrap(),
        initial_revision
    );
    let second = harness
        .verify_project_mode("demo", &workspace, ("quick", false), 30, &monitor)
        .await
        .unwrap();
    assert!(second.passed, "{second:?}");
    assert_ne!(
        second.execution_git_binding.as_ref().unwrap().head_sha,
        first_binding.head_sha
    );
    assert_eq!(second.checks_reused, 0);
    let current = crate::evidence_store::load(&workspace).unwrap();
    assert!(current.iter().any(|record| record
        .execution_receipt
        .as_ref()
        .is_some_and(|receipt| receipt.execution_git_binding == second.execution_git_binding)));
    // Reading an old receipt never infers today's HEAD.
    let mut conflict = proof
        .iter()
        .find(|record| record.execution_receipt.is_some())
        .unwrap()
        .clone();
    conflict.execution_git_binding = Some(fake_binding('2'));
    assert!(conflict.validate().is_err());
    assert!(crate::evidence_store::persist(&workspace, &conflict).is_err());
    let mut json = serde_json::to_value(
        proof
            .iter()
            .find_map(|record| record.execution_receipt.as_ref())
            .unwrap(),
    )
    .unwrap();
    json.as_object_mut()
        .unwrap()
        .remove("execution_git_binding");
    let legacy: crate::evidence::VerificationExecutionReceipt =
        serde_json::from_value(json).unwrap();
    assert!(legacy.execution_git_binding.is_none());
}

#[tokio::test]
async fn language_quality_git_metadata_is_retained_without_fake_full_receipt() {
    let (root, workspace, harness) = git_fixture();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"quality_git_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn value() -> usize {\n    1\n}\n",
    )
    .unwrap();
    git(root.path(), &["add", "."]);
    git(
        root.path(),
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "--no-verify",
            "-m",
            "quality fixture",
        ],
    );
    let binding = harness
        .execution_git_binding(&workspace)
        .await
        .unwrap()
        .unwrap();
    let run = harness
        .language_quality_run(
            "demo",
            &workspace,
            crate::semantic_provider::SemanticLanguage::Rust,
            "rustfmt",
            30,
        )
        .await
        .unwrap();
    assert!(run.success, "{run:?}");
    let proof = crate::evidence_store::load(&workspace).unwrap();
    let record = proof
        .iter()
        .find(|record| record.subject.starts_with("verification:quality-"))
        .expect("the observed language-quality command must persist Evidence");
    assert_eq!(record.execution_git_binding.as_ref(), Some(&binding));
    assert!(
        record.execution_receipt.is_none(),
        "a language provider does not prove the full project matrix"
    );
}

fn fake_binding(head: char) -> ExecutionGitBinding {
    ExecutionGitBinding {
        repository: format!("sha256:{}", "a".repeat(64)),
        head_sha: head.to_string().repeat(40),
        tree_sha: "b".repeat(40),
        dirty: false,
        index_fingerprint: format!("sha256:{}", "c".repeat(64)),
    }
}

#[test]
fn execution_git_identity_separates_static_reuse_and_inflight_runs() {
    let (_root, workspace, harness) = verification_reuse::workspace_fixture();
    let spec = verification_reuse::static_check("rust-check", &["check", "--locked"]);
    let revision = harness.intelligence.current_revision(&workspace).unwrap();
    let first_binding = Some(fake_binding('1'));
    let context = harness_verification_cache::VerificationReuseContext::new(
        &workspace, &revision, "quick", true, 30,
    )
    .with_git_binding(first_binding.clone());
    let mut report =
        verification_reuse::report(verification_reuse::check_result(&spec, true), true);
    report.execution_git_binding = first_binding.clone();
    harness
        .intelligence
        .record_verification_report("demo", &workspace, &revision, &report)
        .unwrap();
    harness.cache_successful_verification_checks(
        &workspace,
        &context,
        std::slice::from_ref(&spec),
        &report,
    );
    let proof = crate::evidence_store::load(&workspace).unwrap();
    assert!(harness
        .cached_verification_check(&workspace, &context, &spec, &proof)
        .is_some());
    let _first = harness.claim_verification_run(&workspace, &context);
    for binding in [
        None,
        Some(fake_binding('2')),
        Some(ExecutionGitBinding {
            dirty: true,
            ..fake_binding('1')
        }),
        Some(ExecutionGitBinding {
            index_fingerprint: format!("sha256:{}", "d".repeat(64)),
            ..fake_binding('1')
        }),
    ] {
        harness.cache_successful_verification_checks(
            &workspace,
            &context,
            std::slice::from_ref(&spec),
            &report,
        );
        assert!(harness
            .cached_verification_check(&workspace, &context, &spec, &proof)
            .is_some());
        let changed = harness_verification_cache::VerificationReuseContext::new(
            &workspace, &revision, "quick", true, 30,
        )
        .with_git_binding(binding);
        assert!(matches!(
            harness.claim_verification_run(&workspace, &changed),
            harness_verification_cache::VerificationRunClaim::Leader(_)
        ));
        assert!(harness
            .cached_verification_check(&workspace, &changed, &spec, &proof)
            .is_none());
    }
}

#[tokio::test]
async fn execution_git_source_mismatch_cannot_be_rebound_by_reused_report() {
    let (_root, workspace, harness) = verification_reuse::workspace_fixture();
    let spec = verification_reuse::static_check("rust-check", &["check", "--locked"]);
    let revision = harness.intelligence.current_revision(&workspace).unwrap();
    let binding = Some(fake_binding('1'));
    let context = harness_verification_cache::VerificationReuseContext::new(
        &workspace, &revision, "quick", true, 30,
    )
    .with_git_binding(binding.clone());
    let mut report =
        verification_reuse::report(verification_reuse::check_result(&spec, true), true);
    report.execution_git_binding = binding;
    harness
        .intelligence
        .record_verification_report("demo", &workspace, &revision, &report)
        .unwrap();
    harness.cache_successful_verification_checks(
        &workspace,
        &context,
        std::slice::from_ref(&spec),
        &report,
    );
    let proof = crate::evidence_store::load(&workspace).unwrap();
    let reused = harness
        .cached_verification_check(&workspace, &context, &spec, &proof)
        .unwrap();
    let mut rebound = verification_reuse::report(reused, true);
    rebound.execution_git_binding = Some(fake_binding('2'));
    let error = harness
        .intelligence
        .record_verification_report("demo", &workspace, &revision, &rebound)
        .unwrap_err();
    assert!(error.to_string().contains("source evidence"));
}

#[tokio::test]
async fn git_probe_failure_prevents_native_verification_evidence() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("README.md"), "# Unborn repository\n").unwrap();
    git(root.path(), &["init", "--quiet"]);
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    let monitor = TaskMonitor::new(["demo".into()]);
    assert!(harness
        .verify_project_mode("demo", &workspace, ("quick", false), 30, &monitor)
        .await
        .is_err());
    assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
}

#[tokio::test]
async fn execution_git_drift_during_real_check_cannot_publish_current_evidence() {
    for index_only in [false, true] {
        let (root, workspace, harness) = git_fixture();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname = \"git_receipt_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            root.path().join("Cargo.lock"),
            "version = 3\n[[package]]\nname = \"git_receipt_fixture\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            "pub fn value() -> usize {\n    1\n}\n",
        )
        .unwrap();
        fs::write(
            root.path().join("build.rs"),
            r#"fn main() {
    std::fs::write(".git/check-started", "").unwrap();
    for _ in 0..400 {
        if std::path::Path::new(".git/check-continue").exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("test coordinator did not release check");
}
"#,
        )
        .unwrap();
        git(root.path(), &["add", "."]);
        git(
            root.path(),
            &[
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "--no-verify",
                "-m",
                "verification fixture",
            ],
        );
        let revision = harness.intelligence.current_revision(&workspace).unwrap();
        let running_harness = harness.clone();
        let running_workspace = workspace.clone();
        let running = tokio::spawn(async move {
            let monitor = TaskMonitor::new(["demo".into()]);
            running_harness
                .verify_project_mode("demo", &running_workspace, ("quick", false), 30, &monitor)
                .await
        });
        let mut started = false;
        for _ in 0..300 {
            if root.path().join(".git/check-started").exists() {
                started = true;
                break;
            }
            if running.is_finished() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        if !started {
            let result = running.await.unwrap();
            panic!("real cargo check did not reach barrier: {result:?}");
        }
        if index_only {
            git(root.path(), &["update-index", "--chmod=+x", "README.md"]);
        } else {
            git(
                root.path(),
                &[
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "--quiet",
                    "--no-verify",
                    "--allow-empty",
                    "-m",
                    "concurrent revision",
                ],
            );
        }
        assert_eq!(
            harness.intelligence.current_revision(&workspace).unwrap(),
            revision
        );
        fs::write(root.path().join(".git/check-continue"), "").unwrap();
        let error = running.await.unwrap().unwrap_err();
        assert!(
            error.to_string().contains("Git identity changed"),
            "{error}"
        );
        assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
    }
}
