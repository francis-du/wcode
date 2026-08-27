---
layout: docs
title: Code Agent Integrations
description: Source-checked Plugin, Skill, MCP, installation, and diagnostic guide
lang: en
alternate: /zh/docs/code-agent-integrations/
permalink: /docs/code-agent-integrations/
---

# Code Agent, MCP, Skill, and Plugin integrations

This guide documents the supported integration paths between **wcode** and current coding agents. The design goal is deliberately small and safe:

- **MCP is the executable capability layer.** Local coding agents should prefer `stdio`; cloud/web connectors use Streamable HTTP + OAuth.
- **Agent Skills are the portable workflow layer.** `SKILL.md` carries a short control-plane map and progressively loads task-specific context instead of duplicating runtime logic.
- **Isolated subagents/worktrees are reasoning/execution helpers, not proof.** Independent work may fan out when the host supports it, but model consensus never clears deterministic Evidence gates and shared writes remain behind wcode's Scheduler/SHA guards.
- **Executable vendor hooks/plugins are not installed by default.** A skill can guide an agent; it does not silently gain shell, credential, or filesystem privileges.
- **The repository Workspace is always explicit.** wcode never guesses a parent directory or lets a plugin silently widen the Workspace.

This file is the **single maintained technical integration guide**. The
user-facing [WIKI](../), repository README, and website link
here instead of duplicating vendor-specific commands. It is organized in four
blocks:

1. shared build, transport, export, and compatibility matrix;
2. local Code Agent configuration, one agent per section;
3. cloud/web connector configuration, kept separate from local stdio setup;
4. shared security rules and primary sources.

## 1. Install wcode

```bash
curl -fsSL https://raw.githubusercontent.com/francis-du/wcode/main/install.sh | sh
```

After replacing an already running HTTP instance:

```bash
wcode restart
```

Clients that cache MCP capabilities may need to reconnect after an upgrade.

## 2. Local coding agents: prefer stdio MCP

The local transport runs the same Harness, Workspace policy, Software Intelligence runtime, Tasks, Prompts, Resources, and tools as the HTTP server, but uses stdin/stdout rather than a public URL:

```bash
wcode --workspace /absolute/path/to/repository mcp-stdio
```

Use an **absolute repository path** in persistent agent configuration. This prevents a client or plugin working directory from accidentally changing the Workspace.

The stdio process does not run the HTTP OAuth flow. The child-process boundary is the transport trust boundary; all wcode filesystem, command, protected-path, SHA, symlink/hard-link, and repository-execution trust policies still apply. With process-wide `--allow-risky-exec` off, exact risky operations may instead stop with a local authorization request; an interactive runtime operator can approve it in the TUI or protected WebUI and retry. Model-requested command names that are not pre-authorized use the same pending-request flow on a per-Workspace basis.

### Shared Product Scope discovery

HTTP and stdio use the same Product Scope model. Agents should begin with `workspace_info`, `scope_status`, `design_status`, and `project_context`, inspect any `scope_status.unmapped_files`, then choose the Product Scope(s) relevant to the task before broad source reads. The canonical scopes are `runtime`, `integrations`, `workspace`, `design`, `graph`, `semantics`, `traceability`, `risk`, `verification`, `evidence`, `reconciliation`, and `experience`.

Clients can discover them from `workspace_info.product_scopes`, Tool `_meta.dev.wcode/productScopes`, or MCP Resource `wcode://runtime/product-scopes`. `scope_status` audits how the selected repository actually maps into those scopes. Pass relevant scopes to `software_context`; `semantic_query` also accepts scope filters. Product Scope metadata narrows context and describes capabilities, but never widens the Workspace or bypasses tool authorization. See [Product Scopes](../product-scopes/) and [Security](../security/); the remote connector setup and troubleshooting sections below are the canonical English integration reference.

Source work should also inspect `language_quality_status`. It reports syntax, semantics, repository-declared/native formatter/linter/type/static/test/security coverage and advanced Verification stages separately, rather than claiming one generic “language supported” state. `language_quality_run` only executes a detected, declared, available, check-only provider through the normal authorization boundary and records current-revision Evidence. See [language-quality.md](../language-quality/).

