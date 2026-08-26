use crate::workspace::Workspace;
use crate::{AUTHOR_URL, PROJECT_URL};
use anyhow::{bail, Result};
use serde::Serialize;

const SKILL: &str = r#"---
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
- Preserve SHA-256 edit preconditions and use wcode's bounded edit tools.
- Treat `delete_path` as exceptional: it only deletes one regular file or empty directory after exact one-shot human authorization in the local TUI; never try to bypass or broaden that approval.
- Do not bypass protected paths, symlink/hard-link protections, or command policy.
- Do not auto-enable `--allow-risky-exec` or auto-approve repository-aware execution. If wcode returns an authorization request, surface it to the operator; after the operator approves it in the local TUI, retry the exact operation. The flag is only for intentional process-wide pre-authorization.

After editing:
1. Run `review_changes`. Treat `maintainability-*` findings as structural signals, not style nits.
2. Inspect `drift_status`, `impact_analysis`, and `risk_status` when Git/exec review is available. Medium-and-higher Verification Plans require independent `maintainability_review` evidence; a correctness Pass does not replace it.
3. Create or continue a `reconciliation_plan` when traceability/drift gaps remain.
4. Run the recommended `verify_project` level. Use `language_quality_run` only for a provider that `language_quality_status` reports as repository-declared, available, and check-only; never substitute formatter fix/write mode for verification.
5. Use real Property/Mutation/Fuzz/Runtime-Canary Evidence when required. Never fabricate a Stage Pass or HumanApproval.
6. Finish with `evidence_status` and report failures, disagreement, stale revisions, and remaining blockers.

Verification is fail-closed per producer: one runner's later Pass does not erase another runner's latest Fail.
"#;

#[derive(Debug, Serialize)]
pub(crate) struct AgentPluginExport {
    pub root: String,
    pub files: Vec<String>,
    pub mcp_setup_required: bool,
    pub note: String,
}

