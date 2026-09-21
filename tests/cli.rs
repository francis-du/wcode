use serde_json::Value;
use std::process::Command;

#[test]
fn release_binary_version_matches_the_package_without_starting_services() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .current_dir(root.path())
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        format!("wcode {}", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

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
        "help-all",
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
        "--performance",
        "--show-config",
    ] {
        assert!(stdout.contains(option), "missing {option} in help");
    }
    for hidden in [
        "--host",
        "--port",
        "--public-url",
        "--password",
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
fn complete_help_exposes_hidden_options_without_starting_the_runtime() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .current_dir(root.path())
        .args([
            "help-all",
            "--workspace",
            "does-not-exist",
            "--max-memory-mb",
            "1",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    for term in [
        "Complete CLI reference",
        "wcode setup",
        "wcode agent-plugin",
        "wcode update",
        "--host",
        "--port",
        "--tunnel-provider",
        "--imessage-to",
        "--allow-sleep",
        "--allow-risky-exec",
        "--allow-destructive-writes",
        "--allow-broad-workspace",
        "--allow-overlapping-workspaces",
        "--max-memory-mb",
        "--max-cpu-percent",
        "--max-parallel-tools",
        "--no-install",
        "--no-install-chatgpt",
        "Deprecated",
        "--input-token-price-per-million-usd",
        "--plan",
        "--profile",
        "remote-http",
        "8765",
        "127.0.0.1",
        "balanced",
        "fast",
        "light",
        "Global option",
    ] {
        assert!(text.contains(term), "complete help omitted {term}");
    }
    assert!(!text.contains('\u{1b}'), "redirected help is plain text");
    assert!(output.stderr.is_empty());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn complete_help_targets_a_command_without_executing_it() {
    let root = tempfile::tempdir().unwrap();
    for (target, option) in [
        ("setup", "--global"),
        ("verification", "--plan-id"),
        ("agent-plugin", "--install-all"),
        ("update", "--help"),
        ("help-all", "--json"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
            .current_dir(root.path())
            .args(["help-all", target])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains(&format!("Usage: wcode {target}")), "{text}");
        assert!(text.contains(option), "{text}");
        assert!(
            text.contains("--max-memory-mb"),
            "inherited hidden flags must be included"
        );
        assert!(
            !text.contains("--host"),
            "root-only flags must not be advertised after a subcommand"
        );
        assert!(output.stderr.is_empty());
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn complete_help_json_is_declarative_and_scoped() {
    let root = tempfile::tempdir().unwrap();
    let invoke = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
            .current_dir(root.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let all = invoke(&["help-all", "--json"]);
    assert_eq!(all["schema_version"], 1);
    assert_eq!(all["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(all["runtime_started"], false);
    assert_eq!(all["scope"], "cli");
    let commands = all["command"]["subcommands"].as_array().unwrap();
    assert!(commands.len() >= 7);
    assert!(commands
        .iter()
        .any(|cmd| cmd["name"] == "agent-plugin" && cmd["hidden"] == true));
    let options = all["command"]["arguments"].as_array().unwrap();
    let port = options.iter().find(|arg| arg["long"] == "port").unwrap();
    assert_eq!(port["default_values"], serde_json::json!(["8765"]));
    assert_eq!(port["global"], false);
    assert_eq!(port["hidden"], true);
    let sub = invoke(&["help-all", "verification", "--json"]);
    assert_eq!(sub["command"]["name"], "verification");
    let plan = sub["command"]["arguments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|arg| arg["long"] == "plan-id")
        .unwrap();
    assert_eq!(plan["aliases"], serde_json::json!(["plan"]));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn complete_help_rejects_invalid_paths_without_running_commands() {
    let root = tempfile::tempdir().unwrap();
    for args in [
        vec!["help-all", "nonexistent"],
        vec!["help-all", "setup", "update"],
        vec!["help-all", "stop"],
        vec!["help-all", "restart"],
        vec!["help-all", "setup", "--", "--global"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
            .current_dir(root.path())
            .args(&args)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}");
        assert!(
            output.stdout.is_empty(),
            "failed lookups must not emit a partial catalog"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("unknown command path"));
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
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

#[test]
fn configuration_preview_is_pure_and_global_options_work_after_subcommands() {
    let root = tempfile::tempdir().unwrap();
    for command in ["mcp-stdio", "setup", "update"] {
        let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
            .current_dir(root.path())
            .args([
                command,
                "--show-config",
                "--performance",
                "fast",
                "--max-memory-mb",
                "128",
                "--read-only",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["preview"], true);
        assert_eq!(value["runtime_started"], false);
        assert_eq!(
            value["resources"]["effective"]["requested_parallel_tools"],
            64
        );
        assert_eq!(
            value["resources"]["effective"]["effective_parallel_tools"],
            16
        );
        assert_eq!(value["permissions"]["write_enabled"], false);
        assert_eq!(value["permissions"]["full_access"], false);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[test]
fn setup_rejects_multiple_roots_instead_of_silently_selecting_the_first() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .args([
            "setup",
            "--dry-run",
            "--json",
            "-w",
            first.path().to_str().unwrap(),
            "-w",
            second.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("setup accepts one project workspace"));
    assert_eq!(std::fs::read_dir(first.path()).unwrap().count(), 0);
    assert_eq!(std::fs::read_dir(second.path()).unwrap().count(), 0);
}

#[test]
fn setup_saves_selected_runtime_options_across_host_formats() {
    for opencode_v2 in [false, true] {
        let root = tempfile::tempdir().unwrap();
        for directory in [".claude", ".codex", ".gemini", ".vscode"] {
            std::fs::create_dir(root.path().join(directory)).unwrap();
        }
        let initial = if opencode_v2 {
            serde_json::json!({"mcp":{"servers":{"other":{"command":["other"]}}}})
        } else {
            serde_json::json!({"mcp":{"other":{"command":["other"]}}})
        };
        std::fs::write(root.path().join("opencode.json"), initial.to_string()).unwrap();
        let arguments = [
            "setup",
            "--project",
            "--performance",
            "fast",
            "--max-memory-mb",
            "256",
            "--max-parallel-tools",
            "7",
            "--read-only",
            "--no-exec",
            "--no-semantic",
            "--json",
        ];
        let expected = serde_json::json!([
            "mcp-stdio",
            "--performance",
            "fast",
            "--max-parallel-tools",
            "7",
            "--max-memory-mb",
            "256",
            "--read-only",
            "--no-exec",
            "--no-semantic",
        ]);
        let invoke = |dry_run: bool| {
            let mut command = Command::new(env!("CARGO_BIN_EXE_wcode"));
            command.current_dir(root.path()).args(arguments);
            if dry_run {
                command.arg("--dry-run");
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice::<Value>(&output.stdout).unwrap()
        };
        let preview = invoke(true);
        assert_eq!(preview["launch"]["args"], expected);
        assert!(!root.path().join(".mcp.json").exists());
        assert_eq!(
            std::fs::read_to_string(root.path().join("opencode.json")).unwrap(),
            initial.to_string()
        );
        let applied = invoke(false);
        assert_eq!(applied["launch"], preview["launch"]);
        for (path, container) in [
            (".mcp.json", "mcpServers"),
            (".gemini/settings.json", "mcpServers"),
            (".vscode/mcp.json", "servers"),
        ] {
            let value: Value =
                serde_json::from_str(&std::fs::read_to_string(root.path().join(path)).unwrap())
                    .unwrap();
            assert_eq!(value[container]["wcode"]["args"], expected, "{path}");
        }
        let codex = std::fs::read_to_string(root.path().join(".codex/config.toml")).unwrap();
        let codex = codex.parse::<toml_edit::DocumentMut>().unwrap();
        let actual = codex["mcp_servers"]["wcode"]["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(serde_json::json!(actual), expected);
        let opencode: Value = serde_json::from_str(
            &std::fs::read_to_string(root.path().join("opencode.json")).unwrap(),
        )
        .unwrap();
        let servers = if opencode_v2 {
            &opencode["mcp"]["servers"]
        } else {
            &opencode["mcp"]
        };
        let mut command = vec![serde_json::json!("wcode")];
        command.extend(expected.as_array().unwrap().iter().cloned());
        assert_eq!(servers["wcode"]["command"], serde_json::json!(command));
        assert_eq!(servers["other"]["command"][0], "other");
        let repeated = invoke(false);
        assert_eq!(repeated["launch"], applied["launch"]);
        assert!(repeated["installed"].as_array().unwrap().is_empty());
        assert!(repeated["updated"].as_array().unwrap().is_empty());
    }
}

#[test]
fn setup_validates_resource_overrides_before_writing_configuration() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".claude")).unwrap();
    for (option, value) in [
        ("--max-memory-mb", "64"),
        ("--max-parallel-tools", "0"),
        ("--max-cpu-percent", "NaN"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
            .current_dir(root.path())
            .args(["setup", "--project", "--json", option, value])
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "setup silently accepted {option}={value}"
        );
        assert!(!root.path().join(".mcp.json").exists());
        assert!(!root.path().join(".agents").exists());
    }
}

#[test]
fn contradictory_safety_and_connection_flags_fail_before_startup() {
    let root = tempfile::tempdir().unwrap();
    for args in [
        vec!["--full-access", "--read-only"],
        vec!["mcp-stdio", "--no-exec", "--full-access"],
        vec!["--no-tunnel", "--public-url", "https://example.test"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
            .current_dir(root.path())
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used with"));
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
