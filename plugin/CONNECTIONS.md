# wcode connection profiles

The canonical package is `skill-only`: its standard `mcp.json` is valid but has
no server entry, because a portable plugin directory is not the repository that
wcode should expose.

Normal installed usage does not require an exported package. `wcode setup`
configures local Hosts with `wcode mcp-stdio`, so the Host working directory is
the Workspace. Export a standalone package only when a package artifact is
actually needed:

```bash
wcode agent-plugin --profile local-stdio
wcode agent-plugin --profile remote-http \
  --remote-url https://your-current-tunnel.example/mcp
```

- `skill-only` keeps `mcpServers` empty.
- `local-stdio` writes the same minimal `wcode mcp-stdio` command. The consuming
  Host working directory becomes the Workspace; the plugin directory is never
  treated as source code.
- `remote-http` writes a Streamable HTTP URL only. OAuth credentials remain in
  the MCP client and are never embedded in the package.
- `auto` chooses `remote-http` when `--remote-url` is present and otherwise uses
  current-directory `local-stdio`.

Remote Streamable HTTP at `/mcp` is preferred. Legacy `/sse` plus
`/message?sessionId=...` exists only for compatible older clients and keeps the
same OAuth, Origin, Workspace, Harness, and authorization policies.