pub(crate) fn export(workspace: &Workspace, output: &str) -> Result<AgentPluginExport> {
    let root = output.trim().trim_end_matches('/');
    if root.is_empty() || root == "." {
        bail!("agent plugin output must be a new repository-relative directory");
    }
    workspace.ensure_directory(root)?;
    workspace.ensure_directory(&format!("{root}/.claude-plugin"))?;
    workspace.ensure_directory(&format!("{root}/.zcode-plugin"))?;
    workspace.ensure_directory(&format!("{root}/skills"))?;
    workspace.ensure_directory(&format!("{root}/skills/wcode-software-intelligence"))?;

    let plugin = format!(
        r#"{{
  "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
  "name": "wcode",
  "version": "{}",
  "description": "Portable wcode Software Intelligence skill for MCP-capable coding agents.",
  "author": {{"name": "francis-du", "url": "{}"}},
  "homepage": "{}",
  "repository": "{}",
  "license": "Apache-2.0",
  "keywords": ["mcp", "software-intelligence", "verification", "agent-skills"]
}}
"#,
        env!("CARGO_PKG_VERSION"),
        AUTHOR_URL,
        crate::DOCS_URL,
        PROJECT_URL
    );
    let claude_plugin = format!(
        r#"{{
  "name": "wcode",
  "version": "{}",
  "description": "wcode Software Intelligence skill; MCP is configured separately with an explicit repository workspace.",
  "author": {{"name": "francis-du", "url": "{}"}},
  "repository": "{}",
  "license": "Apache-2.0"
}}
"#,
        env!("CARGO_PKG_VERSION"),
        AUTHOR_URL,
        PROJECT_URL
    );
    let zcode_plugin = format!(
        r#"{{
  "name": "wcode",
  "version": "{}",
  "description": "wcode Software Intelligence skill and workspace-scoped MCP server for ZCode.",
  "author": {{"name": "francis-du", "url": "{}"}},
  "repository": "{}",
  "license": "Apache-2.0",
  "skills": "skills",
  "mcpServers": {{
    "wcode": {{
      "command": "wcode",
      "args": ["--workspace", "${{CLAUDE_PROJECT_DIR}}", "mcp-stdio"],
      "cwd": "${{CLAUDE_PROJECT_DIR}}"
    }}
  }}
}}
"#,
        env!("CARGO_PKG_VERSION"),
        AUTHOR_URL,
        PROJECT_URL
    );
    let zcode_marketplace = format!(
        r#"{{
  "name": "wcode-plugins",
  "description": "Official wcode plugin marketplace.",
  "plugins": [
    {{
      "name": "wcode",
      "source": ".",
      "description": "Workspace-scoped wcode Software Intelligence skill and MCP server.",
      "version": "{}",
      "category": "developer-tools",
      "tags": ["mcp", "software-intelligence", "verification"],
      "strict": true
    }}
  ]
}}
"#,
        env!("CARGO_PKG_VERSION")
    );
    let readme = r#"# wcode Agent Plugin

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
"#;
    let files = vec![
        format!("{root}/plugin.json"),
        format!("{root}/.claude-plugin/plugin.json"),
        format!("{root}/.zcode-plugin/plugin.json"),
        format!("{root}/marketplace.json"),
        format!("{root}/README.md"),
        format!("{root}/skills/wcode-software-intelligence/SKILL.md"),
    ];
    if let Some(existing) = files
        .iter()
        .find(|path| workspace.root().join(path.as_str()).exists())
    {
        bail!("agent plugin export would overwrite existing file: {existing}");
    }
    workspace.create_file(&files[0], &plugin)?;
    workspace.create_file(&files[1], &claude_plugin)?;
    workspace.create_file(&files[2], &zcode_plugin)?;
    workspace.create_file(&files[3], &zcode_marketplace)?;
    workspace.create_file(&files[4], readme)?;
    workspace.create_file(&files[5], SKILL)?;
    Ok(AgentPluginExport {
        root: root.to_owned(),
        files,
        mcp_setup_required: true,
        note: "Configure wcode MCP separately with an explicit absolute repository workspace."
            .into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exported_plugin_is_portable_skill_only_and_does_not_hide_execution() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(dir.path(), true, false).unwrap();
        let exported = export(&workspace, "wcode-agent-plugin").unwrap();
        assert!(dir.path().join("wcode-agent-plugin/plugin.json").is_file());
        assert!(dir
            .path()
            .join("wcode-agent-plugin/.claude-plugin/plugin.json")
            .is_file());
        let zcode_manifest = std::fs::read_to_string(
            dir.path()
                .join("wcode-agent-plugin/.zcode-plugin/plugin.json"),
        )
        .unwrap();
        let zcode_manifest: serde_json::Value = serde_json::from_str(&zcode_manifest).unwrap();
        assert_eq!(
            zcode_manifest["mcpServers"]["wcode"]["args"],
            serde_json::json!(["--workspace", "${CLAUDE_PROJECT_DIR}", "mcp-stdio"])
        );
        let marketplace =
            std::fs::read_to_string(dir.path().join("wcode-agent-plugin/marketplace.json"))
                .unwrap();
        let marketplace: serde_json::Value = serde_json::from_str(&marketplace).unwrap();
        assert_eq!(marketplace["plugins"][0]["source"], ".");
        assert!(dir
            .path()
            .join("wcode-agent-plugin/skills/wcode-software-intelligence/SKILL.md")
            .is_file());
        assert!(!dir.path().join("wcode-agent-plugin/mcp.json").exists());
        let readme =
            std::fs::read_to_string(dir.path().join("wcode-agent-plugin/README.md")).unwrap();
        assert!(readme.contains(".agents/skills"));
        assert!(readme.contains(".claude/skills"));
        assert!(readme.contains("gemini skills link"));
        assert!(readme.contains("grok --plugin-dir"));
        assert!(readme.contains("CLAUDE_PROJECT_DIR"));
        assert!(readme.contains("copilot plugins install --skill --scope project"));
        assert!(readme.contains("wcode --workspace <ABSOLUTE_REPOSITORY_PATH> mcp-stdio"));
        assert!(readme.contains("language_quality_status"));
        assert!(readme.contains("model consensus is never deterministic proof"));
        assert!(exported.mcp_setup_required);
        assert!(SKILL.contains("language_quality_status"));
        assert!(SKILL.contains("progressive disclosure"));
        assert!(SKILL.contains("never deterministic proof"));
        assert!(!SKILL.contains("dangerously-skip"));
        assert!(!SKILL.contains("curl "));
        assert!(export(&workspace, "wcode-agent-plugin").is_err());
    }
}
