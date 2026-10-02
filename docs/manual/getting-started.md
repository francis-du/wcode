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

For a local coding agent, setup is **install → `wcode setup` → reconnect the agent**. The agent starts its own stdio process; a separate HTTP server or public tunnel is not required. Cloud/web connectors instead use a running `wcode` service and its verified public MCP address.

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
repository. Both configure `wcode mcp-stdio` with the selected performance and
restrictive safety options, preserve unrelated servers, and fail closed on unknown schemas. The binary embeds the canonical Skill and
plugin metadata, so setup does not depend on a `plugin/` directory in the
current repository. Use `wcode setup --dry-run` for a no-write preview.
To save the faster preset for detected agents, preview with
`wcode setup --performance fast --dry-run`, then run `wcode setup --performance fast`
and reconnect the agent. Add `--project` to keep the configuration in this repository.

## 3. Start and connect

For a cloud/web connector or the standalone TUI/WebUI, start the service below.
Local agents configured in step 2 launch stdio themselves; do not start a second
service unless you need those additional surfaces.

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

OAuth client registrations remain persistent without a clock TTL. Access tokens expire after one hour; responses advertise `expires_in: 3600` with `Cache-Control: no-store` and `Pragma: no-cache`. Refresh tokens have a 30-day idle TTL, renewed only by successful refresh rotation. Rotation removes the same grant's old access tokens and preserves other owners. Expired, zero or future issuance timestamps fail closed. State is stored for the configured Workspace roots; restart and migration retain original issuance times rather than extending token lifetime. A replacement tunnel can continue a still-valid session after its current-instance health check; authorization stays on the request's domain.

Local operators can inspect sessions with `GET /oauth/sessions` and revoke a `session_id` through `POST /oauth/sessions/revoke`. These administration APIs require the existing Host/Origin checks and current `X-Wcode-UI-Token`, not an ordinary MCP bearer. Session listings expose no credentials. A successful revoke is persisted across restart; a storage error removes live credentials but does not confirm durable revocation. They are not an RFC 7009 endpoint or Team ACL/SSO; Team ACL and SSO are not implemented.

## 4. Add Design gradually to an existing project

You can use source search, guarded editing and native checks before modeling the whole repository. Global setup configures the agent only. Explicit project setup (`wcode setup --project`, or the interactive Current project choice) seeds a completely missing Design with `.wcode/project.yaml` and an empty `.wcode/design/` directory. The project name comes from the directory; its description is empty and it has no Policy, invented Product vision, requirements or component mappings.

`--dry-run` reports the planned seed without writing. Existing, partial or invalid Design material is preserved and reported for inspection; setup does not reset or silently repair it. Starting wcode and ordinary read-only tools do not perform this initialization. A bare `.wcode` directory is not a completed Design, traceability proof or passing Acceptance.

Ask your connected agent to onboard one real behavior at a time; you do not need to write a YAML model by hand. It should show a draft and ask about unknown business intent:

1. Have the agent read the README, manifests and CI configuration, then inspect actual code with `software_graph`, `file_outline` and `find_symbol`; `project_context` discovers real checks. Confirm its proposed intended behavior and leave unknown intent explicit, instead of converting every observed implementation into a requirement.
2. Declare one Requirement → Component → implementation path/symbol, and Requirement → Acceptance → actual test/check. Mapping lives in these records; there is no separate `mappings.yaml` format.
3. Add missing files with `create_files`. For existing files, `read_files` supplies their SHA and `apply_file_edits` applies guarded updates. There is no `design_update` tool. A completely uninitialized workspace can explicitly use `design_init` for its fuller Product/core-constraint scaffold; after project setup has seeded metadata, extend it rather than calling initialization again.
4. Check `design_status`, then `traceability_status` and `drift_status`. `reconciliation_plan` can turn gaps into a persisted task plan; it does not edit or automatically fix the project. Review changes and run `verify_project`; only actual evidence for the current code-plus-Design Revision proves execution.

The agent's small Rust draft could use these collection-file shapes. It must replace the example behavior, source path and test symbol with inspected facts; this is a format example, not automatic whole-repository Design generation:

```yaml
# .wcode/design/requirements.yaml
- schema_version: 1
  id: REQ-SESSION
  title: Reject expired sessions
  intent: An expired session cannot access the service.
  implemented_by: [component:session]
  acceptance: [AC-SESSION]

# .wcode/design/components.yaml
- schema_version: 1
  id: component:session
  name: Sessions
  responsibilities: [Validate session lifetime]
  implementation:
    - kind: file
      path: src/session.rs

# .wcode/design/acceptance.yaml
- schema_version: 1
  id: AC-SESSION
  title: Expired session is rejected
  statement: The expired-session regression test rejects access.
  verification:
    - kind: test
      path: tests/session.rs
      symbol: rejects_expired_session
```

