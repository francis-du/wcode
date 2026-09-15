---
layout: docs
title: Development Notes
description: wcode implementation constraints, workflow, and release guidance
lang: en
alternate: /zh/docs/development/
permalink: /docs/development/
---

# wcode Development Notes

This page is for maintainers of wcode itself. User-facing setup starts at [Getting Started](../getting-started/); repository-intelligence behavior and precision rules live in [Repository Intelligence & Engineering State](../software-intelligence/) and security boundaries live in [Security](../security/).

## Module map

wcode keeps product responsibility explicit rather than growing one generic runtime/service layer.

- `src/main.rs` — deliberately thin binary launcher; product startup lives under `src/app/`.
- `src/app/` — CLI and startup composition. `mod.rs` wires runtime lifecycle and graceful shutdown, `commands.rs` owns the stable command surface, `setup.rs` and `update.rs` own installation lifecycle, and `tunnel_lifecycle.rs` keeps reconnect/failover policy separate from provider processes.
- `src/scopes/mod.rs` — the canonical Product Scope registry, aliases, source roots, and Tool-to-scope mappings consumed by context, semantics, Convention, Design State, MCP metadata, and operator views.
- `src/runtime/` — Harness/runtime orchestration. `src/runtime/harness/` owns the public Harness core and its Agent Context, profile/cache, graph, review, quality, and verification modules. Repository retrieval keeps policy separate from mechanism: `retrieval.rs` owns bounded task-intent routing/priors, while `repo_map.rs` owns graph construction, provider overlays, ranking execution and provenance. `src/runtime/semantic.rs` owns automatic bounded semantic freshness, `src/runtime/worklist.rs` owns durable Agent progress, `src/runtime/power.rs` owns idle-sleep inhibition, and `src/runtime/tunnel/` owns managed public tunnel providers and health checks.
- `src/integrations/` — model/client integration boundary. `src/integrations/mcp/` owns shared routing, stdio/SSE adapters, dispatch, compact Tool schemas, durable Tasks, authorization, and Web transport; `src/integrations/auth/` owns OAuth/PKCE/DCR and request-origin state. `src/integrations/agent_plugin/` exports canonical package files and `src/integrations/agent_install/` owns Host detection, safe merge/apply, and reporting.
- `src/workspace/` — secure local coding boundary: bounded file/search/edit/move/delete primitives, root/registry isolation, command policy, local authorization, media inspection, Convention checks, and dependency-aware path scheduling.
- `src/design/` — structured Desired Software State loading, stable-ID/reference validation, sparse initialization, and implementation/verification mappings.
- `src/graph/` — lazy Tree-sitter code index plus provider-neutral Software Graph contracts and persisted provider/composite revisions.
- `src/semantics/` — persistent candidate/confirmed/retired Semantic Registry and first-party LSP provider runtime.
- `src/intelligence/` — repository-intelligence contracts: traceability, scoped/task context, drift/impact/risk, and Engineering Observatory projections. `src/intelligence/observatory/architecture.rs` owns the System → Subsystem → Component blueprint plus Design-vs-Actual architecture projection.
- `src/verification/` — Verification Plans, blind reviewer/readiness state, Language Quality providers, and Property/Mutation/Fuzz/Runtime executors.
- `src/evidence/` — provenance-bearing Evidence contracts and bounded persistent storage.
- `src/reconciliation/` — durable desired-to-actual plans plus dependency-aware execution/retry state.
- `src/ui/` — operator experience. `src/ui/monitor/` owns Ratatui runtime, state, metrics, commands, detail panels, overlays, shell actions, i18n, and theme; `src/ui/intelligence_web.rs` plus `src/ui/intelligence_web/` assets serve the protected Engineering Observatory and its project-digital-twin views.

When moving a responsibility, update Product Scope roots and Design State implementation references in the same change. A physical refactor is incomplete when the architecture contract still points at the old owner.

## Runtime invariants

A Tool call has one real lifecycle:

```text
request → queued → semaphore acquired → running → completed | failed
```

The global semaphore remains the total tool-concurrency cap. Process-executing MCP tools and project checks first acquire a separate execution-admission permit, leaving up to four slots of headroom for non-command tools; at one total slot there is no reservation. Both permits follow real work, including a started blocking worker after caller cancellation. Inner CPU, I/O and process queues retain their own bounded capacities. Composite operations must not retain a parent permit while children wait for permits. `parallel_tools`, `review_changes`, `verify_project`, and other internally fanned-out operations therefore schedule real child work through the same global accounting path.

