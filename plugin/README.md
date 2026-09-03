# wcode plugin

`plugin/` is the repository source package for the portable `wcode` Skill and
Agent Plugins manifests. It is not a runtime dependency: the wcode binary
embeds these files at compile time, so users can run setup from any project and
do not need a `plugin/` directory in their current working directory.

The checked-in package is `skill-only`: `mcp.json` is valid and its
`mcpServers` object is empty. Setup or an explicit export supplies the MCP
profile when needed.

## Install, update, and configure

Install wcode with the verified platform installer, then use `wcode update` for
future upgrades. Normal local configuration is one command:

```bash
wcode setup
```

Interactive setup offers **Global (recommended)** first and **Current project**
second. Global setup writes only verified user-level Host config paths and asks
for a local TTY confirmation before changing them. `wcode setup --project`
forces project scope; `wcode setup --dry-run` previews changes.

Both scopes install the same minimal stdio command: `wcode mcp-stdio`. No
repository path is embedded; the MCP Host working directory becomes the default
Workspace. Existing servers are preserved and unknown/JSONC/YAML schemas stay
manual.

The hidden `agent-plugin` command remains for advanced standalone package export:

```bash
# Instructions only
wcode agent-plugin --profile skill-only

# stdio profile; the Host working directory becomes the Workspace
wcode agent-plugin --profile local-stdio

# Remote endpoint; the client still owns OAuth
wcode agent-plugin \
  --profile remote-http \
  --remote-url https://current-host.example/mcp
```

See [CONNECTIONS.md](CONNECTIONS.md) for profile behavior.

## Host-specific package metadata

- `.claude-plugin/plugin.json` supports local Claude plugin development.
- `.codex-plugin/plugin.json` exposes the Skill and a profile-dependent MCP map.
- `.zcode-plugin/plugin.json` exposes the same portable Skill without guessing a
  project-root variable.
- `marketplace.json` describes this directory for compatible marketplaces.

These files are adapters around one Skill. They do not contain hooks,
executable scripts, credentials, or a second copy of wcode's security rules.
The package directory is repository/build input only: installed users do not
need it in their current directory because the `wcode` binary embeds the
canonical Skill and manifests used by setup/export.

## MCP boundary

Local agents should run only:

```text
wcode mcp-stdio
```

The MCP Host working directory is the default Workspace. Use `--workspace` only
as an explicit operator override; setup never needs to bake a repository path
into the Host configuration.

Remote agents should prefer Streamable HTTP at `/mcp` with OAuth. Older clients
may use `/sse` plus the session-specific `/message` endpoint; that path keeps
the same OAuth, Origin, Workspace, Harness, and authorization checks.

Hardened first-party LSP servers may keep a bounded warm session by default. The Skill uses Tree-sitter/search for localization and `semantic_navigation` for cross-file relationships. `--no-semantic` disables LSP execution. Other LSP servers require local `RiskyExecution` trust bound to Workspace + server + current binary identity.

Repository-aware commands can still stop for local approval. The Skill never
approves executable access, repository-operation trust, LSP
session trust, or Full Access on the user's behalf.

## Skill behavior

Once wcode is connected, the Skill treats wcode as the primary repository tool
surface for discovery, source reads, edits, commands, review, verification, and
persistent Worklist progress. It avoids mixing another repository filesystem,
edit, or command tool into the same task unless wcode lacks the capability or
the user explicitly asks; mixed tools can bypass Workspace selection, stale SHA
checks, and Worklist state.

Source is preserved exactly rather than minified for the model. Models localize
with symbols first, then read the necessary body/range; one source read is
bounded to at most 1,000 original lines.

If work truly needs the current user's Home, the user can press `P` in the TUI
and confirm Full Access or explicitly launch with `--full-access`. Hard
protected-path, filesystem-root, symlink/hard-link, no-shell, and command-policy
boundaries remain enforced.
