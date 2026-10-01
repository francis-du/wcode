---
layout: docs
title: Local Acceptance Policy
description: Native Policy capture, operator approval and bounded local authority history
lang: en
alternate: /zh/docs/acceptance-policy/
permalink: /docs/acceptance-policy/
---

# Local Acceptance Policy

This OSS layer separates a repository-editable Policy draft from an operator-approved local baseline. It supports native preview, status, activation and revocation. The working tree now adds Policy consumption, native Change Acceptance Records and a basic GitHub adapter; integration and failure-path validation are in progress. This does not mean a release, installed required check or completed paid pilot. See [Change Acceptance](../change-acceptance/).

## Draft and native capture

Define the optional `acceptance_policy` in `.wcode/project.yaml`. Schema version 1 includes a stable Policy ID, positive project-owned version, default requirements, optional docs-only requirements and scoped rules. Requirements name discovered checks; arbitrary commands cannot be embedded in Policy.

For example, a Node project whose native discovery provides `node-test` may begin with:

```yaml
schema_version: 1
name: example
acceptance_policy:
  schema_version: 1
  id: project-baseline
  version: 1
  default:
    minimum_level: full
    checks: [node-test]
    human_approval: true
```

Use the existing agent and its MCP connection:

```json
{"action":"preview"}
```

The `acceptance_policy` tool captures complete code/Design revision, native command bindings and SHA256 hashes of the same configuration bytes used for discovery. Missing expected files are explicit; binary lockfiles are hashed as bytes. Component and requirement scopes freeze declared ordinary files; symbol references conservatively use file scope. This is declared ownership, not proof of semantic coverage.

At least one explicit discovered check and one present configuration source are required. Unknown checks, incomplete discovery, invalid Design, non-UTF8 root identity, unsafe file mappings and snapshot bounds fail closed.

Changing a package script or Makefile recipe with unchanged command argv changes the definition seal. Ordinary README edits are excluded from that seal. It covers captured native configuration inputs; it does not recursively authenticate every executable, dependency, build script or test body.

## Exact operator approval

Preview returns a `snapshot_digest` through the tool response. Status returns the current `generation`; no record means generation 0. A damaged authority head is an error, never generation 0.

Request activation with the exact digest and generation:

```json
{"action":"activate","expected_generation":0,"snapshot_digest":"sha256:<preview digest>"}
```

Optional `expires_at_ms` is an absolute Unix timestamp in milliseconds, at most 365 days ahead. The server creates a two-minute, one-use HumanDecision request bound to instance, requesting MCP owner, Workspace/root, native revision, action, snapshot digest, expected generation and expiry. Inspect and approve that exact request in the existing TUI authorization panel or protected WebUI Access panel. Retry the same request from the same MCP owner.

Generic MCP elicitation, an OAuth bearer, Full Access, command session grants, caller labels and submitted JSON cannot establish this approval. The server recaptures native inputs after grant consumption and compares the complete digest before committing. Input changes or a generation race consume the grant without activating stale inputs.

Policy approval creates a governance record; it does not create passed verification or HumanApproval Evidence. Operator receipts identify a instance-scoped local request and decision time, not an authenticated Team member.

## Status and revocation

`{"action":"status"}` returns one of:

| Status | Meaning |
| --- | --- |
| inactive | No local authority record exists. |
| active | Current authority is unexpired and its captured native definitions still match. |
| stale_definition | Configuration discovery failed or required definitions changed; historical authority is retained. |
| expired | Activation lifetime has ended. |
| revoked | Latest committed generation is a revocation. |

Here `active` describes Policy authority, not readiness to merge. The historical activation revision remains provenance; legitimate future code changes do not require activation revision equality. A candidate-edited draft cannot replace the approved baseline.

Revoke with `{"action":"revoke","expected_generation":1}`. Revocation needs its own exact local operator approval and revision guard. A missing or invalid repository draft does not prevent revocation of an intact authority record. Rollback/renewal are not separate product commands in this slice.

## Persistence and security limits

Records live in the existing protected local engineering state root under `acceptance-policy`, never in repository-editable Design files. A deterministic generation filename uses exclusive creation for cross-process CAS. Files and bounded commit markers are synchronized before success. Uncommitted or corrupt highest generations block reads and mutations; they never fall back to old active state. History is limited to 256 records and stops explicitly when full; it does not silently prune revocation records. Recovery of a damaged head requires a future explicit operator recovery workflow.

SHA256 checksums and previous-digest links detect corruption and accidental modification; they are not signatures or legal tamper-proof audit. Unix file and directory synchronization is used; Windows file synchronization has a different parent-directory durability boundary.

File tools protect actual configured authority roots. Broad command sandbox profiles also mask these roots: macOS denies reads/writes, Linux refuses unavailable masks, and unsupported backends fail closed. Ordinary bounded Cargo/npm builds and tests still execute repository-controlled code under the host user. A malicious repository process with that user's state access can forge checksummed files; this local history is not an authenticated acceptance authority against arbitrary repository code.

A trusted merge gate must obtain Policy from outside the untrusted change worker and validate commit-bound execution through an independently protected integration. See [Change Acceptance](../change-acceptance/) for native implementation progress; protected deployment and a real PR pilot still need acceptance. See [Security](../security/) and [OSS / Commercial boundaries](../oss-boundary/).