Parallel-first execution is a core repository constraint, not optional guidance. When Agent Context or the active Worklist exposes two or more independent runnable lanes, agents must fan them out concurrently up to the runtime cap; serializing known-independent work is non-compliant. Prefer concurrent top-level calls when the Host supports them, and use `parallel_tools` for compact known operations when it does not.

`parallel_tools` is resource-aware, not a generic race-everything helper. Its scheduler models `reads`, `writes`, `creates`, `moves_from`, `moves_to`, and `deletes`. Independent resources may fan out; overlapping resources are dependency ordered. Same-file `apply_edits` may coalesce only when callers pin the same observed SHA and edits are non-overlapping and unambiguous. Invalid overlap is rejected before execution. Dispatch is completion-driven, not layer-barrier-driven; failed dependencies skip successors, physical parent/subspace aliases share scheduling identity, and cancelling a parent must not detach queued children. Already-running blocking filesystem operations are not rolled back. Coalescing cannot cross intervening dependent operations or exceed the 128-edit transaction limit.

Runtime collections are bounded. Fan-out count, individual/aggregate result bytes, model-facing read/write size, source scans, retained complete ASTs, Git review files/findings, traffic history, persistent state history, and per-path lock maps all have explicit limits. New caches must invalidate from real source/profile/provider revisions rather than request frequency.

Verified-experience activation cache keys must cover complete normalized records and current guarded file membership, using the same input snapshot for invalidation and replay. Metadata-only membership checks cannot replace source SHA or verification proof. A browser's acknowledged revision must have been observed before the corresponding project request; stale cache responses cannot acknowledge a newer revision, and deferred refreshes must retain request/workspace-generation and visibility checks.

The coding-context hot path optimizes both model cost and wall time:

- `agent_context` is the default coding entry point and uses an explicit or adaptive bounded approximate token budget;
- simple direct-target tasks stay small while ambiguous/cross-module work can grow within a fixed bound;
- scope-aware cold repo-map construction avoids a full-repository graph when direct target ownership is known;
- repo-map structure is revision-aware cached, while query ranking is recomputed per task; unambiguous trace-to-code, code-to-test and edit-to-ripple requests may use versioned heuristic priors, but mixed intent abstains to balanced context, exact symbol targets remain stronger than recall-oriented seeds, and tight budgets discard routing metadata before edit-critical SHA/source/test context;
- multiple symbol queries traverse/index a source root once rather than rescanning for every token;
- bounded Hot Source keeps the strongest direct body when useful, while additional bodies remain progressively disclosed;
- fresh semantic/runtime/deterministic graph evidence may strengthen caller/callee/dependency ranking; stale semantic revisions automatically fall back to syntax; simple symbol localization stays on Tree-sitter/search, while explicit cross-file relationship tasks may be routed to `semantic_navigation` and its warm provider session;
- edit targets retain SHA/writeability and direct working-tree state;
- readiness and deterministic `next_actions` tell the agent whether to edit, fetch more source/semantics, or verify;
- timing/cache/savings telemetry stays in Tool Result `_meta` instead of consuming model-visible context.

The monitor reflects real work. Queued/running/completed state, bytes, peak concurrency, Agent Context calls, average model-visible tokens, repo-map cache-hit rate, and saved context are derived from actual request execution. Terminal raw mode, mouse capture, cursor state, and the primary screen must be restored through the existing RAII boundary; Ctrl-C uses the same graceful shutdown path. The monitor does not start for non-TTY stdout or `--no-monitor`.

Managed public tunnels are owned runtime children, separate from the local HTTP server. `--tunnel-provider auto` starts Cloudflare, `localhost.run`, Pinggy, and Tailscale Funnel concurrently in the background; the dashboard never waits for them. A tunnel counts as live only after URL discovery and instance-matched `/healthz` succeeds — a URL string alone is not readiness. Every live tunnel is kept; the first verified tunnel becomes primary, but primary health is hysteretic: one or two failed probes keep it live, the third consecutive failure permits failover, and recovery from unhealthy requires two consecutive successes. Standbys maintain independent bounded health leases; repeated standby probe failures revoke eligibility, stale leases expire, and failover considers only an eligible lease, deterministically preferring fresher verification, then longer uptime, then stable tie order. Asynchronous completions are identity-bound: standby probes carry a lease epoch and a managed-primary result may update global health only while its URL is still the current primary, so a late result from a recycled or demoted tunnel cannot poison its replacement. A dead provider respawns alone through bounded exponential backoff plus deterministic provider jitter; repeated deaths open a cooldown circuit, each due retry is one half-open instance-verified attempt, and death history resets only after sustained stable uptime rather than immediately on connection. If no verified standby remains, public health returns to pending/local state while recovery continues. Quick-tunnel providers may return a new hostname after reconnect, so durable remote clients need a stable endpoint such as Tailscale Funnel or an operator-managed `--public-url`. Normal shutdown aborts an in-flight health task instead of waiting for its network timeout, then kills/waits for owned tunnel children. Never implement recovery by killing or replacing unrelated operator processes.

