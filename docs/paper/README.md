# wcode Research Paper

**wcode: An Evidence-Driven Engineering Control Plane and Model-Free Fitness Evaluation for Coding Agents**

[English manuscript — revision 2](paper.en.md) · [中文原稿](paper.zh-CN.md)

The English revision improves metric definitions, the explanation of a recorded delivery failure, baseline and ablation design, and reproducibility boundaries. It preserves the original experimental snapshot; editorial revision is not a new Fitness evaluation. The Chinese original remains unchanged. Both editions are technical drafts, not accepted or peer-reviewed publications. Authors, affiliations, and venue formatting remain to be finalized.

英文修订版 2 已保存于本目录。实验数据沿用中文原稿的同一快照，新增论证与说明不算新实验；中文原稿未被覆盖。

## Manuscripts and evidence

| File | Purpose |
| --- | --- |
| [paper.en.md](paper.en.md) | English manuscript, revision 2 |
| [paper.zh-CN.md](paper.zh-CN.md) | Original Chinese manuscript |
| [evidence.en.md](evidence.en.md) | English provenance, commands, fingerprints, and evidence limits |
| [evidence.md](evidence.md) | Original Chinese evidence record |
| [snapshot.json](snapshot.json) | Selected fields, not the complete raw report |
| [references.bib](references.bib) | Seven primary-source bibliography entries |
| [revision.en.md](revision.en.md) | Editorial changes and remaining limitations |
| [validation.en.md](validation.en.md) | Actual checks, build scope, and the unrelated workspace formatting blocker |
| [build.py](build.py) and [print.tex](print.tex) | Markdown-to-LaTeX/PDF build support |
| [archive.py](archive.py) | Unexecuted, create-only archival helper; its execution was blocked |

The bilingual documentation landing pages are maintained in `docs/manual/paper.md` and `docs/manual/paper.zh-CN.md`, with reciprocal `/docs/paper/` and `/zh/docs/paper/` routes. Saving these files does not deploy the website.

## Fixed measurement snapshot

The sole quantitative source is:

```text
target/engineering-fitness-1789665000441866000-37653.json
SHA-256: b117266f8c33cebcef26cb2fc9d56a5bf50b48f604d67ffab1944a16b4807ffc
```

There are 60 synthetic development scenarios, three budgets, two cache phases, one observation per cell, 360 measured queries, and 180 separate warmups. Model calls are zero for this measurement only, not for development or writing. Budgets are serialized-byte proxies, not model-token billing. Evidence coverage is not autonomous bug detection or patch correctness.

The raw report remains under `target/`. The attempt to create a document-directory archive was blocked by execution safety checks; no `fitness.raw.json` archive is claimed. Do not remove the original report before arranging a separately authorized archive. Selected fields and hashes do not reconstruct raw samples or uncommitted source, and raw sample scores do not contain all original tool responses.

## Validate retained data

From the repository root:

```sh
python3 tests/paper_artifacts.py
```

This read-only check compares the selected fields, raw-sample aggregates, manuscript tables, concrete example, and local documentation links. It uses the original report or an identical archive when available. It does not execute a new experiment or certify the full product. The original experiment commands and the separately available release-trial command are recorded in `evidence.en.md`.

## Build the English paper

With Pandoc and XeLaTeX already installed, run:

```sh
python3 docs/paper/build.py --check-env
python3 docs/paper/build.py
```

The builder writes to a new directory under `docs/paper/.build/`, prints its location, and retains the standalone LaTeX, PDF, and a build manifest. It does not install dependencies or overwrite earlier build directories. Review the rendered PDF after building; a successful compiler exit does not establish visual correctness.

The repository host did not report Pandoc or a TeX engine on PATH during this revision. The revised PDF attachment is built in a separate document-generation environment; it must not be reported as a repository-host build. `references.bib` is the shared bibliography source record; the numbered reference list is currently written explicitly in the manuscript, not generated through BibTeX.

## Publication limits

The working tree contains uncommitted changes from concurrent development. HEAD alone is not a frozen experimental artifact. Independent real-repository holdouts, same-budget baselines, mechanism ablations, fixed-model task evaluation, and a reviewed source/build artifact remain outstanding. No commit, push, deployment, or submission is implied by these documents.
