use super::*;
use std::sync::OnceLock;

#[test]
fn command_queue_wait_is_bounded_independently_from_execution_timeout() {
    assert_eq!(
        process_queue_wait(Duration::from_secs(1_800)),
        crate::resource::PROCESS_QUEUE_WAIT_CAP
    );
    assert_eq!(
        process_queue_wait(Duration::from_secs(2)),
        Duration::from_secs(2)
    );
}

#[test]
fn cargo_contention_wait_is_bounded_separately_from_execution_timeout() {
    assert_eq!(
        cargo_contention_wait(Duration::from_secs(2)),
        Duration::from_secs(5)
    );
    assert_eq!(
        cargo_contention_wait(Duration::from_secs(20)),
        Duration::from_secs(20)
    );
    assert_eq!(
        cargo_contention_wait(Duration::from_secs(1_800)),
        Duration::from_secs(30)
    );
}

#[test]
fn cargo_contention_classification_separates_registry_and_workspace_locks() {
    assert_eq!(
        cargo_contention_lane("cargo", &["info".into(), "serde".into()]),
        Some(CargoContentionLane::Registry)
    );
    assert_eq!(
        cargo_contention_lane(
            "cargo",
            &[
                "--color".into(),
                "always".into(),
                "test".into(),
                "--quiet".into()
            ]
        ),
        Some(CargoContentionLane::Workspace)
    );
    assert_eq!(
        cargo_contention_lane(
            "cargo",
            &["+nightly".into(), "search".into(), "serde".into()]
        ),
        Some(CargoContentionLane::Registry)
    );
    assert_eq!(
        cargo_contention_lane("cargo", &["fmt".into(), "--check".into()]),
        None
    );
    assert_eq!(cargo_contention_lane("rustc", &["--version".into()]), None);
}

#[tokio::test]
async fn cargo_workspace_gate_serializes_before_child_process_admission() {
    let root = tempfile::tempdir().unwrap();
    let first = acquire_cargo_contention_gate(
        "cargo",
        &["test".into()],
        root.path(),
        Duration::from_secs(1),
    )
    .await
    .unwrap()
    .expect("workspace cargo command must acquire a contention permit");

    let blocked = acquire_cargo_contention_gate(
        "cargo",
        &["check".into()],
        root.path(),
        Duration::from_millis(40),
    )
    .await;
    assert!(
        blocked.is_err(),
        "same-workspace cargo command must queue before spawning"
    );
    drop(first);

    let resumed = acquire_cargo_contention_gate(
        "cargo",
        &["check".into()],
        root.path(),
        Duration::from_secs(1),
    )
    .await
    .unwrap();
    assert!(resumed.is_some());
}

#[tokio::test]
async fn cargo_registry_gate_is_global_but_workspace_gates_are_independent() {
    let left = tempfile::tempdir().unwrap();
    let right = tempfile::tempdir().unwrap();
    let registry = acquire_cargo_contention_gate(
        "cargo",
        &["info".into(), "serde".into()],
        left.path(),
        Duration::from_secs(1),
    )
    .await
    .unwrap()
    .expect("cargo info must use the global registry lane");
    let blocked = acquire_cargo_contention_gate(
        "cargo",
        &["search".into(), "tokio".into()],
        right.path(),
        Duration::from_millis(40),
    )
    .await;
    assert!(
        blocked.is_err(),
        "registry cargo commands must serialize across workspaces"
    );
    drop(registry);

    let workspace = acquire_cargo_contention_gate(
        "cargo",
        &["test".into()],
        left.path(),
        Duration::from_secs(1),
    )
    .await
    .unwrap()
    .expect("build/test cargo commands use a workspace lane");
    let independent = acquire_cargo_contention_gate(
        "cargo",
        &["check".into()],
        right.path(),
        Duration::from_millis(40),
    )
    .await
    .unwrap();
    assert!(
        independent.is_some(),
        "different workspaces must not share build/test cargo locks"
    );
    drop((workspace, independent));
}

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
            std::fs::write("timeout-ready.txt", "ready").unwrap();
            std::thread::sleep(std::time::Duration::from_secs(30));
            std::fs::write("late-effect.txt", "must-not-run").unwrap();
        }
        Some("cancel") => {
            println!("started");
            io::stdout().flush().unwrap();
            std::fs::write("started.txt", "started").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(250));
            std::fs::write("late-effect.txt", "must-not-run").unwrap();
        }
        Some("redact") => {
            println!("password=synthetic-fixture-value");
            io::stdout().flush().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
        Some("count") => {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("verification-count.txt")
                .unwrap();
            file.write_all(b"1").unwrap();
            file.flush().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(250));
            println!("counted");
        }
        _ => {
            let verification_counter = std::env::current_exe()
                .ok()
                .and_then(|path| path.file_stem().and_then(|name| name.to_str()).map(str::to_owned))
                .is_some_and(|name| matches!(name.as_str(), "phpunit" | "psalm"));
            if verification_counter {
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("verification-count.txt")
                    .unwrap();
                file.write_all(b"1").unwrap();
                file.flush().unwrap();
                std::thread::sleep(std::time::Duration::from_millis(250));
                println!("counted");
            } else {
                println!("normal-output");
                eprintln!("normal-diagnostic");
            }
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

fn verification_count_fixture() -> (tempfile::TempDir, Workspace, String) {
    let (root, _trusted, fixture) = command_fixture();
    std::fs::create_dir_all(root.path().join("vendor/bin")).unwrap();
    std::fs::copy(
        root.path().join(&fixture),
        root.path().join("vendor/bin/phpunit"),
    )
    .unwrap();
    std::fs::copy(
        root.path().join(fixture),
        root.path().join("vendor/bin/psalm"),
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), false, true).unwrap();
    (root, workspace, "vendor/bin/phpunit".to_owned())
}

