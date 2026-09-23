---
layout: docs
title: Repository Intelligence & Engineering State
description: How the wcode Engineering Control Plane understands repository design, implementation, impact, drift and proof
lang: en
alternate: /zh/docs/software-intelligence/
permalink: /docs/software-intelligence/
---

# Stop making the agent rediscover your codebase every session

A coding agent can write code quickly and still be wrong about the system around it. wcode keeps a local, reusable model of the repository so every agent can answer four practical questions before and after it edits:

1. **What is this system supposed to do?** — requirements, components, constraints, decisions, and acceptance criteria.
2. **How does it work right now?** — syntax structure plus real LSP references, callers, implementations, and imported runtime/compiler relationships.
3. **What will this change touch?** — Git-aware impact, product scopes, drift, public API/security signals, and maintainability risk.
4. **What proves the change is safe?** — deterministic checks, language-native verification, independent review, and revision-bound Evidence.

This repository-understanding layer is one subsystem of the broader **wcode Engineering Control Plane**. It is available through MCP, stable `intelligence` CLI/API compatibility surfaces, the TUI, and the protected Engineering Observatory, and it persists beyond a single model or conversation.

## The 60-second repository-intelligence model

```text
Your coding agent
      ↓ asks for task context
agent_context
      ↓ focused code + design + tests + semantic gaps
Tree-sitter ── warm LSP ── Design State ── Git Actual State
      ↓
impact / drift / risk
      ↓ guarded edits
verification / reviewers / evidence
      ↓
Engineering Observatory + durable workspace state
```

![wcode Repository Intelligence stack](/assets/wcode-intelligence-stack.svg)

The Engineering Observatory turns that model into a human-readable project digital twin: layered architecture blueprint, Design vs Actual, live engineering activity, Vibe Coding change story, Desired State → Actual State → Change → Proof → Convergence, and explicit drift. The generic Software Graph remains a provenance-bearing substrate and secondary drill-down, not a visualization users must decipher.

Verification mapping and proof are deliberately separate. `Mapped` means every declared verification reference for an Acceptance Criterion resolves; it does not claim execution. `Executed` means qualifying verification Evidence exists, `Passed` requires all effective verification scopes in the latest observed revision to pass; timestamp ties retain failures rather than choosing a favorable record ID. `Fresh` means that unambiguous revision matches the current code-plus-Design-State revision. These counts remain separate in `ProjectObservatory.proof.acceptance`, so 100% traceability mapping cannot be mistaken for a current passing run.

Local `mcp-stdio`, remote Streamable HTTP + OAuth, and legacy SSE share one MCP
core. `agent_context` is the compact coding entry point; deeper Design, Graph,
and Verification tools are called only when the task needs them. Plugin exports
reuse the canonical embedded `plugin/` package, include standard `mcp.json`,
and never contain credentials or an implicit Workspace. Persistent state is
scoped by Workspace. A fact is called semantic only after a real, fresh LSP
provider returns it; otherwise precision remains Tree-sitter syntax.

## Observatory responsiveness and trustworthy status

Task activity and the project snapshot refresh independently. Activity schedules another request after 2 seconds when tasks or approvals are pending, or 8 seconds when idle/unavailable; project revision checks use a separate 8-second schedule. These are polling delays, not latency guarantees. Initial activity does not wait for architecture reconstruction. Each lane is single-flight, hidden pages stop read polling, and permission-changing requests are never automatically replayed after a timeout.

All access mutations bind both the selected workspace and its selection generation, including an A → B → A round trip. A common in-flight owner prevents duplicate Enter/click submissions. Older reads cannot overwrite a completed approval or the new workspace's access panel. An unavailable authorization list or unsampled memory is unknown, not zero.

`proof.current_*` retains current-revision historical record counts. `proof.effective` separately selects the latest result for each subject, evidence kind, producer, model, confidence, target set and policy. A later known full run can supersede an older matching quick run, never the reverse; different producers and scopes remain independent. Human approval is not test proof. Details prioritize failures, return at most 24 rows with redacted bounded summaries, and explicitly mark truncation. The view is not a release gate and does not erase audit history.

Verification history shares one request-local code/Design revision and one evidence load across its plans. Empty histories do neither. The Observatory reuses its inputs for plan evaluation; the next request re-reads inputs so edits and new evidence remain visible. The lightweight proof change signal hashes bounded record identities and metadata, including late arrivals/removals; it does not parse all record bodies and is never treated as verification evidence. Persistent retention limits still apply.

### Thirty-round audit map

