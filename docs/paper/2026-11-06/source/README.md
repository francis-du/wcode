# WCode preprint source

**WCode: Revision-Bound Evidence for Stateful Software Agents**

**Author: Francis Du**

Manuscript date: 6 November 2026

This directory contains the English manuscript and its complete Chinese companion,
the shared style and bibliography, pre-generated bibliography files, the two vector
figures used by the paper, and the build script. These are the inputs used for the
published PDFs. The experiment and its measurements are unchanged by the authorship
update.

## Build the PDFs

Use Python 3 and a full TeX Live or MiKTeX installation with pdfLaTeX, XeLaTeX,
BibTeX, Latin Modern, Noto Serif CJK SC, and Noto Sans CJK SC. Install the CJK fonts before building
the Chinese companion. From this directory, run:

```sh
python3 scripts/build.py --language all
```

Use `--language en` for the English edition or `--language zh` for Chinese.
The output files are `build/main.pdf` and `build/main.zh-CN.pdf`.
The script compiles each edition through BibTeX, records the source hashes,
and reports reference or layout warnings. It does not install dependencies.

The English-only `../wcode-arxiv-source.zip` is a separate compact submission
package: extract it and compile `main.tex` with pdfLaTeX. Its matching `main.bbl`
and all used figures are included. The source package is prepared for submission;
the paper has not been submitted to arXiv or a journal.

## Artifact scope

This public directory reproduces the manuscript from retained vector figures.
The full research archive additionally retains original CSVs, plotting and
validation scripts, source provenance, and experiment receipts. Those archived
records are distinct from this public manuscript build and have not been edited
to remove their original machine paths.

The measured runtime is identified in the paper by source commit
`584abd88a87b9ec31201434a06bd31a8637ca40c` and frozen executable SHA-256
`2ffaac58a3fe25ef98dbef114b48c016ea038c9cb45072ed749bc3b2201fa021`.
The study uses synthetic developer-authored scenarios and does not establish
real-repository issue-resolution success or comparative model performance.

Read the [English paper page](https://wcode.francis.run/docs/paper/)
or the [Chinese paper page](https://wcode.francis.run/zh/docs/paper/).
