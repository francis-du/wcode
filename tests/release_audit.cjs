'use strict';
// Thirty distinct adversarial rounds, not thirty repetitions of a green suite.
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
  ['Host and Origin parsing rejects spoofing and duplicates','auth_origin::tests'],
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
  {round:29,name:'Complete production DOM: 96 width/language/theme/tab combinations',lane:'web',steps:[node('tests/unit/ui/browser.cjs'),swift('tests/unit/ui/browser_webkit.swift')]},
  {round:30,name:'Mutated release readiness and migration inspection must fail closed',lane:'rust',steps:[cargo('intelligence::release_gate::tests'),cargo('migration_audit::tests')]},
);
assert.equal(rounds.length,30);
assert.equal(new Set(rounds.map(r=>r.round)).size,30);

function selectedRounds() {
  const option=process.argv.find(value=>value.startsWith('--rounds='));
  if(!option) return rounds;
  const match=/^--rounds=(\d+)-(\d+)$/.exec(option);
  assert.ok(match,'--rounds must use START-END');
  const start=Number(match[1]), end=Number(match[2]);
  assert.ok(Number.isInteger(start)&&Number.isInteger(end)&&start>=1&&end<=30&&start<=end,'--rounds must stay within 1-30');
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
    suite:complete?'release-adversarial-30':'release-adversarial-shard',
    started_at,finished_at:new Date().toISOString(),
    git:{before:git_before,after:git_after,require_clean:requireClean},
    input:before,stable_inputs:stable,
    rounds:results.length,total_rounds:rounds.length,selected_rounds:{start,end},
    passed:results.every(r=>r.passed)&&stable,results
  };
  const target=path.join(root,'target');
  if(!fs.existsSync(target)) fs.mkdirSync(target);
  assert.ok(fs.lstatSync(target).isDirectory()&&!fs.lstatSync(target).isSymbolicLink());
  const filename=path.join(target,complete?'wcode-adversarial-30.json':`wcode-adversarial-${start}-${end}.json`);
  if(fs.existsSync(filename)) {const st=fs.lstatSync(filename);assert.ok(st.isFile()&&!st.isSymbolicLink()&&st.nlink===1);}
  fs.writeFileSync(filename,JSON.stringify(report,null,2)+'\n');
  console.log(JSON.stringify(report,null,2));
  assert.ok(report.passed,'Adversarial audit failed, Git state changed, or source changed during the run');
}
main().catch(error=>{console.error(error);process.exitCode=1;});
