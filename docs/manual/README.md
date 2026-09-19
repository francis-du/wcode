---
layout: docs
title: Documentation
nav_title: Overview
description: wcode documentation home
lang: en
alternate: /zh/docs/
permalink: /docs/
---

# Make any coding agent understand your repo before it changes it

**wcode gives coding agents the task-ready context, real code relationships, guarded actions, and revision-bound proof they are usually missing.** Use the agent you already like; wcode helps it understand first, change less, and prove the result instead of rebuilding a partial picture from grep, file dumps, and chat history.

## Five-minute mental model

wcode is easiest to understand as five layers around the coding model:

1. **Understand** — `agent_context` combines task-aware retrieval, Design State, code relationships, tests, verified history, and exact edit targets.
2. **Change** — Workspace policy keeps edits bounded, SHA-guarded, root-scoped, and explicit about authority.
3. **Prove** — focused quick checks give fast feedback; deterministic full verification remains the final broad gate.
4. **Learn** — only stable verified changes may teach the local Experience Graph; failed or incomplete work does not become memory.
5. **Observe** — Engineering Observatory acts as a project digital twin: architecture blueprint, live engineering flow, Vibe Coding change story, drift, provenance, risk, and current-revision proof without requiring an IDE.

![wcode Engineering Control Plane loop](/assets/wcode-engineering-loop.svg)

## Find the right view

Press **W** in the TUI to open the protected Engineering Observatory for the selected project. The default tab is **Engineering architecture**. Use the workspace selector to change projects, and the language and theme controls to adjust presentation.

| Surface | What to use it for |
| --- | --- |
| TUI | Connection state, project selection, live activity, and pending authorization. |
| Setup Hub | Setup commands, dry-run previews, launch options, performance presets, and runtime health. |
| Overview | Project pulse, engineering flow, timeline, diagnostics, and language quality. |
| Engineering architecture | System and component ownership, declared and observed dependencies, and architecture inspection. |
| Task activity | Running and queued work, execution timing, and resource use. |
| Verification evidence | Current-revision results, readiness, and verified repository learning. |
| Current changes | Working-tree changes, impact, and verification scope. |
| Requirements | Requirement → component → code → verification → evidence traceability. |
| Project files | The bounded source tree, largest files, and truncation indicators. |
| Access | Authorized projects, executable access, exact operations, and pending requests. |

A mapped check has not necessarily run. A historical pass may belong to an older revision. See [Getting started](getting-started/) for navigation and [Repository Intelligence & Engineering State](software-intelligence/) for evidence semantics.

## Start here

| Goal | Guide |
| --- | --- |
| Install wcode and connect the first repository | [Getting started](getting-started/) |
| Understand repository intelligence and engineering state | [Repository Intelligence & Engineering State](software-intelligence/) |
| Connect a local coding agent or cloud connector | [Code Agent integrations](code-agent-integrations/) |
| Understand workspace, command, OAuth, and media boundaries | [Security](security/) |

## Core concepts

- [Product Scopes](product-scopes/) — product capabilities and source-ownership boundaries.
- [Agentic Engineering](agentic-engineering/) — short instructions, on-demand context, parallel execution, and deterministic verification.
- [Language Quality](language-quality/) — one capability matrix for all 22 indexed languages: syntax, validated semantics, format, lint, types, static analysis, tests, security, and advanced verification, with explicit gaps instead of a Rust-centric support bit.
- [Maintainability Review](maintainability-review/) — structural growth signals, independent review, and Evidence rules.

## Reference, operations and development

- [Engineering Fitness](engineering-fitness/) — model-free retrieval, context, edit-readiness and safety evaluation; separate correctness gates from descriptive performance trials.
- [Research paper](paper/) — English manuscript, Chinese original, metric definitions, diagnostic results, and reproducibility limits.

- [CLI & MCP Reference](reference/) — canonical commands, operator controls, transports, and Tool families.
- [Development](development/) — module boundaries, runtime invariants, release gates, and maintenance constraints.
- [Research-informed improvements](research-upgrades/) — diagnostic context and dependency previews prepared for 0.6.2, primary papers, and evaluation limits.
- [Frontier engineering](frontier-engineering/) — selective context execution, complete verification plans, new research and measurable capability goals.
- [v0.8.1 release notes](releases/v0.8.1/) — Jev Decision Plane hardening, narrower typed judgments, strict increase-only authority, unified naming, and safer WebUI state binding.
- [v0.8.0 release notes](releases/v0.8.0/) — Engineering Digital Twin, bounded code graph/time travel, faster Observatory refresh, ignore-aware scanning, calibrated Decision Plane and task-first TUI.
- [v0.7.6 release notes](releases/v0.7.6/) — model-free Engineering Fitness, edit-ready 1K context, multi-target retrieval and adversarial release verification.
- [v0.7.5 release notes](releases/v0.7.5/) — concurrent verified endpoints, generation-bound trust, conflict-free TUI keys and responsive Observatory content.
- [v0.7.4 release notes](releases/v0.7.4/) — Workspace-session command authorization, standard MCP image/audio content, and refreshed current-product docs/UI.
- [v0.7.3 release notes](releases/v0.7.3/) — autonomous bounded development commands, broader Deno/Flutter quality coverage, stronger repository-intelligence caches, and WebUI/verification reliability.
- [v0.7.2 release notes](releases/v0.7.2/) — clearer Observatory/TUI/Setup state, model-efficient tool discovery, Agent Context retrieval telemetry, and more precise Verification Mesh targets.
- [v0.7.1 release notes](releases/v0.7.1/) — resilient stable tunnels, real-time project switching, hardened semantic auto-discovery, less authorization friction, and stronger parallel execution.
- [v0.7.0 release notes](releases/v0.7.0/) — a rebuilt Engineering Observatory, faster project-state loading, stronger architecture/proof workflows, and hardened release contracts.
- [v0.6.2 release notes](releases/v0.6.2/) — resource-aware parallelism, simpler setup, a clearer Observatory, and effective revision-bound evidence.
- [v0.6.1 release notes](releases/v0.6.1/) — completion-driven parallel scheduling, fewer redundant context calls, and a task-first TUI.
- [Releases](releases/) — latest version plus the complete archive grouped by series. Historical versions stay out of the global sidebar so documentation navigation remains bounded.

## Recommended workflow

```text
agent_context(goal, scopes=...)
  ↓
follow readiness; load deeper context only when needed
  ↓
semantic_navigation only for cross-file relationships
  ↓
implement / edit
  ↓
review_changes
  ↓
verify_project
  ↓
drift / risk / evidence / reconciliation only when required
```

Commands, tool names, protocol names, and field names keep their canonical technical spelling. Explanatory prose follows the selected document language; English and Chinese navigation are separate instead of mixing both languages in one sidebar.
