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

The normal runtime starts the local MCP service, protected WebUI/Setup Hub, OAuth flow, terminal monitor, and the configured public endpoint path for cloud connectors.

Use one repository root by default. Add another root only when one task genuinely needs more than one repository:

```bash
wcode \
  --workspace ~/Code/backend \
  --workspace ~/Code/frontend
```

## 3. Choose the connection type

### Local coding agent

Prefer stdio when the agent runs on the same machine:

```bash
wcode --workspace /absolute/path/to/repository mcp-stdio
```

The stdio transport skips HTTP OAuth but keeps the same Workspace, command, path, SHA, authorization, verification, and Evidence boundaries.

### Cloud or web connector

Use the public `/mcp` URL shown by wcode. The client discovers OAuth metadata, completes PKCE/DCR where supported, and receives a resource-bound token.

By default `--tunnel-provider auto` starts Cloudflare Quick Tunnel, the SSH-based `localhost.run` and Pinggy providers, and Tailscale Funnel concurrently in the background; the dashboard renders immediately and every tunnel that passes the instance-matched health check is listed live. Providers that miss the first round keep retrying every 15 seconds, and a dead tunnel is respawned alone without disturbing the rest. Force one with `--tunnel-provider cloudflare|localhost-run|pinggy|tailscale`. Quick-tunnel URLs can change after restart; use `--public-url` or the Tailscale provider when you need a stable endpoint.

## 4. Initialize Design State only when useful

A connected agent may call `design_init`. Initialization is sparse: it creates Project/Product state without creating empty requirement/component/constraint/acceptance/decision collections and never overwrites existing Design State.

Inspect it with:

```bash
wcode --workspace "$PWD" intelligence
wcode --workspace "$PWD" intelligence --check --json
```

## 5. Give the agent the right first calls

The strong default before editing is one compact call:

```text
agent_context(goal, scopes=...)
  ↓
follow readiness / next_actions
  ↓
symbol_context only if more source is needed
```

`agent_context` chooses a bounded adaptive budget when `budget` is omitted and can carry the relevant Design State, scope-aware repo map, bounded hot source, SHA edit targets, related tests, and readiness. Use `scope_status`, `design_status`, `project_context`, `software_context`, `language_quality_status`, `read_files`, or `search_many` only when the task needs deeper discovery; use `parallel_tools` only after independent operations are known.

After editing:

```text
review_changes
verify_project
```

Add drift / impact / risk / reconciliation / evidence inspection when the change or readiness requires it. Risk-adaptive Verification and Evidence are the approval layer; model confidence does not replace them.

## 6. Use the local operator surfaces

The TUI remains intentionally small:

- `I` — Intelligence overlay.
- `W` — open the protected Project Observatory for the focused Workspace.
- `O` — reopen Setup Hub.
- `L` — switch TUI language manually.
- `+` — add a Workspace.
- `↑/↓` — select a pending authorization request.
- `Y/N` — approve or deny the selected request.

The protected WebUI exposes the same project and command authorization concepts for browser-based operation.

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
