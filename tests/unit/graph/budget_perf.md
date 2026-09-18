# Graph budget and construction measurements

Measured on 2026-09-17 in the shared wcode working tree. Compiler: rustc 1.98.1 (48a229cea, 2026-09-01), host aarch64-apple-darwin, LLVM 22.1.8. These are local, release-build, warm-index graph-construction measurements, not model inference, MCP network latency or repair success rates.

## Reproduction

```sh
cargo test --release --locked --lib graph_build_release_cost -- --ignored --nocapture
cargo test --locked graph_budget_ --quiet
cargo test --locked repo_merge_ --quiet
```

The benchmark parses a generated one-file fixture, warms twice, then retains eleven calls to `software_graph_from_paths`. Each call includes file freshness checks, graph selection/construction and final validation. Serialization happens outside timing; repeated output is byte-equal within each run. No wall-clock assertion is used in CI.

## Observed pre/post results

| Definitions | Before p50 / p95 (ms) | After p50 / p95 (ms) | Nodes / edges | Serialized graph bytes, both runs |
| --- | --- | --- | --- | --- |
| 500 | 1.569458 / 1.617917 | 0.969834 / 1.029083 | 501 / 500 | 355726 |
| 5000 | 82.280792 / 83.585250 | 10.629084 / 12.263417 | 5001 / 5000 | 3563230 |

The baseline already included the global target-budget correctness fix. The measured performance change replaced repeated per-edge growing-list duplicate validation inside the private syntax builder with unique edge emission and one complete graph validation. General `SoftwareGraph::add_edge` behavior is unchanged. Supplemental merge indexing is a separate improvement and is not exercised by this particular benchmark.

Both runs used the same fixture generator and release profile. They were separate pre/post runs rather than alternating algorithms within one process; concurrent work in this shared workspace can affect timings. The first two post-change attempts timed out during compilation after waiting on the shared build lock and produced no benchmark data. The successful post-change run explicitly selected `--lib`. Those timeouts are not passing tests or additional samples. Graph counts and byte lengths matched between runs; byte equality was asserted across samples within each run, not against a persisted cross-build byte oracle.

## Correctness evidence

`graph_budget_reserves_priority_across_files` and `graph_budget_keeps_exact_target_ahead_of_many_callers` both failed before the reservation fix, then passed. The first covers three file orders and both a tiny budget and 6200 unrelated definitions under a 5000-symbol graph cap. The third budget test preserves deterministic unprioritized selection and the hard cap when explicit definitions exceed it.

Merge tests compare edge order and complete provenance against legacy de-duplication, reject a real source rewrite between graph builds without mutating the base, preserve scan truncation, and retain invalid-endpoint/self-edge rejection. They do not establish an atomic snapshot across all repository files.

Sources motivating the work: [Aider repository map](https://aider.chat/docs/repomap.html), [DyRetriever](https://arxiv.org/abs/2608.01927), and [Agent Retrieval Bench](https://arxiv.org/abs/2607.24882). Their performance numbers are not used as wcode results. This report does not claim a new release, a full-current-revision verification pass, or a Kimi/Claude/Codex model comparison; those require separate evidence.