The integration philosophy is documented in [agentic-engineering.md](../agentic-engineering/): always-on instructions stay small, Skills/context are loaded on demand, host subagents may isolate independent work, and mandatory policy remains in wcode's deterministic Harness/authorization/Verification/Evidence layers.

The same workflow now includes a structural maintainability gate. `review_changes` can emit deterministic `maintainability-*` findings, and medium-and-higher risk Verification Plans include a blind `maintainability` job requiring capability `maintainability_review`. Agents that can claim reviewer jobs should advertise that capability only when they can perform an independent structural review; a correctness verdict does not satisfy the maintainability job. See [maintainability-review.md](../maintainability-review/).

## 3. Export the portable Skill / Agent Plugin

Generate a small, non-executable package inside the selected repository:

```bash
wcode --workspace "$PWD" agent-plugin --output wcode-agent-plugin
```

Generated layout:

```text
wcode-agent-plugin/
├── plugin.json                         # Agent Plugins 1.0
├── .claude-plugin/
│   └── plugin.json                     # Claude Code; Grok is Claude-compatible
├── .zcode-plugin/
│   └── plugin.json                     # ZCode Skill + workspace-scoped stdio MCP
├── marketplace.json                    # Local ZCode marketplace
├── README.md
└── skills/
    └── wcode-software-intelligence/
        └── SKILL.md
```

The export intentionally contains **no hooks, no JS/TS/Python executables, no credentials, and no bundled `mcp.json`**. Agent Plugins stdio commands default to the plugin directory, while wcode must be launched with the actual source repository as the Workspace. Configure MCP separately using the commands below.

The ZCode-specific manifest is the narrow exception to the portable package's
no-bundled-MCP rule: ZCode defines `${CLAUDE_PROJECT_DIR}`, so its declarative
`mcpServers.wcode` entry can safely launch `wcode --workspace
${CLAUDE_PROJECT_DIR} mcp-stdio` without guessing or widening the Workspace.

### Current code-agent compatibility matrix

“Plugin” is not one universal installation mechanism. The table below records
the official extension surface checked on **2026-08-26** and the safest wcode
path. “Native package” means the host has an installable package format; it does
not mean that arbitrary vendor hooks are portable.

| Code agent | Native package surface | Portable Skill | Local MCP | Recommended wcode path |
| --- | --- | --- | --- | --- |
| Grok Build | Claude-compatible plugins | Yes | `grok mcp add` | Generated plugin + project stdio MCP |
| ZCode | `.zcode-plugin/plugin.json` + marketplace | Yes | Plugin manifest or Settings | Generated ZCode plugin; no public tunnel |
| Claude Code | `.claude-plugin/plugin.json` + marketplace | Yes | `claude mcp add` | Generated plugin + project stdio MCP |
| OpenAI Codex | `.codex-plugin/plugin.json` + universal directory/marketplace | Yes | `codex mcp add` | Generated Skill + project stdio MCP; native Codex marketplace packaging is separate |
| Cursor | Agent Plugins v1 or `.cursor-plugin/plugin.json` | Yes | `.cursor/mcp.json` | Load the generated Agent Plugin locally + stdio MCP |
| Gemini CLI | `gemini-extension.json` | Yes | `gemini mcp add` | Link/install Skill + project stdio MCP; use an Extension only when bundling more components |
| GitHub Copilot CLI | Agent Plugin/marketplace | Yes | `/plugins` → MCP | Install generated plugin or Skill; add MCP through the dashboard |
| VS Code + GitHub Copilot | Agent Plugins v1 / Copilot / Claude formats | Yes | `.vscode/mcp.json` or `.mcp.json` | Register the generated local plugin + workspace stdio MCP |
| Cline | Executable JS/TS plugin (CLI/SDK/Kanban) | Yes, experimental | `cline mcp` | Skill + stdio MCP; do not treat the VS Code extension as supporting Cline executable plugins |
| Roo Code | Modes/Skills, not a documented portable plugin installer | Yes | `.roo/mcp.json` | Shared Agent Skill + project stdio MCP |
| OpenCode | Executable JS/TS plugin | Yes | `opencode.json(c)` | Shared Agent Skill + project stdio MCP |
| Windsurf / Cascade | No documented agent-package installer | No native Agent Skill install claimed here | `mcp_config.json` | Project stdio MCP; translate reviewed workflow guidance manually if needed |
| Continue | Hub/config packages and rules | No native Agent Skill install claimed here | `.continue/mcpServers/*.yaml` | Project MCP + reviewed `.continue/rules/` guidance |
| Kiro | Power (Agent Plugins v1) | Yes | `.kiro/settings/mcp.json` | Install as a Power for guidance; configure project stdio MCP separately |
| Qoder CLI | `.qoder-plugin/plugin.json` + marketplace | Yes | `.mcp.json` / MCP config | Skill + project stdio MCP; use a Qoder wrapper only when distributing through its marketplace |
| Kimi Code CLI | `kimi.plugin.json` or `.kimi-plugin/plugin.json` | Yes | `.kimi-code/mcp.json` | Shared Agent Skill + project stdio MCP; Kimi wrapper optional |
| Qwen Code | Native Extension or Agent Plugins v1 | Yes | `qwen mcp add` | Install/link generated Agent Plugin + project stdio MCP |
| TRAE | Public docs confirm MCP transport, not a stable portable plugin contract | Rules vary by product/version | MCP settings | Project stdio MCP; verify the current client build before claiming plugin compatibility |

