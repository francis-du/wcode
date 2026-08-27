---
layout: docs
title: Development Notes
description: wcode implementation constraints, workflow, and release guidance
lang: en
alternate: /zh/docs/development/
permalink: /docs/development/
---

# wcode Development Notes

This document contains implementation constraints and release details. User-facing architecture and usage start at the [wcode website](../../).

## Module map

- `src/main.rs` — CLI and startup composition: runtime lifecycle, tunnel supervision, shutdown/restart, plus the explicit `#[path]` wiring for the Product Scope modules below.
- `src/scopes/mod.rs` — the canonical Product Scope registry: twelve capability scopes, aliases, source roots, and MCP tool-to-scope mappings used by context retrieval, semantics, conventions, Design State, and agent discovery.
- `src/runtime/` — process and Harness runtime: `harness.rs` owns the core Harness; `harness_graph.rs`, `harness_project.rs`, `harness_review.rs`, `harness_scope.rs`, `harness_text.rs`, and `harness_verification.rs` split graph/context, Project Observatory assembly, Git review, Product Scope repository auditing, text/project discovery, and verification helpers; `harness_tests.rs` holds focused Harness tests; `control.rs` handles authenticated stop/restart control and `power.rs` platform idle-sleep inhibition.
- `src/integrations/` — model/client integration boundary: `mcp.rs` owns the shared protocol/router and cross-checks the Tool catalog; `mcp_dispatch.rs` executes Tool calls and `mcp_tools.rs` is the compiled extracted Tool catalog/helper module used by discovery and dispatch; `mcp_stdio.rs`, `mcp_tasks.rs`, and `mcp_catalog.rs` cover local stdio, MCP durable Tasks, and Prompts/Resources; `auth.rs` owns OAuth/PKCE/DCR; `task_store.rs` persists MCP Task state; `agent_plugin.rs` exports the non-executable Agent Skill/Plugin package.
- `src/workspace/` — local coding boundary: `mod.rs` owns bounded file/search/edit/move/delete primitives; `media.rs` performs bounded magic-byte media inspection; `roots.rs` and `registry.rs` enforce root/workspace isolation; `fs_safety.rs` contains filesystem safety helpers; `command_policy.rs` owns no-shell command policy; `authorization.rs` handles local human approval; `conventions.rs` reports repository conventions/architecture; `scheduler.rs` builds path-resource dependencies and safe same-file/same-SHA edit coalescing.
- `src/design/mod.rs` — structured `.wcode` Desired Software State loading, validation, stable identifiers, and implementation/test mappings. `ToolHarness::design_init` keeps this state sparse: initialization creates Project/Product only and never materializes empty collection placeholders.
- `src/graph/` — `code_index.rs` provides the lazy 22-language Tree-sitter index; `mod.rs` defines provider-neutral Software Graph contracts; `graph_provider_store.rs` and `graph_store.rs` persist external-provider and composite graph revisions.
- `src/semantics/` — `mod.rs` owns the persistent candidate/confirmed/retired Semantic Registry, `provider.rs` runs the first-party LSP semantic-provider layer, and `store.rs` persists semantic revisions.
- `src/intelligence/` — `mod.rs` orchestrates Software Intelligence; `context.rs` builds scoped task-oriented context; `analysis.rs` handles drift/impact helpers; `observatory.rs` derives the requirement-first Project Observatory model from current Design State, code graph, Git review, risk, and graph history; `risk.rs` applies risk-adaptive policy; `types.rs` carries shared result contracts; `tests.rs` contains focused intelligence tests.
- `src/verification/` — `mod.rs` owns Verification Plans, blind reviewer/readiness state, and verification orchestration; `quality_provider.rs` owns the shared Language Quality Matrix/status/check-only execution contract, while `quality_catalog.rs` and `quality_catalog_extended.rs` map repository-declared/native quality providers across the canonical 22-language surface; `stage_executor.rs` runs cross-language Property/Mutation/Fuzz/Runtime-Canary executors; `store.rs` persists Verification Plan/Job snapshots.
- `src/evidence/` — `mod.rs` defines provenance-bearing Evidence contracts and `store.rs` provides bounded persistent Evidence storage.
- `src/reconciliation/` — `mod.rs` owns desired-to-actual convergence plans and dependency-aware execution/retry semantics; `store.rs` and `execution_store.rs` persist plan and execution state.
- `src/ui/` — `monitor.rs` owns Ratatui monitor state, task/connection lifecycle, dashboard input loop and primary layout; `monitor_detail.rs` owns Setup, workspace activity, Intelligence/help overlays, footer and shared presentation metrics; `intelligence_web.rs` embeds the token-protected requirement-first Project Observatory. The observatory consumes `harness_project.rs` / `intelligence/observatory.rs` and is organized as Desired State → Actual State → Change → Proof → Convergence. Proof is revision-exact, each Requirement has an explicit convergence state/blockers, and the lower views retain architecture alignment, repository/language/Product-Scope code statistics, mapped Git changes, risk, and meaningful structural revisions. The low-level Software Graph remains an intelligence primitive and API, not the primary operator visualization. Local destructive-authorization approval/denial remains inside the TUI boundary.

