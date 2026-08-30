---
layout: docs
title: MCP Client Integrations
description: Project-local setup for MCP clients, plugin packages, and remote connections
lang: en
alternate: /zh/docs/code-agent-integrations/
permalink: /docs/code-agent-integrations/
---

# Connect MCP clients to wcode

Use stdio for a client running on the same machine. Use Streamable HTTP and
OAuth for a remote or web client. A Skill or plugin package adds working
instructions; MCP is what gives the client access to wcode tools.

## 1. Choose a transport

| Client location | Transport | Configuration |
| --- | --- | --- |
| Same machine | stdio | `wcode --workspace /absolute/repo mcp-stdio` |
| Remote, preferred | Streamable HTTP | `https://host/mcp` with OAuth |
| Older remote client | SSE compatibility | `GET /sse` and `POST /message?sessionId=...` with OAuth |

These are three entrances to one implementation. They share JSON-RPC dispatch,
the Harness, Workspace selection, command policy, authorization, Tools,
Prompts, Resources, and Software Intelligence state. SSE is kept for clients
that still implement the 2024 transport; new setups should use `/mcp`.

Never point stdio at a plugin installation folder. Bind it to the real source
repository with an absolute path:

```json
{
  "command": "wcode",
  "args": ["--workspace", "/absolute/repository", "mcp-stdio"]
}
```

## 2. Detect and configure local hosts

Install wcode first:

```bash
curl -fsSL https://raw.githubusercontent.com/francis-du/wcode/main/install.sh | sh
```

Then preview the project-local changes:

```bash
wcode --workspace "$PWD" agent-plugin --install-all --dry-run
```

Apply exactly that plan:

```bash
wcode --workspace "$PWD" agent-plugin --install-all
```

Add `--json` when another tool needs the structured report. Each result is one
of `detected`, `installed`, `updated`, `already_configured`, `manual`,
`unsupported`, or `failed`, and includes its detection evidence and target
file.

The installer only writes inside the repository. It parses JSON and TOML,
merges the `wcode` server, and preserves unrelated settings. Updates use atomic,
SHA-guarded writes. Symlinks, oversized files, an unexpected container type,
invalid JSON/TOML, JSONC, and YAML all fail closed. It does not call a shell,
download a plugin, store a secret, edit an unknown global file, or approve
RiskyExecution.

## 3. Export the Skill and plugin package

`wcode-agent-plugin/` is the canonical package. The Rust exporter embeds its
README, Skill, manifests, and connection notes with `include_str!`; those long
files are not maintained twice.

```bash
# No MCP target; safe to distribute
wcode --workspace "$PWD" agent-plugin --profile skill-only

# Bind stdio to this repository
wcode --workspace "$PWD" agent-plugin --profile local-stdio

# Publish a credential-free remote profile
wcode --workspace "$PWD" agent-plugin \
  --profile remote-http \
  --remote-url https://current-host.example/mcp
```

Every export contains the standard `mcp.json`. The canonical `skill-only`
package has an empty `mcpServers` object. `local-stdio` writes the selected
absolute Workspace. `remote-http` accepts an HTTPS origin or `/mcp` URL without
credentials, query, or fragment. OAuth tokens remain in the MCP client.

The package also carries `.claude-plugin`, `.codex-plugin`, and `.zcode-plugin`
metadata. Those manifests are adapters around the same Skill; they do not add
hooks, executable scripts, or another security policy.

## 4. Host capability and installer matrix

The last column is deliberately narrow. “Config merge tested” means wcode's
adapter safely created or merged that project file in tests. It does not claim
that every host version passed an end-to-end OAuth session.

