---
layout: docs
title: Architecture and module boundaries
nav_title: Architecture boundaries
description: Enforceable guidance for splitting files, modules, directories, crates and repositories
lang: en
alternate: /zh/docs/architecture-boundaries/
permalink: /docs/architecture-boundaries/
---

# Architecture and module boundaries

wcode treats physical structure as an engineering constraint, not a style preference. `convention_status` reports both concrete findings and an `architecture_guidance` contract so agents know when to split before a file or directory becomes a maintenance hotspot.

## Split thresholds

| Signal | Default behavior |
| --- | --- |
| Maintained source reaches **600 lines** | Warning: review a responsibility-based file/module split before more behavior is added. |
| Maintained source exceeds **1,000 lines** | Error: further growth is blocked; decomposition is required. Generated source remains exempt. |
| One directory contains **24+ maintained source files** | Warning: introduce cohesive submodules/subdirectories instead of extending a flat bucket. |
| Rust crate root accumulates **16+ non-entry modules** | Warning: move responsibilities under `src/<domain>/`; repeated prefixes are an additional split signal. |
| A responsibility has a stable public contract and one-way dependency direction | Consider a crate or repository extraction. Do not extract merely to make a diagram look cleaner. |

These thresholds are prompts to inspect ownership, not instructions to make arbitrary tiny files. A good boundary normally separates at least one of: protocol, storage, UI, orchestration, domain logic, lifecycle, authority, deployment trust, or independently testable contract.

## File vs module vs directory vs crate vs repository

**Split a file** when one maintained source file starts carrying multiple independent responsibilities or approaches the 600-line advisory threshold.

**Create a module/directory** when several files share one domain owner, naming prefix, lifecycle, or test boundary. Prefer `src/<domain>/` over many prefixed files at the crate root.

**Extract a crate** only when the dependency can remain one-way, the contract is independently testable, and the extraction does not duplicate canonical state. Cross-domain immutable facts are appropriate candidates; stateful orchestration frequently is not.

**Extract a repository** when source-control trust, licensing, release cadence, deployment authority, or access control is materially different. Repository separation must not create a reverse dependency or private source requirement for the public product.

The commercial split follows this rule: public `wcode` owns canonical engineering truth and Apache-2.0 product capabilities; the separate commercial repository consumes public contracts one-way.

## Agent behavior

Before adding behavior to a warned file or dense directory, agents should inspect `convention_status` and the existing Product Scope. Prefer moving an existing cohesive responsibility before inventing another abstraction. When an architecture move changes ownership or public contracts, update Design State and run the architecture/standalone gates.

A warning does not by itself authorize a large refactor. Preserve public paths and state compatibility unless the task explicitly changes them. A hard `oversized-source-module` finding, by contrast, prevents maintained-source growth until the module is reduced or split.
