---
layout: docs
title: CLI & MCP Reference
description: Canonical wcode CLI, operator controls, transports, and MCP capability reference
lang: en
alternate: /zh/docs/reference/
permalink: /docs/reference/
---

# CLI & MCP Reference

This page is the canonical compact reference for day-to-day wcode operation. Conceptual behavior lives in the focused guides; this page answers “what do I run or call?”.

## Stable CLI command surface

No subcommand starts the normal runtime:

```bash
wcode
```

The default command catalog is intentionally task-level and model-readable:

```bash
wcode setup
wcode update
wcode mcp-stdio
wcode intelligence --help
wcode verification --help
```

The generated `help` subcommand is disabled; use `--help` and
`<command> --help`. `agent-plugin` remains available only for advanced package
export and older automation. Process lifecycle belongs to the terminal/OS or
the MCP Host; `restart` and `stop` are no longer public wcode commands.

`wcode update` targets the directory containing the running executable unless
`WCODE_INSTALL_DIR` explicitly overrides it. It reuses the release installer
contract: download release artifacts, verify SHA-256, stage the candidate, run
`--version` and `--help`, and replace only after those checks pass. Windows waits
for the running executable to exit before replacement.

Local Software Intelligence views:

```bash
wcode intelligence
wcode intelligence --check --json
wcode verification
wcode verification --plan-id VP-...
```

Local coding agents should normally use stdio. From inside the repository:

```bash
wcode mcp-stdio
```

Generated Host configs use the same path-free command:

```bash
wcode mcp-stdio
```

The Host working directory is the default Workspace. `--workspace` remains an
explicit operator override, not an installation requirement.

Cloud/web connectors use the protected public `/mcp` endpoint shown by the runtime.
Older clients can use `/sse`; the first SSE event supplies the matching
`/message?sessionId=...` endpoint. Both remote transports require OAuth and
Origin validation.

When a stdio tool hits a human authorization gate, a client that advertises
form elicitation can approve or decline inside the MCP Host. MCP 2026 uses
`input_required` MRTR; compatible 2025-era stdio sessions use
`elicitation/create`. The response is validated against the pending request,
opaque challenge, and MCP client owner before the existing AuthorizationManager
creates a grant. Clients without form elicitation receive a missing-capability
failure; wcode never turns that limitation into implicit approval.

Agent setup:

```bash
wcode setup
wcode setup --dry-run
wcode setup --project
wcode setup --json
```

The hidden `agent-plugin` command remains available for advanced portable package
export (`skill-only`, `local-stdio`, or `remote-http`) and compatibility with
older automation. It is not required for normal local setup, and setup does
not require the repository source `plugin/` directory because those assets are
embedded in the binary.

## Common CLI options

| Option | Purpose |
| --- | --- |
| `-w, --workspace <PATH>` | Override the default current-directory Workspace. Repeat only when one task genuinely needs multiple roots. |
| `--read-only` | Remove model-facing file mutation capabilities. |
| `--no-exec` | Disable command execution. |
| `--no-semantic` | Disable automatic first-party LSP indexing and semantic-provider execution. |
| `--full-access` | Explicitly expose the current-user Home and enable otherwise-authorizable runtime capabilities; hard protected-path/symlink/hard-link/no-shell/filesystem-root boundaries remain. |
| `--no-tunnel` | Keep the runtime local-only. |
| `--no-monitor` | Disable the live terminal dashboard. |
| `--open` | Open Setup Hub in the browser after startup. |

### Advanced operator options

The runtime still accepts low-frequency deployment and tuning controls such as
`--public-url`, `--tunnel-provider`, `--imessage-to`, `--max-parallel-tools`,
`--max-cpu-percent`, `--max-memory-mb`, `--allow-sleep`, and
`--allow-risky-exec`. They are hidden from default `--help` so ordinary users
and coding agents do not treat them as required setup parameters. Broader
Workspace/destructive-write trust controls are documented in [Security](../security/).

## TUI shortcuts

| Key | Action |
| --- | --- |
| `I` | Open Software Intelligence. |
| `W` | Open the protected Project Observatory for the focused Workspace. |
| `O` | Reopen Setup Hub. |
| `L` | Switch TUI language. |
| `+` | Add a Workspace. |
| `↑ / ↓` | Select a pending authorization request. |
| `Y / N` | Approve or deny the selected request. |
| `P` | Open the explicit Full Access confirmation. |

## Recommended MCP workflow

For normal coding work:

```text
agent_context(goal, scopes=...)
  ↓
follow readiness / next_actions
  ↓
symbol_context only when more source is required
  ↓
apply_edits or apply_file_edits
```

