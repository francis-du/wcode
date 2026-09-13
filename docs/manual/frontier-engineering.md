---
layout: docs
title: Frontier engineering and capability roadmap
description: New research, selective context execution, complete verification plans, and measurable gates toward a standalone coding agent.
lang: en
alternate: /zh/docs/frontier-engineering/
permalink: /docs/frontier-engineering/
---

# Frontier engineering and capability roadmap

## Status

Reviewed on 2026-09-12. This is a new research pass after [the earlier upgrades](../research-upgrades/), not a claim that those papers were all newly discovered. The implemented changes below are included in the [v0.6.2 release preparation](../releases/v0.6.2/); later roadmap items remain unimplemented. A successful source test does not update a running MCP process or its exposed schema.

The implemented scope now also includes task-backed project verification as described below. Selective context retrieval and complete bounded verification planning remain implemented; the standalone agent and general mutation recovery described later are still roadmap items. No external model, embedding service, neural skimmer, new production dependency, automatic permission grant, or new MCP input parameter is introduced in this change.

## New primary research and decisions

| Primary source and inspected version | What it contributes | Decision for wcode |
| --- | --- | --- |
| [SWE-Pruner](https://arxiv.org/abs/2601.16746), first submitted 2026-01-23; current v4 revised 2026-05-07 | Goal-conditioned selection of relevant source lines with a trained 0.6B neural skimmer; warns that indiscriminate compression loses code structure. | Adopt task-directed selection while keeping original source and SHA. Do not deploy the neural model without measuring inference cost, startup, accuracy and hardware requirements. This patch is not a reproduction of SWE-Pruner. |
| [CodeScout](https://arxiv.org/abs/2603.17829), 2026-03-18 | Treats repository localization as its own trained and evaluated capability, rather than assuming a large tool set solves search. | Separate localization fidelity from patch success. Explicit file locations should not pay for redundant global lexical discovery. No RL model or training pipeline is added. Its evaluated localization setting is not proof of every language's repair quality. |
| [Coding Agents are Effective Long-Context Processors](https://arxiv.org/abs/2603.20432), 2026-03-20 | Studies explicit filesystem/tool interaction for long-context reasoning and retrieval. | Keep progressively accessible source instead of requiring every repository fact in the first response. Its QA/retrieval results are not wcode bug-fix scores. |
| [Harness Engineering: a source-code study of eleven systems](https://arxiv.org/abs/2609.00006) | Examines agent loops, context, tools, control, orchestration and extension surfaces across production harness snapshots. | Reuse the current runtime owners; do not add a second orchestration framework or mandatory vector database merely for architectural fashion. This is a sampled source-code study, not a controlled performance comparison or proof that embeddings cannot help. |
| [Towards a Science of Scaling Agent Systems](https://arxiv.org/abs/2512.08296), first submitted 2025-12-09; current v3 revised 2026-04-08 | Controlled experiments show coordination costs and strong dependence on task structure. The revised study expands the experimental setting. | Preserve real dependency-aware concurrency, not a fixed swarm or a reward for maximum tool-call count. Central verification remains distinct from worker agreement. No paper-specific performance threshold becomes a production rule. |
| [SWE-Bench Pro Verified](https://arxiv.org/abs/2609.08149), 2026-09-08 | Examines answer leakage, hidden evaluation information, and inconsistent task/test definitions. | Require complete check accounting and plan explicit protected evaluations. Never infer product superiority from a benchmark with accessible gold patches or from a unit-test count. The external benchmark has not been run here. |
| [Agent Retrieval Bench](https://arxiv.org/abs/2607.24882), 2026-07-27 | Separates repository context retrieval from final patching across `code2test`, `comment2context`, `trace2code`, `edit2ripple`, and natural no-gold cases. No retrieval family wins every task; RepoMap has the best 8K-token budgeted context yield, while selective thresholds still show a natural-case calibration gap. | Route clear retrieval intents through different bounded priors instead of one global ranking. Keep ambiguous mixed requests on balanced context, label routing heuristic, and do not add an embedding/model dependency until it wins a protected cost-and-quality evaluation. |
| [Engineering Reliable Coding Agents](https://arxiv.org/abs/2608.13867), 2026-08-14 | Treats coding-agent reliability as a dependency chain across harness, execution state, retrieval, memory, permissions, verification, observability and resources rather than a model-only property. | Keep wcode's reliability layers independently observable and fail closed: retrieval routing cannot override provenance, learned scheduling can circuit-break itself, and asynchronous tunnel/verification state must reject stale completion. |
| [SWE-bench-Live](https://github.com/microsoft/SWE-bench-Live), inspected 2026-09-12 | Continuously updates multi-language and Windows task sets. Its 2026-08-21 update reports 1,077 MultiLang tasks across 431 repositories and 8 languages, plus 66 Windows tasks across 48 repositories and 9 languages. | Future external evaluation should include contamination-resistant, multi-language and multi-OS suites. This repository has not run or claimed a SWE-bench-Live score in this change. |

Research results are hypotheses and design evidence, not performance certificates for this repository. The latest paper's submission date does not imply that an older but better-supported mechanism is obsolete.

## Implemented: selective context execution

Previously, `agent_context` performed normal repository-wide symbol discovery before resolving an already explicit location, then replaced those candidates with the resolved anchor. A missing path could also populate the source index with unrelated files before returning no target.

The new path resolves guarded anchors first and passes the resulting symbols into the canonical software-context builder. It keeps repository guidance, Design State, confirmed semantics, traceability, verification references, known risks, the persistent Worklist and SHA preconditions. It skips only the redundant lexical discovery when explicit anchors are present, including explicit anchors that cannot be resolved.

For a simple unscoped location query, `repo_map.deferred` is `true`: building the repository graph is deferred, not falsely reported as having found no relationships. Queries asking for callers, impact or architecture, and explicitly scoped queries, continue through graph expansion. Ordinary symbol queries retain their discovery path. Semantic-provider facts keep their existing provenance and freshness checks.

A request such as `src/runtime/worker.rs:120` can therefore return the requested original excerpt without first ranking an unrelated repository map. Cross-file work must still request the relationships it needs. Design/traceability or provider data may legitimately inspect other files; this is not a guarantee that every anchored request accesses exactly one file in every repository.

Local regression fixtures place one target beside 128 irrelevant files. Before the change, the cold target test retained 30 indexed-file records at observation time; the missing-location test retained 129 after a control outline was read. These are cache occupancy observations, not total parse counts. The new acceptance target is one retained target record for both fixtures, in cold and warm target queries, with the exact line, SHA and original excerpt retained. The test's elapsed-time printout is diagnostic only; it is not a statistically controlled latency benchmark.

Additional fixtures preserve ordinary symbol and caller discovery, Design/test references and guidance, plus Python, TypeScript, Go and JSON source at 1,000- and 4,000-token-estimate budgets. Rust anchors and path/SHA protection retain their existing regressions. The token estimate remains serialized bytes divided by four, not a provider tokenizer measurement.

## Implemented: task-aware retrieval routing

Agent Retrieval Bench gives a concrete reason not to treat repository localization as one ranking problem: `trace2code`, `code2test`, `edit2ripple`, and general context acquisition reward different signals, while selective no-gold calibration remains imperfect. wcode therefore adds a small deterministic routing layer rather than an embedding service or model classifier.

`query-intent-rules-v1` recognizes one clear `trace_to_code`, `code_to_test`, or `edit_to_ripple` intent and changes only bounded heuristic priors. Trace work prefers Design-owned implementation/dependency evidence; test work prefers test paths/symbols; ripple work gives more room to direct relationships and the verified Experience Graph. A request with multiple competing intent classes does not guess: it reports `balanced_context`, `specialized=false`, `abstained_from_specialization=true`, and `reason=ambiguous_retrieval_signals`.

The router is not a new precision claim. Exact literal/qualified targets remain stronger than broad Software Context seeds; fresh semantic/deterministic/runtime graph evidence remains stronger than heuristic priors; verified historical co-change stays bounded below direct lexical evidence. At the 1,000-token floor, routing explanation is disposable before direct SHA targets, tests, the strongest repo-map item and diagnostic Hot Source. The policy lives in `src/runtime/harness/retrieval.rs`, separate from graph construction/ranking execution in `repo_map.rs`, so future routing A/B experiments do not turn the graph engine into a policy monolith.

Local regressions cover intent classification, ambiguous abstention, root-level test-path recognition, exact-target precedence, code-to-test ordering and preservation of the existing tight-budget/diagnostic-anchor contracts. These tests validate routing invariants; they are not a reproduction of Agent Retrieval Bench and do not establish an external retrieval score.

## Implemented: complete bounded verification plans

The old verification planner silently took the first eight inferred checks. A mixed Rust/Node/Python/Go/Make fixture infers sixteen; the original exhaustive report accounted for only eight. This can omit tests or builds while still describing the executed prefix as the full gate.

The planner now constructs and sorts the complete selected plan. Its total-plan bound is 32; exceeding that bound produces an explicit `no checks executed` error before dispatch, rather than silently dropping checks. This is a plan-size bound, not a new concurrency setting. The existing semaphore, phase ordering, fail-fast behavior and revision-bound Evidence remain in force.

The sixteen-check fixture revokes its temporary workspace's executable permissions so it exercises scheduling and result accounting without launching language toolchains. It requires all sixteen inferred IDs in exhaustive diagnostic results, all failed for the intended authorization boundary, and no skipped entries. It does not prove that sixteen external language tools were installed or passed. A separate over-capacity fixture requires no dispatch, no authorization request and no Evidence.

## Implemented: reconnectable project verification

`verify_project` now reuses the existing durable Tasks runtime. A modern `2026-07-28` request declaring `io.modelcontextprotocol/tasks` receives a persisted `taskId` without waiting for a verification slot. The client polls `tasks/get` using that ID and its authenticated owner, respecting `pollIntervalMs`. A client without this extension, or a legacy client, keeps the synchronous result. There is no new `async` tool argument and no additional model-facing tool. This follows the [MCP Tasks extension](https://tasks.extensions.modelcontextprotocol.io/) rather than requiring an uninterrupted long request.

The worker remains owned after the creation request ends. Explicit cancellation cancels queued checks; a monotonic deadline also cancels outstanding work when nobody polls. Deadline and worker failures produce a durable failure, not an invalid-parameter error or a passing report. Already-started blocking operations still cannot promise rollback. Runtime restart exposes interrupted work as failed, never silently reruns it; previously completed results remain readable.

`completed` means the underlying tool returned a result, not that the checks passed. Consumers must inspect `result.isError` and `result.structuredContent.passed`. Repeated polling and reopening a completed record return its stored result without generating more verification runs or Evidence. The strict Workspace parser is now shared by ordinary and durable entry points; malformed routing or verification options fail before a durable task, worker or approval is created.

Tests exercise real temporary Git verification, successful and failed reports, occupied slots, requester completion, repeated polls, fresh runtime reads, wrong-owner queries/cancellation, cancellation without Evidence, synchronous fallback, and expiry with no task polling. These are local protocol/runtime fixtures, not a deployed network or multi-platform benchmark.

Limits: this does not implement exactly-once mutation execution, a replay journal, takeover of interrupted workers, or recovery when the creation response and its `taskId` are both lost. Do not reissue a mutation or `verify_project` merely to poll; retained task IDs are essential. Runtime replacement and host extension support must be checked separately before relying on this behavior.

## What functional replacement requires

The current [Codex CLI documentation](https://developers.openai.com/codex/cli/features/) describes an interactive coding loop, review, session resumption, delegation and MCP integration. The [Claude Code subagent documentation](https://code.claude.com/docs/en/sub-agents) describes separate contexts, configurable tools and model selection, permissions and worktree isolation, with different support rules for plugin-provided definitions. Those are capability contracts to test, not boxes satisfied by merely exposing a similarly named tool.

A portable plugin and a standalone coding agent are different delivery surfaces. The proposed strategy is one shared repository/runtime core with a thin optional agent host, not rewriting existing edits, authorization, indexing and verification inside a second system. Provider licensing, authentication and model-specific behavior require their own integration decisions.

| Proposed work, not implemented in this change | Concrete acceptance gate |
| --- | --- |
| Provider-neutral interactive loop and streaming model adapters | Complete inspect/edit/test tasks, preserve tool-call identity, support user interruption and injected 429/disconnection/partial-stream failures without inventing success. Run the same workflow through at least two real configured providers. |
| Durable operation history and recovery | Record prepared, started, completed and outcome-unknown states. Reconnect reads can be retried; an interrupted mutation is reconciled against its recorded effects before retry. Replaying a transcript must not replay a write or publish operation. |
| Isolated parallel work and checkpoints | Give workers explicit base revisions and non-overlapping ownership, return patches for controlled integration, detect conflicts, and reject fallback to the main checkout when isolation disappears. Unfinished user changes survive rollback and cancellation tests. |
| Process sessions, application testing and debugging | Own process trees and ports, expose bounded incremental output, stop children on cancellation, and verify applications via declared test/browser/debug adapters. A PTY or shell adapter needs explicit sandbox and approval design; it must not become a workaround for the current no-shell policy. |
| Useful permissions instead of repeated friction | Classify known read checks, exact approvable mutations and invalid/boundary violations. Display the real operation and scope; preserve denial and client ownership. Host form support and grouped approvals need protocol and UX tests, not silent Full Access. |
| Independent correctness evaluation and developer adoption | Fixed-model paired trials across repositories/languages, protected tests, compatibility matrix, reproducible setup, clear failures and documented recovery. Track first-task completion and real repeated use rather than fabricated popularity. |

The table above is a proposed roadmap, not an implemented feature list. The separately labeled implemented sections describe the delivered source changes; the remaining gates are proposals, not claims of complete Codex or Claude Code equivalence or proof of global rank.

## Evaluation protocol

Use the same model/provider version, prompt, repository revision, dependency lockfiles, time/token budget and hardware for paired comparisons. Separate cold and warm index/cache runs and record tool discovery, tool round trips, model-visible bytes, actual provider tokens where available, latency, targeted localization and hidden-test task success. Include missing anchors, large monorepos, conflicting writes, denied permission, interrupted calls and unavailable providers.

Keep benchmark solutions, later Git history and hidden tests outside the agent's accessible workspace. Disallow fetching task answers while permitting the task's explicitly allowed documentation sources. Run the same protected evaluator for all contenders, retain unsuccessful cases, and report confidence intervals over repeated runs. Do not treat a changed test suite, an omitted gate or an unknown outcome as a passing task.

The immediate checks are `review_changes`, `verify_project(level="full")`, `cargo clippy --locked --all-targets -- -D warnings`, Design validation and matching test/build Evidence revisions. Source-layout checks protect module bounds. A stable new-process MCP smoke test and Linux/macOS/Windows CI remain separate release requirements. This document itself does not attest that any current build passed.
