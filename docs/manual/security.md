---
layout: docs
title: Security
description: Workspace, command, authorization, OAuth, and evidence security boundaries
lang: en
alternate: /zh/docs/security/
permalink: /docs/security/
---

# Security Model

wcode is permissive by default. The calling agent owns authorization: wcode
executes the operations that agent requests and refuses only when an operation
is genuinely impossible, such as malformed path syntax or an exhausted resource
bound. Internal policy gates are no longer a second permission layer.

![wcode security and authorization boundary](/assets/wcode-security-boundary.svg)

## Workspace isolation

Only configured Workspace roots exist from the model's point of view. Model-facing
file operations reject absolute paths, parent traversal, `RootDir`/`Prefix`
components, control characters, empty/`.` paths, symlink components, workspace
escape, and unsafe hard-link cases. The actual configured OAuth and engineering
authority-state directories remain excluded from file access and recursive
discovery. File tools cannot create into, replace, move or delete those roots or
their ancestors. These are path-containment and aliasing invariants, not policy
choices. Protected credential and environment paths are not refused; the agent
decides what to read or write.

Any directory can be exposed as a Workspace root, including a home directory or
a filesystem root, and overlapping Workspace roots are accepted. Choose narrow
roots when you want a smaller working set; wcode no longer enforces that as a
permission rule.

Project markers inside a configured root may become derived subspaces. This
does not widen the outer root: relative paths resolve from the selected
Workspace, canonical paths are checked again, and symlink children are
rejected.

## Hash-guarded writes

Existing-file edits use the observed SHA-256 as a precondition. Atomic replacement
prevents partial writes, while stale revisions fail instead of silently overwriting
newer work. Write size bounds and output bounds remain active.

Deletion is deliberately separate: one regular file or empty directory, with an
exact SHA precondition for files. Recursive/root/symlink/hard-link deletion
remains blocked by path invariants. No human approval is required.

## Command execution

`run_command` accepts an executable plus an argument array; it never implicitly
parses one command string as shell syntax. Command policy still exists as an
opt-in hardening module for embedders that construct a restrictive
`WorkspaceSecurity`, but the default policy is permissive and `run_command`
reaches the command directly.

The OS sandbox is opt-in per call. `run_command` executes directly by default,
regardless of the command shape; the calling agent asks for isolation with the
`sandbox: true` argument (or the `run_command_sandboxed` embedder API) on the
specific call that needs it. When no sandbox backend is available, a requested
sandboxed call falls back to direct execution instead of being refused; an
unavailable sandbox is never the reason a command does not run. Environment
scrubbing, cwd containment, process ownership, timeout, cancellation and
output/resource bounds remain active in both lanes.

`--read-only` disables the file-modification tools, `--no-exec` disables command
execution, and `--no-semantic` disables language-server discovery. Each is an
explicit operator choice and is honoured, including against a session-wide
command grant. What is gone is the default policy layer: without one of these
flags wcode does not refuse an operation as a matter of policy, and a read-only
Workspace does not reject classified writing commands.

Two sets of switches are currently advisory rather than effective:

- the hidden `--allow-risky-exec`, `--allow-destructive-writes`,
  `--allow-overlapping-workspaces` and `--allow-broad-workspace` compatibility
  flags are accepted but do not change behaviour;
- a programmatic `WorkspaceSecurity` built with every flag cleared does not
  re-enable command-policy filtering, because the session command grant is
  reported as present by default and `unrestricted_commands` stays true.

The CLI flags above are the supported way to restrict a run.

The development catalog covers representative native and ecosystem tools for all
22 indexed languages, including `gh`, `just`, `task`, `uv`, `ruff`, `biome`,
`deno`, `docker`, `kubectl`, `terraform`, `fd`, `jq`, `cmake`, `ninja`,
`dotnet`, `mvn`, `gradle`, `swift`, `zig`, `pre-commit`, and `act`; each family
still has command-specific policy for embedders that opt into hardening. Rust
verification may use installed `cargo-nextest` and retains `cargo test` as the
compatible fallback. Consequence checks that remain are path/argument
containment and malformed-input rejections, not policy approvals.

Git keeps its bounded lifecycle hardening for repository-controlled hooks,
helpers, credentials and config injection. Command admission does not supply
credentials or guarantee a working remote transport. It also does not constitute
user authorization to publish, a HumanApproval decision, or acceptance for
merge/deployment; those retain their own intent, evidence and integration
boundaries.

GitHub CLI keeps its argument and credential handling. A permitted command shape
does not approve a human decision, publication or deployment.

Repository-aware LSP servers keep their separate execution lane with bounded
environment, workspace path checks and process supervision. These controls
reduce the execution surface but are not an OS sandbox.

## Human authorization is local

Authorization requests, approvals and the TUI/WebUI approval surfaces remain
available and keep their data structures, but they are informational under the
permissive default. `require_human_decision` returns an already-decided receipt
instead of creating a blocking request, and no tool call returns
`AuthorizationRequired` unless an embedding explicitly opts into restrictive
authorization.

Pending requests are still visible in the TUI and protected WebUI. The model can
request access; MCP responses and file-tool writes cannot approve a request.

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

The Workspace-wide current-runtime grant and Full Access remain operator
controls that change reported policy and Workspace write/exec state. Revoking
the session grant restores the base policy. These controls no longer override a
human decision, because human decisions are not blocking by default.

