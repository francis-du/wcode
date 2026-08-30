---
layout: docs
title: Software Intelligence
description: Implemented wcode Software Intelligence Runtime features and workflows
lang: en
alternate: /zh/docs/software-intelligence/
permalink: /docs/software-intelligence/
---

# wcode Software Intelligence Runtime

This guide covers the software-intelligence features that are available now and
the evidence each view is allowed to claim.

The same state is available through MCP, local CLI commands, the TUI, and the
protected WebUI. The **architecture-first** Project Observatory compares
declared component dependencies with relationships observed in current code,
then reports **observed drift**, **evidence coverage**, and **implementation
coverage** before opening component or requirement detail. Requirement detail
keeps one stable order: Desired State → Actual State → Change → Proof →
Convergence. The Software Graph stays a lower-level API.

Local `mcp-stdio`, remote Streamable HTTP + OAuth, and legacy SSE share one MCP
core. `agent_context` is the compact coding entry point; deeper Design, Graph,
and Verification tools are called only when the task needs them. Plugin exports
reuse the canonical `wcode-agent-plugin/` package, include standard `mcp.json`,
and never contain credentials or an implicit Workspace. Persistent state is
scoped by Workspace. A fact is called semantic only after a real, fresh LSP
provider returns it; otherwise precision remains Tree-sitter syntax.

## What changes for the user

The normal startup and connection flow does not change:

```bash
wcode --workspace "$PWD"
```

You can also inspect durable state without connecting a model:

```bash
wcode --workspace "$PWD" intelligence
wcode --workspace "$PWD" intelligence --check --json
wcode --workspace "$PWD" verification
wcode --workspace "$PWD" verification --plan-id VP-...
```

`intelligence --check` turns the read-only status surface into a fail-closed CI/release gate. It returns non-zero for invalid or uninitialized Design State, incomplete Requirement→Component / Design→Implementation / Acceptance→Verification coverage, or required Convention errors. Product Scope mapping becomes a hard gate only when Design State explicitly declares `CONSTRAINT-PRODUCT-SCOPE-CANONICAL`; third-party repositories can still inspect `scope_status` without being forced into wcode's own 12-scope source layout. Its JSON output includes the same `scope_status` and `conventions` state used by the runtime; Convention warnings do not fail the check.

Repository-aware semantic servers and stage executors are an explicit trust expansion because they can load or run repository-controlled code/configuration. In the interactive runtime, the first exact operation that lacks trust returns a local authorization request; approve it in the TUI or protected WebUI and retry. The approval becomes a session grant for that operation fingerprint. `--allow-risky-exec` is the broader process-wide pre-authorization path:

```bash
wcode --workspace "$PWD" --allow-risky-exec intelligence --refresh-semantic
wcode --workspace "$PWD" --allow-risky-exec verification --plan-id VP-... --execute-stages
```

Press `I` to load Intelligence for the selected project, `C` for the complete
command catalog, and `W` for the protected Project Observatory. The pairing
code remains visible after a client connects. The TUI and WebUI show the same
pending requests and distinguish executable access from an exact repository
operation.

The Observatory file view comes from the current bounded Software Graph
snapshot. It shows the project tree, depth, largest files, and files above the
1,000-line repository limit. If indexing reached its safety bound, the view is
marked as truncated; the browser does not start a second filesystem scan.

Proof counts only Evidence whose code and Design revisions match the current
repository. Local agents use `wcode --workspace <repo> mcp-stdio`; remote
clients prefer `/mcp`; older clients can use `/sse`. Plugin and one-command
Host setup are documented in
[Code Agent Integrations](../code-agent-integrations/).

The normal coding workflow is intentionally smaller:

```text
agent_context(goal, scopes=...)
    ↓
symbol_context only when readiness needs more source
    ↓
apply_edits / apply_file_edits
    ↓
review_changes
    ↓
verify_project
    ↓
deeper drift / risk / reconciliation / evidence only when needed
```

## 1. Install or update wcode

Install the latest release:

```bash
curl -fsSL https://raw.githubusercontent.com/francis-du/wcode/main/install.sh | sh
```

If wcode is already running, restart the installed/current executable so the MCP client receives the new tool schemas:

```bash
wcode restart
```

A client that cached `tools/list` may also need to reconnect or refresh its MCP connection.

