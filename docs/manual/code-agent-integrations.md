---
layout: docs
title: Code Agent Integrations
description: Source-checked Plugin, Skill, MCP, installation, and diagnostic guide
lang: en
alternate: /zh/docs/code-agent-integrations/
permalink: /docs/code-agent-integrations/
---

# Code Agent, MCP, Skill, and Plugin Integrations

This is the single maintained technical guide for connecting wcode to coding agents and cloud/web connectors. MCP is the executable capability layer; Skills are the portable workflow layer; host subagents/worktrees are isolation helpers; mandatory security, Verification, and Evidence rules remain enforced by the wcode runtime.

Local coding agents should use an explicit repository Workspace and prefer stdio. Cloud/web connectors use Streamable HTTP + OAuth. A Plugin or Skill must not carry credentials, hidden shell hooks, arbitrary executable scripts, or implicit Workspace widening.

## 1. Install wcode

macOS and Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/francis-du/wcode/main/install.sh | sh
```

After updating an already running HTTP runtime, use:

```bash
wcode restart
```

to enter the runtime's normal restart path. Clients that cache MCP capabilities may also need to reconnect.

## 2. Local coding agents: prefer stdio MCP

Use the repository itself as the Workspace:

```bash
wcode --workspace /absolute/path/to/repository mcp-stdio
```

Persistent configuration should use an **absolute repository path**, so a host/plugin working directory cannot accidentally change the Workspace. stdio and HTTP share the same Harness, Workspace policy, Software Intelligence runtime, Tasks, Prompts, Resources, and Tools; stdio simply does not use the public OAuth transport.

The generic stdio shape is:

```json
{
  "command": "wcode",
  "args": ["--workspace", "/absolute/path/to/repository", "mcp-stdio"]
}
```

Each host differs mainly in where that command is registered.

### Compact coding path and Product Scope discovery

Do not preload the whole control plane for routine coding. The normal path is:

```text
agent_context(goal, scopes=...)
  ↓
symbol_context only when readiness needs more source
  ↓
apply_edits / apply_file_edits
  ↓
review_changes
  ↓
verify_project
```

When `budget` is omitted, `agent_context` chooses a bounded adaptive budget. It can include relevant Design State, a scope-aware repo map, fresh semantic/runtime relationships when usable, bounded hot source, exact SHA edit targets, related tests, readiness, and deterministic next actions. Use `workspace_info`, `scope_status`, `design_status`, `project_context`, `software_context`, `language_quality_status`, and deeper Graph/Risk/Reconciliation tools only when the task requires more detail.

The canonical Product Scopes are `runtime`, `integrations`, `workspace`, `design`, `graph`, `semantics`, `traceability`, `risk`, `verification`, `evidence`, `reconciliation`, and `experience`. Tool metadata and `wcode://runtime/product-scopes` expose the same registry. Product Scope metadata narrows context; it never expands Workspace or authorization.

### Deferred tool loading for Claude API MCP connectors

This is a **Claude API / MCP connector** optimization, not a wcode server flag and not a Claude Code CLI setting. Clients that support `mcp_toolset` and Tool Search can keep the five common coding Tools non-deferred and lazy-load the larger governance surface:

```json
{
  "type": "mcp_toolset",
  "mcp_server_name": "wcode",
  "default_config": { "defer_loading": true },
  "configs": {
    "agent_context": { "defer_loading": false },
    "symbol_context": { "defer_loading": false },
    "apply_edits": { "defer_loading": false },
    "review_changes": { "defer_loading": false },
    "verify_project": { "defer_loading": false }
  }
}
```

Other hosts should use their own lazy/deferred Tool-loading feature when available. wcode does not emit a private MCP flag to force this behavior.

## 3. Export the portable Skill / Agent Plugin

Generate the package inside the selected repository:

```bash
wcode --workspace "$PWD" agent-plugin --output wcode-agent-plugin
```

Core layout:

```text
wcode-agent-plugin/
├── plugin.json
├── .claude-plugin/plugin.json
├── .zcode-plugin/plugin.json
├── marketplace.json
└── skills/wcode-software-intelligence/SKILL.md
```

