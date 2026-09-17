---
layout: docs
title: Security
description: Workspace, command, authorization, OAuth, and evidence security boundaries
lang: en
alternate: /zh/docs/security/
permalink: /docs/security/
---

# Security Model

wcode is designed around a simple rule: connecting a model must not implicitly expose the machine.

![wcode security and authorization boundary](/assets/wcode-security-boundary.svg)

## Workspace isolation

Only configured Workspace roots exist from the model's point of view. Model-facing file operations reject absolute paths, parent traversal, protected paths, symlink components, workspace escape, and unsafe hard-link cases.

Use repository roots, not a home directory or filesystem root.

Project markers inside a configured root may become derived subspaces. This
does not widen the outer root: relative paths resolve from the selected
Workspace, canonical paths are checked again, and symlink children are
rejected. Manually overlapping configured roots remain blocked by default.

## Hash-guarded writes

Existing-file edits use the observed SHA-256 as a precondition. Atomic replacement prevents partial writes, while stale revisions fail instead of silently overwriting newer work.

Deletion is deliberately separate: one regular file or empty directory requires an exact one-shot local authorization; recursive/root/protected/symlink/hard-link deletion remains blocked.

## No shell execution primitive

`run_command` accepts a bare executable plus argument array. It does not interpret shell syntax. Shell interpreters and path-bearing program names remain blocked from the model-facing execution path.

The default development catalog covers representative native and ecosystem tools for all 22 indexed languages, and each family still has command-specific policy rather than blanket process trust. The bounded operator/development catalog includes `gh`, `just`, `task`, `uv`, `ruff`, `biome`, `deno`, `docker`, `kubectl`, `terraform`, `fd`, `jq`, `cmake`, `ninja`, `dotnet`, `mvn`, `gradle`, `swift`, `zig`, `pre-commit`, and `act`; language-native compilers, formatters, linters, type checkers and test runners are governed separately by exact policy. Rust verification may use installed cargo-nextest and retains `cargo test` as the compatible fallback. Bounded local tests, checks, builds, format/lint, workspace scripts, code generation, dependency maintenance, Docker development workflows, and repository-declared verification executors run autonomously through the hardened Workspace lane. Source-writing modes are tracked against read-only Workspace policy. Inline/interactive interpreter eval where unsafe, compiler/plugin/agent injection, shell interpreters, Workspace escapes, protected paths, credential/config redirection, host-wide tool/runtime mutation, package publication/ownership, remote administrative mutation, and destructive infrastructure shapes remain authorized or permanently blocked according to consequence.

Git mutation remains narrow: only explicit `git add` pathspecs, `git commit -m ...`, and `git push <remote> <refspec>` shapes can enter exact authorization; force/delete/mirror/reset/restore-style mutations remain blocked. An approved `git push` may use the existing SSH Agent through a fixed non-interactive SSH command so normal SSH remotes work. Token-like environment variables, credential helpers, AskPass, arbitrary Git config, proxy helpers, and HTTP extra headers are still stripped, so HTTPS credential mediation is not silently granted.

GitHub CLI has its own bounded policy. Read-only PR/issue/run/workflow/release/repository/search views can run directly. Explicit non-interactive PR/issue creation, comments, workflow dispatch, release creation against an already-existing verified tag, PR merge with an explicit merge method, and run rerun/cancel enter exact authorization. Release asset paths are deliberately separate, and `gh auth`, `gh api`, secrets/variables, extensions, host/repository redirection, admin/auto merge modes, and other credential or policy bypass surfaces remain blocked.

Repository-aware LSP servers can load repository-controlled configuration or code, so wcode keeps a separate hardened LSP lane: only built-in servers with an explicit automatic safety profile may run there by default. `rust-analyzer` must resolve outside the Workspace and also pass a bounded executable-viability probe; a rustup shim whose component is absent is treated as unavailable instead of triggering repeated `semantic_auto` failures. Its environment is scrubbed for credential and execution-injection variables, and wcode disables rust-analyzer build scripts, proc macros, automatic Cargo reload, and check-on-save. The warm session pool is capacity-bounded, keyed by Workspace + server binary identity, serializes one LSP protocol stream per slot, evicts idle/old slots, closes documents that leave the bounded index set, and rebuilds a slot when the server exits or its binary changes. Navigation results are still filtered through the Workspace boundary. `--no-semantic` disables this lane completely. LSP servers without an automatic safety profile still require explicit authorization. Repository-declared Property/Mutation/Fuzz/Runtime executors instead use a hardened autonomous local lane with no shell, bounded cwd/process/output/time, sensitive-environment scrubbing and Workspace path checks. These controls reduce the execution surface but are not an OS sandbox.

## Human authorization is local

Pending authorization requests are visible in the TUI and protected WebUI. The model can request access; it cannot approve its own request.

![wcode authorization and access controls](/assets/wcode-access-management.png)

TUI flow:

```text
↑ / ↓  select request
A      authorize all commands for this Workspace session
Y      approve this exact request
N      deny
```

The command view also uses **F** to toggle the Workspace-wide session grant. The protected WebUI exposes the same switch, and stdio elicitation offers `exact`, `all_commands`, or `deny` when the client supports forms.

Command authorization therefore has two human-selected modes. **Exact authorization** keeps executable access and one fingerprinted repository operation separate. **All commands for this Workspace session** is intentionally unrestricted for direct command execution: once the operator enables it, WCode stops rejecting executables, shells, argument shapes, Git/GitHub operations, credential/publish commands, or other command families for that Workspace until the grant is revoked or the runtime exits. It also supersedes startup `--read-only` / `--no-exec` for direct commands. Resource limits, timeout/cancellation ownership and bounded output remain active. WCode file tools keep their separate Workspace/path/SHA/delete protections.

`RiskyExecution` remains the fingerprint-scoped trust mechanism for exact approval. The Workspace-wide grant is a separate runtime-only operator choice layered above those otherwise-authorizable command requests. For non-automatic LSP servers, exact mode continues to bind Workspace + server + current binary identity; all-command mode intentionally suppresses repeated command/RiskyExecution prompts only inside the selected Workspace.

Exact approval keeps command inspection enabled. Session-wide all-command trust is deliberately different: a command itself may access whatever the host OS/user account permits, while WCode's own file primitives remain Workspace-isolated.

## OAuth and remote MCP

Cloud/web clients normally connect through the protected `/mcp` Resource.
Legacy clients may use `/sse` plus the session-specific `/message` endpoint.
Both remote transports keep:

- Protected Resource Metadata and Authorization Server Metadata;
- Authorization Code + PKCE;
- bounded Dynamic Client Registration compatibility;
- exact redirect handling;
- resource-bound access/refresh tokens;
- refresh-token rotation;
- Origin validation for browser-originated requests.

Client registrations and access/refresh tokens have no clock expiry. They are
persisted per configured Workspace-root set in the user's wcode state directory
and loaded after a process restart. Writes are atomic; Unix files are restricted
to mode `0600`, symlink state files are rejected, malformed state fails closed,
and authorization codes remain short-lived and memory-only. The stores retain
fixed entry limits: an unbound client registration may be reclaimed at client
capacity, and token capacity evicts the oldest token instead of growing without
bound.

A replacement tunnel is accepted only after its public health response matches
the current process. The saved token resource may then migrate to that active
endpoint. OAuth metadata and authorization use the exact Host that received the
request. Historical resources are not active Hosts, and tokens stored for a
different configured Workspace-root set are not loaded.

Host validation and Origin validation are separate checks. A present Origin must be a single valid HTTP(S) origin matching a configured or instance-verified **active** endpoint, including an active non-primary tunnel alias. Missing Origin remains valid for non-browser clients, but malformed, duplicate, credential-bearing or historical-only origins are rejected. Arbitrary client-site origins are not implicitly trusted. Requests still require the original bearer or protected WebUI credential, and legacy SSE sessions remain owner/endpoint-bound.

Verified tunnels enter the trusted endpoint registry before their links are displayed or setup readiness is signalled. A rejected request reports `untrusted_host` or `untrusted_origin` without echoing credentials. For `untrusted_host`, reconnect to a currently registered endpoint and ensure the reverse proxy preserves that public Host; untrusted `Forwarded` / `X-Forwarded-Host` headers cannot register an endpoint. Never disable Origin checking or trust all hosts to repair a connection.

The tunnel provides reachability, not authorization.

## Media and model capability

`read_media` is metadata-first. It can identify bounded PNG/JPEG/GIF/WebP images, common audio formats, and MP4/WebM metadata. The caller explicitly opts into binary payloads with `include_content=true`, which returns standard MCP `image` / `audio` Tool Result content blocks; no private capability extension is required. Video remains metadata-only because MCP has no standard video Tool Result content block.

## Secrets and model context

Credential-like paths are blocked and high-confidence secret text is redacted from model-facing reads and symbol context. Logs and diagnostics must not expose access tokens, refresh tokens, PKCE verifiers, or equivalent credentials. The OAuth state file contains bearer credentials; do not copy it into a repository or share it.

## Verification and Evidence

Security is also an approval problem after code changes. Risk analysis may raise verification depth; deterministic checks, independent reviewers, stage executors, and HumanApproval evidence remain separate producers. One model verdict cannot erase another producer's failure.

## Safe defaults

- Prefer one repository Workspace.
- Prefer stdio for local agents.
- Keep OAuth for Streamable HTTP and legacy SSE connectors.
- Keep SHA preconditions on edits.
- Approve exact command/risky-operation requests instead of enabling broad trust when possible.
- Use `--read-only` or `--no-exec` when a task does not need writes or commands.
- Use `--no-semantic` when the runtime should not start any first-party LSP server.
