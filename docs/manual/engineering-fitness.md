---
layout: docs
title: Engineering Fitness
description: Model-free, deterministic evaluation of repository retrieval, context, edit readiness, safety and cost.
lang: en
alternate: /zh/docs/engineering-fitness/
permalink: /docs/engineering-fitness/
---

# WCode Engineering Fitness

WCode Engineering Fitness is the model-free measurement layer for wcode. It measures deterministic repository-engineering properties that wcode owns directly; it is not a coding-model leaderboard and does not claim that a model will solve a task.

## Why “Engineering Fitness”

“Benchmark” is too easy to read as an end-to-end model ranking, while “Capability Suite” suggests a single opaque capability score. Engineering Fitness instead treats each release like an engineering system under repeatable fitness checks: retrieval quality, semantic accuracy, context efficiency, edit readiness, bug-relevant evidence, robustness and cost remain separate dimensions.

## Measurement contract

Every task freezes a repository revision, task text, Gold Evidence and evaluator version. Gold Evidence names required and useful source identities such as paths, qualified symbols, relationships and verification targets. The evaluator compares observable wcode output with that Gold set. Reference patches, future history and hidden evaluator data must not be exposed to the system under measurement.

The long-term measurement dimensions are listed below. The executable coverage and unmeasured dimensions are stated explicitly later; this table is not a claim that every dimension is implemented.

| Dimension | Deterministic evidence |
| --- | --- |
| Retrieval | required evidence recall, top-K recall/ranking, irrelevant-result ratio |
| Semantics | definition/reference/caller/callee/implementation precision and recall against a known graph |
| Context | Gold Evidence coverage at fixed byte/token/line budgets; response size; cold/warm behavior |
| Edit readiness | exact target identity, current SHA, writable target and mapped verification evidence |
| Bug-relevant evidence | root-cause/impact/test evidence exposed for reviewed mutation fixtures; never mislabeled as autonomous bug detection |
| Robustness | stale-write rejection, invalidation, partial-result honesty, authorization and workspace boundaries |
| Cost | wall time, calls/round trips, serialized bytes, estimated tokens, cache/index/LSP work and resource use where observable |

Do not collapse these dimensions into one release score. A faster result with lower Gold recall is a visible regression, not a hidden trade-off inside an average.

## Evaluation levels

**Fitness checks** are deterministic and suitable for normal CI. Correctness assertions are hard gates; noisy wall-clock measurements are descriptive unless a controlled performance job establishes a statistically justified threshold.

**Fitness trials** are explicit release-profile measurements. Existing `fast_context_benchmark`, `repo_rank_release_cost_comparison` and `competitive_io_benchmark` remain stage-level trials and keep their existing caveats. They are inputs to Engineering Fitness, not model-accuracy claims.

**Agent evaluation** is a separate future layer. When model budget exists, the same frozen tasks can compare a fixed model with baseline tools versus wcode. Agent task success must never be inferred from Engineering Fitness alone.

## Executable checks and trials

Run deterministic checks:

```sh
cargo test --locked --lib engineering_fitness_ -- --nocapture
```

Run explicit optimized measurements and save reports:

```sh
cargo test --release --locked --lib engineering_fitness_trial -- --ignored --nocapture --test-threads=1
```

`FITNESS_REPORT` prints the actual JSON and Markdown paths, shaped like `target/engineering-fitness-<timestamp>-<pid>.json`. Workspace create-only writes preserve earlier reports and reject path/symlink escapes. No model API is called; local build, CPU, memory and disk costs still exist.

**A passing evaluator test does not mean that all measured queries succeeded.** `errors`, `query_errors` and per-sample error records describe tool failures. Failed queries remain in Gold denominators with zero hits. Warmup errors are separate and never represented as successful warmups. The runner does not silently retry at a larger budget. Hard test assertions protect grader correctness, successful-response budget limits and deterministic safety controls, not a demand that retrieval quality be 100%.

## Corpus and delivered evidence

The development corpus contains 60 synthetic scenarios. Rust, Go, TypeScript and Python each cover explicit symbols, single targets, symbol-free natural language, caller/call-chain context, path-qualified localization and impact-oriented queries. Additional cases cover same-name/path disambiguation, four explicit targets, Unicode/CRLF source, missing anchors, read-only access, ten distinct ownership-guard mutation fixtures and 640 distractor files.

The four language translations share one lifecycle behavior family. These are **not 60 independent real projects or a hidden generalization score**. Gold remains in the evaluator, outside each temporary measured Workspace. The removed-guard task measures bug-relevant source exposure, not autonomous bug finding or a mutation-testing kill rate.

