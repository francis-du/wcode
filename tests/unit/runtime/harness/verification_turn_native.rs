use super::*;
use std::fs;
use std::process::Command;
use std::sync::{Condvar, Mutex};

use crate::runtime_telemetry::{TaskTelemetry, TaskTelemetryTicket};

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

// A real completed static check keeps its own admission permit until this test
// explicitly releases its telemetry completion. The legacy sampler therefore
// observes lawful non-Cargo work for its entire one-second polling window.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn verification_phase_waits_for_independent_static_checks_before_counting_cargo() {
    real_cargo_phase_with_static_gate(Duration::ZERO, true).await;
}

async fn real_cargo_phase(hold: Duration) {
    real_cargo_phase_with_static_gate(hold, false).await;
}

async fn real_cargo_phase_with_static_gate(hold: Duration, pause_static: bool) {
    // Cold Windows toolchains need bounded startup/cleanup headroom. These
    // fixture-only deadlines leave the real hold and production wait cap intact.
    let startup_timeout = Duration::from_secs(if cfg!(windows) { 60 } else { 10 });
    let check_timeout = if cfg!(windows) { 180 } else { 60 };
    let finish_timeout = Duration::from_secs(if cfg!(windows) { 60 } else { 30 });
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
    let static_checks: Vec<String> = harness
        .load_project_profile(&workspace)
        .unwrap()
        .0
        .recommended_checks
        .iter()
        .filter(|check| check.level == "quick" && !check.id.ends_with("rust-check"))
        .map(|check| format!("verify:{}", check.id))
        .collect();
    assert!(!static_checks.is_empty());
    let monitor = crate::monitor::TaskMonitor::new(["phase-fixture".into()]);
    let static_gate = Arc::new(StaticCompletionGate::default());
    let _release_static = ReleaseStaticGate(Arc::clone(&static_gate));
    let runner_monitor = PhaseFixtureMonitor {
        inner: monitor.clone(),
        gate: pause_static.then(|| Arc::clone(&static_gate)),
        deadline: Duration::from_secs(check_timeout),
    };
    let runner = harness.clone();
    let mut running = tokio::spawn(async move {
        runner
            .verify_project_mode(
                "phase-fixture",
                &workspace,
                ("quick", false),
                check_timeout,
                &runner_monitor,
            )
            .await
    });
    let islands = [root.path().to_path_buf(), root.path().join("apps/console")];
    let mut started = false;
    let startup_deadline = Instant::now() + startup_timeout;
    while Instant::now() < startup_deadline {
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
    // The hold starts at the real Cargo barrier, including time spent waiting
    // for independent static checks. Their completion must be observed before
    // attributing this Harness's entire admission count to Cargo writers.
    // Reuse the existing startup bound; max(startup_timeout, hold) stays below
    // the build script's unchanged ninety-second barrier lifetime.
    let held_from = Instant::now();
    let settle_deadline = held_from + startup_timeout;
    let mut legacy_occupied = None;
    if started && pause_static {
        while !static_gate.entered() && Instant::now() < settle_deadline {
            if running.is_finished() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if static_gate.entered() {
            // Run the old sampling rule while the real static permit is held.
            // Its release is a handshake after sampling, not a timing guess.
            for _ in 0..100 {
                let occupied = harness.admission_snapshot().execution_in_use;
                legacy_occupied = Some(occupied);
                if occupied == 1 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }
    static_gate.release();
    let mut occupied = harness.admission_snapshot().execution_in_use;
    let mut static_finished = false;
    if started {
        loop {
            static_finished = static_checks_finished(&monitor, &static_checks);
            occupied = harness.admission_snapshot().execution_in_use;
            if (static_finished && occupied == 1)
                || running.is_finished()
                || Instant::now() >= settle_deadline
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    if started {
        tokio::time::sleep(hold.saturating_sub(held_from.elapsed())).await;
    }
    let occupied_after_hold = harness.admission_snapshot().execution_in_use;
    for path in &islands {
        fs::write(path.join("target/phase-continue"), "").unwrap();
    }
    let result = match tokio::time::timeout(finish_timeout, &mut running).await {
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
    assert!(
        static_finished,
        "independent static checks did not finish: {}",
        monitor.observatory_activity("phase-fixture")
    );
    if pause_static {
        assert!(
            legacy_occupied.is_some_and(|occupied| occupied > 1),
            "the controlled static check must falsify the old Cargo-only sampler"
        );
    }
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

fn static_checks_finished(monitor: &crate::monitor::TaskMonitor, expected: &[String]) -> bool {
    let observed = monitor.observatory_activity("phase-fixture");
    !observed["recent_truncated"].as_bool().unwrap_or(true)
        && observed["recent"].as_array().is_some_and(|tasks| {
            expected.iter().all(|tool| {
                tasks.iter().any(|task| {
                    task["tool"].as_str() == Some(tool.as_str())
                        && matches!(task["status"].as_str(), Some("completed" | "failed"))
                })
            })
        })
}

#[derive(Default)]
struct StaticCompletionGate {
    state: Mutex<(bool, bool)>,
    changed: Condvar,
}

impl StaticCompletionGate {
    fn entered(&self) -> bool {
        self.state.lock().unwrap().0
    }

    fn wait(&self, deadline: Duration) {
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        let (state, _) = self
            .changed
            .wait_timeout_while(state, deadline, |state| !state.1)
            .unwrap();
        let released = state.1;
        drop(state);
        assert!(released, "controlled static completion was not released");
    }

    fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.changed.notify_all();
    }
}

struct ReleaseStaticGate(Arc<StaticCompletionGate>);

impl Drop for ReleaseStaticGate {
    fn drop(&mut self) {
        self.0.release();
    }
}

#[derive(Clone)]
struct PhaseFixtureMonitor {
    inner: crate::monitor::TaskMonitor,
    gate: Option<Arc<StaticCompletionGate>>,
    deadline: Duration,
}

struct PhaseFixtureTicket {
    inner: crate::monitor::TaskTicket,
    gate: Option<Arc<StaticCompletionGate>>,
    deadline: Duration,
}

impl TaskTelemetry for PhaseFixtureMonitor {
    type Ticket = PhaseFixtureTicket;

    fn queue(
        &self,
        workspace: impl Into<String>,
        tool: impl Into<String>,
        detail: impl Into<String>,
        request_bytes: u64,
    ) -> Self::Ticket {
        let tool = tool.into();
        PhaseFixtureTicket {
            gate: if tool == "verify:git-diff-check" {
                self.gate.clone()
            } else {
                None
            },
            inner: self.inner.queue(workspace, tool, detail, request_bytes),
            deadline: self.deadline,
        }
    }

    fn record_intelligence_result(&self, workspace: &str, tool: &str, value: &serde_json::Value) {
        self.inner
            .record_intelligence_result(workspace, tool, value);
    }
}

impl TaskTelemetryTicket for PhaseFixtureTicket {
    fn start(&self) {
        self.inner.start();
    }

    fn finish(self, success: bool, response_bytes: u64) {
        if let Some(gate) = self.gate {
            gate.wait(self.deadline);
        }
        self.inner.finish(success, response_bytes);
    }
}
