---
name: wcode
description: Use the wcode plugin and its MCP tools for repository coding, review, debugging, refactoring, architecture, Graph/Design inspection, safe edits, command execution, and verification. Trigger whenever wcode is installed or named, even when its tools are lazily registered or omitted from the host's initial tool list.
---

Tool visibility bootstrap:

- Treat this Skill as proof that the wcode plugin may be installed, not as proof
  that its MCP tools are absent. Tool visibility and Skill visibility are
  separate Host capabilities.
- Look first for the canonical `mcp__wcode__*` namespace. Some Hosts lazy-load
  or abbreviate tools, so inspect the available/deferred tool registry before
  concluding that wcode is unavailable. Search at minimum for `wcode`,
  `agent_context`, `worklist_status`, `software_graph`, `find_symbol`,
  `read_file`, `apply_edits`, `review_changes`, and `verify_project`.
- Once found, use the real wcode MCP tools and keep one namespace for the task.
  Do not switch to an alias such as `hcode` merely because both are registered.
- The minimum coding chain is `agent_context` first, guarded wcode reads/edits
  next, then `review_changes` and `verify_project`; use Design, Graph, drift,
  impact, risk, Evidence, and Verification tools when readiness or risk calls
  for them.

Use the configured wcode MCP server for repository work. Prefer wcode's repository
primitives over overlapping generic filesystem, shell, search, or code-edit tools
when wcode is available for the same action; do not mix competing tool state or
workspace assumptions in one change unless wcode explicitly cannot perform the
operation. A Host may register MCP tools lazily: first use directly visible
`mcp__wcode__*` tools; if the expected tools are absent, inspect the Host's
available/deferred tool registry for `wcode`, `agent_context`, `apply_edits`,
`review_changes`, and `verify_project` before declaring wcode unavailable. Do
not treat loading this Skill, running Cargo directly, or invoking only the wcode
CLI as equivalent to using the wcode MCP workflow. Use the CLI only after the
MCP server is genuinely unavailable or for operator lifecycle commands, and
state that fallback explicitly. Keep instructions short and use progressive disclosure:
load only the project, source, design, or verification detail needed for the current task.

Operator lifecycle (only when the user asks about installing, configuring, or
upgrading wcode; do not run these during ordinary coding work):

- Install wcode with the verified platform installer documented by the project.
- Run `wcode setup` for interactive Host configuration. Global setup is the
  recommended first choice; project setup is available when repository-local
  config is desired. Setup does not require a `plugin/` directory beside the
  user's project because the binary embeds the canonical package.
- Local MCP configuration should be only `wcode mcp-stdio`; do not add
  `--workspace` unless the user intentionally overrides the Host working
  directory. The Host current working directory is the default Workspace.
- Upgrade with `wcode update`.
- Never run setup/update or widen access merely because coding work is blocked.
  Surface the blocker and let the user choose.

Before editing:

1. Start with `agent_context(query, scopes=...)`. Follow
   `readiness.change_strategy` and `readiness.complexity_budget`: a normal
   single-target task starts as `minimal_patch` with no new production file,
   abstraction, config knob, or public API change. Escalate only when current
   requirements, existing duplication, or a proven ownership/boundary problem
   requires it. If an active Worklist is returned, treat it as the task-progress authority: preserve unfinished
   items, use `worklist_status` when resuming, and use revision-safe
   `worklist_update` to append work or advance status instead of replacing the
   list. Omit `workspace`, `budget`, limits, timeouts, and default paths unless
   the task actually needs an override. Use `workspace_info` only when multiple roots/subspaces make the
   target ambiguous. Call `scope_status`, `design_status`, `project_context`, or
   `language_quality_status` only when readiness or the task requires deeper
   inspection.
2. Prefer `file_outline`, `find_symbol`, and `search_code` for structure-first
   localization before loading source bodies. Use `symbol_context` when the
   implementation body is actually needed; it may return up to 1,000 original
   source lines in one call. Use `read_file` / `read_files` for surrounding or
   cross-file context rather than repeatedly fetching tiny slices. Do not minify,
   reformat, or strip syntax from source shown to the model: preserve original
   indentation and line structure so review and exact edits remain reliable.
   Use `semantic_navigation` only when readiness or the task needs cross-file
   references, callers/callees, implementations, hover, or semantic impact;
   prefer `path + symbol` over model-computed LSP offsets. If readiness reports
   syntax-only relationships plus `semantic_provider_hints`, inspect or refresh
   the provider first. A discovered non-automatic LSP may require operator
   authorization; surface that request and retry after approval rather than
   silently treating Tree-sitter-only results as complete semantic evidence.
3. Treat Tree-sitter facts as `precision=syntax`; only fresh provider facts are
   semantic. `semantic_provider_status` distinguishes missing, discovered,
   authorization-required, initialize-pending, and live providers; follow its
   `action` instead of guessing from a generic process error. Hardened
   first-party semantics may be maintained automatically; `--no-semantic` is
   the operator opt-out.

While editing:

- Stay inside the selected Workspace. Preserve SHA-256 preconditions and use
  bounded wcode edit tools.
- Use a parallel-first execution bias. Before the next tool call, split work
  into dependency lanes. When `readiness.parallelism.strategy` is
  `top_level_concurrent_calls`, launch concurrent top-level wcode calls for
  independent discovery, reads, checks,
  reviews, and non-overlapping file edits together in that next action; do not
  serialize them merely because sequential execution is simpler. Serialize only
  true output dependencies, shared mutable state, or overlapping writes. When
  targets are known, prefer one `apply_edits`, `apply_file_edits`, or
  `create_files` call. Keep arguments minimal. Use `parallel_tools` only for
  compact child arguments; its parent Workspace is inherited by children unless
  a child intentionally overrides it.
- Reuse the canonical owner/model/helper before adding another wrapper, mode,
  config option, extension point, compatibility layer, or subsystem. Do not add
  architecture for hypothetical future use. Exceed the reported complexity
  budget only with concrete current-task evidence. A file crossing 1,000 lines
  needs decomposition or an explicit structural reason.
- Treat `delete_path` as exceptional. Never bypass path, symlink, hard-link,
  no-shell, or command-policy checks.
- Authorization has separate scopes: `CommandAccess` grants one executable in
  one Workspace; `RiskyExecution` grants only the fingerprinted trust requested
  by the operation. That is normally one exact repository operation, while a
  non-automatic LSP server uses Workspace + server + current binary identity
  trust so a warm session can be reused without silently widening to a
  replacement binary or another server.
  Surface the request, let the operator decide in the TUI or protected WebUI,
  then retry only the matching operation.
- Never enable `--allow-risky-exec`, `--full-access`, approve a request, or widen
  a Workspace on the user's behalf. If the user explicitly chooses Full Access
  in the TUI or CLI, treat it as an operator decision; hard protected-path,
  symlink/hard-link, no-shell, and filesystem-root boundaries still remain.

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
