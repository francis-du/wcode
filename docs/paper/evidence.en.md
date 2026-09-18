# Evidence and Reproducibility Record

**English revision 2 · September 18, 2026**

This record preserves the original study's measurements, denominators, and limitations. The revision adds editorial clarification and read-only artifact checks, not a new Fitness run. Current repository files must not be assumed identical to historical compilation inputs.

## 1 Canonical report

All quantitative tables use one diagnostic snapshot. Earlier runs, different binaries, and the focused-test matrix are not mixed into its latency table.

```text
Workspace: Code/Rust/wcode
JSON: target/engineering-fitness-1789665000441866000-37653.json
Markdown: target/engineering-fitness-1789665000441866000-37653.md
Report time: 2026-09-18T02:10:00.441866+09:00
JSON bytes: 597957
JSON SHA-256:
b117266f8c33cebcef26cb2fc9d56a5bf50b48f604d67ffab1944a16b4807ffc
Markdown SHA-256:
d6377ea365b839f0da80cb6bd058aee8ffdd53b1f9846ddd4f12dae8f1009c23
```

The timestamp is derived from the nanosecond timestamp in the report filename. `snapshot.json` contains selected fields, not the full raw report. The complete report remains in `target/`; the attempted archival command was blocked by execution safety checks during revision 2. No archive is claimed. `archive.py` is an unexecuted, create-only archival helper, not evidence that an archive exists.

The raw report contains per-sample scores and delivery diagnostics, not the full serialized response to every query. It supports recomputing the published aggregates, but it does not by itself support applying an arbitrary new grader to original tool responses. Exact re-execution also requires the original corpus, evaluator, runtime, and compilation inputs. A source or binary hash identifies bytes; it does not reconstruct them.

## 2 Recorded environment and fingerprints

| Field | Original report value |
| --- | --- |
| Package version | 0.7.5 |
| Git HEAD | 181ec689f1db21840038ab2fe179dd3f567ba899 |
| Working tree dirty | true |
| Build profile | debug |
| Operating system / architecture | macos / aarch64 |
| Compiler | rustc 1.98.1 (48a229cea 2026-09-01) |
| Available parallelism / Harness slots | 10 / 4 |
| Measurement contract / JSON schema | 3 / 2 |
| Scenarios / observations per cell | 60 / 1 |
| Measured query attempts | 360 |
| Model calls / query errors / warmup errors | 0 / 0 / 0 |
| Source stable during run | true |

```text
corpus_sha256:
83cc3eb2b0ddeb2386f0ead9703f0e546688d0f0d9e8c171f9db6633a6c0461e

evaluator_sha256:
a0cf6c25d2fe982bc84994a89ccd18f913c2b90e7dda97dc3c51dcf3fe18c0ca

test_binary_sha256:
5a6ed3688d8b4ada0945462ee5783d5f3416b29e13acb5197df27295211b5d2c

source_snapshot_before == source_snapshot_after:
14d646e7ad0286482292b618047a07944935ada25f6c9850f699dab245ac91cd
```

The source snapshot covers implementation-declared `src`, `tests`, `.wcode/design`, and selected build inputs. Matching before/after hashes do not prove identical compilation inputs or exclude an intervening modification that was subsequently restored. The exact hardware model, OS patch level, isolated load, CPU usage, and resident set size were not fully recorded. No hardware-performance conclusion is claimed.

## 3 Commands executed for the original study

```sh
cargo test --locked --lib engineering_fitness_ -- --nocapture --test-threads=1
```

Recorded result: **40 passed, 0 failed, 2 ignored, 860 filtered out**. Test execution: 50.16 seconds; reported build stage: 36.82 seconds. The nine operational controls passed. The ignored entries were the diagnostic snapshot and seven-repeat trial; ignored does not mean executed.

```sh
cargo test --locked --lib engineering_fitness_diagnostic_snapshot -- --ignored --nocapture --test-threads=1
```

Recorded result: **1 passed, 0 failed, 907 filtered out**. Test execution: 24.60 seconds; reported build stage: 31.07 seconds. This command generated the canonical report.

Concurrent source changes and recompilation occurred between these commands. They are separate records, not a single frozen-binary acceptance result or a full-project pass. The original study did not run the seven-repeat release trial, external baselines, mechanism ablations, end-to-end model evaluation, full-project verification, or a release workflow. Document checks for this revision do not change that statement.

## 4 Denominators and arithmetic

