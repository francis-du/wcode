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
test('change-scope-stays-unknown-without-a-retained-diff', async () => {
  const s=setup();
  s.run('state.changeInspection={path:"src/blob.bin",layer:"working",'
    + 'data:{kind:"binary",path:"src/blob.bin",snapshot_id:"' + 'a'.repeat(64) + '",content:""}};renderObjectInspector();');
  const html=s.node('#objectInspector').innerHTML;
  assert.ok(/Change/.test(html),'the selected change must still be identified');
  assert.ok(!html.includes('+0'),'a binary change cannot read as zero added lines');
  assert.ok(!html.includes('-0'),'a binary change cannot read as zero removed lines');
});
test('stale-graph-cannot-read-as-current', async () => {
  const s=setup();
  const node={id:'symbol:alpha',kind:'function',label:'alpha',attributes:{path:'src/a.rs',source_kind:'function'},provenance:{provider:'tree-sitter',precision:'syntax',revision:'rev-a'}};
  s.run('state.current="A";state.codeGraphView="focus";'
    + 'state.codeGraph={snapshot_id:"GRAPH-old",mode:"all",depth:1,root_ids:["symbol:alpha"],'
    + 'nodes:[{node:' + JSON.stringify(node) + ',distance:0}],edges:[],'
    + 'precision_counts:{},upstream_nodes:0,downstream_nodes:0,truncated:false};'
    + 'state.selectedCodeNode="symbol:alpha";'
    + 'state.project.history=[{id:"GRAPH-new",captured_at_ms:2},{id:"GRAPH-old",captured_at_ms:1}];renderCodeGraph();');
  const stale=s.node('#codeGraphSummary').innerHTML;
  assert.ok(/stale/i.test(stale),'a latest graph the newest capture replaced must say so');
  assert.ok(s.node('#codeGraphMap').innerHTML.includes('GRAPH-new'),
    'the stale reading must name the snapshot that is current now');
  s.run('state.codeGraphSnapshot="GRAPH-old";renderCodeGraph();');
  assert.ok(!/stale/i.test(s.node('#codeGraphSummary').innerHTML),
    'a deliberately pinned snapshot is a comparison, not a stale reading');
});
test('symbol-inspector-cannot-invent-provenance', async () => {
  const s=setup();
  s.run('state.selectedCodeNode="symbol:ghost";'
    + 'state.codeGraph={snapshot_id:"GRAPH-overview",nodes:[{node:{id:"symbol:ghost",label:"ghost",kind:"function",attributes:{}},distance:1}],edges:[]};'
    + 'renderObjectInspector();');
  const html=s.node('#objectInspector').innerHTML;
  assert.ok(/Symbol/.test(html),'the selected symbol must still be identified');
  assert.ok(!html.includes('undefined'),'missing provenance must stay unknown');
  assert.ok(!html.includes('NaN'),'missing provenance must stay unknown');
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
