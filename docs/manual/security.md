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

## Hash-guarded writes

Existing-file edits use the observed SHA-256 as a precondition. Atomic replacement prevents partial writes, while stale revisions fail instead of silently overwriting newer work.

Deletion is deliberately separate: one regular file or empty directory requires an exact one-shot local authorization; recursive/root/protected/symlink/hard-link deletion remains blocked.

## No shell execution primitive

`run_command` accepts a bare executable plus argument array. It does not interpret shell syntax. Shell interpreters and path-bearing program names remain blocked from the model-facing execution path.

A catalog of common development CLIs is pre-authorized at the executable-name layer, but each tool still has a command-specific policy. It now covers Git/GitHub CLI (`gh`), Cargo and common package managers, `just`, `task`, `uv`, `ruff`, `biome`, `deno`, `docker`, `kubectl`, `terraform`, `fd`, `jq`, `cmake`, `ninja`, `dotnet`, `mvn`, `gradle`, `swift`, `zig`, `pre-commit`, and `act`. Strictly local read/check-only shapes such as bounded `fd`, `jq`, Ruff/Biome checks and selected schema/introspection commands may run directly. Repository scripts/build systems, source-writing modes, Docker daemon reads, Kubernetes cluster reads, provider execution, remote mutations, and Compose lifecycle operations require an exact `RiskyExecution` approval. Credential/config redirection and destructive infrastructure shapes remain blocked.

Git mutation remains narrow: only explicit `git add` pathspecs, `git commit -m ...`, and `git push <remote> <refspec>` shapes can enter exact authorization; force/delete/mirror/reset/restore-style mutations remain blocked. An approved `git push` may use the existing SSH Agent through a fixed non-interactive SSH command so normal SSH remotes work. Token-like environment variables, credential helpers, AskPass, arbitrary Git config, proxy helpers, and HTTP extra headers are still stripped, so HTTPS credential mediation is not silently granted.

GitHub CLI has its own bounded policy. Read-only PR/issue/run/workflow/release/repository/search views can run directly. Explicit non-interactive PR/issue creation, comments, workflow dispatch, release creation against an already-existing verified tag, PR merge with an explicit merge method, and run rerun/cancel enter exact authorization. Release asset paths are deliberately separate, and `gh auth`, `gh api`, secrets/variables, extensions, host/repository redirection, admin/auto merge modes, and other credential or policy bypass surfaces remain blocked.

Repository-aware language servers and verification executors are a broader trust boundary because they can load repository-controlled configuration or code. These exact operations require explicit authorization unless the process was started with the broader `--allow-risky-exec` pre-authorization. One exception is the deterministic Harness verification lane: fixed check/test/build shapes are approved by policy, including `cargo nextest run [--locked]` when `cargo-nextest` is both installed and declared by the repository; otherwise Rust verification keeps the `cargo test` fallback.

## Human authorization is local

Pending authorization requests are visible in the TUI and protected WebUI. The model can request access; it cannot approve its own request.

![wcode authorization and access controls](/assets/img_3.png)

TUI flow:

```text
↑ / ↓  select request
Y      approve
N      deny
```

An approval does not disable Workspace isolation or turn command execution into a shell.

## OAuth and remote MCP

Cloud/web clients connect through the protected `/mcp` Resource. wcode keeps:

- Protected Resource Metadata and Authorization Server Metadata;
- Authorization Code + PKCE;
- bounded Dynamic Client Registration compatibility;
- exact redirect handling;
- resource-bound access/refresh tokens;
- refresh-token rotation;
- Origin validation for browser-originated requests.

The tunnel provides reachability, not authorization.

## Media and model capability

`read_media` is metadata-first. It can identify bounded PNG/JPEG/GIF/WebP images, common audio formats, and MP4/WebM metadata without assuming the connected model is multimodal. Image/audio payloads are emitted only when the client explicitly advertises the `run.francis.wcode/media-content` extension for that kind (and optional MIME filter); unknown or unsupported capability fails closed without sending binary content. Video remains metadata-only because MCP has no standard video Tool Result content block.

## Secrets and model context

Credential-like paths are blocked and high-confidence secret text is redacted from model-facing reads and symbol context. Logs and diagnostics must not expose access tokens, refresh tokens, PKCE verifiers, or equivalent credentials.

## Verification and Evidence

Security is also an approval problem after code changes. Risk analysis may raise verification depth; deterministic checks, independent reviewers, stage executors, and HumanApproval evidence remain separate producers. One model verdict cannot erase another producer's failure.

## Safe defaults

- Prefer one repository Workspace.
- Prefer stdio for local agents.
- Keep OAuth for remote connectors.
- Keep SHA preconditions on edits.
- Approve exact command/risky-operation requests instead of enabling broad trust when possible.
- Use `--read-only` or `--no-exec` when a task does not need writes or commands.
