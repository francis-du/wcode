# wcode Agent Plugin

This is a portable Agent Plugins 1.0 skill package. It intentionally does **not** contain `mcp.json`.

wcode must be started with the actual repository as an explicit Workspace. Agent Plugins stdio commands default to the plugin root and the portable specification has no host-workspace variable, so silently bundling `wcode mcp-stdio` would scope wcode to the wrong directory.

Install the Skill using one of the concrete project-local options below, then configure the wcode MCP server separately for the repository you are working in.

## Install the Skill

### Shared Agent Skills path — Codex, Gemini CLI, Grok Build, and compatible agents

```bash
mkdir -p .agents/skills
cp -R wcode-agent-plugin/skills/wcode-software-intelligence .agents/skills/
```

### Claude Code

```bash
mkdir -p .claude/skills
cp -R wcode-agent-plugin/skills/wcode-software-intelligence .claude/skills/
```

For local plugin development without copying the Skill:

```bash
claude --plugin-dir ./wcode-agent-plugin
```

### Gemini CLI

```bash
gemini skills link ./wcode-agent-plugin/skills/wcode-software-intelligence --scope workspace
gemini skills list
```

### Grok Build

Grok reads `.agents/skills/` and Claude-compatible skills directly. To test the whole plugin package without installing it:

```bash
grok --plugin-dir ./wcode-agent-plugin
grok inspect
```

### ZCode

Add this directory as a local marketplace, then install and enable `wcode`. The
ZCode-specific manifest starts `wcode --workspace ${CLAUDE_PROJECT_DIR} mcp-stdio`,
so the open ZCode project remains the explicit Workspace and HTTP OAuth is not
used for this local child process.

### GitHub Copilot CLI

```bash
copilot plugins install --skill --scope project \
  ./wcode-agent-plugin/skills/wcode-software-intelligence/SKILL.md
```

The generated package contains instructions only: no hooks, executable scripts, credentials, or bundled MCP configuration. Keep the agent's own sandbox, trust, and approval controls enabled.

## Configure wcode MCP separately

Recommended local MCP command shape:

```text
wcode --workspace <ABSOLUTE_REPOSITORY_PATH> mcp-stdio
```

The stdio transport uses the same Workspace policy, Harness, Software Intelligence, Tasks, Prompts, Resources, and tools as the HTTP server. It does not use HTTP OAuth because the local child-process boundary is the transport trust boundary.

Repository-aware execution is still explicit. With process-wide `--allow-risky-exec` off, an exact risky operation can stop with a local authorization request; the operator approves it in the wcode TUI and retries. The Skill must never auto-approve that request. Use the flag only when the operator intentionally pre-authorizes repository-aware execution for the whole process.

The shared MCP runtime also exposes wcode Product Scopes. Start with `workspace_info` / `scope_status` / `project_context`; use `scope_status.unmapped_files` to surface architecture gaps, then pass relevant scopes to `software_context` when a task belongs to a bounded capability area. Product Scopes narrow context; they never widen Workspace or execution permissions. Clients that read MCP Resources can inspect `wcode://runtime/product-scopes`.

For source changes, `language_quality_status` reports language support as a capability matrix rather than one boolean: syntax, semantics, repository-declared/native formatter/linter/type/static/test/security providers, plus Property/Mutation/Fuzz/Runtime stages. `language_quality_run` executes only a declared, available, check-only provider through the normal authorization boundary and records current-revision Evidence; it never uses formatter fix/write mode.

The Skill treats always-on instructions as a short map and loads detailed context progressively. Host subagents/worktrees can be used for isolated independent research or review, but shared/dependent writes still belong behind wcode's Scheduler and SHA guards, and model consensus is never deterministic proof.

The Skill also carries wcode's maintainability gate: `review_changes` may emit deterministic `maintainability-*` findings, and medium-and-higher risk Verification Plans require an independent `maintainability_review` job. A correctness Pass does not replace that structural review.
