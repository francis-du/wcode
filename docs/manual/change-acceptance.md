---
layout: docs
title: Change Acceptance
description: Revision-bound native verification and evidence for change acceptance
lang: en
alternate: /zh/docs/change-acceptance/
permalink: /docs/change-acceptance/
---

# Change Acceptance

Keep your existing coding agent. wcode captures a Change Acceptance Record from current Git, approved local Policy, persisted Verification and Evidence. The working-tree implementation is undergoing integration and failure-path validation; this is not a release or a deployed GitHub gate.

## Local flow

Approve a baseline using the [local Policy operator flow](../acceptance-policy/). The MCP tool is `change_acceptance`: inspect, plan, verify, record or history. Its default candidate is HEAD to worktree; for a PR provide complete `base_revision` and `target_revision` SHAs. The tool cannot import proof or self-approve a human requirement.

Plan freezes candidate requirements and native command bindings. Verify executes that candidate's required matrix and native stages. Missing executors, independent review and human requirements remain visible blockers. Record appends bounded history and recaptures inputs. History is historical_only and cannot approve a current revision.

```text
wcode acceptance inspect --base <fullSHA> --head <fullSHA> --json
wcode acceptance verify --base <fullSHA> --head <fullSHA> --json
wcode acceptance inspect --base <fullSHA> --head <fullSHA> --check
wcode acceptance history --json
```

Only a current native ready Record passes `--check`; history has no such flag. Commands do not automatically activate Policy or approve human requests.

## Engineering truth

The Record retains Code/Design revision, Git base/head/tree/index/dirty identity, Policy generation/source seals, Plan, risk, independent check facts, Evidence metadata, structured reasons and actions. Capture timestamps are retained but excluded from the stable content identity.

Required, discovered and mapped do not mean executed. Unknown, unavailable, timed-out and skipped checks do not become passed. Only native receipts matching complete Revision/Git/Policy/signature/verification level prove current checks. Identical files on a new commit, Policy reactivation/revocation/expiry or definition changes invalidate old proof. Model reviews and human decisions cannot erase deterministic failure.

States are ready, blocked, needs_review, incomplete and stale. Missing data, capture errors or cached snapshots cannot infer readiness from counts. TUI and Observatory consume the same native Record.

## Installed publisher operations

The installed binary now provides `github preflight` and `github publish`; compiling `examples/git_publisher.rs` in a candidate checkout is not the operational path. Keep the reviewed binary, publisher identity manifest and authoritative state outside the untrusted worker's writable environment. A disjoint directory is checked by the CLI, but is not an OS sandbox or tenant-isolation guarantee.

```text
wcode github preflight --config-root /trusted/publisher-config --pull 17 --check --json
wcode -w /trusted/candidate github publish \
  --config-root /trusted/publisher-config --pull 17 \
  --workspace-id project-id --base <fullSHA> --head <fullSHA> --json
```

The config root contains an operator-reviewed `.wcode/github-publisher.yaml` with the same credential-free schema as enrollment below. The candidate root must be disjoint from that config root. The publisher reads `WCODE_GITHUB_PUBLISHER_TOKEN` from its trusted launch environment; there is no token flag, arbitrary API URL flag or proof JSON input. Do not inject this credential into a build, test, PR checkout script or `cargo run` from untrusted source. Static-token mode uses a short-lived installation credential delivered by the deployment's secret manager and does not renew it. Dedicated trusted deployments may explicitly enable the App-key mode below; the two sources cannot be mixed.

Preflight inspects repository metadata with bounded GET requests; optional App-key authentication can separately issue an installation token. It checks current PR repository/base/head and either strict classic protection tied to the expected App with administrator enforcement, or effective active rulesets with matching identity, strict expected-App checks and visible empty bypass actors. Effective rules are paged completely within a fixed bound; missing permissions, unavailable/malformed responses, excessive pagination or changing candidates remain incomplete. GitHub can omit bypass actors when the reader lacks ruleset-write access: absence is not proof of an empty bypass list. Ask the ruleset operator to review access; do not broaden the publisher's administration permissions just to obtain a green diagnostic. Merge queues are explicitly rejected because this publisher does not validate merge-group commits. See the [rules API](https://docs.github.com/en/rest/repos/rules) and [classic protection API](https://docs.github.com/en/rest/branches/branch-protection).

