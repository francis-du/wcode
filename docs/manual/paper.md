---
layout: docs
title: Research Paper
description: The wcode system-paper draft, English manuscript, Chinese original, and measurement evidence.
lang: en
alternate: /zh/docs/paper/
permalink: /docs/paper/
---

# Research Paper

## Manuscripts

**wcode: An Evidence-Driven Engineering Control Plane and Model-Free Fitness Evaluation for Coding Agents**

Read the [English manuscript](/paper/paper.en.md) or the [Chinese original](/paper/paper.zh-CN.md). English revision 2 adds explicit metric definitions, a concrete delivery-failure example, and a more precise baseline and ablation plan. It preserves the original measured results; it is not a new evaluation.

This is a technical draft, not a peer-reviewed publication or an accepted submission. Author information and venue formatting remain to be finalized.

## Evidence and scope

The fixed study has 60 synthetic development scenarios, three budgets, two cache phases, 360 measured queries, and 180 separate warmups. A budget unit is an estimate from serialized JSON bytes divided by four, not model-token billing. Evidence delivery and edit inputs do not establish autonomous bug discovery or patch correctness.

See the [English evidence record](/paper/evidence.en.md), [selected data](/paper/snapshot.json), [bibliography](/paper/references.bib), and [revision notes](/paper/revision.en.md). The selected data is not the full raw report or a frozen source artifact.

## Reproduction

From the repository root, run the read-only artifact checks:

```sh
python3 tests/paper_artifacts.py
```

The check uses the canonical report named in `snapshot.json`, or a byte-identical archive when one exists. It checks retained data and manuscript consistency; it does not run a new Fitness experiment. See the [paper directory guide](/paper/README.md) for build requirements and the original experiment commands.
