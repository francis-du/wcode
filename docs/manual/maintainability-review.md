---
layout: docs
title: Maintainability Review
description: Structural maintainability review and evidence policy
lang: en
alternate: /zh/docs/maintainability-review/
permalink: /docs/maintainability-review/
---

# Maintainability Review Policy

wcode treats maintainability as an approval concern, not a cosmetic cleanup pass. This policy adapts the useful structural-review ideas from Cursor's `thermo-nuclear-code-quality-review` skill to wcode's Design State, Product Scope, Risk, Verification, and Evidence model.

Source inspiration: `cursor/plugins` → `cursor-team-kit/skills/thermo-nuclear-code-quality-review/SKILL.md`. wcode does not copy that skill as an external runtime dependency; the relevant rules are represented directly in wcode's Design State and Verification flow.

## Two layers of review

### Deterministic structural signals

`review_changes` may surface facts that can be measured from the current Git change without pretending to understand design intent:

- `maintainability-file-crossed-1k`: the current change would push a maintained source file from below 1,000 lines to above 1,000. The review finding explains the change, while the Workspace mutation policy independently blocks that growth.
- `maintainability-concentrated-growth`: a single source file receives at least 400 net new lines. Review whether the change is concentrating multiple responsibilities or missing a simpler model.
- `maintainability-cross-scope-churn`: a source change spans at least three canonical wcode Product Scopes and at least 1,000 changed lines. Review ownership, dependency direction, and whether independent concerns should be separated.

These findings feed the normal Risk Engine. A high-severity maintainability finding can raise the verification depth just like other architecture risks.

The 1,000-line boundary is a portable wcode core policy for maintained source in every authorized Workspace, not only for the wcode repository. `create_file`, `write_file`, `replace_text`, `apply_edits`, and their batch forms reject creating an oversized maintained source file, crossing the boundary, or growing a file that is already oversized. An existing oversized file may stay the same size or shrink while it is being decomposed, but `verify_project` fails closed on the remaining Convention error until the file is brought back under the boundary. Generated/vendor/build output is exempt. wcode deliberately does not auto-slice a file at an arbitrary line number: decomposition must preserve module, API, import, registration, and ownership semantics.

Convention scans use the root `hugo.toml` build configuration to exclude its `publishDir` (default `public`) and treat declared, initialized Git submodules as separate source owners. Select the submodule Workspace to check its own maintained source. Ordinary `public/` directories and uninitialized submodule paths remain in scope. These ownership facts participate in the Convention cache fingerprint; file-write checks retain their existing bounds.

## Independent maintainability reviewer

Medium-and-higher risk Verification Plans include a blind `maintainability` reviewer job with capability `maintainability_review`. The job carries the rubric below so different model providers receive the same approval contract.

The reviewer should prioritize:

1. **Delete complexity first.** Look for a behavior-preserving restructuring that removes branches, helpers, modes, or layers instead of redistributing them.
2. **Stop spaghetti growth.** Scattered feature checks and special-case conditionals are design smells when a clearer model, policy, state machine, helper, or module can own the behavior.
3. **Keep canonical ownership.** Feature logic belongs in the correct Product Scope and layer. Reuse canonical helpers rather than creating a second local implementation.
4. **Make boundaries explicit.** Challenge unnecessary wrappers, casts, optionality, silent fallbacks, or loosely shaped contracts that hide the real invariant.
5. **Enforce source decomposition.** Maintained source must stay at or below 1,000 physical lines. When an existing file is already oversized, make it smaller or split cohesive responsibilities before adding behavior.
6. **Simplify orchestration.** Independent work should fan out when that produces a clearer flow. Related state transitions should be atomic when partial application would make the system harder to reason about.
7. **Prefer high-conviction findings.** Structural regressions and missed simplifications come before naming or formatting nits.

## Approval bar

Correct behavior is necessary but not sufficient. A maintainability reviewer should not Pass while there is a clear structural regression, obvious canonical-helper duplication, avoidable special-case branching, an oversized maintained source file, or an obvious simpler design that materially deletes complexity. Deterministic `core-policy` verification independently blocks hard Convention errors before repository-native checks run.

The reviewer must submit real Verification evidence. A correctness reviewer cannot substitute for the maintainability reviewer, and deterministic growth signals cannot substitute for the model review: they are separate evidence types with separate precision.

## Development flow

Before editing, use Product Scopes and `agent_context` as the compact ownership/context entry point; use `software_context` only when deeper context is needed. During implementation, prefer existing helpers and direct models, and keep related updates atomic. After editing:

```text
review_changes
  → drift_status / impact_analysis / risk_status
  → verification_plan
  → independent maintainability review when required
  → deterministic/stage verification
  → evidence_status
```

If maintainability review identifies a simpler implementation, prefer fixing the structure before accumulating more verification evidence around a design that is already known to be harder to maintain.
