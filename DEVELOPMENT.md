# wcode development notes

This document contains implementation constraints and release details. User-facing architecture and usage live in [README.md](README.md).

## Module map

- `src/main.rs` — CLI, tracing mode, tunnel lifecycle, shutdown, and platform dependency handling.
- `src/auth.rs` — OAuth 2.1 dynamic registration, PKCE, pairing, token issuance, and metadata.
- `src/mcp.rs` — JSON-RPC routing, tool schemas, task lifecycle integration, and batch concurrency.
- `src/code_index.rs` — lazy multi-language Tree-sitter parsing, symbol/range extraction, qualified-name search, and the bounded in-memory AST cache.
- `src/harness.rs` — the global semaphore plus cached project context, bounded Git change review, syntax-index ownership, and phased quick/full quality gates.
- `src/workspace.rs` — root isolation, search/read/edit/command implementations, redaction, and atomic writes.
- `src/monitor.rs` — the in-memory task ledger and Ratatui dashboard.

## Runtime invariants

A tool call follows one real lifecycle:

```text
request -> queued -> semaphore acquired -> running -> completed | failed
```

The monitor must never simulate work. Queue, active, completion, failure, request bytes, response bytes, and peak concurrency are updated by the same child-task path that acquires the global semaphore. The global semaphore remains the only concurrency gate.

`--max-parallel-tools` is a cap rather than a target. The default is adaptive: eight times the available logical CPU count, clamped to 64–128; an explicit CLI value may raise it to the Harness maximum of 256. Ordinary tool calls acquire one permit. Composite tools (`parallel_tools`, `review_changes`, and `verify_project`) must not hold a parent permit while waiting for children; every child acquires its own permit. This prevents a one-slot configuration from deadlocking and ensures TUI `Slots`/`Peak` values match real work. `parallel_tools` remains read/discovery-only and bounded to 128 children, 512 KiB per child result, and 8 MiB per aggregate response. Verification uses phase barriers so independent cheap checks can overlap without launching compiler-heavy test, Clippy, and build work together.

The positive coding harness must remain bounded and deterministic: project guidance is limited by file, line, and total-character budgets; cache invalidation follows manifest/guidance metadata; Git change review uses five read-only probes and returns classified metadata and risk findings rather than diff bodies; inferred checks use only existing allowlisted command execution; and diagnostic output is tail-bounded before it returns to the model.

The syntax index must remain lazy, bounded, and honest about precision. Directory symbol searches may prefilter source text before parsing, complete Tree-sitter trees stay capped at 128 files, and successful writes invalidate the affected symbol and AST records. Every model-facing result carries `provider=tree-sitter` and `precision=syntax`; it must not imply compiler-level type resolution, overload selection, macro expansion, or dynamic-dispatch accuracy. Adding a grammar requires a real source fixture that proves at least one definition is extracted, plus extension or special-filename routing coverage where applicable. Symbol signatures and returned bodies continue through the workspace redaction boundary.

Runtime collections are bounded as well: MCP batches accept at most 128 items, `parallel_tools` accepts at most 128 read/discovery children, symbol searches scan at most 50,000 source files and retain at most 128 complete ASTs, change review accepts at most 500 files and 64 findings, monitor traffic history keeps at most 4,096 events, and per-file write-lock entries use weak references so inactive paths are pruned instead of accumulating for the process lifetime.

The dashboard:

- uses Crossterm raw mode and the alternate screen;
- renders only through `Terminal::draw`;
- redraws every 150 ms while work is queued/running and every 500 ms while idle;
- renders a dark, rounded card hierarchy with compact metric fallback, focused Workspace cards, and keycap-style shortcuts;
- renders `Slots active / cap`, a process-lifetime `Peak`, and a utilization bar from real child-task transitions;
- labels token economics as estimates: tool-result bytes are divided by four for `CTX`, only measurable full-source bytes omitted by `file_outline`/`symbol_context` count as saved, and USD uses the configured per-million input-token rate;
- enables Crossterm mouse capture inside the terminal session; footer/help link hit regions are derived from the same responsive layout geometry, while keyboard shortcuts remain available;
- restores mouse capture, raw mode, cursor visibility, and the primary screen through an RAII guard;
- consumes Ctrl-C as a Crossterm key event and forwards shutdown to Tokio;
- does not start when stdout is not a TTY or `--no-monitor` is set;
- suppresses tracing output while active so background logs cannot corrupt the screen.

Quick Tunnel lifecycle is independent of the local HTTP server. The main loop polls the cloudflared child with `try_wait`; an unexpected exit updates shared endpoint/tunnel state for the TUI and `/healthz` but does not terminate MCP. For public endpoints, a separate bounded task also runs `curl` directly (never through a shell) against `/healthz` every 25 seconds with a short timeout; three consecutive failures mark the public URL unavailable, while one success clears the failure streak. Normal shutdown stops the health task and still kills and waits for a live cloudflared child so it cannot become a zombie.

