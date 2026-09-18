# English Revision 2

Date: September 18, 2026. Scope: paper text, documentation entry points, typesetting support, and read-only artifact validation. No product code, corpus, evaluator, or existing worklist was changed for this revision.

## Editorial changes

The manuscript now defines the all-required edit-input predicate mathematically, distinguishes evidence-level from task-level denominators, and states the zero-denominator and failed-attempt rules. It clarifies that complete source is relative to the authored Gold fragment, not the entire program.

The Non-Gold aggregate is explicitly a conditional mean over nonempty identity responses. The discussion no longer leaves room to interpret a small value as proof of good retrieval.

The results section includes the recorded `rust-refresh-natural` example: at 1K its identity and file hash were available, but its 93-byte Gold source fragment was not completely delivered. This is diagnosis of final delivery, not a new experiment or an internal-retrieval failure claim.

The future-work protocol specifies non-oracle lexical and syntax-only baselines, one-component-at-a-time ablations, and repository-level holdouts. Correlated budget/cache cells are explicitly not independent task samples; no unsupported confidence interval or significance claim is added.

## Evidence and references

The original report SHA is unchanged: `b117266f8c33cebcef26cb2fc9d56a5bf50b48f604d67ffab1944a16b4807ffc`. All reported values still refer to the same 360-query diagnostic run. The raw report contains scores and diagnostics rather than every original tool response, so recomputing aggregates is not the same as arbitrary regrading or exact rerunning.

The seven literature entries were rechecked at their primary sources. The recent item remains labeled as a preprint, and the MCP reference does not imply conformance certification.

## Repository delivery

English text and evidence are maintained under `docs/paper/`. Reciprocal English and Chinese landing pages under `docs/manual/` link the manuscripts, snapshot, evidence, and bibliography. The manuals' indexes link those landing pages.

`tests/paper_artifacts.py` checks retained raw data, selected fields, publication tables, and document links without running an experiment. `build.py` and `print.tex` provide a Markdown-to-LaTeX/PDF pipeline on a machine with Pandoc and XeLaTeX. No automatic dependency installation is performed.

## Remaining limits

The archival command was blocked by execution safety checks; `fitness.raw.json` was not created. The canonical raw report remains under `target/`, and the archival helper is not presented as executed. The repository host did not report Pandoc, XeLaTeX, or pdfLaTeX on PATH. PDF generation is therefore performed separately in the attachment-build environment, not claimed as a successful repository-host build.

No commit, push, website deployment, or paper submission is implied by saving these documents. A frozen source artifact, independent holdout, controlled baselines and ablations, and fixed-model task evaluation remain outstanding research work.
