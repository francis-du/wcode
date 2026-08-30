use serde_json::Value;
use std::process::Command;

#[test]
fn help_exposes_the_stable_agent_and_transport_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .arg("--help")
        .output()
        .expect("wcode --help must run");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("help is UTF-8");
    for command in ["agent-plugin", "mcp-stdio", "intelligence", "verification"] {
        assert!(stdout.contains(command), "missing {command} in help");
    }
    assert!(stdout.contains("--no-semantic"));
}

#[test]
fn install_all_dry_run_is_structured_and_does_not_write() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .args([
            "--workspace",
            root.path().to_str().unwrap(),
            "agent-plugin",
            "--install-all",
            "--dry-run",
            "--json",
        ])
        .output()
        .expect("installer dry run must run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: Value = serde_json::from_slice(&output.stdout).expect("summary is JSON");
    assert_eq!(summary["dry_run"], true);
    for key in [
        "detected",
        "planned",
        "installed",
        "updated",
        "already_configured",
        "manual",
        "unsupported",
        "failed",
        "results",
    ] {
        assert!(summary[key].is_array(), "missing summary array {key}");
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