## 2. Add Design State to a project

Design-aware tools look for `.wcode/project.yaml` and `.wcode/design/` inside the selected workspace. On an uninitialized writable workspace, `design_init` can create the minimal structure without overwriting existing design files:

```json
{
  "name": "my-service",
  "description": "Example service managed with wcode Design State."
}
```

wcode itself dogfoods the format. `design_init` intentionally creates only meaningful initial state:

```text
.wcode/
├── project.yaml
└── design/
    └── product.yaml
```

Design State is sparse. `requirements.yaml`, `components.yaml`, `constraints.yaml`, `acceptance.yaml`, and `decisions.yaml` are **not** created as empty `[]` placeholders. Add a collection document only when that kind of desired state exists. The loader also accepts one document per item under collection directories such as `design/requirements/`, `design/components/`, `design/constraints/`, `design/acceptance/`, and `design/decisions/`.

Example project metadata:

```yaml
schema_version: 1
name: my-service
description: Example service managed with wcode Design State.
```

Example requirement:

```yaml
- schema_version: 1
  id: REQ-AUTH-001
  title: Refresh tokens rotate
  intent: Reusing an already-consumed refresh token must fail.
  priority: critical
  implemented_by:
    - component:auth
  acceptance:
    - AC-AUTH-001
  constraints:
    - CONSTRAINT-ROTATION
  risk:
    security: critical
```

Example component mapping:

```yaml
- schema_version: 1
  id: component:auth
  name: Authentication
  responsibilities:
    - issue and rotate refresh tokens
  constraints:
    - CONSTRAINT-ROTATION
  implementation:
    - kind: symbol
      path: src/integrations/auth.rs
      symbol: refresh_access_token
```

Example acceptance criterion:

```yaml
- schema_version: 1
  id: AC-AUTH-001
  title: Refresh token reuse is rejected
  statement: A consumed refresh token cannot be exchanged twice.
  verification:
    - kind: test
      path: src/integrations/auth.rs
      symbol: tests::refresh_tokens_rotate_and_preserve_binding
    - kind: check
      id: rust-test
```

Design IDs and references are validated. Implementation and test symbol references are resolved through the existing Tree-sitter index and therefore explicitly report `precision=syntax`, not compiler-level semantics.

## 3. Recommended agent workflow

For substantial coding work, start from one compact task-specific call rather than a fixed sequence of broad status tools:

```text
1. agent_context(goal, scopes=...)
2. follow readiness / next_actions
3. symbol_context only if more source is needed
4. apply_edits or apply_file_edits
5. review_changes
6. language_quality_run / drift / impact / risk only when the task requires them
7. verify_project + required advanced stages
8. evidence_status / reconciliation only when convergence or proof needs deeper inspection
```

`agent_context` uses a bounded adaptive budget when `budget` is omitted. It combines relevant Design State, scope-aware repo-map ranking, fresh semantic/runtime evidence when usable, bounded Hot Source, exact SHA edit targets, related tests, working-tree advisories, readiness, and deterministic next actions. The 1000-token extreme mode prioritizes direct editability; the default adaptive path can grow when the task is ambiguous or cross-module. `project_context`, `scope_status`, `design_status`, `traceability_status`, `software_context`, `language_quality_status`, and graph/risk tools remain available for deliberate deeper inspection rather than mandatory startup overhead.

### `agent_context`

Use `agent_context` as the normal coding entry point. It is designed to replace multiple startup discovery round trips with one bounded edit-ready pack. Repo-map ranking combines direct task relevance with existing Software Graph relationships; fresh semantic/runtime/deterministic evidence can strengthen those relationships, while stale provider facts automatically fall back to syntax. The pack keeps model-visible telemetry out of band in Tool Result `_meta` and reports explicit readiness instead of a generic quality score.

### Product Scopes

wcode has one canonical registry for its own product/control-plane boundaries: `runtime`, `integrations`, `workspace`, `design`, `graph`, `semantics`, `traceability`, `risk`, `verification`, `evidence`, `reconciliation`, and `experience`. `workspace_info` and `project_context` expose the registry. `scope_status` applies it to the selected repository and reports per-scope source counts plus bounded unmapped supported-source paths. `tools/list` attaches `dev.wcode/productScopes` to each Tool `_meta`, and MCP Resource clients can read `wcode://runtime/product-scopes`. The same live scope audit is surfaced through the Intelligence operator views.

