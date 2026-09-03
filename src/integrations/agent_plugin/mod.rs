use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use clap::ValueEnum;
use serde::Serialize;
use serde_json::{json, Value};
use url::{Host, Url};

const PLUGIN_JSON: &str = include_str!("../../../plugin/plugin.json");
const MCP_JSON: &str = include_str!("../../../plugin/mcp.json");
const README: &str = include_str!("../../../plugin/README.md");
const CONNECTIONS: &str = include_str!("../../../plugin/CONNECTIONS.md");
const MARKETPLACE: &str = include_str!("../../../plugin/marketplace.json");
const CLAUDE_PLUGIN: &str = include_str!("../../../plugin/.claude-plugin/plugin.json");
const CODEX_PLUGIN: &str = include_str!("../../../plugin/.codex-plugin/plugin.json");
const ZCODE_PLUGIN: &str = include_str!("../../../plugin/.zcode-plugin/plugin.json");
const SKILL: &str = include_str!("../../../plugin/skills/wcode/SKILL.md");

const CANONICAL_FILES: &[(&str, &str)] = &[
    ("plugin.json", PLUGIN_JSON),
    ("README.md", README),
    ("CONNECTIONS.md", CONNECTIONS),
    ("marketplace.json", MARKETPLACE),
    (".claude-plugin/plugin.json", CLAUDE_PLUGIN),
    (".codex-plugin/plugin.json", CODEX_PLUGIN),
    (".zcode-plugin/plugin.json", ZCODE_PLUGIN),
    ("skills/wcode/SKILL.md", SKILL),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AgentPluginProfile {
    SkillOnly,
    LocalStdio,
    RemoteHttp,
    Auto,
}

#[derive(Debug, Serialize)]
pub(crate) struct AgentPluginExport {
    pub root: String,
    pub files: Vec<String>,
    pub profile: AgentPluginProfile,
    pub mcp_setup_required: bool,
    pub note: String,
}

pub(crate) fn export(
    workspace: &Workspace,
    output: &str,
    requested_profile: AgentPluginProfile,
    remote_url: Option<&str>,
) -> Result<AgentPluginExport> {
    let root = output.trim().trim_end_matches('/');
    if root.is_empty() || root == "." {
        bail!("agent plugin output must be a new repository-relative directory");
    }
    let profile = match (requested_profile, remote_url) {
        (AgentPluginProfile::Auto, Some(_)) => AgentPluginProfile::RemoteHttp,
        (AgentPluginProfile::Auto, None) => AgentPluginProfile::LocalStdio,
        (profile, _) => profile,
    };
    let mcp = mcp_profile(profile, remote_url)?;
    let codex_manifest = codex_manifest_profile(profile, &mcp)?;

    let mut files = CANONICAL_FILES
        .iter()
        .map(|(path, _)| format!("{root}/{path}"))
        .collect::<Vec<_>>();
    files.push(format!("{root}/mcp.json"));
    if let Some(existing) = files
        .iter()
        .find(|path| workspace.root().join(path.as_str()).exists())
    {
        bail!("agent plugin export would overwrite existing file: {existing}");
    }

    for directory in [
        root.to_owned(),
        format!("{root}/.claude-plugin"),
        format!("{root}/.codex-plugin"),
        format!("{root}/.zcode-plugin"),
        format!("{root}/skills"),
        format!("{root}/skills/wcode"),
    ] {
        workspace.ensure_directory(&directory)?;
    }
    for ((relative, content), path) in CANONICAL_FILES.iter().zip(&files) {
        let content = if *relative == ".codex-plugin/plugin.json" {
            codex_manifest.as_str()
        } else {
            content
        };
        workspace.create_file(path, content)?;
    }
    workspace.create_file(files.last().expect("mcp path"), &mcp)?;

    let (mcp_setup_required, note) = match profile {
        AgentPluginProfile::SkillOnly => (
            true,
            "Skill exported without an MCP target; choose local-stdio, remote-http, or run wcode setup."
                .to_owned(),
        ),
        AgentPluginProfile::LocalStdio => (
            false,
            "stdio MCP uses the Host working directory as the default Workspace; no repository path is embedded."
                .to_owned(),
        ),
        AgentPluginProfile::RemoteHttp => (
            false,
            "Streamable HTTP MCP exported without credentials; OAuth remains client-managed."
                .to_owned(),
        ),
        AgentPluginProfile::Auto => unreachable!("auto profile is resolved before export"),
    };
    Ok(AgentPluginExport {
        root: root.to_owned(),
        files,
        profile,
        mcp_setup_required,
        note,
    })
}

fn mcp_profile(profile: AgentPluginProfile, remote_url: Option<&str>) -> Result<String> {
    if profile == AgentPluginProfile::SkillOnly {
        return Ok(MCP_JSON.to_owned());
    }
    let server = match profile {
        AgentPluginProfile::LocalStdio => portable_stdio_server(),
        AgentPluginProfile::RemoteHttp => {
            let url = remote_url.context("--remote-url is required with --profile remote-http")?;
            json!({"type": "streamable-http", "url": normalize_remote_mcp_url(url)?})
        }
        AgentPluginProfile::SkillOnly | AgentPluginProfile::Auto => unreachable!(),
    };
    let mut output = serde_json::to_string_pretty(&json!({
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {"wcode": server}
    }))?;
    output.push('\n');
    Ok(output)
}

fn codex_manifest_profile(profile: AgentPluginProfile, mcp: &str) -> Result<String> {
    if profile == AgentPluginProfile::SkillOnly {
        return Ok(CODEX_PLUGIN.to_owned());
    }
    let mut manifest = serde_json::from_str::<Value>(CODEX_PLUGIN)?;
    let mcp = serde_json::from_str::<Value>(mcp)?;
    manifest["mcpServers"] = mcp["mcpServers"].clone();
    let mut output = serde_json::to_string_pretty(&manifest)?;
    output.push('\n');
    Ok(output)
}

pub(crate) fn local_stdio_server() -> Value {
    json!({
        "command": "wcode",
        "args": ["mcp-stdio"]
    })
}

fn portable_stdio_server() -> Value {
    let mut server = local_stdio_server();
    server["type"] = json!("stdio");
    server
}

pub(crate) fn canonical_skill() -> &'static str {
    SKILL
}

fn normalize_remote_mcp_url(value: &str) -> Result<String> {
    let mut url = Url::parse(value).context("--remote-url must be an absolute URL")?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/" | "/mcp" | "/mcp/")
    {
        bail!("--remote-url must be an origin or /mcp URL without credentials, query, or fragment");
    }
    let safe_scheme = match (url.scheme(), url.host()) {
        ("https", Some(_)) => true,
        ("http", Some(Host::Domain(host))) => host.eq_ignore_ascii_case("localhost"),
        ("http", Some(Host::Ipv4(host))) => host.is_loopback(),
        ("http", Some(Host::Ipv6(host))) => host.is_loopback(),
        _ => false,
    };
    if !safe_scheme {
        bail!("--remote-url must use HTTPS, except loopback HTTP is allowed for local testing");
    }
    url.set_path("/mcp");
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/agent/plugin.rs"]
mod tests;
