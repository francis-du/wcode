---
layout: docs
title: Development Notes
description: wcode implementation constraints, workflow, and release guidance
lang: en
alternate: /zh/docs/development/
permalink: /docs/development/
---

# wcode Development Notes

This page is for maintainers of wcode itself. User-facing setup starts at [Getting Started](../getting-started/); product behavior and precision rules live in [Software Intelligence](../software-intelligence/) and [Security](../security/).

## Module map

wcode keeps product responsibility explicit rather than growing one generic runtime/service layer.

- `src/main.rs` — CLI and startup composition: argument parsing, runtime lifecycle, graceful shutdown/restart, and Product Scope module wiring. Managed tunnel lifecycle is delegated to `src/runtime/tunnel.rs`.
- `src/scopes/mod.rs` — the canonical Product Scope registry, aliases, source roots, and Tool-to-scope mappings consumed by context, semantics, Convention, Design State, MCP metadata, and operator views.
- `src/runtime/` — Harness/runtime orchestration. `harness.rs` owns the public core; `harness_agent_context.rs` builds edit-ready Agent Context; `harness_profile.rs` caches project/manifests/guidance; `harness_repo_map.rs` ranks/caches repo-map structure; `harness_graph.rs`, `harness_project.rs`, `harness_review.rs`, `harness_scope.rs`, `harness_text.rs`, and `harness_verification.rs` split graph/context, Project Observatory assembly, Git review, Product Scope audit, text/project discovery, and verification. `harness_tests.rs` contains focused Harness tests. `control.rs`, `power.rs`, and `tunnel.rs` own authenticated stop/restart, idle-sleep inhibition, and managed public tunnels.
- `src/integrations/` — model/client integration boundary. `mcp.rs` owns shared protocol routing; `mcp_dispatch.rs` executes Tool calls; `mcp_tools.rs` owns the deterministic Tool catalog; `mcp_stdio.rs`, `mcp_tasks.rs`, and `mcp_catalog.rs` cover stdio, durable Tasks, Prompts, and Resources; `auth.rs` owns OAuth/PKCE/DCR; `task_store.rs` persists MCP Task state; `agent_plugin.rs` exports the non-executable Skill/Plugin package.
- `src/workspace/` — secure local coding boundary: bounded file/search/edit/move/delete primitives, root/registry isolation, command policy, local authorization, media inspection, Convention checks, and dependency-aware path scheduling.
- `src/design/` — structured Desired Software State loading, stable-ID/reference validation, sparse initialization, and implementation/verification mappings.
- `src/graph/` — lazy Tree-sitter code index plus provider-neutral Software Graph contracts and persisted provider/composite revisions.
- `src/semantics/` — persistent candidate/confirmed/retired Semantic Registry and first-party LSP provider runtime.
- `src/intelligence/` — traceability, scoped/task context, drift/impact/risk, and Project Observatory projections. `observatory_architecture.rs` owns the architecture-first Design-vs-Actual component projection.
- `src/verification/` — Verification Plans, blind reviewer/readiness state, Language Quality providers, and Property/Mutation/Fuzz/Runtime executors.
- `src/evidence/` — provenance-bearing Evidence contracts and bounded persistent storage.
- `src/reconciliation/` — durable desired-to-actual plans plus dependency-aware execution/retry state.
- `src/ui/` — operator experience. `monitor.rs` owns the main Ratatui event/render loop, `monitor_state.rs` owns task/connection/traffic and Agent Context metrics, `monitor_detail.rs` owns detailed overlays, and `intelligence_web.rs` serves the protected architecture-first Project Observatory.

When moving a responsibility, update Product Scope roots and Design State implementation references in the same change. A physical refactor is incomplete when the architecture contract still points at the old owner.

## Runtime invariants

A Tool call has one real lifecycle:

```text
request → queued → semaphore acquired → running → completed | failed
```

The global semaphore remains the only concurrency gate. Composite operations must not retain a parent permit while children wait for permits. `parallel_tools`, `review_changes`, `verify_project`, and other internally fanned-out operations therefore schedule real child work through the same global accounting path.