The export intentionally contains no generic hooks, JS/TS/Python executables, credentials, or generic bundled `mcp.json`. A copied Plugin directory cannot safely infer which source repository should become the Workspace. ZCode is the current narrow exception because the host exposes `${CLAUDE_PROJECT_DIR}`, allowing its generated manifest to bind stdio MCP to the current project without guessing.

### Current code-agent compatibility matrix

“Plugin” is not a universal install mechanism. Keep package/Skill support separate from local MCP and remote connector support.

| Code agent | Native package surface | Portable Skill | Local MCP | Recommended wcode path |
| --- | --- | --- | --- | --- |
| Grok Build | Claude-compatible Plugin | Yes | `grok mcp add` | Generated Plugin + project stdio MCP |
| ZCode | `.zcode-plugin/plugin.json` + marketplace | Yes | Plugin manifest / Settings | Generated ZCode Plugin; no public tunnel |
| Claude Code | `.claude-plugin/plugin.json` + marketplace | Yes | `claude mcp add` | Generated Plugin + project stdio MCP |
| OpenAI Codex | Codex Plugin/Skill surface | Yes | `codex mcp add` | Generated Skill + project stdio MCP |
| Cursor | Agent Plugins v1 / Cursor Plugin | Yes | `.cursor/mcp.json` | Local Agent Plugin + stdio MCP |
| Gemini CLI | `gemini-extension.json` | Yes | `gemini mcp add` | Skill + project stdio MCP |
| GitHub Copilot CLI | Agent Plugin / marketplace | Yes | Plugin/MCP UI | Plugin/Skill + project stdio MCP |
| VS Code + GitHub Copilot | Agent Plugin / Copilot MCP | Yes | `.vscode/mcp.json` / `.mcp.json` | Local Plugin + workspace stdio MCP |
| Cline | Executable Plugin + MCP | Experimental Skill | `cline mcp` | Skill + stdio MCP; do not equate the VS Code extension with executable Plugin support |
| Roo Code | Modes / Skills | Yes | `.roo/mcp.json` | Shared Skill + project stdio MCP |
| OpenCode | Executable JS/TS Plugin | Yes | `opencode.json(c)` | Shared Skill + project stdio MCP |
| Windsurf / Cascade | No stable portable Agent-package contract claimed here | No native Skill install claimed | `mcp_config.json` | Project stdio MCP |
| Continue | Hub/config/rules | No native Skill install claimed | `.continue/mcpServers/*.yaml` | Project MCP + reviewed Rules |
| Kiro | Power / Agent Plugins v1 | Yes | `.kiro/settings/mcp.json` | Power + project stdio MCP |
| Qoder CLI | `.qoder-plugin/plugin.json` + marketplace | Yes | `.mcp.json` / current MCP config | Skill + project stdio MCP |
| Kimi Code CLI | `kimi.plugin.json` | Yes | `.kimi-code/mcp.json` | Shared Skill + project stdio MCP |
| Qwen Code | Native Extension / Agent Plugins v1 | Yes | `qwen mcp add` | Generated Plugin + project stdio MCP |
| TRAE | MCP surface documented; Plugin contract varies by build | Rules vary | MCP settings | Project stdio MCP; re-check the current client before claiming native Plugin compatibility |

## 4. Grok Build / Grok coding agent

Grok Build can reuse the Claude-compatible generated Plugin shape. Install/link the generated Skill/Plugin and register MCP separately against the actual repository. The Plugin installation directory must not become the Workspace.

### MCP

When the host exposes `grok mcp add`, register the same command represented by the generic stdio template above, with the target repository as the absolute Workspace path.

### Skill / plugin

Use the generated Skill and Claude-compatible manifest for guidance/discovery. The Skill never replaces wcode authorization or Verification.

## 5. ZCode

Use the generated `.zcode-plugin/plugin.json` and local marketplace package. Because ZCode exposes `${CLAUDE_PROJECT_DIR}`, the generated manifest may safely bind `wcode --workspace ${CLAUDE_PROJECT_DIR} mcp-stdio`. Local ZCode use therefore needs no public tunnel.

