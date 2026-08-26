---
layout: wiki
title: Agentic Engineering Model
description: wcode model-neutral agent execution and evidence architecture
permalink: /wiki/agentic-engineering/
---

# Agentic Engineering Model

wcode treats modern “vibe coding” as an execution architecture problem, not as permission to remove engineering constraints.

Research across current coding-agent systems converges on several useful primitives: short always-on repository instructions, progressively disclosed Skills, isolated/parallel workers, deterministic hooks or policy gates, tool protocols such as MCP, and explicit review/test loops. wcode adopts the useful semantics without coupling its product model to one vendor's configuration format.

## The wcode model

```text
Context Policy
  ↓
On-demand Skill / Software Context
  ↓
Isolated Worker(s)
  ↓
Deterministic Gate
  ↓
Revision-exact Evidence
  ↓
Convergence
```

### 1. Context Policy: give the agent a map

Always-on instructions should stay short. They should identify:

- the workspace/security boundary;
- where Desired State lives;
- the Product Scope map;
- how to retrieve task-specific context;
- which verification/control-plane tools are authoritative.

Detailed architecture, requirement history, language tooling, and verification state should be retrieved only when the task needs them. Huge instruction files waste context and become stale copies of facts already present in Design State or the repository.

wcode therefore pushes task detail into `software_context`, `traceability_status`, `scope_status`, `language_quality_status`, symbol navigation, and verification/evidence tools.

### 2. Skills: progressive disclosure, not hidden execution

The portable wcode Agent Skill is instructions-only. It contains no hooks, credentials, scripts, or implicit workspace widening.

Skills should tell a host agent when and how to use wcode capabilities. Mandatory behavior is not considered enforced merely because a Skill says “always do X”. Enforcement belongs in Workspace policy, authorization, Harness verification, or Evidence readiness.

### 3. Isolated workers: parallelize independent reasoning

Modern coding hosts can use subagents, worktrees, or equivalent isolated contexts. wcode encourages them for independent work such as:

- repository research;
- alternative implementation analysis;
- test generation;
- security/adversarial review;
- maintainability review.

But workers must not concurrently mutate shared/dependent state outside wcode's path-resource Scheduler and SHA preconditions. Related state updates should remain atomic when that produces a simpler model.

Multiple model workers agreeing is still model evidence. It is not deterministic proof and cannot clear compiler/test/property/fuzz/mutation/human gates.

### 4. Deterministic Gate: policy that must happen lives outside the prompt

Agent rules are probabilistic instructions. Mandatory controls must live in deterministic mechanisms:

- workspace root/path protection;
- exact destructive authorization;
- command-policy validation;
- SHA edit preconditions;
- Product Scope/convention checks;
- `review_changes` maintainability signals;
- Verification Plan readiness;
- revision matching;
- stage Evidence and human approval.

This lets wcode remain model-neutral: a stronger model improves execution quality without becoming the source of truth for policy.

### 5. Evidence: attach proof to the exact revision

Compiler, static analysis, tests, language-quality checks, independent review, advanced verification stages, and human approval all produce provenance-bearing Evidence.

Evidence is meaningful only for the revision it proves. Project Observatory therefore counts Proof only when code+design revision matches the current repository. A historical green run is history, not current proof.

### 6. Convergence: the output is not “code generated”

The control objective remains:

```text
Desired State → Actual State → Change → Proof → Convergence
```

A coding agent can propose or implement a change. wcode decides whether the repository is traceable, whether actual architecture matches declared design, which quality dimensions exist for the language, what proof exists for the current revision, and what remains to converge.

## Host integrations

Cursor, Claude Code, Codex, Copilot, Gemini, Grok, and other hosts can expose different combinations of rules, Skills, subagents, worktrees, hooks, plugins, and MCP. wcode does not need to mirror each configuration language internally.

The portable contract is:

- use the host's own sandbox/worktree/isolation features;
- use the wcode Skill as a short control-plane map;
- use MCP for structured repository operations and intelligence;
- preserve host/operator approval boundaries;
- write source through wcode's guarded workspace tools when wcode is the editing harness;
- use Evidence/Verification for claims that must survive model changes or runtime restarts.

## Language-aware agent workflow

Before substantial source edits:

1. `workspace_info`
2. `scope_status`
3. `design_status`
4. `project_context`
5. `language_quality_status`
6. task-scoped `software_context` / symbol navigation

After edits:

1. `review_changes`
2. drift / impact / risk as applicable
3. repository-native `language_quality_run` checks that are declared and check-only
4. `verify_project`
5. required Property/Mutation/Fuzz/Runtime stages
6. independent review
7. inspect current-revision `evidence_status` / convergence state

This sequence is guidance; the Harness, authorization system, and Verification Mesh remain the enforcement boundary.

## Research influences

The model deliberately follows current public patterns rather than vendor-specific syntax:

- OpenAI agent-first harness guidance: small repository maps and structured task context;
- Agent Skills: progressive disclosure and portable skill directories;
- Anthropic Claude Code: Skills, isolated subagents, hooks/permissions, MCP;
- Cursor: project rules, Skills, subagents and structural code review;
- GitHub Copilot: repository instructions, Agent Skills and custom agents;
- Gemini CLI: hierarchical context plus hooks/tool policy.

These products evolve. wcode should periodically re-check their official documentation, but the internal invariant should remain stable: model guidance is replaceable; deterministic repository state, policy and Evidence are not.