There are 60 cases × 3 budgets (1,000/2,000/4,000) × 2 cache phases = 360 measurement cells. Ordinary checks take one sample per cell. Explicit trials take seven per cell: 2,520 measured queries plus 180 separately recorded warmups. Repetitions are not distinct tasks or engineering iterations.

| Implemented metric | Interpretation |
| --- | --- |
| Required Recall, Recall@5/@10, NDCG@10 | Deduplicated path + qualified-symbol identities. Delivery order is targets, repo-map items, then hot source; it is not internal candidate rank. |
| Non-Gold Symbol Fraction | Returned identities outside that task's required/useful set, not a claim about irrelevant tokens across the entire response. Reports retain its mean separately from recall so noisy retrieval cannot hide behind high coverage. |
| Bug-relevant Evidence Recall | Required identity and complete-body recall restricted to authored mutation fixtures. It measures whether evidence around a known changed defect pattern is exposed, not whether wcode autonomously discovered a bug. |
| Complete Body Recall | Full authored Gold fragments actually delivered with original bytes, matching path/SHA/line bounds and explicit non-redacted status. |
| All Required Edit Inputs | Every required target has current SHA, full source and writable conditions. This does not include independently mapped or executed verification. |
| Reported Edit Ready | The tool's own state, retained separately rather than trusted as the evaluator. |
| Syntax graph | Four simple call-graph fixtures, explicitly Tree-sitter/syntax rather than live LSP. |
| Cost | Serialized bytes, bytes/4 token estimates, raw query times and descriptive cold/warm percentiles. |

Nine real controls cover four syntax call graphs, guarded edits/cache invalidation, read-only refusal, concurrent same-SHA writes, workspace boundaries and honest graph truncation. Scorer counterexamples challenge wrong-path names, duplicate results, empty denominators, stale/missing SHA, redaction, invented/incomplete bodies, incorrect lines, CRLF and failed-query accounting.

## Interpreting results

Finding a symbol is not equivalent to delivering its body. A tool state saying that at least one target can be edited does not establish that all necessary inputs have arrived. Undefined precision-like denominators remain `null`, not 100%.

Cold means a new ToolHarness, **not cleared OS filesystem caches or process-global language configuration**. Warm means the same Harness after an identical-query warmup. Query timing excludes compilation, temporary repository creation, Harness construction, grading and report writing. With seven samples, nearest-rank p95 equals the maximum; shared-machine latency is descriptive, not a reliable tail estimate or a CI threshold.

Reports retain corpus/evaluator/test-binary fingerprints, Git HEAD, dirty state, observed before/after source snapshots and environment. Stable runtime source snapshots do not prove compilation-input identity, so the actual executed binary is fingerprinted separately. Concurrent input changes must be visible through `source_stable_during_run=false`.

Version 2 path-qualified and source-verified scores are not numerically comparable to the old name-only scorer. Version comparisons require the same corpus, scoring protocol, build profile, budgets, hardware and cache definitions, pairing the same cases rather than comparing only mixed-task averages.

## Counterexamples and contract v3

The checks challenge wcode and the evaluator independently. Frozen corpus membership is not changed to make an optimization look better. Additional network-protocol fixtures retain nine real callers, reject a disconnected component even when its nodes have nonzero degree, preserve transitive task-linked nodes under 48 reordered seed/input combinations, and keep unanchored exploration available. A same-Workspace cold/warm check compares each originally delivered Gold body rather than trusting aggregate counts.

Reducing the focused RepoMap cap from twelve to six failed the nine-caller counterexample. The runtime therefore keeps its bounded capacity and filters by task evidence and graph reachability instead. An exact-cache hit is not sufficient to skip discovery when natural-language search qualifiers remain. These checks measure the stated fixtures, not arbitrary repository completeness.

Contract v3 validates each response against the requested experiment budget, not just the tool's self-reported limit. Failed answerable attempts contribute zero to average NDCG; `ranking_attempts` and `noise_response_samples` expose their different denominators. Undefined human-readable percentages are `N/A`, matching JSON `null`. Non-Gold symbols may include useful but unannotated tests or transitive evidence, so their fraction is not automatically an irrelevant-code rate. The JSON container schema remains version 2; the measurement contract and evaluator fingerprint identify the changed scoring protocol. Regrade older error-containing reports before comparing NDCG.

## Tight-budget source delivery

Independent packet-decoder fixtures check that a 2K request delivers the short target and its cross-file caller, with original source and matching file SHA, across Rust, Go, TypeScript and Python. Read-only workspaces retain readable evidence without gaining write permission. A separate three-body fixture verifies that selecting an unseen relationship neither stops at an already delivered body nor evicts an existing ranked body.