The generated root `plugin.json` is directly useful to Agent Plugins v1 hosts,
but its package intentionally omits a generic `mcp.json`: a copied plugin cannot
reliably infer which repository should become the wcode Workspace. ZCode is the
only generated wrapper that currently has a host-defined project-directory
variable and therefore bundles the stdio declaration.

## 4. Grok Build / Grok coding agent

### MCP

Project-scoped local MCP:

```bash
grok mcp add --scope project wcode -- \
  wcode --workspace "$PWD" mcp-stdio
```

Verify discovery and connectivity:

```bash
grok mcp list
grok mcp doctor wcode
grok inspect
```

If stdio starts but does not connect, inspect Grok's captured stderr log:

```text
~/.grok/logs/mcp/wcode.stderr.log
```

Grok also imports MCP configuration from project `.mcp.json`, Cursor MCP configuration, and Claude configuration. Prefer one authoritative project definition to avoid a same-name server being shadowed by another source.

### Skill / plugin

Test the generated plugin directly:

```bash
grok --plugin-dir ./wcode-agent-plugin
```

Or install only the Skill into the project:

```bash
mkdir -p .grok/skills/wcode-software-intelligence
cp wcode-agent-plugin/skills/wcode-software-intelligence/SKILL.md \
  .grok/skills/wcode-software-intelligence/SKILL.md
```

Grok also reads Claude Code plugins/skills and the shared `~/.agents/skills` family, so the same `SKILL.md` can be reused rather than rewritten.

## 5. ZCode

Do not paste a wcode URL ending in `/mcp` into **Add marketplace**. A marketplace
source must return `marketplace.json`; the protected MCP endpoint correctly
returns HTTP 401 before OAuth, which ZCode will report as `Failed to fetch
marketplace: 401 Unauthorized`. Use **Settings → MCP servers** for a direct MCP
connection, or install the plugin from a real marketplace source.

For this repository checkout, the committed workspace configuration uses local
stdio and avoids the public tunnel entirely:

```json
{
  "mcp": {
    "servers": {
      "wcode": {
        "command": "/Users/francis/Code/Rust/wcode/target/debug/wcode",
        "args": ["--workspace", "/Users/francis/Code/Rust/wcode", "mcp-stdio"]
      }
    }
  }
}
```

ZCode can install wcode directly from this public repository after the root
`marketplace.json` has been published:

1. Open a workspace in ZCode.
2. Open **Settings → Plugins → Create → Add marketplace**.
3. Enter `francis-du/wcode` (or the full public GitHub repository URL).
4. Install and enable `wcode` from the Personal marketplace section.

