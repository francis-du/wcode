'use strict';
// Exercise rendered controls from the shipped bundle, including already queued clicks.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {sandbox, respond, flush} = require('./observatory.cjs');
const {fixture} = require('./acceptance.cjs');
const {task, report} = require('./verification.cjs');
class Control {
  constructor(source) {
    this.dataset = {}; this.attrs = {}; this.listeners = new Map(); this.value = '';
    for (const match of source.matchAll(/([\w-]+)(?:="([^"]*)")?/g)) {
      const [, key, value = ''] = match; this.attrs[key] = value;
      if (key.startsWith('data-')) this.dataset[key.slice(5).replace(/-([a-z])/g, (_, x) => x.toUpperCase())] = value;
      if (key === 'value') this.value = value;
    }
    this.disabled = Object.hasOwn(this.attrs, 'disabled');
  }
  addEventListener(name, fn) { this.listeners.set(name, [...(this.listeners.get(name) || []), fn]); }
  removeEventListener(name, fn) { this.listeners.set(name, (this.listeners.get(name) || []).filter(item => item !== fn)); }
  callbacks() { return [...(this.listeners.get('click') || [])]; }
  click() { if (!this.disabled) this.callbacks().forEach(fn => fn()); }
}
class ControlsHost {
  constructor() { this.html = ''; this.controls = []; this.writes = 0; }
  get innerHTML() { return this.html; }
  set innerHTML(value) {
    this.html = value; this.writes++;
    this.controls = [...value.matchAll(/<(?:button|input)\b([^>]*)>/g)].map(match => new Control(match[1]));
  }
  querySelectorAll(selector) {
    const match = /^\[([^=\]]+)(?:="([^"]*)")?\]$/.exec(selector);
    return match ? this.controls.filter(button => Object.hasOwn(button.attrs, match[1])
      && (match[2] === undefined || button.attrs[match[1]] === match[2])) : [];
  }
  querySelector(selector) { return this.querySelectorAll(selector)[0] || null; }
}
function project(workspace = 'A') {
  const value = fixture(); value.workspace = workspace; value.acceptance.workspace = workspace;
  value.snapshot_revision = 'rendered-control-snapshot'; return value;
}
function prepared(value) {
  const s = sandbox(false, true, {fakeTimers:true}), host = new ControlsHost();
  s.nodes.set('#verificationTask', host); s.context.controlsHost = host; s.context.controlProject = project();
  s.run('els.verificationTask=controlsHost;state.project=controlProject;state.workspaceTab="proof";state.autoRefresh=false;');
  s.host = host;
  if (value) attach(s, value); else s.run('renderVerificationTask();');
  return s;
}
function attach(s, receipt = task()) {
  s.context.controlReceipt = receipt;
  s.run('controlEntry=verificationEntry(true);controlEntry.task=controlReceipt;controlEntry.expectedRevision=controlReceipt.requested_revision;renderVerificationTask();');
}
function callback(s, selector) {
  const button = s.host.querySelector(selector); assert.ok(button, selector);
  const callbacks = button.callbacks(); assert.equal(callbacks.length, 1, selector + ' must have exactly one listener');
  return callbacks[0];
}
function switchWorkspace(s, workspace = 'B', receipt = task({workspace:'B',task_id:'task-in-B'})) {
  s.context.nextProject = project(workspace); s.context.nextTask = receipt;
  s.run('state.current=nextProject.workspace;clearWorkspaceView();state.project=nextProject;state.workspaceTab="proof";');
  if (receipt) attach(s, receipt); else s.run('renderVerificationTask();');
}
async function staleCancelOracle(override) {
  const s = prepared(task()); if (override) { s.run(override); s.run('renderVerificationTask();'); }
  const old = callback(s, '[data-verification-cancel]');
  switchWorkspace(s); old(); await flush();
  assert.equal(s.requests.length, 0, 'QUEUED_CANCEL_MUST_NOT_RETARGET_CURRENT_WORKSPACE');
  assert.equal(s.run('verificationEntry().task.task_id'), 'task-in-B');
  return s;
}

