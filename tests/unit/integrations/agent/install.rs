use super::*;
use serde_json::json;
use std::fs;

#[test]
fn dry_run_plans_project_local_merges_without_writing() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".gemini")).unwrap();
    fs::write(
        root.path().join(".gemini/settings.json"),
        serde_json::to_string_pretty(&json!({
            "theme": "dark",
            "mcpServers": {"other": {"command": "other"}}
        }))
        .unwrap(),
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let before = fs::read_to_string(root.path().join(".gemini/settings.json")).unwrap();
    let summary = apply_install(&workspace, plan_install(&workspace), true);

    assert!(summary.planned.iter().any(|host| host == "Gemini CLI"));
    assert_eq!(
        fs::read_to_string(root.path().join(".gemini/settings.json")).unwrap(),
        before
    );
}

#[test]
fn installer_merges_other_servers_and_is_idempotent() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".claude")).unwrap();
    fs::write(
        root.path().join(".mcp.json"),
        "{\n  \"mcpServers\": {\"other\": {\"command\": \"other\"}}\n}\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let first = apply_install(&workspace, plan_install(&workspace), false);
    assert!(first.updated.iter().any(|host| host == "Claude Code"));
    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.path().join(".mcp.json")).unwrap()).unwrap();
    assert_eq!(config["mcpServers"]["other"]["command"], "other");
    assert_eq!(config["mcpServers"]["wcode"]["command"], "wcode");
    assert_eq!(config["mcpServers"]["wcode"]["args"], json!(["mcp-stdio"]));
    assert!(!config.to_string().contains("--workspace"));
    assert!(!config.to_string().contains("secret"));

    let second = apply_install(&workspace, plan_install(&workspace), false);
    assert!(second
        .already_configured
        .iter()
        .any(|host| host == "Claude Code"));
}

#[test]
fn unknown_json_schema_shape_fails_closed_and_preserves_file() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".gemini")).unwrap();
    let path = root.path().join(".gemini/settings.json");
    fs::write(&path, "{\"mcpServers\": []}\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let summary = apply_install(&workspace, plan_install(&workspace), false);
    assert!(summary.failed.iter().any(|host| host == "Gemini CLI"));
    assert_eq!(fs::read_to_string(path).unwrap(), "{\"mcpServers\": []}\n");
}

#[test]
fn opencode_merge_handles_v1_and_v2_without_losing_other_servers() {
    for (initial, container) in [
        (
            json!({"mcp": {"other": {"type": "local", "command": ["other"]}}}),
            "v1",
        ),
        (
            json!({"mcp": {"servers": {"other": {"type": "local", "command": ["other"]}}}}),
            "v2",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("opencode.json"),
            serde_json::to_string_pretty(&initial).unwrap(),
        )
        .unwrap();
        let workspace = Workspace::new(root.path(), true, false).unwrap();
        let first = apply_install(&workspace, plan_install(&workspace), false);
        assert!(
            first.updated.iter().any(|host| host == "OpenCode"),
            "{container}"
        );

        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.path().join("opencode.json")).unwrap())
                .unwrap();
        let servers = if container == "v2" {
            &config["mcp"]["servers"]
        } else {
            &config["mcp"]
        };
        assert_eq!(servers["other"]["command"][0], "other");
        assert_eq!(servers["wcode"]["type"], "local");
        assert_eq!(servers["wcode"]["command"][0], "wcode");

        let second = apply_install(&workspace, plan_install(&workspace), false);
        assert!(second
            .already_configured
            .iter()
            .any(|host| host == "OpenCode"));
    }
}

#[test]
fn global_setup_uses_user_config_paths_without_embedding_a_workspace() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".codex")).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let summary = apply_install(&workspace, plan_global_install(&workspace), false);

    assert_eq!(summary.scope, "global");
    assert!(summary.installed.iter().any(|host| host == "OpenAI Codex"));
    let config = fs::read_to_string(root.path().join(".codex/config.toml")).unwrap();
    assert!(config.contains("command = \"wcode\""));
    assert!(config.contains("\"mcp-stdio\""));
    assert!(!config.contains("--workspace"));
    assert_eq!(
        fs::read_to_string(root.path().join(".agents/skills/wcode/SKILL.md")).unwrap(),
        agent_plugin::canonical_skill()
    );
    assert!(summary.results.iter().any(|result| {
        result.host == "Global Agent Skill" && result.detail.starts_with("User-level skill")
    }));
}

