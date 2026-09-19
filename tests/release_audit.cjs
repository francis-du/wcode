'use strict';
// One hundred distinct adversarial rounds, not repetitions of a green suite.
// Run from the repository root: node tests/release_audit.cjs
// No publishing, credentials, arbitrary commands, or source mutations.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const {execFile, execFileSync} = require('node:child_process');
const {promisify} = require('node:util');
const exec = promisify(execFile);
const root = path.resolve(__dirname, '..');
const cargo = filter => ({program:'cargo',args:['test','--locked','--quiet','--lib',filter],kind:'rust'});
const cargoExact = filter => ({program:'cargo',args:['test','--locked','--quiet','--lib',filter,'--','--exact'],kind:'rust'});
const cargoTest = (test,filter) => ({program:'cargo',args:['test','--locked','--quiet','--test',test,filter],kind:'rust'});
const node = (file, scenario) => ({program:process.execPath,args:[file,'.',...(scenario?[scenario]:[])],kind:'json'});
const swift = file => ({program:'swift',args:[file],kind:'json'});
const rustRounds = [
  ['Same-URL cleanup cannot revoke replacement trust','stale_endpoint_cleanup_preserves_same_url_replacement_trust'],
  ['Queued Connected cannot borrow a newer registration','queued_connection_cannot_borrow_a_newer_registration'],
  ['Pre-quarantine success cannot undo quarantine','quarantine_invalidates_a_pre_quarantine_success'],
  ['Duplicate completion is not a second failure','duplicate_probe_completion_is_not_a_second_failure'],
  ['Every probe attempt owns a distinct epoch','probe_epoch_is_unique_for_each_attempt'],
  ['Retained aliases are probed without a provider child','retained_aliases_receive_instance_matched_probes_without_a_provider_child'],
  ['Bounded aliases survive unrelated startup failures','retained_aliases_are_bounded_and_reconnect_failure_is_not_revocation_evidence'],
  ['Retired alias cleanup preserves its live provider','retired_alias_cleanup_does_not_recycle_its_live_replacement_provider'],
  ['Host and Origin parsing rejects spoofing and duplicates','auth_origin::tests::request_hosts_reject_duplicates_spoofed_forwarding_and_url_syntax'],
  ['Historical OAuth resources cannot reactivate a Host','old_tunnel_resource_does_not_make_old_host_active_after_restart'],
  ['Late endpoint health cannot poison a replacement lease','stale_endpoint_probe_result_cannot_mutate_a_replacement_lease'],
  ['Health hysteresis requires real failure/recovery sequences','public_url_health_requires_three_failures_and_two_successes_to_recover'],
  ['Cancelled tunnel startup cleans descendants without collateral kills','cancelled_tunnel_startup_closes_descendant_pipes_without_touching_other_children'],
  ['Author and bulk authorization keys stay disjoint','author_and_bulk_authorization_never_share_a_key'],
  ['Modified chords cannot trigger plain-key grants','modified_keys_cannot_trigger_unmodified_permissions_or_navigation'],
  ['Invisible command panels cannot toggle permissions','invisible_command_panel_cannot_toggle_permissions'],
  ['Repeated key events cannot repeat grants or links','repeated_keys_never_repeat_grants_toggles_or_external_links'],
  ['Input and confirmation contexts are exclusive','input_and_full_access_confirmation_are_exclusive_contexts'],
  ['Navigation is routed only to its visible context','navigation_routes_only_to_the_visible_context'],
];
const rounds = rustRounds.map(([name,filter],i)=>({round:i+1,name,lane:'rust',steps:[cargo(filter)]}));
rounds[1].steps.push(cargo('tunnel_event_stays_compact_and_preserves_endpoint_ownership'));
rounds.push({round:20,name:'Refresh status, timeouts, cancellation and revision rollback',lane:'web',steps:[node('tests/unit/ui/refresh.cjs')]});
const scenarios = ['cached-render-failure','activity-render-failure','effective-zero-beats-history','failed-evidence-is-not-green','activity-old-error-cannot-poison-new-workspace','proof-details-are-bounded-and-escaped'];
scenarios.forEach((name,i)=>rounds.push({round:21+i,name,lane:'web',steps:[node('tests/unit/ui/adversarial.cjs',name)]}));
rounds.push(
  {round:27,name:'Workspace-scoped semantic and authorization responses',lane:'web',steps:[node('tests/unit/ui/release.cjs')]},
  {round:28,name:'Nested grids and long-content geometry across 12 widths',lane:'web',steps:[node('tests/unit/ui/layout.cjs'),swift('tests/unit/ui/layout_webkit.swift')]},
  {round:29,name:'Complete production DOM: 240 WebKit scenarios including Code Graph fullscreen',lane:'web',steps:[node('tests/unit/ui/code_graph.cjs'),node('tests/unit/ui/web_i18n.cjs'),node('tests/unit/ui/browser.cjs'),swift('tests/unit/ui/browser_webkit.swift')]},
  {round:30,name:'Release regressions and fail-closed readiness',lane:'rust',steps:[cargo('intelligence::release_gate::tests'),cargo('migration_audit::tests'),cargo('syntax_search_cache_eviction_is_not_reported_as_file_failure'),cargo('workspace_relative_verification_executable_resolves_from_workspace_root'),cargo('syntax_search_defaults_cover_more_than_one_thousand_files_and_skip_comments'),cargo('direct_run_command_revision_flights_coalesce_verification_shape'),cargo('engineering_fitness_live_controls_preserve_safety_and_precision'),cargo('positive_harness_tools_flow_through_mcp'),cargo('postlude_budget_'),cargoTest('release_contract','release_metadata_versions_match_the_cargo_package')]},
);
const extraRustRounds = [
  "auth_origin::tests::browser_origins_follow_active_aliases_not_the_primary_or_token_history",
  "auth_origin::tests::changing_primary_keeps_previous_origins_registered",
  "auth_origin::tests::malformed_and_duplicate_origin_headers_are_not_treated_as_absent",
  "auth_origin::tests::missing_host_does_not_resurrect_an_unregistered_primary",
  "auth_origin::tests::origin_headers_reject_url_repairs_credentials_and_untrusted_origins",
  "auth_origin::tests::primary_promotion_preserves_the_endpoint_owner_epoch",
  "auth_origin::tests::removed_tunnel_is_historical_only",
  "auth_origin::tests::selects_only_registered_request_origins",
  "auth_origin::tests::stale_endpoint_cleanup_cannot_remove_a_new_same_url_registration",
  "auth_origin::tests::unknown_epoch_never_removes_another_endpoint",
  "auth::tests::authorization_metadata_prefers_dcr_until_cimd_is_safely_supported",
  "auth::tests::chatgpt_oauth_callback_includes_issuer_and_exchanges_resource_bound_code",
  "auth::tests::each_auth_state_has_an_independent_instance_id",
  "auth::tests::expired_authorization_code_is_rejected_and_removed",
  "auth::tests::issued_token_state_remains_bounded",
  "auth::tests::modern_mcp_registration_profiles_cover_web_and_native_agents",
  "auth::tests::oauth_metadata_uses_the_tunnel_that_received_the_request",
  "auth::tests::old_access_tokens_follow_verified_tunnel_aliases_without_expiry",
  "auth::tests::old_refresh_token_remains_valid_for_runtime_lifetime",
  "auth::tests::pairing_code_is_always_six_ascii_digits",
  "auth::tests::pairing_failures_lock_out_client_and_success_clears_attempts",
  "auth::tests::parses_chatgpt_resource_and_scope",
  "auth::tests::percent_encoded_backslash_remains_data",
  "auth::tests::pkce_matches_rfc_example",
  "auth::tests::protected_resource_metadata_matches_mcp_resource_identifier",
  "auth::tests::redirect_uri_policy_allows_https_and_loopback_only",
  "auth::tests::refresh_token_binding_mismatch_does_not_rotate",
  "auth::tests::refresh_token_moves_to_a_verified_reconnected_tunnel",
  "authorization::tests::command_access_requests_retain_the_requested_program",
  "authorization::tests::denial_never_creates_a_grant",
  "authorization::tests::destructive_approval_is_exact_and_consumed_once",
  "authorization::tests::pending_request_deduplication_is_workspace_and_kind_bound",
  "authorization::tests::session_grants_require_explicit_human_approval",
  "authorization::tests::workspace_command_grant_resolves_command_requests_but_not_delete",
  "execution::tests::blocked_worklist_projects_to_blocked_execution_without_chat_state",
  "execution::tests::bound_reconciliation_plan_survives_expected_repository_revision_change",
  "execution::tests::execution_checkpoint_tracks_worklist_and_requires_verification_to_complete",
  "execution::tests::execution_handoff_creates_clean_lineage_without_transcript_state",
  "execution::tests::execution_steering_is_revision_guarded_and_survives_reload",
  "execution::tests::model_terminal_proposal_is_advisory_and_cannot_self_complete",
  "execution::tests::scope_steering_against_bound_plan_requires_replan",
  "execution::tests::verification_ready_cannot_complete_until_current_reconciliation_converges",
  "execution::tests::verification_steering_floor_is_monotonic_across_applied_directives",
  "execution::tests::verification_steering_floor_raises_policy_and_rejects_weaker_proof",
  "execution::tests::worklist_restart_creates_a_new_execution_generation",
  "intelligence::tests::verification::configured_stage_executor_produces_real_persistent_stage_evidence",
  "intelligence::tests::verification::design_revision_change_invalidates_plan_and_evidence",
  "intelligence::tests::verification::equal_timestamp_human_approval_conflict_fails_closed",
  "intelligence::tests::verification::equal_timestamp_stage_evidence_conflicts_fail_closed_per_producer",
  "intelligence::tests::verification::explicit_human_approval_clears_only_the_human_blocker",
  "intelligence::tests::verification::explicit_presentation_stage_executor_opts_the_language_into_targeting",
  "intelligence::tests::verification::low_risk_verification_becomes_ready_after_deterministic_and_blind_review_pass",
  "intelligence::tests::verification::required_stage_evidence_replaces_automation_gap_blockers",
  "intelligence::tests::verification::required_stage_without_local_executor_is_an_explicit_verification_gap",
  "intelligence::tests::verification::reviewer_disagreement_is_persisted_as_evidence_once",
  "resource::process_queue::admission_tests::bounded_child_wait_expires_without_leaking_capacity_or_waiters",
  "resource::process_queue::admission_tests::host_process_cap_bounds_aggregate_subspace_activity",
  "resource::process_queue::admission_tests::independent_process_queues_keep_compiler_and_probe_limits",
  "resource::process_queue::admission_tests::inspection_capacity_scales_with_memory_cpu_and_tool_bounds",
  "resource::process_queue::admission_tests::inspection_queue_still_obeys_resource_pressure_admission",
  "resource::process_queue::admission_tests::subspace_process_admission_isolated_under_shared_host_capacity",
  "mcp::mcp_tools::tests::cancelled_blocking_worker_remains_visible_until_real_capacity_is_released",
  "mcp::mcp_tools::tests::cancelled_blocking_worker_retains_its_real_permit_until_finished",
  "mcp::mcp_tools::tests::cancelled_execution_worker_retains_both_admission_permits",
  "mcp::mcp_tools::tests::cancelled_queued_blocking_worker_never_starts",
  "mcp::mcp_tools::tests::panicking_blocking_worker_releases_its_permit",
  "mcp::mcp_writer::tests::active_writer_lease_gates_file_and_mutating_command_tools",
  "mcp::mcp_writer::tests::bound_mutation_requires_explicit_plan_approval",
  "mcp::mcp_writer::tests::mutation_domain_groups_subspaces_and_separates_linked_worktrees",
  "mcp::mcp_writer::tests::writer_restart_with_durable_claim_fails_closed_without_runtime_lease",
];
extraRustRounds.forEach((filter,index)=>rounds.push({
  round:31+index,
  name:`Boundary ${31+index}: ${filter.split('::').at(-1).replaceAll('_',' ')}`,
  lane:'rust',
  steps:[cargoExact(filter)],
}));
assert.equal(extraRustRounds.length,70);
assert.equal(new Set(extraRustRounds).size,70);
assert.equal(rounds.length,100);
assert.equal(new Set(rounds.map(r=>r.round)).size,100);
assert.equal(new Set(rounds.map(r=>r.name)).size,100);