Each budget–phase group contains 60 attempts, 59 answerable tasks, 58 writable tasks with required evidence, and 98 required identities. At 1K: 85 identity hits, 67 complete-source hits, 97 current-hash hits, and 28 tasks with every required edit input. At 2K and 4K: 98, 98, 98, and 58. Query errors and over-budget successful responses are zero.

The 1K natural-language subset has 12 scenarios and identity/source/edit-input values of 100% / 50% / 33.3%. Four impact scenarios have 100% / 50% / 0%; eight relationship scenarios have 75% / 50% / 0%; ten bug-relevant scenarios have 55% / 55% / 10%. These are development subsets, not independent real-project samples.

```text
Cold response-size reduction, 2K relative to 4K:
(12045.266666666666 - 7330.116666666667)
/ 12045.266666666666 = 0.39145252076887736
```

This is approximately 39.15% fewer serialized bytes, not an observed model-token or API-cost reduction. The 180 warmups are separate from 360 measured queries.

Revision 2 spells out the all-required edit-input predicate and its task-level denominator. It also clarifies that the Non-Gold aggregate is a mean over nonempty identity responses, not a pooled symbol fraction. No scorer, corpus, measured value, or experimental fingerprint was changed for those explanations.

## 5 Claim-to-implementation mapping

| Source | Path or symbol | Scope |
| --- | --- | --- |
| S1 | README.md; Cargo.toml | System description, package fields, and operational boundaries |
| S2 | docs/manual/engineering-fitness.md; Fitness design requirement | Protocol and unmeasured dimensions |
| S3 | src/runtime/harness/agent_context.rs | Assembly, original-source preservation, and budget checks; not optimality |
| S4 | fitness/corpus.rs; controls.rs; checks.rs; counterexample modules | Gold and selected controls; not a hidden holdout |
| S5 | fitness/scoring.rs | Independent identity/source/edit-input scoring; not patch correctness |
| S6 | fitness/report.rs; delivery.rs; breakdown.rs | Aggregation, cache definitions, fingerprints, and diagnostics |
| S7 | Canonical JSON/Markdown and original command results | Historical observation, not a fully frozen source release |
| S8 | src/runtime/harness/quality/verification_run.rs; src/evidence/experience.rs | Verification and historical hints; independent benefit unmeasured |

Here, `fitness/` abbreviates `tests/unit/runtime/harness/fitness/`. Paths identify implementation responsibility; current line numbers do not identify historical executable behavior.

## 6 Artifact validation versus experimental reproduction

From the repository root:

```sh
python3 tests/paper_artifacts.py
```

This is a read-only document check. It verifies the fixed raw-report SHA, selected metadata, unique cell count, warmup count, six aggregate groups, raw-sample count sums, manuscript table entries, and document links. It accepts the original report or an identical archive when available. It does not call a model, execute the measured runtime, prove grader correctness, or certify an immutable source release. The raw report is required; the attachment package is not sufficient for this repository-side check by itself.

To repeat the original experiment, use Section 3 with appropriately frozen inputs. Newly generated results belong to a new report and must not overwrite this paper's snapshot. The existing release-trial command is documented below, not reported as executed:

```sh
cargo test --release --locked --lib engineering_fitness_trial -- --ignored --nocapture --test-threads=1
```

It uses seven observations per cell: 2,520 measured queries plus 180 warmups. Repetitions are not distinct tasks, and nearest-rank p95 with seven observations equals the maximum. Neither those repetitions nor the 360 diagnostic cells should be treated as independent draws from a real-repository population.

## 7 Literature and editorial provenance

All seven entries were checked against primary arXiv records or the official MCP specification on September 18, 2026. References [1–5] concern task evaluation, interfaces, or retrieval methods; [6] is the versioned MCP specification; [7] is a preprint, version 2 dated September 16, 2026. None supplies a direct numerical baseline for the wcode experiment. Canonical addresses are retained in `references.bib` and the manuscript.

The original Chinese manuscript has SHA-256 `a2497804a6a0d1151ddb7215ed09df20dc3ab155ecbb35b243b174cbaafaeed5`; its evidence record has SHA-256 `aa397443cb596a4d08a9d23a7da6259f0e95ec8d1523c161ced44731ac2a8faf`. The English revision is an expanded editorial version, not a claim that every sentence is a literal translation. The unchanged shared snapshot preserves numerical correspondence.

This manuscript was prepared with AI assistance. Empirical values come from the recorded original commands. Authors, affiliations, venue formatting, and final scholarly responsibility remain for the maintainer to confirm. `model_calls=0` describes the Fitness measurement only, not development or manuscript preparation.
