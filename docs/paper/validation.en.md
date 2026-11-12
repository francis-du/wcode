# English Revision 2 — Validation Record

Date: September 18, 2026. This record concerns document delivery and retained-data consistency, not a new Check history experiment.

## Current availability of the raw report (checked 2026-11-06)

The canonical raw report at `target/engineering-fitness-1789665000441866000-37653.json` is **no longer present**, and no `docs/paper/fitness.raw.json` archive exists. The archival attempt described below was blocked, so the raw bytes were never copied into the document directory and the file has since disappeared from `target/`.

Consequences, stated plainly:

- `python3 tests/paper_artifacts.py` now reports `raw_report_unavailable` with `raw_verified: false`. It still verifies the manuscript structure, the two manuscript tables against the retained selected fields in `snapshot.json`, and the bilingual documentation links.
- The raw-sample aggregates, the concrete delivery-failure example, and the nine historical controls recorded on September 18 cannot be re-verified against the original bytes. Their recorded values remain in this document and in `snapshot.json`, but they are historical claims, not currently checkable ones.
- The expected report size and SHA-256 remain the identity of the original report; they are not evidence that the bytes are readable now.

A future archival copy with the recorded SHA-256 would restore full verification; the check detects either location.

## Repository checks

`python3 tests/paper_artifacts.py` completed successfully. It verified 360 unique measurement cells, 180 separately recorded warmups, six selected-data groups against the canonical raw report, two manuscript tables, the recorded delivery-failure example, and reciprocal bilingual document links. The nine controls are historical results read from the report, not newly executed controls.

`cargo test --locked --test documentation_layout` completed successfully: **7 passed, 0 failed, 0 ignored**. The recorded test duration was 0.03 seconds after a 17.20-second build stage. This is the documentation integration suite, not the full product suite.

`review_changes` completed. The workspace also contained substantial unrelated staged, unstaged, and untracked work from concurrent development. Those changes were not reset, committed, or attributed to this paper revision.

`verify_project(level="quick")` did not pass. Its first executed check, `cargo fmt --check`, failed on Rust formatting in the concurrent working tree, including `src/runtime/harness/context_budget.rs` and Fitness report code. The fail-fast run skipped `git-diff-check`, `rust-check`, and `focused-rust-test`. These product files were not reformatted as part of the paper task. A passing paper check must not be reported as a passing whole-workspace gate.

## Artifact identity and build scope

English manuscript SHA-256:

```text
c5454bbdc02ed63f7f37e76acfa7ade5582865eecb66daa80a3db2bda78ad2e7
```

Canonical raw experimental report SHA-256:

```text
b117266f8c33cebcef26cb2fc9d56a5bf50b48f604d67ffab1944a16b4807ffc
```

The raw report was read and checked, but archival execution was blocked; it remains in the original `target/` location. No archive success is claimed.

The attachment-generation environment built a 10-page English PDF from the matching manuscript with Pandoc 3.1.11.1 and XeLaTeX (TeX Live 2025/dev/Debian). The log reported no overfull or missing-character warnings. All ten pages were rendered for layout review. Build fingerprints accompany the attachment package. This is not a repository-host build: Pandoc and TeX engines were not available on that host's PATH.

No commit, push, website deployment, paper submission, model-driven evaluation, or new Fitness run was performed for this revision. Writing remained AI-assisted.

## Current verification state (checked 2026-11-12)

The checks below were run on the current tree. They concern artifact and repository consistency, not a new Check history experiment.

- `python3 tests/paper_artifacts.py` exits 0 with `raw_report_unavailable`: the canonical raw report is still absent, so raw-sample and control verification stay skipped and the manuscript is checked against the retained selected fields only.
- `cargo fmt --all -- --check` and `cargo clippy --locked --all-targets -- -D warnings` are clean, and `cargo test --locked --test documentation_layout` passes 8 tests.
- `node tests/release_audit.cjs --require-clean` passes all 300 distinct adversarial rounds with stable inputs and a clean Git tree.
- The macOS, Linux and Windows archives for v0.9.14 were published and re-verified from fresh downloads by the release workflow.

The September 18 record above still stands: a frozen source artifact, independent holdout, controlled baselines and fixed-model task evaluation remain outstanding research work.
