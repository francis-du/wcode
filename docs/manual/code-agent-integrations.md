---
layout: docs
title: MCP Client Integrations
description: Global-first local setup, portable Skill/plugin packaging, and remote MCP connections
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
| Same machine | stdio | `wcode mcp-stdio` (Host working directory = Workspace) |
| Remote, preferred | Streamable HTTP | `https://host/mcp` with OAuth |
| Older remote client | SSE compatibility | `GET /sse` and `POST /message?sessionId=...` with OAuth |

These are three entrances to one implementation. They share JSON-RPC dispatch,
the Harness, Workspace selection, command policy, authorization, Tools,
Prompts, Resources, and Software Intelligence state. SSE is kept for clients
that still implement the 2024 transport; new setups should use `/mcp`.

Local stdio configuration is intentionally path-free:

```json
{
  "command": "wcode",
  "args": ["mcp-stdio"]
}
```

The MCP Host working directory is the default Workspace. This lets one global
Host configuration work across repositories and avoids accidentally treating a
plugin/package directory as source code. `--workspace` is only an explicit
operator override.

### Human authorization over stdio

`stdin` and `stdout` are the MCP protocol channel, so wcode never pauses that
stream to read a terminal yes/no prompt. Instead, a Host that supports form
elicitation receives the authorization request through MCP and presents the
user interaction itself. Protocol 2026 uses `input_required` MRTR; compatible
2025-era stdio uses `elicitation/create`. Approval is accepted only when the
reply matches the pending authorization, an opaque challenge, and the MCP
client owner. There is no MCP tool that lets a model approve its own request.

If the Host does not advertise form elicitation, the gated command fails with a
missing-client-capability error. That is deliberate: unsupported interaction is
not treated as consent.

## 2. Detect and configure local hosts

Install wcode first:

```bash
curl -fsSL https://raw.githubusercontent.com/francis-du/wcode/main/install.sh | sh
```

Then configure local Hosts:

```bash
wcode setup
```

Use `wcode setup --dry-run` for a no-write preview. Add `--json` when another tool needs the structured report. Each result is one
of `detected`, `installed`, `updated`, `already_configured`, `manual`,
`unsupported`, or `failed`, and includes its detection evidence and target
file.

Interactive setup offers **Global (recommended)** first and **Current project**
second. Global setup writes only verified user-level Host configuration paths
after local confirmation; project mode writes recognized repository-local
config. Both install `wcode mcp-stdio` without a repository path and preserve
unrelated servers. Updates use parsed JSON/TOML plus atomic SHA-guarded writes;
unknown/JSONC/YAML shapes fail closed. Setup does not download a plugin, require
a `plugin/` directory in the user's project, store credentials, or approve
RiskyExecution.

## 3. Export the Skill and plugin package

`plugin/` is the canonical source package. The Rust binary embeds its README,
Skill, manifests, and connection notes with `include_str!`, so installed setup
works from any directory even when no package folder exists there.

```bash
# No MCP target; safe to distribute
wcode agent-plugin --profile skill-only

# stdio profile; the consuming Host working directory selects the Workspace
wcode agent-plugin --profile local-stdio

# Publish a credential-free remote profile
wcode agent-plugin \
  --profile remote-http \
  --remote-url https://current-host.example/mcp
```

Every export contains the standard `mcp.json`. The canonical `skill-only`
package has an empty `mcpServers` object. `local-stdio` writes only
`wcode mcp-stdio` and uses the consuming Host working directory. `remote-http`
accepts an HTTPS origin or `/mcp` URL without
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
| Grok Build | — | ✓ | ✓ | varies | varies | varies | — | ✓ | Manual stdio setup |
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

- **Kimi Code CLI:** add `wcode mcp-stdio` through the current MCP UI or CLI.
  The Host working directory selects the Workspace; wcode does not assume a repository schema.
- **Cline:** use its MCP settings screen or CLI. Current MCP state is
  application-level, so the repository installer leaves it alone.
- **Roo Code and Windsurf:** use the workspace MCP settings page. Extension
  global state is left alone.
- **Continue:** common installations use YAML. Manual setup is safer than a
  lossy rewrite.
- **Zed:** project settings may be JSONC; preserve the comments.
- **JetBrains / Junie, TRAE, and CodeBuddy:** use the current IDE MCP page.
- **ZCode:** install the exported package, then configure `wcode mcp-stdio`.
- **Grok Build:** copy the portable Skill and add `wcode mcp-stdio`; the Host
  working directory selects the source repository.

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
agent_context(goal, scopes=...)
    ↓ readiness + parallelism
independent lanes ── concurrent top-level MCP calls
    ↓ true dependencies only
bounded edits → review_changes → verify_project
```

Keep calls compact: omit the default `workspace` and server-default path, limit,
timeout, and budget values. Use `workspace_info` only when multiple roots or
subspaces make the target ambiguous. For edits, use `apply_edits` for one file
and `apply_file_edits` for independent files. When inputs are already known,
prefer `search_many`, `read_files`, `apply_file_edits`, and `create_files` because they
reduce round trips without one giant nested argument object. Use separate
concurrent top-level calls for independent lanes when the Host supports them;
reserve `parallel_tools` for small compact fan-out. `agent_context` is the
compact first pass and now reports an explicit parallelism strategy; call
`symbol_context` only when readiness says more source is needed.

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

LSP trust is separate from those command labels. A non-automatic warm LSP session uses `RiskyExecution` scoped to Workspace + server + current binary identity; approval lets refresh/navigation reuse that exact server, not a replacement binary, another server, or an unrelated repository operation.

## 9. Troubleshooting

- Run `wcode setup --dry-run --json` and inspect `evidence`, `target`, and
  `guidance` for the Host.
- If OAuth opens the wrong domain, reconnect using the current tunnel URL. The
  metadata response should contain that same origin.
- If a temporary tunnel hostname changed after a runtime restart, authorize the
  new resource or use a stable `--public-url`.
- If `/mcp` returns 401, follow the `WWW-Authenticate` resource metadata rather
  than adding a static token.
- If an old client needs SSE, configure `/sse`; the server sends the matching
  `/message?sessionId=...` endpoint as its first event.
- If a retry is still blocked, check whether the pending request is for
  executable access, an exact repository operation, or an LSP
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