The round numbers identify different scenarios, not thirty identical full-test runs or thirty separate features. Rounds 1–20 are independently reported by `tests/unit/ui/audit.cjs`, exercised from the Rust unit suite. The corresponding report is `target/wcode-audit.json`; history read-count measurements are in `target/wcode-history-perf.json`.

| Rounds | Scope and assertions |
| --- | --- |
| 1–4 | Cross-workspace executable grant/revoke replies; duplicate grant/revoke submission. |
| 5–8 | Late project creation, repeated project submission, read-after-write ordering, A → B → A responses. |
| 9–12 | Approval-count ordering, failed access reads, missing memory samples, unavailable activity. |
| 13–16 | Severity ordering, HTML HTTP errors, activity during a blocked project refresh, hidden/paused scheduling. |
| 17–20 | Concurrent initial activity, single poll-loop ownership, recovered evidence, escaped/truncated detail rows. |
| 21 | Compare individual versus shared verification-history input reads and exact output equivalence. |
| 22 | Reject foreign verification plans before reading their workspace; empty histories avoid scans; edits/new evidence invalidate the next request. |
| 23 | Retry scope, timestamp ties, target normalization, one-way full/quick supersession and approval/proof separation. |
| 24 | Acceptance conflicts, code-plus-Design plan freshness, bounded/redacted effective detail rows and retained history. |
| 25 | Metadata-only evidence signals detect late records/removals and reject symlink records. |
| 26 | Existing UI behavior, syntax, responsive layout contracts, endpoint protection and bilingual documentation compatibility. |
| 27 | Worktree diff, Rust formatting and type-check gates. |
| 28 | All-target Clippy, including test sources, with warnings denied. |
| 29 | Full Rust suite, including JavaScript behavioral and integration/contract tests. |
| 30 | Optimized release build and verification-record/revision consistency review. |

This automated audit does not replace real-browser visual/accessibility testing, remote-connection testing or cross-platform CI. Installing/reconnecting the running process remains a separate operator action.

## What changes for the user

The normal repository flow is:

```bash
wcode setup
wcode
```

The current directory is the default Workspace. You can also inspect durable state without connecting a model:

```bash
wcode intelligence
wcode intelligence --check --json
wcode verification
wcode verification --plan-id VP-...
```

`intelligence --check` turns the read-only status surface into a fail-closed CI/release gate. It returns non-zero for invalid or uninitialized Design State, incomplete Requirement→Component / Design→Implementation / Acceptance→Verification coverage, or required Convention errors. Product Scope mapping becomes a hard gate only when Design State explicitly declares `CONSTRAINT-PRODUCT-SCOPE-CANONICAL`; third-party repositories can still inspect `scope_status` without being forced into wcode's own 12-scope source layout. Its JSON output includes the same `scope_status` and `conventions` state used by the runtime; Convention warnings do not fail the check.

LSP servers and stage executors can load or run repository-controlled code/configuration, so they do not share one blanket trust rule. Hardened first-party LSP servers are enabled by default through a separate bounded lane; today `rust-analyzer` is the first automatic profile. `--no-semantic` disables every first-party LSP execution. LSP servers without an automatic profile and stage executors remain explicit trust expansions: the first untrusted exact operation returns a local authorization request that can be approved in the TUI or protected WebUI and retried. `--allow-risky-exec` remains the broader process-wide pre-authorization path for those non-automatic operations:

```bash
wcode --no-semantic
wcode --allow-risky-exec verification --plan-id VP-... --execute-stages
```

Press `I` to load Intelligence for the selected project, `C` for the complete
command catalog, and `W` for the protected Engineering Observatory. The pairing
code remains visible after a client connects. The TUI and WebUI show the same
pending requests and distinguish executable access from an exact repository
operation.

The Observatory file view comes from the current bounded Software Graph
snapshot. It shows the project tree, depth, largest files, and files above the
1,000-line repository limit. If indexing reached its safety bound, the view is
marked as truncated; the browser does not start a second filesystem scan.

The summary-first Observatory separates four immediately actionable signals: executing tools, pending approvals, working-tree changes, and current-version evidence. Unknown Git status is not a clean tree; missing evidence is not a passing run. The architecture surface starts from a System → Subsystem blueprint with responsibilities, ownership, size, dependency direction, changes and drift; Components are a second-level drill-down, while the raw dependency graph is a secondary diagnostic view. A live engineering flow and bounded timeline project real Harness activity, revision-bound proof, and architecture revisions without creating a second control-state source. Requirements, file structure, diagnostics and provider matrices remain progressively disclosed.

The protected `/intelligence/activity` endpoint reads the existing monitor without executing Git, rebuilding the graph or resetting TUI observation windows. It returns at most 12 task rows for the selected workspace, with queue time separate from execution time. Completed/failed totals are process-lifetime history, not current blockers; process queues and resident memory are explicitly shared by all workspaces. Raw task arguments and other workspaces' task records are not returned.