`configuration_verified` is an observation of check configuration, not current Acceptance, credential Check-write permission or deployment isolation; separate fields remain false. `preflight --check` exits nonzero when that observation is incomplete. Publish always repeats preflight, requires explicit event base/head SHAs, rejects stale events instead of retargeting, starts the Check as failure, and only changes it to success after fresh native Acceptance and exact acknowledgement guards. It does not run missing verification, activate Policy, approve reviews, change remote rules or merge. `--read-only` and `--no-exec` reject publication before credentials or network access.

A trusted controller may invoke the command for a verified PR event and again after native verification or Policy changes. It must independently authenticate event delivery and choose the enrolled repository; client fields are not an authorization mechanism. On restart or retry, run fresh preflight/publication rather than replaying an old receipt. An outage is not permission to merge, and the deployment must handle invalidating an already-published success when Policy or evidence changes. The current sequential Check protocol cannot promise immediate revocation during a GitHub outage. Local HTTP fixture tests exercise protocol behavior with actual native verification, but are not a customer deployment or a remote required-check pilot.

## Optional installation-token renewal

A trusted publisher can opt in to installation-token renewal without restarting its watcher or inbox worker. On Unix, the preferred file mode keeps a dedicated GitHub App signing key in a private directory outside candidate and inbox paths, owned by the publisher user. Both the file and immediate directory must deny group/other access; symbolic-link ancestors, hard links, relative paths, oversized files and encrypted/unsupported keys are rejected. Cross-platform service launchers may instead inject the PEM directly through `WCODE_GITHUB_APP_PRIVATE_KEY`; wcode validates the bounded bytes in memory and never persists them. Unencrypted RSA PKCS#1 and PKCS#8 PEM are supported. Exactly one App-key source is accepted.

```sh
unset WCODE_GITHUB_PUBLISHER_TOKEN
export WCODE_GITHUB_APP_PRIVATE_KEY_FILE=/srv/wcode/private/app-signing-key.pem # Unix
# Or inject PEM contents as WCODE_GITHUB_APP_PRIVATE_KEY on a trusted service host.
export WCODE_GITHUB_INSTALLATION_ID=789
# Invoke the existing preflight, publish, watch-candidate or work-inbox command.
```

The installation ID is trusted launch configuration, not a webhook-selected identity. Event/inbox commands can use their explicit installation ID when the environment value is absent; conflicting values are rejected. Supplying both an App-key setting and a static token fails rather than silently selecting a credential. Existing publication permission/root checks and raw-event authentication precede key access. Verify-event, intake, status and archival do not open this key.

Renewal signs an RS256 App JWT, checks the repository's current installation/App identity and suspension state, and requests a token for exactly the enrolled numeric repository ID. Requested permissions are Checks write plus Pull requests, Contents, Administration and Metadata read; extra/missing permissions, other repositories or invalid expiry in the response are rejected. No permissions are escalated to hide missing ruleset visibility. The endpoint is fixed to GitHub HTTPS, redirects are disabled, and tokens remain opaque bounded strings rather than assumed 40-character values. See [installation access tokens](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-an-installation-access-token-for-a-github-app) and [App JWT claims](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-json-web-token-jwt-for-a-github-app).

Each provider shares one renewal among concurrent callers. Requests renew inside a 120-second expiry margin, using both wall-clock and monotonic limits; clock rollback cannot extend the cache. Exchange/wait is bounded to 35 seconds, and failed exchanges back off from 5 to at most 300 seconds without reusing an expired or near-expiry credential. Signing runs off the async executor and retains a separate capacity slot until the real blocking work ends, even after its waiting request is cancelled or times out. File mode rereads the PEM at renewal, allowing an operator-provisioned replacement without changing the candidate or Check lock; in-memory mode intentionally keeps only the launch-time PEM for that process lifetime. A cached token can remain usable until renewal even if its signing key file is subsequently removed; removing a file is not remote token revocation.

A 401 invalidates only the credential actually rejected. A late response for an older token cannot discard its replacement. No failed request is automatically replayed, particularly a Check mutation with an ambiguous remote effect; the existing guarded watcher/queue decides subsequent work. Renewal failure is unavailability, never fresh native proof or confirmed Check revocation. App-key preflight reports credential_renewal_enabled=true and conservative remote_mutations=true because it may issue a credential, while repository_mutations stays false.