`agent_context` uses adaptive bounded sizing when `budget` is omitted. Use `workspace_info`, `scope_status`, `design_status`, `project_context`, `software_context`, and `language_quality_status` only for deeper inspection rather than as a mandatory startup sequence.

After the edit:

```text
review_changes
  ↓
drift_status + impact_analysis + risk_status
  ↓
language_quality_run / reconciliation_plan as needed
  ↓
verification_plan + verify_project + required stages
  ↓
evidence_status
```

## MCP capability map

### Workspace and project discovery

| Tool | Use it for |
| --- | --- |
| `workspace_info` | Workspace roots, permissions, security policy, scheduler capabilities, Product Scope registry. |
| `project_context` | Project type, repository guidance, inferred checks, bounded convention report. |
| `scope_status` | Product Scope mapping and bounded unmapped source paths. |
| `convention_status` | Naming, architecture-domain, oversized-module and repository-structure findings. |

### Desired state and context

| Tool | Use it for |
| --- | --- |
| `design_init` | Create sparse Design State without overwriting existing design files. |
| `design_status` | Validate structured Desired State. |
| `traceability_status` | Requirement → Component → implementation and Acceptance → verification coverage. |
| `agent_context` | Primary coding entry point: adaptive/explicit token budget, relevant design, scope-aware repo-map, bounded hot source, SHA edit targets, active Worklist recovery, verification refs, readiness and next actions. |
| `worklist_status` / `worklist_update` | Recover and update the durable model work plan across reconnects/model switches without dropping unfinished items; updates are revision-guarded. |
| `software_context` | Deeper budget-aware software-intelligence context with optional Product Scope narrowing and graph context. |

### Source navigation and bounded I/O

| Tool | Use it for |
| --- | --- |
| `file_outline` / `find_symbol` / `symbol_context` | Tree-sitter definition navigation with explicit syntax precision. |
| `search_code` / `search_many` | Exact repository discovery; prefer the bulk form when queries are known together. |
| `read_file` / `read_files` | Original-format UTF-8 reads with SHA-256 edit preconditions, bounded to at most 1,000 source lines per file/call. |
| `read_media` | Metadata-first bounded media inspection; binary content requires explicit client capability. |
| `parallel_tools` | Fan out already-known independent reads/discovery/writes through path-resource scheduling. |

### Workspace mutation

| Tool | Use it for |
| --- | --- |
| `apply_edits` / `write_file` / `replace_text` | Atomic existing-file edits guarded by the observed SHA. |
| `create_file` / `create_files` / `create_directory` | Create new workspace content without overwrite. |
| `move_path` / `move_paths` | Move/rename bounded workspace paths without destination overwrite. |
| `delete_path` | Delete one file or empty directory after exact one-shot local authorization. |
| `run_command` | No-shell policy-checked execution. Non-default/risky operations remain authorization-bound. |

### Graph, semantics, and language quality

| Tool | Use it for |
| --- | --- |
| `software_graph` | Build/persist the composite graph with provider/precision/revision provenance. |
| `graph_history` / `graph_query` / `graph_diff` | Inspect meaningful graph revisions and structural change. |
| `graph_provider_import` / `graph_provider_status` | External SCIP/LSP/compiler/runtime graph facts. |
| `semantic_status` / `semantic_query` | Persistent candidate/confirmed/retired semantic facts. |
| `semantic_record` / `semantic_confirm` / `semantic_retire` | Human-governed semantic lifecycle. |
| `semantic_provider_status` / `semantic_provider_refresh` | Inspect first-party LSP availability/automatic eligibility or force a bounded refresh. Status exposes the selected provider, `canonical`, `available_candidates`, `launch_ready`, and `session_validated`; `runnable` becomes true only after live initialization. Refresh reports canonical→alternate recovery in `fallbacks`. All 22 indexed languages have one tested canonical launch profile. Hardened providers are auto-maintained by default; non-automatic providers retain explicit trust. |
| `semantic_navigation` | Reuse the warm LSP session for symbol-first definition/hover, references, incoming/outgoing calls, implementations, or cross-file impact. Prefer syntax/search tools for simple localization; unavailable providers return explicit syntax fallback; `unsupported` capabilities and LSP `failures` remain separate from successful empty relationship sets. |
| `language_quality_status` / `language_quality_run` | Explicit syntax/semantic/format/lint/type/static/test/security capability matrix and check-only execution. |

### Change, risk, verification, and evidence

