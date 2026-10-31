use super::*;
use std::fs;
use std::process::Command;

// Real Cargo checks in two manifest islands share one Wcode Workspace. The
// first build script holds a bounded barrier; the sibling must not reserve a
// tool slot while waiting behind the same artifact writer. No sleeps emulate
// compilation and no production cache is cleared.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn verification_phase_real_cargo_siblings_wait_outside_tool_capacity() {
    real_cargo_phase(Duration::ZERO).await;
}

// Exercise the real thirty-second boundary, not only a short scheduling probe.
// This uses dependency-free disposable Cargo projects, never clears a user cache,
// and does not change the production external contention cap.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn verification_phase_cold_sibling_survives_external_cargo_wait_cap() {
    real_cargo_phase(Duration::from_secs(32)).await;
}

async fn real_cargo_phase(hold: Duration) {
    let root = tempfile::tempdir().unwrap();
    for (relative, name) in [(".", "phase_root"), ("apps/console", "phase_console")] {
        let directory = root.path().join(relative);
        fs::create_dir_all(directory.join("src")).unwrap();
        fs::create_dir_all(directory.join("target")).unwrap();
        fs::write(directory.join("Cargo.toml"), format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\nexclude = [\"apps/console\"]\n"
        )).unwrap();
        fs::write(
            directory.join("Cargo.lock"),
            format!("version = 3\n[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
        )
        .unwrap();
        fs::write(
            directory.join("src/lib.rs"),
            "pub fn value() -> usize {\n    1\n}\n",
        )
        .unwrap();
        fs::write(
            directory.join("build.rs"),
            r#"fn main() {
    std::fs::write("target/phase-started", "").unwrap();
    for _ in 0..3600 {
        if std::path::Path::new("target/phase-continue").exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("test coordinator did not release check");
}
"#,
        )
        .unwrap();
    }
    fs::write(root.path().join(".gitignore"), "target/\n").unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["add", "."],
        vec![
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
            "fixture",
        ],
    ] {
        let result = Command::new("git")
            .args(args)
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(8).unwrap();
    let runner = harness.clone();
    let mut running = tokio::spawn(async move {
        let monitor = crate::monitor::TaskMonitor::new(["phase-fixture".into()]);
        runner
            .verify_project_mode("phase-fixture", &workspace, ("quick", false), 60, &monitor)
            .await
    });
    let islands = [root.path().to_path_buf(), root.path().join("apps/console")];
    let mut started = false;
    for _ in 0..400 {
        if islands
            .iter()
            .any(|path| path.join("target/phase-started").exists())
        {
            started = true;
            break;
        }
        if running.is_finished() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    // Other independent static checks may still be finishing. Observe only this
    // Harness, and release the child barriers before asserting any result.
    let mut occupied = harness.admission_snapshot().execution_in_use;
    if started {
        for _ in 0..100 {
            occupied = harness.admission_snapshot().execution_in_use;
            if occupied == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    let held_from = Instant::now();
    if started && !hold.is_zero() {
        tokio::time::sleep(hold).await;
    }
    let occupied_after_hold = harness.admission_snapshot().execution_in_use;
    for path in &islands {
        fs::write(path.join("target/phase-continue"), "").unwrap();
    }
    let result = match tokio::time::timeout(Duration::from_secs(30), &mut running).await {
        Ok(result) => result.unwrap(),
        Err(_) => {
            running.abort();
            let _ = running.await;
            panic!("released Cargo siblings exceeded their bounded fixture deadline");
        }
    };
    assert!(
        started,
        "real Cargo never reached the controlled barrier: {result:?}"
    );
    let report = result.unwrap();
    assert!(report.passed, "{report:?}");
    if !hold.is_zero() {
        assert!(held_from.elapsed() >= hold);
        assert_eq!(
            occupied_after_hold, 1,
            "a queued sibling consumed capacity or its running predecessor ended early"
        );
    }
    assert_eq!(
        occupied, 1,
        "a sibling Cargo writer consumed tool capacity while queued"
    );
    let cargo_checks: Vec<_> = report
        .checks
        .iter()
        .filter(|check| check.id.ends_with("rust-check"))
        .collect();
    assert_eq!(
        cargo_checks.len(),
        2,
        "both native manifest islands must be checked: {report:?}"
    );
    assert!(cargo_checks
        .iter()
        .all(|check| check.success && !check.reused));
    assert!(islands
        .iter()
        .all(|path| path.join("target/phase-started").exists()));
    assert_eq!(harness.admission_snapshot().execution_in_use, 0);
}
