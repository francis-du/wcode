use super::*;

#[test]
fn skill_only_export_reuses_every_canonical_source() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), true, false).unwrap();
    let exported = export(
        &workspace,
        "dist/wcode-agent-plugin",
        AgentPluginProfile::SkillOnly,
        None,
    )
    .unwrap();

    for (relative, expected) in CANONICAL_FILES {
        assert_eq!(
            std::fs::read_to_string(dir.path().join("dist/wcode-agent-plugin").join(relative))
                .unwrap(),
            *expected
        );
    }
    let mcp = std::fs::read_to_string(dir.path().join("dist/wcode-agent-plugin/mcp.json")).unwrap();
    assert_eq!(mcp, MCP_JSON);
    assert_eq!(
        serde_json::from_str::<Value>(&mcp).unwrap()["mcpServers"],
        json!({})
    );
    assert!(exported.mcp_setup_required);
    assert!(export(
        &workspace,
        "dist/wcode-agent-plugin",
        AgentPluginProfile::SkillOnly,
        None
    )
    .is_err());
}

#[test]
fn local_and_remote_profiles_never_guess_or_embed_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path(), true, false).unwrap();
    export(
        &workspace,
        "local-plugin",
        AgentPluginProfile::LocalStdio,
        None,
    )
    .unwrap();
    let local: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("local-plugin/mcp.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        local["mcpServers"]["wcode"]["args"],
        json!([
            "--workspace",
            workspace.root().to_str().unwrap(),
            "mcp-stdio"
        ])
    );
    assert_eq!(local["mcpServers"]["wcode"]["type"], "stdio");
    assert!(!local.to_string().contains("PLUGIN_ROOT"));
    let codex: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("local-plugin/.codex-plugin/plugin.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(codex["mcpServers"], local["mcpServers"]);

    export(
        &workspace,
        "remote-plugin",
        AgentPluginProfile::RemoteHttp,
        Some("https://current-tunnel.example"),
    )
    .unwrap();
    let remote = std::fs::read_to_string(dir.path().join("remote-plugin/mcp.json")).unwrap();
    let remote: Value = serde_json::from_str(&remote).unwrap();
    assert_eq!(
        remote["mcpServers"]["wcode"]["url"],
        "https://current-tunnel.example/mcp"
    );
    assert_eq!(remote["mcpServers"]["wcode"]["type"], "streamable-http");
    assert!(!remote.to_string().to_ascii_lowercase().contains("token"));
    assert!(normalize_remote_mcp_url("https://user:secret@example.com/mcp").is_err());
}

#[test]
fn canonical_manifest_versions_match_the_crate() {
    for manifest in [PLUGIN_JSON, CLAUDE_PLUGIN, CODEX_PLUGIN, ZCODE_PLUGIN] {
        let value: Value = serde_json::from_str(manifest).unwrap();
        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
    }
    for marketplace in [MARKETPLACE, include_str!("../../../../marketplace.json")] {
        let value: Value = serde_json::from_str(marketplace).unwrap();
        assert_eq!(value["plugins"][0]["version"], env!("CARGO_PKG_VERSION"));
    }
    assert!(SKILL.contains("progressive disclosure"));
    assert!(!SKILL.contains("dangerously-skip"));
}