## Runtime invariants

A tool call follows one real lifecycle:

```text
request -> queued -> semaphore acquired -> running -> completed | failed
```

The monitor must never simulate work. Queue, active, completion, failure, request bytes, response bytes, and peak concurrency are updated by the same child-task path that acquires the global semaphore. The global semaphore remains the only concurrency gate.

`--max-parallel-tools` is a cap rather than a target. The default is adaptive: twelve times the available logical CPU count, clamped to 96–192; an explicit CLI value may raise it to the Harness maximum of 256. Ordinary tool calls acquire one permit. Composite tools (`parallel_tools`, `review_changes`, `verify_project`, and change-intelligence tools that internally invoke `review_changes`) must not hold a parent permit while waiting for children; every child acquires its own permit. This prevents a one-slot configuration from deadlocking and ensures TUI `Slots`/`Peak` values match real work. `parallel_tools` accepts bounded independent read/discovery operations and workspace-write children. Before execution, `scheduler.rs` builds a path-resource model and dependency layers: overlapping read/write paths, parent/child create/move/delete dependencies, and other conflicting operations are ordered; same-file `apply_edits` children may be coalesced only when they use the same SHA-256 and their edits do not conflict. Invalid overlaps are rejected before execution. The fan-out remains bounded to 128 children, 512 KiB per child result, and 8 MiB per aggregate response. Verification uses phase barriers so independent cheap checks can overlap without launching compiler-heavy test, Clippy, and build work together.

The positive coding harness must remain bounded and deterministic: project guidance is limited by file, line, and total-character budgets; cache invalidation follows manifest/guidance metadata; Git change review uses five read-only probes and returns classified metadata and risk findings rather than diff bodies; inferred checks use only existing allowlisted command execution; and diagnostic output is tail-bounded before it returns to the model.

Language support is also an explicit capability vector, never a boolean. Syntax and semantic provider status remain separate from repository quality tooling; `language_quality_status` adds format/lint/type/static/test/security discovery using repository declarations and native project conventions, and advanced Property/Mutation/Fuzz/Runtime coverage remains in the Verification executor registry. A known tool is not repository policy merely because it exists in wcode's catalog. `language_quality_run` must remain check-only, preserve the trusted runtime authorization boundary, never invoke formatter/fixer write modes, and convert real results into revision-exact Evidence. See `language-quality.md`.

Agentic engineering follows the same separation of concerns: always-on instructions are a short map, detailed knowledge is retrieved progressively, isolated subagents/worktrees are useful for independent reasoning, but mandatory behavior belongs in deterministic Workspace/Harness/Verification/Evidence mechanisms and model consensus cannot clear a deterministic gate. See `agentic-engineering.md`.

