# Exact-candidate GitHub publisher deployment template

This opt-in template targets `francis-du/wcode`. It is not an installed App, enabled workflow, branch rule or customer deployment. Keep it outside `.github/workflows` until a trusted operator deliberately provisions and reviews the deployment.

## Fixed installed publisher

Build the reviewed Wcode revision outside any candidate checkout, then install its `wcode` binary as `/opt/wcode/bin/wcode`. Use the built-in `github preflight` and `github publish` commands. Do not compile or run the publisher from PR-provided source. `examples/git_publisher.rs` remains a lower-level API example, not the operational entrypoint for this template.

Provision an operator-reviewed `.wcode/github-publisher.yaml` under a trusted configuration directory. It binds repository name, numeric repository ID, expected App ID and required Check name using the existing enrollment schema. The configuration directory must not overlap the managed candidate checkout. Keep configuration, native Policy/Evidence/Verification state and the binary outside the untrusted execution worker's writable environment.

Supply the short-lived installation credential only through `WCODE_GITHUB_PUBLISHER_TOKEN` in the publisher's protected environment. Do not put it in argv, PR artifacts, build/test environments or logs. Preflight requires enough visibility to inspect the selected protection rules; missing metadata remains incomplete and does not justify silently broadening management privileges. Static-token mode does not issue or renew credentials; the explicit App-key mode below is a separate, more privileged deployment choice. See the [Checks API](https://docs.github.com/en/rest/checks/runs) and [secure use of Actions](https://docs.github.com/en/actions/reference/security/secure-use).

```sh
/opt/wcode/bin/wcode github preflight \
  --config-root /srv/wcode/publisher-config --pull "$PULL_NUMBER" --check --json
/opt/wcode/bin/wcode -w /srv/wcode/candidates/current github publish \
  --config-root /srv/wcode/publisher-config --workspace-id candidate \
  --pull "$PULL_NUMBER" --base "$CANDIDATE_BASE" --head "$CANDIDATE_HEAD" --json
```

Both SHAs must be complete, independently selected identities for the intended PR. A stale dispatch fails instead of silently retargeting another head. Publication repeats preflight, starts with a failed Check, freshly captures native Acceptance, and only publishes success after the exact-candidate and acknowledgement guards. It never runs missing project checks, approves Policy, imports worker PASS JSON, changes branch rules or merges a PR.

## Optional App-key renewal on a trusted publisher host

Instead of providing a static token, unset `WCODE_GITHUB_PUBLISHER_TOKEN` and provide `WCODE_GITHUB_INSTALLATION_ID` plus exactly one key source. Unix deployments should use `WCODE_GITHUB_APP_PRIVATE_KEY_FILE` with a private publisher-owned PEM outside candidate/inbox directories; cross-platform protected service launchers may inject PEM contents directly as `WCODE_GITHUB_APP_PRIVATE_KEY`, which wcode validates in memory and never persists. The legacy `WCODE_GITHUB_APP_KEY_FILE` remains Unix-compatible but is deprecated. Use a dedicated App: custody of its private key is more powerful than custody of a repository-scoped token. Do not put the key, its contents or any credential source in workflow inputs, PR artifacts or candidate execution environments.

The installed provider verifies live repository/installation/App identity, requests only the enrolled repository with Checks write and bounded read permissions, validates the returned scope/expiry, and renews on demand within 120 seconds of expiry. Concurrent callers share one bounded exchange; failed renewal backs off without expired-token fallback. An operator-replaced PEM is re-read at renewal. Neither this mode nor a 401 automatically replays Check writes. Existing single-writer guards and native acceptance checks remain required.