Streamable HTTP, `mcp-stdio`, and legacy `/sse` + `/message` call the same JSON-RPC dispatch, Harness, and Workspace implementation. SSE sessions are owner/origin bound, capacity bounded, channel bounded, and removed when the stream closes. Notifications return 202 without a response event; a saturated session returns 429 rather than blocking the server. Supported protocol revisions remain explicit. Modern Tool/Task/Resource behavior is enabled only when the request revision/capability actually permits it; legacy or capability-unknown clients fail closed where required. MCP Tasks are durable coordination records, not promises that process execution survives runtime replacement.

OAuth origin selection is request-specific. Only a Host registered after instance-matched health verification may become an OAuth issuer for a request. Access tokens continue across verified aliases, and refresh rotates the binding to the Host handling the request. Client and token state is atomically persisted under a hash of the configured Workspace roots, while authorization codes stay memory-only. Loading historical token resources supports restart and tunnel migration without registering those old origins as active request Hosts. Stores remain bounded and malformed or symlinked state fails closed.

The Agent installer never executes a Host CLI. Detection is filesystem/PATH evidence only; safe adapters write project-local files, parse before merge, and use normal Workspace atomic writes. JSONC/YAML and unknown schema shapes remain manual. Keep adapter metadata in the registry instead of adding Host-specific branches to `main.rs`.

Media remains metadata-first. `include_content=true` is the explicit per-call opt-in for standard MCP `image` / `audio` Tool Result content blocks; no private client extension is required. Video remains metadata-only because MCP has no standard video Tool Result content block.

## Engineering Control Plane and repository-intelligence invariants

Repository intelligence is one subsystem of the wider Engineering Control Plane. Stable `wcode intelligence` / `wcode verification` CLI and `/intelligence/*` HTTP names remain compatibility surfaces; user-facing terminology is Engineering Observatory / Repository Intelligence rather than treating “Software Intelligence” as the whole product.

Engineering Observatory is **hierarchy-first and architecture-first**. It first shows a System → Subsystem → Component → Code blueprint derived from Design State and implementation ownership, then live engineering flow, Vibe Coding change impact, declared Design dependencies, current code-derived Actual relationships, observed drift, Evidence Coverage, and Implementation Coverage. Component Inspector and Requirement drill-down follow. The raw dependency graph is secondary. Requirement detail preserves:

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
- derived subspaces only inside an already configured parent, with relative resolution from the selected Workspace and symlink-component rejection; manually overlapping configured roots stay blocked;
- rejection of absolute paths, parent traversal, protected paths, symlink components, and unsafe hard-link writes;
- SHA-256 edit preconditions, per-file locking, post-lock path re-resolution, bounded atomic writes, and create-without-overwrite semantics;
- `delete_path` as the only model-facing deletion primitive, limited to one regular file or empty directory after exact one-shot human approval; recursive/root/protected/symlink/hard-link deletion stays permanently blocked;
- no-shell execution: a model supplies a bare executable plus argument array, never shell syntax or a path-bearing interpreter;
- command-specific policy for known development CLIs rather than “authorize program means authorize every subcommand”;
- hardened autonomous execution for bounded local developer workflows across all indexed languages, including repository scripts/builds, code generation and repository-declared verification executors; exact `RiskyExecution` remains for genuinely consequential remote/external operations that are not part of that bounded local lifecycle;
- permanently blocked credential/admin/destructive surfaces including force/delete/mirror Git pushes, `git reset`/`restore` mutation paths, `gh auth`/`api`/secret/variable/extension bypasses, Kubernetes cluster mutation, Terraform apply/destroy/import/state-secret surfaces, shell interpreters, and filesystem/config redirection outside the selected Workspace;
- Git mutation only through explicit-path `git add`, message-only `git commit`, and explicit remote+ref non-force `git push`. An approved SSH push may use the current `SSH_AUTH_SOCK` only through wcode's fixed non-interactive SSH command. Token-like environment variables, Credential Helper, AskPass, arbitrary Git config, proxy helpers, HTTP extra headers, hooks, and external diff helpers remain stripped/disabled;
- `gh` remote mutations remain non-interactive and option-allowlisted; new/unknown write flags fail closed rather than silently inheriting trust;
- URL arguments never embed credentials, and protected credential/environment files remain outside model-facing filesystem/index surfaces;
- every indexed language must keep exactly one contract-tested canonical LSP launch profile; provider-specific executable aliases/arguments (including Workspace/runtime-unique state such as JDT LS `-data`) belong in the LSP adapter, and only proven alternates may remain in fallback order; compatibility tests do not pretend the external server is installed, while live initialize failures must remain observable and may fall through only to a separately trusted installed alternate;
- automatic first-party LSP execution is limited to built-in providers with an explicit hardened profile: the executable must resolve outside the Workspace, pass a bounded viability check rather than mere path existence, keep stdio/output/time bounded, scrub credential and execution-injection environment variables, and disable provider-specific repository-code execution where supported; warm sessions are capacity/idle bounded, keyed to Workspace + LSP server binary identity, synchronize documents by source revision, and never bypass result-side Workspace filtering; `--no-semantic` disables the lane; non-profiled LSPs still require explicit trust, while bounded repository-declared verification executors use the hardened autonomous local executor lane;
- bounded output, timeout termination, sensitive-environment scrubbing, and disabled interactive prompts;
- HTTPS/loopback public URL policy, OAuth PKCE/resource binding, bounded DCR metadata, exact redirect policy, Origin validation, and refresh-token rotation.