These are not vendor names and they do not replace business/domain semantic scopes. Known Product Scope aliases are canonicalized; unknown scope strings remain valid freeform semantic scopes. For task retrieval, recognized scopes narrow source navigation to the registered source roots. For `semantic_query`, scoped facts must overlap a requested scope while unscoped facts stay global. The maintained mapping is documented in [product-scopes.md](../product-scopes/).

### `software_context`

Use it when the task starts from behavior or intent instead of a filename.

Example arguments:

```json
{
  "query": "workspace command security",
  "intent": "modify",
  "budget": 12000,
  "scopes": ["workspace"]
}
```

It canonicalizes optional `scopes`, token-scores the task query, uses the requested `budget` to cap returned context, and returns matching requirements, components, constraints, acceptance criteria, decisions, structured `design_items` with their intent/relations, syntax-level symbols, known risks, and bounded traceability coverage. Recognized Product Scopes narrow source/symbol navigation; when no recognized Product Scope is supplied, source navigation remains workspace-wide. It also returns `graph_context`: a bounded neighborhood from fresh semantic/runtime provider graphs, ranked by task text, semantic expansion tokens, and overlap with already-matched symbol paths. Each returned node/edge keeps provider and precision provenance.

### `traceability_status`

This resolves the declared chain:

```text
Requirement
  → Component
  → File / Symbol
  → Acceptance Criterion
  → Test / Harness Check
```

Coverage is returned as separate dimensions rather than one health score:

- requirement → component
- design → implementation
- acceptance → verification

### 22-language Semantic Providers

`semantic_provider_status` scans the workspace and reports provider availability for every language already supported by wcode's syntax index: Bash, C, C++, C#, CSS, Dart, Elixir, Go, HTML, Java, JavaScript, Lua, OCaml/interfaces, PHP, Python, R, Ruby, Rust, Swift, TypeScript, and TSX.

The registry auto-detects common LSP servers such as `clangd`, `csharp-ls`/OmniSharp, `gopls`, `jdtls`, `typescript-language-server`, `pyright`/`pylsp`, `rust-analyzer`, `sourcekit-lsp`, `ocamllsp`, `lua-language-server`, `ruby-lsp`, PHP/Elixir/Dart/R language servers, and the HTML/CSS/Bash servers. Availability is reported separately from language support: wcode does not claim a server is runnable when its executable is missing.

`semantic_provider_refresh` starts the detected server over bounded stdio LSP and requests real hierarchical Document Symbols. When the server advertises Call Hierarchy and/or Implementation support, wcode also imports those relationships. Successful first-party nodes carry `source_sha256`; provider status therefore reports `fresh` / `stale`, and stale LSP revisions are excluded from graph overlays, impact, reconciliation, and `software_context.graph_context`. Refresh also computes a revision key from source hashes, provider executable metadata, and the symbol bound: an unchanged revision is returned as `cached=true` without relaunching the language server. A server that returns no semantic symbols does not create a fake semantic revision.

Because language servers can evaluate project configuration, build metadata, plugins, or generated project state, refresh requires explicit repository trust. With `--allow-risky-exec` disabled, an unapproved refresh fails closed and creates a local `RiskyExecution` authorization request; approve it in the TUI or protected WebUI, then retry the same refresh. Use the flag when the operator intentionally wants process-wide pre-authorization:

```bash
wcode --workspace "$PWD" --allow-risky-exec intelligence --refresh-semantic
```

Without approval (or the process-wide flag)—or without an installed server—the Tree-sitter graph remains available as `precision=syntax`. External SCIP/compiler/runtime indexers can still use `graph_provider_import`; the first-party LSP registry supplements rather than replaces the provider-neutral import contract.

### Language Quality Matrix

Language support is a capability vector, not a checkbox. `language_quality_status` reuses the same 22-language surface and reports syntax, semantic, format, lint, type-check, static-analysis, test, security, Property, Mutation, Fuzz, and Runtime-Canary coverage separately. Repository manifests/configuration and package scripts define intent; known ecosystem tools may remain visible candidates but are not treated as repository policy until declared. Missing executables and missing dimensions remain explicit gaps.