function selectedRounds() {
  const option=process.argv.find(value=>value.startsWith('--rounds='));
  if(!option) return rounds;
  const match=/^--rounds=(\d+)-(\d+)$/.exec(option);
  assert.ok(match,'--rounds must use START-END');
  const start=Number(match[1]), end=Number(match[2]);
  assert.ok(Number.isInteger(start)&&Number.isInteger(end)&&start>=1&&end<=100&&start<=end,'--rounds must stay within 1-100');
  const selected=rounds.filter(round=>round.round>=start&&round.round<=end);
  assert.equal(selected.length,end-start+1,'selected release-audit range must be contiguous');
  return selected;
}
function gitState() {
  const head=execFileSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'}).trim();
  const status=execFileSync('git',['status','--porcelain=v1','--untracked-files=all'],{cwd:root,encoding:'utf8'}).trim();
  return {
    head,
    clean:status.length===0,
    changed_entries:status?status.split(/\r?\n/).length:0,
    status_sha256:crypto.createHash('sha256').update(status).digest('hex'),
  };
}
function digest() {
  const hash=crypto.createHash('sha256');
  let count=0;
  function visit(relative) {
    const absolute=path.join(root,relative), stat=fs.lstatSync(absolute);
    assert.ok(!stat.isSymbolicLink(),'Audit inputs cannot be symlinks: '+relative);
    if(stat.isDirectory()) {
      for(const name of fs.readdirSync(absolute).sort()) visit(path.join(relative,name));
    } else if(stat.isFile() && /\.(rs|js|cjs|swift|css|html|yaml|yml|md|json|toml|lock)$/.test(relative)) {
      assert.ok(++count<=10000,'Audit input file bound exceeded');
      assert.ok(stat.size<=8*1024*1024,'Audit input byte bound exceeded');
      hash.update(relative.replaceAll(path.sep,'/')).update('\0').update(fs.readFileSync(absolute)).update('\0');
    }
  }
  for(const relative of ['src','tests','.wcode/design','docs/manual','plugin','.github/workflows','Cargo.toml','Cargo.lock','marketplace.json']) visit(relative);
  return {sha256:hash.digest('hex'),files:count};
}
async function check(step) {
  const started=Date.now();
  const {stdout,stderr}=await exec(step.program,step.args,{cwd:root,timeout:120000,maxBuffer:2*1024*1024,windowsHide:true});
  let cases=0;
  if(step.kind==='rust') {
    const matches=[...stdout.matchAll(/test result: ok\. (\d+) passed;/g)];
    cases=matches.reduce((n,m)=>n+Number(m[1]),0);
    assert.ok(cases>0,'A zero-test Rust match is not verification');
  } else {
    const report=JSON.parse(stdout);
    if(Array.isArray(report.results)) {
      assert.ok(report.results.length>0,'Empty result set');
      assert.ok(report.results.every(r=>r.passed===true||(Array.isArray(r.errors)&&r.errors.length===0)),'Scenario failure');
      assert.ok(report.failures===undefined||report.failures===0,'Browser failure');
      cases=report.cases||report.results.length;
    } else assert.equal(report.passed,true,'Fixture generation failed');
  }
  return {command:[path.basename(step.program),...step.args],exit_code:0,elapsed_ms:Date.now()-started,cases,output_sha256:crypto.createHash('sha256').update(stdout).update(stderr).digest('hex')};
}
async function main() {
  assert.equal(process.platform,'darwin','This complete audit requires macOS WebKit; other CI platforms run the portable cargo suite');
  const selected=selectedRounds();
  const requireClean=process.argv.includes('--require-clean');
  const git_before=gitState();
  if(requireClean) assert.ok(git_before.clean,'Final release audit requires a clean worktree');
  const before=digest(), started_at=new Date().toISOString(), results=[];
  // Rust compilation is serialized; the independent browser/JS lane runs concurrently.
  await Promise.all(['rust','web'].map(async lane=>{
    for(const round of selected.filter(r=>r.lane===lane)) {
      const result={round:round.round,name:round.name,passed:false,steps:[]};
      try {for(const step of round.steps) result.steps.push(await check(step));result.passed=true;}
      catch(error) {result.error=String(error.message);result.diagnostics=String(error.stderr||error.stdout||'').slice(-6000);}
      results.push(result);
    }
  }));
  results.sort((a,b)=>a.round-b.round);
  const after=digest(), git_after=gitState();
  const stable=before.sha256===after.sha256
    && git_before.head===git_after.head
    && git_before.status_sha256===git_after.status_sha256;
  const start=selected[0].round, end=selected[selected.length-1].round;
  const complete=selected.length===rounds.length;
  const report={
    suite:complete?'release-adversarial-100':'release-adversarial-shard',
    started_at,finished_at:new Date().toISOString(),
    git:{before:git_before,after:git_after,require_clean:requireClean},
    input:before,stable_inputs:stable,
    rounds:results.length,total_rounds:rounds.length,selected_rounds:{start,end},
    passed:results.every(r=>r.passed)&&stable,results
  };
  const target=path.join(root,'target');
  if(!fs.existsSync(target)) fs.mkdirSync(target);
  assert.ok(fs.lstatSync(target).isDirectory()&&!fs.lstatSync(target).isSymbolicLink());
  const filename=path.join(target,complete?'wcode-adversarial-100.json':`wcode-adversarial-${start}-${end}.json`);
  if(fs.existsSync(filename)) {const st=fs.lstatSync(filename);assert.ok(st.isFile()&&!st.isSymbolicLink()&&st.nlink===1);}
  fs.writeFileSync(filename,JSON.stringify(report,null,2)+'\n');
  console.log(JSON.stringify(report,null,2));
  assert.ok(report.passed,'Adversarial audit failed, Git state changed, or source changed during the run');
}
main().catch(error=>{console.error(error);process.exitCode=1;});