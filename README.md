<p align="center">
  <img src="docs/assets/wcode-logo.svg" alt="wcode" width="320">
</p>

<p align="center">
  <a href="https://github.com/francis-du/wcode/actions/workflows/release.yml"><img src="https://github.com/francis-du/wcode/actions/workflows/release.yml/badge.svg" alt="Build and release"></a>
  <a href="https://github.com/francis-du/wcode/releases"><img src="https://img.shields.io/github/v/release/francis-du/wcode?display_name=tag&amp;color=8b7cff" alt="GitHub release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-665cff.svg" alt="Apache-2.0"></a>
  <a href="https://wcode.francis.run/"><img src="https://img.shields.io/badge/docs-wcode.francis.run-f05aa6" alt="Documentation"></a>
</p>

# Make any coding agent understand your repo before it changes it.

**wcode is not another coding agent. It is the missing repository layer for Claude Code, Codex, Cursor, Copilot, and other agents: task-ready context, real code relationships, guarded actions, and proof tied to the current revision.**

Coding agents are already good at writing code. The expensive failures happen around the code: they miss a caller, guess an architecture boundary, edit too much, run with too much access, or declare success without enough proof. **wcode fixes that layer without asking you to replace the agent you already use.**

> **Understand first. Change less. Prove it works.**

### What wcode fixes

| The problem | What wcode does |
| --- | --- |
| **The agent does not understand the repo** | Builds task-ready context from code, architecture, requirements, tests, conventions, and the active worklist |
| **Search misses real relationships** | Uses LSP for callers, references, implementations, hover, and cross-file impact; precision stays explicit when only syntax is available |
| **A small task turns into a redesign** | Starts ordinary work with a minimal-change strategy and complexity budget, then surfaces scope and maintainability growth |
| **Agent tools have too much machine access** | Keeps work inside governed Workspace roots with no-shell execution, protected paths, SHA-safe writes, and exact human authorization |
| **“Looks good” becomes the merge criterion** | Runs deterministic checks and keeps revision-bound verification and Evidence so proof survives the chat |
| **Every model starts from zero** | Keeps repository intelligence and work state local and reusable across coding agents and sessions |

Claude Code, Codex, Copilot, Cursor, Gemini CLI, Qwen Code, Kiro, OpenCode, and other MCP clients can all use the same local runtime.

<p align="center">
  <a href="docs/assets/wcode-architecture.png"><img src="docs/assets/wcode-architecture.png" alt="wcode Project Observatory architecture view" width="49%"></a>
  <a href="docs/assets/wcode-verification-detail.png"><img src="docs/assets/wcode-verification-detail.png" alt="wcode requirement verification and evidence view" width="49%"></a>
</p>

<p align="center"><sub>Architecture and change impact on the left. Requirement-to-proof traceability on the right.</sub></p>