| Host | Package | Portable Skill | stdio | Streamable HTTP | SSE | OAuth | One-command install | Manual-only | wcode evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Claude Code | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | Config merge tested |
| OpenAI Codex | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | — | Config merge tested |
| GitHub Copilot CLI | — | ✓ | ✓ | ✓ | varies | varies | ✓ | — | Config merge tested |
| VS Code + Copilot | — | ✓ | ✓ | ✓ | — | ✓ | ✓ | — | Config merge tested |
| Cursor | — | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | Config merge tested |
| Gemini CLI | — | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | Config merge tested |
| Qwen Code | — | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | Config merge tested |
| Kiro | — | ✓ | ✓ | ✓ | varies | ✓ | ✓ | — | Config merge tested |
| Qoder CLI | — | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | Config merge tested |
| Cline | — | ✓ | ✓ | varies | varies | varies | — | ✓ | Application settings only |
| Kimi Code CLI | — | ✓ | ✓ | ✓ | varies | ✓ | — | ✓ | Vendor-documented transport |
| OpenCode | — | ✓ | ✓ | ✓ | — | ✓ | ✓ | — | V1/V2 config merge tested |
| Roo Code | — | ✓ | ✓ | ✓ | varies | varies | — | ✓ | Vendor-documented transport |
| Continue | — | ✓ | ✓ | varies | varies | varies | — | ✓ | Schema/version dependent |
| ZCode | ✓ | ✓ | ✓ | varies | varies | varies | — | ✓ | Package validation only |
| Grok Build | — | ✓ | ✓ | varies | varies | varies | — | ✓ | Manual binding |
| Windsurf | — | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | Vendor-documented transport |
| JetBrains / Junie | — | ✓ | ✓ | varies | varies | varies | — | ✓ | UI setup only |
| Zed | — | ✓ | ✓ | varies | varies | varies | — | ✓ | JSONC left untouched |
| TRAE | — | ✓ | ✓ | ✓ | ✓ | varies | — | ✓ | OAuth not claimed |
| CodeBuddy | — | ✓ | ✓ | varies | varies | varies | — | ✓ | Unknown schema left untouched |
| ChatGPT / Claude / Grok / Mistral web | — | — | — | ✓ | varies | ✓ | — | ✓ | Account setup |

Safe automatic targets are `.mcp.json`, `.codex/config.toml`,
`.vscode/mcp.json`, `.cursor/mcp.json`, `.gemini/settings.json`,
`.qwen/settings.json`, `.kiro/settings/mcp.json`, and `opencode.json`. A Host
must have detection evidence before wcode creates its file. OpenCode V1 and V2
containers are detected before the `wcode` entry is merged.

## 5. Manual hosts

- **Kimi Code CLI:** add the absolute stdio command through the current MCP UI
  or CLI. wcode does not assume a repository schema.
- **Cline:** use its MCP settings screen or CLI. Current MCP state is
  application-level, so the repository installer leaves it alone.
- **Roo Code and Windsurf:** use the workspace MCP settings page. Extension
  global state is left alone.
- **Continue:** common installations use YAML. Manual setup is safer than a
  lossy rewrite.
- **Zed:** project settings may be JSONC; preserve the comments.
- **JetBrains / Junie, TRAE, and CodeBuddy:** use the current IDE MCP page.
- **ZCode:** install the exported package, then bind stdio to the source
  repository.
- **Grok Build:** copy the portable Skill and add an explicit repository-bound
  stdio server.

“Manual” is a result, not a failed installation disguised as success.

## 6. Web clients, tunnels, and OAuth sessions

ChatGPT Web, Claude Web, Grok Web, and Mistral use account-level connector
settings. Paste the current HTTPS `/mcp` URL and complete OAuth in the browser.
No local repository file can safely perform that account action.

OAuth client registrations, access tokens, and refresh tokens do not expire by
time. wcode stores them in the user's state directory, scoped by the configured
Workspace roots, and reloads them after a restart. A tunnel admitted by the
instance-matched health check can continue the session; refreshing through a
replacement hostname moves the token binding to that hostname.

The authorize page, token endpoint, and metadata use the exact Host from the
incoming request, so tunnel B cannot redirect the browser to tunnel A. Unknown
hosts remain rejected. A historical token resource does not make an old tunnel
hostname active again.

