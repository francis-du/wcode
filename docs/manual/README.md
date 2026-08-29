---
layout: docs
title: Documentation
nav_title: Overview
description: wcode documentation home
lang: en
alternate: /zh/docs/
permalink: /docs/
---

# wcode Documentation

This is the English documentation home. The product site stays focused on the product and quick start; protocol details, agent integration, security boundaries, quality policy, and development constraints live here.

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
- [v0.4.1 release notes](releases/v0.4.1/) — tunnel URL extraction and Agent Context budget fixes, expanded client matrix, redesigned docs.
- [v0.4.2 release notes](releases/v0.4.2/) — concurrent background tunnels with Tailscale Funnel, iMessage delivery of live links, per-tunnel retry, and graceful shutdown.
- [v0.4.0 release notes](releases/v0.4.0/) — faster Agent Context, architecture-first observability, resilient tunnels, and bounded development CLI automation.
- [v0.3.0 release notes](releases/v0.3.0/) — historical 0.3 product shape and major changes.

## Recommended workflow

```text
agent_context(goal, scopes=...)
  ↓
follow readiness; load deeper context only when needed
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
