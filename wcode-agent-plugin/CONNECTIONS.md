# wcode connection profiles

The canonical package is `skill-only`: its standard `mcp.json` is valid but has
no server entry, because a portable plugin directory is not the repository that
wcode should expose.

Export a repository-bound package when the target is known:

```bash
wcode --workspace "$PWD" agent-plugin --profile local-stdio
wcode --workspace "$PWD" agent-plugin --profile remote-http \
  --remote-url https://your-current-tunnel.example/mcp
```

- `skill-only` keeps `mcpServers` empty.
- `local-stdio` writes the selected canonical repository path into the stdio
  arguments. It never uses the plugin directory as the Workspace.
- `remote-http` writes a Streamable HTTP URL only. OAuth credentials remain in
  the MCP client and are never embedded in the package.
- `auto` chooses `remote-http` when `--remote-url` is present and otherwise uses
  `local-stdio` for the explicitly selected Workspace.

Remote Streamable HTTP at `/mcp` is preferred. Legacy `/sse` plus
`/message?sessionId=...` exists only for compatible older clients and keeps the
same OAuth, Origin, Workspace, Harness, and authorization policies.
