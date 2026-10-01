---
layout: docs
title: Failure Memory
description: Project-owned native failure observations shared across coding models
lang: en
alternate: /zh/docs/failure-memory/
permalink: /docs/failure-memory/
---

# Failure memory across coding models

Verified co-change history improves code retrieval. It deliberately excludes failed changes; it does not teach a model why a guarded write or verification failed. The new source implementation closes this gap by recording bounded native failure categories in the existing local engineering journal and recalling fixed corrective advice in `agent_context`, including scoped worker handoffs.

The categories cover stale SHA, missing native approval, protected paths, source limits, failed verification, timeouts, stale revision and incomplete discovery. They are observed by native tool handling, not supplied as arbitrary model instructions. Raw error prose, command output, source bodies, prompts and reasoning are not stored as lessons. Unrecognized diagnostics remain unclassified; this layer does not infer a fix for every compiler or application bug.

Each selected rule reports its observation count and whether it has recurred. Recall uses task keywords, currently readable affected paths and the IDs of native checks discovered in the current Project Profile. This also recalls relevant failures without path metadata after a new Harness or session; a generic query need not name the failed check. Check IDs select historical advice only and do not prove recovery or current verification. Recall returns with at most two rules, or one at a 1,000-token budget. Missing paths are omitted. The same project's bounded journal survives a fresh Harness or Workspace; another project gets its own history. It retains up to 512 milestones, reserving the latest two distinct observations per native failure category so successful context traffic cannot erase those lessons. Counts describe retained history, not lifetime totals; incomplete history remains explicit. Canonical record names, payload identities and event deduplication prevent copied observations from increasing recurrence.

Workspace state keeps the existing key for UTF-8 roots and uses platform-specific lossless encoding for other roots. It never falls back to a shared lossy key; old non-UTF-8 state is not automatically imported because its ownership can be ambiguous. Journal appends and reads are serialized within one runtime process, with bounded recovery of prior overflow; this is not a cross-process transaction guarantee. Read failures remain visible, and records more than five minutes ahead of the current clock are excluded from recall with partial coverage.

These are historical advisories. Existing SHA guards, protected paths, native approvals and deterministic checks remain the executable constraints. An observation does not prove the current revision fails, and a later successful task does not prove the old cause has been repaired. Repository-local `.wcode/failure-rules.yaml` literals are matched against native MCP `payload.error` through the production journal callback, with a 4,096-byte UTF-8 bound before copying. Arguments, stdout and stderr are not matching inputs; raw errors are never persisted. Duplicate milestone IDs do not inflate recurrence. These repository rules are untrusted advisory data, not operator-approved Policy. Model-generated fixes and causal repair claims still require independent proof; same-check recovery associations are not proof of the cause. This implementation does not guarantee that every model follows advice.

The same work also isolates volatile worker-handoff telemetry in MCP `_meta`, preserving source, SHA, revision, lease and permission fields in the model payload, and validates effective permissions before reusing concurrent Project Profile results. Internal cache hits, delivered bytes, estimated tokens and provider-reported cached tokens are distinct measurements. No provider billing reduction is claimed.

Run the focused `failure_memory_`, `context_lessons_`, journal, `convention_cache::` and worker-handoff tests, then native `review_changes` and `verify_project`. Source test results do not prove that a previously running server process has loaded the new behavior.