v0.4.3 can recover a pre-v0.4.3 `wcode-<uuid>` client registration the next time
that client completes `/authorize`. The redirect URI is still validated, and
the registration is not stored until the local pairing code succeeds.

Clients without OAuth should use local stdio, a local bridge, or a trusted
reverse proxy. The `/sse` compatibility endpoint still requires OAuth, Origin
validation, and the normal Workspace boundary.

## 7. A compact coding flow

A useful default sequence is:

```text
workspace_info → agent_context(goal, scopes=...) → symbol_context
    ↓
read_file / apply_edits
    ↓
review_changes → verify_project → evidence_status
```

Use `search_many` and `read_files` when the inputs are already known; use
parallel tool calls only for genuinely independent work. `agent_context` is the
compact first pass. `symbol_context` adds syntax detail. `apply_edits` keeps one
SHA precondition across a group of non-overlapping edits.

Some model APIs expose server-tool features such as `defer_loading`. That is an
API connector option, not a Claude Code, Codex, or generic MCP project setting.
Do not copy API-only JSON into a host's local MCP configuration.

## 8. Workspace and command authorization

The configured root is the outer boundary. Discovered project markers create
derived subspaces inside it; a host can select `Rust/wcode` without registering
a manually overlapping root. Relative WebUI paths resolve from the selected
Workspace, and symlink children are rejected.

Command approval has two layers:

1. **Executable access** allows one program name in one Workspace.
2. **Exact repository operation** allows one argument fingerprint in that same
   Workspace.

Approving `cargo` does not approve arbitrary `cargo` arguments. An approval for
`cargo test` does not cover `cargo fmt`, another Workspace, or another
subspace. Denial creates no grant.

Semantic-provider trust is separate from those command labels. A non-automatic
warm LSP uses `RiskyExecution` scoped to Workspace + Provider + current
provider-binary identity; approving it allows refresh/navigation to reuse that
exact provider, not a replacement binary, another provider, or an unrelated
repository operation.

## 9. Troubleshooting

- Run `agent-plugin --install-all --dry-run --json` and inspect `evidence`,
  `target`, and `guidance` for the Host.
- If OAuth opens the wrong domain, reconnect using the current tunnel URL. The
  metadata response should contain that same origin.
- If a temporary tunnel hostname changed after a runtime restart, authorize the
  new resource or use a stable `--public-url`.
- If `/mcp` returns 401, follow the `WWW-Authenticate` resource metadata rather
  than adding a static token.
- If an old client needs SSE, configure `/sse`; the server sends the matching
  `/message?sessionId=...` endpoint as its first event.
- If a retry is still blocked, check whether the pending request is for
  executable access, an exact repository operation, or a semantic-provider
  session, then approve only that matching request in the selected Workspace.

## 10. Primary references

- [Agent Plugins 1.0 specification](https://agent-plugins.org/specification)
- [Agent Plugins MCP servers](https://agent-plugins.org/plugin-authors/mcp-servers)
- [MCP legacy SSE transport](https://modelcontextprotocol.io/specification/2024-11-05/basic/transports)
- [MCP backwards compatibility](https://modelcontextprotocol.io/specification/2025-03-26/basic/transports)
- [Claude Code MCP](https://code.claude.com/docs/en/mcp)
- [OpenAI Codex MCP](https://learn.chatgpt.com/docs/extend/mcp?surface=cli)
- [GitHub Copilot CLI](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference)
- [VS Code MCP configuration](https://code.visualstudio.com/docs/agents/reference/mcp-configuration)
- [Gemini CLI](https://google-gemini.github.io/gemini-cli/docs/cli/tutorials.html)
- [Qwen Code MCP](https://qwenlm.github.io/qwen-code-docs/en/users/features/mcp/)
- [Kiro MCP](https://kiro.dev/docs/mcp/configuration/)
- [Qoder CLI MCP](https://docs.qoder.com/cli/mcp-reference)
- [OpenCode MCP](https://opencode.ai/v2/docs/mcp-servers/)
- [Cline configuration](https://docs.cline.bot/getting-started/config)