## 6. Claude Code

Install/link the generated `.claude-plugin` / Skill and register project stdio MCP through `claude mcp add` or the current project MCP settings. Do not apply the Claude API `defer_loading` JSON above to Claude Code CLI configuration; that is an API Connector feature.

## 7. OpenAI Codex

Use the generated portable Skill and register the absolute repository stdio command with `codex mcp add` or current Codex MCP configuration. Native marketplace packaging and the wcode Skill export are separate layers.

## 8. Cursor

Load the generated Agent Plugin/Skill locally and register project stdio in `.cursor/mcp.json`. Cursor worktrees/subagents may isolate independent work, but shared mutations still pass through wcode Workspace/Scheduler/SHA boundaries when wcode is the editing harness.

## 9. Gemini CLI

Use the Skill/Extension guidance and register project stdio through `gemini mcp add` or current project MCP configuration. Host hooks are not wcode proof; deterministic gates remain in Harness/Evidence.

## 10. GitHub Copilot CLI

Install the generated Plugin or Skill and register project MCP through the current Plugin/MCP management surface. GitHub remote operations executed through wcode `run_command` remain subject to the bounded `git` / `gh` command policy.

## 11. Cline

Cline exposes MCP and its own executable Plugin surface. wcode recommends **Skill + stdio MCP** and does not generate an executable Cline Plugin by default, avoiding hidden scripts, hooks, and credentials.

## 12. Roo Code

Use the shared Agent Skill and register project stdio in `.roo/mcp.json`. Roo modes/rules may reference the Skill, but should not copy a second independent wcode security policy.

## 13. OpenCode

Use the shared Skill and configure MCP in `opencode.json` / `opencode.jsonc`. Core wcode behavior does not require a generated executable JS/TS Plugin.

## 14. Windsurf / Cascade

Register project stdio MCP in `mcp_config.json`. This guide does not claim a stable portable Agent Skill/Plugin installer for Windsurf; any copied workflow guidance should stay reviewed and non-executable.

## 15. Continue

Register project MCP through `.continue/mcpServers/*.yaml` or current Continue configuration. A short reviewed `.continue/rules/` note can point to the wcode workflow, but should not duplicate the full control plane.

## 16. Kiro

Use the generated Skill/Plugin as a Power when appropriate and configure project stdio separately in `.kiro/settings/mcp.json`. The Power provides guidance; MCP provides executable capability.

## 17. Qoder CLI

Use the Skill plus `.mcp.json` / current MCP configuration. Add a Qoder-specific marketplace wrapper only when distribution needs it; normal repository coding does not require another executable layer.

## 18. Kimi Code CLI

Use the shared Skill and register project stdio through `.kimi-code/mcp.json`. A Kimi-specific wrapper is an optional distribution layer, not a runtime requirement.

## 19. Qwen Code

Install/link the generated Agent Plugin and register project stdio through `qwen mcp add` or current MCP settings.

## 20. VS Code + GitHub Copilot

Register project stdio in `.vscode/mcp.json` or `.mcp.json` and optionally register the generated Agent Plugin/Skill. The Workspace must point at the real source repository, not an extension/plugin installation directory.

## 21. TRAE

TRAE exposes MCP integration, while Rules/Plugin contracts can change between product builds. The stable claim in this guide is project stdio MCP; re-check the current client and primary documentation before making a native Plugin compatibility claim.

## 22. Cloud/web connector: Grok Web

Cloud/web connectors cannot spawn the local stdio child process, so use the protected public MCP resource:

```text
https://<public-host>/mcp
```

The default managed tunnel starts Cloudflare, `localhost.run`, Pinggy, and Tailscale Funnel concurrently and keeps every instance-health-verified endpoint. For long-lived deployments, prefer the Tailscale provider or a stable `--public-url https://...` reverse proxy.

### OAuth validation

Remote connectors use Streamable HTTP + OAuth. PKCE, Protected Resource Metadata, Resource binding, DCR metadata validation, redirect policy, and Origin validation remain controlled by wcode. A tunnel provides reachability, not authorization.

