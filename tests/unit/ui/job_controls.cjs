'use strict';
// Execute shipped Jobs code and its bound callbacks. DOM, timers and HTTP are
// deterministic substitutes; this is not a browser or native backend test.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.resolve(__dirname, '../../../src/ui/intelligence_web/app/jobs.js'), 'utf8');
const A = 'TASK-' + '1'.repeat(20) + '-' + 'a'.repeat(32);
const B = 'TASK-' + '2'.repeat(20) + '-' + 'b'.repeat(32);
const flush = () => new Promise(resolve => setImmediate(resolve));
function job(id, workspace = 'A', patch = {}) {
  const stream = () => ({text:'output',total_bytes:6,truncated:false,redacted:false});
  return {schema_version:1,kind:'command_job',workspace,task_id:id,tool:'run_command',origin:'ui',status:'working',can_cancel:true,
    success:null,exit_code:null,error:null,completion_is_not_success:true,acceptance_ready:false,stdout:stream(),stderr:stream(),...patch};
}
function fixture(code = source) {
  const hosts = new Map(), html = new Map(), requests = [], timers = new Map(); let timerId = 0;
  class Element {
    constructor(attributes = '') {
      this.handlers = {}; this.dataset = {}; this.value = ''; this.attributes = attributes;
      for (const match of attributes.matchAll(/data-([a-z-]+)(?:="([^"]*)")?/g)) {
        this.dataset[match[1].replace(/-([a-z])/g, (_, c) => c.toUpperCase())] = match[2] || '';
      }
    }
    addEventListener(type, handler) { this.handlers[type] = handler; }
    click() { return this.handlers.click?.(); }
    focus() { context.document.activeElement = this; }
    set innerHTML(value) {
      this.markup = value;
      this.children = Array.from(value.matchAll(/<(?:button|select)\b([^>]*)>/g), match => new Element(match[1]));
    }
    get innerHTML() { return this.markup || ''; }
    querySelectorAll(selector) {
      const attribute = selector.slice(1,-1);
      return (this.children || []).filter(child => new RegExp('(?:^|\\s)' + attribute + '(?:=|\\s|$)').test(child.attributes));
    }
    querySelector(selector) { return this.querySelectorAll(selector)[0] || null; }
  }
  const host = id => { if (!hosts.has(id)) hosts.set(id, new Element()); return hosts.get(id); };
  const state = {current:'A',workspaceEpoch:1,workspaceTab:'activity',runtimeJobsSequence:0};
  const context = vm.createContext({state,token:'session-A',AbortController,
    document:{hidden:false,getElementById:host},
    localized:(english)=>english,esc:value=>String(value??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#039;'}[c])),
    num:value=>value,statusLabel:value=>value,
    invalidate:key=>html.delete(key),
    setHtml:(key,node,markup,bind)=>{if(html.get(key)===markup)return;html.set(key,markup);node.innerHTML=markup;bind?.();},
    setTimeout:fn=>{const id=++timerId;timers.set(id,fn);return id;},clearTimeout:id=>timers.delete(id),
    observationStamp:()=>({workspace:state.current,owner:context.token,epoch:state.workspaceEpoch}),
    observationCurrent:stamp=>stamp.workspace===state.current&&stamp.owner===context.token&&stamp.epoch===state.workspaceEpoch,
    uiJson:async(url,method,body,options)=>{
      requests.push({url,method,body,workspace:options.workspace});
      const id=url.split('/')[3];
      if (url.endsWith('/cancel')) return job(id, options.workspace, {status:'cancelled',can_cancel:false,success:false});
      if (id) return job(id, options.workspace);
      return {schema_version:1,kind:'command_job_list',workspace:options.workspace,items:[],truncated:false,coverage:'retained_bounded',discovery_is_not_execution:true};
    }});
  const run = expression => vm.runInContext(expression, context);
  vm.runInContext(code, context); run('clearRuntimeJobs();');
  function select(id = A, patch = {}) {
    state.runtimeJobs = [A,B].map(task_id=>({task_id,workspace:state.current,origin:'ui',status:'working',can_cancel:true}));
    state.runtimeJobsLoaded = true;state.runtimeJobSelected=id;state.runtimeJob=job(id,state.current,patch);run('renderRuntimeJobs();');
  }
  function replaceScope(owner = 'session-B', workspace = 'B') {
    context.token=owner;state.current=workspace;state.workspaceEpoch++;run('clearRuntimeJobs();');
  }
  const control = (area,selector) => {const node=host(area).querySelector(selector);assert.ok(node,selector+' must exist');return node;};
  select();return {state,context,requests,timers,host,run,select,replaceScope,control};
}
async function staleCancelOracle(code) {
  const f=fixture(code),old=f.control('runtimeJobDetail','[data-job-cancel]');
  f.select(B);old.click();await flush();
  assert.equal(f.requests.length,0,'STALE_CANCEL_MUST_NOT_TARGET_REPLACEMENT: ' + JSON.stringify(f.requests));
}
test('a prior task cancellation control cannot cancel the newly selected task',async()=>staleCancelOracle(source));
test('a prior exact-status control cannot refresh the newly selected task',async()=>{
  const f=fixture(),old=f.control('runtimeJobDetail','[data-job-status]');f.select(B);old.click();await flush();assert.equal(f.requests.length,0);
});
test('a detached discovery control cannot read under a replacement session',async()=>{
  const f=fixture(),old=f.control('runtimeJobForm','[data-jobs-refresh]');f.replaceScope();f.select();old.click();await flush();assert.equal(f.requests.length,0);
});
test('a detached retained-row control cannot select a task in another scope',async()=>{
  const f=fixture(),old=f.control('runtimeJobList','[data-job-id]');f.replaceScope();f.select();old.click();await flush();assert.equal(f.requests.length,0);
});
test('a prior cancellation control stays inert after A to B to A scope replacement',async()=>{
  const f=fixture(),old=f.control('runtimeJobDetail','[data-job-cancel]');f.replaceScope();f.replaceScope('session-A','A');f.select();old.click();await flush();assert.equal(f.requests.length,0);
});
test('current cancellation remains explicit, exact-ID and coalesces rapid clicks',async()=>{
  const f=fixture(),button=f.control('runtimeJobDetail','[data-job-cancel]');button.click();button.click();await flush();
  assert.equal(f.requests.length,1);assert.equal(f.requests[0].url,'/intelligence/jobs/'+A+'/cancel');
  assert.equal(f.requests[0].method,'POST');assert.deepEqual(JSON.parse(JSON.stringify(f.requests[0].body)),{});
  assert.equal(f.state.runtimeJob.status,'cancelled');
});
test('unchanged markup after an owner change binds fresh usable detail and list controls',async()=>{
  const f=fixture();f.replaceScope('session-B','A');f.select();
  f.control('runtimeJobList','[data-job-id]').click();await flush();assert.equal(f.requests.length,1);
  f.control('runtimeJobDetail','[data-job-cancel]').click();await flush();assert.equal(f.requests.length,2);assert.equal(f.requests[1].method,'POST');
});
test('current detail refresh remains a GET for the selected task',async()=>{
  const f=fixture();f.select(B);f.control('runtimeJobDetail','[data-job-status]').click();await flush();
  assert.equal(f.requests.length,1);assert.equal(f.requests[0].method,'GET');assert.equal(f.requests[0].url,'/intelligence/jobs/'+B);
});
test('a queued old poll cannot read a replacement selection or scope',async()=>{
  for(const replace of [f=>f.select(B),f=>{f.replaceScope();f.select();}]){
    const f=fixture();f.run('scheduleRuntimeJobObservation();');const poll=[...f.timers.values()][0];assert.ok(poll);replace(f);poll();await flush();assert.equal(f.requests.length,0);
  }
});
test('a queued poll rechecks page visibility and the observation budget',async()=>{
  for(const change of [f=>{f.context.document.hidden=true;},f=>{f.state.workspaceTab='overview';},f=>{f.state.runtimeJobsPolls=120;}]){
    const f=fixture();f.run('scheduleRuntimeJobObservation();');const poll=[...f.timers.values()][0];change(f);poll();await flush();assert.equal(f.requests.length,0);
  }
});
test('a queued poll cannot race a newer completed manual observation',async()=>{
  const f=fixture();f.run('scheduleRuntimeJobObservation();');const poll=[...f.timers.values()][0];
  await f.run('observeRuntimeJob();');assert.equal(f.requests.length,1);poll();await flush();assert.equal(f.requests.length,1);
});
test('a current scheduled observation remains GET-only and bounded',async()=>{
  const f=fixture();f.run('scheduleRuntimeJobObservation();');const poll=[...f.timers.values()][0];poll();await flush();
  assert.equal(f.requests.length,1);assert.equal(f.requests[0].method,'GET');assert.equal(f.requests[0].url,'/intelligence/jobs/'+A);assert.equal(f.state.runtimeJobsPolls,1);
});
test('MCP jobs and uncertain cancellation never gain a cancellation request from stale controls',async()=>{
  for(const change of [f=>f.select(A,{origin:'mcp',can_cancel:false}),f=>{f.state.runtimeJobsUncertain=true;}]){
    const f=fixture(),old=f.control('runtimeJobDetail','[data-job-cancel]');change(f);old.click();await flush();assert.equal(f.requests.length,0);
  }
});

test('navigation away from Activity invalidates its retained controls without a request',async()=>{
  for(const [area,selector] of [['runtimeJobForm','[data-jobs-refresh]'],['runtimeJobList','[data-job-id]'],['runtimeJobDetail','[data-job-status]'],['runtimeJobDetail','[data-job-cancel]']]){
    const f=fixture(),old=f.control(area,selector);f.state.workspaceTab='overview';old.click();await flush();assert.equal(f.requests.length,0);
  }
});
test('ablation of only selected-task binding reproduces the wrong-target cancellation',async()=>{
  const mutant=source.replace('currentScope() && state.runtimeJobSelected === selected','currentScope()');
  assert.notEqual(mutant,source);await staleCancelOracle(source);
  await assert.rejects(staleCancelOracle(mutant),error=>error.code==='ERR_ASSERTION'&&/STALE_CANCEL_MUST_NOT_TARGET_REPLACEMENT/.test(error.message));
});
test('ablation of only callback scope binding admits an obsolete cancellation control',async()=>{
  const mutant=source.replace('runtimeJobsScope() === scope && state.workspaceTab === "activity"','state.workspaceTab === "activity"');assert.notEqual(mutant,source);
  async function oracle(code){
    const f=fixture(code),old=f.control('runtimeJobDetail','[data-job-cancel]');f.replaceScope();f.select();old.click();await flush();
    assert.equal(f.requests.length,0,'OBSOLETE_SCOPE_MUST_NOT_CANCEL');
  }
  await oracle(source);await assert.rejects(oracle(mutant),error=>error.code==='ERR_ASSERTION'&&/OBSOLETE_SCOPE_MUST_NOT_CANCEL/.test(error.message));
});
test('ablation of only poll sequence binding admits an already superseded observation',async()=>{
  const mutant=source.replace(' || state.runtimeJobsSequence !== sequence','');assert.notEqual(mutant,source);
  async function oracle(code){
    const f=fixture(code);f.run('scheduleRuntimeJobObservation();');const poll=[...f.timers.values()][0];
    await f.run('observeRuntimeJob();');poll();await flush();assert.equal(f.requests.length,1,'SUPERSEDED_POLL_MUST_NOT_RUN');
  }
  await oracle(source);await assert.rejects(oracle(mutant),error=>error.code==='ERR_ASSERTION'&&/SUPERSEDED_POLL_MUST_NOT_RUN/.test(error.message));
});