Preflight still only reads repository configuration, but App-mode authentication can issue a token, so its JSON explicitly reports credential renewal and possible remote authentication mutation. It does not create/install an App or change repository permissions. Non-Unix private-file mode fails closed rather than pretending to validate ACLs; in-memory App-key renewal and externally managed static-token mode are cross-platform. Guarded child processes strip canonical App private-key variables and the legacy key-file variable. See the [credential renewal contract](../../docs/manual/change-acceptance.md#optional-installation-token-renewal) and [GitHub installation authentication](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-an-installation-access-token-for-a-github-app).

## Supervised durable delivery

A separate foreground receiver can use `github serve-inbox` behind a trusted HTTPS proxy. It validates raw signatures and persists signed bodies, but never reads the publisher API credential. The publisher's foreground `github work-inbox` command processes due deliveries without an external busy-poll shell loop:

```sh
/opt/wcode/bin/wcode -w /srv/wcode/candidates/current github work-inbox \
  --config-root /srv/wcode/publisher-config --installation-id "$INSTALLATION_ID" \
  --inbox /srv/wcode/deliveries --workspace-id candidate --json
```

Initialize the private inbox deliberately with `github inbox-init` first. The worker needs the matching webhook verification key in `WCODE_GITHUB_WEBHOOK_SECRET` and the separate API credential above. Configure service supervision, key rollover, HTTPS, connection limits and isolation independently; these commands do not install, daemonize or restart themselves. A changed configuration or key requires a controlled restart and reconciliation of pending deliveries, not deletion of replay tombstones.

The worker serializes one publication at a time, respects persisted retry deadlines and the five-attempt limit, and does not replay completed events. Idle polls are spaced one second apart; unavailable polls wait five seconds before polling again, without bypassing the longer per-event backoff. Output is bounded JSON Lines for startup, actual outcomes, errors and shutdown; idle polls do not flood logs and raw error payloads are not copied into output. `SIGINT` and `SIGTERM` on Unix stop new polls and let the current attempt finish; Ctrl+C provides the non-Unix path. The supervisor must allow that drain to finish. A forced kill can leave an interrupted attempt and ambiguous remote effects, which remain subject to the inbox recovery rules.

After a controlled key change, an old-key pending entry cannot starve later current-key deliveries. A redelivery of the identical raw bytes that actually authenticates with the new key can refresh queued/retry signature headers; `duplicate=true, reauthenticated=true` confirms the persisted update. Attempts and original body identity stay intact and backoff cannot shorten. Repeating the new signature is then a no-write duplicate. Completed/exhausted/in-flight events are not reset; entries without a current valid signature remain visible and require reconciliation. Do not assume that provider redelivery signs with a particular key, manually edit the snapshot, or delete tombstones to clear the queue.

A completed delivery is not a continuously valid merge authorization. Use the separately supervised fixed-candidate controller below after intake, or explicitly invoke fresh publication. Monitor inbox capacity and exhausted entries; no automatic deletion, archival or anti-rollback guarantee is added by the worker.

## Continuous candidate controller

```sh
/opt/wcode/bin/wcode -w /srv/wcode/candidates/current github watch-candidate \
  --config-root /srv/wcode/publisher-config --workspace-id candidate \
  --pull "$PULL_NUMBER" --base "$CANDIDATE_BASE" --head "$CANDIDATE_HEAD" --json
```

This foreground process rechecks live deployment and native proof after a 30-second completion-to-next-round delay, backing off to 60/120/240 seconds on errors. It owns one newly created red Check and reuses its exact ID: unchanged fresh proof only reads, changed proof goes through red-first recapture, and failed preflight attempts denial of the original Check instead of retargeting a new head. It never runs PR tests, grants human approval, imports a saved receipt or changes inbox replay history. New verification can make the candidate ready; revocation or missing current proof cannot reuse old green. Graceful SIGINT/SIGTERM drains the current round and read-back-confirms denial before reporting stopped. Errors or GitHub outages explicitly leave denial unconfirmed; forced kill cannot clean up remotely.

Cooperative processes sharing the same native authority-state root now enforce nonblocking single-writer admission across watchers, inbox workers, one-shot publishers and lower-level Check writes. Set the same protected `WCODE_STATE_DIR` for these processes. Admission keys use the API, numeric repository ID, App, Check name and head SHA, not credentials, candidate/config directories or PR/base identities. A watcher keeps its slot between rounds and through its shutdown denial attempt. Contenders receive an explicit busy error without Check writes; read-only preflight, inbox status and intake remain available. Inbox contention does not spend attempts or shorten backoff and cannot starve a different due candidate.

The private `github-publication-locks` subdirectory contains stable empty lock files, not proof, tokens or cached Check IDs. Guards explicitly unlock; they never delete the inode or expire an active holder. Do not clear a live lock by removing these files. Local process death releases only local admission, not a request already accepted by GitHub. Unix private-path and inode checks are tested; Windows ACLs and native deployment validation remain separate. Distinct state roots, hosts, older binaries and noncooperative writers still require external coordination.

The supervisor must deliberately route a new candidate, protect/provision App signing keys or renew externally managed static credentials, handle configuration changes and restart/cutover failures; the process does not install or supervise itself. Polling plus GitHub updates is not an atomic merge lock or an outage-proof revocation guarantee. These local native/HTTP regression results do not replace a real deployment Pilot.

## Explicit capacity maintenance

Drain the inbox worker and inspect its generation before invoking `github archive-inbox --config-root /srv/wcode/publisher-config --installation-id <id> --inbox /srv/wcode/deliveries --expected-generation <generation> --output /private/archives/delivery-<generation>.json --json`. The new private destination must be outside the live inbox. Only completed/exhausted payloads are exported; queued/retry/interrupted work is preserved. Full export and read-back confirmation precede compaction, and the returned digest/generation checkpoint must be retained independently. `github inspect-inbox-archive` validates that checkpoint digest without importing payloads or publishing Checks.

Online terminal replay tombstones remain after compaction or archive-file loss, so a signed duplicate cannot become new work. Active payload capacity is 128, compact replay capacity 16,384, and the aggregate snapshot is still limited to eight MiB. Full indices never silently prune. Explicit archival upgrades a legacy inbox to schema 2; older binaries cannot read that state. Export and snapshot replacement are not a cross-file transaction, so failed/uncertain acknowledgement requires inspecting the actual files and generation. Archives contain confidential raw signed payloads without encryption; configure custody, backup and ACLs independently. This maintenance does not delete publication lock files, restore proof or manage remote effects.

## Opt-in Actions dispatch

`github-gate.yml` uses `workflow_dispatch`, a dedicated `wcode-publisher` runner and a protected environment. Set protected variables for the candidate path, workspace identity, native state root and publisher configuration root. Provide the fresh installation credential as the configured secret. Dispatch requires the PR number and complete base/head SHAs.

The script passes input values through quoted environment variables, not directly interpolated shell source. It invokes the fixed installed CLI without a checkout, build, PR artifact download or worker proof import. Keep the workflow itself on a reviewed protected ref and restrict who can dispatch it. The regression test executes this template with the built binary in read-only mode: its arguments must parse and reach the publication safety guard before any credential access or mutation.

The operator must separately require the exact Check and expected App, strict base updates and the intended bypass restrictions. Merge queues remain unsupported by the PR-head publisher. No remote rules are modified by this template. See [protected branches](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches).

## Trust and outage limits

Local native stores are not signed remote worker attestations. Separate directories and process supervision alone do not establish hostile same-user or tenant isolation. The intake process and untrusted code must not be able to obtain the publisher credential or forge the protected native verification state. Trustworthy execution/state ingestion and OS or container isolation require their own deployment validation.

Check publication uses sequential guards, not an atomic cross-system merge transaction. Policy or remote state can change after observation. A later guard failure attempts to replace success with failure; a GitHub outage can prevent that revocation, and an earlier same-SHA success can remain visible. The adapter reports unavailability, not a claimed rollback. A stronger continuously enforced merge guarantee needs external coordination.

Local tests exercise real temporary Git repositories, native verification, bounded loopback HTTP, durable queue recovery, and installed CLI contracts. These results are not an attestation of an installed GitHub App, customer isolation or a real remote PR pilot.