- [Documentation](https://wcode.francis.run/docs/)
- [中文文档](https://wcode.francis.run/zh/docs/)
- [Agent and MCP setup](https://wcode.francis.run/docs/code-agent-integrations/)
- [Software Intelligence](https://wcode.francis.run/docs/software-intelligence/)
- [v0.6 — Parallel-first agents, authorizable project commands, and stronger repository context](https://wcode.francis.run/docs/releases/v0.6/)
- [Releases](https://github.com/francis-du/wcode/releases)

## Install

macOS and Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/francis-du/wcode/main/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/francis-du/wcode/main/install.ps1 | iex
```

Update an existing installation with the same checksum and smoke-test contract:

```bash
wcode update
```

## Configure and start

From the repository you want an Agent to work in:

```bash
wcode setup
wcode
```

That is the normal path. `wcode setup` configures supported local Hosts with the
path-free `wcode mcp-stdio` command; Global setup is recommended, project scope
is available when repository-local configuration is intentional, and unknown
config schemas fail closed. The binary already embeds the `wcode` Skill and
plugin package, so the target repository needs no plugin directory.

`wcode` uses the current directory as the default Workspace. The TUI opens
immediately and the runtime exposes the same governed repository layer to local
stdio clients and protected remote MCP clients. Advanced tunnel, OAuth,
resource, and deployment details belong in the focused documentation rather
than the first-run path.

The selected root remains the security boundary. Project markers below it are
discovered as subspaces, so a broad root such as `~/Code` can safely expose
`Rust/wcode`, `Rust/scopwis`, or `Web` without turning manually overlapping
workspaces into the default.

## Connect a client

For supported local coding agents, `wcode setup` is the normal configuration
entry point. It reports `installed`, `updated`, `already_configured`, `manual`,
`unsupported`, and `failed` outcomes. JSON and TOML files are parsed and merged;
other MCP servers are preserved. Unknown schemas, JSONC, and YAML stay untouched
when a safe merge cannot be guaranteed. No shell command is run, no third-party
package is downloaded, and no token is written to the repository.

Verified automatic adapters cover Claude Code, OpenAI Codex, GitHub Copilot
CLI, VS Code + Copilot, Cursor, Gemini CLI, Qwen Code, Kiro, Qoder CLI, and
OpenCode across the scopes each Host safely supports. The report gives explicit manual steps for Cline, Kimi Code CLI, Roo
Code, Continue, ZCode, Grok Build, Windsurf, JetBrains/Junie, Zed, TRAE,
CodeBuddy, and account-level web connectors.

### MCP transports

| Use case | Transport | Endpoint and authentication |
| --- | --- | --- |
| Local MCP client | stdio | `wcode mcp-stdio` — Host working directory is the default Workspace |
| Remote client, preferred | Streamable HTTP | `https://host/mcp` + OAuth |
| Older remote client | SSE compatibility | `GET /sse` + `POST /message?sessionId=...` + OAuth |

All transports call the same JSON-RPC dispatch, Harness, Workspace policy, and
authorization system. SSE is a compatibility layer, not a second tool runtime.
For local stdio the Host working directory selects the Workspace; setup does not
bake an absolute repository path into the MCP entry.

MCP calls are intentionally compact in v0.5.2: model-visible schemas omit server
defaults and low-frequency tuning fields, and agents are instructed not to send
the default Workspace or inferable path/limit/timeout/budget values. For speed,
independent dependency lanes should use concurrent top-level MCP calls when the
Host supports them; bulk `read_files`, `search_many`, `apply_file_edits`, and
`create_files` remain the low-noise path when the inputs are already known.
For stdio, command authorization stays human-controlled: MCP 2026 clients use
`input_required` multi-round-trip elicitation, while compatible 2025-era stdio
clients receive `elicitation/create`. The retry is bound to the MCP client,
pending authorization, and an opaque challenge; there is no model-callable
approval tool. If a Host cannot open an approval form, wcode returns the pending `AUTH-...` request ID and tells the user to approve it in the TUI or protected WebUI before retrying; access is never widened automatically. wcode does not add anonymous or
static-secret fallback just because a client cannot complete OAuth; use stdio,
a local bridge, or a trusted reverse proxy for that client.

### Tunnel and OAuth sessions

OAuth client registrations and tokens have no clock expiry. wcode stores them
in the user's state directory, separated by the configured Workspace roots,
and reloads them after a process restart. A replacement tunnel can continue the same
session after it passes the instance health check. Managed tunnels self-heal;
when all public tunnels are unavailable, wcode falls back to the local endpoint
while reconnecting. Temporary Cloudflare/localhost.run/Pinggy hostnames can
change on reconnect, so durable remote connectors should use Tailscale Funnel
or another stable `--public-url`.

Authorization pages and metadata still use the domain that received the
request: `tunnel-b.example` never sends approval to `tunnel-a.example`. An old
token can be recognized as a historical resource binding without making its
old hostname active again. Unknown hosts remain outside the boundary. A stable
`--public-url` is still useful because it avoids changing the URL configured in
the client.

## Export the portable plugin

`plugin/` is the repository source package for manifests, connection notes, and
the canonical `wcode` Skill. The binary embeds these assets at compile time, so
installed users do **not** need a `plugin/` directory beside their repository or
current working directory.

```bash
# Safe package with no MCP target
wcode agent-plugin --profile skill-only

# stdio profile; the Host working directory becomes the Workspace
wcode agent-plugin --profile local-stdio

# Remote URL only; OAuth credentials stay in the client
wcode agent-plugin \
  --profile remote-http \
  --remote-url https://current-tunnel.example/mcp
```

The standard `mcp.json` is present in every export. `skill-only` leaves its
server map empty. `local-stdio` writes only `wcode mcp-stdio`; the MCP Host's
working directory becomes the default Workspace. `remote-http` accepts HTTPS
without credentials, query, or fragment and never embeds an OAuth token.

## Repository capabilities

wcode exposes a compact set of repository operations, not a remote shell:

- discover project type, subspaces, repository instructions, and Product Scopes;
- search files and symbols with bounded Tree-sitter context;
- read text and supported media with explicit size and capability checks;
- make atomic, SHA-256-guarded edits inside the selected Workspace;
- review the current Git change, map impact, and derive risk;
- run project checks under a no-shell command policy;
- persist Design State, Software Graph revisions, Verification Plans, and Evidence.

The syntax index covers Bash, C, C++, C#, CSS, Dart, Elixir, Go, HTML, Java,
JavaScript, Lua, OCaml, OCaml Interface, PHP, Python, R, Ruby, Rust, Swift,
TypeScript, and TSX. v0.5 gives every one of those 22 languages exactly one
tested canonical LSP launch profile; PHP, Python, and Ruby also keep bounded
installed-provider fallbacks when the canonical server fails initialization.
Hardened first-party LSP semantics are enabled by default when an eligible server is installed; the first automatic profile is `rust-analyzer`. Automatic workers stay bounded, run only for the most-specific
project Workspaces, and keep stale semantic revisions out of graph consumers.
A bounded warm session is reused across indexing and `semantic_navigation`; document
sync follows each server's advertised LSP Full / Incremental / None policy instead of
assuming one `didOpen` / `didChange` shape for every language. Agents keep
Tree-sitter/search for simple localization and use the warm LSP path for
cross-file references, callers, implementations, and semantic impact. Use
`--no-semantic` to disable every first-party LSP server. LSP servers without an automatic safety profile retain explicit execution trust. Semantic precision
is reported only after a real server answers for the current source revision;
otherwise wcode says `precision=syntax`.

## Authorization is deliberately two-step

Command authorization has two separate scopes:

1. **Executable access** allows one program name, such as `cargo`, in one
   Workspace.
2. **Exact repository operation** allows one fingerprinted operation, such as
   `cargo test`, in that same Workspace.

Approving `cargo` does not approve every future argument. An exact approval
does not cross into another workspace or subspace. Pending requests can be
approved or denied in the TUI or protected WebUI; denied requests create no
grant.

Path deletion is narrower again: only one regular file or empty directory can
be removed after exact approval. Recursive, root, protected-path, symlink, and
hard-link deletion remains blocked.

## TUI and WebUI

The terminal view uses the same dark violet/pink palette as the documentation
site and keeps the main screen to four layers:

1. current MCP endpoint, tunnel, OAuth session, and runtime status;
2. active, queued, completed, and failed counters;
3. per-subspace work;
4. 30-second throughput and slot use.

Press `I` to load Software Intelligence for the selected project, `C` to see
the complete command catalog, `W` for the protected WebUI, `O` for setup, and
`?` for keys and links. The pairing code stays in the header after a client
connects, so reconnecting does not hide it. Authorization requests state
whether they need executable access or approval for one exact repository
operation.

The WebUI includes a bounded project tree and a largest-files view. Both come
from the same indexed snapshot as the architecture view; files above the
repository's 1,000-line limit are marked, and a partial scan is reported as
truncated instead of being presented as complete.

## Local inspection

An MCP client is optional for status and verification:

```bash
wcode intelligence
wcode intelligence --check --json
wcode verification
wcode verification --plan-id VP-...
```

`intelligence --check` is the repository gate for initialized, valid Design
State and complete required traceability. Repository-aware LSP servers and
advanced verification stages can require an exact local authorization before
they run.

## Runtime lifecycle

Foreground HTTP/TUI runtimes stop through Ctrl-C or SIGTERM. stdio runtimes are
owned by the MCP Host that launched them. Upgrades use `wcode update`; there is
no separate restart/stop control socket or per-user runtime token file.

## Security summary

- only configured Workspaces and controlled derived subspaces are visible;
- traversal, symlink escape, common secrets, VCS internals, and broad roots are
  blocked by default;
- writes are bounded, atomic, and guarded by the caller's current SHA-256;
- commands run without a shell and pass executable plus exact-operation policy;
- remote MCP uses Origin checks, OAuth/PKCE, constrained redirects, rotating
  refresh tokens, and resource-bound access tokens;
- plugin and installer output contains no credentials and does not approve
  RiskyExecution.

See [Security](https://wcode.francis.run/docs/security/) for the full boundary
and [Development](https://wcode.francis.run/docs/development/) for repository
checks and release gates.
