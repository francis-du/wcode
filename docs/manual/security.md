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

A small safe command set can be pre-authorized. Other model-requested bare executable names become per-Workspace pending authorization requests that the operator may approve or deny. Git mutation remains narrow: only explicit `git add` pathspecs, `git commit -m ...`, and `git push <remote> <refspec>` shapes can enter exact `RiskyExecution` authorization; force/delete/mirror/reset/restore-style mutations remain blocked.

Exact Git mutation approval does not grant credential access. The command runner still removes token-like environment variables, `SSH_AUTH_SOCK`, `GIT_ASKPASS`, global Git config, credential helpers, AskPass/SSH/proxy helper commands, and configured HTTP extra headers. As a result, an authenticated remote push can still fail even after the operation itself is approved; credential mediation is intentionally a separate future trust boundary rather than an implicit side effect of `git push` authorization.

Repository-aware language servers and verification executors are a broader trust boundary because they can load repository-controlled configuration or code. These exact operations require explicit authorization unless the process was started with the broader `--allow-risky-exec` pre-authorization.

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