`parallel_tools` is resource-aware, not a generic race-everything helper. Its scheduler models `reads`, `writes`, `creates`, `moves_from`, `moves_to`, and `deletes`. Independent resources may fan out; overlapping resources are dependency ordered. Same-file `apply_edits` may coalesce only when callers pin the same observed SHA and edits are non-overlapping and unambiguous. Invalid overlap is rejected before execution.

Runtime collections are bounded. Fan-out count, individual/aggregate result bytes, model-facing read/write size, source scans, retained complete ASTs, Git review files/findings, traffic history, persistent state history, and per-path lock maps all have explicit limits. New caches must invalidate from real source/profile/provider revisions rather than request frequency.

The coding-context hot path optimizes both model cost and wall time:

- `agent_context` is the default coding entry point and uses an explicit or adaptive bounded approximate token budget;
- simple direct-target tasks stay small while ambiguous/cross-module work can grow within a fixed bound;
- scope-aware cold repo-map construction avoids a full-repository graph when direct target ownership is known;
- repo-map structure is revision-aware cached, while query ranking is recomputed per task;
- multiple symbol queries traverse/index a source root once rather than rescanning for every token;
- bounded Hot Source keeps the strongest direct body when useful, while additional bodies remain progressively disclosed;
- fresh semantic/runtime/deterministic graph evidence may strengthen caller/callee/dependency ranking; stale semantic revisions automatically fall back to syntax;
- edit targets retain SHA/writeability and direct working-tree state;
- readiness and deterministic `next_actions` tell the agent whether to edit, fetch more source/semantics, or verify;
- timing/cache/savings telemetry stays in Tool Result `_meta` instead of consuming model-visible context.

The monitor reflects real work. Queued/running/completed state, bytes, peak concurrency, Agent Context calls, average model-visible tokens, repo-map cache-hit rate, and saved context are derived from actual request execution. Terminal raw mode, mouse capture, cursor state, and the primary screen must be restored through the existing RAII boundary; Ctrl-C uses the same graceful shutdown path. The monitor does not start for non-TTY stdout or `--no-monitor`.

Managed public tunnels are owned runtime children, separate from the local HTTP server. `--tunnel-provider auto` starts Cloudflare, `localhost.run`, Pinggy, and Tailscale Funnel concurrently in the background; the dashboard never waits for them. A tunnel counts as live only after URL discovery and instance-matched `/healthz` succeeds — a URL string alone is not readiness. Every live tunnel is kept; the first to land owns the primary endpoint. A dead tunnel respawns alone after an exponential backoff (15s..300s) and never restarts the process; if the primary dies the next live tunnel is promoted. Normal shutdown (Ctrl-C or SIGTERM) aborts owned tasks and kills/waits for every tunnel child. Never implement recovery by killing or replacing unrelated operator processes.

HTTP MCP and `mcp-stdio` call the same protocol/Harness/Workspace implementation. Supported protocol revisions remain explicit. Modern Tool/Task/Resource behavior is enabled only when the request revision/capability actually permits it; legacy or capability-unknown clients fail closed where required. MCP Tasks are durable coordination records, not promises that process execution survives runtime replacement.

Media remains metadata-first. Binary image/audio content is emitted only when the current MCP request advertises the matching `run.francis.wcode/media-content` extension; unknown capability remains metadata-only/fail-closed and video remains metadata-only.

## Software Intelligence invariants

Software Intelligence is exposed through MCP, local `wcode intelligence` / `wcode verification` CLI views, the live TUI, and the protected Project Observatory.

Project Observatory is **architecture-first**. It first shows the whole component architecture, declared Design dependencies, current code-derived Actual relationships, observed drift, Evidence Coverage, and Implementation Coverage. Component Inspector and Requirement drill-down follow. Requirement detail preserves:

```text
Desired State → Actual State → Change → Proof → Convergence
```

