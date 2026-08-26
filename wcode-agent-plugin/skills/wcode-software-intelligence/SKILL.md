---
name: wcode-software-intelligence
description: Use wcode's Design State, Software Graph, risk, verification, evidence, and reconciliation workflow for safe repository changes.
---

Use the configured `wcode` MCP server as the software-intelligence control layer for this repository.

Treat always-on agent instructions as a short map, not a giant manual. Load task-specific Design State, Product Scope, symbol, semantic, language-quality, and verification detail on demand through wcode. Skills and repository docs provide progressive disclosure; mandatory policy belongs in Harness gates, authorization, and Evidence rather than in instructions the model is expected to remember.

Before substantial edits:
1. Call `workspace_info`, then `scope_status`, `design_status`, `project_context`, and `language_quality_status` when the task touches source code or quality gates. Treat relevant `scope_status.unmapped_files` and language-quality gaps as explicit architecture/verification debt before adding production modules.
2. Inspect the Product Scope registry exposed by wcode. Choose the scope(s) that bound the requested behavior; canonical scopes include `runtime`, `integrations`, `workspace`, `design`, `graph`, `semantics`, `traceability`, `risk`, `verification`, `evidence`, `reconciliation`, and `experience`.
3. If Design State exists, call `software_context` for the requested requirement, behavior, or subsystem and pass the relevant `scopes` when they are known. Product Scopes narrow context; they do not widen permissions.
4. `project_context` already includes a bounded convention report; call `convention_status` when naming, Product Scope mapping, architecture-domain classification, unclassified root source files, or other repository-architecture findings need separate inspection.
5. Prefer `find_symbol`, `file_outline`, and `symbol_context` over broad file reads.
6. Treat Tree-sitter relationships as `precision=syntax`. Only real fresh provider facts are semantic/runtime precision.
7. Treat language support as a capability vector, not a checkbox: syntax, semantics, format, lint, type/static analysis, tests, security, Property, Mutation, Fuzz, and Runtime-Canary may have different coverage. Prefer repository-declared or language-native providers before introducing a new formatter/linter.
8. When the host supports subagents/worktrees, use isolated workers for independent research, test synthesis, or review. Keep dependent/shared writes behind wcode's scheduler and SHA guards. Multiple model workers agreeing is still model evidence, never deterministic proof.

When editing:
- Before adding a branch, helper, wrapper, mode, or layer, ask whether the behavior can be expressed more directly by deleting complexity or reusing the canonical model/helper. Prefer code-judo simplification over moving the same complexity around.
- Keep feature logic in its canonical Product Scope/layer. Avoid scattered special cases, avoid unnecessary casts/optionality or pass-through abstractions that hide the invariant, and keep independent work parallel / related state updates atomic when that materially simplifies reasoning.
- Treat pushing a file from below 1,000 lines to above 1,000 as a strong change-review smell that needs decomposition or explicit structural justification; keep Convention's 2,000 production-line rule as the separate repository-level oversized-module threshold.
- Stay inside configured Workspace roots.
- Preserve SHA-256 edit preconditions and use wcode's bounded edit tools. When several targets are already known, prefer one `apply_edits`, `apply_file_edits`, or `create_files` call over serial single-file mutations.
- Treat `delete_path` as exceptional: it only deletes one regular file or empty directory after exact one-shot human authorization in the local TUI or protected WebUI; never try to bypass or broaden that approval.
- Do not bypass protected paths, symlink/hard-link protections, the no-shell boundary, or command policy. A model may request a non-default bare executable name, but it must surface the generated `CommandAccess` request and wait for explicit per-Workspace operator approval before retrying.
- Do not auto-enable `--allow-risky-exec` or auto-approve repository-aware execution. If wcode returns an authorization request, surface it to the operator; after the operator approves it in the local TUI or protected WebUI, retry the exact operation. The flag is only for intentional process-wide pre-authorization.

After editing:
1. Run `review_changes`. Treat `maintainability-*` findings as structural signals, not style nits.
2. Inspect `drift_status`, `impact_analysis`, and `risk_status` when Git/exec review is available. Medium-and-higher Verification Plans require independent `maintainability_review` evidence; a correctness Pass does not replace it.
3. Create or continue a `reconciliation_plan` when traceability/drift gaps remain.
4. Run the recommended `verify_project` level. Use `language_quality_run` only for a provider that `language_quality_status` reports as repository-declared, available, and check-only; never substitute formatter fix/write mode for verification.
5. Use real Property/Mutation/Fuzz/Runtime-Canary Evidence when required. Never fabricate a Stage Pass or HumanApproval.
6. Finish with `evidence_status` and report failures, disagreement, stale revisions, and remaining blockers.

Verification is fail-closed per producer: one runner's later Pass does not erase another runner's latest Fail.