`language_quality_run` accepts one provider returned by the matrix and runs it only when the language is detected and the provider is repository-declared, available, and registered as check-only. The command still crosses the normal repository-aware authorization boundary. Formatter/fixer write modes are intentionally absent. The result is converted to a Verification Report and persisted as current code+design revision Evidence, so a historical green run never proves a later revision.

Current provider families include native/check-mode Rust, Go, Dart, .NET, Maven/Gradle, Mix, Dune and SwiftPM flows plus repository-declared Ruff/mypy/Pyright/Bandit, Biome/ESLint/Stylelint/TypeScript, clang-format/clang-tidy, Checkstyle/SpotBugs/Spotless, Credo/Dialyzer, ShellCheck/shfmt, StyLua/Luacheck, PHPStan/Psalm/PHPUnit, lintr/testthat, RuboCop/RSpec and swift-format/SwiftLint. This describes registry capability, not host installation. See [language-quality.md](../language-quality/).

### Graph history and diff

`software_graph` persists deduplicated meaningful graph snapshots. `graph_history` lists them, `graph_query` reads one revision or neighborhood, and `graph_diff` compares two revisions (or the latest two by default). Diff aligns nodes by stable node ID and edges by `from + to + kind + provider + precision`; a provenance-revision/attribute change is reported as `changed` rather than noisy delete/add churn. Repeated stable edge identities are compared as revision multisets, so future richer SCIP/runtime providers do not lose duplicate relationships. The Project Observatory uses this history for its architecture-revision timeline and latest Node/Edge `+ / - / ~` delta, while its feature architecture is regenerated from the current repository on refresh.

### Change intelligence

The following tools analyze the current Git working tree and Design State:

| Tool | Purpose |
| --- | --- |
| `drift_status` | Detect implementation drift and design drift. |
| `impact_analysis` | Map changed paths to declared components, requirements, acceptance criteria, implementation symbols, public-API/security signals, and transitive reverse callers from the bounded composite call graph. It consumes real semantic/runtime Calls when provider facts exist and syntax Calls otherwise; provider/precision/truncation stay explicit. |
| `risk_status` | Combine Git review, including deterministic maintainability findings, drift, and traceability gaps into structured risks plus a risk-adaptive verification profile. |
| `reconciliation_plan` | Build and persist a bounded convergence plan with drift IDs, graph-aware impact, tasks, Change IR intents, and a Verification Plan. |
| `reconciliation_status` / `reconciliation_history` | Reload one persisted plan or list recent plans after reconnects/restarts. |
| `reconciliation_execution_status` | Read durable dependency-aware task execution and synchronize Verification/Human Approval tasks from real evidence. |
| `reconciliation_claim` / `reconciliation_submit` / `reconciliation_retry` | Claim runnable implementation/design tasks, persist success/failure evidence, and explicitly requeue failed work. Source modification itself still uses normal wcode edit tools and their security invariants. |

These tools require command execution because they internally use the bounded Git change-review path. They therefore do not work with `--no-exec`.

`review_changes` also reports three deterministic maintainability signals. `maintainability-file-crossed-1k` marks a non-deleted source file that crossed 1,000 lines in the current change. `maintainability-concentrated-growth` marks at least 400 net new lines in one source file. `maintainability-cross-scope-churn` marks at least 1,000 changed source lines across three or more Product Scopes. These are review signals, not a verdict on design. The Convention Engine uses the same 1,000-line boundary for the repository as a whole. See [maintainability-review.md](../maintainability-review/).

## 4. Verification Mesh

`verification_plan` converts the current risk level into a verification policy and creates blind independent reviewer jobs.

The deterministic level is currently mapped to the existing Harness:

- low risk → `quick`
- medium/high/critical risk → `full`

The plan can also require Property, Mutation, Fuzz, Runtime/Canary evidence, adversarial review, or human approval. Medium-and-higher risk plans include a blind `maintainability` reviewer job in addition to correctness; that job requires `maintainability_review` and carries structural guidance around deleting complexity, avoiding scattered special cases, keeping canonical ownership, questioning 1,000-line threshold crossings, and simplifying orchestration. A correctness Pass cannot substitute for the maintainability job. Verification Plan/Job state is persisted per workspace, so another wcode process or model executor can resume queued/claimed work. `verification_executor_status` reports the cross-language runner registry and whether each executable is actually available. `verification_execute_stages` runs every applicable available runner for a required stage (skipping only a producer whose latest Evidence already passes) and converts each real command result into persistent Stage Evidence; one runner failure is not hidden by another runner's later success. `verification_stage_submit` remains the provider-neutral adapter for CI/external systems. `verification_status` keeps the latest result per producer and aggregates fail-closed as `Fail > Disagree > Inconclusive > Pass`; a Plan also becomes stale when the workspace code revision changes after plan creation.