test('a queued Cancel cannot retarget another workspace or a replacement task in the same entry', async () => {
  await staleCancelOracle();
  const s = prepared(task()), old = callback(s, '[data-verification-cancel]');
  attach(s, task({task_id:'replacement-task'})); old(); await flush();
  assert.equal(s.requests.length, 0);
  assert.equal(s.run('verificationEntry().task.task_id'), 'replacement-task');
});
test('queued Run cannot cross workspace epoch entry owner or page boundaries', async () => {
  for (const change of [
    s => switchWorkspace(s, 'B', null),
    s => s.run('clearWorkspaceView();state.project=controlProject;renderVerificationTask();'),
    s => s.run('verificationEntry(true);renderVerificationTask();'),
    s => s.run('verificationEntry(true).owner="expired-session";renderVerificationTask();'),
    s => s.run('state.workspaceTab="activity";'),
  ]) {
    const s = prepared(), old = callback(s, '[data-verification-run="full"]');
    change(s); old(); await flush(); assert.equal(s.requests.length, 0);
  }
});
test('queued status result refresh discovery reconnect evidence and approval controls cannot leave their rendered scope', async () => {
  const terminal = task({status:'completed',terminal:true,result_available:true,
    report:report(),authorization_required:true,next_action:'inspect_result_and_refresh_project'});
  for (const selector of ['[data-verification-status]','[data-verification-result]','[data-verification-refresh]',
    '[data-verification-discover]','[data-verification-discovered]','[data-acceptance-evidence]','[data-summary-action]']) {
    const s = prepared(terminal);
    s.run('controlEntry.result=controlReceipt;controlEntry.discovered=[{task_id:"retained-A",status:"working"}];controlEntry.discoveryLoaded=true;renderVerificationTask();');
    const old = callback(s, selector); switchWorkspace(s);
    s.context.actionCalls = []; s.run('observeVerificationTask=(...args)=>actionCalls.push(["observe",args]);refreshVerificationAcceptance=(...args)=>actionCalls.push(["refresh",args]);discoverVerificationTasks=()=>actionCalls.push(["discover"]);openAcceptanceEvidence=(...args)=>actionCalls.push(["evidence",args]);setAccessPanel=(...args)=>actionCalls.push(["access",args]);loadAccess=()=>actionCalls.push(["loadAccess"]);');
    old(); await flush(); assert.equal(s.context.actionCalls.length, 0, selector);
  }
  const reconnect = prepared(), old = callback(reconnect, '[data-verification-reconnect]');
  reconnect.host.querySelector('[data-verification-reconnect-id]').value = 'retained-A';
  switchWorkspace(reconnect, 'B', null);
  reconnect.host.querySelector('[data-verification-reconnect-id]').value = 'retained-B';
  old(); await flush(); assert.equal(reconnect.requests.length, 0);
});
test('identical markup rebinds a replacement entry without leaving current buttons inert', async () => {
  const s = prepared(task()), old = callback(s, '[data-verification-cancel]');
  const button = s.host.querySelector('[data-verification-cancel]'), writes = s.host.writes;
  s.run('state.verificationTasks.delete(state.current);'); attach(s, task());
  assert.equal(s.host.writes, writes, 'unchanged markup must preserve DOM');
  assert.equal(s.host.querySelector('[data-verification-cancel]'), button);
  old(); await flush(); assert.equal(s.requests.length, 0, 'old entry listener must stay inert');
  button.click(); await flush(); assert.equal(s.requests.length, 1);
  assert.equal(s.requests[0].url, '/intelligence/verification/task-native-1/cancel');
  respond(s.requests[0], task({status:'cancelled',terminal:true,next_action:'refresh_project'})); await flush();
});
test('same-task observation generations rebind one usable listener without freezing the request sequence', async () => {
  const s = prepared(task()), old = callback(s, '[data-verification-status]');
  const button = s.host.querySelector('[data-verification-status]'), writes = s.host.writes;
  s.run('controlEntry.sequence++;renderVerificationTask();renderVerificationTask();');
  assert.equal(s.host.writes, writes); assert.equal(button.callbacks().length, 1);
  old(); await flush(); assert.equal(s.requests.length, 0, 'superseded render callback must not act');
  button.click(); await flush(); assert.equal(s.requests.length, 1);
  assert.equal(s.requests[0].url, '/intelligence/verification/task-native-1');
  assert.equal(s.requests[0].options.method, 'GET');
  respond(s.requests[0], task()); await flush(); assert.equal(s.run('verificationEntry().busy'), false);
});
test('first rendered Quick and Full run normally without an entry and coalesce repeated clicks', async () => {
  for (const level of ['quick','full']) {
    const s = prepared(), click = callback(s, '[data-verification-run="' + level + '"]');
    assert.equal(s.run('verificationEntry()'), undefined); click(); click(); await flush();
    assert.equal(s.requests.length, 1);
    assert.equal(s.requests[0].url, '/intelligence/verification/run');
    assert.equal(JSON.parse(s.requests[0].options.body).level, level);
    respond(s.requests[0], task()); await flush();
    assert.equal(s.run('verificationEntry().task.task_id'), 'task-native-1');
    assert.equal(s.run('acceptanceView().status'), 'blocked');
  }
});
test('standalone recovery binds its actual caller surface and rejects saved selections after a scope switch', async () => {
  const s = prepared(task()); s.context.recoveryHost = new ControlsHost();
  s.run('state.workspaceTab="activity";controlEntry.discovered=[{task_id:"retained-A",status:"working"}];recoveryHost.innerHTML=verificationRecoveryHtml(controlEntry);bindVerificationRecovery(recoveryHost,controlEntry);');
  const button = s.context.recoveryHost.querySelector('[data-verification-discovered]'), old = button.callbacks()[0];
  button.click(); await flush(); assert.equal(s.requests.length, 1);
  assert.equal(s.requests[0].url, '/intelligence/verification/retained-A');
  respond(s.requests[0], task({task_id:'retained-A'})); await flush();
  switchWorkspace(s); old(); await flush(); assert.equal(s.requests.length, 1);
});
test('hidden or inactive Proof controls pause clicks and the returned current view remains usable', async () => {
  for (const pause of ['document.hidden=true','state.workspaceTab="activity"']) {
    const s = prepared(task()), old = callback(s, '[data-verification-cancel]');
    s.run(pause); old(); await flush(); assert.equal(s.requests.length, 0);
    s.run('document.hidden=false;state.workspaceTab="proof";renderVerificationTask();');
    s.host.querySelector('[data-verification-cancel]').click(); await flush();
    assert.equal(s.requests.length, 1); assert.equal(s.requests[0].url, '/intelligence/verification/task-native-1/cancel');
    respond(s.requests[0], task({status:'input_required',authorization_required:true})); await flush();
  }
});
test('current read and recovery controls retain their rendered intent without granting Acceptance', async () => {
  const terminal = task({status:'completed',terminal:true,result_available:true,report:report(),
    authorization_required:true,next_action:'inspect_result_and_refresh_project'});
  const expectations = [
    ['[data-verification-result]', {action:'observe',args:{result:true,manual:true}}],
    ['[data-verification-refresh]', {action:'refresh',same:true}],
    ['[data-verification-discover]', {action:'discover'}],
    ['[data-acceptance-evidence]', {action:'evidence',id:'EV-native'}],
    ['[data-summary-action]', {action:'access',open:true}],
  ];
  for (const [selector, expected] of expectations) {
    const s = prepared(terminal); s.run('controlEntry.result=controlReceipt;renderVerificationTask();');
    s.context.actionCalls = [];
    s.run('observeVerificationTask=args=>actionCalls.push({action:"observe",args});refreshVerificationAcceptance=value=>actionCalls.push({action:"refresh",same:value===controlEntry});discoverVerificationTasks=()=>actionCalls.push({action:"discover"});openAcceptanceEvidence=id=>actionCalls.push({action:"evidence",id});setAccessPanel=open=>actionCalls.push({action:"access",open});loadAccess=async()=>{};');
    callback(s, selector)(); await flush();
    assert.deepEqual(JSON.parse(JSON.stringify(s.context.actionCalls)), [expected], selector);
    assert.equal(s.run('acceptanceView().status'), 'blocked'); assert.equal(s.requests.length, 0);
  }
  const reconnect = prepared(); reconnect.host.querySelector('[data-verification-reconnect-id]').value = '  retained-A  ';
  callback(reconnect, '[data-verification-reconnect]')(); await flush();
  assert.equal(reconnect.requests.length, 1); assert.equal(reconnect.requests[0].url, '/intelligence/verification/retained-A');
  respond(reconnect.requests[0], task({task_id:'retained-A'})); await flush();
  assert.equal(reconnect.run('verificationEntry().task.task_id'), 'retained-A');
});
test('LF and CRLF independent render-binding ablation exposes the original wrong-workspace Cancel', async () => {
  const fs = require('node:fs'), path = require('node:path');
  const source = fs.readFileSync(path.resolve(__dirname, '../../../src/ui/intelligence_web/app/verification.js'), 'utf8');
  for (const ending of ['\n','\r\n']) {
    const text = source.replace(/\r\n?/g, '\n').replace(/\n/g, ending);
    const start = text.indexOf('function bindVerificationControls('), end = text.indexOf('function renderVerificationTask()', start);
    assert.ok(start >= 0 && end > start, 'the shipped binding must be found');
    const body = text.slice(start, end), guard = 'if (current()) void action();';
    assert.equal(body.split(guard).length, 2, 'ablate exactly one consumed binding decision');
    await staleCancelOracle(body);
    await assert.rejects(staleCancelOracle(body.replace(guard, 'void action();')),
      /QUEUED_CANCEL_MUST_NOT_RETARGET_CURRENT_WORKSPACE/);
  }
});
