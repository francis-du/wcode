---
layout: docs
title: Getting Started
description: Install, start, connect, and use wcode with one repository
lang: en
alternate: /zh/docs/getting-started/
permalink: /docs/getting-started/
---

# Get one repository working with wcode

wcode is not another coding agent. It is the local repository layer your existing agent calls when it needs to understand code, follow real cross-file relationships, make guarded changes, or prove the current revision works.

The first successful setup is only three steps: **install → `wcode setup` → run `wcode` from the repository**. Everything else on this page explains what those three steps give you.

## 1. Install

macOS and Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/francis-du/wcode/main/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/francis-du/wcode/main/install.ps1 | iex
```

## 2. Configure your coding agents

```bash
cd /absolute/path/to/repository
wcode setup
```

Interactive `wcode setup` offers **Global (recommended)** first and **Current
project** second. Global mode configures verified user-level Host files so one
setup works across repositories; project mode keeps configuration in this
repository. Both install only `wcode mcp-stdio`, preserve unrelated servers,
and fail closed on unknown schemas. The binary embeds the canonical Skill and
plugin metadata, so setup does not depend on a `plugin/` directory in the
current repository. Use `wcode setup --dry-run` for a no-write preview.

## 3. Start and connect

```bash
wcode
```

The current directory is the default Workspace, so normal use does not need
`--workspace "$PWD"`. This starts the local MCP service, protected WebUI, OAuth
server, TUI, and configured public connectivity. Project markers below the root
become selectable subspaces automatically.

### Local coding agent

Prefer stdio when the agent runs on the same machine:

```bash
wcode mcp-stdio
```

The MCP Host working directory becomes the default Workspace. The stdio
transport skips HTTP OAuth but keeps the same Workspace, command, path, SHA,
authorization, verification, and Evidence boundaries.

### Cloud or web connector

Use the public `/mcp` URL shown by wcode. The client discovers OAuth metadata, completes PKCE/DCR where supported, and receives a resource-bound token.

The transport model is:

- Local MCP: stdio.
- Remote MCP, preferred: Streamable HTTP at `/mcp` with OAuth.
- Legacy remote compatibility: `GET /sse` plus `POST /message` with OAuth.

All three use the same Harness and Workspace policy. SSE does not provide an
anonymous compatibility path.

Managed public connectivity is automatic in the normal runtime. Advanced tunnel provider selection and stable reverse-proxy options are documented in the [CLI & MCP Reference](../reference/); they are not required for the normal local setup path.

OAuth client registrations and tokens have no clock expiry. They are stored in
the user's state directory for the configured Workspace roots and loaded by the
next wcode process. A replacement tunnel can keep the session after it passes
the current instance health check. Authorization always stays on the domain
that received the request, and unknown or inactive hosts are rejected.

## 4. Let the agent start small

You do not need to design the whole repository before the first task. A connected
agent starts with `agent_context`, which defaults ordinary work toward a minimal
change and asks for stronger context only when the task needs it.

If the project benefits from durable requirements or architecture constraints,
call `design_init` then. It creates sparse Project/Product state and practical
baseline constraints without overwriting existing Design State.

Inspect it with:

```bash
wcode intelligence
wcode intelligence --check --json
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

`agent_context` chooses a bounded adaptive budget when `budget` is omitted and can carry the relevant Design State, scope-aware repo map, bounded hot source, SHA edit targets, related tests, readiness, and explicit parallelism guidance. Models should send only required MCP arguments: omit the default Workspace and server-default path/limit/timeout/budget values. Split work into dependency lanes first; run independent discovery, reads, reviews, and file-local edits as concurrent top-level calls when the Host supports it, while serializing real dependencies. Use `read_files`, `search_many`, `apply_file_edits`, or `create_files` when inputs are already known; reserve nested `parallel_tools` for compact fan-out. For ordinary localization keep using `find_symbol` / `search_code`, and use `semantic_navigation` only when readiness requests stronger cross-file relationships.

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
- `P` — review the explicit Full Access confirmation for current-user Home
  access and all otherwise-authorizable runtime capabilities; hard protected
  paths, symlink/hard-link, no-shell, and filesystem-root boundaries remain.

The protected WebUI exposes the same requests. It labels executable access and
exact repository operations separately; approving one does not imply the
other.

## 7. Common modes

```bash
wcode --read-only
wcode --no-exec
wcode --no-semantic
wcode --no-monitor
wcode --open
```

Keep the default security and resource posture unless the task genuinely needs a narrower or operator-level override. Advanced transport/resource flags live in the [CLI & MCP Reference](../reference/); trust-boundary controls live in [Security](../security/).