#[test]
fn configured_global_setup_uses_reviewed_launch_options_and_file_versions() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".codex")).unwrap();
    let path = root.path().join(".codex/config.toml");
    fs::write(&path, "# keep this comment\nmodel = \"test\"\n").unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let args = ["mcp-stdio", "--performance", "light", "--read-only"].map(str::to_owned);
    let plan = plan_configured_install(&workspace, true, &args);
    let preview = apply_install(&workspace, plan.clone(), true);
    assert_eq!(preview.launch["args"], json!(args));
    let applied = apply_install(&workspace, plan, false);
    assert!(applied.failed.is_empty(), "{:?}", applied.failed);
    let config = fs::read_to_string(&path).unwrap();
    assert!(config.contains("# keep this comment"));
    let doc = config.parse::<toml_edit::DocumentMut>().unwrap();
    let saved = doc["mcp_servers"]["wcode"]["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(saved, args);
    let next_args = ["mcp-stdio", "--performance", "fast"].map(str::to_owned);
    let reviewed = plan_configured_install(&workspace, true, &next_args);
    let changed = format!("{config}\n# edited after preview\n");
    fs::write(&path, &changed).unwrap();
    let outcome = apply_install(&workspace, reviewed, false);
    assert!(outcome.failed.iter().any(|name| name == "OpenAI Codex"));
    assert_eq!(fs::read_to_string(path).unwrap(), changed);
}

#[test]
fn kimi_setup_merges_project_mcp_and_is_idempotent() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".kimi-code")).unwrap();
    let target = root.path().join(".kimi-code/mcp.json");
    let initial = "{\"mcpServers\":{\"other\":{\"url\":\"https://example.test/mcp\"}}}\n";
    fs::write(&target, initial).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let plan = plan_install(&workspace);
    let preview = apply_install(&workspace, plan.clone(), true);
    assert!(preview.planned.iter().any(|name| name == "Kimi Code CLI"));
    assert_eq!(fs::read_to_string(&target).unwrap(), initial);
    let applied = apply_install(&workspace, plan, false);
    assert!(applied.updated.iter().any(|name| name == "Kimi Code CLI"));
    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
    assert_eq!(
        config["mcpServers"]["other"]["url"],
        "https://example.test/mcp"
    );
    assert_eq!(
        config["mcpServers"]["wcode"],
        json!({"command": "wcode", "args": ["mcp-stdio"]})
    );
    let repeat = apply_install(&workspace, plan_install(&workspace), false);
    assert!(repeat
        .already_configured
        .iter()
        .any(|name| name == "Kimi Code CLI"));
}

#[test]
fn kimi_global_setup_updates_launch_without_dropping_kimi_options() {
    let home = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(home.path(), true, false).unwrap();
    let config_home = workspace.root().join("kimi-profile");
    fs::create_dir(&config_home).unwrap();
    let target = config_home.join("mcp.json");
    let initial = json!({"mcpServers": {
        "other": {"url": "https://example.test/mcp"},
        "wcode": {"command": "wcode", "args": ["mcp-stdio"], "toolTimeoutMs": 90000}
    }});
    fs::write(&target, initial.to_string()).unwrap();
    let server = agent_plugin::local_stdio_server();
    let plan = plan_kimi_global(&workspace, Some(&config_home), &server).unwrap();
    assert_eq!(plan.outcome, FileOutcome::Update);
    merge::apply(&workspace, &plan).unwrap();
    let merged: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
    assert_eq!(
        merged["mcpServers"]["other"],
        initial["mcpServers"]["other"]
    );
    let entry = &merged["mcpServers"]["wcode"];
    let binary = Path::new(entry["command"].as_str().unwrap());
    assert!(binary.is_absolute() && binary.is_file());
    assert_eq!(entry["args"], json!(["mcp-stdio"]));
    assert_eq!(entry["toolTimeoutMs"], 90000);
    assert_eq!(
        plan_kimi_global(&workspace, Some(&config_home), &server)
            .unwrap()
            .outcome,
        FileOutcome::Already
    );
}

