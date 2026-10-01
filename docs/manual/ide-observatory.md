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

## Next priorities and tradeoffs

1. **Validate source-connected attention first.** Reusing existing producers and source boundaries gives the largest immediate reduction in context switching. Preserve stable item identities, keyboard focus, incomplete coverage, and truthful empty states. A failure to refresh is not zero problems.
2. **Add result and output inspection.** A read-only command/check viewer should search the retained output and show stream, producer, command identity, lifecycle state, timing, and omitted-output flags. Test adapters can then attach test identity, failure location, and real coverage data. Arbitrary command replay must remain separate from reading output.
3. **Add commit-bound external checks.** CI/PR observation should distinguish local changes, commit SHA, check suite/job/step, attempt, platform, and conclusion. Authentication and polling limits are integration concerns; a last successful run cannot prove the current worktree.
4. **Add runtime signal correlation.** Align existing execution/task/evidence identifiers first. Application traces, logs, metrics, and profiles require explicit collection, retention, redaction, resource identity, and clock handling. wcode runtime health is not application observability.
5. **Add debugger and interactive terminal protocols when justified.** Breakpoints, stepping, stack frames, watch values, and locals require a debugger session and supported adapters. A terminal session requires process/PTY lifecycle, input ownership, cancellation, and platform testing. These cannot be inferred from ordinary command execution.

## Remaining limits

No complete debugger UI, breakpoint/watch/locals workflow, arbitrary interactive terminal session, external CI/PR log explorer, or external trace ingestion is established by this slice. Comprehensive test discovery, individual result history, coverage visualization, full historical source browsing, and safe Git conflict resolution also need further work.

The source tree and graphs are bounded observations. A file outside a visible snapshot is not necessarily absent. A mapped test has not necessarily run; a historical pass may be stale; a missing provider is not a clean diagnostic result. Existing Workspace, protected-path, command authorization, OAuth, and revision rules continue to govern every surface.

## Acceptance criteria

An operator should be able to identify the selected workspace and freshness, select a real problem, understand its producer and cause, reach the relevant context, and determine which evidence is current. Keyboard and narrow-screen flows, workspace switches, stale responses, failed refreshes, unavailable providers, and truncated data must be verified against the actual implementation.

Use the project verification gates plus native TUI rendering and WebUI interaction checks. Documentation and static checks alone do not prove end-to-end usability, cross-platform behavior, or that an IDE is no longer needed for every workflow.
