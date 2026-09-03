---
layout: docs
title: Agentic Engineering Model
description: wcode model-neutral agent execution and evidence architecture
lang: en
alternate: /zh/docs/agentic-engineering/
permalink: /docs/agentic-engineering/
---

# Agentic Engineering Model

wcode treats modern agentic coding as an execution-architecture problem, not as permission to remove engineering constraints. The model is deliberately vendor-neutral: hosts may differ in rules, Skills, subagents, worktrees, hooks, plugins, or MCP support, while wcode keeps repository state, authorization, verification, and Evidence semantics stable.

## The wcode model

```text
Compact Context Policy
  ↓
agent_context / on-demand Skill
  ↓
Isolated Worker(s)
  ↓
Bounded Workspace mutation
  ↓
Deterministic Gate
  ↓
Revision-exact Evidence
  ↓
Convergence
```

### 1. Context Policy: give the agent a map

Always-on instructions should stay short. They identify the Workspace/security boundary, Desired State, Product Scopes, the preferred coding path, and authoritative verification tools. Detailed architecture, requirement history, source bodies, language tooling, and verification state should be loaded only when needed.

For normal coding, `agent_context` is the primary entry point. Its bounded adaptive pack can include relevant Design State, scope-aware repo-map results, a small amount of hot source, edit SHA preconditions, related tests, readiness, deterministic next actions, and an explicit parallelism strategy. Readiness also exposes a `change_strategy` and `complexity_budget`: a normal single-target task starts as `minimal_patch` with zero new production files, abstractions, configuration knobs, or public API changes. The agent may escalate only when the current requirement, existing duplication, or a proven ownership/boundary problem requires it; hypothetical future extensibility is not evidence. MCP calls should stay minimal: omit the default Workspace and server-default path/limit/timeout/budget values unless the task genuinely overrides them. Use `symbol_context`, `software_context`, `scope_status`, `language_quality_status`, and deeper graph/traceability tools only when the pack says more context is needed.

### 2. Skills: progressive disclosure, not hidden execution

The portable wcode Agent Skill is instructions-only. It contains no hooks, credentials, scripts, or implicit Workspace widening. A Skill may explain when to use wcode capabilities, but mandatory behavior is not considered enforced merely because a prompt says “always do X”. Enforcement belongs in Workspace policy, authorization, Harness verification, or Evidence readiness.

### 3. Isolated workers: parallelize independent reasoning

Subagents, worktrees, equivalent isolated contexts, and concurrent top-level MCP calls are useful for independent repository research, alternative implementation analysis, test generation, security review, and maintainability review. Split the task into dependency lanes before execution. Independent lanes should fan out when it reduces latency or context interference; only true data/path dependencies should serialize. When the inputs are already known, prefer bulk primitives such as `read_files`, `search_many`, `apply_file_edits`, and `create_files`. Use nested `parallel_tools` only for compact fan-out so Host call displays do not become large recursive JSON payloads.

Workers must not concurrently mutate shared or dependent state outside wcode's path-resource Scheduler and SHA preconditions. Related state updates should remain atomic when partial application would make the system harder to reason about. Agreement between multiple models is still model evidence, not deterministic proof.

### 4. Deterministic Gate: mandatory policy lives outside the prompt

Mandatory controls live in deterministic mechanisms:

- Workspace root/path protection;
- exact destructive and risky-operation authorization;
- no-shell command-policy validation;
- SHA edit preconditions;
- Product Scope and Convention checks;
- `review_changes` maintainability signals;
- Verification Plan readiness and revision matching;
- stage Evidence and HumanApproval.

This keeps wcode model-neutral: a stronger model improves execution quality without becoming the source of truth for policy.

### 5. Evidence: attach proof to the exact revision

Compiler checks, static analysis, tests, language-quality providers, independent review, advanced verification stages, and human approval produce provenance-bearing Evidence. Evidence is meaningful only for the code+design revision it proves. Historical green results remain history rather than current proof.

### 6. Convergence: the output is not “code generated”

The control objective is:

```text
Desired State → Actual State → Change → Proof → Convergence
```

An agent can propose or implement a change. wcode determines whether the implementation is traceable, whether the observed architecture aligns with declared Design State, what proof exists for the current revision, and what remains to converge.

## Host integrations

Cursor, Claude Code, Codex, Copilot, Gemini, Grok, and other hosts can expose different combinations of rules, Skills, subagents, worktrees, hooks, plugins, and MCP. The portable contract is:

- use the host's own sandbox/worktree/isolation features;
- use the wcode Skill as a short control-plane map;
- use MCP for structured repository operations and intelligence;
- preserve host/operator approval boundaries;
- mutate source through wcode's guarded Workspace tools when wcode is the editing harness;
- use Verification/Evidence for claims that must survive model changes or runtime restarts.

See [Code Agent Integrations](../code-agent-integrations/) for host-specific connection patterns.

## Language-aware agent workflow

Before substantial source edits:

1. call `agent_context(goal, scopes=...)` without a manual budget unless the task needs one;
2. follow `readiness`, `next_actions`, `readiness.parallelism`, `readiness.change_strategy`, and `readiness.complexity_budget`; reuse existing owners/helpers and do not exceed the complexity budget without concrete current-task evidence;
3. omit default/inferable MCP arguments and keep `find_symbol` / `search_code` as the cheap localization path; use `semantic_navigation` when readiness identifies cross-file references, callers, implementations, rename impact, or equivalent semantic relationships;
4. use `symbol_context` only when more source is required;
5. use `language_quality_status`, `scope_status`, `design_status`, `traceability_status`, or deeper graph context only when the task needs those facts.

After edits:

1. `review_changes`;
2. drift / impact / risk when applicable;
3. repository-declared check-only `language_quality_run` providers as needed;
4. `verify_project`;
5. required Property / Mutation / Fuzz / Runtime stages and independent review;
6. inspect current-revision `evidence_status` and convergence state.

The sequence is guidance. Workspace policy, authorization, Harness verification, and the Verification Mesh remain the enforcement boundary.

## Research influences

The model follows durable public patterns rather than vendor-specific syntax:

- compact repository maps and task-specific context;
- task-adaptive code navigation: cheap syntax/search for localization, warm semantic sessions for cross-file completeness;
- Agent Skills and progressive disclosure;
- isolated subagents/worktrees for independent work;
- deterministic hooks, policy, and verification gates;
- MCP and equivalent structured tool protocols;
- explicit review/test/evidence loops.

Host products evolve. wcode should periodically re-check primary documentation, while keeping the internal invariant stable: model guidance is replaceable; deterministic repository state, policy, and Evidence are not.