#[tokio::test]
async fn timed_out_command_returns_partial_diagnostics_without_replaying_effects() {
    let (root, _workspace, program) = command_fixture();
    let executable = root.path().join(&program);
    let mut command = tokio::process::Command::new(executable);
    command
        .arg("timeout")
        .current_dir(root.path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let child = command.spawn().expect("timeout fixture must start");
    let ready = root.path().join("timeout-ready.txt");
    timeout(Duration::from_secs(10), async {
        while !ready.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("timeout fixture must flush diagnostics before the timed collection begins");

    let result = collect_command_result(
        child,
        &program,
        &["timeout".into()],
        tokio::time::Instant::now() + Duration::from_secs(1),
        0,
    )
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
async fn workspace_all_command_grant_skips_repetitive_command_authorization() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    workspace.revoke_command("cargo").unwrap();
    let workspace_id = workspace.authorization_workspace_id();
    workspace
        .authorization
        .set_workspace_commands_granted(&workspace_id, true);

    let result = workspace
        .run_command("cargo", &["--version".into()], ".", 30)
        .await
        .expect("workspace-wide command authorization should skip CommandAccess prompts");
    assert!(result.success, "{}", result.stderr);
    assert!(workspace.authorization.latest_pending().is_none());
    assert!(workspace.risky_operation_authorized("synthetic-risky-operation"));
}

#[cfg(unix)]
#[tokio::test]
async fn workspace_all_command_grant_bypasses_shell_policy_read_only_and_startup_no_exec() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let workspace_id = workspace.authorization_workspace_id();
    workspace
        .authorization
        .set_workspace_commands_granted(&workspace_id, true);

    let result = workspace
        .run_command(
            "sh",
            &[
                "-c".into(),
                "printf unrestricted > unrestricted-command.txt".into(),
            ],
            ".",
            10,
        )
        .await
        .expect("explicit all-command authorization must bypass WCode command policy");
    assert!(result.success, "{}", result.stderr);
    assert_eq!(
        std::fs::read_to_string(root.path().join("unrestricted-command.txt")).unwrap(),
        "unrestricted"
    );
    assert!(workspace.authorization.latest_pending().is_none());
}

#[cfg(unix)]
#[tokio::test]
async fn full_access_command_lane_bypasses_command_policy_and_argument_filters() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new_with_security(
        root.path(),
        false,
        false,
        WorkspaceSecurity {
            allow_unrestricted_commands: true,
            ..WorkspaceSecurity::default()
        },
    )
    .unwrap();

    let result = workspace
        .run_command(
            "/bin/sh",
            &[
                "-c".into(),
                "test -d / && printf full-access > full-access-command.txt".into(),
            ],
            ".",
            10,
        )
        .await
        .expect("full-access command lane must not apply command-policy or argument filters");
    assert!(result.success, "{}", result.stderr);
    assert_eq!(
        std::fs::read_to_string(root.path().join("full-access-command.txt")).unwrap(),
        "full-access"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn revoking_workspace_all_command_grant_restores_normal_policy() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let workspace_id = workspace.authorization_workspace_id();
    workspace
        .authorization
        .set_workspace_commands_granted(&workspace_id, true);
    assert!(
        workspace
            .run_command("/bin/sh", &["-c".into(), "exit 0".into()], ".", 10)
            .await
            .unwrap()
            .success
    );

    workspace
        .authorization
        .set_workspace_commands_granted(&workspace_id, false);
    assert!(workspace
        .run_command("/bin/sh", &["-c".into(), "exit 0".into()], ".", 10)
        .await
        .is_err());
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
async fn exact_workspace_verification_executable_runs_without_runtime_authorization() {
    let (_root, workspace, program) = verification_count_fixture();
    assert!(workspace.workspace_program_available(&program));
    let result = workspace
        .run_verification_command(&program, &[], ".", 30)
        .await
        .expect("exact workspace-local verification executable should run autonomously");
    assert!(result.success, "{}", result.stderr);
    assert!(workspace.authorization.requests(10).is_empty());
}

#[tokio::test]
async fn exact_revision_verification_commands_coalesce_one_underlying_process() {
    let (root, workspace, program) = verification_count_fixture();
    let first = workspace.clone();
    let second = workspace.clone();
    let args = Vec::new();
    let (left, right) = tokio::join!(
        first.run_verification_command_at_revision(&program, &args, ".", 30, "rev-a"),
        second.run_verification_command_at_revision(&program, &args, ".", 30, "rev-a")
    );
    assert!(left.unwrap().success);
    assert!(right.unwrap().success);
    assert_eq!(
        std::fs::read_to_string(root.path().join("verification-count.txt")).unwrap(),
        "1",
        "followers must reuse the leader instead of spawning a duplicate process"
    );
}

#[tokio::test]
async fn verification_command_flights_separate_revisions() {
    let (root, workspace, program) = verification_count_fixture();
    let first = workspace.clone();
    let second = workspace.clone();
    let args = Vec::new();
    let (left, right) = tokio::join!(
        first.run_verification_command_at_revision(&program, &args, ".", 30, "rev-a"),
        second.run_verification_command_at_revision(&program, &args, ".", 30, "rev-b")
    );
    assert!(left.unwrap().success);
    assert!(right.unwrap().success);
    assert_eq!(
        std::fs::read_to_string(root.path().join("verification-count.txt")).unwrap(),
        "11",
        "different revisions must execute independently"
    );
}

#[tokio::test]
async fn direct_run_command_revision_flights_coalesce_verification_shape() {
    let (root, workspace, program) = verification_count_fixture();
    let workspace_id = workspace.authorization_workspace_id();
    workspace
        .authorization
        .set_workspace_commands_granted(&workspace_id, true);
    let first = workspace.clone();
    let second = workspace.clone();
    let args = Vec::new();
    let (left, right) = tokio::join!(
        first.run_command_at_revision(&program, &args, ".", 30, "rev-direct"),
        second.run_command_at_revision(&program, &args, ".", 30, "rev-direct")
    );
    assert!(left.unwrap().success);
    assert!(right.unwrap().success);
    assert_eq!(
        std::fs::read_to_string(root.path().join("verification-count.txt")).unwrap(),
        "1"
    );
}

#[tokio::test]
async fn verification_command_flights_do_not_cross_workspaces() {
    let (left_root, left_workspace, left_program) = verification_count_fixture();
    let (right_root, right_workspace, right_program) = verification_count_fixture();
    let args = Vec::new();
    let (left, right) = tokio::join!(
        left_workspace.run_verification_command_at_revision(
            &left_program,
            &args,
            ".",
            30,
            "same-revision"
        ),
        right_workspace.run_verification_command_at_revision(
            &right_program,
            &args,
            ".",
            30,
            "same-revision"
        )
    );
    assert!(left.unwrap().success);
    assert!(right.unwrap().success);
    assert_eq!(
        std::fs::read_to_string(left_root.path().join("verification-count.txt")).unwrap(),
        "1"
    );
    assert_eq!(
        std::fs::read_to_string(right_root.path().join("verification-count.txt")).unwrap(),
        "1"
    );
}

#[tokio::test]
async fn verification_command_flights_separate_commands_and_timeouts() {
    let (root, workspace, phpunit) = verification_count_fixture();
    let empty = Vec::new();
    let psalm = "vendor/bin/psalm".to_owned();
    let psalm_args = vec!["--output-format=json".to_owned()];
    let (first, second) = tokio::join!(
        workspace.run_verification_command_at_revision(&phpunit, &empty, ".", 30, "rev-command"),
        workspace.run_verification_command_at_revision(&psalm, &psalm_args, ".", 30, "rev-command")
    );
    assert!(first.unwrap().success);
    assert!(second.unwrap().success);
    assert_eq!(
        std::fs::read_to_string(root.path().join("verification-count.txt")).unwrap(),
        "11",
        "different commands must not share a flight"
    );

    std::fs::remove_file(root.path().join("verification-count.txt")).unwrap();
    let (first, second) = tokio::join!(
        workspace.run_verification_command_at_revision(&phpunit, &empty, ".", 30, "rev-timeout"),
        workspace.run_verification_command_at_revision(&phpunit, &empty, ".", 31, "rev-timeout")
    );
    assert!(first.unwrap().success);
    assert!(second.unwrap().success);
    assert_eq!(
        std::fs::read_to_string(root.path().join("verification-count.txt")).unwrap(),
        "11",
        "different timeout contracts must not share a flight"
    );
}

#[tokio::test]
async fn cancelled_verification_leader_releases_followers_and_allows_retry() {
    let (root, workspace, program) = verification_count_fixture();
    let leader_workspace = workspace.clone();
    let leader_program = program.clone();
    let leader = tokio::spawn(async move {
        leader_workspace
            .run_verification_command_at_revision(&leader_program, &[], ".", 30, "rev-cancel")
            .await
    });
    let count = root.path().join("verification-count.txt");
    timeout(Duration::from_secs(10), async {
        while !count.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("leader must start before follower joins");

    let follower_workspace = workspace.clone();
    let follower_program = program.clone();
    let follower = tokio::spawn(async move {
        follower_workspace
            .run_verification_command_at_revision(&follower_program, &[], ".", 30, "rev-cancel")
            .await
    });
    tokio::time::sleep(Duration::from_millis(25)).await;
    leader.abort();
    let follower_result = timeout(Duration::from_secs(2), follower)
        .await
        .expect("follower must be released when its leader is cancelled")
        .unwrap();
    assert!(follower_result.is_err());

    let retry = workspace
        .run_verification_command_at_revision(&program, &[], ".", 30, "rev-cancel")
        .await
        .expect("a cancelled flight must not block a fresh retry");
    assert!(retry.success);
    assert_eq!(std::fs::read_to_string(count).unwrap(), "11");
}

#[cfg(unix)]
#[test]
fn workspace_program_availability_requires_real_executable_permission() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("tools")).unwrap();
    let tool = root.path().join("tools/check");
    std::fs::write(&tool, "#!/bin/sh\nexit 0\n").unwrap();
    let workspace = Workspace::new(root.path(), false, true).unwrap();
    assert!(!workspace.workspace_program_available("tools/check"));

    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(&tool).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&tool, permissions).unwrap();
    assert!(workspace.workspace_program_available("tools/check"));
}

#[cfg(unix)]
#[test]
fn workspace_verification_executable_rejects_symlink_and_hardlink_aliases() {
    use std::os::unix::fs::symlink;

    let (root, _trusted, fixture) = command_fixture();
    std::fs::create_dir_all(root.path().join("vendor/bin")).unwrap();
    let phpunit = root.path().join("vendor/bin/phpunit");
    std::fs::copy(root.path().join(fixture), &phpunit).unwrap();
    symlink(&phpunit, root.path().join("vendor/bin/phpstan")).unwrap();
    let workspace = Workspace::new(root.path(), false, true).unwrap();
    assert!(!workspace.workspace_program_available("vendor/bin/phpstan"));

    std::fs::hard_link(&phpunit, root.path().join("vendor/bin/psalm")).unwrap();
    assert!(!workspace.workspace_program_available("vendor/bin/phpunit"));
    assert!(!workspace.workspace_program_available("vendor/bin/psalm"));
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
    timeout(Duration::from_secs(30), async {
        while !started.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture must start before cancellation");
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!root.path().join("late-effect.txt").exists());
}

#[test]
fn timeout_diagnostics_preserve_existing_redaction() {
    let timeout_diagnostic = "[wcode: command timed out; termination requested.]".to_owned();
    let (stdout, stderr, redacted) = redact_command_streams(
        "token=fixture-redaction-value".to_owned(),
        timeout_diagnostic.clone(),
    );
    assert!(redacted);
    assert!(!stdout.contains("fixture-redaction-value"));
    assert!(stdout.contains("[REDACTED]"));
    assert_eq!(stderr, timeout_diagnostic);
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

#[test]
fn development_commands_do_not_need_separate_risky_execution_approval() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    for (program, values) in [
        ("make", vec!["deploy"]),
        ("npm", vec!["run", "deploy"]),
        ("deno", vec!["run", "--allow-all", "main.ts"]),
        ("dotnet", vec!["tool", "install", "example"]),
        ("mvn", vec!["deploy"]),
        ("gradle", vec!["publish"]),
    ] {
        let arguments = values.into_iter().map(str::to_owned).collect::<Vec<_>>();
        assert!(
            workspace.development_command_shape_allowed(program, &arguments),
            "development command unexpectedly requires a separate risky-exec approval: {program} {arguments:?}"
        );
    }

    for (program, values) in [
        ("python3", vec!["-c", "print('x')"]),
        ("node", vec!["-e", "process.exit(0)"]),
        ("uv", vec!["auth", "login"]),
    ] {
        let arguments = values.into_iter().map(str::to_owned).collect::<Vec<_>>();
        assert!(
            !workspace.development_command_shape_allowed(program, &arguments),
            "permanent development safety boundary unexpectedly opened: {program} {arguments:?}"
        );
    }
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