Maintainability review is an approval gate, not a cosmetic pass. `review_changes` may deterministically flag a source file that crosses from below 1,000 lines to above 1,000 in the current change, concentrated net growth in one source file, and large changes spanning three or more Product Scopes. Those signals feed the normal Risk Engine. They do not pretend to prove spaghetti or abstraction quality. Medium-and-higher risk Verification Plans include a dedicated blind `maintainability` reviewer whose job carries the structural rubric: first look for a behavior-preserving simplification that deletes branches/helpers/layers; reject scattered special cases, boundary leaks, unnecessary wrappers/casts/optionality, and canonical-helper duplication; question file growth; prefer independent work to fan out and related state updates to be atomic when that makes the model simpler. Correctness evidence cannot substitute for this reviewer. The 1,000-line rule is a change-review smell, while Convention's 2,000 production-line threshold remains the repository-level oversized-module signal. See `maintainability-review.md`.

The syntax index must remain lazy, bounded, and honest about precision. Directory symbol searches may prefilter source text before parsing, complete Tree-sitter trees stay capped at 128 files, and successful writes invalidate the affected symbol and AST records. Every model-facing result carries `provider=tree-sitter` and `precision=syntax`; it must not imply compiler-level type resolution, overload selection, macro expansion, or dynamic-dispatch accuracy. Adding a grammar requires a real source fixture that proves at least one definition is extracted, plus extension or special-filename routing coverage where applicable. Symbol signatures and returned bodies continue through the workspace redaction boundary.

Runtime collections are bounded as well: MCP batches accept at most 128 items, `parallel_tools` accepts at most 128 scheduled read/discovery or workspace-write children, symbol searches scan at most 50,000 source files and retain at most 128 complete ASTs, change review accepts at most 500 files and 64 findings, monitor traffic history keeps at most 4,096 events, and per-file write-lock entries use weak references so inactive paths are pruned instead of accumulating for the process lifetime.

The dashboard:

- uses Crossterm raw mode and the alternate screen;
- renders only through `Terminal::draw`;
- redraws every 150 ms while work is queued/running and every 500 ms while idle;
- renders a dark, rounded card hierarchy with compact metric fallback, focused Workspace cards, and keycap-style shortcuts;
- renders `Slots active / cap`, a process-lifetime `Peak`, and a utilization bar from real child-task transitions;
- labels token economics as estimates: tool-result bytes are divided by four for `CTX`, only measurable full-source bytes omitted by `file_outline`/`symbol_context` count as saved, and USD uses the configured per-million input-token rate;
- enables Crossterm mouse capture inside the terminal session; footer/help link hit regions are derived from the same responsive layout geometry, while keyboard shortcuts remain available;
- restores mouse capture, raw mode, cursor visibility, and the primary screen through an RAII guard;
- consumes Ctrl-C as a Crossterm key event and forwards shutdown to Tokio;
- does not start when stdout is not a TTY or `--no-monitor` is set;
- suppresses tracing output while active so background logs cannot corrupt the screen.

Quick Tunnel lifecycle is independent of the local HTTP server. Axum starts accepting local requests before public readiness is evaluated. Every process owns a random `instance_id`; startup does not open the Setup Hub or report readiness until the public `/healthz` response returns `ok: true` with that exact ID. This prevents DNS/TLS warm-up and another concurrently running wcode process from producing a false-positive readiness result. The main loop polls only its own cloudflared child with `try_wait`; an unexpected exit requests a complete runtime restart. A per-instance bounded task also checks the public endpoint every 25 seconds; three consecutive failures request the same restart, while one success clears the failure streak. Normal shutdown stops the health task, aborts the local server task, and kills and waits for the owned cloudflared child so no task or child process leaks into another instance.

The complete runtime is the recovery boundary. If the owned `cloudflared` process exits or the public URL fails three consecutive checks, wcode restores the terminal, stops its tasks and child process, then replaces or relaunches the same executable with the original argument vector. Unix uses `exec` after releasing owned resources; Windows launches a replacement after a short port-release delay. `wcode restart` and `wcode stop` enter that same graceful path through a bearer-authenticated local control route. Its 256-bit random token is stored in a per-user, mode-0600 runtime file and compared in constant time; the route remains authenticated because it is also reachable through the public reverse proxy. Runtime replacement never reuses a Quick Tunnel URL or claims that OAuth state survived a full restart.

