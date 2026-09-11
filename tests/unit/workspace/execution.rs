use super::*;
use std::sync::OnceLock;

fn command_fixture() -> (tempfile::TempDir, Workspace, String) {
    static BUILT: OnceLock<tempfile::TempDir> = OnceLock::new();
    let built = BUILT.get_or_init(|| {
        let root = tempfile::tempdir().unwrap();
        let source = r#"
use std::io::{self, Write};
fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("large") => {
            println!("stdout-start");
            eprintln!("stderr-start");
            for _ in 0..40000 {
                println!("ordinary progress line");
                eprintln!("ordinary diagnostic line");
            }
            println!("stdout-final-summary");
            eprintln!("stderr-final-error");
            std::process::exit(7);
        }
        Some("timeout") => {
            std::fs::write("effect.txt", "already-applied").unwrap();
            println!("stdout-before-timeout");
            eprintln!("stderr-before-timeout");
            io::stdout().flush().unwrap();
            io::stderr().flush().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(30));
            std::fs::write("late-effect.txt", "must-not-run").unwrap();
        }
        Some("cancel") => {
            println!("started");
            io::stdout().flush().unwrap();
            std::fs::write("started.txt", "started").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1000));
            std::fs::write("late-effect.txt", "must-not-run").unwrap();
        }
        Some("redact") => {
            println!("password=synthetic-fixture-value");
            io::stdout().flush().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
        _ => {
            println!("normal-output");
            eprintln!("normal-diagnostic");
        }
    }
}
"#;
        std::fs::write(root.path().join("fixture.rs"), source).unwrap();
        let output = std::process::Command::new("rustc")
            .arg("fixture.rs")
            .arg("-o")
            .arg(format!("fixture{}", std::env::consts::EXE_SUFFIX))
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        root
    });
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("bin")).unwrap();
    let name = format!("fixture{}", std::env::consts::EXE_SUFFIX);
    let program = format!("bin/{name}");
    std::fs::copy(built.path().join(name), root.path().join(&program)).unwrap();
    // Trust applies only to this synthetic temporary Workspace, never the host.
    let workspace = Workspace::new_with_security(
        root.path(),
        true,
        true,
        WorkspaceSecurity {
            allow_risky_exec: true,
            ..WorkspaceSecurity::default()
        },
    )
    .unwrap();
    (root, workspace, program)
}

#[tokio::test]
async fn timed_out_command_returns_partial_diagnostics_without_replaying_effects() {
    let (root, workspace, program) = command_fixture();
    let result = workspace
        .run_trusted_runtime_command(&program, &["timeout".into()], ".", 2)
        .await
        .expect("timeout must preserve a failed command result and its captured diagnostics");
    assert!(!result.success);
    assert!(result.stdout.contains("stdout-before-timeout"));
    assert!(result.stderr.contains("stderr-before-timeout"));
    assert_eq!(
        std::fs::read_to_string(root.path().join("effect.txt")).unwrap(),
        "already-applied"
    );
    assert!(!root.path().join("late-effect.txt").exists());
    let value = serde_json::to_value(result).unwrap();
    assert_eq!(value["timed_out"], true);
    assert_eq!(value["output_incomplete"], false);
    assert!(value["retry_guidance"]
        .as_str()
        .unwrap()
        .contains("Inspect"));
}

#[tokio::test]
async fn normal_command_output_and_exit_status_remain_compatible() {
    let (_root, workspace, program) = command_fixture();
    let result = workspace
        .run_trusted_runtime_command(&program, &[], ".", 10)
        .await
        .unwrap();
    assert!(result.success);
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.stdout, "normal-output");
    assert_eq!(result.stderr, "normal-diagnostic");
    assert!(!result.truncated);
    assert!(!result.redacted);
    assert!(!result.timed_out);
    assert!(!result.output_incomplete);
    assert!(result.retry_guidance.is_none());
}

#[tokio::test]
async fn large_command_streams_stay_bounded_and_do_not_deadlock() {
    let (_root, workspace, program) = command_fixture();
    let result = workspace
        .run_trusted_runtime_command(&program, &["large".into()], ".", 20)
        .await
        .unwrap();
    assert!(!result.success);
    assert_eq!(result.exit_code, Some(7));
    assert!(!result.timed_out);
    assert!(!result.output_incomplete);
    assert!(result.truncated);
    assert!(result.stdout.starts_with("stdout-start"));
    assert!(result.stderr.starts_with("stderr-start"));
    assert!(result.stdout.len() <= MAX_OUTPUT_BYTES);
    assert!(result.stderr.len() <= MAX_OUTPUT_BYTES);
}

#[tokio::test]
async fn cancelling_command_owns_the_process_and_pipe_readers() {
    let (root, workspace, program) = command_fixture();
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(async move {
        workspace
            .run_trusted_runtime_command(&program, &["cancel".into()], ".", 10)
            .await
    });
    let started = root.path().join("started.txt");
    timeout(Duration::from_secs(10), async {
        while !started.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture must start before cancellation");
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(!root.path().join("late-effect.txt").exists());
}

#[tokio::test]
async fn timeout_diagnostics_preserve_existing_redaction() {
    let (_root, workspace, program) = command_fixture();
    let result = workspace
        .run_trusted_runtime_command(&program, &["redact".into()], ".", 2)
        .await
        .unwrap();
    assert!(result.timed_out);
    assert!(!result.success);
    assert!(result.redacted);
    assert!(!result.stdout.contains("synthetic-fixture-value"));
    assert!(result.stdout.contains("[REDACTED]"));
}

#[tokio::test]
async fn git_probe_finishes_while_repository_process_capacity_is_occupied() {
    let root = tempfile::tempdir().unwrap();
    assert!(std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(root.path())
        .status()
        .unwrap()
        .success());
    std::fs::write(root.path().join("visible.txt"), "probe fixture\n").unwrap();
    let workspace = Workspace::new(root.path(), false, true).unwrap();
    let governor = crate::resource::global();
    let mut occupied = Vec::new();
    for _ in 0..crate::resource::limits().child_processes {
        occupied.push(governor.acquire_child().await.unwrap());
    }
    let started = std::time::Instant::now();
    let response = timeout(
        Duration::from_secs(2),
        workspace.run_command(
            "git",
            &[
                "status".into(),
                "--short".into(),
                "--untracked-files=all".into(),
            ],
            ".",
            10,
        ),
    )
    .await;
    let elapsed = started.elapsed();
    drop(occupied);
    let result = response
        .expect("a read-only Git probe must not wait behind occupied repository process slots")
        .unwrap();
    assert!(result.success, "{}", result.stderr);
    assert!(result.stdout.contains("visible.txt"));
    assert!(workspace.authorization.requests(10).is_empty());
    eprintln!("isolated Git probe completed in {} ms", elapsed.as_millis());
}

#[tokio::test]
async fn direct_commands_use_the_same_completion_contract() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, true).unwrap();
    let result = workspace
        .run_command("git", &["--version".into()], ".", 10)
        .await
        .unwrap();
    assert!(result.success);
    assert_eq!(result.exit_code, Some(0));
    assert!(!result.timed_out);
    assert!(!result.output_incomplete);
    assert!(result.retry_guidance.is_none());
    assert!(workspace.authorization.requests(10).is_empty());
}