For an unpushed checkout, add the local `wcode-agent-plugin` directory instead.
Its `marketplace.json` resolves the plugin from the same directory.

The ZCode manifest registers both the Skill and a local stdio MCP server. It
uses ZCode's `${CLAUDE_PROJECT_DIR}` variable for `--workspace` and `cwd`, so the
currently open project is the explicit wcode Workspace. This path does not need
the public Quick Tunnel or OAuth.

`Failed to fetch marketplace: 401 Unauthorized` happens before MCP startup. It
means ZCode could not fetch the marketplace source; it is not an MCP/OAuth error.
Confirm that the repository is public, the root `marketplace.json` is present on
the selected branch, and GitHub is reachable, then refresh that marketplace.

## 6. Claude Code

Project-scoped stdio MCP:

```bash
claude mcp add --transport stdio --scope project wcode -- \
  wcode --workspace "$PWD" mcp-stdio
```

Verify:

```bash
claude mcp get wcode
claude mcp list
```

Project MCP configurations require workspace approval before Claude Code connects to them.

Test the generated plugin:

```bash
claude --plugin-dir ./wcode-agent-plugin
```

Or install just the Skill for this repository:

```bash
mkdir -p .claude/skills/wcode-software-intelligence
cp wcode-agent-plugin/skills/wcode-software-intelligence/SKILL.md \
  .claude/skills/wcode-software-intelligence/SKILL.md
```

## 7. OpenAI Codex

Project-local stdio MCP keeps Codex on the same bounded wcode Workspace/Harness path without needing a public tunnel:

```bash
codex mcp add wcode -- \
  wcode --workspace "$PWD" mcp-stdio
codex mcp list
```

Codex also supports Streamable HTTP MCP and OAuth. For a remote wcode server, configure the URL ending in `/mcp`; when the authorization server does not advertise Client ID Metadata Documents, Codex can use Dynamic Client Registration instead. wcode intentionally does not fetch arbitrary client-provided metadata URLs, so it keeps the smaller DCR trust boundary rather than adding an outbound SSRF/DNS-rebinding surface.

Install the generated Skill at repository scope using Codex's shared Agent Skills location:

```bash
mkdir -p .agents/skills/wcode-software-intelligence
cp wcode-agent-plugin/skills/wcode-software-intelligence/SKILL.md \
  .agents/skills/wcode-software-intelligence/SKILL.md
```

Codex also has a native plugin system. A native package uses
`.codex-plugin/plugin.json` and can bundle Skills, MCP servers, or both; Codex
and ChatGPT share the universal plugin directory, while a personal local
marketplace is the supported development path. The current wcode exporter does
not pretend its root Agent Plugins manifest is a Codex manifest. Until a Codex
marketplace package is published, use the repository Skill copy above and the
explicit `codex mcp add` command. This avoids silently binding a cached plugin
copy to the wrong source Workspace.

Use Codex's normal sandbox and approval policy for the repository; do not weaken approval settings just because wcode itself has Workspace guardrails.

## 8. Cursor

Cursor supports the Agent Plugins open standard, so the root `plugin.json` is directly compatible.

For local plugin development:

```bash
mkdir -p ~/.cursor/plugins/local
ln -s "$PWD/wcode-agent-plugin" ~/.cursor/plugins/local/wcode
```

Then restart Cursor or run **Developer: Reload Window**.

Project stdio MCP can be configured in `.cursor/mcp.json`. Use an explicit absolute repository path:

```json
{
  "mcpServers": {
    "wcode": {
      "command": "wcode",
      "args": ["--workspace", "/absolute/path/to/repository", "mcp-stdio"]
    }
  }
}
```

Cursor Agent CLI diagnostics:

```bash
cursor-agent mcp list
cursor-agent mcp list-tools wcode
```

Remote wcode HTTP also works with Cursor OAuth, but local stdio avoids tunnel/OAuth overhead for repository-local work.

## 9. Gemini CLI