Built-in discovery recognizes common ecosystems such as proptest/quickcheck/cargo-fuzz/cargo-mutants, Go property/fuzz tests, Hypothesis/mutmut, fast-check/Stryker, jqwik/PIT, FsCheck/.NET Stryker, SwiftCheck/Muter, StreamData, Glados, Rantly, Eris/Infection, QCheck, and R quickcheck when the corresponding project/tool is present. Those integrations are convenience adapters, not a closed list.

Every one of the 22 indexed languages can provide project-specific runners through `.wcode/executors.yaml` without changing wcode itself:

```yaml
schema_version: 1
executors:
  - id: service-canary
    stage: runtime_canary
    languages: [go]
    program: ./tools/check-canary
    args: [--environment, staging]
    cwd: .
    timeout_seconds: 60
```

Configured executors run without a shell, remain workspace-scoped, hide configured arguments from status/UI serialization, and scrub sensitive environment/output. Workspace-relative programs are resolved through the same canonical-root and symlink protections as other workspace operations. Without process-wide `--allow-risky-exec`, the first exact executor operation creates a local `RuntimeExecutor` authorization request; approve it in the TUI or protected WebUI and retry. Use the flag when repository-aware executor work is intentionally pre-authorized for the process:

```bash
wcode --workspace "$PWD" --allow-risky-exec verification --plan-id VP-... --execute-stages
```

A missing executable is reported as unavailable/missing; it never produces pass Evidence.

### MCP 2026 long-running Tasks

For MCP `2026-07-28`, wcode advertises the official `io.modelcontextprotocol/tasks` extension. The extension remains per-request opt-in: a client must include it in `_meta.io.modelcontextprotocol/clientCapabilities.extensions` on the request that wants Task behavior. Only the two deliberately long-running tools are task-augmented today: `semantic_provider_refresh` and `verification_execute_stages`. Clients that do not opt in keep the existing synchronous `tools/call` response.

A Task is persisted before its handle is returned, scoped to a SHA-256 fingerprint of the authenticated OAuth `client_id` (never the raw bearer token), and bounded per workspace. `tasks/get` polls status and returns the original Tool Result after completion; `tasks/update` is currently ack-only because these tools do not issue input requests; `tasks/cancel` persists `cancelled` before aborting the worker so a late completion cannot overwrite cancellation. Terminal tasks may be reclaimed only when capacity is needed; active tasks are never evicted to make space. If a runtime process is replaced while a Task is still working, the next read marks it failed rather than pretending the worker survived the restart.

### Independent reviewer jobs

A model or external reviewer first claims a job with `verification_claim`.

Example correctness reviewer:

```json
{
  "reviewer": "reviewer-a",
  "capabilities": ["correctness_review"],
  "role": "correctness"
}
```

Current capability names include:

```text
correctness_review
maintainability_review
architecture_review
security_review
adversarial_review
design_review
performance_review
compatibility_review
test_synthesis
```

The first-pass job is blind: it does not expose other reviewer submissions.

Submit a structured result with `verification_submit`:

```json
{
  "job_id": "VJ-00000001",
  "reviewer": "reviewer-a",
  "submission": {
    "verdict": "pass",
    "summary": "No correctness issue found.",
    "claims": ["The stale-write precondition is preserved."],
    "risks": [],
    "model": "provider/model/version"
  }
}
```

Use `verification_status` with the returned plan ID to inspect queued/claimed/submitted jobs, reviewer failures/inconclusive results, disagreement, the latest deterministic aggregate result for the change subject, explicit blockers, and the final `ready` decision.

When independent reviewers disagree, wcode records the disagreement itself as `EvidenceResult::Disagree` so downstream UI/risk logic does not need to infer it again.

## 5. Evidence

Every successful `verify_project` run now records deterministic runtime Evidence for its checks. Acceptance criteria whose declared verification references were exercised also receive evidence.

