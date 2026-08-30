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
3. Prefer `find_symbol`, `file_outline`, `search_code`, and `symbol_context` for
   cheap localization. Use `semantic_navigation` only when readiness or the task
   needs cross-file references, callers/callees, implementations, hover, or
   semantic impact; prefer `path + symbol` over model-computed LSP offsets.
4. Treat Tree-sitter facts as `precision=syntax`; only fresh provider facts are
   semantic. Hardened first-party semantics may be maintained automatically;
   `--no-semantic` is the operator opt-out.

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
- Authorization has separate scopes: `CommandAccess` grants one executable in
  one Workspace; `RiskyExecution` grants only the fingerprinted trust requested
  by the operation. That is normally one exact repository operation, while a
  non-automatic language server uses Workspace + Provider + current binary
  identity trust so a warm session can be reused without silently widening to
  a replacement binary or other providers.
  Surface the request, let the operator decide in the TUI or protected WebUI,
  then retry only the matching operation.
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