Project stdio MCP:

```bash
gemini mcp add --scope project wcode wcode \
  --workspace "$PWD" mcp-stdio
```

Gemini CLI discovers MCP Tools, Prompts, and Resources; wcode exposes all three.

Link the generated Skill for development:

```bash
gemini skills link ./wcode-agent-plugin/skills/wcode-software-intelligence \
  --scope workspace
```

Or install a standalone Skill package/repository with `gemini skills install ... --scope workspace`.

Verify in Gemini:

```text
/skills list
/skills reload
```

Gemini requires consent before activating non-built-in Skills and treats untrusted folders conservatively. Do not use `--consent` in shared setup instructions unless the source has already been independently reviewed.

## 10. GitHub Copilot CLI

Install the Skill into the current repository:

```bash
copilot plugins install --skill --scope project \
  ./wcode-agent-plugin/skills/wcode-software-intelligence/SKILL.md
```

Or install the plugin directory where your Copilot CLI version supports local plugin sources:

```bash
copilot plugins install ./wcode-agent-plugin
```

For MCP, use Copilot's `/plugins` dashboard or `/mcp` flow; current Copilot CLI documentation intentionally separates MCP installation from `copilot plugins install`. Do not invent an MCP install command in automation when the client expects its interactive/policy-controlled registry flow.

## 11. Cline

Install the Skill at project scope:

```bash
mkdir -p .cline/skills/wcode-software-intelligence
cp wcode-agent-plugin/skills/wcode-software-intelligence/SKILL.md \
  .cline/skills/wcode-software-intelligence/SKILL.md
```

Skills are currently an experimental Cline feature and may need to be enabled in Settings.

Cline CLI exposes:

```bash
cline mcp
cline doctor
```

Use its MCP configuration UI/command to register the local command:

```text
wcode --workspace /absolute/path/to/repository mcp-stdio
```

Cline also has an executable JS/TS plugin API. wcode deliberately does not emit a Cline executable plugin because Cline plugins/hooks can execute code; the Skill + MCP path provides the needed capability with a smaller trust surface.

## 12. Roo Code

Project MCP lives in `.roo/mcp.json`. Configure stdio with an explicit repository path:

```json
{
  "mcpServers": {
    "wcode": {
      "command": "wcode",
      "args": ["--workspace", "/absolute/path/to/repository", "mcp-stdio"]
    }
  }
}
```

Install the Skill using the shared Agent Skills location:

```bash
mkdir -p .agents/skills/wcode-software-intelligence
cp wcode-agent-plugin/skills/wcode-software-intelligence/SKILL.md \
  .agents/skills/wcode-software-intelligence/SKILL.md
```

Roo has per-mode tool controls and MCP approval. Keep wcode MCP auto-approval disabled until the repository and tool set have been reviewed.

## 13. OpenCode

Install the Skill in a shared Agent Skills location:

```bash
mkdir -p .agents/skills/wcode-software-intelligence
cp wcode-agent-plugin/skills/wcode-software-intelligence/SKILL.md \
  .agents/skills/wcode-software-intelligence/SKILL.md
```

OpenCode also recognizes `.opencode/skills/` and Claude-compatible skill locations.

Configure wcode as a local MCP in `opencode.json` / `opencode.jsonc` using the local MCP form documented by OpenCode and the command:

```text
wcode --workspace /absolute/path/to/repository mcp-stdio
```

For sensitive repositories, configure OpenCode permissions so wcode MCP tools require approval rather than enabling all MCP tools automatically.

OpenCode has in-process executable plugins. They are unnecessary for wcode integration and intentionally not generated.

## 14. Windsurf / Cascade

Windsurf supports stdio, Streamable HTTP, and SSE MCP, plus Tools, Resources, and Prompts.

Add a local server through Windsurf's MCP settings or raw `mcp_config.json`:

```json
{
  "mcpServers": {
    "wcode": {
      "command": "wcode",
      "args": ["--workspace", "/absolute/path/to/repository", "mcp-stdio"]
    }
  }
}
```

