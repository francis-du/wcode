---
layout: docs
title: OSS and Commercial Boundary
description: Public source-control, license, contract and authority boundaries between wcode OSS and commercial layers
lang: en
alternate: /zh/docs/oss-boundary/
permalink: /docs/oss-boundary/
---

# OSS and Commercial Boundary

wcode is the public Apache-2.0 product. Canonical engineering truth—Design, Verification, Evidence, Policy semantics, Acceptance, workspace safety, MCP/CLI/TUI/WebUI and reusable provider contracts—stays in this repository.

Team and Enterprise implementation lives in a separate source-control repository. A local developer may keep it as the sibling `../wcode-commercial`, but public wcode never requires that checkout to build, test, package, start or verify itself.

## Physical repository boundary

```text
Code/Rust/
├── wcode/                 # public, Apache-2.0
│   ├── crates/core-types/
│   ├── src/
│   ├── tests/
│   └── docs/
└── wcode-commercial/      # separate private-oriented repository
    ├── team/
    ├── enterprise/
    ├── docs/
    └── Cargo.toml
```

The dependency direction is one-way:

```text
Enterprise -> Team -> public wcode API -> wcode-core-types
```

There is no `commercial/` Cargo member, workspace exclusion, or archive rule hiding commercial source inside the public repository anymore. Source-control separation is the boundary. The OSS package still uses an explicit include allowlist, and architecture tests reject local path dependencies that leave the public checkout or packages named `wcode-team` / `wcode-enterprise`.

## What stays OSS

The public repository intentionally remains useful by itself. Commercialization must not create artificial gaps in the local product.

- canonical Design/Requirement/Component state;
- Software Graph, semantic providers and repository intelligence;
- Verification, Evidence, Risk and Reconciliation;
- local Acceptance Policy and native Acceptance records;
- workspace isolation, guarded edits/execution and authorization;
- MCP transports, OAuth, Tasks and agent/plugin integration;
- CLI, TUI and local WebUI/Observatory;
- generic GitHub exact-candidate gate/inbox/credential protocols;
- immutable shared contracts under `wcode-core-types`.

Commercial code may coordinate these capabilities for organizations, deployments and customers, but it cannot replace their truth model.

## What belongs in the commercial repository

The separate repository owns capabilities whose source access, deployment trust or product lifecycle differs from OSS, for example organization/member governance, shared Policy distribution, Team audit/history, Enterprise composition, SSO/OIDC, tenant isolation, hosted control-plane operations, commercial recovery procedures and customer-facing deployment integrations.

A commercial wrapper may add authorization or workflow. It may not turn failed, stale, skipped or incomplete OSS verification into Ready, reconstruct native authority from imported data, or copy canonical models into a second implementation.

## Public contract rule

Commercial code must consume public Rust contracts or released/pinned package interfaces. It must never reach into `wcode/src` with path attributes, `include!`, symlinks, generated source copies or a duplicated store schema merely because both repositories happen to be adjacent locally.

Local development currently uses a sibling path dependency. Commercial CI/release builds must bind an explicit compatible wcode revision or released version. A local `../wcode` path is a development convenience, not a release provenance contract.

The first physical OSS extraction, `wcode-core-types`, owns dependency-light immutable facts such as Git binding, required-check/report DTOs and platform authority/intelligence state-root derivation. Further crate extraction follows the [architecture boundary rules](../architecture-boundaries/): stable ownership, independently testable contract and one-way dependency first; extraction second.

## Packaging and CI

The OSS workflow tests only OSS. It no longer conditionally discovers or builds commercial source. The public source package and standalone verifier must succeed when the commercial repository does not exist.

Commercial CI is independently responsible for selecting an exact wcode revision, building Team/Enterprise against public contracts, running equivalence/security/operational tests, proving no commercial layer upgrades canonical verification truth, and recording the wcode revision used for each commercial release.

The two repositories may be tested together locally, but a green commercial build is not evidence that an OSS verification result passed, and vice versa.

## License boundary

Existing wcode source remains Apache-2.0. Moving commercial files out of the public repository does not relicense any OSS source. The separate commercial repository owns its own LICENSE/NOTICE and final terms require the appropriate legal review.

A public repository must not contain customer confidential data, proprietary commercial implementation, production credentials or private deployment state merely because package tooling would exclude those files.

## State and authority boundary

Source separation and runtime isolation are different problems. A sibling directory does not prove tenant isolation. `WCODE_STATE_DIR`, verification/evidence stores, publisher credentials and Team governance storage still require protected deployment identities and appropriate filesystem/process boundaries.

Canonical Acceptance must continue to come from current revision-bound OSS state. Commercial history, CI machine reports, exceptions and organization roles remain separate facts unless a public authority contract explicitly says otherwise.

## Completion criteria

The boundary is healthy only when public wcode builds/tests/packages with no commercial checkout, OSS Cargo metadata has no commercial dependency path, OSS source includes cannot escape the repository, commercial consumers use explicit public APIs, commercial tests bind an exact wcode revision, and release/docs/CI no longer imply that private source is present inside the OSS repository.

Historical in-repository `commercial/` references are migration history only, not the current architecture.