## Security invariants

Changes must preserve:

- canonical workspace-root isolation, root-identity rechecks, absolute-path and parent-traversal rejection; Unix root checks pin device/inode so same-path directory replacement is detected;
- default rejection of filesystem-root/home workspaces and parent/child overlapping workspaces;
- no model-facing delete primitive;
- protected-path denial across direct reads/writes, source indexing, directory traversal, and command arguments;
- symlink-component rejection and Unix hard-link write rejection;
- SHA-256 edit preconditions, per-file locks, post-lock path re-resolution, bounded write size, and destructive-reduction gating;
- create-new semantics that cannot overwrite a raced target, plus fsynced temporary content and atomic replacement for existing files;
- no-shell command execution with an explicit bounded/risky split; model-facing `run_command` admits only constrained Git/ripgrep inspection plus exact `cargo fmt --check`, `cargo check`, and `cargo check --locked` shapes by default, while `verify_project` uses a crate-internal exact-shape lane that temporarily enables only an already-approved inferred verification command before delegating to the same workspace command policy; Git mutation remains blocked regardless of mode, inherited `GIT_*` state is cleared, repository discovery is capped at the selected root, helper-capable Git features/config are overridden or rejected, and Rust response files plus Cargo/Go/package-manager redirection options remain blocked;
- timeout termination, bounded streaming stdout/stderr, sensitive environment scrubbing, and disabled interactive Git prompting/helpers;
- explicit public endpoints must be HTTPS or loopback HTTP base URLs without user information, query strings, or fragments;
- bounded OAuth registration metadata, strict HTTPS/loopback redirect validation, single-use expiring authorization codes, non-expiring capacity-bounded resource-bound access tokens, and atomic expiring refresh-token rotation with lazy expiry cleanup;
- secret redaction and `.env*` scan exclusion.

`--allow-risky-exec`, `--allow-destructive-writes`, `--allow-overlapping-workspaces`, and `--allow-broad-workspace` are trust-boundary expansions. Tests and documentation must treat them as explicit operator decisions, not defaults. Risky execution is not an OS filesystem sandbox.

Do not weaken these constraints for UI, performance, or harness convenience.

## Cross-platform dependency handling

`ensure_cloudflared()` first checks `cloudflared --version`.

- macOS: Homebrew is detected and `brew install cloudflared` may run.
- Windows: winget is detected and the exact `Cloudflare.cloudflared` package may be installed.
- Linux: apt/dnf/yum/pacman are detected only to produce a useful platform hint. Automatic distro installation is deliberately avoided because repository setup differs by distribution.
- `--no-install` disables automatic installation everywhere.

Installer processes are started directly with argument arrays, never through a shell.

## Required verification

Run all checks before release:

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked -- -D warnings
cargo build --release --locked
git diff
git status
```

Cargo/Go/package-manager metadata and quality commands can follow repository-controlled members, path dependencies, symlinks, configuration, build scripts, procedural macros, or tests. The model-facing default lane therefore permits only exact `cargo fmt --check`, `cargo check`, and `cargo check --locked` shapes in addition to constrained Git/ripgrep inspection; every redirection option and broader project command stays behind `--allow-risky-exec`. `verify_project` is the other narrow exception: it can temporarily enable execution only after `validate_verification_command_shape` accepts an exact Harness-inferred command. These lanes are not OS sandboxes—especially `cargo check`, which may run project-controlled code—so never broaden either allowlist into arbitrary arguments.

Tests cover monitor lifecycle, current/peak slot accounting, token/savings accumulation and TUI rendering, mouse link hit-testing, public-endpoint/tunnel state propagation, OAuth redirect policy/registration bounds/code expiry/non-expiring access-token binding/refresh rotation, fan-out order and one-slot deadlock resistance, phased verification, bounded Git review parsing and probes, syntax-index routing through MCP, real fixtures for every embedded grammar family, qualified-name search, text-prefilter misses, AST cache reuse and write invalidation, independent workspace accounting, failures, request/response bytes, small layouts, links, overlapping-root rejection, same-path Unix root replacement, protected paths, destructive-write gating, symlink/hard-link aliases, safe/risky command policy, stale writes, write-lock pruning, command concurrency, and secret redaction.

## Release artifacts

`.github/workflows/release.yml` first runs formatting, locked dependency checking, tests, and Clippy with warnings denied. It then tests and builds optimized release artifacts for:

- Linux x86_64
- macOS Apple Silicon
- macOS Intel
- macOS Universal (Apple Silicon + Intel)
- Windows x86_64

The release profile preserves every runtime feature while minimizing the binary through size optimization, fat LTO, one codegen unit, abort-on-panic release code, disabled incremental compilation, symbol stripping, and narrowly selected Tokio/Axum features. Every packaged binary must pass `wcode --version`; the workflow records exact binary/archive byte counts and uses maximum archive compression. A `v*` tag creates archives and checksums through GitHub Actions.
