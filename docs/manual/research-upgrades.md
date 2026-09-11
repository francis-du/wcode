---
layout: docs
title: Research-informed improvements
description: Diagnostic context and dependency previews prepared for 0.6.2, with research rationale and explicit limits.
lang: en
alternate: /zh/docs/research-upgrades/
permalink: /docs/research-upgrades/
---

# Research-informed improvements

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

## Diagnostic context

Use the existing `agent_context` query with an explicit location, for example `error[E0308] at src/runtime/harness/context_budget.rs:33:9`. Supported forms include `file:line`, `file:line:column`, `file#Lline`, and plain supported source/config/document filenames. Backslash-separated relative paths are accepted. Quoted whitespace-free tokens are supported; this is not a complete parser for every language's traceback syntax.

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

Exact fixed-form Git status and diff checks use a separate inspection queue with one to four permits depending on memory, CPU and tool limits. Other commands and repository executors retain the existing heavy-process limit (two at the default budget). Policy checks, user authorization, process supervision and resource-pressure admission still happen; changing the queue cannot approve a mutation or helper option. Exhausting outer tool slots can still delay admission before either process queue.

Resource telemetry exposes `child_queue` and `probe_queue` with `active`, `limit`, `waiting`, cumulative `waits`, `total_wait_ms` and `max_wait_ms`. Active values count reserved permits, not CPU-running processes; cancelled waits clean up their metrics. The wide TUI header shows `PROC`, `GIT` and combined inner `Q`, separately from tool slots and their peak. No new authorization setting or automatic restart is involved.

Regression controls compare sixteen versus thirty-two started blocking workers, keep real Git inspection responsive while all heavy permits are occupied, exercise cancellation/closed queues/resource pressure, and compare guarded file batches without weakening SHA or per-file error checks. Benchmark fixtures report their actual worker count and raw times; test-build results are not production, model or network speed measurements. A newly built runtime must be started before these changes affect the live dashboard.

## Consistent retrieval and saturation handling

`symbol_context` compares the file body's SHA with the indexed revision before returning signatures, ranges or call relations. A mismatch invalidates the stale record and permits one bounded retry; a changed symbol identity or continued file churn returns an explicit error. Equal file size and modification time are not accepted as content identity. This guarantees consistency of the returned file context, not an atomic snapshot of the entire repository or permanent freshness after the response.

Cold `ensure_indexed`, single-query search and multi-query search share a per-Workspace-root/file build flight. A waiter holds neither the global index-state lock nor a CPU permit, rechecks the cache after acquiring the flight, and reuses a successful build. Warm cache hits keep the fast path. At most 256 independent live flights are registered; inactive entries are reclaimed and active ones are never evicted to satisfy capacity. File/prefix invalidation and aggressive memory trimming invalidate in-flight publication generations. Failed reads or an unwound empty coordination lock do not permanently block subsequent builds. This is duplicate-work suppression, not incremental Tree-sitter parsing or cross-request snapshot storage.

Process-executing MCP tools (`run_command`, `language_quality_run`) and project verification checks acquire execution admission before a global tool slot. At 32 total slots, at most 28 such requests may hold global slots, leaving four available to non-command traffic. Non-command tools may still use all 32. Single-slot configurations retain one usable slot. Both permits stay with started blocking work through caller cancellation; queued cancellation releases its reservations. FIFO admission follows [Tokio semaphore semantics](https://docs.rs/tokio/latest/tokio/sync/struct.Semaphore.html). This is not a hard latency guarantee under other tool, CPU or memory saturation and does not bypass user approval.

Executed fixtures record actual build counts and elapsed time in `target/wcode-index-sharing.json` and real in-process MCP read latency with 32 queued command requests in `target/wcode-admission.json`. They verify unchanged targets, revision rejection, independent files, bounded flight cleanup, late publication, FIFO and permit recovery. These are local test-build diagnostics, not model/network benchmarks; the files are regenerated by tests, not release evidence by themselves.

## Verification and limits

Regression tests cover diagnostic-line retention at 1,000/1,400/4,000-token budgets, non-code files, missing and blocked anchors, portable location syntax, changed SHAs, occupied execution slots, coalescing, invalid preview flags and unchanged normal execution. The tests are local fixtures, not SWE-bench, Agent Retrieval Bench, or a model-quality evaluation.

Run `review_changes` followed by `verify_project(level="full")`. The full Rust gate includes `cargo clippy --locked --all-targets -- -D warnings` so test code receives the same Clippy coverage as CI. Inspect the actual test report and revision-bound evidence; this document does not itself attest that a particular build passed.

No universal latency, token-cost or solve-rate improvement is claimed. Measure context hit quality, tool round trips, payload size and wall-clock time separately on representative repositories before assigning a speedup. New runtime smoke tests and the cross-platform CI matrix remain necessary before publishing these changes.