`--allow-risky-exec`, `--allow-destructive-writes`, overlapping/broad Workspace flags, and equivalent switches are trust-boundary expansions. Treat them as explicit operator decisions, never default convenience settings. Exact session authorization is preferable when the operation can be narrowly described. None of these mechanisms is an OS sandbox.

Known development CLI policy should be widened by **adding an inspected command family**, not by falling back to generic execution. Bounded local development operations — reads, tests, checks, builds, format/lint, workspace scripts, code generation and dependency maintenance — are autonomous when the command family can enforce Workspace/no-shell/environment/process boundaries. Inline interpreter eval, compiler/plugin/agent injection, remote mutation, credential access, host-wide changes and destructive infrastructure remain authorized or blocked according to consequence. Security tests must cover both an allowed representative shape and blocked escape/mutation shapes for every new family.

## Cross-platform dependency handling

Managed tunnel dependencies are provider-specific. Cloudflare uses `cloudflared`; explicit Cloudflare selection may use the existing Homebrew/winget installation flow unless `--no-install` is set. `localhost.run` and Pinggy use the system `ssh` client and never trigger package installation. Tailscale Funnel uses the `tailscale` CLI (logged in, Funnel enabled on the tailnet) and exposes the machine's stable `ts.net` URL; only one wcode instance per machine can hold the Funnel listener. In `auto` mode, missing `cloudflared` is skipped so a zero-install SSH provider can be attempted immediately.

Known development CLIs are not automatically installed. Harness/quality/provider discovery reports real availability. Optional acceleration must keep a correct fallback when one exists. Rust full verification, for example, prefers `cargo-nextest` only when the repository declares nextest configuration and the executable is installed; otherwise it uses `cargo test`. The strict Harness lane admits only fixed `cargo nextest run [--locked]` shapes, not arbitrary nextest arguments.

Language servers and stage executors similarly expose registered-vs-available state. Hardened automatic LSP workers run only for the most-specific discovered Workspace so a broad parent root and its project subspaces do not index the same files twice; every real automatic refresh acquires the same global Harness semaphore as model-facing work. Never turn “known candidate” into “installed/runnable” merely because wcode recognizes its ecosystem.

## Required verification

The Setup Hub behavior tests require Node.js on PATH. They execute the embedded
JavaScript with Node built-ins and a simulated DOM; no npm packages are needed.
Node is a development/test prerequisite, not a dependency of the shipped wcode
binary. These tests do not replace real-browser visual and accessibility checks.

Before release, the repository must pass the full gate:

```bash
git diff --check
cargo check --locked
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
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

Packaged binaries must report the expected `wcode --version` and render `wcode --help` successfully. macOS artifacts are explicitly ad-hoc re-signed after the final strip/lipo step, verified with `codesign`, and the Universal archive is installed through `install.sh` as a release smoke test before upload. The Unix installer stages and smoke-tests a downloaded binary before atomically replacing an existing installation. Archives and checksums are release artifacts. Historical release notes describe their tagged version and should not be mass-rewritten to reflect later product semantics.

Version consistency across Cargo/package metadata and generated Agent Plugin/marketplace manifests is a release gate. Tag pushes are the single automatic publish trigger; publishing the resulting GitHub Release must not start a duplicate release pipeline. Keep history documents on their historical version while current package/plugin manifests match the release being prepared. Do not commit, tag, push, or publish merely because local checks pass; those are explicit release actions.
