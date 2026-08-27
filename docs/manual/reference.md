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

## Start and control the runtime

The normal path is intentionally small:

```bash
wcode --workspace "$PWD"
```

Useful control commands:

```bash
wcode restart
wcode stop
```

Local Software Intelligence views:

```bash
wcode --workspace "$PWD" intelligence
wcode --workspace "$PWD" intelligence --check --json
wcode --workspace "$PWD" verification
wcode --workspace "$PWD" verification --plan-id VP-...
```

Local coding agents should normally use stdio:

```bash
wcode --workspace /absolute/path/to/repository mcp-stdio
```

Cloud/web connectors use the protected public `/mcp` endpoint shown by the runtime.

## Common CLI options

| Option | Purpose |
| --- | --- |
| `-w, --workspace <PATH>` | Expose one repository root. Repeat only when one task genuinely needs multiple roots. |
| `-j, --max-parallel-tools <N>` | Override adaptive bounded tool concurrency. |
| `--public-url https://…` | Use a stable reverse-proxy URL instead of a temporary Quick Tunnel. |
| `--read-only` | Remove model-facing file mutation capabilities. |
| `--no-exec` | Disable command execution. |
| `--no-open` | Do not open Setup Hub automatically. |
| `--no-monitor` | Disable the live terminal dashboard. |
| `--allow-sleep` | Do not hold the platform idle-sleep assertion while serving. |
| `--allow-risky-exec` | Process-wide pre-authorization for repository-aware execution. Prefer exact session approvals when possible. |

Advanced trust-boundary flags exist for exceptional deployments. They are intentionally not the normal setup path; see [Security](../security/).

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

## Recommended MCP workflow

Before a substantial edit:

```text
workspace_info
  ↓
scope_status + design_status + project_context
  ↓
language_quality_status (for source/quality work)
  ↓
software_context(scopes=...)
  ↓
precise source navigation / edits
```

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
| `software_context` | Budget-aware task context with optional Product Scope narrowing and graph context. |

### Source navigation and bounded I/O

| Tool | Use it for |
| --- | --- |
| `file_outline` / `find_symbol` / `symbol_context` | Tree-sitter definition navigation with explicit syntax precision. |
| `search_code` / `search_many` | Exact repository discovery; prefer the bulk form when queries are known together. |
| `read_file` / `read_files` | Bounded UTF-8 reads with SHA-256 edit preconditions. |
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
| `semantic_provider_status` / `semantic_provider_refresh` | First-party LSP provider availability and bounded semantic refresh. |
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

- **CommandAccess** authorizes a bare executable for one Workspace.
- **RiskyExecution** authorizes one exact repository-aware operation fingerprint for the current session.
- **RuntimeExecutor** covers one exact advanced verification executor operation.
- **Destructive delete** is one-shot and separate from reusable session grants.

Git mutation remains deliberately narrow. Only explicit pathspec `git add`, message-only `git commit -m ...`, and non-force/non-delete `git push <remote> <refspec>` shapes can enter exact approval. Approval does not forward SSH agents, tokens, credential helpers, or arbitrary Git configuration.

## Diagnostics

The public/local health surface keeps connection state layered rather than flattening everything to “connected/disconnected”. Inspect the runtime TUI first, then `/healthz` when debugging HTTP/tunnel/OAuth state.

For repository correctness use:

```bash
wcode --workspace "$PWD" intelligence --check --json
```

For implementation quality use the repository-native checks returned by `project_context` / `language_quality_status`, then finish with `verify_project`.

## Related guides

- [Getting Started](../getting-started/)
- [Software Intelligence](../software-intelligence/)
- [Code Agent Integrations](../code-agent-integrations/)
- [Security](../security/)
- [Language Quality](../language-quality/)
- [Maintainability Review](../maintainability-review/)
