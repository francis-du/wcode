---
layout: docs
title: IDE independent engineering observability
description: Official product research, current workbench scope, and the remaining observation gaps.
lang: en
alternate: /zh/docs/ide-independent-observatory/
permalink: /docs/ide-independent-observatory/
---

# Observe engineering work without an IDE

wcode should let an operator understand the repository, inspect agent activity, locate problems, review changes, and assess proof without opening an IDE. The first useful improvement is to connect existing information to its source and next action. More status cards alone cannot provide that workflow.

This 2026-09-30 review compares representative official documentation from editors, Git tools, CI systems, terminal workbenches, and observability platforms. It is a capability survey, not an exhaustive review of every product, benchmark, or a claim that wcode already replaces every IDE function.

## What the official products establish

The product behavior below comes from the linked primary sources. The proposed adaptations are wcode design choices, not measured improvements inherited from those products.

| Official source | Observed behavior | Useful adaptation for wcode |
| --- | --- | --- |
| [VS Code code navigation](https://code.visualstudio.com/docs/editing/editingevolved) | Quick file and symbol navigation, breadcrumbs, definition/reference previews, and a Problems list connected to source. | Preserve context while moving from an engineering signal to a file, symbol, and exact line. Keep syntax and live semantic precision explicit. |
| [JetBrains Problems](https://www.jetbrains.com/help/idea/problems-tool-window.html) | Severity filtering, source jumps, and a preview of the selected issue in context. | One selectable attention list should expose cause, producer, observation time, and a relevant destination. |
| [VS Code testing](https://code.visualstudio.com/docs/debugtest/testing) | Framework-dependent discovery, test status, detailed output, source navigation, and coverage. | Distinguish discovered/mapped tests from executed results. Introduce per-test results only when a real runner adapter supplies them. |
| [VS Code terminal shell integration](https://code.visualstudio.com/docs/terminal/shell-integration) | Working directory and command detection, command output boundaries, exit status, and output navigation. | A command inspector needs program, arguments, cwd, queue time, runtime, exit/timeout state, and bounded redacted output. |
| [Zed agent review](https://zed.dev/docs/ai/agent-panel#reviewing-changes) and [Lazygit](https://github.com/jesseduffield/lazygit#features) | Zed supports review by changed file and hunk. Lazygit provides line/hunk selection, filtering, and commit comparison. | Reuse wcode's existing comparison layers and snapshot-bound source/diff inspection. Mutating Git actions require their own existing policy checks. |
| [GitHub status checks](https://docs.github.com/en/pull-requests/reference/status-checks) and [workflow logs](https://docs.github.com/en/actions/how-tos/monitor-workflows/use-workflow-run-logs) | Checks distinguish lifecycle status from final conclusion, annotate source, and expose searchable job/step logs. | Bind any future CI/PR projection to repository, commit, run attempt, producer, and platform. A skipped check is distinct from an executed pass. |
| [OpenTelemetry log correlation](https://opentelemetry.io/docs/specs/otel/logs/) and [Grafana trace navigation](https://grafana.com/docs/grafana/latest/visualizations/explore/trace-integration/) | Time, trace/span identity, and resource identity connect signals; a span can open related logs. | Correlate execution, tool task, command, verification, and evidence identities before building a richer timeline. External application telemetry needs a separate ingest contract. |
| [K9s commands](https://k9scli.io/topics/commands/) | Keyboard navigation, context switching, resource filtering, logs, and descriptions are discoverable from the workbench. | Keep TUI selection, filtering, details, and shortcuts consistent across attention, tasks, and provider views. |

## Market tradeoffs for agents and tokens

| Primary source | Confirmed behavior | Principle adopted here |
| --- | --- | --- |
| [Claude Code Agent teams](https://code.claude.com/docs/en/agent-teams) | Independent sessions coordinate through shared tasks and claims; extra contexts and coordination increase token use. | Parallelize substantial independent work; serialize shared-file edits and hand off scoped revisions and concise results. |
| [OpenCode Agents](https://opencode.ai/v2/docs/agents) | Primary agents delegate to child sessions with their own permissions and model configuration. | The Host spawns models; wcode coordinates leased work without granting file permissions. |
| [Cursor Worktrees](https://prod.cursor.com/docs/configuration/worktrees) and [Cloud Agents](https://cursor.com/docs/cloud-agent) | Separate checkouts or remote VMs isolate files and execution and produce reviewable artifacts. | Shared workspaces use non-overlapping scopes; use existing Host worktrees when environment isolation is necessary and verify the combined revision. |
| [OpenAI prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching) | Caches reuse prompt prefixes; effectiveness depends on model, requests, and measured cache accounting. | Keep the tool catalog stable and progressively disclose task routing. Fewer bytes do not establish billing or cache benefits. |

`worklist_claim` and `worklist_submit` expose canonical model entry points. Leases, dependencies, current revisions and scope conflicts protect coordination; the lead independently verifies the combined result. Higher parallelism and lower total token use are separate measurement goals.

Claim, lease renewal, and submit acknowledgements include a Worklist summary with `items_included: false`, current revision, status counts, and runnable lanes. The claimed item remains in `handoff.item`; a submitted result remains in `result`. Read `worklist_status` for the complete retained item list (`items_included: true`). This avoids repeating unrelated history in each lane acknowledgement without discarding stored tasks or changing ownership and proof rules. Scoped handoffs remove global Worklist and Execution progress before context budgeting and action planning, preventing unrelated lanes from demanding duplicate parallel work. Pending directives, verification floors, and replan requirements remain intact; ordinary coordinator Agent Context still discovers the Worklist. Handoff context byte counts and byte/4 token estimates are recomputed after filtering. Serialized response bytes are a local measurement, not provider token or billing savings.

## Existing foundation

The current Observatory already exposes architecture and Code Graph inspection, task activity, durable Execution, verification evidence, change impact, requirements, language-provider coverage, and a bounded file tree. It also has a command palette and project navigator. The TUI already shows runtime/endpoint state, workspaces, resource pressure, task activity, engineering summaries, and authorization.

[Change inspection](../change-inspection/) already supplies read-only working, staged, and unstaged comparisons, before/after syntax mapping, changed-line navigation, and protected source windows from graph nodes. Source identity, repository revision, precision, redaction, and truncation remain part of those views. The file-browser bridge reuses these boundaries for protected source navigation.

These are different kinds of observations. Architecture declarations are intent; syntax relationships are syntax; live provider results may supply semantics; check output is execution data; revision-bound Evidence supplies proof under its own rules. Showing them together must preserve those distinctions.

### Architecture evidence and bounded observation

A resolved implementation mapping establishes where a Design reference points in source. An observed dependency establishes a retained relationship between source endpoints. Executed verification has its own revision and result. These are independent observations: a mapped implementation is not a passing test, and an unobserved declared dependency does not mean the capability is unimplemented.

The Observatory keeps its existing 5,000 source-symbol budget. Design implementation Symbol references are resolved in per-file batches so exact entry definitions receive priority over unrelated declarations. Existing requested callers retain priority; directly reachable one-hop call endpoints can use remaining capacity only under the existing resolver rules. Other definitions fill the remaining budget across files. More Design references do not increase the cap. Unresolved references, ambiguous homonyms and receivers with unknown types never justify invented call edges or stronger precision.

Component ownership comes from exact Symbol references, explicit File references and their resolved trace mappings. A Symbol reference matches qualified names before short names and does not claim unrelated sibling definitions. A direct match remains uncertain when names are ambiguous, the graph/scan is truncated, or the canonical trace lookup is explicitly unresolved. A single component ID backed only by that uncertain match is not a confirmed owner. An explicit File reference can cover the file's nodes; a resolved file trace binds only its File node, while a uniquely resolved symbol trace independently confirms its exact definition. Only endpoints with one unambiguous component owner each, belonging to different components, can contribute an actual dependency, and only at syntax, semantic, deterministic or runtime precision. Syntax remains syntax; the existing stronger-precision rule for blocking drift remains separate. Ambiguous ownership, one-sided ownership and declared/heuristic/mixed relationships remain candidates. They do not count as actual, aligned or blocking, and owner combinations are never expanded into invented component pairs. Edges with no Design owner at either endpoint are outside this candidate projection.

The source inspector preserves original endpoint node IDs, paths and symbols, relation kind, provider, precision and revision. Owner explanations distinguish `symbol_reference`, `ambiguous_symbol_reference`, `file_reference`, `trace_symbol` and `trace_file`, with explicit unique/ambiguous/unowned state. Each confirmed component pair retains at most 8 raw edge records, alongside `evidence_total` and `evidence_truncated`. The architecture view retains at most 128 `dependency_candidates`, with `dependency_candidates_total` and `dependency_candidates_truncated`; each endpoint retains at most 16 owner samples and its full owner count. These totals describe the bounded observed input, not every dependency in the repository.

Inspect the raw source and ownership explanation before treating a candidate as an architectural fact. Missing or truncated evidence stays explicit. Older snapshots without the new evidence fields remain readable, but missing fields cannot supply original-edge evidence or establish complete observation. Neither graph inspection nor a resolved test mapping records an executed check or supplies acceptance.

## This implementation slice

The following changes are implemented in this working-tree slice. Verification is underway; this document does not attest that full checks, runtime acceptance, or publication have completed.

| Surface | Implemented change | Boundary |
| --- | --- | --- |
| Shared project state | A bounded Problems/attention projection of existing policy, drift, evidence, verification, provider, and structural signals. | The projection describes its source, revision/freshness, and incomplete coverage. It cannot manufacture compiler diagnostics or proof. |
| TUI | Summary, Attention, Tasks, Agents, and Providers views provide selectable rows and contextual details. | Selection must remain bound to the current workspace and row identity as live data changes. Hidden or narrow layouts must retain readable controls. |
| WebUI | An actionable observation coverage strip and selectable attention items route operators to the relevant existing workbench view. | Navigation does not execute checks, approve requests, or mark work complete. |
| Project files | Interactive file-tree and largest-file entries open protected source inspection, with 240-line paging and snapshot/SHA checks. | Source retrieval remains bounded and revision-aware; protected, missing, stale, redacted, or binary content stays explicit. |
| macOS Menu Bar | `wcode menu-bar` observes the same protected local runtime-presence contract published by the HTTP/MCP runtime and `wcode mcp-stdio`. `wcode menu-bar --json` exposes the portable bounded projection. | The first slice is observation-only: runtime/MCP/task counts and partial/unknown state. It stores no UI/OAuth token, owner, command arguments, source path, or raw diagnostic, and it cannot approve, execute, cancel, or establish verification/Acceptance. |

The operator path is: choose a workspace → select an attention item → inspect its details → open the relevant source, activity, provider, changes, or proof view → resolve the cause through the agent or an authorized operation → inspect fresh proof. TUI, WebUI, and the Menu Bar can use different layouts but should describe the same underlying runtime truth.

The Menu Bar helper is a separate accessory UI process rather than a second wcode daemon or a normal Dock application. Its native status item is created only after the macOS main-thread event loop is active. HTTP/MCP and stdio processes publish short-lived heartbeats under the protected local authority state root; records older than the active window are not treated as live. The menu shows transport counts, the latest runtime version and Workspace IDs, recent MCP activity, task/queue counts, and explicit coverage state. Corrupt, aliased, excessive, or incomplete observations remain partial or unknown. On non-macOS systems, the native tray is not implemented yet, but `wcode menu-bar --json` keeps the status contract portable for a future Windows/Linux tray.

## Capability audit and ablation decisions (2026-10-31)

This source audit separates implemented behavior, proposed additions and external acceptance. Live web search was unavailable during this iteration, so the older primary-source review above was not reverified as current research. Source mappings establish where code or tests exist; they do not establish executed checks or customer readiness.

| Capability | Source-level finding | Decision |
| --- | --- | --- |
| Engineering work queue | The canonical Worklist already owns task state, dependencies and leases. The former eight-row ownership view could hide unfinished work behind completed reports. | Extend the existing read-only projection with open-task priority, status/text filters, paging, notes and dependency context. Do not create another task store. |
| Team evidence inspection | wteam already receives project, revision, runner, source digest, observed time and received time. Its earlier table hid the runner, source digest and received time. | Expose project/revision groups and source details in the existing console. Different digests are different contexts, not an automatic failure or approval. |
| Command job inspection | `app/jobs.js` and `web_jobs.rs` already provide retained task discovery, status/origin filters, exact-task output and UI-owned cancellation. | Keep this implementation; this iteration adds local output search and retained-line location. Further producer/timing metadata remains driven by concrete debugging needs. |
| Verified experience | `evidence/experience.rs` already contains normalized co-change retrieval and a temporal comparison against raw-count history. | Evaluate the existing mechanism before adding vector storage, full transcripts or another summarization model. |
| Multi-agent coordination | `worklist_delegation.rs` already implements scoped leases, dependencies and reported results; the Host creates model sessions. | Keep coordination and independent verification. Do not add a second scheduler or an always-on model swarm without a demonstrated gap. |
| Optional decision assistance | `intelligence/jev.rs` and `jev_policy.rs` already implement optional advisory assistance and deterministic fallback. | Keep it optional. No additional default planner/model calls; wider use requires controlled task-quality and real-cost measurements. |
| GitHub acceptance integration | The native publisher, candidate watcher, enrollment and inbox exist under `integrations/git/`. | Preserve the integration. Real App installation, isolated deployment and customer PR acceptance stay external prerequisites. A generic CI log explorer is a separate proposal. |
| Application telemetry ingestion | Process logging and runtime health do not establish application OpenTelemetry/OTLP ingestion. | Defer until a real resource, collection and retention contract is available; first connect existing execution/task/evidence identities. |
| General debugger and interactive terminal | No complete DAP breakpoint/stack workflow or interactive PTY contract is established by this product slice. | Defer. Engineering management does not require rebuilding a general IDE in this iteration. |

The work queue resolves dependencies against the full observed list before applying filters and eight-row paging. Missing, malformed and partial dependencies remain explicit. Dependency satisfaction does not grant runnable authority. Current runnable labels require the backend's reported IDs and a fresh, complete observation. Worker completion remains a report, not verification. Workspace/owner changes reset local filters, expired leases are recomputed on lightweight revision polls, and background updates preserve search focus and Chinese input composition.

The isolated diagnostic `node tests/unit/ui/worklist_ablation.cjs .` loads the production bundle into separate VMs and changes one feature path at a time without editing production files:

| Removed feature | Baseline | Isolated ablation | Restored |
| --- | --- | --- | --- |
| Unfinished-work priority | 2/2 open tasks visible behind eight completed records | 0/2 visible | 2/2 visible |
| Full-observation dependency index | Hidden completed prerequisite resolves | Prerequisite incorrectly appears missing | Resolves |
| Workspace/owner/epoch reset | 3/3 identity boundaries reset the filter | 0/3 reset | 3/3 reset |
| Stale/partial/historical suppression | 3/3 uncertain observations suppress current runnable labels | 0/3 suppress them | 3/3 suppress them |

These are reproducible synthetic behavior comparisons, not a model benchmark or a claim about time, token cost or overall engineering productivity. Browser interactions and project checks remain separate verification obligations. The new console projections add no model calls, dependencies, execution permissions or automatic task transitions.

### Retained output location (second iteration)

The existing Jobs inspector now searches retained output locally: at most 256 query characters, case-sensitive literal matching, a stdout/stderr selector and at most 25 matching retained lines per page. Line numbers belong to each retained fragment; the streams do not form a timeline. Truncation and redaction remain explicit, including when no retained text matches. Clearing the search restores the original output blocks; search neither fetches additional output nor replays a command.

Search state binds the exact task, Workspace, session owner and epoch. Selecting another task withdraws old output before the response arrives; failed or invalid detail clears old matches. Same-task updates preserve the query, valid pagination, focus and Chinese composition. `tests/unit/ui/job_output.cjs` verifies these behaviors and separately ablates literal matching, identity boundaries and coverage explanations inside isolated VMs: matching lines 2/5→incorrect 5/5→restored 2/5, task/owner boundaries 2/2→0/2→2/2, coverage statements 4/4→0/4→4/4. These are functional comparisons, with no measured latency or model-cost claim.

### Primary-source research refresh (2026-10-30 UTC)

Search became available after the initial source audit. The following primary sources were rechecked; this is a bounded review, not an exhaustive claim about the newest research.

- [AgentRewind (2026-08-14)](https://arxiv.org/html/2608.14380v1) evaluates aligned context/workspace recovery and component ablations. Its recovery excludes external service effects and has filesystem/history limits. The engineering implication here is to retain exact task, input and failure context; a shared dirty workspace is not a basis for adding unrestricted rollback.
- [SWE-Marathon (2026-06-05)](https://arxiv.org/html/2606.07682v1) studies long-horizon software work with independent verification layers. Its failure analysis and stated evaluation cost support real behavior checks and exact run provenance. Its diagnostic subset is not a universal failure-rate estimate, and the full benchmark is not a per-edit development gate.
- [OpenAI's coding-evaluation audit (2026-07-08)](https://openai.com/index/separating-signal-from-noise-coding-evaluations/) identifies both overly narrow and incomplete tests in SWE-Bench Pro. Test repairs should preserve the product contract: the static-asset check now compares the complete compiled script, and isolated runtime fixtures provide the renderer dependencies used by production.

These sources inform prioritization; they do not measure an improvement in wcode or wteam. Continue with bounded projections over existing facts, actual CI evaluation identity and independent checks. Additional planners, autonomous retry loops, general rollback and new telemetry stores remain deferred until a concrete requirement and a reproducible comparison justify them.

## Inspect design, implementation and command output

The current source connects two existing observation paths:

| Starting point | Action | What opens |
| --- | --- | --- |
| Requirements → implementation or acceptance-test reference | **Inspect mapped file** | The mapped file in the protected Code Graph source inspector, bound to the observed code and Design revision. |
| Architecture → component implementation or observed changed file | **Inspect mapped file** | The exact observed file through the same snapshot/SHA checks. |
| Activity → a `run_command` row with a current durable job binding | **Inspect output** | That exact retained task, including origin, lifecycle, reported outcome and bounded stdout/stderr. |
| Activity → Command jobs | Select a retained task, then search or choose an output stream | Local inspection of the already returned output; no command is launched or rerun. |

Resolved design mappings can provide a file path even when it is outside the separately bounded file-tree response. Extensionless files such as `Dockerfile` and `Makefile` keep their exact identity. An unavailable source action means this observation cannot supply the path, not that the file is absent. Source replies from an obsolete workspace, selection or revision cannot replace the active inspection. Opening an acceptance-test file does not execute the test or establish evidence.

The Jobs list and each output stream scroll independently. Search uses literal matching, including punctuation and non-English text; the **Match case** option can be turned off for case-insensitive matching; select **stdout**, **stderr**, or both. Matching line numbers are relative to the retained window. Matching results are paged across both streams, at most 25 lines per page. The existing 32 KiB per-stream retention bound and redaction still apply, and truncation/redaction notices remain visible when a stream is filtered out. No match in retained output says nothing about omitted logs.

Refreshing the same task preserves the reading position, expanded/collapsed streams and keyboard focus. **Jump to end** is explicit; polling does not pull the reader away from older output. Search controls retain focus while typing. Selecting another task or switching workspace/session clears the prior task's output and query immediately. Unavailable or stale activity correlations have no output-navigation action; they do not fall back to a different task. A completed task remains distinct from a successful command, verification or Acceptance.

Leaving Activity or hiding the page pauses new reads of the selected working task. Returning to a visible Activity view resumes reads of the same task in the same workspace/session at the existing two-second polling interval, using the remaining 120-observation budget. Navigation and visibility changes neither renew that budget nor rerun the command. An observation error or unconfirmed cancellation keeps automatic observation paused; unknown, input-required and terminal states are not polled automatically. Use **Refresh exact task** to request the current status and renew the budget.

## Next priorities and tradeoffs

1. **Validate source-connected attention first.** Reusing existing producers and source boundaries gives the largest immediate reduction in context switching. Preserve stable item identities, keyboard focus, incomplete coverage, and truthful empty states. A failure to refresh is not zero problems.
2. **Extend result and output inspection.** Retained command output now has local search, stream filters, stable scrolling and an exact activity-to-task link. Follow-up work should add native check/test identities, failure locations, producer metadata and real coverage data. Full command program/arguments/cwd are not exported by the current observation contract. Arbitrary command replay remains separate from reading output.
3. **Add commit-bound external checks.** CI/PR observation should distinguish local changes, commit SHA, check suite/job/step, attempt, platform, and conclusion. Authentication and polling limits are integration concerns; a last successful run cannot prove the current worktree.
4. **Add runtime signal correlation.** Align existing execution/task/evidence identifiers first. Application traces, logs, metrics, and profiles require explicit collection, retention, redaction, resource identity, and clock handling. wcode runtime health is not application observability.
5. **Add debugger and interactive terminal protocols when justified.** Breakpoints, stepping, stack frames, watch values, and locals require a debugger session and supported adapters. A terminal session requires process/PTY lifecycle, input ownership, cancellation, and platform testing. These cannot be inferred from ordinary command execution.

## Remaining limits

No complete debugger UI, breakpoint/watch/locals workflow, arbitrary interactive terminal session, external CI/PR log explorer, or external trace ingestion is established by this slice. Comprehensive test discovery, individual result history, coverage visualization, full historical source browsing, and safe Git conflict resolution also need further work.

The source tree and graphs are bounded observations. A file outside a visible snapshot is not necessarily absent. A mapped test has not necessarily run; a historical pass may be stale; a missing provider is not a clean diagnostic result. Existing Workspace, protected-path, command authorization, OAuth, and revision rules continue to govern every surface.

## Acceptance criteria

An operator should be able to identify the selected workspace and freshness, select a real problem, understand its producer and cause, reach the relevant context, and determine which evidence is current. Keyboard and narrow-screen flows, workspace switches, stale responses, failed refreshes, unavailable providers, and truncated data must be verified against the actual implementation.

Use the project verification gates plus native TUI rendering and WebUI interaction checks. Documentation and static checks alone do not prove end-to-end usability, cross-platform behavior, or that an IDE is no longer needed for every workflow.
