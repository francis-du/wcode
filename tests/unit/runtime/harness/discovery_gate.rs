use super::*;
use crate::evidence::{EvidenceAuthority, EvidenceResult};
use crate::monitor::TaskMonitor;
use std::{fs, path::Path, process::Command};

/// Fixture-only deadline for real Git and static-check work. A loaded CI runner
/// needs more headroom than a warm laptop; the production wait caps are
/// unchanged, so this only decides when the fixture reports a stalled check.
fn fixture_timeout_seconds() -> u64 {
    if cfg!(windows) {
        180
    } else {
        90
    }
}

/// `verify_project_mode` retries a saturated probe semaphore itself, so these
/// cases call it directly and keep exercising one honest verification attempt.
async fn verify_project_once(
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &Workspace,
    mode: (&str, bool),
    timeout_seconds: u64,
    monitor: &TaskMonitor,
) -> Result<VerificationReport> {
    harness
        .verify_project_mode(workspace_id, workspace, mode, timeout_seconds, monitor)
        .await
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
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
}

#[tokio::test]
async fn partial_discovery_cannot_pass_native_verification() {
    for depth_partial in [false, true] {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("README.md"), "# Discovery fixture\n").unwrap();
        fs::write(root.path().join(".gitignore"), ".wcode/\n").unwrap();
        let incomplete_path = if depth_partial {
            let directory = root.path().join("deep/".repeat(10));
            fs::create_dir_all(&directory).unwrap();
            let path = directory.join("package.json");
            fs::write(&path, r#"{"scripts":{"test":"node -e 'process.exit(1)'"}}"#).unwrap();
            path
        } else {
            let path = root.path().join("codegen.yaml");
            fs::write(&path, "schema: [\n").unwrap();
            path
        };
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["add", "."]);
        git(
            root.path(),
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "--no-verify",
                "-m",
                "discovery",
            ],
        );
        let workspace = Workspace::new(root.path(), true, true).unwrap();
        let harness = ToolHarness::new(2).unwrap();
        let monitor = TaskMonitor::new(["discovery".to_owned()]);
        let (profile, _) = harness.load_project_profile(&workspace).unwrap();
        assert!(!profile.discovery.complete);
        let reason = if depth_partial {
            "scan_depth_unknown"
        } else {
            "profile_source_invalid"
        };
        assert!(profile
            .discovery
            .reasons
            .iter()
            .any(|value| value == reason));
        // The whole test suite shares one process-wide probe semaphore, so a
        // sibling test can hold every slot and fail a real-Git probe without
        // starting it. That is a scheduling artifact, not the behaviour under
        // test, so wait for the capacity to drain instead of failing the case.
        let review_deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let review = loop {
            match harness
                .review_changes("discovery", &workspace, fixture_timeout_seconds(), &monitor)
                .await
            {
                Ok(report) => break report,
                Err(error) if error.to_string().contains("capacity remained busy") => {
                    assert!(
                        std::time::Instant::now() < review_deadline,
                        "change review stayed saturated: {error}"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(1_000)).await;
                }
                Err(error) => panic!("change review failed: {error}"),
            }
        };
        let plan = harness
            .verification_plan("discovery", &workspace, &review)
            .unwrap();
        let required = plan.required_checks.as_ref().unwrap();
        assert!(required
            .iter()
            .any(|check| check.id == "profile-discovery-completeness"));
        assert!(required.iter().any(|check| check.id == "git-diff-check"));

        for (level, fail_fast) in [("quick", true), ("full", false)] {
            let report = verify_project_once(
                &harness,
                "discovery",
                &workspace,
                (level, fail_fast),
                fixture_timeout_seconds(),
                &monitor,
            )
            .await
            .unwrap();
            assert!(!report.passed, "{depth_partial}: {report:?}");
            let independent = report
                .checks
                .iter()
                .find(|check| check.id == "git-diff-check")
                .unwrap();
            assert!(independent.success);
            assert!(
                !independent.reused,
                "a real independent Git check must execute"
            );
            assert_eq!(independent.exit_code, Some(0));
            let blocker = report
                .checks
                .iter()
                .find(|check| check.id == "profile-discovery-completeness")
                .unwrap();
            assert!(!blocker.success);
            assert!(!blocker.reused);
            assert_eq!(
                blocker.exit_code, None,
                "metadata completeness is not an external process"
            );
            assert!(blocker.stderr_tail.contains(reason));
            assert_eq!(
                blocker.execution,
                crate::evidence::VerificationCheckExecution::Executed,
                "a deterministic native metadata evaluation did run, without a process exit code"
            );
            assert!(!report.skipped_checks.iter().any(|id| id == &blocker.id));
            let proof = crate::evidence_store::load(&workspace).unwrap();
            assert!(proof
                .iter()
                .any(|record| record.subject == "verification:git-diff-check"
                    && record.authority == EvidenceAuthority::NativeVerification
                    && record.result == EvidenceResult::Pass));
            let aggregate = proof
                .iter()
                .find(|record| {
                    record.producer == "verify_project"
                        && record.policy.as_deref()
                            == Some(format!("deterministic/{level}/v2").as_str())
                        && record.execution_receipt.is_some()
                })
                .unwrap();
            assert_eq!(aggregate.result, EvidenceResult::Fail);
            let receipt = aggregate.execution_receipt.as_ref().unwrap();
            assert_eq!(receipt.result(), EvidenceResult::Fail);
            assert!(receipt
                .checks
                .iter()
                .any(|check| check.check.id == blocker.id
                    && check.execution == crate::evidence::VerificationCheckExecution::Executed));
            assert!(receipt
                .checks
                .iter()
                .any(|check| check.check.id == blocker.id && check.result == EvidenceResult::Fail));
            let status = harness
                .verification_status("discovery", &workspace, &plan.id)
                .unwrap();
            assert!(!status.ready);
            assert_eq!(
                status.deterministic_result,
                Some(EvidenceResult::Fail),
                "the native discovery failure itself must block readiness"
            );
            assert!(status
                .blockers
                .iter()
                .any(|value| value == "deterministic-verification-failed"));
        }
        if depth_partial {
            fs::remove_dir_all(root.path().join("deep")).unwrap();
        } else {
            fs::remove_file(&incomplete_path).unwrap();
        }
        let (complete, cache_hit) = harness.load_project_profile(&workspace).unwrap();
        assert!(!cache_hit);
        assert!(complete.discovery.complete);
        let recovered = verify_project_once(
            &harness,
            "discovery",
            &workspace,
            ("full", false),
            fixture_timeout_seconds(),
            &monitor,
        )
        .await
        .unwrap();
        assert!(recovered.passed, "{recovered:?}");
        assert!(!recovered
            .checks
            .iter()
            .any(|check| check.id == "profile-discovery-completeness"));
        // The whole test suite shares one process-wide probe semaphore, so a
        // sibling test can hold every slot and fail a real-Git probe without
        // starting it. That is a scheduling artifact, not the behaviour under
        // test, so wait for the capacity to drain instead of failing the case.
        let review_deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let review = loop {
            match harness
                .review_changes("discovery", &workspace, fixture_timeout_seconds(), &monitor)
                .await
            {
                Ok(report) => break report,
                Err(error) if error.to_string().contains("capacity remained busy") => {
                    assert!(
                        std::time::Instant::now() < review_deadline,
                        "change review stayed saturated: {error}"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(1_000)).await;
                }
                Err(error) => panic!("change review failed: {error}"),
            }
        };
        let new_plan = harness
            .verification_plan("discovery", &workspace, &review)
            .unwrap();
        assert!(!new_plan
            .required_checks
            .as_ref()
            .unwrap()
            .iter()
            .any(|check| check.id == "profile-discovery-completeness"));
        let recovered_status = harness
            .verification_status("discovery", &workspace, &new_plan.id)
            .unwrap();
        assert_eq!(
            recovered_status.deterministic_result,
            Some(EvidenceResult::Pass)
        );
    }
}
