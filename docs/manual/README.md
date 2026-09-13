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

For retrieval, clear `trace→code`, `code→test`, and `edit→ripple` tasks use different bounded heuristic priors. Ambiguous mixed requests deliberately abstain to balanced context. Exact targets and stronger fresh semantic/deterministic/runtime evidence remain above these heuristics.

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
- [Language Quality](language-quality/) — syntax, semantics, formatting, linting, types, tests, security, and advanced verification capabilities.
- [Maintainability Review](maintainability-review/) — structural growth signals, independent review, and Evidence rules.

## Reference, operations and development

- [CLI & MCP Reference](reference/) — canonical commands, operator controls, transports, and Tool families.
- [Development](development/) — module boundaries, runtime invariants, release gates, and maintenance constraints.
- [Research-informed improvements](research-upgrades/) — diagnostic context and dependency previews prepared for 0.6.2, primary papers, and evaluation limits.
- [Frontier engineering](frontier-engineering/) — selective context execution, complete verification plans, new research and measurable capability goals.
- [v0.7.0 release preparation](releases/v0.7.0/) — a rebuilt Engineering Observatory, faster project-state loading, stronger architecture/proof workflows, and hardened release contracts.
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
