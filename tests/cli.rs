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
    for command in [
        "setup",
        "update",
        "mcp-stdio",
        "intelligence",
        "verification",
    ] {
        assert!(stdout.contains(command), "missing {command} in help");
    }
    assert!(stdout.contains("╭─ WCode"));
    assert!(stdout.contains("__          __"));
    assert!(stdout.contains("QUICK START"));
    assert!(stdout.contains("wcode                         Start WCode for the current project."));
    assert!(stdout.contains("Most users do not need --workspace"));
    assert!(stdout.contains("Language servers are discovered automatically"));
    assert!(stdout.contains("Do not discover or run language servers"));
    assert!(stdout.contains("Open the WCode setup page"));
    for hidden in ["agent-plugin", "help"] {
        assert!(
            !stdout.lines().any(|line| {
                line.trim_start()
                    .strip_prefix(hidden)
                    .is_some_and(|rest| rest.starts_with(char::is_whitespace))
            }),
            "{hidden} should not occupy the default command catalog"
        );
    }
    for option in [
        "--workspace",
        "--read-only",
        "--no-exec",
        "--no-semantic",
        "--no-tunnel",
        "--no-monitor",
        "--open",
    ] {
        assert!(stdout.contains(option), "missing {option} in help");
    }
    for hidden in [
        "--host",
        "--port",
        "--public-url",
        "--tunnel-provider",
        "--allow-risky-exec",
        "--allow-destructive-writes",
        "--max-parallel-tools",
        "--max-cpu-percent",
        "--max-memory-mb",
    ] {
        assert!(
            !stdout.contains(hidden),
            "advanced option {hidden} should stay out of default help"
        );
    }
}

#[test]
fn update_help_is_stable_and_hidden_agent_plugin_remains_compatible() {
    let update = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .args(["update", "--help"])
        .output()
        .expect("wcode update --help must run");
    assert!(update.status.success());
    let update_help = String::from_utf8(update.stdout).unwrap();
    assert!(update_help.contains("latest verified release"));
    assert!(update_help.contains("Update WCode"));

    let intelligence = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .args(["intelligence", "--help"])
        .output()
        .expect("wcode intelligence --help must run");
    let intelligence_help = String::from_utf8(intelligence.stdout).unwrap();
    assert!(intelligence_help.contains("language-server readiness"));
    assert!(intelligence_help.contains("Discover and initialize available language servers"));
    assert!(intelligence_help.contains("required project checks"));
    assert!(!intelligence_help.contains("Reconciliation runtime state"));

    let plugin = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .args(["agent-plugin", "--help"])
        .output()
        .expect("hidden agent-plugin help must run");
    assert!(plugin.status.success());

    for removed in ["restart", "stop"] {
        let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
            .args([removed, "--help"])
            .output()
            .expect("removed command should fail cleanly");
        assert!(!output.status.success(), "{removed} should be removed");
    }
}

#[test]
fn setup_dry_run_is_structured_and_does_not_write() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .args([
            "--workspace",
            root.path().to_str().unwrap(),
            "setup",
            "--dry-run",
            "--json",
        ])
        .output()
        .expect("setup dry run must run");
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
