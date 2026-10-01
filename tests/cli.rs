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
        "menu-bar",
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
fn menu_bar_json_is_read_only_and_reports_no_runtime_as_offline() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .current_dir(root.path())
        .env("WCODE_STATE_DIR", state.path())
        .args(["menu-bar", "--json"])
        .output()
        .expect("wcode menu-bar --json must run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["kind"], "menu_bar_status");
    assert_eq!(value["observation_only"], true);
    assert_eq!(value["summary"]["state"], "offline");
    assert_eq!(value["summary"]["runtime_count"], 0);
    assert_eq!(value["presence"]["records"], serde_json::json!([]));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    assert_eq!(std::fs::read_dir(state.path()).unwrap().count(), 0);
}

#[test]
fn stdio_runtime_publishes_menu_bar_presence_and_cleans_it_on_eof() {
    use std::io::Write;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .current_dir(root.path())
        .env("WCODE_STATE_DIR", state.path())
        .args([
            "mcp-stdio",
            "--no-monitor",
            "--no-semantic",
            "--no-exec",
            "--read-only",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("wcode mcp-stdio must start");

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list",
        "params": {
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {}
            }
        }
    });
    writeln!(
        child.stdin.as_mut().unwrap(),
        "{}",
        serde_json::to_string(&request).unwrap()
    )
    .unwrap();
    child.stdin.as_mut().unwrap().flush().unwrap();

    let presence = state.path().join("runtime-presence").join("v1");
    let deadline = Instant::now() + Duration::from_secs(5);
    let record = loop {
        let record = std::fs::read_dir(&presence).ok().and_then(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .find(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
                .and_then(|path| std::fs::read(path).ok())
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .filter(|value| value["mcp_connected"] == true)
        });
        if let Some(record) = record {
            break record;
        }
        assert!(
            Instant::now() < deadline,
            "stdio runtime did not publish connected presence"
        );
        std::thread::sleep(Duration::from_millis(25));
    };
    assert_eq!(record["transport"], "stdio");
    assert_eq!(record["mcp_connected"], true);
    assert!(record["updated_at_ms"].as_u64().is_some());
    for forbidden in ["ui_token", "oauth_token", "owner", "arguments", "command"] {
        assert!(!record.to_string().contains(forbidden));
    }

    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    assert_eq!(response["id"], 1);
    assert!(response["result"]["tools"].is_array());

    let retained = std::fs::read_dir(&presence)
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json"));
    assert!(
        !retained,
        "stdio runtime presence must be removed on clean EOF"
    );
}

#[cfg(unix)]
#[test]
fn http_runtime_publishes_menu_bar_presence_and_cleans_it_on_terminate() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .current_dir(root.path())
        .env("WCODE_STATE_DIR", state.path())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            "0",
            "--no-tunnel",
            "--no-monitor",
            "--no-semantic",
            "--no-exec",
            "--read-only",
            "--allow-sleep",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("isolated HTTP runtime must start");

    let presence = state.path().join("runtime-presence").join("v1");
    let deadline = Instant::now() + Duration::from_secs(15);
    let record = loop {
        let record = std::fs::read_dir(&presence).ok().and_then(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .find(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
                .and_then(|path| std::fs::read(path).ok())
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .filter(|value| value["transport"] == "http")
        });
        if let Some(record) = record {
            break record;
        }
        if Instant::now() >= deadline {
            let state_entries = std::fs::read_dir(state.path())
                .ok()
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let presence_entries = std::fs::read_dir(&presence)
                .ok()
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!(
                "HTTP runtime did not publish menu bar presence; status={:?}\nstate={state_entries:?}\npresence={presence_entries:?}\nstdout={}\nstderr={}",
                output.status.code(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    };

    let local_url = record["local_url"].as_str().unwrap();
    let parsed = url::Url::parse(local_url).unwrap();
    assert_eq!(parsed.host_str(), Some("127.0.0.1"));
    let port = parsed.port().unwrap();
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_ok());
    assert_eq!(record["mcp_connected"], false);
    assert_eq!(record["active_tasks"], 0);
    assert_eq!(record["queued_tasks"], 0);

    let code = unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    assert_eq!(code, 0);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let retained = std::fs::read_dir(&presence)
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json"));
    assert!(
        !retained,
        "HTTP runtime presence must be removed on clean termination"
    );
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
    assert_eq!(summary["design_initialization"]["status"], "planned");
    assert_eq!(summary["design_initialization"]["policy_activated"], false);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn setup_project_bootstraps_design_without_workspace_argument() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname='brownfield-project'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("src/lib.rs"),
        "pub fn existing_code() {}\n",
    )
    .unwrap();

    for expected in ["initialized", "existing"] {
        let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
            .current_dir(root.path())
            .args(["setup", "--project", "--json"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(summary["design_initialization"]["status"], expected);
        assert_eq!(summary["design_initialization"]["policy_activated"], false);
        assert_eq!(
            summary["acceptance_policy_suggestion"]["status"],
            "draft_available"
        );
        let source = std::fs::read_to_string(root.path().join(".wcode/project.yaml")).unwrap();
        let project: wcode::design::ProjectDesign = serde_yaml::from_str(&source).unwrap();
        assert_eq!(
            project.name,
            root.path().file_name().unwrap().to_str().unwrap()
        );
        assert!(project.description.is_empty());
        assert!(project.acceptance_policy.is_none());
        assert!(root.path().join(".wcode/design").is_dir());
        let mut design_paths = std::fs::read_dir(root.path().join(".wcode/design"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        design_paths.sort();
        assert_eq!(design_paths, ["constraints.yaml", "product.yaml"]);
        assert!(!root.path().join("target").exists());
        assert_eq!(
            std::fs::read_to_string(root.path().join("src/lib.rs")).unwrap(),
            "pub fn existing_code() {}\n"
        );
        let mut initialized_paths = std::fs::read_dir(root.path().join(".wcode"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        initialized_paths.sort();
        assert_eq!(initialized_paths, ["design", "project.yaml"]);
    }
}

#[test]
fn setup_project_readonly_does_not_bootstrap_design() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .current_dir(root.path())
        .args(["setup", "--project", "--read-only", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["design_initialization"]["status"], "blocked");
    assert_eq!(summary["design_initialization"]["policy_activated"], false);
    assert!(!root.path().join(".wcode").exists());
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