Reviewer submissions create model-review Evidence containing producer/model identity, current design/code revision, policy, result, confidence, and timestamp.

Read it with:

```json
{
  "subject": "AC-AUTH-001",
  "limit": 50
}
```

or omit `subject` for the latest evidence in the selected workspace.

### Persistence model

Durable Software Intelligence state lives in wcode's user-level state directory, keyed by the canonical workspace root. Evidence uses bounded immutable records; Verification uses immutable Plan/Job snapshots; Semantic Facts use immutable revisions; Graph Provider facts and composite Software Graph snapshots retain bounded history; Reconciliation Plans/execution and MCP Task snapshots are stored independently. None of these stores modify the repository or require the workspace to be writable.

Risk is intentionally recomputed from current Design/Git/Code state. Graph history is queryable through `graph_history` / `graph_query` and directly comparable through `graph_diff`. First-party LSP providers expose source-hash freshness and stale revisions are not overlaid; a newly built `software_graph` therefore combines current source with only usable latest provider revisions.

## 6. Current MCP tool surface

### Desired State, semantics, and software structure

- `agent_context`
- `design_init`
- `design_status`
- `traceability_status`
- `software_context`
- `semantic_status` / `semantic_query`
- `semantic_record` / `semantic_confirm` / `semantic_retire`
- `semantic_provider_status` / `semantic_provider_refresh`
- `software_graph`
- `graph_provider_import` / `graph_provider_status`
- `graph_history` / `graph_query` / `graph_diff`

### Change intelligence and convergence

- `review_changes`
- `drift_status`
- `impact_analysis`
- `risk_status`
- `reconciliation_plan`
- `reconciliation_status` / `reconciliation_history`
- `reconciliation_execution_status`
- `reconciliation_claim` / `reconciliation_submit` / `reconciliation_retry`

### Verification and evidence

- `verification_plan`
- `verification_claim` / `verification_submit`
- `verification_executor_status` / `verification_execute_stages`
- `verification_stage_submit`
- `verification_approve`
- `verification_status` / `verification_history`
- `verify_project`
- `evidence_status`

### Existing low-level coding primitives

- `workspace_info`
- `scope_status`
- `project_context`
- `convention_status`
- `search_code` / `search_many`
- `file_outline`
- `find_symbol`
- `symbol_context`
- `read_file` / `read_files`
- `read_media` — metadata-first bounded media inspection; image/audio content requires a matching per-request `run.francis.wcode/media-content` client capability, while unknown/legacy capability fails closed and video remains metadata-only
- `path_info`
- `parallel_tools`
- `replace_text` / `write_file` / `apply_edits` / `apply_file_edits`
- `create_file` / `create_files` / `create_directory`
- `move_path` / `move_paths`
- `delete_path`
- `run_command`

## 7. Implemented now and precision boundaries

Implemented now:

- `design_init` bootstrap plus structured Design State loading/validation;
- a canonical 12-scope Product Scope registry used by source architecture, `workspace_info` / `project_context`, scoped `software_context` navigation, `semantic_query` filtering, convention mapping, MCP Tool `_meta.dev.wcode/productScopes`, and the `wcode://runtime/product-scopes` Resource while preserving freeform business scopes; `scope_status` audits how the selected repository actually maps into that registry and returns bounded unmapped source paths;
- bounded cross-language convention policies and repository architecture findings, included in `project_context` and available directly through `convention_status`, including Product Scope mapping gaps;
- dependency-aware scheduling for independent workspace reads/writes, including path-conflict ordering and safe same-file/same-SHA `apply_edits` coalescing;
- exact one-shot human-authorized `delete_path` for a regular file or empty directory; file deletion requires the current SHA-256, while recursive/root/protected/symlink/hard-link deletion stays blocked;
- a persistent Semantic Registry with candidate/confirmed/retired lifecycle, provenance, human attestation, confirmed-semantic expansion, and provenance-bearing `graph_context` retrieval inside `software_context`;
- composite Software Graph with declared Design nodes/edges, Tree-sitter code nodes, cross-file syntax calls, fresh first-party LSP semantic facts, external semantic/runtime providers, durable graph history/query, and bounded structural `graph_diff`;
- a 22-language first-party Semantic Provider registry with real LSP Document Symbol, Call Hierarchy, and Implementation ingestion, source-hash freshness/stale exclusion, and revision-cache reuse when semantic inputs have not changed;
- Requirement → Component → implementation/test traceability;
- Git-aware drift, graph-aware transitive impact, structured risk, and deterministic maintainability review findings for 1,000-line threshold crossings, concentrated source growth, and cross-Product-Scope churn;
- risk-adaptive Verification Plans with persistent blind reviewer Plan/Job state; medium-and-higher risk plans include a dedicated `maintainability_review` job with structural simplification guidance, while reviewer disagreement Evidence, per-producer fail-closed stage aggregation, HumanApproval Evidence, Verification history, and stale-workspace-revision protection remain explicit;
- cross-language Property / Mutation / Fuzz / Runtime-Canary execution through built-in ecosystem discovery plus `.wcode/executors.yaml`; every applicable available runner executes unless that producer already has passing Evidence, while external `verification_stage_submit` remains available for CI/provider integrations;
- MCP `2026-07-28` task augmentation for `semantic_provider_refresh` and `verification_execute_stages`, with durable-before-handle storage, OAuth-client scoping, polling, bounded cancellation, and synchronous fallback for clients that do not opt into the extension;
- persistent Reconciliation Plans plus dependency-aware claim/submit/retry execution state and reconciliation Evidence;
- local `wcode intelligence --refresh-semantic` / `wcode verification --execute-stages` CLI flows in addition to read-only status views;
- live TUI Software Intelligence overlay (`I`), complete command catalog (`C`), persistent pairing code, and protected Project Observatory (`W`) with an architecture-first component graph, Design-vs-Actual dependency overlay, observed-drift/evidence/implementation coverage metrics, Component Inspector, Requirement drill-down, verification, ADR/constraint context, a bounded project tree and largest-file view, code statistics, mapped Git changes, risk, and architecture revision history;
- MCP exposure of the complete higher-level runtime.

Precision and integration boundaries are explicit rather than hidden:

- the always-available code index remains Tree-sitter `precision=syntax`; a first-party LSP adapter may upgrade individual facts to `precision=semantic` only after a real installed server responds, while SCIP/compiler/runtime providers can still enter through the external import contract;
- all 22 indexed languages share one semantic-provider and verification-executor architecture, but wcode does not bundle every third-party LSP/test binary. `semantic_provider_status` and `verification_executor_status` expose exact host availability instead of pretending absent tools exist;
- repository-aware LSP refresh and Property/Mutation/Fuzz/Runtime execution require explicit operator trust: exact operations can receive local TUI or protected-WebUI session grants and be retried, while `--allow-risky-exec` is the process-wide pre-authorization path; neither is an OS sandbox;
- model-facing command execution uses command-specific policy for the built-in development CLI catalog and exact `RiskyExecution` fingerprints for bounded repository/remote operations. Repository mutation stays narrower: only explicit-path `git add`, message-only `git commit`, and explicit remote+ref non-force `git push` shapes can cross exact approval; an approved SSH push may use the current SSH Agent only through wcode's fixed non-interactive SSH command. Force/delete/reset/restore-style mutation, shell interpreters, credential-bypass surfaces, workspace escapes, and protected resources remain blocked;
- `read_media` never infers vision/audio support from a model or vendor name. `include_content=true` emits an image/audio MCP content block only when the current request declares the matching `run.francis.wcode/media-content` extension; otherwise it returns a structured capability error without binary content;
- Reconciliation execution coordinates durable tasks and evidence, but source edits still use the normal bounded/hash-guarded wcode edit surface instead of a hidden unrestricted patch engine;
- destructive deletion is deliberately outside normal write flow: the first `delete_path` attempt creates an exact local authorization request, the operator approves or denies it in the TUI or protected WebUI, and only a matching retry can consume the one-shot grant.

## 8. Dogfood this repository

The wcode repository already contains `.wcode/project.yaml` and `.wcode/design/*.yaml`, so after installing/restarting the current build you can ask a connected agent:

> Use `agent_context` for the requested wcode change. Follow its readiness/next actions, edit through guarded Workspace tools, run `review_changes` and `verify_project`, then use drift/risk/reconciliation/evidence tools only if the task still needs deeper convergence analysis.

That exercises the implemented Software Intelligence path end to end without requiring a separate demo project.