An App private key is more powerful than a repository-scoped token. Use a dedicated App and protect the signer host from candidate code, intake processes and other tenants. Downscoping the issued token does not limit what a stolen App key could sign. Guarded child commands strip both canonical App-private-key variables and the legacy `WCODE_GITHUB_APP_KEY_FILE` compatibility variable. This mode does not create a remote App, grant permissions, configure TLS, establish OS/tenant isolation, rotate keys on GitHub or claim memory zeroization. Non-Unix private-file mode remains fail-closed until native ACL validation exists; cross-platform in-memory PEM mode and externally managed static tokens remain available. `WCODE_GITHUB_APP_KEY_FILE` stays accepted for compatibility on Unix but is deprecated in favor of `WCODE_GITHUB_APP_PRIVATE_KEY_FILE`. Local RSA/HTTP and CLI tests are not a live installation or customer-deployment attestation.

## Continuous fixed-candidate reconciliation

`github watch-candidate` fills the post-publication recheck path without replaying completed deliveries:

```text
wcode -w /trusted/candidate github watch-candidate \
  --config-root /trusted/publisher-config --pull 17 \
  --workspace-id project-id --base <fullSHA> --head <fullSHA> --json
```

It runs in the foreground with the same write/exec and disjoint-config guards as `publish`, using the separately supplied installation credential. Every round repeats live preflight and native Policy/Verification/Evidence capture. It creates one red Check for this process, then reuses that exact Check ID. Unchanged native facts are recaptured and compared with the live Check without rewriting it; changed facts pass through the same red-first and acknowledgement guards. The controller does not execute missing tests or approvals: new genuine verification can turn a blocked candidate ready, while Policy revocation, changed proof or incomplete metadata removes readiness. Restoring Policy alone does not validate old-generation checks.

