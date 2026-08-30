---
layout: docs
title: Getting Started
description: Install, start, connect, and use wcode with one repository
lang: en
alternate: /zh/docs/getting-started/
permalink: /docs/getting-started/
---

# Getting Started

This page is the shortest path from an existing repository to a working wcode runtime.

## 1. Install

macOS and Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/francis-du/wcode/main/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/francis-du/wcode/main/install.ps1 | iex
```

## 2. Start in a repository

```bash
cd /absolute/path/to/repository
wcode --workspace "$PWD"
```

This starts the local MCP service, protected WebUI, OAuth server, TUI, and any configured public tunnels.

Use one repository root by default. Add another root only when one task genuinely needs more than one repository:

```bash
wcode \
  --workspace ~/Code/backend \
  --workspace ~/Code/frontend
```

Project markers below the root become selectable subspaces. If you start at
`~/Code`, an agent can work in `Rust/wcode` without registering a second,
manually overlapping Workspace.

## 3. Connect an agent

For known local hosts, preview and apply project-local configuration:

```bash
wcode --workspace "$PWD" agent-plugin --install-all --dry-run
wcode --workspace "$PWD" agent-plugin --install-all
```

The first command never writes. The second merges `wcode` into known JSON or
TOML files, preserves other servers, and reports hosts that still need manual
setup.

### Local coding agent

Prefer stdio when the agent runs on the same machine:

```bash
wcode --workspace /absolute/path/to/repository mcp-stdio
```

The stdio transport skips HTTP OAuth but keeps the same Workspace, command, path, SHA, authorization, verification, and Evidence boundaries.

### Cloud or web connector

Use the public `/mcp` URL shown by wcode. The client discovers OAuth metadata, completes PKCE/DCR where supported, and receives a resource-bound token.

The transport model is:

- Local MCP: stdio.
- Remote MCP, preferred: Streamable HTTP at `/mcp` with OAuth.
- Legacy remote compatibility: `GET /sse` plus `POST /message` with OAuth.

All three use the same Harness and Workspace policy. SSE does not provide an
anonymous compatibility path.

By default `--tunnel-provider auto` starts Cloudflare Quick Tunnel, the SSH-based `localhost.run` and Pinggy providers, and Tailscale Funnel concurrently in the background; the dashboard renders immediately and every tunnel that passes the instance-matched health check is listed live. Providers that miss the first round keep retrying every 15 seconds, and a dead tunnel is respawned alone without disturbing the rest. Force one with `--tunnel-provider cloudflare|localhost-run|pinggy|tailscale`. Quick-tunnel URLs can change after restart; use `--public-url` or the Tailscale provider when you need a stable endpoint.

OAuth client registrations and tokens have no clock expiry. They are stored in
the user's state directory for the configured Workspace roots and loaded again
after `wcode restart`. A replacement tunnel can keep the session after it
passes the current instance health check. Authorization always stays on the
domain that received the request, and unknown or inactive hosts are rejected.

## 4. Initialize Design State only when useful

A connected agent may call `design_init`. It creates Project/Product state and
three practical baseline constraints for module size, test placement, and
Design-reference updates. Other collections stay absent until they carry real
project decisions. Existing Design State is never overwritten.

Inspect it with:

```bash
wcode --workspace "$PWD" intelligence
wcode --workspace "$PWD" intelligence --check --json
```

## 5. Give the agent the right first calls

Before editing, start with one compact call:

```text
agent_context(goal, scopes=...)
  ↓
follow readiness / next_actions
  ↓
semantic_navigation only for recommended cross-file relationships
  ↓
symbol_context only if more source is needed
```

`agent_context` chooses a bounded adaptive budget when `budget` is omitted and can carry the relevant Design State, scope-aware repo map, bounded hot source, SHA edit targets, related tests, and readiness. For ordinary localization keep using `find_symbol` / `search_code`; when readiness flags a syntax-only cross-file relationship task, `semantic_navigation` reuses the warm LSP session for references, callers, callees, or implementations. Use `scope_status`, `design_status`, `project_context`, `software_context`, `language_quality_status`, `read_files`, or `search_many` only when the task needs deeper discovery; use `parallel_tools` only after independent operations are known.

After editing:

```text
review_changes
verify_project
```

Add drift / impact / risk / reconciliation / evidence inspection when the change or readiness requires it. Risk-adaptive Verification and Evidence are the approval layer; model confidence does not replace them.

## 6. Use the local operator surfaces

The TUI main screen is split into connection state, four runtime counters,
subspace activity, and 30-second throughput. The useful keys are:

- `I` — Intelligence overlay.
- `W` — open the protected Project Observatory for the focused Workspace.
- `O` — reopen Setup Hub.
- `L` — switch TUI language manually.
- `+` — add a Workspace.
- `↑/↓` — select a pending authorization request.
- `Y/N` — approve or deny the selected request.

The protected WebUI exposes the same requests. It labels executable access and
exact repository operations separately; approving one does not imply the
other.

## 7. Common modes

```bash
wcode --workspace "$PWD" --read-only
wcode --workspace "$PWD" --no-exec
wcode --workspace "$PWD" --no-monitor
wcode --workspace "$PWD" --open
wcode --workspace "$PWD" --tunnel-provider localhost-run
wcode --workspace "$PWD" --tunnel-provider pinggy
wcode --workspace "$PWD" --tunnel-provider tailscale
wcode --workspace "$PWD" --public-url https://mcp.example.com
```

Keep the default security boundary unless the task requires a narrower, explicit exception. See [Security](../security/) and [Code Agent Integrations](../code-agent-integrations/) for deeper configuration.