For remote HTTP, use the wcode URL ending in `/mcp`. Enterprise MCP allowlists may require an exact command/argument shape; use an absolute repository path and keep the configured argument count stable.

## 15. Continue

Continue supports custom MCP servers in Agent mode. Configure wcode at project scope in `.continue/mcpServers/wcode.yaml`:

```yaml
name: wcode
version: 1.0.0
schema: v1
mcpServers:
  - name: wcode
    command: wcode
    args:
      - --workspace
      - /absolute/path/to/repository
      - mcp-stdio
```

Continue can also import/copy MCP JSON configuration used by clients such as Claude, Cursor, or Cline. Keep one authoritative project definition to avoid duplicate same-name servers.

Continue's current documented customization model is not the same as native Agent Skills discovery, so wcode does **not** claim that Continue directly installs the generated `SKILL.md`. Use MCP for executable capability; if you want the workflow instructions in Continue, add an equivalent reviewed Continue rule rather than silently converting/installing executable plugin code.

Keep Continue's normal tool approval policy enabled for sensitive repositories.

## 16. Kiro

Kiro calls installable plugins **Powers**. A Power follows Agent Plugins v1 and
can contain `plugin.json`, `skills/`, and `mcp.json`. Install Powers from Kiro's
Powers view or a reviewed GitHub URL. Because the generated wcode package omits
generic `mcp.json`, installing it as a Power supplies workflow guidance only.

Configure the executable capability at workspace scope in
`.kiro/settings/mcp.json`:

```json
{
  "mcpServers": {
    "wcode": {
      "command": "wcode",
      "args": ["--workspace", "/absolute/path/to/repository", "mcp-stdio"],
      "disabled": false,
      "autoApprove": []
    }
  }
}
```

Leave `autoApprove` empty until the server and repository have been reviewed.

## 17. Qoder CLI

Qoder plugins can bundle commands, agents, Skills, hooks, workflows, and MCP.
Their optional manifest lives at `.qoder-plugin/plugin.json`; marketplaces use
`marketplace.json`. Manage them interactively with `/plugins`, or use
`qoder plugins install`, `qoder plugins validate`, and
`qoder plugins marketplace` from the shell.

wcode does not emit a Qoder-specific executable wrapper. Reuse the generated
Skill and configure this local stdio command through Qoder's MCP settings:

```text
wcode --workspace /absolute/path/to/repository mcp-stdio
```

This keeps the wcode package hook-free while preserving Qoder's normal plugin
and MCP approval boundaries.

## 18. Kimi Code CLI

Kimi plugins use `kimi.plugin.json` or `.kimi-plugin/plugin.json` and may bundle
Skills, agents, hooks, commands, and MCP servers. Install/manage them with
`/plugins`; custom sources can be local directories, GitHub URLs, zip URLs, or a
marketplace JSON. Plugin changes require `/reload` or a new session.

For wcode, install the shared Skill at `.agents/skills/` as shown above, then
create project-local `.kimi-code/mcp.json` with the explicit repository path:

```json
{
  "mcpServers": {
    "wcode": {
      "command": "wcode",
      "args": ["--workspace", "/absolute/path/to/repository", "mcp-stdio"]
    }
  }
}
```

Use `/mcp` to inspect status and `/mcp-config` to edit or authorize a remote
server. Project stdio entries execute commands when a trusted workspace opens.

## 19. Qwen Code

Qwen Code natively loads Agent Plugins v1 without rewriting `plugin.json` or
`SKILL.md`, so the generated package can be installed or linked directly:

```bash
qwen extensions install ./wcode-agent-plugin
# development checkout:
qwen extensions link ./wcode-agent-plugin
```

The package supplies the Skill. Add wcode's executable capability separately at
project scope:

```bash
qwen mcp add --scope project wcode wcode \
  --workspace "$PWD" mcp-stdio
```

Qwen's Agent Plugins v1 runtime supports Skills plus stdio/Streamable HTTP MCP,
but ignores hooks, commands, agents, and legacy SSE from that portable format.

## 20. VS Code + GitHub Copilot