Strong positive semantic/runtime/deterministic evidence may identify blocking observed drift. A relationship not observed in weak syntax evidence is not proof of absence and stays advisory. Browser code must not reconstruct business ownership independently of Harness/Intelligence contracts, and a generic global node-ball graph must not become the primary project view.

`src/scopes/mod.rs` is the only canonical Product Scope registry. Source ownership, semantic Product Scope aliases, `agent_context`/`software_context` narrowing, `scope_status`, `workspace_info`/`project_context`, Convention checks, Tool `_meta.dev.wcode/productScopes`, the `wcode://runtime/product-scopes` Resource, and operator views must derive from that registry. Unknown semantic scope strings remain valid freeform business/domain scopes.

Tree-sitter remains `provider=tree-sitter`, `precision=syntax`. It must never imply compiler-level overload/type/macro/dynamic-dispatch certainty. First-party LSP facts become `precision=semantic` only after a real installed provider returns valid data. First-party nodes retain source SHA; missing/mismatched SHA makes a provider revision stale. Stale semantic facts are excluded from graph overlays, Agent Context ranking, impact, reconciliation, and graph-aware context until refreshed. External SCIP/compiler/runtime providers retain their own provenance.

Design State under `.wcode/project.yaml` and `.wcode/design/` is the Desired State source. Initialization remains sparse: do not create empty collection files merely to satisfy shape. IDs and cross-references are stable; source mappings are repository-relative and must not encode unstable line numbers.

Verification Plans are risk-adaptive orchestration state rather than proof. Deterministic checks, independent reviewers, Property/Mutation/Fuzz/Runtime executors, and HumanApproval are separate producers. Reviewer disagreement is retained as disagreement rather than majority-voted away. Required stage evidence aggregates per producer with fail-closed precedence, and stale workspace revision blocks readiness. `verify_project` records deterministic Evidence only after a real Harness report; Acceptance Evidence is emitted only for verification references actually exercised by that report.

Persistent intelligence state lives outside the repository in a bounded per-user/per-Workspace state directory. Evidence, Verification, Semantic revisions, provider/composite Graph snapshots, Reconciliation plans/execution, and MCP Task snapshots have distinct persistence contracts. Repository `.wcode/` remains Desired State, not a dump of runtime caches.

## Security invariants

Changes must preserve all of the following boundaries:

- canonical Workspace root isolation and root-identity rechecks;
- rejection of absolute paths, parent traversal, protected paths, symlink components, and unsafe hard-link writes;
- SHA-256 edit preconditions, per-file locking, post-lock path re-resolution, bounded atomic writes, and create-without-overwrite semantics;
- `delete_path` as the only model-facing deletion primitive, limited to one regular file or empty directory after exact one-shot human approval; recursive/root/protected/symlink/hard-link deletion stays permanently blocked;
- no-shell execution: a model supplies a bare executable plus argument array, never shell syntax or a path-bearing interpreter;
- command-specific policy for known development CLIs rather than “authorize program means authorize every subcommand”;
- exact `RiskyExecution` fingerprints for repository scripts/builds, bounded remote writes, Docker/Kubernetes external-data reads, and other operations that cross the normal read/check boundary;
- permanently blocked credential/admin/destructive surfaces including force/delete/mirror Git pushes, `git reset`/`restore` mutation paths, `gh auth`/`api`/secret/variable/extension bypasses, Kubernetes cluster mutation, Terraform apply/destroy/import/state-secret surfaces, shell interpreters, and filesystem/config redirection outside the selected Workspace;
- Git mutation only through explicit-path `git add`, message-only `git commit`, and explicit remote+ref non-force `git push`. An approved SSH push may use the current `SSH_AUTH_SOCK` only through wcode's fixed non-interactive SSH command. Token-like environment variables, Credential Helper, AskPass, arbitrary Git config, proxy helpers, HTTP extra headers, hooks, and external diff helpers remain stripped/disabled;
- `gh` remote mutations remain non-interactive and option-allowlisted; new/unknown write flags fail closed rather than silently inheriting trust;
- URL arguments never embed credentials, and protected credential/environment files remain outside model-facing filesystem/index surfaces;
- repository-aware LSP and advanced verification executors require explicit trust unless process-wide `--allow-risky-exec` intentionally pre-authorizes them;
- bounded output, timeout termination, sensitive-environment scrubbing, and disabled interactive prompts;
- HTTPS/loopback public URL policy, OAuth PKCE/resource binding, bounded DCR metadata, exact redirect policy, Origin validation, and refresh-token rotation.

