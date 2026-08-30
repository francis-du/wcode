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

A catalog of common development CLIs is pre-authorized at the executable-name layer, but each tool still has a command-specific policy. It now covers Git/GitHub CLI (`gh`), Cargo and common package managers, `just`, `task`, `uv`, `ruff`, `biome`, `deno`, `docker`, `kubectl`, `terraform`, `fd`, `jq`, `cmake`, `ninja`, `dotnet`, `mvn`, `gradle`, `swift`, `zig`, `pre-commit`, and `act`. Strictly local read/check-only shapes such as bounded `fd`, `jq`, Ruff/Biome checks and selected schema/introspection commands may run directly. Repository scripts/build systems, source-writing modes, Docker daemon reads, Kubernetes cluster reads, provider execution, remote mutations, and Compose lifecycle operations require an exact `RiskyExecution` approval. Credential/config redirection and destructive infrastructure shapes remain blocked.

Git mutation remains narrow: only explicit `git add` pathspecs, `git commit -m ...`, and `git push <remote> <refspec>` shapes can enter exact authorization; force/delete/mirror/reset/restore-style mutations remain blocked. An approved `git push` may use the existing SSH Agent through a fixed non-interactive SSH command so normal SSH remotes work. Token-like environment variables, credential helpers, AskPass, arbitrary Git config, proxy helpers, and HTTP extra headers are still stripped, so HTTPS credential mediation is not silently granted.

GitHub CLI has its own bounded policy. Read-only PR/issue/run/workflow/release/repository/search views can run directly. Explicit non-interactive PR/issue creation, comments, workflow dispatch, release creation against an already-existing verified tag, PR merge with an explicit merge method, and run rerun/cancel enter exact authorization. Release asset paths are deliberately separate, and `gh auth`, `gh api`, secrets/variables, extensions, host/repository redirection, admin/auto merge modes, and other credential or policy bypass surfaces remain blocked.

Repository-aware language servers and verification executors are a broader trust boundary because they can load repository-controlled configuration or code. wcode therefore has a separate hardened semantic lane: only built-in providers with an explicit automatic safety profile may run there by default. The first automatic profile is `rust-analyzer`; its executable must resolve outside the Workspace, its environment is scrubbed for credential and execution-injection variables, and wcode disables rust-analyzer build scripts, proc macros, automatic Cargo reload, and check-on-save. The warm session pool is capacity-bounded, keyed by Workspace + provider-binary identity, serializes one provider protocol stream per slot, evicts idle/old slots, closes documents that leave the bounded index set, and rebuilds a slot when the server exits or the provider binary changes. Navigation results are still filtered back through the Workspace boundary. `--no-semantic` disables this lane completely. Language servers without an automatic safety profile and non-deterministic verification executors still require explicit authorization unless the process uses the broader `--allow-risky-exec` pre-authorization. The deterministic Harness verification lane remains a separate exception for fixed check/test/build shapes, including `cargo nextest run [--locked]` when `cargo-nextest` is installed and declared; otherwise Rust verification keeps the `cargo test` fallback. These controls reduce the execution surface but are not an OS sandbox.

## Human authorization is local

Pending authorization requests are visible in the TUI and protected WebUI. The model can request access; it cannot approve its own request.

![wcode authorization and access controls](/assets/img_3.png)

TUI flow:

```text
↑ / ↓  select request
Y      approve
N      deny
```

Command requests have two distinct scopes. **Executable access** permits one
program name in one Workspace. **Exact repository operation** permits one
fingerprinted argument set in that Workspace. The WebUI and TUI show these
labels separately. Approving `cargo` does not approve every `cargo` command;
approving `cargo test` does not cover different arguments or another subspace.
A denial creates no grant.

An approval does not disable Workspace isolation or turn command execution into a shell.

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

The tunnel provides reachability, not authorization.

## Media and model capability

`read_media` is metadata-first. It can identify bounded PNG/JPEG/GIF/WebP images, common audio formats, and MP4/WebM metadata without assuming the connected model is multimodal. Image/audio payloads are emitted only when the client explicitly advertises the `run.francis.wcode/media-content` extension for that kind (and optional MIME filter); unknown or unsupported capability fails closed without sending binary content. Video remains metadata-only because MCP has no standard video Tool Result content block.

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
- Use `--no-semantic` when the runtime should not start any first-party language server.
