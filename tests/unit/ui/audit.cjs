'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {sandbox, project, respond, flush} = require('./observatory.cjs');
const results = [];
async function test(round, name, fn) {
  try { await fn(); results.push({round,name,passed:true}); }
  catch (error) { results.push({round,name,passed:false,error:error.stack}); }
}
const seed = s => {s.context.fixture=project();s.run('state.project=fixture;');};
async function main() {
  await test(1,'adding an executable cannot overwrite another workspace',async()=>{
    const s=sandbox();s.node('#commandCandidate').value='cargo';
    const pending=s.run('addCommandFromUi()');await flush();
    assert.equal(s.requests[0].options.headers['X-Wcode-Workspace'],'A');
    s.run('state.current="B";clearWorkspaceView();');
    respond(s.requests[0],{allowed_commands:['private-A']});await pending;
    assert.equal(s.run('state.access'),null);
  });
  await test(2,'revoking an executable cannot overwrite another workspace',async()=>{
    const s=sandbox();const pending=s.run('revokeCommandFromUi("cargo")');await flush();
    s.run('state.current="B";clearWorkspaceView();');
    respond(s.requests[0],{allowed_commands:['private-A']});await pending;
    assert.equal(s.run('state.access'),null);
  });
  await test(3,'Enter repeats do not submit duplicate executable grants',async()=>{
    const s=sandbox();s.node('#commandCandidate').value='cargo';
    const a=s.run('addCommandFromUi()'),b=s.run('addCommandFromUi()');await flush();
    const count=s.requests.length;for(const r of s.requests)respond(r,{allowed_commands:['cargo']});
    await Promise.all([a,b]);assert.equal(count,1);
  });
  await test(4,'duplicate revocations are single flight',async()=>{
    const s=sandbox();const a=s.run('revokeCommandFromUi("cargo")'),b=s.run('revokeCommandFromUi("cargo")');
    await flush();const count=s.requests.length;for(const r of s.requests)respond(r,{allowed_commands:[]});
    await Promise.all([a,b]);assert.equal(count,1);
  });
  await test(5,'adding a workspace cannot redirect a later selection',async()=>{
    const s=sandbox();s.node('#workspacePath').value='/fixture/C';
    s.run('refreshProject=async ({workspace})=>{state.current=workspace;};loadAccess=async()=>{};');
    const pending=s.run('addWorkspaceFromUi()');await flush();s.run('state.current="B";');
    respond(s.requests[0],{workspace:{id:'C'}});await pending;assert.equal(s.run('state.current'),'B');
  });
  await test(6,'repeated workspace submission emits one write',async()=>{
    const s=sandbox();s.node('#workspacePath').value='/fixture/C';
    s.run('refreshProject=async()=>{};loadAccess=async()=>{};');
    const a=s.run('addWorkspaceFromUi()'),b=s.run('addWorkspaceFromUi()');await flush();const count=s.requests.length;
    for(const r of s.requests)respond(r,{workspace:{id:'C'}});await Promise.all([a,b]);assert.equal(count,1);
  });
  await test(7,'a pre-mutation access read cannot undo a completed write',async()=>{
    const s=sandbox();const read=s.run('loadAccess()');await flush();s.node('#commandCandidate').value='cargo';
    const write=s.run('addCommandFromUi()');await flush();
    respond(s.requests[3],{allowed_commands:['cargo']});await write;
    for(const r of s.requests.slice(0,3))respond(r,r.url.endsWith('authorizations')?{pending:[]}:{allowed_commands:[]});
    await read;assert.equal(s.run('state.access.allowed_commands.includes("cargo")'),true);
  });
  await test(8,'A to B to A does not revive an old mutation response',async()=>{
    const s=sandbox();s.node('#commandCandidate').value='cargo';const write=s.run('addCommandFromUi()');await flush();
    s.run('state.current="B";clearWorkspaceView();state.current="A";clearWorkspaceView();');
    respond(s.requests[0],{allowed_commands:['stale-A']});await write;assert.equal(s.run('state.access'),null);
  });
  await test(9,'a delayed activity response cannot undo an approval count',async()=>{
    const s=sandbox();seed(s);s.run('state.accessLoaded=true;');
    const activity=s.run('refreshActivity()');await flush();
    const approve=s.run('decideAuthorization("AUTH-test",true)');await flush();
    respond(s.requests[1],{pending:[],request:{status:'approved'}});await flush();respond(s.requests[2],{allowed_commands:[]});await approve;
    respond(s.requests[0],{workspace:'A',pending_authorizations:1,activity:{available:true,active:0,recent:[]}});await activity;
    assert.equal(s.run('pendingCount()'),0);
  });
  await test(10,'failed access loading remains unknown rather than no pending requests',async()=>{
    const s=sandbox();const read=s.run('loadAccess()');await flush();
    for(const r of s.requests)respond(r,{error:'unavailable'},false);await read;
    assert.ok(!s.node('#authorizationList').innerHTML.includes('No pending authorizations'));
  });
  await test(11,'unmeasured memory stays unknown while a measured zero stays zero',async()=>{
    const s=sandbox();s.context.fixture={workspace:'A',activity:{available:true,recent:[]},resources:{limits:{resident_memory_bytes:null}}};
    s.run('state.activitySnapshot=fixture;renderActivity();');assert.ok(!s.node('#resourceStatus').innerHTML.includes('>0 MiB<'));
    s.run('state.activitySnapshot.resources.limits.resident_memory_bytes=0;renderActivity();');assert.ok(s.node('#resourceStatus').innerHTML.includes('>0 MiB<'));
  });
  await test(12,'explicitly unavailable activity never looks idle',async()=>{
    const s=sandbox();s.context.fixture={activity:{available:false,recent:[]}};s.run('state.activitySnapshot=fixture;renderActivity();');
    assert.ok(s.node('#activity').innerHTML.includes('unavailable'));
  });
  await test(13,'highest-severity signals are visible before informational ones',async()=>{
    const s=sandbox();seed(s);s.run('state.project.risk={risks:[{level:"critical",summary:"critical risk"}]};');
    const signals=JSON.parse(s.run('JSON.stringify(attentionSignals())'));
    assert.equal(signals[0].tone,'bad');assert.ok(signals[0].detail.includes('critical'));
  });
  await test(14,'HTML error bodies preserve HTTP status for diagnosis',async()=>{
    const s=sandbox();const read=s.run('uiJson("/intelligence/project")');
    const rejected=assert.rejects(read,/HTTP 403/);await flush();
    s.requests[0].resolve({ok:false,status:403,json:async()=>{throw new SyntaxError('HTML body');}});await rejected;
  });
  await test(15,'activity schedules its next update even while the project is blocked',async()=>{
    const s=sandbox(false,true,{fakeTimers:true});seed(s);
    s.run('state.inFlight=true;state.activitySnapshot={activity:{available:true,active:1,recent:[]}};');
    const tick=s.run('activityTick()');await flush();
    respond(s.requests[0],{workspace:'A',pending_authorizations:0,activity:{available:true,active:2,recent:[]}});await tick;
    assert.equal(s.run('state.activitySnapshot.activity.active'),2);
    assert.ok([...s.timers.values()].some(t=>t.ms===2000));
  });
  await test(16,'pause and hidden tabs cancel schedules without replaying mutations',async()=>{
    const s=sandbox(false,true,{fakeTimers:true});s.run('schedule();');
    assert.ok(s.timers.size>=2);s.context.document.hidden=true;s.events.visibilitychange();
    assert.equal(s.timers.size,0);assert.equal(s.requests.length,0);
    s.context.document.hidden=false;s.run('state.autoRefresh=false;');s.events.visibilitychange();
    assert.equal(s.timers.size,0);assert.equal(s.requests.length,0);
  });
  await test(17,'the first screen requests live activity without waiting for the project',async()=>{
    const s=sandbox(false,true,{fakeTimers:true});s.run('startObservatory()');await flush();
    assert.ok(s.requests.some(r=>r.url==='/intelligence/activity'));
    assert.ok(s.requests.some(r=>r.url==='/intelligence/revision'));
    assert.equal(s.run('state.inFlight'),true);
  });
  await test(18,'overlapping activity ticks cannot create duplicate poll loops',async()=>{
    const s=sandbox(false,true,{fakeTimers:true});seed(s);
    const a=s.run('activityTick()'),b=s.run('activityTick()');await flush();assert.equal(s.requests.length,1);
    respond(s.requests[0],{workspace:'A',pending_authorizations:0,activity:{available:true,active:0,recent:[]}});
    await Promise.all([a,b]);assert.equal([...s.timers.values()].filter(t=>t.ms===8000).length,1);
  });
  await test(19,'effective verification results do not treat recovered history as active failure',async()=>{
    const s=sandbox();seed(s);
    s.run('state.project.proof={current_evidence:2,current_failed:1,effective:{total:1,failed:0,passed:1,inconclusive:0,disagreed:0,items:[{subject:"verification:rust-test",producer:"cargo test",result:"pass",timestamp_ms:1}],truncated:false}};renderStats();renderAttention();renderProofSummary();');
    assert.ok(!s.node('#attention').innerHTML.includes('Failure evidence recorded'));
    assert.ok(s.node('#proofSummary').innerHTML.includes('verification:rust-test'));
  });
  await test(20,'proof details escape source labels and expose truncated observations',async()=>{
    const s=sandbox();seed(s);
    s.run('state.project.proof.effective={total:2,failed:1,items:[{subject:"<script>bad</script>",producer:"<img>",result:"fail",timestamp_ms:1}],truncated:true};renderProofSummary();');
    assert.ok(s.node('#proofSummary').innerHTML.includes('&lt;script&gt;bad&lt;/script&gt;'));
    assert.ok(!s.node('#proofSummary').innerHTML.includes('<script>'));
    assert.ok(s.node('#proofSummary').innerHTML.includes('truncated'));
  });
  await test(21,'architecture system map keeps one readable subsystem inspector visible',async()=>{
    const s=sandbox();seed(s);
    s.run('globalThis.inspectorRenders=0;renderSubsystemInspector=()=>{inspectorRenders++;};state.architectureView="blueprint";renderArchitecture();');
    assert.ok(s.run('inspectorRenders')>=1);
  });
  await test(22,'revision outages keep the last snapshot without rebuilding the project every poll',async()=>{
    const s=sandbox();seed(s);s.run('globalThis.projectRefreshes=0;refreshProject=async()=>{projectRefreshes++;return true;};');
    const poll=s.run('pollRevision()');await flush();
    s.requests[0].reject(new Error('revision unavailable'));await poll;
    assert.equal(s.run('projectRefreshes'),0);assert.equal(s.run('state.syncError'),true);
  });
  const report={suite:'observatory-audit',results};
  fs.mkdirSync(path.join(process.argv[2],'target'),{recursive:true});
  fs.writeFileSync(path.join(process.argv[2],'target/wcode-audit.json'),JSON.stringify(report,null,2));
  console.log(JSON.stringify(report,null,2));
  assert.equal(results.length,22);assert.ok(results.every(item=>item.passed),'audit scenarios failed');
}
main().catch(error=>{console.error(error);process.exitCode=1;});
