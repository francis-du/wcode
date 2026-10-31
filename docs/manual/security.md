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

Only configured Workspace roots exist from the model's point of view. Model-facing file operations reject absolute paths, parent traversal, protected paths, symlink components, workspace escape, and unsafe hard-link cases. The actual configured OAuth and engineering authority-state directories are excluded from file access and recursive discovery, including a Full Access Home Workspace. File tools cannot create into, replace, move or delete those roots or their ancestors. This protects file-tool entry points; bounded direct builds, scripts and LSP processes still run with the host user's permissions. The strong OS sandbox applies to the broad command lane described below. It is not tenant isolation or protection against an attacker controlling that OS account.

Use repository roots, not a home directory or filesystem root.

Project markers inside a configured root may become derived subspaces. This
does not widen the outer root: relative paths resolve from the selected
Workspace, canonical paths are checked again, and symlink children are
rejected. Manually overlapping configured roots remain blocked by default.

## Hash-guarded writes

Existing-file edits use the observed SHA-256 as a precondition. Atomic replacement prevents partial writes, while stale revisions fail instead of silently overwriting newer work.

Deletion is deliberately separate: one regular file or empty directory requires an exact one-shot local authorization; recursive/root/protected/symlink/hard-link deletion remains blocked.

## Permissive command execution with a strong sandbox

`run_command` accepts an executable plus an argument array; it never implicitly parses one command string as shell syntax. With normal execution enabled, otherwise-valid commands, including unknown tools and explicit shell invocation, need no second `CommandAccess` or command-scoped `RiskyExecution` approval. An explicitly revoked executable still requires reauthorization, and `--no-exec` disables ordinary command execution. Normal command policy continues to check executable names, arguments, consequences and guarded Workspace cwd; a command grant does not widen file-tool access.

Known bounded development shapes use the hardened direct lane. Command shapes outside bounded profiles, including shell script execution such as `sh -c ...`, require the strong OS sandbox. A bounded syntax-only check such as `sh -n script.sh` can remain direct; the same constrained profile exists for `bash`, `dash` and `zsh`. macOS uses Seatbelt and Linux uses bubblewrap. Windows has no supported backend, and every unavailable or unenforceable backend rejects broad execution with `sandbox_unavailable`; there is no unsandboxed fallback. Broad execution denies external network access, keeps the host filesystem read-only outside the selected Workspace and private scratch, and masks or denies protected credential/repository-control paths and configured authority-state roots. Environment scrubbing, cwd, process ownership, timeout, cancellation and output/resource bounds remain active.

Without an explicit all-command override, read-only Workspace policy rejects classified writing commands and gives broad commands read-only Workspace access. Bounded direct repository code is still not OS-isolated, so an inspection-only session should combine `--read-only --no-exec --no-semantic`. The session override and its effect on Workspace writes are described below.

The default development catalog covers representative native and ecosystem tools for all 22 indexed languages, and each family still has command-specific policy rather than blanket process trust. The bounded operator/development catalog includes `gh`, `just`, `task`, `uv`, `ruff`, `biome`, `deno`, `docker`, `kubectl`, `terraform`, `fd`, `jq`, `cmake`, `ninja`, `dotnet`, `mvn`, `gradle`, `swift`, `zig`, `pre-commit`, and `act`; language-native compilers, formatters, linters, type checkers and test runners are governed separately by exact policy. Rust verification may use installed cargo-nextest and retains `cargo test` as the compatible fallback. Bounded local tests, checks, builds, format/lint, workspace scripts, code generation, dependency maintenance, Docker development workflows, and repository-declared verification executors run autonomously through the hardened Workspace lane. Source-writing modes are tracked against read-only Workspace policy. Under normal command policy, consequence-specific denials still apply to credential flows, compiler/plugin/agent injection, Workspace escapes, protected paths, host-wide tool/runtime mutation, package publication/ownership, remote administrative mutation, and destructive infrastructure shapes.

