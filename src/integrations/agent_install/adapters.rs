#[derive(Clone, Copy, Debug)]
pub(crate) enum AdapterKind {
    JsonMcpServers { path: &'static str },
    JsonServers { path: &'static str },
    CodexToml,
    OpenCode,
    Manual,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AgentHost {
    pub id: &'static str,
    pub name: &'static str,
    pub binaries: &'static [&'static str],
    pub project_markers: &'static [&'static str],
    #[cfg(target_os = "macos")]
    pub mac_apps: &'static [&'static str],
    pub adapter: AdapterKind,
    pub guidance: &'static str,
}

pub(crate) const HOSTS: &[AgentHost] = &[
    host(
        "claude-code",
        "Claude Code",
        &["claude"],
        &[".claude"],
        &[],
        AdapterKind::JsonMcpServers { path: ".mcp.json" },
        "Project .mcp.json; review the repository trust prompt before enabling it.",
    ),
    host(
        "openai-codex",
        "OpenAI Codex",
        &["codex"],
        &[".codex"],
        &[],
        AdapterKind::CodexToml,
        "Project .codex/config.toml; Codex loads it only for trusted projects.",
    ),
    host(
        "copilot-cli",
        "GitHub Copilot CLI",
        &["copilot"],
        &[".github/mcp.json"],
        &[],
        AdapterKind::JsonMcpServers { path: ".mcp.json" },
        "Project .mcp.json; Copilot still requires project trust.",
    ),
    host(
        "vscode-copilot",
        "VS Code + GitHub Copilot",
        &["code", "code-insiders"],
        &[".vscode"],
        &[
            "Visual Studio Code.app",
            "Visual Studio Code - Insiders.app",
        ],
        AdapterKind::JsonServers {
            path: ".vscode/mcp.json",
        },
        "Workspace .vscode/mcp.json using the VS Code servers schema.",
    ),
    host(
        "cursor",
        "Cursor",
        &["cursor"],
        &[".cursor"],
        &["Cursor.app"],
        AdapterKind::JsonMcpServers {
            path: ".cursor/mcp.json",
        },
        "Project .cursor/mcp.json.",
    ),
    host(
        "gemini-cli",
        "Gemini CLI",
        &["gemini"],
        &[".gemini"],
        &[],
        AdapterKind::JsonMcpServers {
            path: ".gemini/settings.json",
        },
        "Workspace .gemini/settings.json.",
    ),
    host(
        "qwen-code",
        "Qwen Code",
        &["qwen"],
        &[".qwen"],
        &[],
        AdapterKind::JsonMcpServers {
            path: ".qwen/settings.json",
        },
        "Project .qwen/settings.json; Qwen requests approval for project MCP config.",
    ),
    host(
        "kiro",
        "Kiro",
        &["kiro", "kiro-cli"],
        &[".kiro"],
        &["Kiro.app"],
        AdapterKind::JsonMcpServers {
            path: ".kiro/settings/mcp.json",
        },
        "Workspace .kiro/settings/mcp.json; no tools are auto-approved.",
    ),
    host(
        "qoder-cli",
        "Qoder CLI",
        &["qoder"],
        &[".qoder"],
        &[],
        AdapterKind::JsonMcpServers { path: ".mcp.json" },
        "Shared project .mcp.json; review Qoder trust before connecting.",
    ),
    host(
        "opencode",
        "OpenCode",
        &["opencode"],
        &["opencode.json"],
        &[],
        AdapterKind::OpenCode,
        "Project opencode.json; existing V1 and V2 MCP containers are detected before merge.",
    ),
    manual(
        "cline",
        "Cline",
        &["cline"],
        &[".cline"],
        "Cline keeps MCP settings in application state; use its MCP settings screen or CLI.",
    ),
    manual(
        "kimi-code",
        "Kimi Code CLI",
        &["kimi", "kimi-code"],
        &[".kimi"],
        "No stable repository-local MCP schema was verified; use Kimi's current MCP UI or CLI.",
    ),
    manual(
        "roo-code",
        "Roo Code",
        &[],
        &[".roo"],
        "Use Roo Code's workspace MCP settings; extension-global state is left untouched.",
    ),
    manual(
        "continue",
        "Continue",
        &["cn"],
        &[".continue"],
        "Continue commonly uses YAML; automatic YAML rewriting is disabled.",
    ),
    manual(
        "zcode",
        "ZCode",
        &["zcode"],
        &[".zcode-plugin"],
        "Install the exported plugin or skill, then bind MCP to this repository.",
    ),
    manual(
        "grok-build",
        "Grok Build",
        &["grok"],
        &[".agents"],
        "Install the portable skill, then add the repository-bound stdio MCP entry.",
    ),
    manual(
        "windsurf",
        "Windsurf",
        &["windsurf"],
        &[".windsurf"],
        "Use Windsurf's MCP settings UI; no project-local merge target is verified.",
    ),
    manual_apps(
        "jetbrains-junie",
        "JetBrains / Junie",
        &[],
        &[".idea"],
        &["IntelliJ IDEA.app", "RustRover.app", "WebStorm.app"],
        "Use the IDE MCP settings UI. Repository code does not edit account or IDE state.",
    ),
    manual_apps(
        "zed",
        "Zed",
        &["zed"],
        &[".zed"],
        &["Zed.app"],
        "Zed settings may be JSONC; use its project settings UI to preserve comments.",
    ),
    manual_apps(
        "trae",
        "TRAE",
        &["trae"],
        &[".trae"],
        &["TRAE.app"],
        "Use TRAE's MCP settings UI; no safe project-local schema is verified.",
    ),
    manual(
        "codebuddy",
        "CodeBuddy",
        &["codebuddy"],
        &[".codebuddy"],
        "Use CodeBuddy's project MCP settings; unknown schemas are not rewritten.",
    ),
    manual(
        "chatgpt-web",
        "ChatGPT Web",
        &[],
        &[],
        "Create an account connector with the current HTTPS /mcp URL and complete OAuth.",
    ),
    manual(
        "claude-web",
        "Claude Web",
        &[],
        &[],
        "Add the current HTTPS /mcp URL in Integrations and complete OAuth.",
    ),
    manual(
        "grok-web",
        "Grok Web",
        &[],
        &[],
        "Add the current HTTPS /mcp URL in Connectors and complete OAuth.",
    ),
    manual(
        "mistral",
        "Mistral",
        &[],
        &[],
        "Use the account connector UI when available; never store OAuth tokens in the repository.",
    ),
];

const fn host(
    id: &'static str,
    name: &'static str,
    binaries: &'static [&'static str],
    project_markers: &'static [&'static str],
    mac_apps: &'static [&'static str],
    adapter: AdapterKind,
    guidance: &'static str,
) -> AgentHost {
    #[cfg(not(target_os = "macos"))]
    let _ = mac_apps;
    AgentHost {
        id,
        name,
        binaries,
        project_markers,
        #[cfg(target_os = "macos")]
        mac_apps,
        adapter,
        guidance,
    }
}

const fn manual(
    id: &'static str,
    name: &'static str,
    binaries: &'static [&'static str],
    project_markers: &'static [&'static str],
    guidance: &'static str,
) -> AgentHost {
    host(
        id,
        name,
        binaries,
        project_markers,
        &[],
        AdapterKind::Manual,
        guidance,
    )
}

const fn manual_apps(
    id: &'static str,
    name: &'static str,
    binaries: &'static [&'static str],
    project_markers: &'static [&'static str],
    mac_apps: &'static [&'static str],
    guidance: &'static str,
) -> AgentHost {
    host(
        id,
        name,
        binaries,
        project_markers,
        mac_apps,
        AdapterKind::Manual,
        guidance,
    )
}
