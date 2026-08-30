<p align="center">
  <img src="docs/assets/wcode-logo.svg" alt="wcode" width="320">
</p>

<p align="center">
  <a href="https://github.com/francis-du/wcode/actions/workflows/release.yml"><img src="https://github.com/francis-du/wcode/actions/workflows/release.yml/badge.svg" alt="Build and release"></a>
  <a href="https://github.com/francis-du/wcode/releases"><img src="https://img.shields.io/github/v/release/francis-du/wcode?display_name=tag&amp;color=8b7cff" alt="GitHub release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-665cff.svg" alt="Apache-2.0"></a>
  <a href="https://wcode.francis.run/"><img src="https://img.shields.io/badge/docs-wcode.francis.run-f05aa6" alt="Documentation"></a>
</p>

wcode is a local software-intelligence runtime for code repositories. MCP
clients get bounded repository tools, while design, implementation state, and
verification evidence remain in the repository runtime instead of a chat.

You choose the Workspace. wcode owns the filesystem and command boundary;
Claude Code, Codex, Copilot, Cursor, Gemini CLI, Qwen Code, Kiro, OpenCode, and
other clients connect through MCP.

- [Documentation](https://wcode.francis.run/docs/)
- [中文文档](https://wcode.francis.run/zh/docs/)
- [Agent and MCP setup](https://wcode.francis.run/docs/code-agent-integrations/)
- [Software Intelligence](https://wcode.francis.run/docs/software-intelligence/)
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

## Start

```bash
wcode --workspace "$PWD"
```

The TUI opens immediately. In the normal configuration wcode also starts the
local HTTP service, protected WebUI, OAuth server, and managed HTTPS tunnels.
Every tunnel is accepted only after its public `/healthz` response matches the
current runtime instance.

The selected root remains the security boundary. Project markers below it are
discovered as subspaces, so a broad root such as `~/Code` can safely expose
`Rust/wcode`, `Rust/scopwis`, or `Web` without turning manually overlapping
workspaces into the default.

## Connect a client

Preview every safe project-local change:

```bash
wcode --workspace "$PWD" agent-plugin --install-all --dry-run
```

Apply the plan:

```bash
wcode --workspace "$PWD" agent-plugin --install-all
```

The installer detects known hosts, reports its evidence, and returns separate
`installed`, `updated`, `already_configured`, `manual`, `unsupported`, and
`failed` results. JSON and TOML files are parsed and merged; other MCP servers
are preserved. Unknown schemas, JSONC, and YAML stay untouched when a safe
merge cannot be guaranteed. No shell command is run, no third-party package is
downloaded, and no token is written to the repository.

Current project-local adapters cover Claude Code, OpenAI Codex, GitHub Copilot
CLI, VS Code + Copilot, Cursor, Gemini CLI, Qwen Code, Kiro, Qoder CLI, and
OpenCode. The report gives explicit manual steps for Cline, Kimi Code CLI, Roo
Code, Continue, ZCode, Grok Build, Windsurf, JetBrains/Junie, Zed, TRAE,
CodeBuddy, and account-level web connectors.

### MCP transports

| Use case | Transport | Endpoint and authentication |
| --- | --- | --- |
| Local MCP client | stdio | `wcode --workspace /absolute/repo mcp-stdio` |
| Remote client, preferred | Streamable HTTP | `https://host/mcp` + OAuth |
| Older remote client | SSE compatibility | `GET /sse` + `POST /message?sessionId=...` + OAuth |

All transports call the same JSON-RPC dispatch, Harness, Workspace policy, and
authorization system. SSE is a compatibility layer, not a second tool runtime.
wcode does not add anonymous or static-secret fallback just because a client
cannot complete OAuth; use stdio, a local bridge, or a trusted reverse proxy for
that client.

### Tunnel and OAuth sessions

OAuth client registrations and tokens have no clock expiry. wcode stores them
in the user's state directory, separated by the configured Workspace roots,
and reloads them after a restart. A replacement tunnel can continue the same
session after it passes the instance health check.

Authorization pages and metadata still use the domain that received the
request: `tunnel-b.example` never sends approval to `tunnel-a.example`. An old
token can be recognized as a historical resource binding without making its
old hostname active again. Unknown hosts remain outside the boundary. A stable
`--public-url` is still useful because it avoids changing the URL configured in
the client.

## Export the portable plugin

`wcode-agent-plugin/` is the source of truth for the package, README, connection
notes, manifests, and Skill. The Rust exporter embeds those files instead of
maintaining another copy.

```bash
# Safe package with no MCP target
wcode --workspace "$PWD" agent-plugin --profile skill-only

# stdio bound to this exact repository
wcode --workspace "$PWD" agent-plugin --profile local-stdio

# Remote URL only; OAuth credentials stay in the client
wcode --workspace "$PWD" agent-plugin \
  --profile remote-http \
  --remote-url https://current-tunnel.example/mcp
```

The standard `mcp.json` is present in every export. `skill-only` leaves its
server map empty. `local-stdio` writes the canonical absolute Workspace, never
the plugin directory. `remote-http` accepts HTTPS without credentials, query,
or fragment and never embeds an OAuth token.

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
TypeScript, and TSX. Hardened first-party semantics are enabled by default when
an eligible installed provider is present; the first automatic profile is
`rust-analyzer`. Automatic workers stay bounded, run only for the most-specific
project Workspaces, and keep stale semantic revisions out of graph consumers.
A bounded warm session is reused across indexing and `semantic_navigation`, with
SHA-driven `didOpen` / `didChange` / `didClose` synchronization. Agents keep
Tree-sitter/search for simple localization and use the warm LSP path for
cross-file references, callers, implementations, and semantic impact. Use
`--no-semantic` to disable every first-party language server. Providers without
an automatic safety profile retain explicit execution trust. Semantic precision
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
wcode --workspace "$PWD" intelligence
wcode --workspace "$PWD" intelligence --check --json
wcode --workspace "$PWD" verification
wcode --workspace "$PWD" verification --plan-id VP-...
```

`intelligence --check` is the repository gate for initialized, valid Design
State and complete required traceability. Repository-aware language servers and
advanced verification stages can require an exact local authorization before
they run.

## Runtime control

```bash
wcode restart
wcode stop
```

These commands use the per-user authenticated runtime control file. Restart
restores the original arguments after cleaning up the terminal, server, and
owned tunnel processes.

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
