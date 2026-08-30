---
name: wcode-software-intelligence
description: Use wcode's Design State, Software Graph, risk, verification, evidence, and reconciliation workflow for safe repository changes.
---

Use the configured wcode MCP server for repository work. Keep instructions
short and use progressive disclosure: load only the project, source, design, or
verification detail needed for the current task.

Before editing:

1. Call `workspace_info` and use the closest discovered subspace for the task.
2. Start with `agent_context(goal, scopes=...)`. Call `scope_status`,
   `design_status`, `project_context`, or `language_quality_status` only when
   readiness or the task requires deeper inspection.
3. Prefer `find_symbol`, `file_outline`, and `symbol_context` to broad reads.
4. Treat Tree-sitter facts as `precision=syntax`; only fresh provider facts are
   semantic.

While editing:

- Stay inside the selected Workspace. Preserve SHA-256 preconditions and use
  bounded wcode edit tools.
- When targets are known, prefer one `apply_edits`, `apply_file_edits`, or
  `create_files` call. Parallelize only independent work.
- Reuse the canonical model or helper before adding another wrapper, mode, or
  special case. A file crossing 1,000 lines needs decomposition or an explicit
  structural reason.
- Treat `delete_path` as exceptional. Never bypass path, symlink, hard-link,
  no-shell, or command-policy checks.
- Authorization has two layers: `CommandAccess` grants one executable in one
  Workspace; `RiskyExecution` grants one exact repository operation. Surface
  the request, let the operator decide in the TUI or protected WebUI, then retry
  only the matching operation.
- Never enable `--allow-risky-exec`, approve a request, or widen a Workspace on
  the user's behalf.

After editing:

1. Run `review_changes`; treat `maintainability-*` findings as structural
   signals.
2. Use `drift_status`, `impact_analysis`, and `risk_status` when the change
   needs them.
3. Run the recommended `verify_project` level. Use `language_quality_run` only
   for a declared, available, check-only provider.
4. Continue `reconciliation_plan` when traceability or drift gaps remain.
5. Finish with `evidence_status`; report failures, disagreement, stale
   revisions, and remaining blockers. Never invent a Stage Pass or
   HumanApproval.

One producer's Pass does not erase another producer's latest Fail.