While running, wcode holds the platform's idle-sleep assertion: IOKit on macOS, `systemd-inhibit` on Linux, and `SetThreadExecutionState` on Windows. Display sleep and screen locking remain enabled. `--allow-sleep` opts out, and failure to acquire an inhibitor is reported without preventing startup.

MCP stays dual-transport and dual-era while the ecosystem migrates. Streamable HTTP remains the cloud/web connector path; `mcp-stdio` is the local coding-agent path, but both call the same `handle_message` / Harness / Workspace runtime rather than duplicating protocol or security logic. The modern lane accepts MCP `2026-07-28` stateless requests with routing headers and self-describing request metadata, rejects JSON-RPC batches, exposes a bootstrap-tolerant `server/discover`, and advertises Tools, Prompts, Resources, the official `io.modelcontextprotocol/tasks` extension, plus the optional `run.francis.wcode/media-content` extension. `read_media` is metadata-first; image/audio content is emitted only when that same request advertises a matching media capability, while legacy or capability-unknown clients remain metadata-only/fail closed and video stays metadata-only. Non-bootstrap modern calls remain strict. Final 2026 protocol errors are preserved (`-32020` HeaderMismatch, `-32021` MissingRequiredClientCapability, `-32022` UnsupportedProtocolVersion). Task behavior activates only when that same request opts in through client capabilities; only `semantic_provider_refresh` and `verification_execute_stages` are task-augmented. A Task record must be durable before returning its handle, owner-scoped to an OAuth client fingerprint (or the local stdio process identity), must never store bearer tokens/raw tool arguments, and cancellation must persist before aborting the worker. Task state locking must encompass locate/read/update so polling/cancellation do not perform redundant unlocked disk reads. The compatibility lane continues to accept `2025-11-25`, `2025-06-18`, `2025-03-26`, and `2024-11-05` clients through `initialize`. If an HTTP `Origin` header is present it must match the configured public MCP origin. HTTP OAuth remains PKCE + Protected Resource Metadata + canonical `/mcp` Resource Indicator binding + DCR. DCR must validate public-client metadata (`application_type`, grants, response types, `token_endpoint_auth_method=none`, and `mcp` scope); authorization codes are always internally resource-bound, while a legacy token request may omit `resource` only by inheriting that existing binding. Client ID Metadata Documents are intentionally not fetched by default because arbitrary client-controlled metadata URLs would introduce a new outbound SSRF/DNS-rebinding trust boundary.

The product website remains dependency-free static HTML/CSS/JavaScript
(`docs/index.html`, `docs/assets/site.css`, `docs/assets/site.js`). The same
Pages workflow builds the canonical Markdown documentation under `docs/manual/` into
English `/docs/` and Simplified Chinese `/zh/docs/` HTML trees through Jekyll, then
uploads one `_site` artifact. Every bilingual page declares `lang`, `permalink`, and
a reciprocal `alternate`; each language index links only its own tree. The product
homepage is an entry surface rather than a second reference manual. All maintained
product, integration, architecture, development, and release Markdown belongs under
`docs/manual/`; root Markdown is
reserved for the project README and repository policy files. Package-local
documentation stays with the package it describes. Do not create a second
vendor command matrix in README or website copy: agent/client installation and
security guidance has one technical source in
[Code Agent Integrations](../code-agent-integrations/).

Compatibility claims must cite current primary vendor/project documentation and
classify local stdio capability, remote transport capability, plugin/Skill
discovery, OAuth behavior, and verified end-to-end behavior separately instead
of flattening them into one “supported” badge. Generated portable plugins must
remain non-executable by default: no hooks, scripts, secrets, or implicit
Workspace widening. Documentation changes must pass whitespace, local-link, and
layout checks; rendered or deployed success may be claimed only when it was
actually verified.

## Software Intelligence invariants

Software Intelligence has four supported product surfaces: MCP, local `wcode intelligence` / `wcode verification` CLI views, the live TUI Intelligence overlay, and the token-protected `/intelligence` Project Observatory. The Web surface must stay requirement-first: functional design, component responsibilities/dependencies, actual implementation symbols/files, acceptance verification, code statistics, Git change mapping, and revision history come from the same Harness/Intelligence contracts. Do not reconstruct business relationships in browser JavaScript or make a generic global node graph the primary project view. User-facing usage and precision boundaries live in [Software Intelligence](../software-intelligence/).

