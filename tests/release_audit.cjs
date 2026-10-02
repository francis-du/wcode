'use strict';
// Three hundred distinct adversarial rounds, not repetitions of a green suite.
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
const FULL_AUDIT_ROUNDS = 300;
const POSTLUDE_RUST_ROUNDS = 200;
const cargo = filter => ({program:'cargo',args:['test','--locked','--quiet','--lib',filter],kind:'rust'});
const cargoExact = filter => ({program:'cargo',args:['test','--locked','--quiet','--lib',filter,'--','--exact'],kind:'rust'});
const cargoTest = (test,filter) => ({program:'cargo',args:['test','--locked','--quiet','--test',test,filter],kind:'rust'});
const node = (file, scenario) => ({program:process.execPath,args:[file,'.',...(scenario?[scenario]:[])],kind:'json'});
const swift = (file,expected_cases) => ({program:'swift',args:[file],kind:'json',expected_cases,
  expected_suite:file==='tests/unit/ui/browser_webkit.swift'?'full-browser-adversarial':'webkit-layout'});
const rustRounds = [
  ['Same-URL cleanup cannot revoke replacement trust','stale_endpoint_cleanup_preserves_same_url_replacement_trust'],
  ['Queued Connected cannot borrow a newer registration','queued_connection_cannot_borrow_a_newer_registration'],
  ['Pre-quarantine success cannot undo quarantine','quarantine_invalidates_a_pre_quarantine_success'],
  ['Duplicate completion is not a second failure','duplicate_probe_completion_is_not_a_second_failure'],
  ['Every probe attempt owns a distinct epoch','probe_epoch_is_unique_for_each_attempt'],
  ['Retained aliases are probed without a provider child','retained_aliases_receive_instance_matched_probes_without_a_provider_child'],
  ['Bounded aliases survive unrelated startup failures','retained_aliases_are_bounded_and_reconnect_failure_is_not_revocation_evidence'],
  ['Retired alias cleanup preserves its live provider','retired_alias_cleanup_does_not_recycle_its_live_replacement_provider'],
  ['Host and Origin parsing rejects spoofing and duplicates','request_hosts_accept_custom_domains_but_reject_duplicates_and_url_syntax'],
  ['Historical OAuth resources cannot reactivate a Host','historical_resource_does_not_block_a_custom_host_after_restart'],
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
  {round:28,name:'Nested grids and long-content geometry across 12 widths',lane:'web',steps:[node('tests/unit/ui/layout.cjs'),swift('tests/unit/ui/layout_webkit.swift',12)]},
  {round:29,name:'Complete production DOM: 256 WebKit scenarios including Code Graph fullscreen',lane:'web',steps:[node('tests/unit/ui/code_graph.cjs'),node('tests/unit/ui/web_i18n.cjs'),node('tests/unit/ui/browser.cjs'),swift('tests/unit/ui/browser_webkit.swift',256)]},
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
  "auth_origin::tests::selects_registered_origins_and_accepts_custom_request_hosts",
  "auth_origin::tests::stale_endpoint_cleanup_cannot_remove_a_new_same_url_registration",
  "auth_origin::tests::unknown_epoch_never_removes_another_endpoint",
  "auth::tests::authorization_metadata_prefers_dcr_until_cimd_is_safely_supported",
  "auth::tests::chatgpt_oauth_callback_includes_issuer_and_exchanges_resource_bound_code",
  "auth::tests::each_auth_state_has_an_independent_instance_id",
  "auth::tests::expired_authorization_code_is_rejected_and_removed",
  "auth::tests::issued_token_state_remains_bounded",
  "auth::tests::modern_mcp_registration_profiles_cover_web_and_native_agents",
  "auth::tests::oauth_metadata_uses_the_tunnel_that_received_the_request",
  "auth::tests::current_access_tokens_follow_verified_tunnel_aliases",
  "auth::tests::expired_refresh_token_cannot_rotate_credentials",
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

function listedLibTests() {
  const list = extra => execFileSync(
    'cargo',['test','--locked','--quiet','--lib','--',...extra,'--list'],
    {cwd:root,encoding:'utf8',timeout:120000,maxBuffer:4*1024*1024,windowsHide:true},
  ).split(/\r?\n/)
    .filter(line=>line.endsWith(': test'))
    .map(line=>line.slice(0,-6));
  const ignored=new Set(list(['--ignored']));
  return list([]).filter(testName=>!ignored.has(testName)).sort();
}
function existingRustRoundCovers(testName) {
  return rounds.some(round=>round.steps.some(step=>{
    if(step.program!=='cargo'||!step.args.includes('--lib')) return false;
    const libIndex=step.args.indexOf('--lib');
    const filter=step.args[libIndex+1];
    if(!filter||filter==='--') return false;
    return step.args.includes('--exact') ? testName===filter : testName.includes(filter);
  }));
}
function selectPostludeRustRounds(currentLibTests) {
  const buckets=new Map();
  for(const testName of currentLibTests) {
    if(existingRustRoundCovers(testName)) continue;
    const group=testName.split('::')[0]||'root';
    if(!buckets.has(group)) buckets.set(group,[]);
    buckets.get(group).push(testName);
  }
  const groups=[...buckets.keys()].sort();
  const selected=[];
  for(let index=0;selected.length<POSTLUDE_RUST_ROUNDS;index++) {
    let progressed=false;
    for(const group of groups) {
      const candidate=buckets.get(group)[index];
      if(!candidate) continue;
      selected.push(candidate);
      progressed=true;
      if(selected.length===POSTLUDE_RUST_ROUNDS) break;
    }
    assert.ok(progressed,`Need ${POSTLUDE_RUST_ROUNDS} uncovered lib tests for the 300-round audit`);
  }
  return selected;
}
function buildRounds(currentLibTests) {
  const currentLibTestSet=new Set(currentLibTests);
  for(const filter of extraRustRounds) {
    assert.ok(currentLibTestSet.has(filter), `Static adversarial Rust round is stale or missing: ${filter}`);
  }
  for(const round of rounds) for(const step of round.steps) {
    if(step.kind!=='rust'||!step.args.includes('--lib')) continue;
    const filter=step.args[step.args.indexOf('--lib')+1];
    assert.ok(currentLibTests.some(name=>step.args.includes('--exact')?name===filter:name.includes(filter)),
      `Adversarial Rust round ${round.round} is stale or missing: ${filter}`);
  }
  const postludeRustRounds=selectPostludeRustRounds(currentLibTests);
  assert.equal(postludeRustRounds.length,POSTLUDE_RUST_ROUNDS);
  assert.equal(new Set(postludeRustRounds).size,POSTLUDE_RUST_ROUNDS);
  const allRounds=rounds.concat(postludeRustRounds.map((filter,index)=>({
    round:101+index,
    name:`Postlude ${101+index}: ${filter.replaceAll('::',' / ').replaceAll('_',' ')}`,
    lane:'rust',
    steps:[cargoExact(filter)],
  })));
  assert.equal(allRounds.length,FULL_AUDIT_ROUNDS);
  assert.equal(new Set(allRounds.map(r=>r.round)).size,FULL_AUDIT_ROUNDS);
  assert.equal(new Set(allRounds.map(r=>r.name)).size,FULL_AUDIT_ROUNDS);
  return allRounds;
}

function parseOptions(argv) {
  const options={start:1,end:FULL_AUDIT_ROUNDS,require_clean:false,help:false};
  const seen=new Set();
  for(const arg of argv) {
    const key=arg.startsWith('--rounds=')?'--rounds':arg==='-h'?'--help':arg;
    assert.ok(!seen.has(key),`Duplicate audit option: ${key}`);
    seen.add(key);
    if(key==='--help') options.help=true;
    else if(key==='--require-clean') options.require_clean=true;
    else if(key==='--rounds') {
      const match=/^--rounds=(\d+)-(\d+)$/.exec(arg);
      assert.ok(match,'--rounds must use START-END');
      options.start=Number(match[1]);options.end=Number(match[2]);
      assert.ok(Number.isSafeInteger(options.start)&&Number.isSafeInteger(options.end)
        &&options.start>=1&&options.end<=FULL_AUDIT_ROUNDS&&options.start<=options.end,
        '--rounds must stay within 1-300');
    } else assert.fail(`Unknown audit option: ${arg}`);
  }
  assert.ok(!options.help||seen.size===1,'--help cannot be combined with execution options');
  return options;
}
function selectedRounds(argv=process.argv.slice(2), candidates=rounds) {
  const {start,end}=parseOptions(argv);
  const selected=candidates.filter(round=>round.round>=start&&round.round<=end);
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
// Explicit OSS audit scope. Include bytes, not just source extensions: Rust can
// include arbitrary binary assets and Python/shell fixtures affect test behavior.
const AUDIT_INPUTS=[
  'src','tests','crates','examples','.wcode/design','docs/manual','plugin','.github/workflows',
  'Cargo.toml','Cargo.lock','marketplace.json','README.md','LICENSE','NOTICE','install.sh','install.ps1',
  '.gitattributes','.gitignore','.wcode/project.yaml','.wcode/executors.yaml','.wcode/architecture.toml',
  '.cargo/audit.toml',
];
const OPTIONAL_INPUTS=['.cargo/config','.cargo/config.toml','build.rs','rust-toolchain','rust-toolchain.toml'];
function digest(directory=root) {
  const hash=crypto.createHash('sha256').update('wcode-release-inputs-v2\0');
  const chunk=Buffer.alloc(64*1024);
  let count=0,entries=0,totalBytes=0;
  const same=(a,b)=>['dev','ino','size','mtimeNs','ctimeNs','mode','nlink'].every(key=>a[key]===b[key]);
  function ancestors(relative) {
    let parent=path.dirname(relative);
    while(parent!=='.') {
      const stat=fs.lstatSync(path.join(directory,parent));
      assert.ok(stat.isDirectory()&&!stat.isSymbolicLink(),'Audit input parent must not be a symlink: '+parent);
      parent=path.dirname(parent);
    }
  }
  function visit(relative,optional=false) {
    // Generated Python caches are not test inputs; scripts and all fixture bytes are.
    if(relative.startsWith('tests'+path.sep)&&relative.split(path.sep).includes('__pycache__')) return;
    assert.ok(++entries<=20000,'Audit input entry bound exceeded');
    ancestors(relative);
    const absolute=path.join(directory,relative);
    let stat;
    try { stat=fs.lstatSync(absolute,{bigint:true}); }
    catch(error) {
      if(optional&&error.code==='ENOENT') {hash.update('absent\0'+relative+'\0');return;}
      throw error;
    }
    assert.ok(!stat.isSymbolicLink(),'Audit inputs cannot be symlinks: '+relative);
    const name=relative.replaceAll(path.sep,'/');
    if(stat.isDirectory()) {
      hash.update('directory\0'+name+'\0');
      for(const child of fs.readdirSync(absolute).sort()) visit(path.join(relative,child));
      assert.ok(same(stat,fs.lstatSync(absolute,{bigint:true})),'Audit input directory changed while scanning: '+relative);
      return;
    }
    assert.ok(stat.isFile()&&stat.nlink===1n,'Audit input must be a regular single-link file: '+relative);
    assert.ok(++count<=10000,'Audit input file bound exceeded');
    assert.ok(stat.size<=8n*1024n*1024n,'Audit input byte bound exceeded');
    const fd=fs.openSync(absolute,fs.constants.O_RDONLY|(fs.constants.O_NOFOLLOW||0)|(fs.constants.O_NONBLOCK||0));
    const fileHash=crypto.createHash('sha256');
    let bytes=0;
    try {
      assert.ok(same(stat,fs.fstatSync(fd,{bigint:true})),'Audit input changed before reading: '+relative);
      for(;;) {
        const length=fs.readSync(fd,chunk,0,chunk.length,null);
        if(length===0) break;
        bytes+=length;totalBytes+=length;
        assert.ok(bytes<=8*1024*1024&&totalBytes<=256*1024*1024,'Audit input byte bound exceeded');
        fileHash.update(chunk.subarray(0,length));
      }
      ancestors(relative);
      assert.ok(BigInt(bytes)===stat.size&&same(stat,fs.fstatSync(fd,{bigint:true}))
        &&same(stat,fs.lstatSync(absolute,{bigint:true})),'Audit input changed while reading: '+relative);
    } finally {fs.closeSync(fd);}
    hash.update('file\0'+name+'\0'+String(stat.mode&0o777n)+'\0'+bytes+'\0'+fileHash.digest('hex')+'\0');
  }
  for(const relative of AUDIT_INPUTS) visit(relative);
  for(const relative of OPTIONAL_INPUTS) visit(relative,true);
  return {schema_version:2,scope:'oss-release-audit-v2',sha256:hash.digest('hex'),files:count,bytes:totalBytes};
}
function validateBrowserMatrix(results) {
  // Pin the complete reviewed matrix independently of the runner's own counters.
  const expected=new Set();
  const add=(widths,views)=>{
    for(const width of widths) for(const language of ['en','zh-CN']) for(const theme of ['dark','light']) for(const view of views) {
      expected.add(JSON.stringify([width,language,theme,view]));
    }
  };
  add([320,375,720,900,1024,1240,1280,1440,1461,1597,1676,1920],['proof','overview']);
  add([320,720,1024,1440],['codegraph','codegraph-full','codegraph-source','activity','changes','requirements','files','architecture-blueprint','architecture-components','architecture-dependencies']);
  assert.equal(results.length,expected.size,'Incomplete browser scenario matrix');
  for(const result of results) {
    const {width,language,theme,scenario}=result;
    assert.ok(expected.delete(JSON.stringify([width,language,theme,scenario])),
      'Duplicate or unexpected browser scenario');
    assert.equal(result.fontStatus,'loaded','Browser geometry requires completed font loading');
    const graph=scenario.startsWith('codegraph'), architecture=scenario.startsWith('architecture-');
    assert.equal(result.tab,graph||architecture?'architecture':scenario,'Requested tab was not observed');
    if(graph||architecture) {
      const view=graph?'codegraph':scenario==='architecture-components'?'components':scenario==='architecture-dependencies'?'graph':'blueprint';
      assert.equal(result.architectureView,view,'Requested architecture view was not observed');
    }
    if(graph) {
      assert.equal(result.codeGraphView,scenario==='codegraph'?'overview':'focus','Requested graph view was not observed');
      assert.equal(result.codeGraphFull,scenario==='codegraph-full','Requested fullscreen state was not observed');
      if(scenario==='codegraph-source') assert.equal(result.codeGraphInspectorOpen,true,'Source inspector was not observed');
    }
  }
  assert.equal(expected.size,0,'Browser matrix contains missing scenarios');
}
function validateLayoutMatrix(results) {
  const expected=new Set([375,720,900,1024,1240,1280,1440,1461,1500,1597,1676,1920]);
  assert.equal(results.length,expected.size,'Incomplete layout viewport matrix');
  for(const result of results) assert.ok(expected.delete(result.width),'Duplicate or unexpected layout viewport');
  assert.equal(expected.size,0,'Layout matrix contains missing viewports');
}
function casesFromOutput(step, stdout) {
  if(step.kind==='rust') {
    // The final libtest summary belongs to this invocation, not its nested fixtures.
    const summary=[...stdout.matchAll(/^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed;/gm)].at(-1);
    assert.ok(summary&&summary[1]==='ok'&&Number(summary[3])===0,'Missing or failed Rust test summary');
    const cases=Number(summary[2]);
    assert.ok(Number.isSafeInteger(cases)&&cases>0,'A zero-test Rust match is not verification');
    if(step.args?.includes('--exact')) assert.equal(cases,1,'Exact Rust selector must run one test');
    return cases;
  }
  const report=JSON.parse(stdout);
  assert.ok(report&&typeof report==='object'&&!Array.isArray(report),'Invalid JSON test report');
  if(step.expected_suite!==undefined) assert.equal(report.suite,step.expected_suite,'Unexpected test runner suite');
  if(report.passed!==undefined) assert.equal(report.passed,true,'Report explicitly failed');
  for(const key of ['failures','failed_cases']) if(report[key]!==undefined) assert.equal(report[key],0,key+' must be zero');
  if(report.runner_error!==undefined) assert.equal(report.runner_error,'','Runner did not complete');
  if(!Array.isArray(report.results)) {
    assert.equal(step.expected_cases,undefined,'Browser report is missing its result matrix');
    assert.equal(report.passed,true,'Fixture generation failed');
    assert.equal(report.results,undefined,'Malformed result matrix');
    for(const key of ['cases','expected_cases','total_cases']) assert.equal(report[key],undefined,'Fixture must not claim test cases');
    return 0;
  }
  const cases=report.results.length;
  assert.ok(cases>0,'Empty result set');
  for(const result of report.results) {
    assert.ok(result&&typeof result==='object'&&!Array.isArray(result),'Invalid scenario result');
    if(result.passed!==undefined) assert.equal(result.passed,true,'Scenario explicitly failed');
    if(result.errors!==undefined) assert.ok(Array.isArray(result.errors)&&result.errors.length===0,'Scenario errors');
    assert.ok(result.passed===true||Array.isArray(result.errors),'Missing scenario outcome');
  }
  for(const key of ['cases','expected_cases','total_cases']) {
    if(report[key]!==undefined) assert.equal(report[key],cases,'Incomplete or inconsistent '+key);
  }
  if(step.expected_cases!==undefined) assert.equal(cases,step.expected_cases,'Browser matrix coverage changed');
  if(report.suite==='full-browser-adversarial') {
    for(const key of ['cases','expected_cases','total_cases']) assert.equal(report[key],cases,'Missing browser coverage: '+key);
    assert.equal(report.runner_error,'','Missing browser completion');
    assert.equal(report.failed_cases,0,'Missing browser failure count');
    assert.equal(report.failures,0,'Missing browser failure total');
  }
  if(step.expected_suite==='full-browser-adversarial') validateBrowserMatrix(report.results);
  if(step.expected_suite==='webkit-layout') validateLayoutMatrix(report.results);
  return cases;
}
async function check(step) {
  const started=Date.now();
  const {stdout,stderr}=await exec(step.program,step.args,{cwd:root,timeout:120000,maxBuffer:2*1024*1024,windowsHide:true});
  const cases=casesFromOutput(step,stdout);
  return {command:[path.basename(step.program),...step.args],exit_code:0,elapsed_ms:Date.now()-started,cases,output_sha256:crypto.createHash('sha256').update(stdout).update(stderr).digest('hex')};
}
async function main() {
  const argv=process.argv.slice(2), options=parseOptions(argv);
  if(options.help) {
    console.log('Usage: node tests/release_audit.cjs [--rounds=START-END] [--require-clean]\nDefault: all 300 rounds on macOS. --help performs no audit or Cargo invocation.');
    return;
  }
  assert.equal(process.platform,'darwin','This complete audit requires macOS WebKit; other CI platforms run the portable cargo suite');
  const requireClean=options.require_clean, git_before=gitState();
  if(requireClean) assert.ok(git_before.clean,'Final release audit requires a clean worktree');
  // Bind source before discovery/compilation, not after an inventory may have gone stale.
  const before=digest(), started_at=new Date().toISOString(), results=[];
  const currentLibTests=listedLibTests(), allRounds=buildRounds(currentLibTests);
  const selected=selectedRounds(argv,allRounds);
  const plan_sha256=crypto.createHash('sha256').update(JSON.stringify(allRounds)).digest('hex');
  const inventory_sha256=crypto.createHash('sha256').update(JSON.stringify(currentLibTests)).digest('hex');
  // Rust compilation is serialized; the independent browser/JS lane runs concurrently.
  await Promise.all(['rust','web'].map(async lane=>{
    for(const round of selected.filter(r=>r.lane===lane)) {
      const result={round:round.round,name:round.name,passed:false,steps:[]};
      try {for(const step of round.steps) result.steps.push(await check(step));result.passed=true;}
      catch(error) {
        result.error=String(error.message);
        const stderr=String(error.stderr||'');
        const stdout=String(error.stdout||'');
        result.diagnostics=(stderr+'\n--- stdout ---\n'+stdout).slice(-12000);
      }
      results.push(result);
    }
  }));
  results.sort((a,b)=>a.round-b.round);
  const after=digest(), git_after=gitState();
  const stable=before.sha256===after.sha256
    && git_before.head===git_after.head
    && git_before.status_sha256===git_after.status_sha256;
  const start=selected[0].round, end=selected[selected.length-1].round;
  const complete=selected.length===allRounds.length;
  const report={
    suite:complete?'release-adversarial-300':'release-adversarial-shard',
    started_at,finished_at:new Date().toISOString(),
    git:{before:git_before,after:git_after,require_clean:requireClean},
    input:before,stable_inputs:stable,plan_sha256,inventory_sha256,
    rounds:results.length,total_rounds:allRounds.length,selected_rounds:{start,end},
    passed:results.every(r=>r.passed)&&stable,results
  };
  const target=path.join(root,'target');
  if(!fs.existsSync(target)) fs.mkdirSync(target);
  assert.ok(fs.lstatSync(target).isDirectory()&&!fs.lstatSync(target).isSymbolicLink());
  const filename=path.join(target,complete?'wcode-adversarial-300.json':`wcode-adversarial-${start}-${end}.json`);
  if(fs.existsSync(filename)) {const st=fs.lstatSync(filename);assert.ok(st.isFile()&&!st.isSymbolicLink()&&st.nlink===1);}
  fs.writeFileSync(filename,JSON.stringify(report,null,2)+'\n');
  console.log(JSON.stringify(report,null,2));
  assert.ok(report.passed,'Adversarial audit failed, Git state changed, or source changed during the run');
}
module.exports={digest,casesFromOutput,selectedRounds,buildRounds,parseOptions};
if(require.main===module) main().catch(error=>{console.error(error);process.exitCode=1;});