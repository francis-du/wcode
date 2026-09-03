---
layout: docs
title: Product Scopes
description: Canonical wcode product capability and source ownership map
lang: en
alternate: /zh/docs/product-scopes/
permalink: /docs/product-scopes/
---

# wcode Product Scopes

wcode organizes its control-plane capabilities by product behavior rather than generic backend layers. The same scope model is used for source architecture, semantic filtering, software-context retrieval, MCP tool metadata, project context, convention governance, and agent guidance.

## Canonical scopes

| Scope | Product responsibility | Primary source roots |
| --- | --- | --- |
| `runtime` | Harness, scheduling/runtime control, power/process coordination | `src/main.rs`, `src/runtime/`, `src/scopes/` |
| `integrations` | MCP, OAuth, agent plugins, connector-facing tasks/resources/prompts | `src/integrations/` |
| `workspace` | Secure filesystem primitives, authorization, scheduler, conventions | `src/workspace/` |
| `design` | Structured Desired Software State and validation | `src/design/` |
| `graph` | Syntax index, composite Software Graph, graph persistence/providers | `src/graph/` |
| `semantics` | Persistent semantic registry and first-party LSP adapters | `src/semantics/` |
| `traceability` | Requirement traceability, scoped context, Project Observatory architecture projection, drift and impact | `src/intelligence/` traceability/observatory sources excluding `risk.rs` |
| `risk` | Risk-adaptive policy and verification depth | `src/intelligence/risk.rs` |
| `verification` | Deterministic and staged verification, blind review mesh | `src/verification/` |
| `evidence` | Provenance-bearing persistent evidence | `src/evidence/` |
| `reconciliation` | Durable convergence planning and dependency-aware execution | `src/reconciliation/` |
| `experience` | TUI/WebUI operator experience | `src/ui/` |

`main.rs` is startup composition and `src/scopes/mod.rs` is the canonical registry. Product scopes describe wcode itself; semantic facts may additionally carry freeform business scopes.

## Runtime behavior

`software_context` accepts optional `scopes`. Recognized Product Scope aliases are canonicalized and narrow source/symbol navigation to the corresponding source roots. The canonical scopes are returned in the response so an agent can see which product boundary shaped its context.

`semantic_query` also accepts optional `scopes`. Scoped facts must overlap a requested scope; unscoped facts remain global. Unknown scope strings are preserved as freeform business scopes rather than rejected.

The Convention Engine classifies source files by architecture domain and Product Scope. It reports unclassified root Rust modules, unmapped Product Scope files, language naming findings, flat domain growth, and oversized modules without silently rewriting the repository.

## MCP and agent discovery

Every MCP tool advertises `dev.wcode/productScopes` in Tool `_meta`. Agents that ignore custom Tool metadata can discover the same product model through the MCP resource `wcode://runtime/product-scopes`. `scope_status` applies that model to the selected repository and reports mapped/unmapped supported source files, so the scope model participates in architecture governance rather than remaining discovery metadata. The same live audit is visible in the TUI Intelligence overlay and the protected `/intelligence/status` payload; `/intelligence/scopes` exposes the focused workspace audit for operator tooling.

Recommended agent flow:

1. `agent_context(goal, scopes=...)` — use the compact edit-ready pack as the normal coding entry point.
2. `scope_status` — only when a scope audit is useful; inspect per-scope source counts and bounded `unmapped_files` before adding production structure.
3. `design_status`, `project_context`, and `traceability_status` — load the full desired-state/repository contract only when the task needs it.
4. Choose the Product Scope(s) relevant to the requested behavior.
5. `software_context(query, scopes=...)` — retrieve deeper bounded context only when the compact pack is insufficient.
6. Navigate with `find_symbol`, `symbol_context`, graph/semantic/traceability tools as needed.
7. Mutate only through Workspace primitives and the dependency-aware Scheduler.
8. Run `review_changes`, risk/impact checks, verification, evidence and reconciliation gates according to the change.

## Scheduler boundary

`parallel_tools` is not a read-only fan-out helper. It uses the reusable Scheduler resource model (`reads`, `writes`, `creates`, `moves_from`, `moves_to`, `deletes`). Independent work fans out; overlapping resources are dependency ordered. Same-file `apply_edits` operations may coalesce only when they use the same observed SHA and non-overlapping, unambiguous edits.

## Design State contract

Design State maps these scopes to real components, implementation symbols and acceptance tests. Do not add future-only components without a real implementation mapping. Traceability must remain resolvable after refactors; physical file moves require corresponding Design State path updates.

## Scope design rules

- Prefer product responsibility over generic technical-layer names.
- Keep one canonical registry; do not recreate scope alias tables in MCP, semantics, UI or docs.
- A new first-class wcode capability should map to a Product Scope, source root, MCP tool metadata (when exposed), and Design State component/acceptance chain.
- Business/domain semantics remain freeform and must not be conflated with wcode Product Scopes.
- Scope filters must change retrieval or execution behavior where appropriate; a decorative scope label is not sufficient.