`src/scopes/mod.rs` is the single canonical Product Scope registry. The current scopes are `runtime`, `integrations`, `workspace`, `design`, `graph`, `semantics`, `traceability`, `risk`, `verification`, `evidence`, `reconciliation`, and `experience`. Source architecture, Product Scope aliases, `software_context.scopes`, `semantic_query.scopes`, `workspace_info.product_scopes`, convention reporting, MCP Tool `_meta.dev.wcode/productScopes`, and the `wcode://runtime/product-scopes` MCP Resource must derive from that registry rather than keeping independent lookup tables. Recognized Product Scopes may narrow source/context retrieval; unknown semantic scopes remain valid freeform business/domain scopes. See [product-scopes.md](../product-scopes/).

Design State under `.wcode/project.yaml` and `.wcode/design/` is the desired-state source. The loader accepts consolidated collection YAML files or per-item collection directories, validates IDs and cross-references, and rejects unsafe repository-relative code/test references. Design objects must not encode unstable source line numbers.

Tree-sitter Software Graph and symbol resolution remain syntax precision. Every graph fact or trace reference that comes from the syntax index must preserve provider/precision/revision provenance and must not be promoted to compiler-grade type, overload, macro, or dispatch truth. The first-party Semantic Provider layer is separate: all 22 syntax-indexed languages have LSP adapter candidates, but `precision=semantic` may be emitted only after a real installed server returns LSP Document Symbol / Call Hierarchy / Implementation data. First-party semantic nodes must retain source hashes; a mismatched/missing source makes that LSP revision stale and stale first-party facts must be excluded from graph overlays, impact/reconciliation, and graph-aware context. Provider refresh is bounded, uses stdio without a shell, scrubs sensitive environment state, and rejects workspace-external locations. Without process-wide `--allow-risky-exec`, the exact refresh operation must receive a local `RiskyExecution` session authorization and be retried; the flag remains the process-wide pre-authorization path. A persisted revision may be reused only when source hashes, provider executable metadata, and semantic index bounds still match. `software_context.graph_context` may consume only non-stale provenance-bearing semantic/runtime provider facts and must remain budget-bounded. `graph_diff` aligns stable node/edge identity before comparing revisions; repeated stable edge identities must be treated as revision multisets rather than collapsed by a single-value map. External SCIP/compiler/runtime facts still enter through the provider-neutral import contract. Change impact remains conservative across whatever provenance-bearing graph facts are actually available; it must never pretend missing dynamic-dispatch/type facts were resolved.

`drift_status`, `risk_status`, `impact_analysis`, `verification_plan`, and `reconciliation_plan` internally reuse the bounded Git change-review path and therefore require command execution. They are composite operations and must not hold a parent semaphore permit while their Git probes acquire child permits.

Verification Plans are risk-adaptive orchestration state, not proof by themselves. Blind independent reviewer jobs must remain isolated on first pass. `verification_claim` matches declared reviewer capabilities; `verification_submit` requires the same reviewer identity that claimed the job. Model-review Evidence stays below deterministic compiler/test/static evidence in trust. When reviewer verdicts disagree, the runtime persists one `EvidenceResult::Disagree` record for that plan/subject instead of hiding the disagreement behind majority voting. Required Property/Mutation/Fuzz/Runtime stages aggregate the latest Evidence independently per producer, with fail-closed precedence `Fail > Disagree > Inconclusive > Pass`; a late Pass from one producer must never hide another producer's current Fail. Automatic stage execution must run every applicable available runner unless that exact producer already has a current Pass, and one executor-infrastructure error must be reported without silently converting it into success.

`verify_project` records deterministic Evidence after the Harness report is complete. Acceptance Evidence may only be emitted when a declared verification reference was actually exercised by the report. Evidence must keep producer, design/code revision, policy, result, confidence, timestamp, and bounded artifact digest provenance.