`--allow-risky-exec`, `--allow-destructive-writes`, overlapping/broad Workspace flags, and equivalent switches are trust-boundary expansions. Treat them as explicit operator decisions, never default convenience settings. Exact session authorization is preferable when the operation can be narrowly described. None of these mechanisms is an OS sandbox.

Known development CLI policy should be widened by **adding an inspected command family**, not by falling back to generic execution. Local read/check operations may be direct; repository code execution, remote mutation, and external daemon/cluster reads require exact authorization; destructive infrastructure/credential-bypass operations remain blocked. Security tests must cover both an allowed representative shape and blocked escape/mutation shapes for every new family.

## Cross-platform dependency handling

Managed tunnel dependencies are provider-specific. Cloudflare uses `cloudflared`; explicit Cloudflare selection may use the existing Homebrew/winget installation flow unless `--no-install` is set. `localhost.run` and Pinggy use the system `ssh` client and never trigger package installation. Tailscale Funnel uses the `tailscale` CLI (logged in, Funnel enabled on the tailnet) and exposes the machine's stable `ts.net` URL; only one wcode instance per machine can hold the Funnel listener. In `auto` mode, missing `cloudflared` is skipped so a zero-install SSH provider can be attempted immediately.

Known development CLIs are not automatically installed. Harness/quality/provider discovery reports real availability. Optional acceleration must keep a correct fallback when one exists. Rust full verification, for example, prefers `cargo-nextest` only when the repository declares nextest configuration and the executable is installed; otherwise it uses `cargo test`. The strict Harness lane admits only fixed `cargo nextest run [--locked]` shapes, not arbitrary nextest arguments.

Language servers and stage executors similarly expose registered-vs-available state. Never turn “known candidate” into “installed/runnable” merely because wcode recognizes its ecosystem.

## Required verification

Before release, the repository must pass the full gate:

```bash
git diff --check
cargo check --locked
cargo fmt --check
cargo test --locked
cargo clippy --locked -- -D warnings
cargo build --release --locked
```

`verify_project(level="full")` is the preferred Harness-controlled path for the same release-quality checks. Also validate current Design/Traceability and documentation parity. A successful build alone does not prove release readiness when Design or bilingual docs are stale.

Documentation changes must preserve reciprocal `alternate` routes, same top-level bilingual section structure, the same critical technical facts, local links, installer commands, and the single hosted `/docs/` + `/zh/docs/` manual model. Host-specific integration commands have one canonical technical guide in [Code Agent Integrations](../code-agent-integrations/) rather than being duplicated into README and website copy.

When a command/tooling optimization changes the recommended agent workflow, update Getting Started, the docs index, Agentic Engineering, Reference/Security where relevant, and the automated bilingual contract in the same change. Avoid the failure mode where both languages remain synchronized but both keep an obsolete workflow.

## Release artifacts

`.github/workflows/release.yml` validates the repository before tag artifacts are published. Release packages target:

- Linux x86_64;
- macOS Apple Silicon;
- macOS Intel;
- macOS Universal;
- Windows x86_64.

Packaged binaries must report the expected `wcode --version`. Archives and checksums are release artifacts. Historical release notes describe their tagged version and should not be mass-rewritten to reflect later product semantics.

Version consistency across Cargo/package metadata and generated Agent Plugin/marketplace manifests is a release gate. Keep history documents on their historical version while current package/plugin manifests match the release being prepared. Do not commit, tag, push, or publish merely because local checks pass; those are explicit release actions.
