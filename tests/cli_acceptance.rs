use serde_json::Value;
use std::process::Command;

#[test]
fn acceptance_help_exposes_real_actions_without_starting_services_or_approvals() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .current_dir(root.path())
        .args(["acceptance", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    for action in ["inspect", "plan", "verify", "record", "history", "metrics"] {
        assert!(help.contains(action), "missing {action}");
    }
    let command_names = help
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect::<Vec<_>>();
    for forbidden in ["activate", "approve", "exception-approve", "import"] {
        assert!(!command_names.contains(&forbidden));
        let rejected = Command::new(env!("CARGO_BIN_EXE_wcode"))
            .current_dir(root.path())
            .args(["acceptance", forbidden, "--help"])
            .output()
            .unwrap();
        assert!(
            !rejected.status.success(),
            "unexpected authority command {forbidden}"
        );
        assert!(String::from_utf8_lossy(&rejected.stderr).contains("unrecognized subcommand"));
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .current_dir(root.path())
        .args(["acceptance", "inspect", "--help"])
        .output()
        .unwrap();
    let help = String::from_utf8(output.stdout).unwrap();
    for option in ["--base", "--head", "--check", "--json"] {
        assert!(help.contains(option), "missing {option}");
    }
}

#[test]
fn acceptance_cli_rejects_refs_readonly_writes_and_disabled_execution() {
    let root = tempfile::tempdir().unwrap();
    let sha = "a".repeat(40);
    for arguments in [
        vec!["acceptance", "inspect", "--base", "HEAD"],
        vec!["acceptance", "inspect", "--base", &sha, "--head", "main"],
        vec![
            "acceptance",
            "verify",
            "--base",
            &sha,
            "--timeout-seconds",
            "1801",
        ],
        vec!["acceptance", "history", "--check"],
        vec![
            "--no-monitor",
            "--no-semantic",
            "--read-only",
            "acceptance",
            "plan",
            "--base",
            &sha,
        ],
        vec![
            "--no-monitor",
            "--no-semantic",
            "--read-only",
            "acceptance",
            "record",
            "--base",
            &sha,
        ],
        vec![
            "--no-monitor",
            "--no-semantic",
            "--no-exec",
            "acceptance",
            "verify",
            "--base",
            &sha,
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
            .current_dir(root.path())
            .args(&arguments)
            .output()
            .unwrap();
        assert!(!output.status.success(), "silently accepted {arguments:?}");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[test]
fn acceptance_inspect_emits_native_incomplete_and_check_exits_nonzero() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let base = "a".repeat(40);
    let invoke = |check: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_wcode"));
        command
            .current_dir(root.path())
            .env("WCODE_STATE_DIR", state.path())
            .args([
                "--no-monitor",
                "--no-semantic",
                "acceptance",
                "inspect",
                "--base",
                &base,
                "--json",
            ]);
        if check {
            command.arg("--check");
        }
        command.output().unwrap()
    };
    let inspected = invoke(false);
    assert!(
        inspected.status.success(),
        "{}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    let record: Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(record["schema_version"], 1);
    assert_eq!(record["state"], "incomplete");
    assert!(record["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason["code"] == "policy_inactive"));
    assert!(record["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason["code"] == "git_capture_incomplete"));
    assert!(record["policy"].is_null());
    let checked = invoke(true);
    assert!(!checked.status.success());
    let checked_record: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(checked_record["state"], "incomplete");
    assert!(String::from_utf8_lossy(&checked.stderr).contains("cannot pass the gate"));
}