Runtime intelligence state is deliberately bounded. Durable state is stored outside the repository in a per-user, per-workspace wcode state directory: Evidence uses immutable records; Verification Plan/Job state uses immutable snapshots; Semantic Facts use immutable lifecycle revisions; Graph Provider revisions and composite Software Graph snapshots retain bounded history/diff inputs; Reconciliation Plans/execution and MCP Task snapshots are stored independently. Store paths are keyed by the canonical workspace root, reject symlink/non-regular state entries, enforce size limits, and prune bounded history. MCP Task state is a durable request/worker coordination record, not a claim that process execution survives restart: a still-working task whose `runtime_instance_id` no longer matches must be failed on read. Risk remains derived from current Design/Git/Code. Reconciliation execution may claim/submit/retry dependency-ready tasks, but source mutation must still flow through the normal Workspace edit primitives. Property/mutation/fuzz/runtime requirements are satisfied only by real Stage Evidence, produced either by `verification_execute_stages` from available registered executors or by external `verification_stage_submit`. The executor registry must distinguish registered from actually available programs, serialize no configured arguments, execute without a shell, resolve workspace-relative programs through the canonical/symlink-safe Workspace boundary, and scrub sensitive environment/output. Without process-wide `--allow-risky-exec`, each not-yet-approved runtime-executor operation must fail closed into a local `RuntimeExecutor` session-authorization request and succeed only after operator approval and retry. Verification Plans remain blocked when their code revision is stale.

## Security invariants

Changes must preserve:

- canonical workspace-root isolation, root-identity rechecks, absolute-path and parent-traversal rejection; Unix root checks pin device/inode so same-path directory replacement is detected;
- default rejection of filesystem-root/home workspaces and parent/child overlapping workspaces;
- `delete_path` is the only model-facing deletion primitive: it can remove one regular file or one empty directory only after an exact one-shot human authorization in the local TUI or protected WebUI; regular files require their current SHA-256, and recursive deletion, workspace-root deletion, protected paths, symlink aliases, and hard-linked files remain permanently blocked;
- protected-path denial across direct reads/writes, source indexing, directory traversal, and command arguments;
- symlink-component rejection and Unix hard-link write rejection;
- SHA-256 edit preconditions, per-file locks, post-lock path re-resolution, bounded write size, and destructive-reduction gating;
- create-new semantics that cannot overwrite a raced target, plus same-directory temporary files and atomic replacement for existing files; the interactive coding path does not force a full data+directory fsync on every small edit;
- no-shell command execution with an explicit default/risky split; a small safe command set is pre-authorized, while other model-requested bare executable names fail closed into per-Workspace human authorization and become runnable only after approval. Shell interpreters and path-bearing program names remain permanently blocked, and command arguments still pass protected-path/workspace-escape validation. `verify_project` keeps its crate-internal exact-shape lane. Git mutation is narrowly authorizable only for explicit pathspec `add`, message-only `commit`, and non-force/non-delete `push` shapes with an explicit remote and refspec after exact `RiskyExecution` approval; `reset`, `restore`, force/delete/mirror pushes, broad dot staging, helper-capable options, embedded URL credentials and other mutation shapes remain blocked. Inherited `GIT_*` state is cleared, repository discovery is capped at the selected root, hooks/signing/ext/file protocols, credential helpers, AskPass/SSH/proxy helper commands, configured HTTP extra headers, and external diff helpers are disabled or constrained. Exact push authorization therefore does not imply credential forwarding; authenticated push requires a separate explicit credential boundary rather than inheriting SSH agents/tokens. Rust response files plus Cargo/Go/package-manager redirection options remain blocked;
- timeout termination, bounded streaming stdout/stderr, sensitive environment scrubbing, and disabled interactive Git prompting/helpers;
- explicit public endpoints must be HTTPS or loopback HTTP base URLs without user information, query strings, or fragments;
- bounded OAuth registration metadata, strict HTTPS/loopback redirect validation, single-use expiring authorization codes, non-expiring capacity-bounded resource-bound access tokens, and atomic expiring refresh-token rotation with lazy expiry cleanup;
- secret redaction and `.env*` scan exclusion.