| Tool | Use it for |
| --- | --- |
| `review_changes` | Bounded Git review, numstat, whitespace checks, and maintainability signals. |
| `drift_status` | Implementation/design drift against the current working tree. |
| `impact_analysis` | Design mapping plus bounded reverse-call impact. |
| `risk_status` | Structured risks and risk-adaptive verification profile. |
| `reconciliation_plan` | Persist a desired-to-actual convergence plan. |
| `reconciliation_status` / `reconciliation_history` | Recover prior plans across reconnects/restarts. |
| `reconciliation_execution_status` / `reconciliation_claim` / `reconciliation_submit` / `reconciliation_retry` | Durable dependency-aware convergence execution. |
| `verification_plan` | Create risk-adaptive deterministic/stage/reviewer requirements. |
| `verification_claim` / `verification_submit` | Blind independent reviewer jobs. |
| `verification_executor_status` / `verification_execute_stages` | Property/Mutation/Fuzz/Runtime-Canary runner state and execution. |
| `verification_stage_submit` | Attach external stage verdicts/artifact digests. |
| `verification_approve` | Record explicit HumanApproval Evidence for critical plans. |
| `verification_status` / `verification_history` | Read readiness, blockers, stale revision, disagreement, and plan history. |
| `verify_project` | Run inferred quick/full repository quality gates and record deterministic Evidence. |
| `evidence_status` | Read persistent provenance-bearing Evidence for the selected Workspace. |

## Precision rules

Tree-sitter facts are `precision=syntax`. Real LSP facts are `precision=semantic`; deterministic filesystem/design facts and runtime/provider facts retain their own precision. The Project Observatory exposes the active provider/precision instead of presenting syntax fallback as compiler truth.

A missing relationship in a bounded syntax graph is not proof that the relationship does not exist. Negative inference stays advisory unless stronger evidence supports it. See [Software Intelligence](../software-intelligence/) and [Language Quality](../language-quality/).

## Authorization model

The model may request access; it cannot approve itself. Pending requests are decided in the local TUI or token-protected WebUI.

Important distinctions:

- **Executable access (`CommandAccess`)** authorizes a bare executable for one Workspace.
- **Fingerprint-scoped trust (`RiskyExecution`)** authorizes only the requested trust fingerprint in that Workspace and session. Command/repository mutations bind the exact operation and arguments; a non-automatic warm semantic provider binds Workspace + Provider + current provider-binary identity so refresh/navigation can reuse that exact provider without granting a replacement binary or other providers.
- **RuntimeExecutor** covers one exact advanced verification executor operation.
- **Destructive delete** is one-shot and separate from reusable session grants.

Git mutation remains deliberately narrow. Only explicit pathspec `git add`, message-only `git commit -m ...`, and non-force/non-delete `git push <remote> <refspec>` shapes can enter exact approval. An approved push may use the current SSH Agent through wcode's fixed non-interactive SSH command; token environments, credential helpers, AskPass, arbitrary Git configuration and force/delete forms remain blocked.

Known development CLIs receive command-specific policy instead of generic program-wide authorization: `gh`, `just`, `task`, `uv`, `ruff`, `biome`, `deno`, `docker`, `kubectl`, `terraform`, `fd`, `jq`, `cmake`, `ninja`, `dotnet`, `mvn`, `gradle`, `swift`, `zig`, `pre-commit`, and `act`. Strict local read/check shapes can run directly; repository build/runners, Docker/Kubernetes data access, and bounded source/remote mutations enter exact authorization. Kubernetes cluster mutation, Terraform apply/destroy/state-secret surfaces, Gradle/Maven publishing, host toolchain mutation, and command/file-loading escape hatches remain blocked. Rust full verification can prefer declared, installed cargo-nextest while retaining `cargo test` as fallback.

## Diagnostics

The public/local health surface keeps connection state layered rather than flattening everything to “connected/disconnected”. Inspect the runtime TUI first, then `/healthz` when debugging HTTP, tunnel, or OAuth state. OAuth registrations and tokens are restored for the same configured Workspace roots after a process restart. A replacement tunnel can keep that session after it passes the instance health check. Managed tunnel recovery is automatic, but session-style quick-tunnel hostnames may change; use a stable endpoint for durable remote connector configuration. Metadata and authorization must show the same host the client used; unknown or inactive hosts remain rejected.

For repository correctness use:

```bash
wcode intelligence --check --json
```

For implementation quality use the repository-native checks returned by `project_context` / `language_quality_status`, then finish with `verify_project`.

## Related guides

- [Getting Started](../getting-started/)
- [Software Intelligence](../software-intelligence/)
- [Code Agent Integrations](../code-agent-integrations/)
- [Security](../security/)
- [Language Quality](../language-quality/)
- [Maintainability Review](../maintainability-review/)