The eight-second refresh loop is single-flight and pauses new polls while hidden. A revision is acknowledged only after its project fetch succeeds; late responses cannot replace another selected workspace. Evidence identity participates in the revision signal, so completed checks can refresh proof without a source edit. Failed refreshes retain a visibly stale snapshot instead of silently presenting old numbers as live. Optional browser-storage failures do not prevent loading the UI. Access reads retain their workspace and request generation; double-clicking a pending decision sends one mutation. Exact-operation arguments accept a JSON string array, preserving spaces and empty arguments instead of attempting shell parsing.

The `current_*` proof counters include only Evidence whose code and Design revisions match the current
repository; acceptance mapping, execution, pass and freshness remain separate. Local agents use `wcode mcp-stdio` from the Host's project working directory; remote
clients prefer `/mcp`; older clients can use `/sse`. Plugin and one-command
Host setup are documented in
[Code Agent Integrations](../code-agent-integrations/).

The normal coding workflow is intentionally smaller:

```text
agent_context(goal, scopes=...)
    ↓ readiness / next_actions / parallelism
independent lanes ── concurrent top-level MCP calls
    ↓ real dependencies only
bounded edits → review_changes → verify_project
    ↓
deeper drift / risk / reconciliation / evidence only when needed
```

## 1. Install or update wcode

Install the latest release:

```bash
curl -fsSL https://raw.githubusercontent.com/francis-du/wcode/main/install.sh | sh
```

After `wcode update`, let the terminal or MCP Host start the updated executable on its next run. A client that cached `tools/list` may need to reconnect or refresh its MCP connection so it receives the new schemas.

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
2. follow readiness / next_actions / parallelism; run independent lanes concurrently at the Host level when supported
3. omit default/inferable MCP arguments; use semantic_navigation only when readiness recommends cross-file references/calls/implementations and keep find_symbol/search_code for simple localization
4. symbol_context only if more source is needed
5. apply_edits or apply_file_edits
6. review_changes
7. language_quality_run / drift / impact / risk only when the task requires them
8. verify_project + required advanced stages
9. evidence_status / reconciliation only when convergence or proof needs deeper inspection
```

`agent_context` uses a bounded adaptive budget when `budget` is omitted. It combines relevant Design State, scope-aware repo-map ranking, fresh semantic/runtime evidence when usable, bounded Hot Source, exact SHA edit targets, related tests, working-tree advisories, readiness, deterministic next actions, and an explicit parallelism strategy. Model-facing MCP calls should omit the default Workspace and server-default path/limit/timeout/budget values. The 1,000-token floor mode prioritizes direct editability; the default adaptive path can grow when the task is ambiguous or cross-module. `project_context`, `scope_status`, `design_status`, `traceability_status`, `software_context`, `language_quality_status`, and graph/risk tools remain available for deliberate deeper inspection rather than mandatory startup overhead.

### `agent_context`

Use `agent_context` as the normal coding entry point. It is designed to replace multiple startup discovery round trips with one bounded edit-ready pack. Repo-map ranking combines direct task relevance with existing Software Graph relationships; fresh semantic/runtime/deterministic evidence can strengthen those relationships, while stale provider facts automatically fall back to syntax. When the task language asks for callers, references, implementations, rename impact, or other cross-file relationships and the current graph is syntax-only, readiness recommends `semantic_navigation`; ordinary symbol localization does not pay that LSP cost. The pack keeps model-visible telemetry out of band in Tool Result `_meta` and reports explicit readiness instead of a generic quality score.

#### Task-aware repository retrieval

Repository localization is not treated as one universal ranking problem. `repo_map.routing` reports a versioned, bounded heuristic policy:

| Retrieval intent | Typical request | What gets extra bounded priority |
| --- | --- | --- |
| `trace_to_code` | “Where is `REQ-AUTH-001` implemented?” | Design-owned implementation paths and direct implementation/dependency relationships |
| `code_to_test` | “Which regression tests verify this symbol?” | Test files and test symbols related to the code target |
| `edit_to_ripple` | “What callers or files are affected by this rename?” | Direct graph relationships plus verified co-change history |
| `balanced_context` | General or mixed requests | The normal relevance/graph mix without specialized routing |

The router is `provider=query-intent-rules-v1`, `precision=heuristic`. It specializes only when one intent is clear. If test, traceability, and impact signals conflict in the same request, it deliberately abstains from specialization and returns `balanced_context` with the reason `ambiguous_retrieval_signals`; readiness exposes an advisory rather than inventing confidence.

Routing changes only weak priors. An exact literal or qualified symbol target remains stronger than recall-oriented retrieval seeds, and fresh semantic, deterministic, or runtime relationships retain their stronger provenance. Under a tight token budget, routing explanation is discarded before direct SHA edit targets, verification references, the strongest repo-map item, or diagnostic Hot Source. This keeps task awareness useful without letting metadata crowd out the material needed to edit safely.

### Task capability recommendations

`agent_context.capabilities` recommends a bounded tool set for the task and Execution phase. `recommended_actions` groups the actions; compacted packs retain ordered `recommended_tools` names when action metadata is omitted. A concentrated Jev capability-group choice may add safe read/advisory actions to the deterministic baseline. It cannot add write or command authority, remove deterministic work, or relax runtime checks.

The `metadata_first` disclosure policy is a Host recommendation. `dev.wcode/preloadRecommended` marks the small bootstrap set; a Host with deferred tool discovery can then load exact schemas for task recommendations and required explicit, safety, authorization, verification or recovery actions. Tools omitted from a recommendation remain callable through normal dispatch and authorization. A Host that ignores the hints still receives the same complete, deterministic `tools/list` catalog. Task changes do not mutate that catalog or emit list-changed notifications.

Catalog telemetry compares serialized bytes for the full catalog, preload set and proposed task selection. These are local size measurements, not evidence that the Host deferred schemas or reduced model tokens, cache misses or end-to-end latency. Actual Host behavior needs separate observation.

### Product Scopes

wcode has one canonical registry for its own product/control-plane boundaries: `runtime`, `integrations`, `workspace`, `design`, `graph`, `semantics`, `traceability`, `risk`, `verification`, `evidence`, `reconciliation`, and `experience`. `workspace_info` and `project_context` expose the registry. `scope_status` applies it to the selected repository and reports per-scope source counts plus bounded unmapped supported-source paths. `tools/list` attaches `dev.wcode/productScopes` to each Tool `_meta`, and MCP Resource clients can read `wcode://runtime/product-scopes`. The same live scope audit is surfaced through the Intelligence operator views.