Collections are YAML lists; a single item under `design/requirements/`, `design/components/` or `design/acceptance/` is one object instead. IDs must be unique and references must resolve. A component can use `{kind: symbol, path: src/session.rs, symbol: validate_session}` for a discovered symbol; Acceptance can use `{kind: check, id: rust-test}` only when that check is actually discovered. A resolvable mapping is not a test run or a Pass.

Project setup may also suggest an Acceptance Policy draft. Only a separate interactive TTY confirmation can write that draft; dry-run/JSON does not accept it, and existing Policy fields are preserved. Activating Policy is a separate native preview plus exact operator approval flow. Neither metadata initialization nor installation confirmation activates Policy.

Inspect local state with:

```bash
wcode intelligence
wcode intelligence --check --json
```

Use the strict `--check` gate after addressing the declared Design and coverage gaps; an incomplete onboarding state should remain incomplete. See [Software Intelligence](../software-intelligence/) for the full schema and verification workflow.

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

Reuse discovered tool schemas and context already supplied by `agent_context`; `project_context` is not a second mandatory startup call. Parallel lane counts are hints bounded by the runtime cap, not proof that writes are independent. In a compact `parallel_tools` batch, ready successors start on completion rather than waiting for a whole layer; failed dependencies skip their successors while independent work continues.

After editing:

```text
review_changes
verify_project
```

Add drift / impact / risk / reconciliation / evidence inspection when the change or readiness requires it. Risk-adaptive Verification and Evidence are the approval layer; model confidence does not replace them.

## 6. Use the local operator surfaces

The TUI prioritizes connection state and subspace activity; the redundant
OVERVIEW panel is removed. Short terminals use a compact header, and 30-second
throughput appears only when recent traffic and available space justify it.
The useful keys are:

- `I` — Repository Intelligence overlay.
- `W` — open the protected Project Status for the focused Workspace.
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

### Navigate the Project Status

Select the specific project in the workspace selector before inspecting its state. The default **Engineering architecture** tab starts with the system map; component and dependency details are available from that view.

| Tab | Question it answers |
| --- | --- |
| Overview | What needs attention, and what has happened recently? |
| Engineering architecture | Which systems and components own the code, and how do they relate? |
| Task activity | What is running or queued, and where is time or memory being used? |
| Verification evidence | What evidence exists for this revision, and what remains unresolved? |
| Current changes | Which files changed, what do they affect, and which checks are needed? |
| Requirements | How does a requirement connect to implementation and proof? |
| Project files | Which source files are present in the snapshot, and which are largest? |

The language and theme buttons are in the top bar. **Access** opens project and command permissions. On narrow screens, scroll the tab row to reach additional views.

Read status labels before interpreting the numbers:

- **Mapped** means a verification reference resolves; **Executed** means qualifying evidence exists.
- **Passed** describes the effective results for the observed revision; **Fresh** means the evidence matches the current code and design.
- **Syntax** describes Tree-sitter relationships. Semantic precision requires a live provider and matching source.
- **Truncated** means a bounded view is partial. An unavailable snapshot or unknown Git state does not mean an empty repository.

If the view looks stale, check the selected workspace and refresh status, then use **Refresh**. Pausing **Auto refresh** stops automatic updates, including queued background rebuilds. You can still use **Refresh** while paused; it completes the requested update even when a cached snapshot appears first. A **Cached snapshot · refreshing…** label means the displayed snapshot is still being updated, not that current-revision proof has passed. Hidden tabs pause polling; returning to the page resumes it when auto refresh is enabled. Use the current protected URL opened by **W** if authorization has expired with the runtime.

## 7. Common modes

Use `wcode help-all` to see every supported CLI command and parameter, including
advanced options hidden from standard help. `wcode help-all setup` focuses on
one command; `wcode help-all --json` returns the catalog for automation.
Help never starts services or runs the selected command.

Keep the default balanced budget, or choose `wcode --performance fast` for more
capacity and `wcode --performance light` for a smaller budget. Check the resolved
settings without starting anything using `wcode --show-config`. The same preset
works in a Host command: `wcode mcp-stdio --performance fast`. See the
[configuration reference](../reference/#simple-performance-configuration) for
budgets, explicit overrides and what needs a new process.

```bash
wcode --read-only
wcode --no-exec
wcode --no-semantic
wcode --no-monitor
wcode --open
```

Keep the default security and resource posture unless the task genuinely needs a narrower or operator-level override. Advanced transport/resource flags live in the [CLI & MCP Reference](../reference/); trust-boundary controls live in [Security](../security/).
