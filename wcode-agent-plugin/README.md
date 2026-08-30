# wcode Agent Plugin

This directory is the source package used by the repository and by
`wcode agent-plugin`. It contains Agent Plugins 1.0 metadata, Host manifests,
the portable Skill, and a standard `mcp.json`.

The checked-in package is `skill-only`: `mcp.json` is valid and its
`mcpServers` object is empty. A portable plugin does not know which source
repository the user intends to expose, and the plugin directory is not a safe
guess.

## Configure a repository

Preview every supported project-local change:

```bash
wcode --workspace "$PWD" agent-plugin --install-all --dry-run
```

Apply it:

```bash
wcode --workspace "$PWD" agent-plugin --install-all
```

The installer merges `wcode` into known JSON or TOML files. It keeps unrelated
MCP servers, leaves unknown/JSONC/YAML schemas alone, and reports account or UI
setup as `manual`.

To export a standalone package instead:

```bash
# Instructions only
wcode --workspace "$PWD" agent-plugin --profile skill-only

# stdio bound to this exact repository
wcode --workspace "$PWD" agent-plugin --profile local-stdio

# Remote endpoint; the client still owns OAuth
wcode --workspace "$PWD" agent-plugin \
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

## MCP boundary

Local agents should run:

```text
wcode --workspace <ABSOLUTE_REPOSITORY_PATH> mcp-stdio
```

Remote agents should prefer Streamable HTTP at `/mcp` with OAuth. Older clients
may use `/sse` plus the session-specific `/message` endpoint; that path keeps
the same OAuth, Origin, Workspace, Harness, and authorization checks.

Repository-aware commands can still stop for local approval. The Skill never
approves executable access or an exact repository operation on the user's
behalf.
