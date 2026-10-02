---
layout: docs
title: Research-informed improvements
description: Research-informed context retrieval, tool orchestration, model-host efficiency, verification targeting, and explicit implementation limits.
lang: en
alternate: /zh/docs/research-upgrades/
permalink: /docs/research-upgrades/
---

# Research-informed improvements

## 2026-09-28: evidence-local code-to-test retrieval (working tree)

Selection criteria: a primary paper or maintained implementation, concrete repository-level evaluation or inspectable code, a mechanism relevant to wcode, and a falsifiable local acceptance test. These sources are not a cross-model leaderboard; their reported success rates and costs do not transfer to wcode.

| Source | Useful evidence and limitation | Decision for wcode |
| --- | --- | --- |
| [SWE-agent, NeurIPS 2024](https://arxiv.org/abs/2405.15793) | Evaluates agent-computer interface design for repository navigation, editing and execution. Results depend on its model, tools and tasks. | Retain bounded edit-ready interfaces and measure what the agent actually receives. No new agent loop. |
| [Agentless, 2024 revised paper](https://arxiv.org/abs/2407.01489) and [official implementation](https://github.com/OpenAutoCoder/Agentless/blob/main/README_swebench.md) | Hierarchical localization, repair and regression/reproduction validation provide a concrete simple baseline. Python SWE-bench Lite results do not establish Rust or multilingual performance. | Localize to individual symbols and preserve both reproduction and existing regressions. |
| [SWE-bench, ICLR 2024](https://arxiv.org/abs/2310.06770) | Real issue/patch tasks and executable evaluation motivate separating retrieval quality from issue resolution. Its original corpus is Python. | Local synthetic context checks remain diagnostics, not model task-success claims. |
| [SWE-ContextBench, 2026 preprint](https://arxiv.org/abs/2602.08316) | Studies experience reuse; relevant summaries and autonomous retrieval are different conditions. Oracle-related experience is not evidence of reliable automatic selection. | Keep verified, intent-conditioned experience; no unfiltered trajectory memory. Further transfer needs held-out evaluation. |
| [Aider repository-map implementation](https://github.com/Aider-AI/aider/blob/main/aider/repomap.py) | Tree-sitter and graph ranking supply budgeted context. Centrality or membership in a test file is not task relevance. | Keep the existing graph, enforce evidence-local test admission. |
| [mini-SWE-agent](https://github.com/SWE-agent/mini-swe-agent) | A small agent core is a useful simplicity baseline; advertised scores vary with model and evaluation setup. | No additional planner, model dependency or fixed multi-agent orchestration for this change. |

The reproduced defect was in code-to-test membership, not missing PageRank: an exact source target still allowed every test-path symbol to start a new graph neighborhood. In the initial fixture (one target, one real test, forty unrelated tests), the old 4,000-budget response contained ten unrelated tests among twelve repo-map items, with the relevant test ninth.

The fix keeps test-path-only seeds for unanchored exploration. With an exact target, test candidates need bounded graph reachability, existing Design/verified-experience evidence, or a target-text match inside that symbol. The existing bounded supplemental search collects match lines in the same traversal for code-to-test requests; matches are checked against symbol ranges and source revisions. This recovers macro assertions and target-named regression candidates without promoting the rest of their file. Text-only candidates use reason `test_target_text_match`; they create no call edges and prove no execution or test coverage. Comments, strings and same-line constructs remain possible heuristic matches. Partial, redacted, truncated or stale observations cannot establish complete coverage.

A follow-up counterexample put 700 target mentions in an early documentation file: the former content-result prefix omitted a later macro test. The internal context search now reserves the first hit of each pattern across a bounded prefix of matching files during that same scan. Global query coverage remains first, file representatives follow, then remaining prefix rows. The existing scan, retained-excerpt and response byte limits still apply; ordinary search ordering and pagination are unchanged. Chinese/English, cold/warm, single/two-target regressions recover the later tests without admitting the unrelated test. Multiple matches within one file or too many matching files can still be omitted, so this is an explicitly partial sample, not exhaustive test discovery.

A further audit reproduced three boundary defects: JSX/TSX `.test`/`.spec` files lost text-only candidates; literal discovery prematurely discarded explicit targets after the fourth even though its parser and query plan allow eight; and omitting targets from the four-query supplemental scan failed to mark the graph partial. Literal discovery now uses the existing eight-query envelope, JSX/TSX follow the same test-path policy, and supplemental target omission stays visible. Cold/warm regressions check four, five and eight exact targets plus all four JSX/TSX filename forms. The supplement still searches at most four targets; this change does not imply complete recall for all eight.

Verification commands: `cargo test --locked --lib context_search_`, `cargo test --locked code_to_test_ -- --nocapture`, the existing repo-map/fitness regressions, and the full repository gate. Tests cover Chinese/English cold/warm retrieval, macro/name candidates, unrelated same-file tests, stale/redacted/invalid search rows, preserved Design/experience anchors and unanchored exploration. Search checks also cover rare patterns, unique rows, one scan/read per file, unchanged public pagination, original SHA, redaction, clipped lines and unreadable/oversized files. Test definitions are not execution evidence; consult the current run results.

Next measurable slice: a frozen, repository-disjoint issue holdout with the same model, budget and environment for baseline and candidate; report patch success, first-correct-file, delivered irrelevant context, tool/read cost and latency separately. The current synthetic fixture does not establish a global percentage gain. Do not add more retrieval engines or memory mechanisms until that comparison identifies a concrete gap. Existing tunnels, resource budgets, authorization and release behavior are unchanged.


## 2026-09-17: source-derived counterexample candidates (working tree)

The opt-in `review_changes(adversarial=true)` now supplements the question packet with `candidate_search`. It inspects at most six distinct changed source files, admits files up to 256 KiB by guarded metadata, visits at most 32 matching AST nodes per file, and returns at most three mutation proposals. The existing Tree-sitter index supplies real node identities; strings and comments do not become code candidates. Boolean literals can be flipped, and narrowly admitted simple comparisons can switch equality or boundary operators. A comparison such as `count < 8` additionally proposes the symbolic binding values `7`, `8`, `9`; its actual parameter binding and type domain remain unvalidated. Checked arithmetic avoids overflowing the signed 64-bit boundary-value generator.

Each proposal carries the source SHA, exact original/replacement bytes, byte offsets, line bounds and `proposed-not-typechecked` status. Security-attributed files rank first, followed by a static cost heuristic and deterministic path/byte order. This is not a measured defect probability or latency estimate. Duplicate and reordered file lists cannot change the selected proposals. The scan covers changed files, not only changed diff hunks. Missing, denied, oversized, unsupported or malformed sources and all output limits remain explicit; an empty match list is checked against file-level parse status instead of being called a successful analysis. Workspace, symlink, redaction and stale-SHA checks are preserved. AST work uses a real bounded tool permit on a blocking worker; ordinary review performs no candidate scan. An invalid non-boolean `adversarial` flag is rejected rather than silently disabling review.

[Hypothesis stateful testing](https://hypothesis.readthedocs.io/en/latest/stateful.html) separates generated actions and their preconditions from an independently specified invariant/model; [cargo-mutants](https://mutants.rs/) seeks test gaps by introducing changes. Here those ideas inform proposal boundaries, not a new autonomous execution engine: `executed=false`, `oracle_required=true`, and syntax precision remain explicit. Mutants must be typechecked in an isolated copy after a nonempty passing baseline. Build failures, missing tests and timeouts are not kills; survivors require reachability/equivalence review. No candidate, or one killed mutant, proves neither overall correctness nor closure of unrelated QA claims.

Run `cargo test --locked --lib harness_quality -- --nocapture` and the `review_changes_adversarial_mode_attaches_non_evidence_questions` test. A frozen Rust fixture actually compiles and runs one passing baseline test and one failing `<` to `<=` mutant using generated boundary values and an independent expected-value table, while leaving its source Workspace unchanged. This fixture is not an automatically executed Mutation stage on the user's project. Four-language checks exercise Rust/Go/TypeScript/Python AST candidate extraction, not four-language compilation. Arbitrary argument synthesis, domain inference, concurrent schedule generation and automatic experiment adjudication remain outside this slice.

## 2026-09-17: adversarial QA as falsification, not self-confidence (working tree)

This working-tree change adds an adversarial mode to `review_changes` (`adversarial=true`), a bounded model-free challenge layer over the same deterministic review facts. The design follows a simple rule: a reviewer question may expose an assumption, but it cannot close itself. OpenAI's [Harness engineering](https://openai.com/index/harness-engineering/) describes additional agent review and feedback loops as engineering infrastructure rather than a reason to trust one first-pass output; [Codex Security](https://openai.com/index/why-codex-security-doesnt-include-sast/) emphasizes validating whether a control actually guarantees the intended property instead of merely recognizing familiar code shapes. Self-Refine shows that feedback/revision can improve some model outputs, while [How Much LLM Does a Self-Revising Agent Actually Need?](https://arxiv.org/abs/2604.07236) is a useful warning that more LLM revision is not monotonically better. wcode therefore externalizes the challenge protocol and keeps proof outside the critique loop.

The tool turns current change-review signals into at most twelve unique falsification questions. Baseline questions challenge product acceptance, hidden impact, current-revision evidence freshness and negative paths. When source and tests change together it challenges test overfit; source-without-test, security-sensitive, manifest, deleted-test, generated-output, large-change, untracked-file and truncated-review signals add targeted questions. Each item states the claim under challenge, a counterexample question, the current deterministic signal, the evidence needed to close it and the wcode tools that can gather that evidence.

The packet is explicitly `challenge-packet-not-evidence`. It is not persisted as a Pass, does not raise semantic precision and cannot override a deterministic failure. Each question also carries a bounded counterexample experiment: an experiment kind, relevant changed targets, a falsifiable hypothesis, the concrete failure condition, the tools or Verification stages that can execute the experiment, and the evidence class that can close the question. These are executable hypotheses, not claims that Mutation/Property/Fuzz/Runtime work has already run. For changes that warrant model review, the packet points into the existing Verification Mesh: create a verification plan and claim a blind reviewer with role `adversarial`. That review remains a distinct Evidence producer, while deterministic verification, Property/Mutation/Fuzz/Runtime stages and HumanApproval retain their existing fail-closed rules. A clean tree produces no adversarial questions, so the feature does not force an endless self-critique loop when there is nothing to review.

Run `cargo test --locked --lib adversarial_qa_ -- --nocapture` for the deterministic regressions. They verify the non-Evidence contract, baseline challenge categories, targeted questions for review findings, uniqueness, hard bounds and clean-tree silence. These tests measure the challenge protocol itself; they do not claim that a model will answer every question correctly or that repeated critique beats Kimi, Claude Code or Codex on coding tasks.

## 2026-09-17: diagnostic formats to guarded edits (working tree)

The existing `agent_context` query now understands Python `File "src/worker file.py", line 37`, compiler `src/worker.ts(23,7)` / `src/service.cs(23,7)`, and PHP-style `in src/handler.php on line 29` locations. Quoted paths preserve spaces and Unicode, including a quoted complete `file:line:column` inside presentation brackets. This is input normalization for the existing source/SHA path, not a new tool or an automatic root-cause conclusion.

Line markers are bound to their physical diagnostic line. Duplicate locations are removed before the existing four-anchor limit. Invalid structured coordinates stay invalid rather than falling back to line one; URLs, malformed quotes and oversized quoted tokens cannot produce a local filename suffix. No URL decoding, shell execution or permission changes are introduced. Existing Workspace boundaries, protected paths, symlink rejection, source redaction and stale-SHA rejection remain enforced. This is a bounded parser for the documented shapes, not every language's stack-trace syntax; unknown formats still require an explicit location.

Run `cargo test --locked --lib trace_format_ -- --nocapture` for the deterministic regressions in `tests/unit/runtime/harness/diagnostic_formats.rs`. Twelve tests cover a 72-combination Python-frame matrix, compiler/PHP forms, quote/bracket nesting, invalid coordinates, duplicate/cap behavior and path boundaries. A local Harness fixture calls `agent_context` once at each 1,000/1,400/4,000 budget, receives line 37 plus the source SHA, applies a guarded edit from that response and rejects the old SHA afterward. This tests the retrieval-to-edit protocol, not an actual Python execution, remote MCP latency or model coding superiority.

## 2026-09-17: global graph budgets and construction cost (working tree)

[DyRetriever](https://arxiv.org/abs/2608.01927), submitted 2026-08-03, motivates retrieving dependencies on demand instead of always paying for a complete static graph. [Agent Retrieval Bench](https://arxiv.org/abs/2607.24882) separates repository retrieval from patch success. This implementation retains wcode's deterministic syntax graph and existing budgets; it does not add the paper's LLM-driven graph construction or transfer its reported speedup.

Two executable regressions showed that per-file priority was insufficient: unrelated definitions in the first priority file could exhaust the budget before the next exact target, and many callers before an exact definition in the same file could exclude that definition. Selection now reserves explicit targets globally, then direct callers, then remaining definitions. Tests cover multiple file orders, a 6,200-filler fixture with a 5,000-symbol graph budget, and a budget smaller than the number of requested definitions. Limits remain hard and omitted definitions remain reported as truncation.

The private syntax builder appends already-deduplicated edges and validates the complete graph once, rather than using a full growing-list duplicate scan for every edge. The general public graph insertion API is unchanged. Supplemental graph merging uses a transient set keyed by endpoints, edge kind and complete provenance; it preserves existing order and independent evidence, rejects conflicting revisions for overlapping nodes, and still validates endpoints and self-edges before returning. This guards the overlapping sources, not an atomic snapshot of every repository file.

Reproduce the opt-in stage benchmark with `cargo test --release --locked --lib graph_build_release_cost -- --ignored --nocapture`. It uses 500 and 5,000 definitions, two warmups and eleven retained warm-cache samples; serialization is outside timing and every sample is checked against the first result. It reports median/p95, graph size and edge counts, not model inference, cold repository discovery, MCP latency or patch success. No elapsed-time threshold determines CI success. The local pre/post record in `tests/unit/graph/budget_perf.md` reports 5,000-definition construction at 82.281 / 83.585 ms p50 / p95 before versus 10.629 / 12.263 ms after; graph counts and serialized byte lengths match. These were separate builds on a shared workspace, not an alternating paired experiment.

## 2026-09-17: exact-target ranking and graph reuse (working tree)

This is unreleased source work. [Aider's repository map](https://aider.chat/docs/repomap.html) motivates graph-ranked bounded context; [SWE-Explore](https://arxiv.org/abs/2606.07297) evaluates ranked code regions under a fixed line budget, and [Agent Retrieval Bench](https://arxiv.org/abs/2607.24882) separates context acquisition from patch success. These sources motivate measuring ordering, coverage and cost separately; this change does not reproduce their datasets or claim improved model benchmark scores.

A 14-symbol star-graph regression exposed a central helper ranking above the exact symbol named in the query. Exact targets now have explicit precedence over graph popularity. Remaining ordering keeps the existing relevance score, direct-seed preference, qualified name and path, with canonical symbol ID as a deterministic tie-break. Already-given review/frame files are excluded before selecting the prefix. The [Rust standard library selection API](https://doc.rust-lang.org/std/primitive.slice.html#method.select_nth_unstable_by) partitions candidates before sorting only the returned prefix: O(n + k log k) selection work instead of sorting the entire list. Regression tests compare ordered IDs against a full sort under this same corrected precedence, including empty inputs, ties, reversed inputs and limits around the input length.

No-op relationship augmentation returns a borrowed snapshot. It allocates an owned graph only for actual supplementation; the upstream cache fingerprint, source freshness validation and truncation reporting remain in force. Pointer-identity tests prevent accidentally restoring a full clone in this path.

Reproduce with `cargo test --release --locked --lib repo_rank_release_cost_comparison -- --ignored --nocapture`. The opt-in microbenchmark uses 3 warmups and 31 retained samples, identical prepared candidate buffers, alternating selection order and ordered-ID equality checks. Both selection variants use the corrected comparator so the timing isolates the selection algorithm, not a change in ranking quality. A local 2026-09-17 run used Rust 1.98.1, aarch64-apple-darwin:

| Candidates; keep 16 | Full sort p50 / p95 (microseconds) | Top-K p50 / p95 (microseconds) |
| --- | --- | --- |
| 128 | 34.833 / 35.625 | 15.750 / 16.709 |
| 6,000 | 1,306.583 / 2,205.125 | 299.875 / 546.083 |
| 12,000 | 2,066.041 / 2,219.583 | 471.917 / 505.417 |

The separate complete-graph case has 5,000 nodes: copying took 1,230.417 microseconds p50 and 1,298.084 p95; the no-op path returned the original snapshot. The benchmark derives the fixture size from the actual symbol cap and asserts no truncation, rather than suppressing legitimate recovery to manufacture a no-copy result. Candidate counts above the runtime graph cap exercise the selector independently, not a promise to return that many symbols. These are stage-level local measurements, not end-to-end agent latency, network performance, memory-allocation accounting, code-generation quality or a Kimi/Claude/Codex comparison. Timing is never a CI pass threshold.

## 2026-09-17: competitive code I/O follow-up (working tree)

This follow-up is source work, not a published-release claim. A newly built runtime and refreshed tool catalog are required. The running connector may still advertise the older exact-search schema.

The comparison is capability-based: [Kimi's official tools](https://www.kimi.com/code/docs/en/kimi-code-cli/reference/tools.html) provide ripgrep-backed regex search, output modes, pagination and stale-read edit checks; [Claude Code subagents](https://code.claude.com/docs/en/sub-agents) isolate exploration context; the [Codex prompting guide](https://developers.openai.com/cookbook/examples/gpt-5/codex_prompting_guide) recommends batching known reads and using a well-defined patch interface. These are documented tool/workflow properties, not controlled rankings of model coding accuracy or generation speed.

`search_code` accepts a string or an array of up to 32 queries. One string defaults to automatic exact-first matching with token-AND fallback only when no exact lines were found; arrays default to exact matching. Both paths read each scanned file once in one traversal. Explicit `regex`, `tokens_all` and `tokens_any` remain available. `search_many` defaults to exact and `scan_patterns` to regex; neither silently enables array-auto fallback. Regex matching is line-oriented, including anchors; multiline regex and general glob filters are not added in this follow-up.

Search returns unique matching lines, `queries` provenance, original-file `sha256`, per-query matched-line counts, and separate `scan_truncated`, `results_truncated`, `failed_files`, `skipped_files` and `coverage_complete`. Counts are matching lines, not regex occurrence counts or proven bugs. Rare-pattern representatives precede the remaining path/line order. `offset` and `next_offset` page current file contents; `auto_page=true` raises the bounded one-call result budget to 2,000 while ordinary calls keep the smaller economy page. Edits between calls can shift results, so this is not an atomic repository snapshot. Budget exhaustion without a continuation requires narrower paths/patterns rather than a claim of exhaustive coverage.

`search_syntax` now defaults to repository-wide discovery up to 50,000 source files instead of the former 1,000-file default. Comment AST nodes are excluded unless `include_comments=true`. Go syntax hits also expose guard evidence and bounded bug-pattern signals. `scan_patterns` keeps text-regex mode, skips pure comment lines/blocks by default, and accepts `preset=go_common_bugs` or one `pattern` for AST-validated `nil_deref`, `err_swallowed`, `index_mismatch`, `empty_test`, and `unguarded_subscript` candidates. Pattern filtering happens before the retained-result budget is consumed; `coverage_complete=false` still means callers must not claim exhaustive absence.

`output_mode` can be `content`, `files_with_matches` or `count_matches`; both body-free modes return file paths, SHA and matched-line counts. `scan_patterns` merges overlapping context into each file's `context_lines`. A source SHA from a search can supply the existing guarded edit precondition without a redundant file read, but agents must still inspect enough context to justify a change. Redacted or clipped text must not be treated as a complete original body. Search uses existing protected-path and source-read checks and imposes file, byte, regex compilation, retained-result and response budgets.

Validated multi-edits are assembled into one output buffer in original-range order instead of repeatedly moving the document tail. Unique anchors, original line bounds, overlap rejection, output-size bounds and stale SHA rejection remain intact. AST searches now include source SHA and report early-stop/read-failure conditions rather than claiming complete coverage.

Reproduce the opt-in local benchmark with `cargo test --release --locked --lib competitive_io_benchmark -- --ignored --nocapture` (requires installed `rg`). It checks a 768-file, three-pattern fixture against ripgrep's actual path/line result set, samples batched versus repeated queries, reports JSON bytes for body/full and files-only responses, and compares 128 edits on the same approximately 1 MiB document. Search uses 15 warm-cache samples and edit assembly 31, with alternating execution order and median/p95 reporting. The ordinary test suite ignores this environment-dependent benchmark; explicitly running it is separate evidence. This measures neither model inference, remote MCP latency nor end-to-end task pass rate. Different security checks and output formats mean the raw-ripgrep timing is a reference, not an identical-workload claim.

## Status and scope

These changes are included in the [v0.6.2 release preparation](../releases/v0.6.2/), not retroactively added to v0.6.1. A newly built runtime and refreshed tool schema are required. In particular, never send `dry_run` to an older runtime that does not advertise that field: ignoring an unknown argument is not a safe preview.

The implementation extends `agent_context` and `parallel_tools`. It adds no model backend, embedding service, vector database, production dependency, or autonomous permission grant. Research was reviewed on 2026-09-10, including a paper submitted on 2026-09-08; this is a focused selection, not an exhaustive literature survey.

## Research decisions

| Primary source | Finding relevant to wcode | Implementation decision and limit |
| --- | --- | --- |
| [Agent Retrieval Bench](https://arxiv.org/abs/2607.24882), 2026-07-27 | Repository context acquisition deserves evaluation separately from patch success; different retrieval signals favor different methods. | Add file-and-line anchors and test exact source exposure, missing locations and bounded packs. No claim of reproducing its benchmark or proving a stack frame is the root cause. |
| [Authority Is Not a String / CapScope](https://arxiv.org/abs/2609.08371), 2026-09-08 | Capability checks outside model context can limit actions induced by repository text and tool output. | Keep locations and previews as data, not authority. Preview never consumes or grants authorization. This is not an implementation of CapScope's per-agent capability system or a claim of prompt-injection immunity. |
| [The Complexity Trap](https://arxiv.org/abs/2508.21433), 2025-08-29, revised 2025-10-27 | In its SWE-agent experiments, simple observation masking was competitive with model-generated summaries. | Use bounded original excerpts instead of adding a summarization model. We do not implement trajectory masking here or transfer the paper's cost reductions to wcode. |
| [Do Context Files Help Coding Agents?](https://arxiv.org/abs/2607.27250), 2026-07-28 | A small two-agent ablation did not detect a correctness gain from context files; statistical power limits that conclusion. | Keep mandatory instructions short and make richer context demand-driven. This does not establish that repository instructions are useless. |
| [LLMCompiler](https://arxiv.org/abs/2312.04511), ICML 2024; first submitted 2023-12-07 | Separating planning, task dispatch and execution enables dependency-aware parallel calls. | Expose a no-execution view of the existing scheduler graph. Do not copy reported speedup multipliers or add another model-based planner. |

## 2026-09-15 follow-up: retrieval precision and model-host efficiency

The v0.7.2 follow-up keeps the control plane model-neutral, but deliberately optimizes the interfaces used by current coding models and Hosts. The decision is capability-first rather than vendor-first: a Host may use deferred tool search, prompt caching, native parallel calls, all of them, or none of them. Core repository semantics, authorization and verification do not change based on a model brand string.

| Primary source | Current signal | wcode decision |
| --- | --- | --- |
| [Agent Retrieval Bench](https://arxiv.org/abs/2607.24882), 2026-07-27 | No retrieval family dominates every coding task; budgeted context yield and next-needed files matter independently from final patch generation. | Keep `agent_context` adaptive and task-routed, expose candidate→delivery efficiency, and prefer exact anchors/relationships over simply increasing context size. |
| [ContextBench](https://arxiv.org/abs/2602.05892), 2026-02-05 | Coding agents tend to over-retrieve and there is a measurable gap between explored and actually used context. | Preserve bounded context packs, avoid mandatory second-pass repository dumps, and measure delivered context rather than treating recall alone as success. |
| [CORE-Bench](https://arxiv.org/abs/2606.11864), 2026-06-10 | Agentic repository retrieval is materially different from isolated snippet search. | Keep repository-state, issue-to-edit and broader-context routing explicit instead of replacing it with one generic embedding query. |
| [Anthropic advanced tool use](https://www.anthropic.com/engineering/advanced-tool-use) | Deferred tool discovery can reduce tool-definition context and improve tool selection when catalogs are large. | Mark only a small wcode core as `dev.wcode/preloadRecommended=true`; specialist tools remain discoverable on demand. This is an advisory `_meta` hint, not an Anthropic-only dependency. |
| [OpenAI Codex agent loop](https://openai.com/index/unrolling-the-codex-agent-loop/) and [Agents API](https://openai.com/index/introducing-the-agents-api/) | Exact stable prompt/tool prefixes improve cache reuse; tool search and programmatic orchestration reduce unnecessary tool context. | Keep tool order and server instructions deterministic, keep the catalog compact, and use one stable core preload set rather than model-specific catalogs. |
| [Gemini context caching](https://ai.google.dev/gemini-api/docs/caching) | Repeated stable prefixes improve implicit cache-hit opportunities. | Keep static instructions and tool definitions stable; dynamic repository state stays in tool results and Agent Context rather than being baked into definitions. |

The preload hint does not mean a tool call result is cache-safe, idempotent, or permission-free. Standard MCP annotations remain the source for read-only/destructive/idempotency semantics, and wcode still performs its own runtime policy checks. Hosts that ignore the hint receive the same deterministic full catalog and behavior.

The same release narrows advanced Verification Mesh targets conservatively. High/Critical plans prefer source files that already carry deterministic risk attribution, while CSS/HTML presentation sources do not manufacture language-level Property/Mutation/Fuzz targets unless an explicit advanced executor opts them in. Full deterministic verification and Security/Adversarial review stay intact. Missing real Rust mutation or fuzz executors remain explicit gaps; ordinary `cargo test` is never relabeled as mutation or fuzz evidence.

## 2026-09-15 polyglot quality follow-up

The polyglot pass cross-checked current ecosystem contracts instead of treating Rust conventions as universal. Dart documents `dart analyze` as static analysis and its analyzer also emits lints and type-system diagnostics, so one real analyzer run can honestly cover lint/type/static dimensions without being executed three times. Prettier documents `--check` as non-writing CI validation, while `--write` is the mutation mode. Vitest documents `vitest run` as the single-run non-watch mode; Jest documents `--runInBand` as a bounded serial test run. Biome exposes independent formatter/linter switches, so the registry uses distinct format and lint checks rather than executing the same broad `biome check` twice. Standard Ruby's normal `standardrb` command reports violations while fixes require `--fix`; R's startup documentation shows that `.Renviron`/`.Rprofile` can be loaded by default, and styler documents `dry="fail"` as non-writing. Therefore built-in R quality/LSP commands use `--vanilla` and the R formatter gate uses styler dry-fail.

Primary references: [Dart analyze](https://dart.dev/tools/dart-analyze), [Dart analysis/lints](https://dart.dev/tools/analysis), [Prettier CLI](https://prettier.io/docs/cli), [Vitest CLI](https://vitest.dev/guide/cli), [Jest CLI](https://jestjs.io/docs/30.0/cli), [Biome CLI](https://biomejs.dev/reference/cli/), [Standard Ruby](https://github.com/standardrb/standard), [R Startup](https://www.stat.ethz.ch/R-manual/R-devel/library/base/html/Startup.html), and [styler `style_pkg`](https://styler.r-lib.org/reference/style_pkg.html).

The same audit tightened advanced-stage truthfulness: built-in Property discovery now requires framework declaration plus matching-language source usage. JS/TS fast-check uses a fixed Vitest/Jest runner; arbitrary `test`, `mutation`, or `mutate` package scripts cannot mint advanced Evidence. JS/TS Stryker remains an explicit executor configuration because its repository configuration can execute JavaScript. This deliberately favors explicit gaps over broad but unverifiable green coverage.

## Diagnostic context

The 2026-09-17 source update recognizes explicit CJK punctuation around location tokens, for example `修复：src/worker.rs:120，检查：src/model.rs#L9`. Diagnostic snippets now use the same original UTF-8 prefix and actual returned line bounds as direct symbol snippets; truncation is metadata, not an inserted ellipsis. Unredacted `read_file` windows preserve interior LF, CRLF and mixed separators while keeping the existing omitted final terminator convention. Sanitized text still carries `redacted: true`; it must not be treated as original bytes for an edit.

Even under tight budgets, a retained snippet keeps its path, symbol ID when present, SHA, provenance, line bounds and redaction state. Readiness binds a usable nonempty, unredacted body to a direct target and its writable file SHA; unrelated or stale bodies cannot independently mark that target ready. Partial target delivery is explicitly advisory, not a claim that every requested body is present. These changes do not grant permissions, prove a diagnostic's root cause, automatically fix test failures, or replace final verification.

Reproduce the deterministic regressions with `cargo test --locked --lib diagnostic_context_ -- --nocapture`. The fixtures cover punctuation and path boundaries, tight-budget metadata, redaction, LF/CRLF/mixed/EOF windows, and using a returned diagnostic excerpt directly in a guarded edit followed by rejection of the old SHA. This is a local correctness suite, not a model benchmark or an end-to-end latency measurement.

Use the existing `agent_context` query with an explicit location, for example `error[E0308] at src/runtime/harness/context/budget.rs:33:9`. Supported forms include `file:line`, `file:line:column`, `file#Lline`, and plain supported source/config/document filenames. Backslash-separated relative paths are accepted. Quoted whitespace-free tokens are supported; this is not a complete parser for every language's traceback syntax.

At most four distinct location anchors are considered. A line-bearing anchor selects the smallest syntax definition containing that line when the outline and file SHA agree. Unsupported syntax formats and plain-file anchors retain a deterministic file target rather than inventing a symbol. The excerpt starts at the supplied line and includes up to thirteen lines, with an initial 1,600-character cap; the existing token budget can reduce it further. File SHA and excerpt SHA must agree, and syntax facts are never relabeled as compiler semantics.

The `retrieval` field exposes `strategy`, `resolved`, `anchors` and guidance. Anchor states distinguish `resolved`, `unavailable`, `outside_boundary`, `invalid_location` and `changed_during_read`. Unavailable explicit locations do not promote unrelated lexical hits into edit-ready targets. Normal symbol queries without location anchors retain the existing retrieval path. A stack frame is an inspection location, not a proven root cause.

Paths still pass Workspace protections. Parent traversal, protected files and symlinks are not normalized into permission; absolute locations must belong to the selected root. Foreign CI checkout paths are not guessed by matching a suffix. Source and SHA are re-read for a new request, not reused as stale edit permission.

## Dependency previews

After the runtime advertises the field, pass `dry_run: true` to `parallel_tools` with the same tasks intended for execution. The preview uses the real preflight, coalescing rules and physical-root dependency graph, then returns before any child is dispatched. It works even when execution slots are occupied and does not queue a command or authorization request.

The response uses `execution: "dependency-preview"` and `tasks_executed: 0`. It includes zero-based task `index`, `depends_on`, `coalesced_into`, path counts, `waves`, `initial_ready`, concurrency bounds and coalescing counts. Numeric indices avoid echoing arbitrary IDs, file contents or payload secrets. Waves describe dependency structure, not whole-layer barriers; actual execution remains completion-driven.

`authorization_checked: false` and `file_preconditions_checked: false` are essential: a plan is not proof that writes are authorized, files exist, SHAs match, or every tool-specific argument is valid. Actual execution rebuilds and validates its own state. The preview does not infer logical dependencies that are absent from the resource model. `dry_run: false`, or omission of the field, keeps normal execution. Invalid flag types fail before child execution.

Preview is optional when dependencies are uncertain, not another mandatory call for every task. Independent top-level calls and known-input bulk tools remain preferred when their dependencies are already understood.

## Usage-driven workflow improvements

Explicit operator queries (`git commit`, `git status`, `提交`, `全量检查`) without Product Scopes use a compact operation-context route. It returns next actions without scanning symbols or Design State. Source questions such as `inspect commit` keep the ordinary code route. No fictional context-savings baseline is reported for a scan that was not run.

Agent Context also removes a repo-map signature before final sizing when that exact symbol already has complete, non-redacted current source and a matching delivered file SHA. This is metadata deduplication, not relationship inference: the repo-map row, ranking reason and relationships remain, while stale, redacted, truncated or identity-mismatched entries keep their metadata. Whole relation-free rows still yield only under actual budget pressure through the existing guarded compactor.

Before shrinking source under a tight budget, the compactor accounts for the serialized bytes needed to restore original symbol ranges. Follow-up coordinates therefore cannot turn a shrink into negative byte progress; SHA, original coordinates, and truncation flags remain explicit.

The MCP `tools/list` catalog no longer repeats a display `title` that was mechanically derived from each canonical tool `name`. Names, compact descriptions, input schemas, annotations and Product Scope metadata remain unchanged, and the full task-independent protocol catalog is still available. This reduces real serialized host-catalog bytes without pretending that the host loads only the task manifest; provider/model input, orchestration context and retry token accounting remain separate missing E2E measurements.

Verification now finishes the current independent phase and stops before later phases when a check fails. `skipped_checks` are not counted as executed, and never produce passing evidence. Set `fail_fast: false` on `verify_project` for exhaustive diagnostics. Large successful logs retain bounded test-result totals and a tail; failed logs keep the prior larger diagnostic allowance. `output_truncated` explicitly marks omitted output.

Common Git inspection forms (branch/tag listing, current branch and remote URL inspection) run through the existing read policy. Explicit branch creation/switching, lightweight or message-bearing annotated tags, and `git restore --staged -- <paths>` enter exact human approval rather than permanent denial. This is not unrestricted Git: forced replacement/deletion, broad pathspecs, shell/helper/config redirection and protected paths remain outside these supported forms. Malformed arguments or invalid working directories fail before generating a useless approval request.

The TUI displays the selected request's identity, Workspace and wrapped details; `PgUp`/`PgDn` scroll long summaries while approval controls remain visible. Existing request-ID binding and hidden-overlay protections remain. A client without form elicitation still requires operator approval in the TUI or protected WebUI; repository changes cannot add a missing client capability. Grouped command approval was evaluated but is not included in this implementation.

These choices borrow task-specific tool selection from [Anthropic's advanced tool use](https://www.anthropic.com/engineering/advanced-tool-use) and capability-preserving runtime control from [AgenTRIM](https://arxiv.org/abs/2601.12449), revised 2026-08-30. They are bounded engineering adaptations, not reproductions of those systems or transfers of their benchmark numbers.

Local acceptance fixtures compare two executed checks against five in exhaustive mode after a deliberately invalid Cargo manifest, require over 80% reduction for a synthetic successful test log while retaining its totals, and cap operation-context payloads at 4,000 bytes. These thresholds are executable test expectations, not a claim about end-to-end agent speed. Only a completed test run establishes whether the current revision meets them.

## Audit hardening

Git commit and annotated-tag message values are treated as literal text only after the complete command form is validated. A message mentioning `../migration` or `.env` does not read those paths and can request exact human approval. Actual path arguments, message-file options and control characters retain their checks; recognizing a message is not permission to execute.

Explicit `verify_project` level, timeout and fail-fast values are validated before dispatch. Invalid values are not silently replaced by defaults. Operation contexts report `workspace_exec_disabled` and no executable next actions when execution is disabled. Narrow authorization panels use shorter controls so both approve and deny remain visible, including at 40 by 10 cells.

Acceptance evidence is inconclusive when some required references were not executed, while a known failure remains a failure. Language-quality runs produce only their check evidence, not a whole-project verification pass or generic test-acceptance pass. Legacy whole-project language-quality records are excluded from deterministic gate aggregation.

Deterministic results use the latest record per producer and verification policy. A later quick pass cannot clear an earlier full failure, and independent producers' failures remain visible. A newer full run can replace that producer's older quick result because it repeats those checks. Equal-timestamp conflicting records fail closed. These rules remain scoped to the plan's exact code and Design revision.

## Command timeout diagnostics

Ordinary commands and repository executors share one result collector. A command timeout now returns a failed `CommandResult` with the captured, redacted stdout/stderr rather than discarding the diagnostic output. `timed_out` describes the command deadline, not an HTTP transport timeout. `output_incomplete` marks a pipe read failure or an unfinished drain; false means the emitted streams were drained, not that the requested command finished its intended work. `success` cannot be true on timeout or incomplete capture, even if an exit code of zero is observed during cleanup.

The collector owns both pipe-reader tasks, so cancellation cannot detach readers. Process cleanup and subsequent pipe draining each have a two-second grace bound. Failed results include `retry_guidance`: inspect effects before retrying. Existing authorization, process-group supervision, output redaction and execution slots remain in force. No rollback, automatic command replay or exactly-once guarantee is added. This follows the distinction between a lost response and absent side effects discussed in [AWS Builders' Library](https://aws.amazon.com/builders-library/making-retries-safe-with-idempotent-APIs/).

Regression fixtures run synthetic Rust executables in temporary workspaces: timeouts retain diagnostics and already-applied effects, cancellation prevents the delayed effect, large simultaneous streams remain bounded, normal exit behavior is preserved, and timeout output still passes redaction. The raw stream collector still retains a bounded prefix of up to 256 KiB per stream; head/tail retention is not included, so a final diagnostic beyond that bound can still be omitted. A terminated process is not proof that remote effects were undone.

## Concurrency and process queues

`SLOTS` is tool admission occupancy, not CPU core use; `PEAK` is the largest observed simultaneous tool occupancy since startup. A bulk read or edit remains one outer tool even when it processes many files internally. Low occupancy without queued independent work is not evidence that the scheduler is limiting throughput. Do not inflate these counters or create unnecessary work to fill them.

Foreground CPU work now scales to the minimum of available hardware parallelism, eight workers, the memory-derived bound and requested tool parallelism; the background CPU target remains unchanged. The blocking-thread ceiling is 64 so a batch of 32 independent blocking requests is not capped at sixteen threads. Batch file mutations use one shared bounded I/O pool, with sixteen workers at the default 512 MiB budget, rather than occupying the CPU indexing pool during filesystem waits. Reads retain the CPU pool: widening warm-read concurrency regressed the local paired experiment and was reverted. These are capacity bounds, not a promise of proportional speedup or a hard limit on descendant-process memory.

Exact fixed-form Git status and diff checks use a separate inspection queue. Other commands and repository executors use the memory/CPU-derived heavy-process queue; the balanced 512 MiB profile currently provides four heavy-process permits. Heavy-command execution admission is separately bounded to heavy-process capacity, so excess commands wait before taking a global Tool permit and a 32-slot outer tool surface does not admit 28 commands merely to queue behind four processes. Policy checks, user authorization, process supervision and resource-pressure admission still happen; changing the queue cannot approve a mutation or helper option.

Resource telemetry exposes `child_queue` and `probe_queue` with `active`, `limit`, `waiting`, cumulative `waits`, `total_wait_ms`, lifetime `max_wait_ms`, plus the latest wait and its age. Active values count reserved permits, not CPU-running processes; cancelled waits clean up their metrics. Runtime Drift uses only a recent wait that exceeds the five-second admission bound, while lifetime peaks remain descriptive history. The wide TUI header shows `PROC`, `GIT` and combined inner `Q`, separately from tool slots and their peak. No new authorization setting or automatic restart is involved.

Regression controls compare sixteen versus thirty-two started blocking workers, keep real Git inspection responsive while all heavy permits are occupied, exercise cancellation/closed queues/resource pressure, and compare guarded file batches without weakening SHA or per-file error checks. Benchmark fixtures report their actual worker count and raw times; test-build results are not production, model or network speed measurements. A newly built runtime must be started before these changes affect the live dashboard.

## Consistent retrieval and saturation handling

`symbol_context` compares the file body's SHA with the indexed revision before returning signatures, ranges or call relations. A mismatch invalidates the stale record and permits one bounded retry; a changed symbol identity or continued file churn returns an explicit error. Equal file size and modification time are not accepted as content identity. This guarantees consistency of the returned file context, not an atomic snapshot of the entire repository or permanent freshness after the response.

Cold `ensure_indexed`, single-query search and multi-query search share a per-Workspace-root/file build flight. A waiter holds neither the global index-state lock nor a CPU permit, rechecks the cache after acquiring the flight, and reuses a successful build. Warm cache hits keep the fast path. At most 256 independent live flights are registered; inactive entries are reclaimed and active ones are never evicted to satisfy capacity. File/prefix invalidation and aggressive memory trimming invalidate in-flight publication generations. Failed reads or an unwound empty coordination lock do not permanently block subsequent builds. This is duplicate-work suppression, not incremental Tree-sitter parsing or cross-request snapshot storage.

Process-executing MCP tools (`run_command`, `language_quality_run`) and project verification checks acquire execution admission before a global tool slot. At 32 total slots, at most 28 such requests may hold global slots, leaving four available to non-command traffic. Non-command tools may still use all 32. Single-slot configurations retain one usable slot. Both permits stay with started blocking work through caller cancellation; queued cancellation releases its reservations. FIFO admission follows [Tokio semaphore semantics](https://docs.rs/tokio/latest/tokio/sync/struct.Semaphore.html). This is not a hard latency guarantee under other tool, CPU or memory saturation and does not bypass user approval.

Executed fixtures record actual build counts and elapsed time in `target/wcode-index-sharing.json` and real in-process MCP read latency with 32 queued command requests in `target/wcode-admission.json`. They verify unchanged targets, revision rejection, independent files, bounded flight cleanup, late publication, FIFO and permit recovery. These are local test-build diagnostics, not model/network benchmarks; the files are regenerated by tests, not release evidence by themselves.

## Input-bound caches and truthful refresh baselines

The 2026-09-15 local follow-up uses [Anthropic's evaluation-driven tool design](https://www.anthropic.com/engineering/writing-tools-for-agents) and [Less Context, Better Agents](https://arxiv.org/html/2606.10209v1), submitted 2026-06-08, as research inputs. The latter studies enterprise expense workflows, not repository repair; its results do not justify imposing a five-call window on every coding model. wcode keeps exact source/SHA context and evaluates its own retrieval behavior before adding model-specific policy. No model fine-tuning, remote model evaluation or benchmark reproduction is claimed by this change.

Verified-experience activation is now keyed by the complete normalized history, including trajectory and retrieval intent, plus the current guarded file-membership snapshot. Record count and the final revision alone cannot detect earlier-record replacement or file removal. The same prepared snapshot feeds the key and temporal replay; unchanged requests share the cached decision, and concurrent identical misses share one replay. Membership checks use Workspace metadata rather than reading whole source files solely to discard their SHA. Metadata is not verification proof, and actual edit/source SHA checks remain separate.

The browser acknowledges only revision signals observed before a project request, never an independent later response. A manual refresh may retain an earlier safe baseline; a first snapshot without one is displayed immediately and conservatively rebuilt once after a successful revision poll. Stale cache responses clear the baseline. Deferred rebuilds are request/workspace-generation bound and do not start on hidden pages. [MDN's cancellation documentation](https://developer.mozilla.org/en-US/docs/Web/API/AbortController/abort) distinguishes cancellable network/body work; generation checks additionally prevent already-completed obsolete work from changing current UI state. These rules do not claim an atomic repository-wide snapshot.

Regression fixtures cover history rewrites with unchanged tails, deletion/directory/symlink replacement, restored files, trajectory and intent changes, cached replay reuse, six-thread request overlap, premature revision acknowledgement and obsolete/hidden-page refresh callbacks. `target/wcode-experience-membership.json` compares five local trials over 96 records and twelve 512 KiB files, asserting identical prepared paths. Its times describe guarded membership preparation, not end-to-end agent or model performance. All changes still need a newly started runtime to affect an already running instance; local verification does not authorize publication.

## Verification and limits

Regression tests cover diagnostic-line retention at 1,000/1,400/4,000-token budgets, non-code files, missing and blocked anchors, portable location syntax, changed SHAs, occupied execution slots, coalescing, invalid preview flags and unchanged normal execution. The tests are local fixtures, not SWE-bench, Agent Retrieval Bench, or a model-quality evaluation.

Run `review_changes` followed by `verify_project(level="full")`. The full Rust gate includes `cargo clippy --locked --all-targets -- -D warnings` so test code receives the same Clippy coverage as CI. Inspect the actual test report and revision-bound evidence; this document does not itself attest that a particular build passed.

No universal latency, token-cost or solve-rate improvement is claimed. Measure context hit quality, tool round trips, payload size and wall-clock time separately on representative repositories before assigning a speedup. New runtime smoke tests and the cross-platform CI matrix remain necessary before publishing these changes.

## AI Change Acceptance commercial architecture (2026-09-30)

This is an audited target design and implementation plan, not a claim that the commercial workflow is shipped. Source baseline: `ce698df`, plus the preserved uncommitted source-inspection, TUI JobView and Web Jobs skeleton. The successful `c7a3d3f` CI/audit proves that earlier commit only. No release, tag, billing or hosted deployment is authorized by this plan.

### A. Current capability map

Maturity: **Complete** means the stated bounded contract is implemented; **Partial** means a usable implementation has material gaps; **Foundation** means the engine exists without the required product entry; **Missing** means no implementation was found; **Exclude** means outside this product's intended scope. All maturity statements below describe the audit baseline.

| Capability | Current implementation | Maturity | Reusable components | Gap | Commercial importance |
| --- | --- | --- | --- | --- | --- |
| Project tools positioning | README, Product State, software-intelligence manual | Partial | README / ProjectDesign / Product Scopes | Lead with acceptance of an AI change; preserve agent neutrality | Core |
| Understand → Change → Inspect | MCP workflow prompts, agent_context, guarded writes, change review | Complete | ToolHarness / Workspace | These operations alone do not produce a Change Acceptance Record | Core |
| Product ownership | 12 Product Scopes, source/test ownership and gate | Complete | scopes / convention_status | Reuse scopes; no parallel acceptance ownership taxonomy | Core |
| Design State | Requirements, components, constraints, ADRs, AcceptanceCriterion | Complete | DesignState / AcceptanceCriterion | Criterion verification refs are mappings, not executed proof | Core |
| Software Graph | Syntax graph, snapshots, bounded impact traversal | Complete | CodeIndex / SoftwareGraphSnapshot | Preserve syntax/provider precision and partial coverage | Core |
| Semantic navigation | Optional provider capability, provenance and freshness | Partial | semantic_navigation / graph_provider_store | Availability depends on installed provider; never infer semantic certainty | High |
| Traceability | Requirement/component/implementation/AC mappings | Complete | RequirementTrace / TraceResolutionSnapshot | Structural coverage must stay separate from execution coverage | Core |
| Risk | Revision-bound heuristic profiles and escalation | Partial | Risk / VerificationProfile | Risk is advisory context; cannot clear deterministic failure | Core |
| Change inspection | Staged/unstaged/untracked diff, files/symbols/impact/source | Complete | ChangeReviewReport / worktree status | Bounded and snapshot-aware; newly dirty editor changes still need final gates | Core |
| Verification discovery | ProjectContext, CheckSpec, languages/islands/native check discovery | Complete | ProjectProfile / CheckSpec | Policy must select exact required check IDs and execution levels | Core |
| Verification execution | verify_project; bounded no-shell runner; failed/skipped/reused reports | Complete | ToolHarness::verify_project / VerificationReport | Core runner preserves skipped failure and detects revision changes | Core |
| Exact AC test execution | analysis mapping and generated AC Evidence | Partial | VerificationRef / execution receipt | A generic test check can currently imply other mapped tests ran | Critical |
| Plan-strength enforcement | VerificationPlan, VerificationStatus, execution floor | Partial | VerificationPlan / Execution floor | Aggregate quick Pass does not prove every full-plan check executed | Critical |
| Stage automation | Property/mutation/fuzz adapters and stage targets | Partial | StageExecutorRegistry / stage targets | Missing tooling remains automation gap; external reports need trusted receipts | High |
| Evidence ledger | Revision, kind, producer, confidence, targets, bounded persistence | Partial | Evidence / evidence_store | Producer strings/digests are not authenticated execution identities | Critical |
| Evidence freshness | Code + Design hash, effective/current aggregation | Complete | Revision / latest_current | Add Git head/base/tree and policy binding to acceptance envelope | Critical |
| Human approval | verification_approve and frozen reconciliation snapshot | Partial | AuthorizationManager / plan digest | Caller-supplied confirmed/approver does not authenticate a human | Critical |
| Independent review | Role jobs, claims, review submissions, disagreement | Partial | VerificationJob / ReviewerRole | Client names are forgeable; blind flag has no result access isolation | High |
| Reconciliation | Desired-state drift, frozen plan, execution and readiness gate | Partial | ReconciliationPlan / ApprovedPlanSnapshot | Reuse gate after authority and coverage hardening | Core |
| Local release gate | Design, scope, convention and structural coverage checks | Complete | release_gate / Scope / Convention | Not a customer merge gate; mapped coverage does not prove tests ran | High |
| Change Acceptance Record | Existing evidence/status/review inputs | Missing | VerificationStatus / Evidence / Risk | One immutable, explainable revision-bound decision model | Critical |
| Project Acceptance Policy | Design acceptance, risk profile, check refs, runtime policy | Foundation | ProjectDesign / AcceptanceCriterion / Risk | Versioned deterministic selection, trusted policy baseline, exceptions | Critical |
| GitHub external merge gate | Own CI; bounded GitHub reads and repository workflow | Missing | Git reader / existing CI | Commit-bound publisher, PR events, required App check and stale handling | Critical |
| Other Git providers | Vendor-neutral core | Missing | Provider-neutral core | Adapter contract now; GitLab/Bitbucket later | Later |
| Team organizations and projects | Workspace registry | Missing | Workspaces registry | Organization/member/project identity and scoped authorization | High |
| High-risk team roles | Command/risky/delete authorization | Foundation | AuthorizationManager | Owner/Admin/Developer/Reviewer/Viewer mapped to real operations | High |
| Engineering journal | Bounded append-oriented milestone JSON | Partial | Engineering milestones / journal | No verified actor, policy/exception event, chain/checkpoint or audit export | High |
| OAuth connection | Access/refresh grants, client binding, refresh rotation | Partial | AuthState / PublicEndpoints / auth tokens | No time expiry/revoke/session UI; old access remains after refresh | Critical |
| Workspace access | Root/path guards and read-only enforcement | Complete | Workspace / Workspaces / fs_safety | Single operator chooses roots; OAuth client has no per-root principal ACL | Critical |
| Command boundary | Direct argv, policy shapes, authorization, process limits | Complete | CommandResult / run_command | Ordinary repo builds/scripts run as host user; not OS tenant isolation | Critical |
| Full Access / broad sandbox | macOS Seatbelt / Linux bubblewrap broad-command lane | Partial | WorkspaceSecurity / authorization | Does not isolate ordinary builds/LSP or shared authority state | Critical |
| Protected paths / redaction | fs_safety, environment/content/header redaction | Partial | fs_safety / redaction | Runtime authority state needs protection; redaction is not write isolation | Critical |
| Remote MCP / tunnel | OAuth, endpoint provenance, local probe and provider recovery | Partial | AppState / AuthState / tunnel health | Detailed anonymous health currently discloses roots/launch profiles | Critical |
| LSP execution | Capability probes, bounded children, scrubbed environment | Complete | semantic providers / command execution | Installed LSP process retains host-user access | High |
| Evidence storage isolation | User-state, permissions, bounded records | Partial | workspace_state_directory / Evidence | Same-user scripts can modify local authority state; not tamper-proof | Critical |
| Project Status | Attention, change/source bridge, evidence inspector, graph | Partial | ProjectObservatory / ProjectAttentionView | Default architecture page; no unified current-change decision or actions | Core |
| Acceptance → file → symbol | Typed AC path/symbol/provider refs and source bridge | Foundation | FeatureAcceptanceView / source bridge | AC rows are not actionable; retain selected change/revision throughout | Core |
| TUI | Attention/proof/agents/provider views and Web handoff | Partial | TaskMonitor / console | No unified acceptance summary; JobView adapter not connected | High |
| Web Jobs | Empty hosts and state/CSS skeleton | Missing | TaskRuntime / TaskRecord | No backend/API/module; not a delivered task console | Deferred |
| Setup connection | Config preview/merge, agent setup hub, guarded configuration | Complete | Setup / agent_install | Public setup is a connection guide, not an approval authority | Core |
| First acceptance onboarding | Native discovery and setup primitives | Foundation | ProjectProfile / setup planning | Dry-run suggestion → confirm → first revision-bound result | Core |
| Agent integrations | Provider-neutral MCP, plugins/configs, implement/review/verify prompts | Partial | MCP / agent_plugin / agent_install | Host-version OAuth E2E is not implied by config tests; acceptance entry missing | Core |
| Multi-agent work | Worklist CAS, scoped claims, private lease tokens, bounded results | Complete | Worklist / writer lease / task claim | Host spawns agents; reports do not become verification evidence | High |
| Context efficiency | Bounded context, progressive schemas, compact acknowledgements | Complete | Agent Context / tool manifest | Byte/4 estimates do not establish billed token savings or model success | High |
| Pilot metrics / export | Check history and milestone foundations | Foundation | Check history / journal | Acceptance counts, missing/stale findings and measured durations | High |
| Vendor telemetry | Local engineering observations | Missing | Local runtime observation | Optional explicit schema and opt-in sink; no default source upload | Later |
| Release pipeline / tests | Three-platform CI, native browser, adversarial artifacts | Complete | GitHub Actions / release contracts | Each result applies to its precise SHA; no current release requested | Core |
| Own agent/IDE/chat/billing/remote shell | Not required for acceptance | Exclude | No component needed | Preserve useful OSS inspection/editing; stop expanding unrelated product surface | None |

Audit sources: [Verification runtime](../../src/intelligence/runtime/design.rs), [analysis](../../src/intelligence/analysis.rs), [Verification protocol](../../src/verification/mod.rs), [Evidence](../../src/evidence/mod.rs), [Evidence store](../../src/evidence/store.rs), [journal](../../src/evidence/journal.rs), [authorization](../../src/workspace/authorization.rs), [OAuth](../../src/integrations/auth/mod.rs), [runtime](../../src/integrations/mcp/mod.rs), [command execution](../../src/workspace/operations/execution.rs), [setup](../../src/app/setup.rs), [release gate](../../src/intelligence/release_gate.rs), and current Design/Worklist/tool reports. Findings are source-call-chain observations, not a completed exploit test.

### B. Target product architecture

The first commercial scenario is **AI Change Acceptance**. The outcome is: make AI-generated code shippable with evidence. Existing coding agents remain replaceable. The product loop is **Understand → Change → Inspect → Verify → Evidence → Accept / Block**.

| Layer | Responsibility | Authority |
| --- | --- | --- |
| Existing Apache-2.0 OSS core | Workspace, Design, graph, risk, guarded changes, real verification, Evidence, reconciliation | Preserve current bounds, guards and deterministic failure precedence |
| Acceptance layer, OSS | Harden existing VerificationStatus; deterministic policy selection; Change Acceptance projection/history/export | One canonical engine over existing plans/evidence; no second executor or front-end green-light algorithm |
| Team layer | Verified actors, organization/project membership, policy changes, human decisions, shared history/audit | Project-scoped permissions, explicit separation of author and reviewer where policy requires it |
| Integration layer | Trusted execution receipts, GitHub App publisher, PR revision synchronization | Provider adapter cannot weaken core decision; credentials stay outside untrusted workers |
| Optional hosted layer | Shared coordination, operational management, federation and optional metrics | Explicit deployment/data contract; never required for useful local OSS acceptance |

`stable` in the requirement view means structural convergence, not Accepted. `passed/fresh` aggregate counters can include historical records, so they must not decide current acceptance. Reconciliation and Execution continue consuming the same hardened Verification decision.

Policy is a versioned extension of existing project Design acceptance, verification refs and risk constraints. It references existing component/requirement/AC/check identities. No arbitrary executable policy hooks or second requirements database. A customer policy is selected from a trusted approved baseline, not silently weakened by the PR being evaluated.

### C. Data model and deterministic decision contract

These are target contracts, not currently available APIs.

| Model | Reuse / addition | Required contents and invariants |
| --- | --- | --- |
| Acceptance | New envelope over existing ChangeReviewReport, VerificationStatus, Risk and Evidence | Schema/id/project/workspace/repository, base/head/tree, dirty code hash + Design hash, policy version/digest, captured time, verified actor/producer/optional agent identity, bounded changes/impact/precision, verification items, evidence refs, human decisions and final decision |
| Policy | Extend project Design State | Version/id/digest, selectors over paths/components/requirements/ACs, exact required checks/stages/levels, human-review/risk rules, explicit docs-only rules, allowed exceptions, immutable core constraints; sorted deterministic matches |
| Actor | New authenticated authority type | ID, kind (operator/agent/integration/system), authentication source, organization/project permissions, optional verified host identity; producer display text is never authority |
| Organization | New Team model | ID, members/roles, projects and policy/integration ownership; local single operator remains supported |
| Project | Extend Workspace identity with Team binding | Stable ID, canonical repository identity, roots, trusted policy source, deployment mode, membership and integration bindings; aliases do not grant cross-project access |
| Audit Event | Extend journal storage mechanics with a typed audit stream | Event ID/time, actor, project, exact revisions, operation/outcome, evidence/acceptance/policy refs, exception reason, previous/event digest and export checkpoint; explicit retention gaps |
| Integration | New provider adapter model | Provider/install/repository binding, capability scopes, credential reference, expiry/revoke state, trusted receipt authority, delivery/idempotency state; no raw secrets in records |

Each verification item separates orthogonal facts:
- **Selection:** required, discovered and mapped, with mapping precision/provider.
- **Execution:** not_executed, running, completed, skipped or unavailable; check ID, runner/command receipt, scope, start/end and exit.
- **Result:** pass, fail, inconclusive or unknown. A completed command is not automatically a passed test.
- **Freshness:** current, stale or unbound; code, Design, Git target and policy relation.
- **Authority:** internal executor, authenticated integration, operator, self_reported or legacy_unknown.

Test path/symbol mappings are not execution receipts. Output tails and generic `cargo test` check names cannot prove an individual test ran, particularly when tests are filtered, ignored, skipped or from another language island. Command-level success can satisfy an explicit command-level requirement; test-specific requirements need exact runner coverage.

Evidence retains producer, kind/type, revision, scope/targets, timestamp, freshness, source/artifact digest, precision/confidence, verification/check relation and authenticated receipt authority. Historical records without authority remain `legacy_unknown`; migration must not promote them. Evidence limits/truncation stay explicit.

Human decision is approved, rejected, needs_review or exception_approved, separate from test result. A one-shot operator grant binds workspace/server instance, plan digest, code+Design revision, policy, decision and statement digest, with expiry and replay protection. MCP client confirmation and OAuth client identity alone do not prove a human participated. Full Access never grants HumanDecision. Exceptions name the waived policy rule and reason; they cannot turn a failed test into a pass or waive core identity/revision/authorization constraints.

Final decision contains `status`, `blocking_reasons`, `warnings`, `required_actions`, `evidence_summary`, `verification_summary`, `risk_summary` and full revision identity. All reasons are retained even when one determines the primary status:
1. Candidate revision/policy mismatch → **stale**.
2. Current deterministic failure, rejected required review or violated core constraint → **blocked**.
3. Required execution/receipt/discovery unavailable, partial identity or coverage → **incomplete**.
4. Deterministic requirements satisfied but authenticated human/independent review missing → **needs_review**.
5. All required current authoritative conditions satisfied → **ready**.

The engine captures inputs before evaluation and rechecks them before persisting or publishing. A changed Git commit invalidates old Acceptance even if source bytes match. Dirty local acceptance binds its content hash and cannot authorize an uncommitted GitHub SHA. Partial bounded scans never produce ready by omission. Corrupt, oversized or missing Evidence must remain visible as incomplete input; a failed read of the newest Verification snapshot must not silently recover an older approved state. CAR evaluation needs explicit input-completeness provenance, rather than interpreting discarded records as an absence of failure.

### D. Threat model

| Threat | Current concern | Required mitigation / failure behavior |
| --- | --- | --- |
| Agent bypass | Prompt/Skill compliance is advisory | Required external Git check; core decision remains authoritative |
| Stale revision | Code hash alone does not identify Git head | Base/head/tree + code/Design/policy binding, pre/post validation and stale records |
| Forged approval | confirmed=true and arbitrary approver | Operator-issued one-shot grant; authenticated actor; MCP self-approval denied |
| Fake verification | Arbitrary producer/verdict/digest/targets | Internal or authenticated receipt, precise checks/test coverage; self-reports advisory |
| Hidden deterministic failure | Same-producer later Pass can replace Fail | Authenticate producer and receipt; only real trusted re-execution may supersede |
| False independent review | Same client claims several names/roles | Opaque owner-bound claims, result visibility rules and policy separation of duties |
| Policy downgrade | Agent edits policy/workflow in evaluated change | Approved baseline policy digest; policy changes need authorized review |
| Compromised integration | Publisher credential or replayed callback | Least privilege App, verified delivery, repo/project/SHA binding, idempotency and revoke |
| Cross-project access | OAuth client currently accesses exposed roots | Verified principal → project ACL; do not advertise current root registry as Team RBAC |
| Credential leakage | Detailed public health; logs/state/build environment | Minimal public probe, authenticated diagnostics, redaction and protected credential references |
| Command escape | Builds/LSP execute as host user | Honest local trust model; isolated worker UID/container/VM for shared deployments |
| Evidence tampering | Same-user local state and unsigned JSON | Separate authority store, trusted receipt validation, chained/checkpointed exports; corruption incomplete |
| Integration outage | Lost check publish or stale head query | Leave gate pending/failing, surface retry action; no success fallback |
| Restart/concurrency | Expired grant, changed head or interrupted record write | Atomic writes, one-shot/restart invalidation, bounded queues and recovery tests |
| Shared deployment | One process/OS user shares roots and authority | One tenant/operator boundary per runtime until real project ACL + worker isolation exists |

Workspace path isolation is not an OS sandbox. The current broad-command sandbox does not isolate ordinary builds or LSP processes. Local mode assumes a trusted OS account and repository toolchain. A team-managed runtime requires explicit tenant/project identity and isolated workers; a dedicated deployment has its own authority state and credentials.

Do not claim “source never leaves the device”: source returned through MCP may reach the chosen host/model; configured remote MCP/tunnel clients can receive it; optional providers and Git/CI integrations have their own data flows. Evidence/export may contain source-derived paths, summaries or output. Document destinations and user-configured retention.

Audit is append-oriented and practically tamper-evident with chained digests and trusted checkpoints, not legally immutable. A same-user attacker can rewrite an unanchored chain. Bounded retention/export must expose missing history rather than silently claim completeness.

### E. UX and external merge flow

1. **Setup:** `wcode setup` detects repository/languages/native tests/CI/Git provider/agents, shows a dry-run policy suggestion and data destinations, then applies only confirmed configuration.
2. **Change:** The developer uses their existing coding agent. wcode selects an explicit base and candidate revision; shows dirty/clean state and partial coverage.
3. **Acceptance:** The authenticated home opens Current Change and one decision: status, required passed/missing/skipped/stale, risk, reasons and next actions.
4. **Blocked / Inspect:** Select a blocking reason → associated AC/check → evidence or missing receipt → changed file → symbol, retaining project/revision/breadcrumb.
5. **Verify:** Run exact required verification via existing bounded executor; show actual running/completed/skipped/unavailable, not mapped-as-run.
6. **Human Review:** An operator sees the frozen candidate, policy and evidence; approves/rejects or explicitly requests an allowed exception. This never updates test outcome.
7. **Ready:** Generate a new immutable record when conditions are satisfied. Historical decisions remain inspectable.
8. **Merge:** The Git adapter verifies live PR head and trusted policy/record binding, publishes the corresponding Check, and branch protection enforces it.

Advanced graph, metrics, execution and provider detail remain available through progressive disclosure. Reuse existing attention, evidence inspector and source bridge. Do not add generic charts or a second command terminal as substitutes for an acceptance action.

**GitHub P0 design:** a trusted GitHub App publisher owns `checks:write` outside the PR worker. Checks bind `head_sha` and record/policy digests. Branch protection requires the named check from the expected App. Only internally **ready** yields success; blocked/incomplete/stale/needs_review never publish neutral, skipped or success. Missing required checks fail internally even though GitHub itself can accept neutral/skipped conclusions.

Handle PR opened/reopened/synchronize, publisher retry and head changes. If merge queue is enabled, support `merge_group` and its synthetic merge SHA as a distinct candidate. Required-gate jobs have no path/commit skip filters, run despite upstream failures, and explicitly inspect failed/missing dependencies. Distinguish PR head from synthetic test merge commits; never relabel a receipt for one as proof for the other.

Untrusted PR code receives no publisher credentials and cannot edit the approved policy or trusted wcode binary. A privileged `pull_request_target`/`workflow_run` publisher must not check out and run PR code. Artifacts supplied by a worker are untrusted until their execution authority and revision binding are verified; a self-authored hash or Pass JSON is insufficient. Local same-user artifacts must not be advertised as adversarially secure hosted receipts.

Primary sources: [protected branches](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches), [required-check behavior](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks), [Checks API](https://docs.github.com/en/rest/checks/runs), [Actions security](https://docs.github.com/en/actions/reference/security/secure-use). These inform the adapter; the core engine remains Git-provider-neutral.

### F. Implementation plan

Each task includes purpose, owner scope, dependencies, location, acceptance criteria, verification and material risk. These are planned tasks; none is complete merely because it appears below.

**P0 — a real paid-pilot path**

- **P0-A authority hardening.** Purpose: stop agent self-approval and forged external proof. Owner: verification/evidence/workspace/integrations. Dependencies: this audit. Location: existing MCP dispatch, AuthorizationManager, Verification/Evidence runtime. Acceptance: MCP booleans/producer strings cannot create authoritative human or deterministic proof; operator grant is exact/expiring/one-shot. Verify: impersonation, all-command bypass, replay, stale plan and restart failures. Risk: existing permissive clients need explicit migration; do not silently discard their reports.
- **P0-B exact verification coverage.** Purpose: keep mapped/executed/skipped/failed separate. Owner: verification/traceability. Depends: P0-A authority semantics. Location: analysis, Verification runtime and focused tests. Acceptance: generic test Pass cannot prove a specific mapped test; quick cannot satisfy a full required plan; failed/skipped required checks block. Verify: filtered/ignored/cross-language/missing/reused/fail-fast/full-level cases. Risk: conservative legacy evidence becomes incomplete rather than ready.
- **P0-C versioned policy and candidate identity.** Purpose: choose requirements deterministically. Owner: design/verification/workspace. Depends: A+B. Location: Design State extensions, existing check discovery and Git reader. Acceptance: policy digest/version, trusted baseline, docs-only/auth/payment rules, exact base/head/tree/dirty binding, partial scans block. Verify: rule ordering, policy downgrade, identical-content new SHA and concurrent edits. Risk: unclear base revision must be explicit.
- **P0-D canonical Acceptance Record.** Purpose: explain why this revision can advance. Owner: verification/evidence/reconciliation. Depends: C. Location: shared status projection, bounded store/history/export. Acceptance: statuses/reasons/actions/provenance, immutable record digest and before/after guards; all consumers share one decision. Verify: old evidence/new revision, negative outcomes, truncation/corruption and interrupted writes. Risk: bounded history cannot imply complete lifetime audit.
- **P0-E operator acceptance UX.** Purpose: one continuous decision-to-source workflow. Owner: experience/integrations. Depends: D+A. Location: existing TUI/WebUI, protected narrow operation routes. Acceptance: current change/status/required failures and actionable evidence/check/file/symbol navigation; authenticated human decision stays separate. Verify: unknown/stale/cross-workspace/late-response/permission/keyboard/native browser cases. Risk: public Setup and OAuth-agent identity are not approval channels.
- **P0-F guided setup dry-run.** Purpose: reach a first record without empty YAML. Owner: experience/runtime/integrations/design. Depends: C+D. Location: existing setup/discovery/templates. Acceptance: repository/stack/CI/provider/agents detected, exact policy preview, explicit confirmation, unrelated config preserved. Verify: no-write dry-run, ambiguous repository/tool unavailable/read-only and config merge. Risk: discovery does not prove every host connected.
- **P0-G GitHub external gate.** Purpose: enforce acceptance outside Agent cooperation. Owner: integrations/verification/workspace. Depends: A–D+F. Location: narrow provider adapter + trusted publisher/setup contract. Acceptance: required App check exact SHA, synchronize invalidation, only ready success, no privileged untrusted checkout/credentials. Verify: stale head, neutral/skipped, forged artifact, outage, retry, revoked credential and policy downgrade. Risk: installation/repository permission and secure worker boundary must be provisioned.
- **P0-H deployability and minimal audit.** Purpose: explain pilot trust/data boundaries and decisions. Owner: workspace/evidence/integrations. Depends: A+D+G. Location: OAuth lifecycle, public health, protected state, typed decision events/export and deployment docs. Acceptance: expiry/revoke/session visibility, minimal anonymous probe, bounded authorization, verified actors and revision/policy decision history. Verify: expired/revoked/rotated access, saturation, cross-root denial for team mode, corrupt/recovery/export gaps. Risk: do not call same-user local runtime multi-tenant isolation.
- **P0-I real pilot demo and acceptance.** Purpose: demonstrate enforceable business value. Owner: verification/experience/integrations. Depends: A–H. Location: isolated real repository/PR, real native verification and downloaded check/audit records. Acceptance: existing Agent changes repository → required check missing → real PR blocked → real verification executes → current Evidence → new Record → real PR ready; preserve both records and exact SHAs. Verify: full suite + stale/concurrent/bypass/authorization/integration outage/restart adversarial cases. Risk: no mock core decision, no historical CI substitute, no claim complete without real Git platform result.

**P1 — team operations and commercial validation**

- **P1-A organization/roles.** Purpose: shared responsibility. Owner: workspace/integrations. Depends: P0-H. Location: principal/project registry and protected operator APIs. Acceptance: Owner/Admin manage projects/integrations; policy edit and exception require explicit permission; Reviewer can review, Viewer inspect, Developer verify; per-project ACL. Verify: forbidden role/cross-project/self-review. Risk: avoid blanket RBAC over harmless reads.
- **P1-B shared policy/history/audit.** Purpose: explain past allow/block decisions. Owner: design/evidence. Depends: P1-A+P0-D. Location: existing persistence/export plus chain/checkpoints. Acceptance: actor/policy/revision/exception lineage, bounded retention and explicit gaps, export validation. Verify: tamper/corrupt/missing/checkpoint/restart. Risk: no legal immutability claim.
- **P1-C reviewer ownership and visibility.** Purpose: authentic independent review. Owner: verification/integrations. Depends: P1-A+P0-A. Location: existing claim/job/status protocol. Acceptance: private owner-bound leases, no reviewer impersonation, blind output filtered until eligible, author/reviewer separation. Verify: forged names, stolen public ID, duplicate actor, expiry and unauthorized result reads. Risk: host identity must be verified.
- **P1-D pilot metrics.** Purpose: measured customer comparison. Owner: evidence/experience. Depends: P0-D+H. Location: bounded aggregation/export. Acceptance: acceptance/blocked/missing/stale/review/exception counts, raw measured verification duration and acceptance lead time with denominators/window. Verify: duplicate/idempotent events, missing history, no source payload. Risk: never invent saved hours/tokens/production bugs.
- **P1-E operations and integrations.** Purpose: reliable renewals. Owner: integrations/runtime. Depends: P0-G+H. Location: credential rotation/revoke, delivery retry/health, backups/export/import/support runbooks. Acceptance: recoverable safe outages and bounded queues, visible integration state. Verify: revoked secrets, prolonged outage, retries, restore. Risk: secret handling outside agent worker.

**P2 — optional scale, after the pilot**

- **P2-A optional hosted coordination/enterprise auth.** Purpose: reduce shared deployment burden. Owner: integrations/workspace. Depends: P1-A/B/E. Location: optional service adapter and deployment model. Acceptance: SSO/federation verified actors, tenant worker/state separation and explicit data contracts. Verify: tenant escape/credential scope/recovery. Risk: no mandatory cloud dependency for OSS.
- **P2-B GitLab/Bitbucket/CI adapters.** Purpose: reuse core acceptance across providers. Owner: integrations. Depends: proven P0-G adapter contract. Location: provider-specific revision/publisher adapters. Acceptance: same SHA/policy/failure/receipt invariants. Verify: provider stale/outage/branch-policy bypass. Risk: prioritize measured customer demand.
- **P2-C privacy-preserving optional telemetry.** Purpose: measured product onboarding/operations. Owner: runtime/evidence. Depends: P1-D. Location: explicit configurable event sink. Acceptance: documented opt-in and disable; setup/repository/agent/acceptance/block/verification/stale/review/exception/gate events only, no default source/file/command output/evidence upload. Verify: payload allowlist and disabled/offline operation. Risk: aggregate identifiers can still be sensitive.

For every implementation stage: `review_changes` → `verify_project quick` → relevant focused tests; at P0 completion run full and negative/adversarial checks. No user-facing ready claim before the actual external gate is observed.

### Commercial boundary and current Worklist

The Apache-2.0 local core remains useful and unrestricted by artificial commercial limits. Team value is coordination, governance, shared evidence and operational convenience. No Stripe, CRM, cloud IDE, own model, arbitrary remote shell, mobile app or large enterprise console is planned here.

Preserve unfinished source-inspection/editor work and JobView/Web Jobs skeleton. They are auxiliary Inspect/Change work, not evidence that the commercial flow exists. Existing Worklist history remains; add the commercial P0 dependency chain and defer incompatible IDE expansion without deleting its work. Release tasks remain blocked by the user's no-release instruction.

Audit completion means that capabilities and gaps have been classified. P0 completion requires the real demo above. Documentation, counters, syntax mapping, an Agent report and an older clean CI are not acceptance evidence for a new revision.

### First implementation checkpoint

The first foundation slice hardens existing Verification, Evidence and operator boundaries: exact required-command receipts and minimum verification level, conservative named-test mappings, cache provenance, complete revision/plan-bound human grants, advisory MCP stage reports, expiring/revocable OAuth sessions and protected runtime state. It also rejects incomplete/unbound revision identities, conflicting evidence IDs, corrupt or oversized authoritative records and unsafe retention that would erase a current native failure. Verification snapshots carry a monotonic persistence generation, separate from their code/Design revision; a late older snapshot or conflicting newest generation cannot become the recovered decision. The existing TUI JobView is connected to real durable MCP command jobs, with bounded redacted logs, truthful failed outcomes and owner/workspace-bound cancellation.

These changes reuse the OSS core. Their validation is tracked in the Worklist against the current code and Design revision. They do not implement a Change Acceptance Record, approved project Acceptance Policy, Git SHA/base/tree identity, exact per-test event adapter, trusted CI receipt, verified team actor, exception workflow, external merge gate, team deployment isolation or audit lineage. Job observation is supporting Inspect/Verify work; canonical Acceptance UX and Web Jobs remain pending. P0 remains incomplete until its full real PR flow is implemented and observed.

### Commit-aware inputs and policy drafts

The next slice adds bounded execution Git identity to native project and language-quality evidence: private repository/Workspace scope digest, full HEAD/tree object IDs, index fingerprint and dirty state. Static reuse and in-flight coalescing include this identity and validate the actual source receipt; an observed commit or index change during execution rejects publication. Old evidence with no Git binding stays unknown. The existing Code/Design content guards remain necessary: dirty is not a content digest, and before/after probes are not an atomic filesystem snapshot.

Existing `review_changes` keeps its current worktree response by default. Explicit `base_revision` requests a separate base-change metadata response; `target_revision` defaults to `HEAD` and may be `worktree`. Clean current-commit inspection retains both rename paths and file modes. Unknown, truncated, denied, dirty commit or noncurrent target capture is incomplete and reports an error. Its `metadata_only` authority cannot approve a baseline or produce Acceptance.

Repository discovery now reports bounded completeness, issue counts and reason tags. Fingerprinting and parsing share the same captured input bytes; invalid/unreadable/over-limit input cannot silently become a complete check inventory. A partial inventory adds an unmet deterministic requirement to quick/full verification while retaining independent checks that actually ran.

`ProjectDesign` has an optional versioned, typed Acceptance Policy draft with a digest and deterministic rule accumulation. Explicit docs-only selection requires complete old/new paths, known ordinary nonexecutable modes and allowed Markdown scopes; Agent instruction and Skill Markdown retain baseline requirements. Arbitrary embedded Markdown still needs a trusted graph-based risk floor. Referenced check IDs and component/requirement mappings must be resolved from complete trusted inputs before any future activation.

Evidence records distinguish native verification, native stage, local operator, self-reported and legacy unknown authority. Generic Agent submissions cannot satisfy native stage or human requirements; native failure cannot be replaced by advisory review. Parsing a draft, recording Git metadata or checking source labels never activates policy.

Focused and full verification results for this slice are tracked against the current code and Design revision in the Worklist. Approved policy activation, the canonical Change Acceptance Record, Git-bound stage/human decisions, trusted external merge checks, Team actors and the real pilot PR demonstration remain pending. The running MCP process requires an explicit upgrade/restart to serve newly compiled behavior; local source tests alone do not prove deployment.
