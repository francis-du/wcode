'use strict';
const assert = require('node:assert/strict');
const {sandbox, project, respond, flush} = require('./observatory.cjs');
const cases = new Map();
const test = (name, fn) => cases.set(name, fn);
function setup() {
  const s = sandbox(false, true, {fakeTimers:true});
  s.context.console = {...console, warn(){}};
  s.context.fixture = project();
  s.run('state.project=fixture;');
  return s;
}
test('cached-render-failure', async () => {
  const s=setup();
  s.run('cacheWorkspaceSnapshot();state.current="B";state.project=null;renderProject=()=>{throw new Error("private render");};');
  const pending=s.run('refreshProject({workspace:"A",reason:"manual"})').then(value=>({value}),error=>({error}));
  await flush();
  // A bad cached render must not prevent the authoritative network retry.
  const request=s.requests.find(r=>r.url==='/intelligence/project');
  if(request) respond(request,project());
  const outcome=await pending;
  if(outcome.error) throw outcome.error;
  assert.equal(outcome.value,false);
  assert.ok(s.requests.length>0);
  assert.equal(s.run('state.inFlight'),false);
});
test('activity-render-failure', async () => {
  const s=setup();
  s.run('renderActivity=()=>{throw new Error("private activity render");};');
  const pending=s.run('refreshActivity()'); await flush();
  respond(s.requests[0],{workspace:'A',activity:{available:true,recent:[]}});
  await pending;
  assert.equal(s.run('state.activityError'),true);
  assert.equal(s.run('state.activityController'),null);
});
test('effective-zero-beats-history', async () => {
  const s=setup();
  s.run('state.project.proof={current_passed:99,effective:{total:1,passed:0,failed:1,items:[{id:"e1",subject:"failed check",result:"fail",timestamp_ms:1} ]}};renderProofSummary();');
  const html=s.node('#proofSummary').innerHTML;
  assert.ok(!html.includes('<strong>99</strong>'),'historical pass count must not replace effective zero');
});
test('failed-evidence-is-not-green', async () => {
  const s=setup();
  s.run('state.project.proof={effective:{total:1,passed:0,failed:1,items:[{id:"e1",subject:"failed check",result:"fail",timestamp_ms:1}]}};renderProofSummary();');
  assert.ok(!/inspector-chip good[^>]*>[^<]*(fail|失败)/i.test(s.node('#proofSummary').innerHTML));
});
test('activity-old-error-cannot-poison-new-workspace', async () => {
  const s=setup();
  const old=s.run('refreshActivity()'); await flush();
  s.run('state.current="B";clearWorkspaceView();');
  s.requests[0].reject(new Error('old failure')); await old;
  assert.equal(s.run('state.activityError'),false);
});
test('proof-details-are-bounded-and-escaped', async () => {
  const s=setup();
  s.context.items=Array.from({length:100},(_,i)=>({id:'e'+i,subject:'<img onerror=alert(1)>',summary:'<script>bad</script>',result:'fail',producer:'<b>bad</b>',timestamp_ms:i+1}));
  s.run('state.project.proof={effective:{total:100,failed:100,passed:0,items,truncated:true}};renderProofSummary();');
  const html=s.node('#proofSummary').innerHTML;
  assert.equal((html.match(/data-evidence-key=/g)||[]).length,32);
  assert.ok(!html.includes('<script>')&&!html.includes('<img onerror'));
  assert.ok(html.includes('Details truncated'));
});
async function main() {
  const only=process.argv[3], results=[];
  if(only&&!cases.has(only)) throw new Error('Unknown adversarial scenario: '+only);
  for(const [name,fn] of cases) {
    if(only&&name!==only) continue;
    try {await fn();results.push({name,passed:true});}
    catch(error) {results.push({name,passed:false,error:error.stack});}
  }
  assert.ok(results.length>0);
  console.log(JSON.stringify({suite:'observatory-adversarial',results},null,2));
  assert.ok(results.every(r=>r.passed),'adversarial scenarios failed');
}
main().catch(error=>{console.error(error);process.exitCode=1;});