These are not vendor names and they do not replace business/domain semantic scopes. Known Product Scope aliases are canonicalized; unknown scope strings remain valid freeform semantic scopes. For task retrieval, recognized scopes narrow source navigation to the registered source roots. For `semantic_query`, scoped facts must overlap a requested scope while unscoped facts stay global. The maintained mapping is documented in [product-scopes.md](../product-scopes/).

### `software_context`

Use it when the task starts from behavior or intent instead of a filename.

Example arguments:

```json
{
  "query": "workspace command security",
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

### LSP support for 22 languages

`semantic_provider_status` scans the workspace and reports provider availability for every language already supported by wcode's syntax index: Bash, C, C++, C#, CSS, Dart, Elixir, Go, HTML, Java, JavaScript, Lua, OCaml/interfaces, PHP, Python, R, Ruby, Rust, Swift, TypeScript, and TSX.

The current compatibility contract does not treat “a provider name exists in the registry” as support. Every one of the 22 indexed languages has exactly one canonical launch profile, provider-specific arguments are unit-tested, and every canonical profile is exercised through a real spawned stdio mock-LSP `initialize` handshake on the Rust test matrix. Only real alternates remain. Runtime availability is still separate from compatibility: wcode never claims a server is runnable when its executable is missing or when live `initialize` fails. `semantic_provider_status` exposes whether the selected candidate is canonical and how many installed candidates are available.

| Language | Canonical LSP launch profile | Installed alternate |
| --- | --- | --- |
| Bash | `bash-language-server start` | — |
| C / C++ | `clangd` | — |
| C# | `csharp-ls` | — |
| CSS | `vscode-css-language-server --stdio` | — |
| Dart | `dart language-server --client-id wcode --client-version <version>` | — |
| Elixir | ElixirLS `language_server.sh` / `language_server.bat` (`elixir-ls` distro wrapper also recognized) | — |
| Go | `gopls serve` | — |
| HTML | `vscode-html-language-server --stdio` | — |
| Java | `jdtls -data <per-workspace-state-dir>` | — |
| JavaScript / TypeScript / TSX | `typescript-language-server --stdio` | — |
| Lua | `lua-language-server` | — |
| OCaml / interface | `ocamllsp` | — |
| PHP | `phpactor language-server` | `intelephense --stdio` |
| Python | `pyright-langserver --stdio` | `pylsp` |
| R | `R --vanilla --no-echo -e languageserver::run()` | — |
| Ruby | `ruby-lsp` | `solargraph stdio` |
| Rust | `rust-analyzer` | — |
| Swift | `sourcekit-lsp` | — |

For providers with an alternate, both foreground navigation and manual semantic refresh can recover from a canonical provider that is installed but fails initialization. The alternate crosses its own normal trust boundary and successful refreshes report the switch in `fallbacks`; wcode never silently broadens one provider grant into another.

The runtime automatically maintains only providers that have an explicit automatic hardening profile; currently `rust-analyzer` is that automatic profile. It watches only the most-specific discovered project Workspaces, waits for a short stable-source window before refreshing, retries failures with bounded exponential backoff, and acquires the same global Harness semaphore as model-facing work. This prevents a broad root and its nested project subspaces from launching duplicate semantic indexing.

The Harness owns a bounded warm session pool keyed by Workspace, LSP server, and current binary identity; a live session is reused by both background indexing and foreground navigation. The coordinator periodically prunes idle unleased slots, capacity eviction never removes a leased slot, and an all-busy pool fails closed instead of temporarily exceeding its process bound. LSP binary replacement also waits for the active lease to finish before the old slot is dropped and a new one can start. `semantic_provider_refresh` remains available as a force-refresh surface.

Within a warm session, document synchronization follows the server's advertised `textDocumentSync` contract. Numeric Full/Incremental compatibility capabilities open with full content; detailed options honor `openClose`; Full changes send the whole document, Incremental changes send a valid replacement range in the negotiated UTF-8/UTF-16/UTF-32 position encoding, and None leaves disk-backed content to the server instead of sending an unsupported change. `didClose` is sent only when the server requested open/close synchronization. A refresh requests real hierarchical Document Symbols and, when supported, imports a bounded set of high-value Call Hierarchy / Implementation relationships rather than expanding every variable and field. Successful first-party nodes carry `source_sha256`; provider status therefore reports `fresh` / `stale`, and stale LSP revisions are excluded from graph overlays, impact, reconciliation, and `software_context.graph_context`. The graph revision key still comes from source hashes, provider executable metadata, and the symbol bound: unchanged inputs skip graph reconstruction, while a runtime may warm one provider session so later semantic queries do not pay startup cost. A server that returns no semantic symbols does not create a fake semantic revision.

Automatic execution is intentionally LSP-server-specific rather than a general LSP exemption. The current `rust-analyzer` profile rejects an executable that resolves inside the Workspace, scrubs credential and execution-injection environment variables, and sends initialization options that disable build scripts, proc macros, Cargo auto-reload, and check-on-save. This reduces the default execution surface but is not an OS sandbox: LSP servers can still parse project metadata and configuration. `--no-semantic` is the fail-closed opt-out. A detected LSP server without an automatic safety profile requires a `RiskyExecution` grant bound to Workspace + server + current binary identity; refresh and navigation may reuse that exact warm session, while replacing the executable invalidates the old grant. Process-wide `--allow-risky-exec` remains the deliberate broader trust path.

When no trusted/installed provider can run, Tree-sitter remains available as `precision=syntax`. External SCIP/compiler/runtime indexers can still use `graph_provider_import`; the first-party LSP registry supplements rather than replaces the provider-neutral import contract.

### `semantic_navigation`

Use `semantic_navigation` for relationship questions where repository text search is incomplete and for guarded LSP mutation planning. Prefer `path + symbol` for symbol relationships and rename; wcode resolves the symbol through its syntax index and converts its 1-based UTF-8 byte position into the position encoding negotiated with the LSP server. A direct `line + character` selector remains available for callers that already own a precise source position. `intent=rename_plan` plus `new_name` asks the live LSP for the complete WorkspaceEdit and returns SHA-pinned `apply_file_edits` arguments. `intent=organize_imports_plan` accepts only one edit-only action for the current file. `intent=quick_fix_plan` requires version-matched bounded `publishDiagnostics` at the selected position and refuses to guess when multiple safe fixes lack one unique preferred action. Provider commands are never executed; resource operations, external or multi-file code-action edits, stale or partial plans, sensitive guarded source and ambiguous edits all fail closed. Semantic mutation intents have no syntax fallback and never write automatically.

The `intent` controls which LSP requests are issued: `definition`, `hover`, `references`, `incoming_calls`, `outgoing_calls`, `calls`, `implementations`, `impact`, `rename_plan`, `organize_imports_plan`, or `quick_fix_plan`. `impact` deliberately favors cross-file completeness—references, incoming callers, and implementations—rather than issuing every supported request. The result distinguishes `unsupported` capabilities from `failures`: an empty relationship list means a supported request completed with no matching relationships, while an LSP timeout/error is surfaced separately and is never presented as negative semantic evidence. If no trusted LSP server is available, the tool returns `precision=syntax` with `routing=tree_sitter_fallback` instead of pretending semantic precision. For simple “where is this symbol?” work, continue to use `find_symbol` / `search_code`; this keeps LSP cost proportional to the tasks that benefit from semantic completeness.

The TUI Intelligence view separates installed `available`, policy/trust `launch-ready`, live-initialized `validated`, and final runnable/fresh state, then exposes warm session count, synchronized document count, pending authorization and missing-server counts, provider starts, and fresh/stale state so operators can tell whether session reuse is actually working.

### Language Quality Matrix

Language support is a capability vector, not a checkbox. `language_quality_status` reuses the same 22-language surface and reports syntax, semantic, format, lint, type-check, static-analysis, test, security, Property, Mutation, Fuzz, and Runtime-Canary coverage separately. Semantic counts as covered only after a real LSP initialization; executable discovery alone is not enough. Repository manifests/configuration and package scripts define intent, but only declared + available + check-only + runnable quality providers satisfy the matrix. Discovery-only scripts stay visible without manufacturing green coverage. One real provider can expose bounded secondary `covers` dimensions so a Dart analyzer or compiler/build check is not executed twice merely to fill two columns. Missing executables and missing dimensions remain explicit gaps.

`language_quality_run` accepts one provider returned by the matrix and runs it only when the language is detected and the provider is repository-declared, available, runnable, and registered as check-only. Exact approved shapes reuse the autonomous verification/development lane; other registered check-only providers use the bounded trusted-runtime lane rather than inventing a separate approval workflow. Formatter/fixer write modes are intentionally absent. The result is converted to a Verification Report and persisted as current code+design revision Evidence, so a historical green run never proves a later revision.

Current provider families include native/check-mode Rust, Go, pure Dart/Flutter, Deno, .NET, Maven/Gradle, Mix, Dune and SwiftPM flows plus repository-declared Ruff/mypy/Pyright/Bandit, Prettier/Biome/ESLint/Stylelint/HTMLHint/TypeScript, clang-format/clang-tidy, Checkstyle/SpotBugs/Spotless, Credo/Dialyzer, ShellCheck/shfmt, StyLua/Luacheck, PHPStan/Psalm/PHPUnit/locked Composer advisory audit, R styler/lintr/testthat, Ruby RuboCop/Standard/RSpec, and swift-format/SwiftLint. Scoped npm packages are matched by their literal package key rather than JSON-pointer paths; CSS/HTML plugins and Biome opt-ins must be explicit before those languages turn green. Deno dependency checks are frozen, Composer security audit is locked and may consult the project's configured advisory repositories, R inline checks use `Rscript --vanilla`, and RuboCop lint is not duplicated as fake format coverage. This describes registry capability, not host installation. See [language-quality.md](../language-quality/).

### Graph history and diff

`software_graph` persists deduplicated meaningful graph snapshots. `graph_history` lists them, `graph_query` reads one revision or neighborhood, and `graph_diff` compares two revisions (or the latest two by default). Diff aligns nodes by stable node ID and edges by `from + to + kind + provider + precision`; a provenance-revision/attribute change is reported as `changed` rather than noisy delete/add churn. Repeated stable edge identities are compared as revision multisets, so future richer SCIP/runtime providers do not lose duplicate relationships. The Engineering Observatory uses this history for its architecture-revision timeline and latest Node/Edge `+ / - / ~` delta, while its feature architecture is regenerated from the current repository on refresh.

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

`review_changes` also reports deterministic maintainability signals. `maintainability-file-crossed-1k` explains a change that would cross the 1,000-line maintained-source boundary; `maintainability-oversized-source-growth` flags continued growth in an already oversized file; `maintainability-concentrated-growth` marks at least 400 net new lines in one source file; and `maintainability-cross-scope-churn` marks at least 1,000 changed source lines across three or more Product Scopes. Review signals remain separate from design judgment, but the 1,000-line maintained-source boundary itself is a wcode core policy: Workspace mutation tools block new/crossing/growing violations and `verify_project` runs a fail-fast `core-policy` Convention check before repository-native commands. Existing oversized files may only stay the same size or shrink while being semantically decomposed. Generated/vendor/build output is exempt. See [maintainability-review.md](../maintainability-review/).

## 4. Verification Mesh

`verification_plan` converts the current risk level into a verification policy and creates blind independent reviewer jobs.

The deterministic level is currently mapped to the existing Harness:

- low risk → `quick`
- medium/high/critical risk → `full`

The plan can also require Property, Mutation, Fuzz, Runtime/Canary evidence, adversarial review, or human approval. Plans at medium risk or higher include a blind `maintainability` reviewer job in addition to correctness; that job requires `maintainability_review` and carries structural guidance around deleting complexity, avoiding scattered special cases, keeping canonical ownership, questioning 1,000-line threshold crossings, and simplifying orchestration. A correctness Pass cannot substitute for the maintainability job.

Verification Plan/Job state is persisted per workspace, so another wcode process or model executor can resume queued/claimed work. `verification_executor_status` reports the cross-language runner registry and whether each executable is actually available. `verification_execute_stages` runs every applicable available runner for a required stage (skipping only a producer whose latest Evidence already passes) and converts each real command result into persistent Stage Evidence; one runner failure is not hidden by another runner's later success. `verification_stage_submit` remains the provider-neutral adapter for CI/external systems. `verification_status` keeps the latest result per producer and aggregates fail-closed as `Fail > Disagree > Inconclusive > Pass`; a Plan also becomes stale when the workspace code revision changes after plan creation.

Built-in discovery recognizes common ecosystems such as proptest/quickcheck/cargo-fuzz/cargo-mutants, Go property/fuzz tests, Hypothesis/mutmut, fast-check with fixed Vitest/Jest runners, jqwik/PIT, FsCheck/.NET Stryker, SwiftCheck/Muter, StreamData, Glados, Rantly, Eris/Infection, QCheck, and R quickcheck. Property adapters require both framework declaration and matching-language source usage; a dependency name alone is not proof that a Property suite exists. Arbitrary package scripts are never relabeled as Property or Mutation evidence. JS/TS Stryker remains an explicit `.wcode/executors.yaml` decision because Stryker configuration can execute repository JavaScript. Those integrations are convenience adapters, not a closed list.

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

Configured executors run without a shell, remain workspace-scoped, hide configured arguments from status/UI serialization, and scrub sensitive environment/output. Workspace-relative programs are resolved through the same canonical-root and symlink protections as other workspace operations. Property/Mutation/Fuzz/Runtime executors are part of the hardened autonomous local development lane: they use bounded cwd, process count, output and timeout and do not create repetitive RuntimeExecutor authorization requests. A missing executable is reported as unavailable/missing; it never produces pass Evidence.

### MCP 2026 long-running Tasks

For MCP `2026-07-28`, wcode advertises the official `io.modelcontextprotocol/tasks` extension. The extension remains per-request opt-in: a client must include it in `_meta.io.modelcontextprotocol/clientCapabilities.extensions` on the request that wants Task behavior. Long semantic setup/refresh, verification-stage execution, and `verify_project` are task-augmented. `run_command` is conditional: it becomes a Task only when the caller explicitly sets `task_mode=true`; ordinary command calls remain synchronous even for task-capable clients. A task-mode command sent by a client/protocol without Tasks support fails instead of silently becoming a blocking request.

A Task is persisted before its handle is returned, scoped to a SHA-256 fingerprint of the authenticated OAuth `client_id` (never the raw bearer token), and bounded per workspace. `tasks/get` polls status and returns the original Tool Result after completion; while a task-mode command is still working, its private `dev.wcode/liveCommandOutput` metadata contains only a redacted 32 KiB-per-stream **tail of a separate bounded progress stream**, not the whole accumulated output on every poll. Pipe readers continue draining after the final Tool Result's 256 KiB-per-stream prefix capture saturates, but progress crosses a bounded channel and the Task worker redacts complete lines before tailing; an overlong pending line is fail-closed as redacted instead of retaining unbounded memory or dropping a sensitive key prefix before its value. The metadata distinguishes final-result capture truncation from sanitized bytes dropped only to form the current live tail. The completed Tool Result keeps the existing bounded-prefix capture contract unchanged. `tasks/update` is currently ack-only because these tools do not issue input requests; `tasks/cancel` persists `cancelled` before aborting the worker so a late completion cannot overwrite cancellation. For `run_command`, worker cancellation owns the same supervised child/process-tree lifetime as a synchronous call, so stopping the Task terminates a bounded dev server instead of detaching it. Command policy, authorization, sandboxing, cwd/resource/output bounds and the 1800-second command ceiling are unchanged. Terminal tasks may be reclaimed only when capacity is needed; active tasks are never evicted to make space. If a runtime process is replaced while a Task is still working, the next read marks it failed rather than pretending the worker survived the restart.

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

Durable repository-intelligence and engineering-state data lives in wcode's user-level state directory, keyed by the canonical workspace root. Evidence uses bounded immutable records; Verification uses immutable Plan/Job snapshots; Semantic Facts use immutable revisions; Graph Provider facts and composite Software Graph snapshots retain bounded history; Reconciliation Plans/execution and MCP Task snapshots are stored independently. None of these stores modify the repository or require the workspace to be writable.

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
- `read_media` — metadata-first bounded media inspection; `include_content=true` explicitly opts into standard MCP image/audio content blocks, while video remains metadata-only
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
- first-party LSP support for all 22 indexed languages with real LSP Document Symbol, Call Hierarchy, and Implementation ingestion, source-hash freshness/stale exclusion, and revision-cache reuse when semantic inputs have not changed;
- Requirement → Component → implementation/test traceability;
- Git-aware drift, graph-aware transitive impact, structured risk, and deterministic maintainability review findings for 1,000-line threshold crossings, concentrated source growth, and cross-Product-Scope churn;
- risk-adaptive Verification Plans with persistent blind reviewer Plan/Job state; plans at medium risk or higher include a dedicated `maintainability_review` job with structural simplification guidance, while reviewer disagreement Evidence, per-producer fail-closed stage aggregation, HumanApproval Evidence, Verification history, and stale-workspace-revision protection remain explicit;
- cross-language Property / Mutation / Fuzz / Runtime-Canary execution through built-in ecosystem discovery plus `.wcode/executors.yaml`; every applicable available runner executes unless that producer already has passing Evidence, while external `verification_stage_submit` remains available for CI/provider integrations;
- MCP `2026-07-28` task augmentation for `semantic_provider_refresh` and `verification_execute_stages`, with durable-before-handle storage, OAuth-client scoping, polling, bounded cancellation, and synchronous fallback for clients that do not opt into the extension;
- persistent Reconciliation Plans plus dependency-aware claim/submit/retry execution state and reconciliation Evidence;
- local `wcode intelligence --refresh-semantic` / `wcode verification --execute-stages` CLI flows in addition to read-only status views;
- live TUI repository-intelligence overlay (`I`), complete command catalog (`C`), persistent pairing code, and protected Engineering Observatory (`W`) with a System → Subsystem → Component blueprint, live Understand → Change → Prove → Learn → Observe activity, Files → Components → Requirements → Verification → Drift change story, secondary Design-vs-Actual dependency graph, Component Inspector, Requirement drill-down, verification, ADR/constraint context, bounded project tree, code statistics, mapped Git changes, risk, and architecture revision history;
- MCP exposure of the complete higher-level runtime.

Precision and integration boundaries are explicit rather than hidden:

- the always-available code index remains Tree-sitter `precision=syntax`; a first-party LSP adapter may upgrade individual facts to `precision=semantic` only after a real installed server responds, while SCIP/compiler/runtime providers can still enter through the external import contract;
- all 22 indexed languages share one LSP and verification-executor architecture, but wcode does not bundle every third-party LSP/test binary. `semantic_provider_status` and `verification_executor_status` expose exact host availability instead of pretending absent tools exist;
- hardened first-party LSP servers may auto-refresh through the bounded LSP lane and can be disabled with `--no-semantic`; non-profiled LSP refresh plus Property/Mutation/Fuzz/Runtime execution still require explicit operator trust, while `--allow-risky-exec` remains process-wide pre-authorization; none of these mechanisms is an OS sandbox;
- model-facing command execution uses command-specific policy for the built-in development CLI catalog and exact `RiskyExecution` fingerprints for bounded repository/remote operations. Repository mutation stays narrower: only explicit-path `git add`, message-only `git commit`, and explicit remote+ref non-force `git push` shapes can cross exact approval; an approved SSH push may use the current SSH Agent only through wcode's fixed non-interactive SSH command. Force/delete/reset/restore-style mutation, shell interpreters, credential-bypass surfaces, workspace escapes, and protected resources remain blocked;
- `read_media` never infers capability from a model or vendor name. Metadata is the default; `include_content=true` is the explicit caller opt-in that emits a standard MCP image/audio content block without requiring a private extension;
- Reconciliation execution coordinates durable tasks and evidence, but source edits still use the normal bounded/hash-guarded wcode edit surface instead of a hidden unrestricted patch engine;
- destructive deletion is deliberately outside normal write flow: the first `delete_path` attempt creates an exact local authorization request, the operator approves or denies it in the TUI or protected WebUI, and only a matching retry can consume the one-shot grant.

## 8. Dogfood this repository

The wcode repository already contains `.wcode/project.yaml` and `.wcode/design/*.yaml`, so after installing/restarting the current build you can ask a connected agent:

> Use `agent_context` for the requested wcode change. Follow its readiness/next actions, edit through guarded Workspace tools, run `review_changes` and `verify_project`, then use drift/risk/reconciliation/evidence tools only if the task still needs deeper convergence analysis.

That exercises the implemented repository-intelligence and Engineering Control Plane path end to end without requiring a separate demo project.