Git has a bounded lifecycle policy. With ordinary execution enabled and no explicit revocation, supported forms run without a second command approval: explicit-pathspec or `-A` staging, message-only commits, non-force push to the current upstream or a configured remote plus explicit branch, explicit-branch fetch, `pull --ff-only`, bounded branch/switch/tag operations, and `restore --staged` for explicit paths. Shape and consequence checks still block force/delete/mirror pushes, reset/clean, worktree-content restore, editor/hook/config injection and unsupported forms. Bounded direct Git calls disable repository-controlled hooks and helpers. Only bounded Git push may use the current SSH Agent through a fixed non-interactive SSH command; token-like environment variables, credential helpers, AskPass, arbitrary config, proxy helpers and HTTP extra headers remain stripped. Command admission does not supply credentials or guarantee a working remote transport. It also does not constitute user authorization to publish, a HumanApproval decision, or acceptance for merge/deployment; those retain their own intent, evidence and integration boundaries.

GitHub CLI has its own bounded policy. Read-only PR/issue/run/workflow/release/repository/search views and `gh api` GET requests confined to `repos/{owner}/{repo}` can run directly. Supported explicit non-interactive PR/issue creation, comments, workflow dispatch, release creation against an existing verified tag, PR merge with one explicit merge method, and run rerun/cancel also need no second command approval under ordinary execution. Release asset upload paths, `gh auth`, arbitrary API requests, secrets/variables, extensions, host/repository redirection and admin/auto merge modes remain blocked by normal policy. Preconfigured `GH_TOKEN`, `GITHUB_TOKEN` and `GH_ENTERPRISE_TOKEN` are explicit provider credential allowances; repository/host/config redirects and execution-injection variables are still scrubbed. A permitted command shape does not approve a human decision, publication or deployment.

Repository-aware LSP servers can load repository-controlled configuration or code, so wcode keeps a separate hardened LSP lane: only built-in servers with an explicit automatic safety profile may run there by default. `rust-analyzer` must resolve outside the Workspace and also pass a bounded executable-viability probe; a rustup shim whose component is absent is treated as unavailable instead of triggering repeated `semantic_auto` failures. Its environment is scrubbed for credential and execution-injection variables, and wcode disables rust-analyzer build scripts, proc macros, automatic Cargo reload, and check-on-save. The warm session pool is capacity-bounded, keyed by Workspace + server binary identity, serializes one LSP protocol stream per slot, evicts idle/old slots, closes documents that leave the bounded index set, and rebuilds a slot when the server exits or its binary changes. Navigation results are still filtered through the Workspace boundary. `--no-semantic` disables this lane completely. LSP servers without an automatic safety profile still require the applicable explicit runtime trust; permissive ordinary `run_command` does not grant that trust. Repository-declared Property/Mutation/Fuzz/Runtime executors instead use a hardened autonomous local lane with no shell, bounded cwd/process/output/time, sensitive-environment scrubbing and Workspace path checks. These controls reduce the execution surface but are not an OS sandbox.

## Human authorization is local

Pending authorization requests are visible in the TUI and protected WebUI. The model can request access; MCP responses and file-tool writes cannot approve its HumanDecision request.

A HumanApproval uses a separate exact local-operator grant with a two-minute expiry and one-time consumption. It binds the server instance, Workspace, requesting MCP owner, complete plan digest, code and Design revision, policy and decision statement. Caller-supplied `confirmed`, `approver` or producer names, generic MCP form elicitation, command session grants and Full Access cannot authorize this decision. An approval is recorded as a human decision, never as an executed or passed test. This local operator boundary does not provide Team roles or isolate a malicious process running as the same OS user.

![wcode authorization and access controls](/assets/wcode-access-management.png)

Demonstration of the current production interface with fixture requests; no access was granted.

TUI flow:

```text
↑ / ↓  select request
A      authorize all commands for this Workspace session
Y      approve this exact request
N      deny
```

The command view also uses **F** to toggle the Workspace-wide session grant. The protected WebUI exposes the same switch, and stdio elicitation offers `exact`, `all_commands`, or `deny` when the client supports forms.

Ordinary command execution needs no separate human command grant when execution is enabled, except for explicitly revoked executable access. The Workspace-wide current-runtime grant and Full Access select `session_override`; it takes precedence over `--no-exec`, explicit command revocation, read-only command-write classification and normal command-policy filtering. Broad commands still require the strong sandbox, with Workspace writes enabled inside that boundary. Revoking the session grant restores the base execution/write policy and any explicit command revocations. Without an override, the canonical policy is `disabled` when exec is off and `permissive_sandboxed` when exec is on. These states do not change protected authority-state roots, guarded cwd, process/resource bounds, environment scrubbing or WCode's file-tool/delete controls.

`RiskyExecution` remains the fingerprint-scoped trust mechanism for non-command execution outside automatic profiles, such as an unprofiled LSP server or advanced stage executor. Ordinary `run_command` does not need it and does not itself satisfy those independent trust checks. An explicit runtime risk grant, Workspace session override or Full Access can satisfy the existing runtime trust mechanism where supported; none can authorize HumanApproval. For non-automatic LSP servers, exact mode binds Workspace + server + current binary identity. Hardened automatic LSP and repository-declared verification profiles retain their own executable, argument and environment restrictions.

WCode's own file primitives remain Workspace-isolated regardless of command grants.

Local Policy activation and revocation reuse exact HumanDecision and record separate governance history; see [Local Acceptance Policy](../acceptance-policy/). Broad command sandboxes mask actual configured authority-state roots rather than directory names: macOS denies access and Linux refuses unavailable masks. Ordinary bounded builds and tests still run repository code with the host user's access to state. Checksummed local history alone cannot authenticate authority against arbitrary repository code; a commercial merge gate needs trusted Policy and execution integration outside the untrusted worker.

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

Client registrations remain persistent without a clock TTL. Access tokens expire after one hour; token responses return `expires_in: 3600`, `Cache-Control: no-store` and `Pragma: no-cache`. Refresh tokens expire after 30 days without a successful rotation; each refresh issues a new refresh token, restarts that idle TTL and immediately removes old access tokens for the same client and authorization grant. Other grant owners remain valid. Expired, zero or future `issued_at_ms` values fail closed. Existing persisted grants retain their original issuance time on migration and restart; loading them never renews their lifetime.

State is persisted per configured Workspace-root set in the user's wcode state directory and loaded after a process restart. Writes are atomic; Unix files are restricted to mode `0600`, symlink state files are rejected, malformed state fails closed, and authorization codes remain short-lived, one-shot and memory-only. Stores retain fixed entry limits: unbound client registrations may be reclaimed at client capacity, and token capacity evicts the oldest token instead of growing without bound.

The implemented local-operator session administration APIs are `GET /oauth/sessions` and `POST /oauth/sessions/revoke`, with a `session_id` JSON field for revocation. They enforce the existing Host and Origin rules and require the current server's `X-Wcode-UI-Token`; an ordinary MCP bearer does not authorize them. Listings contain bounded session metadata and an opaque session ID, never access/refresh tokens or private grant owner IDs. Successful revocation persists removal of that grant's access and refresh tokens, so restart does not restore them. A write failure returns `revocation_not_persisted` and removes live credentials without confirming durable revocation. These are operator administration APIs, not an RFC 7009 endpoint, Team ACL or SSO; Team ACL and SSO are not implemented.

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
- Use exact grants for explicitly revoked executables or non-automatic providers when broader runtime trust is unnecessary.
- Use `--read-only` or `--no-exec` when a task does not need writes or commands.
- Use `--no-semantic` when the runtime should not start any first-party LSP server.
