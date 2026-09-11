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

## Start here

| Goal | Guide |
| --- | --- |
| Install wcode and connect the first repository | [Getting started](getting-started/) |
| Understand the software-intelligence loop | [Software Intelligence](software-intelligence/) |
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
- [v0.6.2 release preparation](releases/v0.6.2/) — resource-aware parallelism, simpler setup, a clearer Observatory, and effective revision-bound evidence.
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