VS Code supports Agent Plugins v1, existing Copilot plugins, and Claude plugin
formats. Enable `chat.plugins.enabled`, then use **Chat: Install Plugin From
Source** for a Git repository, or register the generated checkout directly in
user `settings.json`:

```json
{
  "chat.pluginLocations": {
    "/absolute/path/to/wcode-agent-plugin": true
  }
}
```

The generated package contributes its Skill. Add the executable capability with
**MCP: Add Server**, or create workspace `.vscode/mcp.json` / portable
`.mcp.json` using the same explicit stdio command:

```json
{
  "servers": {
    "wcode": {
      "type": "stdio",
      "command": "wcode",
      "args": ["--workspace", "/absolute/path/to/repository", "mcp-stdio"]
    }
  }
}
```

Review plugin hooks and MCP declarations before enabling them; both can execute
local code. wcode's generated Agent Plugins package contains neither.

## 21. TRAE

TRAE is retained as a transport-level integration. Configure the same explicit
stdio command through the current client's MCP settings:

```text
wcode --workspace /absolute/path/to/repository mcp-stdio
```

The public documentation available during this review did not establish a
stable, portable Agent Plugin manifest or marketplace contract. Do not rename an
MCP connection “plugin support”; re-check the installed TRAE release before
documenting product-specific rules or remote OAuth behavior.

## 22. Cloud/web connector: Grok Web

This is intentionally separated from local Code Agent setup. xAI's servers must
reach wcode over public HTTPS, so Grok Web uses Streamable HTTP + OAuth rather
than local stdio.

Start normal wcode:

```bash
wcode --workspace "$PWD"
```

Use the **MCP URL shown by wcode, including `/mcp`**, for example:

```text
https://example.trycloudflare.com/mcp
```

A Quick Tunnel URL is temporary. After a tunnel restart, update or reconnect
the Grok connector with the new URL.

### OAuth validation

Given:

```bash
MCP_URL='https://example.trycloudflare.com/mcp'
BASE_URL="${MCP_URL%/mcp}"
```

1. `curl -i "$MCP_URL"` should return HTTP 401 with a Bearer challenge and
   `resource_metadata`.
2. `curl "$BASE_URL/.well-known/oauth-protected-resource/mcp"` should identify
   `$BASE_URL/mcp` as the resource and `$BASE_URL` as its authorization server.
3. `curl "$BASE_URL/.well-known/oauth-authorization-server"` should advertise
   `/authorize`, `/token`, `/register`, S256 PKCE, and the supported grants.
4. After an earlier failed registration, remove and reconnect the connector so
   Grok performs discovery and DCR again instead of reusing stale credentials.

Current wcode also implements strict-connector compatibility for standards-shaped
401 challenges, complete public-client DCR metadata, PKCE shape validation,
canonical `/mcp` Resource Indicator binding, safe legacy token inheritance,
modern `server/discover`, and the final MCP 2026 error codes. These are generic
protocol fixes; without the original Grok log, no single item should be claimed
as the cause of a past failure.

## 23. Security checklist for every agent

Before enabling wcode or any third-party plugin:

1. **Use the smallest Workspace.** Never point wcode at your home directory or filesystem root unless you deliberately opt into broad workspaces.
2. **Prefer stdio locally.** Do not expose a public tunnel just to connect a local coding agent.
3. **Keep remote OAuth credentials client-managed.** Do not put bearer tokens, refresh tokens, or client secrets into Skill files or committed MCP config.
4. **Review executable plugin components.** Skills are instructions; hooks/plugins/scripts are code. wcode's generated package contains no executable hooks.
5. **Do not auto-enable or auto-approve risky repository execution.** Leave process-wide `--allow-risky-exec` off when you want interactive control: an exact language-server, broader command, or runtime-executor operation can request a local session authorization and must be retried after the operator approves it in the TUI. Use the flag only when the operator intentionally pre-authorizes repository-aware execution for the whole process.
6. **Use agent approval controls.** Cursor/Windsurf/Roo/OpenCode/Gemini/Claude/Grok all have trust or approval mechanisms; do not disable them globally just to make setup faster.
7. **Use scopes for context, not permission bypass.** Product Scopes can narrow source/semantic retrieval but do not grant filesystem, command, deletion, or human-approval privileges.
8. **Diagnose before broadening permissions.** A failing MCP handshake is not a reason to turn on shell access, disable OAuth, or expose a broader Workspace.