Do not route a same-machine coding agent through the public endpoint merely for configuration symmetry; prefer stdio locally.

## 23. Chat and cloud client matrix

The coding-agent matrix in section 3 covers hosts that act on a repository. Chat and cloud clients split differently: stdio-capable desktop hosts, clients that accept a custom remote MCP URL, and catalog-only clients that cannot reach a private wcode at all. Claims below reflect public vendor documentation as of August 2026; "not found" means no public documentation exists, not that support will never ship.

International clients:

| Client | stdio | Remote URL | OAuth | wcode path |
| --- | --- | --- | --- | --- |
| Claude Code | Yes | Yes | Yes | stdio or remote; first-class |
| claude.ai web Connectors | No | Yes | Yes | remote only; expects search/fetch tools |
| Claude Desktop | Yes | Partial | Partial | stdio config file; use Claude Code for remote |
| ChatGPT Developer Mode / Apps SDK | Desktop only | Yes (beta) | Yes | remote only; requires search/fetch, elevated-risk flags |
| Gemini CLI | Yes | Yes | Yes | stdio or remote |
| gemini.google.com consumer | No | Catalog only | n/a | not connectable |
| Gemini Enterprise / API | No | Yes | Yes | remote only |
| VS Code + GitHub Copilot | Yes | Yes | Yes | stdio or remote |
| GitHub Copilot CLI / coding agent | Yes | Yes | Yes | repo `.mcp.json` stdio |
| Cursor | Yes | Yes | Yes | stdio or remote |
| Windsurf / Cascade | Yes | Yes | Yes | stdio or remote |
| JetBrains AI Assistant / Junie | Yes | Yes | Yes | stdio or remote |
| Zed | Yes | Yes | Via bridge | stdio or remote |
| Cline / Roo Code | Yes | Yes | Via bridge | stdio |
| OpenCode | Yes | Bearer only | No | stdio |
| Continue | Yes | Partial | No | stdio |
| Grok custom connector (grok.com) | No | Yes | Yes | remote only |
| Grok Build | Yes | Yes | Yes | stdio; section 4 |
| Mistral Le Chat | No | Yes | Auth, details unverified | remote only |
| Perplexity | macOS app | Yes | Unverified | remote only |
| Microsoft 365 Copilot | No | Admin catalog | n/a | not connectable |
| Notion AI / Slack AI / Discord / Replit / Cody | No | Not found | n/a | not connectable |
| Neovim (avante / codecompanion) | Yes | Partial | Via bridge | stdio |

Chinese clients:

| Client | stdio | Remote URL | OAuth | wcode path |
| --- | --- | --- | --- | --- |
| ZCode (Zhipu) | Yes | Yes | Yes | generated Plugin; section 5 |
| Kimi Code CLI (Moonshot) | Yes | Yes | Yes | stdio or remote |
| Qwen Code CLI (Alibaba) | Yes | Yes | Yes | stdio or remote |
| CodeBuddy (Tencent) | Yes | Yes | Yes | stdio or remote |
| Qoder CLI (Alibaba) | Yes | Yes | Unverified | stdio; section 17 |
| Trae / Trae Work (ByteDance) | Yes | SSE | Unverified | stdio; section 21 |
| Comate Zulu (Baidu) | Yes | SSE | Unverified | stdio |
| Qwen Chat desktop / Tongyi app | No | Marketplace-led | Unverified | catalog only |
| MiniMax Agent / Nano AI toolbox / Spark agent platform | No | Config entry exists | Unverified | platform or catalog only |
| WorkBuddy (Tencent) | Not documented | MCP ecosystem + custom Skills claimed | Unverified | not verified |
| Doubao Work / Pro office mode (ByteDance) | No — GUI virtual desktop, no MCP/API | No | n/a | not connectable |
| Yuanbao / Wenxin web | No | Not found | n/a | not connectable |
| DeepSeek official clients | No | No | n/a | use DeepSeek as a model inside another client |
| SenseTime Raccoon / Step / Monica | Not found | Not found | n/a | not connectable |

