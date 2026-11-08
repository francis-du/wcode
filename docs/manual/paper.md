---
layout: docs
title: Research Paper
description: A preprint by Francis Du on revision-bound evidence for stateful software agents.
author: Francis Du
lang: en
alternate: /zh/docs/paper/
permalink: /docs/paper/
---

# WCode: Revision-Bound Evidence for Stateful Software Agents

**Francis Du** · **6 November 2026** · Preprint

WCode treats evidence as an observation bound to its subject, producer, authority, coverage, policy, and code–design revision. The paper specifies an evidence-admission contract and studies how the runtime connects bounded source delivery, guarded edits, verification reuse, and producer-aware reconciliation.

## Read the paper

[English PDF — 10 pages](/paper/2026-11-06/wcode-preprint.en.pdf) · [Chinese PDF — 8 pages](/paper/2026-11-06/wcode-preprint.zh-CN.pdf) · [LaTeX source ZIP](/paper/2026-11-06/wcode-arxiv-source.zip)

<p><a href="/paper/2026-11-06/wcode-preprint.en.pdf" target="_blank" rel="noopener">Open the English PDF in a new tab</a> · <a href="/paper/2026-11-06/wcode-preprint.en.pdf" download>Download the English PDF</a></p>

<object data="/paper/2026-11-06/wcode-preprint.en.pdf" type="application/pdf" aria-label="WCode preprint by Francis Du, English edition" style="display:block;width:100%;height:75vh;min-height:420px">
  <p>The embedded PDF viewer is unavailable in this browser. <a href="/paper/2026-11-06/wcode-preprint.en.pdf">Open or download the English PDF</a>.</p>
</object>

## Study and scope

The model-free diagnostic covers 60 developer-authored synthetic scenarios in four languages, three response budgets, two harness-cache phases, and seven repetitions, yielding 2,520 measured calls. Twenty-two selected contract counterexamples were replayed individually on the same retained executable; all 22 tests and eleven operational controls passed.

The preprint is a frozen experiment bound to the source commit and executable recorded in its artifact guide. It does not describe the current product: capability pages such as the [MCP reference](../reference/) and the [Observatory review](../ide-independent-observatory/) cover later behavior, and no claim on this page should be read as attributing those later capabilities to the measured revision.

The results distinguish source delivery, edit-input readiness, and the visibility of structured decision records. The required source is small, and repetitions reuse the same fixtures. The findings support the tested runtime contracts. Independent real-repository issue resolution, matched agent comparisons, and operator studies remain future evaluation.

## Source and citation

The [versioned English and Chinese LaTeX sources](https://github.com/francis-du/wcode/tree/main/docs/paper/2026-11-06/source) support rebuilding the manuscript. See the [build and artifact guide](/paper/2026-11-06/README.md), [BibTeX citation](/paper/2026-11-06/citation.bib), and [SHA-256 checksums](/paper/2026-11-06/SHA256SUMS).

Francis Du. 2026. *WCode: Revision-Bound Evidence for Stateful Software Agents*. Preprint, 6 November 2026.

The [earlier draft](/paper/README.md) documents a separate 360-query diagnostic snapshot.
