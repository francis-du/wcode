# wcode

`wcode` is a fast, authenticated local MCP bridge for giving ChatGPT scoped access to one or more local development workspaces.

Repository: https://github.com/francis-du/wcode

## Features

- Multiple workspace roots from one process.
- Native MCP concurrency for independent requests and JSON-RPC batch items.
- Configurable global concurrency with `--max-parallel-tools` (default `8`, maximum `64`).
- Per-file write locks so unrelated files can be edited concurrently.
- SHA-256 edit preconditions to reject stale writes.
- Parallel search with Rayon and byte-level substring prefiltering.
- Blocking filesystem work is moved off Tokio's async runtime.
- Bulk `read_files` and `search_many` tools reduce MCP round trips.
- Live terminal task monitor with per-workspace queue, active jobs, success/failure counters, durations, and request/response byte accounting.
- High-confidence context redaction for secrets such as API keys, access tokens, passwords, client secrets, and private-key blocks.
- OAuth 2.1 dynamic client registration, PKCE, and a six-digit pairing code.
- Release builds for Linux, macOS Universal, and Windows x86_64.

## Install

From source:

```bash
cargo install --path .
```

Or download a release archive from:

```text
https://github.com/francis-du/wcode/releases
```

## Start

Expose the current directory:

```bash
wcode --workspace "$PWD"
```

Expose several repositories or directories through the same MCP server:

```bash
wcode \
  --workspace /code/backend \
  --workspace /code/frontend \
  --workspace /code/shared \
  --max-parallel-tools 16
```

Workspace IDs are derived from directory names. The first root is the default. Every file and command tool accepts an optional `workspace` argument; call `workspace_info` to discover the active IDs.

For local-only testing:

```bash
wcode --workspace "$PWD" --no-tunnel
```

Disable file writes and command execution:

```bash
wcode --workspace "$PWD" --read-only --no-exec
```

Use an existing HTTPS reverse proxy:

```bash
wcode --workspace "$PWD" --public-url https://wcode.example.com
```

## Live task monitor

When stdout is an interactive terminal, the live monitor starts automatically and refreshes roughly every 250 ms.

It tracks each workspace independently:

```text
╭─ wcode 0.1.0 · live ───────────────────────────────────────────────
│ MCP        https://example.trycloudflare.com/mcp
│ Pair code  123456
│ Parallel   16 tool calls
├─ Workspaces ───────────────────────────────────────────────────────
│ workspace          calls queue active  done fail    req/resp
│ *backend              18     1      3    14    0   12.4K/91.2K
│  frontend              9     0      1     8    0    7.3K/40.1K
├─ Recent tasks ─────────────────────────────────────────────────────
│ #27   ▶ backend    search_many        running   430ms  1.1K/0B
│ #26   ✓ frontend   read_files         done       18ms  420B/8.8K
╰─ Ctrl-C to stop ───────────────────────────────────────────────────
```

The counters are an in-memory operational ledger for the current process. They include calls, queued/running work, completion/failure counts, elapsed time, and approximate MCP request/response bytes. Nothing is written into the user's repositories for monitoring.

Disable the dynamic monitor when you want plain logs or redirected output:

```bash
wcode --workspace "$PWD" --no-monitor
```

Detailed internal logs remain available through `RUST_LOG`, for example:

```bash
RUST_LOG=wcode=debug wcode --workspace "$PWD" --no-monitor
```

## Fast MCP tools

The server exposes the normal workspace tools plus two bulk fast paths:

- `workspace_info` — list configured roots and capabilities.
- `list_files` — recursively list visible files.
- `search_code` — exact substring search.
- `search_many` — search up to 32 substrings in one traversal.
- `read_file` — read one UTF-8 file and return its SHA-256 edit precondition.
- `read_files` — read up to 32 files in one MCP round trip; failures are isolated per file.
- `replace_text` — atomic exact replacement with stale-write protection.
- `create_file` — atomic new-file creation without overwrite.
- `run_command` — run an allowlisted command without a shell.

Independent tool calls may execute concurrently. Dependent flows such as `read_file -> replace_text` should remain sequential.

## Multi-workspace editing

Every file and command tool is resolved against exactly one configured root. Absolute paths and `..` traversal are rejected, and symlinks that resolve outside the selected workspace are blocked.

Writes use a lock per target file instead of one lock for the whole workspace. Two edits to different files can therefore proceed concurrently. `replace_text` rechecks the supplied SHA-256 after it acquires the target-file lock, so concurrent stale edits fail rather than overwrite newer content.

## Context minimization and secret redaction

`wcode` minimizes model-facing workspace context without changing the meaning of the requested task:

- `.env*`, logs, IDE metadata, dependency trees, and build output are skipped by scans.
- Command subprocesses have environment variables with secret/token/password/key-like names removed.
- `read_file`, `read_files`, `search_code`, and `search_many` redact high-confidence literal secret assignments and private-key blocks before returning content through MCP.
- Redacted file reads include `redacted: true`; the SHA-256 still refers to the original on-disk file so stale-write protection remains correct.

This privacy layer is for secret protection and context minimization. It does not disguise the nature of a task or attempt to make coding activity appear to be something else.

## Connect ChatGPT

1. Start `wcode` and keep the terminal open.
2. In ChatGPT Developer mode, add a custom MCP server.
3. Paste the MCP URL shown in the terminal.
4. Choose OAuth authentication when prompted.
5. Enter the six-digit pairing code on the `wcode` authorization page.

The local status and OAuth pages use the same dark UI and link to https://github.com/francis-du/wcode.

## Tunnel behavior

By default `wcode` starts a Cloudflare Quick Tunnel. On macOS, a missing `cloudflared` can be installed automatically with Homebrew. On Linux and Windows, install `cloudflared` yourself, use `--public-url`, or use `--no-tunnel` for local protocol testing.

## Releases and GitHub Actions

`.github/workflows/release.yml` tests and packages all supported platforms:

- Linux x86_64 — `wcode-linux-x86_64.tar.gz`
- macOS Universal (Apple Silicon + Intel) — `wcode-macos-universal.tar.gz`
- Windows x86_64 — `wcode-windows-x86_64.zip`

Pushes and pull requests build downloadable workflow artifacts. A tag such as `v0.1.0` creates a GitHub Release with all archives and a `SHA256SUMS` file.

Example:

```bash
git tag v0.1.0
git push origin v0.1.0
```

## Security boundaries

- Workspace paths are canonicalized and cannot escape configured roots.
- New files never overwrite existing files.
- Existing-file edits require an exact SHA-256 precondition.
- Atomic replacement is supported on macOS/Linux and Windows.
- Commands run without a shell and with bounded output/timeouts.
- Destructive Git subcommands such as `push`, `clean`, `reset`, `checkout`, `restore`, and `rebase` are blocked when invoked through the MCP command tool.
- OAuth clients, authorization codes, access tokens, and refresh tokens are held only in memory.

`wcode` does not bypass ChatGPT usage limits. MCP supplies tools to a conversation; it does not turn the ChatGPT web app into an externally callable model API.