Chinese MCP marketplaces (ModelScope plaza, Qianfan, Bailian, Tencent Cloud, Volcano, Trae market, Nano AI toolbox) distribute servers; they are not installation paths for a private wcode.

### Registering wcode on verified remote clients

The stdio command is identical everywhere; only the registration surface differs.

```bash
claude mcp add wcode -- wcode --workspace /absolute/path/to/repository mcp-stdio
claude mcp add --transport http wcode https://<public-host>/mcp
gemini mcp add --transport http wcode https://<public-host>/mcp
qwen  mcp add --transport http wcode https://<public-host>/mcp
kimi  mcp add --transport http --auth oauth wcode https://<public-host>/mcp
```

- **claude.ai Connectors**: Settings → Connectors → Add connector → paste the public URL → OAuth. Treat it as a retrieval client; it expects server-side `search`/`fetch`.
- **ChatGPT**: Settings → Connectors → Advanced → Developer Mode → add the public URL. Server-side `search`/`fetch` is mandatory, Memory is disabled, and the mode is flagged elevated risk.
- **Gemini / Qwen / Kimi CLIs**: the commands above; remote URLs trigger the OAuth flow, or supply static headers. Kimi stores tokens under `~/.kimi/mcp-oauth/`; config lives in `~/.qwen/settings.json` or `~/.kimi/mcp.json`.
- **CodeBuddy**: IDE MCP tab or `.codebuddy/settings.json`; documented OAuth on first remote connect.
- **Cursor / Windsurf / JetBrains / Zed**: same stdio command in `.cursor/mcp.json`, `~/.codeium/windsurf/mcp_config.json`, the JetBrains MCP settings page, or Zed's `context_servers`. Remote HTTP works with varying OAuth maturity; Zed may need the `mcp-remote` bridge.
- **Clients without OAuth**: put a trusted reverse proxy with static authentication in front of wcode, or switch clients.

### Choosing a path

- Single-machine coding: stdio with an absolute repository path. No tunnel, no OAuth, no public exposure.
- Remote or web clients: managed tunnel or stable `--public-url`, then register `https://<public-host>/mcp` and complete OAuth.
- Catalog-only clients (Yuanbao, Doubao, Wenxin web, Nano AI, gemini.google.com, M365 Copilot): they cannot reach a private wcode; do not weaken the wcode security boundary to satisfy them.

## 24. Security checklist for every agent

- The Workspace is explicit; persistent local config should use the absolute repository path.
- Local agents prefer stdio; cloud/web clients use protected public `/mcp`.
- Skills/Plugins carry no credentials, hidden shell hooks, or arbitrary executables.
- `agent_context` is the normal coding entry point; do not preload the entire control plane on every task.
- Source writes use SHA-guarded Workspace primitives.
- Risky commands and remote mutations are authorized by exact operation; a model cannot approve its own request.
- Verification/Evidence must match the current revision; model consensus never replaces deterministic proof.
- Host sandboxes/worktrees/subagents are useful isolation helpers, not replacements for the wcode security boundary.

## 25. Why wcode does not generate vendor-specific executable plugins by default

Executable Plugin formats often introduce host-specific runtimes, hooks, scripts, credentials, or working-directory semantics. Bundling those into one “universal Plugin” would widen the trust boundary and create multiple copies of the same security facts.

wcode therefore keeps the portable layer to Skills/manifests, executable capability in MCP, real repository access in Workspace policy, and mandatory approval in Authorization/Verification/Evidence. Add a thin host wrapper only when the host exposes a clear declarative contract that preserves these boundaries.

## Primary references checked for this compatibility guide

Compatibility surfaces evolve. When maintaining this page, check each host's primary documentation and verify separately:

- native package / Skill surface;
- local stdio MCP support;
- remote MCP / OAuth behavior;
- Workspace/project-path semantics;
- Tool Search / lazy-loading support;
- whether the claimed wcode path was actually tested end to end.

Do not collapse “has MCP”, “can install a Skill”, “has a Plugin marketplace”, and “verified with wcode end to end” into a single `supported=true`. English and Chinese pages must update the same compatibility matrix and security conclusions together.