## 24. Why wcode does not generate vendor-specific executable plugins by default

The common denominator across modern agents is now strong enough: MCP provides executable capabilities and Agent Skills provide portable workflows. Vendor plugin APIs add hooks, in-process code, package installation, or lifecycle scripts. Generating those by default would increase supply-chain risk and package complexity without adding core wcode capability.

wcode therefore uses a thin-adapter strategy:

```text
Agent / IDE
  ├─ Agent Skill / Agent Plugin metadata   (portable workflow)
  └─ MCP client
       └─ wcode mcp-stdio                  (local)
          or
       └─ https://.../mcp + OAuth          (cloud/web)
                ↓
       one wcode Harness / policy / evidence runtime
```

This keeps client support broad while preserving one implementation of security, graph/context, verification, and evidence.

## Primary references checked for this compatibility guide

- xAI: [Grok Connectors](https://docs.x.ai/grok/connectors), [Custom MCP Tunneling](https://docs.x.ai/grok/connectors/custom-mcp-tunneling), [Grok Build MCP Servers](https://docs.x.ai/build/features/mcp-servers), [Skills / Plugins](https://docs.x.ai/build/features/skills-plugins-marketplaces)
- Claude Code: [MCP](https://code.claude.com/docs/en/mcp), [Skills](https://code.claude.com/docs/en/skills)
- OpenAI Codex: [documentation](https://developers.openai.com/codex/), [plugins](https://developers.openai.com/codex/build-plugins)
- Cursor: [MCP](https://docs.cursor.com/context/model-context-protocol), [Plugins](https://cursor.com/docs/plugins)
- ZCode: [Plugins](https://zcode.z.ai/cn/docs/plugin), [MCP](https://zcode.z.ai/cn/docs/mcp-services)
- Gemini CLI: [MCP](https://geminicli.com/docs/tools/mcp-server/), [Skills](https://geminicli.com/docs/cli/skills/), [Extensions](https://geminicli.com/docs/extensions/)
- GitHub: [Copilot CLI plugin reference](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-plugin-reference)
- VS Code: [Agent Plugins](https://code.visualstudio.com/docs/agent-customization/agent-plugins), [MCP](https://code.visualstudio.com/docs/agent-customization/mcp-servers)
- Cline: [Skills](https://docs.cline.bot/customization/skills), [MCP](https://docs.cline.bot/mcp/mcp-overview)
- Roo Code: [MCP](https://roocodeinc.github.io/Roo-Code/features/mcp/using-mcp-in-roo/), [Skills](https://roocodeinc.github.io/Roo-Code/features/skills/)
- OpenCode: [MCP](https://opencode.ai/docs/mcp-servers/), [Skills](https://opencode.ai/docs/skills/)
- Windsurf: [MCP](https://docs.windsurf.com/windsurf/cascade/mcp)
- Continue: [MCP](https://docs.continue.dev/customize/deep-dives/mcp)
- Kiro: [Powers](https://kiro.dev/docs/powers/), [MCP](https://kiro.dev/docs/mcp/configuration/)
- Qoder CLI: [plugins](https://docs.qoder.com/cli/plugins-reference)
- Kimi Code CLI: [plugins](https://www.kimi.com/code/docs/en/kimi-code-cli/customization/plugins.html), [Skills](https://www.kimi.com/code/docs/en/kimi-code-cli/customization/skills.html), [MCP](https://www.kimi.com/code/docs/en/kimi-code-cli/customization/mcp.html)
- Qwen Code: [Agent Plugins v1](https://qwenlm.github.io/qwen-code-docs/en/users/extension/agent-plugins/), [MCP](https://qwenlm.github.io/qwen-code-docs/en/users/features/mcp/)