`RiskyExecution` remains the fingerprint-scoped trust mechanism for non-command
execution outside automatic profiles, such as an unprofiled LSP server or
advanced stage executor. Under the permissive default the trust check is
satisfied; an embedding can still construct a restrictive configuration.

WCode's own file primitives remain Workspace-isolated regardless of command grants.

Local Policy activation and revocation reuse exact HumanDecision records and
record separate governance history; see [Local Acceptance Policy](../acceptance-policy/).
Ordinary bounded builds and tests still run repository code with the host user's
access to state. Checksummed local history alone cannot authenticate authority
against arbitrary repository code; a commercial merge gate needs trusted Policy
and execution integration outside the untrusted worker.

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

The protected WebUI **Access** panel exposes the same boundary through an **OAuth sessions** card: it lists bounded metadata (opaque session ID, client ID, resource, access/refresh counts and expiry) and requires an explicit confirmed **Revoke** action. The card never displays tokens or private grant owner IDs. A successful revoke reports the persisted removal counts; a persistence failure keeps the live removal but reports that durable revocation was not confirmed, so a restart may restore the session. The card is operator-only and does not broaden the HTTP authorization above.

A replacement tunnel is accepted only after its public health response matches
the current process. The saved token resource may then migrate to that active
endpoint. OAuth metadata and authorization use the exact Host that received the
request. Historical resources are not active Hosts, and tokens stored for a
different configured Workspace-root set are not loaded.

Host validation and Origin validation are separate checks. A present Origin must be a single valid HTTP(S) origin matching a configured or instance-verified **active** endpoint, including an active non-primary tunnel alias. Missing Origin remains valid for non-browser clients, but malformed, duplicate, credential-bearing or historical-only origins are rejected. Arbitrary client-site origins are not implicitly trusted. Requests still require the original bearer or protected WebUI credential, and legacy SSE sessions remain owner/endpoint-bound.

Request Host resolution is deliberately weaker than Origin validation and the two must not be read as one rule. Resolution first matches a configured or instance-verified active endpoint; a well-formed HTTPS authority outside that set is still accepted so a custom reverse proxy in front of the runtime keeps serving requests. That fallback does not add the authority to the active Origin allow-list, does not make it an OAuth resource, and does not relax the Origin check above: a browser request bearing an Origin must still match an active endpoint. `untrusted_host` therefore reports a missing, malformed or duplicate Host (or an inactive primary), not the mere absence of that Host from the active set.

Verified tunnels enter the trusted endpoint registry before their links are displayed or setup readiness is signalled. A rejected request reports `untrusted_host` or `untrusted_origin` without echoing credentials. For `untrusted_host`, reconnect to a currently registered endpoint and ensure the reverse proxy preserves that public Host; untrusted `Forwarded` / `X-Forwarded-Host` headers cannot register an endpoint. Never disable Origin checking or trust all hosts to repair a connection.

The tunnel provides reachability, not authorization.

## Intelligence HTTP responses

Every `/intelligence` page and `/intelligence/*` response carries protected project
data, so a scoped middleware
(`src/integrations/mcp/cache_policy.rs`) guarantees three headers on each one:

| Header | Value | Why |
| --- | --- | --- |
| `Cache-Control` | `no-store` | Project structure, evidence, authorization requests and command policy must not be retained by a shared or intermediary cache. |
| `X-Content-Type-Options` | `nosniff` | Same protection the setup page already sends. |
| `Referrer-Policy` | `no-referrer` | Workspace roots and plan identifiers must not leak through a referrer. |

Handlers may set these explicitly; the middleware only fills a header that is
absent, so an explicit policy (such as the `public, max-age=3600` lifetime for
static assets) is preserved. Non-intelligence routes (`/healthz`, `/mcp`,
`/sse`) are out of scope.

## Media and model capability

`read_media` is metadata-first. It can identify bounded PNG/JPEG/GIF/WebP images, common audio formats, and MP4/WebM metadata. The caller explicitly opts into binary payloads with `include_content=true`, which returns standard MCP `image` / `audio` Tool Result content blocks; no private capability extension is required. Video remains metadata-only because MCP has no standard video Tool Result content block.

## Secrets and model context

High-confidence secret text is redacted from model-facing reads and symbol
context. Logs and diagnostics must not expose access tokens, refresh tokens,
PKCE verifiers, or equivalent credentials. The OAuth state file contains bearer
credentials; do not copy it into a repository or share it. Credential-like paths
are not blocked; the calling agent is responsible for not exfiltrating them.

## Verification and Evidence

Security is also an approval problem after code changes. Risk analysis may raise verification depth; deterministic checks, independent reviewers, stage executors, and HumanApproval evidence remain separate producers. One model verdict cannot erase another producer's failure.

## Safe defaults

- Prefer one repository Workspace as a working-set choice, not a permission rule.
- Prefer stdio for local agents.
- Keep OAuth for Streamable HTTP and legacy SSE connectors.
- Keep SHA preconditions on edits.
- Treat authorization as the calling agent's responsibility; wcode no longer
  enforces it as a policy gate.
- `--read-only`, `--no-exec`, `--no-semantic` and `--full-access` remain
  accepted for compatibility, but the default path is permissive.