#[test]
fn kimi_setup_initializes_empty_configs_but_preserves_broken_json() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".kimi-code")).unwrap();
    let target = root.path().join(".kimi-code/mcp.json");
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    fs::write(&target, " \n").unwrap();
    let installed = apply_install(&workspace, plan_install(&workspace), false);
    assert!(installed.updated.iter().any(|name| name == "Kimi Code CLI"));
    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
    assert_eq!(config["mcpServers"]["wcode"]["args"], json!(["mcp-stdio"]));

    let malformed = "{\"mcpServers\":";
    fs::write(&target, malformed).unwrap();
    let rejected = apply_install(&workspace, plan_install(&workspace), false);
    assert!(rejected.failed.iter().any(|name| name == "Kimi Code CLI"));
    assert_eq!(fs::read_to_string(target).unwrap(), malformed);
}

#[test]
fn kimi_setup_reports_disabled_entries_without_changing_them() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".kimi-code")).unwrap();
    let target = root.path().join(".kimi-code/mcp.json");
    let initial = json!({"mcpServers": {
        "wcode": {"command": "wcode", "args": ["mcp-stdio"], "enabled": false}
    }});
    fs::write(&target, initial.to_string()).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let summary = apply_install(&workspace, plan_install(&workspace), false);
    let kimi = summary
        .results
        .iter()
        .find(|result| result.host_id == "kimi-code")
        .unwrap();
    assert_eq!(kimi.status, AgentInstallStatus::Manual);
    assert!(kimi.detail.contains("disabled"));
    assert_eq!(fs::read_to_string(target).unwrap(), initial.to_string());
}

#[test]
fn kimi_remote_config_is_preserved_without_blocking_other_hosts() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".kimi-code")).unwrap();
    fs::create_dir(root.path().join(".gemini")).unwrap();
    let target = root.path().join(".kimi-code/mcp.json");
    let original = r#"{"mcpServers":{"wcode":{"url":"https://example.test/mcp"}}}"#;
    fs::write(&target, original).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let plan = plan_install(&workspace);
    let preview = apply_install(&workspace, plan.clone(), true);
    assert!(preview.failed.is_empty());
    assert!(preview.manual.iter().any(|name| name == "Kimi Code CLI"));
    assert!(preview.planned.iter().any(|name| name == "Gemini CLI"));
    let applied = apply_install(&workspace, plan, false);
    assert!(applied.installed.iter().any(|name| name == "Gemini CLI"));
    assert_eq!(fs::read_to_string(target).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn setup_preview_rejects_a_linked_config_directory() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let config = outside.path().join("mcp.json");
    let content = r#"{"mcpServers":{"wcode":{"command":"wcode","args":["mcp-stdio"]}}}"#;
    fs::write(&config, content).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join(".kimi-code")).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let summary = apply_install(&workspace, plan_install(&workspace), true);
    let kimi = summary
        .results
        .iter()
        .find(|result| result.host_id == "kimi-code")
        .unwrap();
    assert_eq!(kimi.status, AgentInstallStatus::Failed);
    assert!(kimi.detail.contains("symlink"));
    assert_eq!(fs::read_to_string(config).unwrap(), content);
}

#[test]
fn kimi_global_config_stays_inside_the_setup_home() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    assert_eq!(
        kimi_global_config(&home, None).unwrap(),
        ".kimi-code/mcp.json"
    );
    assert_eq!(
        PathBuf::from(kimi_global_config(&home, Some(&home.join("custom"))).unwrap()),
        PathBuf::from("custom").join("mcp.json")
    );
    assert!(kimi_global_config(&home, Some(root.path())).is_err());
    assert!(kimi_global_config(&home, Some(&home.join("../other"))).is_err());
    assert!(kimi_global_config(&home, Some(Path::new("relative"))).is_err());
}

#[test]
fn manual_only_detection_does_not_create_a_portable_skill_plan() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let detections = BTreeMap::from([(
        "jetbrains-junie".to_owned(),
        DetectedHost {
            id: "jetbrains-junie".to_owned(),
            name: "JetBrains / Junie".to_owned(),
            detected: true,
            evidence: vec!["test marker".to_owned()],
        },
    )]);
    let mut writes = BTreeMap::new();
    let mut actions = Vec::new();

    add_skill_plan(&workspace, &detections, &mut writes, &mut actions);

    assert!(writes.is_empty());
    assert!(actions.is_empty());
}
