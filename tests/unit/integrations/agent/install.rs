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