`--allow-risky-exec`, `--allow-destructive-writes`, `--allow-overlapping-workspaces`, and `--allow-broad-workspace` are trust-boundary expansions. Tests and documentation must treat them as explicit operator decisions, not defaults. `--allow-risky-exec` pre-authorizes the process broadly; when it is absent, exact `RiskyExecution` / `RuntimeExecutor` fingerprints can instead be approved for the current local session through the TUI or protected WebUI. Model-requested command names have a separate per-Workspace `CommandAccess` approval path. Destructive delete remains a separate one-shot grant. Neither path is an OS filesystem sandbox.

Do not weaken these constraints for UI, performance, or harness convenience.

## Cross-platform dependency handling

`ensure_cloudflared()` first checks `cloudflared --version`.

- macOS: Homebrew is detected and `brew install cloudflared` may run.
- Windows: winget is detected and the exact `Cloudflare.cloudflared` package may be installed.
- Linux: apt/dnf/yum/pacman are detected only to produce a useful platform hint. Automatic distro installation is deliberately avoided because repository setup differs by distribution.
- `--no-install` disables automatic installation everywhere.

Installer processes are started directly with argument arrays, never through a shell.

## Required verification

Run all checks before release:

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked -- -D warnings
cargo build --release --locked
git diff
git status
```

Cargo/Go/package-manager metadata and quality commands can follow repository-controlled members, path dependencies, symlinks, configuration, build scripts, procedural macros, or tests. The model-facing default lane therefore keeps only constrained Git/ripgrep inspection and exact low-risk Cargo shapes pre-authorized. A model may request another bare executable name, but the first attempt creates a per-Workspace `CommandAccess` request and must be explicitly approved before retry. Repository-aware argument shapes can still require a separate exact `RiskyExecution` session approval, or the operator can pre-authorize that broader trust boundary with `--allow-risky-exec`; shell interpreters, redirection/escape arguments, and otherwise-invalid shapes remain blocked. `verify_project` is the other narrow exception: it can temporarily enable execution only after `validate_verification_command_shape` accepts an exact Harness-inferred command. These lanes are not OS sandboxes—especially `cargo check`, which may run project-controlled code—so never broaden either allowlist into arbitrary arguments.

Tests cover monitor lifecycle, current/peak slot accounting, token/savings accumulation and TUI rendering, mouse link hit-testing, public-endpoint/tunnel state propagation, OAuth redirect policy/registration bounds/code expiry/non-expiring access-token binding/refresh rotation, human authorization approval/denial and one-shot destructive grants, scheduler dependency ordering and safe same-SHA edit coalescing, cross-language convention reporting, fan-out order and one-slot deadlock resistance, phased verification, bounded Git review parsing and probes, syntax-index routing through MCP, real fixtures for every embedded grammar family, qualified-name search, text-prefilter misses, AST cache reuse and write invalidation, independent workspace accounting, failures, request/response bytes, small layouts, links, overlapping-root rejection, same-path Unix root replacement, protected paths, destructive-write gating, one-shot file/empty-directory deletion, symlink/hard-link aliases, safe/risky command policy, stale writes, write-lock pruning, command concurrency, and secret redaction.

## Release artifacts

`.github/workflows/release.yml` runs formatting, locked dependency checking, and Clippy on Linux, then runs the test suite on Linux, macOS, and Windows for ordinary pushes and pull requests. CLI binaries and archives are built only for `v*` release tags, after those CI gates pass. Release artifacts are produced for:

- Linux x86_64
- macOS Apple Silicon
- macOS Intel
- macOS Universal (Apple Silicon + Intel)
- Windows x86_64

The release profile preserves every runtime feature while minimizing the binary through size optimization, fat LTO, one codegen unit, abort-on-panic release code, disabled incremental compilation, symbol stripping, and narrowly selected Tokio/Axum features. Every packaged binary must pass `wcode --version`; the workflow records exact binary/archive byte counts and uses maximum archive compression. A `v*` tag creates archives and checksums through GitHub Actions.