Polls wait 30 seconds after completed work; consecutive unavailable rounds wait 60, 120 and then at most 240 seconds, with no overlapping requests or catch-up burst. These are scheduling delays, not maximum revocation latency. JSON Lines report fresh observations or explicit unavailability, never a cached success as current proof. If the PR head/base or protection changes, the controller tries to deny only its own original Check and does not follow the new head. Check name, App and SHA are revalidated before updating an owned ID. The [Checks API](https://docs.github.com/en/rest/checks/runs#update-a-check-run) provides the underlying update operation; it is not a cross-system merge transaction.

Unix SIGINT/SIGTERM handlers are registered before startup; shutdown stops new rounds, finishes the current operation and denies the owned Check, checking the acknowledgement again before reporting a successful stop. Reporting failure also attempts denial. Network failure, invalid credentials or foreign Check metadata yields explicit unconfirmed denial; a previous remote success may still be visible. Force-kill cannot perform graceful cleanup. Configure one supervised publisher per binding; do not run competing inbox workers, one-shot publishers and watchers for the same candidate. A restart creates a new guarded Check rather than importing old IDs. The trusted supervisor still owns new-candidate selection, signing-key custody or external static-token renewal, configuration changes and crash recovery. No daemon, TLS service, branch rule or tenant boundary is installed by this command, and real remote deployment remains a separate gate.

### Same-root publisher coordination

Cooperative publisher processes sharing the protected native state root now use nonblocking OS admission for the same API, numeric repository ID, expected App, Check name and head SHA. Configure a common `WCODE_STATE_DIR`; different local checkout labels, configuration paths, credentials, PR numbers or base revisions do not create separate slots for a shared Check/head. A watcher retains admission between rounds and through shutdown denial; one-shot publication retains it for its actual work. Every Check mutation, including the lower-level provider interface, requires a matching opaque guard. Busy callers make no Check writes, while read-only preflight, inbox status and intake remain available.

An inbox worker acquires candidate admission before consuming an attempt. A busy candidate keeps its state, attempts and backoff, and the bounded due scan can process another authenticated candidate. If none can be admitted, the worker reports unavailable instead of claiming idle or exhausting retries. Admission stays held through the final durable state write. This does not change interrupted-attempt recovery or replay tombstones.

Stable empty files under `github-publication-locks` are explicitly unlocked, never truncated, deleted or subject to time-based takeover. They contain no tokens, proof or Check IDs. Do not remove a live lock file to unblock work: that can create two independent lock inodes. Unix permissions, aliases, hard links and named/held inode identity are checked; Windows ACL deployment remains separate. Process death only releases local admission, not remote requests already accepted before the crash. Separate state roots/hosts, older binaries and noncooperative writers still require external coordination, and no atomic merge or rollback guarantee is claimed. See [Rust file-lock semantics](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock).

## Authenticate PR delivery before publication

The working-tree binary also provides `github verify-event` and `github publish-event`. A trusted receiver delivers the unchanged JSON request body on stdin. The library verifies `X-Hub-Signature-256` using constant-time HMAC-SHA256 verification, before interpreting PR identity. Configure a separate high-entropy webhook key of 16–4,096 bytes, not the publisher API token. Payloads over one MiB, ambiguous/missing headers, incorrect signatures, wrong installation/repository identities, closed/draft/merged PRs, unsupported actions and incomplete commit SHAs are rejected. Supported actions are opened, reopened, synchronize, ready_for_review and edited. A fork head is allowed only with a pinned enrolled base repository and complete candidate identities.

```text
wcode github verify-event --config-root /trusted/publisher-config \
  --installation-id 789 --json < /trusted/inbox/raw-delivery.json
wcode -w /trusted/candidate github publish-event \
  --config-root /trusted/publisher-config --installation-id 789 \
  --workspace-id project-id --json < /trusted/inbox/raw-delivery.json
```

The trusted receiver supplies `WCODE_GITHUB_WEBHOOK_SECRET` and the original headers through `WCODE_GITHUB_SIGNATURE_256`, `WCODE_GITHUB_EVENT`, `WCODE_GITHUB_DELIVERY` and `WCODE_GITHUB_CONTENT_TYPE`. These are launch-environment inputs, not CLI secret flags. It must reject duplicate headers before flattening them into environment values, close the body pipe, and enforce a wall-clock/input deadline. Configuration root, installation ID and candidate path come from trusted routing configuration, never fields chosen by the event. Preserve the original UTF-8 bytes; parsing and reserializing JSON before signature verification invalidates the signature. See [GitHub signature validation](https://docs.github.com/en/webhooks/using-webhooks/validating-webhook-deliveries) and [delivery best practices](https://docs.github.com/en/webhooks/using-webhooks/best-practices-for-using-webhooks).

Verify-event performs no network requests or state writes. It returns bounded authenticated routing metadata and a SHA-256 body digest, not the raw request or sender-controlled text. A valid shared-secret signature does not prove freshness, human approval, native verification or replay protection; those output fields remain false. The delivery-ID header is not signed with the payload. Replacing it therefore cannot change the body digest or create new engineering authority. The raw stdin commands do not provide durable replay state or transport deadlines themselves. The inbox commands below add durable intake and bounded loopback HTTP reception; TLS and trusted connection/header limits remain deployment responsibilities.

Publish-event rejects disabled write/exec modes or overlapping candidate/config roots first, authenticates the delivery next, and only then reads the separately provisioned API credential. It repeats the existing live preflight and native capture for the signed event's exact base/head. Stale delivery is rejected instead of being silently retargeted, and body fields claiming ready, PASS, sender role or human approval have no effect. Signature checking cannot fill missing native verification, approve a review, or revoke old success during an API outage. Local signed-delivery/native-verification tests are not a remote customer pilot.

## Durable intake and retry

Use a newly created private inbox separate from candidate code. Its parent directory must already exist and its absolute path must have no symlink components. The configured repository, numeric repository/App/installation IDs, Check name and inbox root are pinned; an existing, missing or damaged store is never silently reset.

```text
wcode github inbox-init --config-root /trusted/publisher-config \
  --installation-id 789 --inbox /trusted/deliveries --json
wcode github serve-inbox --config-root /trusted/publisher-config \
  --installation-id 789 --inbox /trusted/deliveries --listen 127.0.0.1:8788
wcode github inbox-status --config-root /trusted/publisher-config \
  --installation-id 789 --inbox /trusted/deliveries --json
wcode -w /trusted/candidate github publish-next \
  --config-root /trusted/publisher-config --installation-id 789 \
  --inbox /trusted/deliveries --workspace-id project-id --json
```

Serve-inbox runs in the foreground and accepts only `POST /github/webhook`. It requires the separately provisioned webhook key but never reads a publisher API token. Place it behind an HTTPS reverse proxy that preserves the original body and duplicate headers and bounds connections, header time and rate. Intake allows two requests, at most one MiB each, a five-second body deadline and an eight-second response budget. Signature/routing failure returns 401; overload, storage conflict or unconfirmed persistence returns 503, never accepted. Only confirmed persistence returns 202. Storage work stays off the async executor and retains its capacity permit until it really ends, even if the response deadline expires. GitHub requires a timely response; failures need explicit redelivery by the supervising integration, not an assumption that a webhook was accepted. See [GitHub delivery best practices](https://docs.github.com/en/webhooks/using-webhooks/best-practices-for-using-webhooks).

For automatic consumption, run a separate foreground worker from the fixed installation:

```text
wcode -w /trusted/candidate github work-inbox \
  --config-root /trusted/publisher-config --installation-id 789 \
  --inbox /trusted/deliveries --workspace-id project-id --json
```

The worker processes one due delivery at a time, waits one second between successful/idle polls and five seconds after unavailable polls, and still obeys each event's longer persisted retry deadline and attempt limit. Slow attempts do not cause a catch-up burst. Its JSON Lines stream reports startup, fresh publication outcomes, sanitized errors and shutdown; idle polls do not flood output and counters have no current Acceptance authority. On Unix SIGINT/SIGTERM stop new polls and let the active publication and state write finish; the non-Unix path handles Ctrl+C. Allow sufficient graceful-stop time in the service supervisor: forcibly killing the process can still leave an interrupted, remotely ambiguous attempt. No service is installed or daemonized. Configuration/key changes require a controlled restart, and completed events are not automatically republished after verification or Policy changes. See [Tokio graceful shutdown](https://tokio.rs/tokio/topics/shutdown).

An existing trusted receiver can instead invoke `github enqueue-event` with the same config/installation/inbox options, the original signed body on stdin and the header environment described above. Enqueue does not access API credentials. The inbox stores the original signed body and headers privately for reauthentication, encoded as base64, **not encrypted**; neither secret is persisted. Status never returns those bodies or signatures.

The intact inbox deduplicates signed body digests across redelivery, changed unsigned delivery IDs and process restarts. A short state-file lock serializes atomic snapshot writes; a separate worker lock covers one publication attempt without blocking enqueue or status. Guards explicitly unlock when the protected operation ends, so a duplicated descriptor does not keep a finished operation busy. No expiring lease starts a second worker over an unfinished request. Interrupted `publishing` remains visible, then retries after its due time; every attempt rechecks the signature with the current key, live preflight, exact candidate and native Acceptance. Backoff is 10/20/40/80/160 seconds with a maximum of five attempts. API failure or unconfirmed local completion is never a success receipt.

Key changes cannot make an unverifiable queue head block newer authenticated deliveries: the worker scans the bounded due set, retains old-key entries unchanged, and selects the first entry authenticated with the current key. When every due entry fails authentication it reports unavailability, not an empty queue, without consuming publication attempts. Invalid-length worker keys cannot mutate recovery state.

A current-key signed redelivery of the **same raw body** can update obsolete saved authentication headers for a queued/retry entry. The acknowledgement keeps `duplicate=true` and adds `reauthenticated=true`; generation advances only after the refreshed snapshot is durable. Original bytes/digest, receive time, attempt count and retry state are preserved, and retry deadlines never move earlier. Repeating the same current signature becomes an ordinary unchanged duplicate. Completed, exhausted and in-flight entries are not reset. No key is automatically generated, accepted from the request, installed or rotated, and GitHub redelivery behavior is not assumed: without a delivery that actually verifies under the current key, those original entries still require operator reconciliation. This is queue-authentication recovery, not event freshness or Acceptance.

The store retains at most 128 events within an eight-MiB snapshot, including completed replay tombstones. Capacity fails explicitly rather than deleting history or bypassing deduplication. A completed queue entry means delivery processing finished, including a blocked Check; it is not current Acceptance. No cached green receipt is replayed. For later verification/Policy changes, use the separately supervised `github watch-candidate` controller or an explicit fresh `github publish`, not replay of a completed event. A controller must inspect `exhausted`, manage secure archival, drain/reconcile deliveries before key rollover and supervise the foreground worker; these commands do not install a service or silently discard poison entries.

Remote effects are not exactly-once: an interrupted HTTP operation may have reached GitHub. Retry starts from fresh guards, not a claimed remote rollback. Local checksums detect corruption, not hostile same-user edits or restoration of an old intact snapshot. Unix private permissions and alias rejection do not establish Windows ACLs, tenant isolation, TLS or an external anti-rollback checkpoint. Those deployment and real remote Pilot gates remain separate.

## Explicit terminal-delivery archive

`github archive-inbox` frees completed/exhausted payload slots without forgetting their replay identities. It requires an existing valid inbox, write permission and the exact generation reported by `inbox-status`; it never reads API/webhook credentials or contacts GitHub. Stop or drain the active inbox worker first: archive refuses a held worker lock rather than interrupting publication. Unrelated watcher locks and read-only diagnostics stay independent.

```text
wcode github archive-inbox --config-root /trusted/publisher-config \
  --installation-id 789 --inbox /trusted/deliveries \
  --expected-generation 42 --output /private/archives/deliveries-42.json --json
wcode github inspect-inbox-archive --config-root /trusted/publisher-config \
  --installation-id 789 --inbox /trusted/deliveries \
  --archive /private/archives/deliveries-42.json --digest <independently-retained-sha256> --read-only --json
```

The destination must be new, absolute, outside the inbox, and below an existing private parent without aliases. The archive retains original signed bodies/headers and terminal metadata, **unencrypted and confidential**. It is synchronized and read back against its digest before compaction. Stdout contains only the checkpoint: exact archive digest/bytes, source and resulting generation, root digest and counts. Preserve that checkpoint independently. Inspection validates the supplied digest, enrollment/root binding and archived records, but never imports them or returns body/signature data.

Compaction retains up to 16,384 compact terminal replay tombstones beside at most 128 active payload records; the whole snapshot remains bounded to eight MiB. Status reports these counts separately. Queued, retry and interrupted publishing entries are never archived. Redelivery still authenticates, then returns `duplicate=true, archived=true` without resetting attempts or republishing; even losing the external archive cannot erase its online replay identity. A full compact index still fails explicitly rather than deleting history. This is bounded operational capacity, not unlimited retention.

Untouched version-1 snapshots keep their original checksum representation. The explicit first archive operation writes version 2 with the replay index; older binaries must reject it, not be used to downgrade or silently discard the new index. Archive and queue update are not a multi-file transaction: a failed export leaves the queue uncompacted and may leave a partial create-only destination; an uncertain final snapshot acknowledgement requires inspecting generation rather than claiming rollback or blindly retrying. No archive file, old snapshot or checkpoint can restore current Acceptance. Encryption, off-site custody, independent checkpoint protection, Windows ACLs and cross-machine recovery still need deployment validation.

## Persistence and Git provider

Protected local `acceptance-history` is bounded to 256 records of at most 2 MiB. Corruption and capacity fail explicitly. Hashes detect corruption; they are not signatures or protection from hostile repository code under the same OS user. Restart-readable records retain historical-only authority.

The OSS adapter is in `src/integrations/git/`. Enroll its non-secret repository identity once from the project:

```text
wcode setup --project \
  --github-repository owner/repo \
  --github-repository-id <numeric-repository-id> \
  --github-app-id <numeric-app-id>
```

Enrollment adds `.wcode/github-publisher.yaml` with repository/App/Check identity alongside normal project Host configuration and Design bootstrap. Invalid identities, blank Check names, malformed existing manifests and identity conflicts are rejected before those setup writes. The parent directory must pass Workspace guards even when the manifest is missing. The final enrollment write rechecks its inputs without overwriting another identity; setup is not an atomic multi-file transaction. It stores no publisher token and changes no GitHub settings; `--dry-run --json` previews the same enrollment without writing. `examples/git_publisher.rs` reads that manifest, obtains its credential separately from the publisher environment, and uses the public fresh-capture method. It validates current PR base/head, repository ID, expected Check App and acknowledgements without running repository commands or importing worker PASS JSON. Isolate the publisher, Policy and credentials from the untrusted worker.

GitHub required checks can accept neutral or skipped. This adapter requires exact SHA and expected App with completed/success; configure branch protection to require that check source. See [protected branches](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches) and [Checks API](https://docs.github.com/en/rest/checks/runs).

An API outage fails the current publication but cannot retroactively revoke an existing success at the same SHA. Remote PR, local inputs and Policy do not form an atomic transaction. A real remote PR pilot, protected deployment and organization governance require separate validation; protocol tests do not establish deployment.