The runtime prepares two ranked bodies at 2K and allows bounded related-source expansion, while keeping the final serialized budget unchanged. Redundant signature and call-summary metadata is compacted before source and file preconditions. CRLF/Unicode long-body checks exercise 1K–4K budgets and one/four Harness slots. A per-Gold 2K cold/warm check rejects lost evidence. These tests do not promise that every long function fits a small budget, and the frozen 60-case corpus and Contract v3 scoring remain unchanged.

## Delivery diagnostics and byte metrics

Reports now include `breakdown.language`, `breakdown.category`, `breakdown.target_count` and `breakdown.misses`. Every partition reuses the same aggregation, including failed queries. Each score retains `delivery`: missing final identities, identified targets without complete bodies, missing current file SHA, and unavailable write inputs. These flags can overlap; intentional read-only conditions are not failed writes. A final missing identity does not prove an internal search failure.

`complete_gold_density` divides the union of original UTF-8 byte ranges for fully verified required fragments by serialized response bytes. Duplicated or overlapping/nested Gold spans count once per file, and repeating a response body cannot improve the numerator. Partial, invented, redacted or stale bodies earn no full-fragment bytes. This response-only metric excludes unavailable responses and reports its sample/byte denominator; it must be read alongside failures and recall. Its complement is NOT a noise rate: safety metadata, useful non-Gold context and partial source are not credited here. `fresh_sha_recall` includes failed queries as zero hits. `complete_bodies_per_1k_budget_tokens` uses allocated budgets for all answerable attempts, not provider token usage.

`raw_required_source_bytes` is an uncompressed source-only lower bound. More than `4 * requested_budget` bytes proves the original source cannot fit under the current bytes/4 contract; a smaller bound does not prove feasibility once JSON and mandatory metadata are included. No-answer/invalid annotations remain unavailable. Long-body counterexamples test this distinction.

Save a one-sample-per-cell diagnostic snapshot without model calls:

```sh
cargo test --locked --lib engineering_fitness_diagnostic_snapshot -- --ignored --nocapture --test-threads=1
```

The snapshot prints real create-only JSON/Markdown paths and a 1K cold miss map. Full trials retain seven samples per cell. These are additive delivery metrics v2; the frozen 60 tasks and primary Contract v3 scores are unchanged.

Independent 1K checks cover three tiny functions in one or three files, cross-file caller evidence, explicit module requests, same-name source/test definitions, oversized UTF-8/CRLF bodies, and twelve escaped-query variants. The runtime prepares a bounded relevant pair even at 1K. It compacts redundant routing, ranking and matching-signature metadata before discarding evidence; retained bodies keep their existing file SHA/readonly entries. The temporary query echo is excluded conservatively from packing cost and removed before final byte accounting; it cannot buy extra delivered bytes. Explicit targets and long-source truncation remain protected.

## Multi-target collisions and body-state diagnostics

Independent packet fixtures now cover a three-stage chain in four languages and four Rust functions whose names collide with their module declarations. Candidate selection reserves one definition per requested literal before duplicate matches consume the bounded slots. Ordinary implementation requests prefer a function over its same-named module; an explicit module request retains the module. This does not increase candidate limits or establish unambiguous semantic resolution.

Under tight budgets, target summaries may inherit provider and precision from `provenance_defaults.targets` and omit redundant kind, language and line ranges only while a same-ID, same-path, current-SHA full unredacted body carries the exact range. Missing, stale, mismatched or partial evidence cannot justify this compaction; explicit provenance overrides stay intact. If that body must subsequently yield, its original coordinates return to the target for follow-up reads. Empty optional sections and late explanatory selection labels yield before source; all delivered bytes remain within the original budget.

Delivery v2 partitions each required identity into complete, absent, verified partial-original, or unusable body evidence. A partial body must match the current SHA, original bytes and actual line bounds, overlap the authored fragment, and be explicitly unredacted. It receives no complete-body or complete-fragment byte credit. A valid full copy wins over an invalid duplicate. Failed queries are counted separately as `unobserved_body_due_to_error_count`, never as successful absences. JSON and Markdown retain all five counts; the successful body states plus unobserved errors sum to the required denominator.

## Not measured yet

Live LSP definition/reference/implementation accuracy, complete network MCP behavior, independent verification-mapping quality, model understanding, autonomous bug discovery, generated-patch correctness and CPU/RSS remain unmeasured. Existing fast-context, repo-rank and IO microtrials retain their independent commands; this report does not claim to execute or merge them automatically.
