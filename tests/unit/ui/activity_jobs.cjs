'use strict';
// Production renderer, activity bridge and Jobs request path; DOM/HTTP are
// deterministic substitutes. Native projection coverage lives in mcp/http.rs.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { sandbox, project, respond, flush } = require('./observatory.cjs');
const A = 'TASK-' + '1'.repeat(20) + '-' + 'a'.repeat(32);
const B = 'TASK-' + '2'.repeat(20) + '-' + 'b'.repeat(32);
const task = (patch = {}) => ({ id: 7, tool: 'run_command', command_job_id: A,
  status: 'running', slot_counted: true, wait_ms: 10, run_ms: 50, ...patch });
function detail(id = A, workspace = 'A', patch = {}) {
  const stream = text => ({ text, total_bytes: Buffer.byteLength(text), truncated: false, redacted: false });
  return { schema_version: 1, kind: 'command_job', task_id: id, workspace, tool: 'run_command',
    origin: 'mcp', status: 'completed', can_cancel: false, success: false, exit_code: 1, error: null,
    stdout: stream('retained output'), stderr: stream('actual failure'),
    completion_is_not_success: true, acceptance_ready: false, ...patch };
}
function fixture(rows = [task()]) {
  const s = sandbox(false, true, { fakeTimers: true });
  s.context.document.body = {};
  s.context.document.activeElement = s.context.document.body;
  let html = '', buttons = [];
  const host = s.node('#activity');
  Object.defineProperty(host, 'innerHTML', {
    get: () => html,
    set: value => {
      if (buttons.includes(s.context.document.activeElement)) s.context.document.activeElement = s.context.document.body;
      html = value;
      buttons = Array.from(value.matchAll(/<button\b[^>]*data-activity-job="([^"]+)"[^>]*data-activity-task="([^"]+)"[^>]*>/g), match => ({
        dataset: { activityJob: match[1], activityTask: match[2] }, onclick: null,
        click() { return this.onclick?.(); },
        focus(options) { this.focusOptions = options; s.context.document.activeElement = this; },
      }));
    },
  });
  host.querySelectorAll = selector => selector === '[data-activity-job]' ? buttons : [];
  s.context.fixture = project('A');
  s.context.observed = { workspace: 'A', activity: { workspace: 'A', available: true,
    active: 1, queued: 0, completed: 2, failed: 1, recent: rows, recent_truncated: true } };
  s.run('state.workspaceTab="activity";state.project=fixture;state.activitySnapshot=observed;state.activityUpdated=Date.now();renderActivity();');
  return { ...s, get buttons() { return buttons; }, get html() { return html; },
    render() { s.run('renderActivity();'); },
    selection(index = 0) {
      return s.run(`activityJobSelection(state.activitySnapshot.activity.recent[${index}],state.activityJobsRender)`);
    },
  };
}
async function settle(f, value = detail()) {
  await flush();
  assert.equal(f.requests.length, 1, 'one exact observation request is expected');
  respond(f.requests[0], value);
  await flush();
}
test('live task output navigation reads its exact bound job without discovery or execution', async () => {
  const f = fixture();
  assert.equal(f.buttons.length, 1);
  assert.match(f.html, /Inspect output/);
  assert.match(f.html, /truncated/);
  assert.match(f.html, /historical totals, not current blockers/);
  assert.equal(f.requests.length, 0);
  f.buttons[0].click();
  await settle(f);
  assert.equal(f.requests[0].url, '/intelligence/jobs/' + A);
  assert.equal(f.requests[0].options.method, 'GET');
  assert.equal(f.requests[0].options.headers['X-Wcode-Workspace'], 'A');
  assert.equal(f.run('state.runtimeJobSelected'), A);
  assert.equal(f.run('state.runtimeJob.success'), false);
  assert.equal(f.run('state.runtimeJobs.length'), 0, 'a navigation binding must not fabricate discovery rows');
  assert.match(f.node('#runtimeJobOutput').innerHTML, /actual failure/);
  assert.doesNotMatch(f.node('#runtimeJobDetail').innerHTML, /data-job-cancel/);
});
test('missing or malformed bindings and non-command tasks never offer output navigation', () => {
  for (const row of [task({command_job_id:null}), task({command_job_id:'<script>'}),
    task({command_job_id:A.toUpperCase()}), task({tool:'verify_project'}), task({id:-1}),
    task({id:Number.MAX_SAFE_INTEGER+1}), task({status:'passed'})]) {
    const f = fixture([row]);
    assert.equal(f.buttons.length, 0);
    assert.equal(f.selection(), null);
    assert.doesNotMatch(f.html, /<script>/);
    assert.equal(f.requests.length, 0);
  }
});
test('duplicate monitor identities and overbounded activity cannot invent a unique job link', () => {
  for (const rows of [[task(), task({command_job_id:B})], Array.from({length:13},(_,i)=>task({id:i+1}))]) {
    const f = fixture(rows);
    assert.equal(f.buttons.length, 0);
    assert.equal(f.selection(), null);
  }
});
test('cached project activity and foreign outer or nested workspace keep output unavailable', () => {
  for (const change of [
    'state.project.activity=observed.activity;state.activitySnapshot=null',
    'state.activitySnapshot.workspace="B"',
    'state.activitySnapshot.activity.workspace="B"',
    'state.activitySnapshot.activity.available=false',
    'state.activityError=true',
  ]) {
    const f=fixture();f.run(change);f.render();
    assert.equal(f.buttons.length, 0);
    assert.equal(f.requests.length, 0);
  }
});
test('previous render callbacks are inert while identical retained markup gets a fresh binding', async () => {
  const f=fixture(), button=f.buttons[0], previous=button.onclick;
  f.render();
  assert.equal(f.buttons[0],button,'unchanged HTML should retain keyboard focus and its DOM node');
  previous();await flush();assert.equal(f.requests.length,0);
  f.buttons[0].click();await settle(f);
  assert.equal(f.requests[0].url,'/intelligence/jobs/'+A);
});
test('same-scope activity updates preserve focus on the exact retained task without scrolling', () => {
  const f=fixture(),previous=f.buttons[0];previous.focus();
  f.run('state.activitySnapshot={...observed,activity:{...observed.activity,recent:[{...observed.activity.recent[0],run_ms:3000}]}};');
  f.render();
  assert.notEqual(f.buttons[0],previous,'changed live timing must exercise replacement DOM');
  assert.equal(f.context.document.activeElement,f.buttons[0]);
  assert.equal(f.buttons[0].focusOptions.preventScroll,true);
  assert.equal(f.requests.length,0);
});
test('activity refresh never transfers keyboard focus to another task, job or workspace scope', () => {
  for(const change of [
    'state.activitySnapshot.activity.recent[0]={...observed.activity.recent[0],id:8}',
    `state.activitySnapshot.activity.recent[0]={...observed.activity.recent[0],command_job_id:${JSON.stringify(B)}}`,
    'state.activitySnapshot.activity.recent=[]',
    'state.workspaceEpoch++;state.activitySnapshot.activity.recent[0].run_ms=3000',
    'state.runtimeJobsScope.owner="replacement-owner";state.activitySnapshot.activity.recent[0].run_ms=3000',
  ]) {
    const f=fixture();f.buttons[0].focus();f.run(change);f.render();
    assert.equal(f.context.document.activeElement,f.context.document.body);
    assert.equal(f.requests.length,0);
  }
});
test('activity updates do not steal focus from the output search or another control', () => {
  const f=fixture(),search=f.node('#runtimeJobOutputFilters');
  f.context.document.activeElement=search;
  f.run('state.activitySnapshot.activity.recent[0].run_ms=3000;');f.render();
  assert.equal(f.context.document.activeElement,search);
  assert.equal(f.buttons[0].focusOptions,undefined);
  assert.equal(f.requests.length,0);
});
test('replacing the activity snapshot or its rows invalidates old callbacks before repaint', async () => {
  for (const change of [
    'state.activitySnapshot={...observed,activity:{...observed.activity}}',
    'state.activitySnapshot.activity.recent=[...observed.activity.recent]',
    'state.activitySnapshot.activity.recent[0]={...observed.activity.recent[0]}',
  ]) {
    const f=fixture(), previous=f.buttons[0].onclick;f.run(change);previous();await flush();
    assert.equal(f.requests.length,0);
  }
});
test('task identity and bound job cannot mutate behind a rendered navigation control', async () => {
  for (const patch of [{id:8},{command_job_id:B},{tool:'read_file'},{status:'passed'}]) {
    const f=fixture(),previous=f.buttons[0].onclick;
    Object.assign(f.context.observed.activity.recent[0],patch);previous();await flush();
    assert.equal(f.requests.length,0);
  }
});
test('hidden pages, navigation, stale ownership and workspace epochs withdraw old controls', async () => {
  for (const change of [
    'document.hidden=true', 'state.workspaceTab="overview"', 'state.activityError=true',
    'state.runtimeJobsScope.owner="replacement-owner"', 'state.workspaceEpoch++',
    'state.current="B";clearRuntimeJobs();state.current="A";clearRuntimeJobs()',
  ]) {
    const f=fixture(),previous=f.buttons[0].onclick;f.run(change);previous();await flush();
    assert.equal(f.requests.length,0);
  }
});
test('returning to Activity cannot reactivate an old render callback', async () => {
  const f=fixture(),previous=f.buttons[0].onclick;
  f.run('activateWorkspaceTab("overview");activateWorkspaceTab("activity");');
  previous();await flush();assert.equal(f.requests.length,0);
  f.buttons[0].click();await settle(f);
});
test('unknown exact jobs do not select a substitute or rerun a command', async () => {
  const f=fixture();f.buttons[0].click();await flush();
  assert.equal(f.requests.length,1);
  f.requests[0].resolve({ok:false,status:404,json:async()=>({error:'PRIVATE BACKEND MESSAGE'})});
  await flush();
  assert.equal(f.run('state.runtimeJob'),null);
  assert.equal(f.run('state.runtimeJobSelected'),A);
  assert.match(f.node('#runtimeJobMessage').innerHTML,/unknown/);
  assert.doesNotMatch(f.node('#runtimeJobMessage').innerHTML,/PRIVATE/);
  assert.equal(f.requests[0].options.method,'GET');
});
test('rapid repeated navigation stays one exact GET and a late foreign reply is rejected', async () => {
  const f=fixture(),click=f.buttons[0].onclick;click();click();await flush();
  assert.equal(f.requests.length,1);
  f.run('state.current="B";clearWorkspaceView();');
  respond(f.requests[0],detail());await flush();
  assert.equal(f.run('state.runtimeJob'),null);
  assert.equal(f.run('state.runtimeJobSelected'),'');
  assert.doesNotMatch(f.node('#runtimeJobOutput').innerHTML,/actual failure/);
});
test('an arbitrary task ID or forged render cannot use the activity source allowance', async () => {
  const f=fixture();f.context.selection=f.selection();
  assert.equal(await f.run(`observeActivityJob(${JSON.stringify(B)},selection)`),false);
  assert.equal(await f.run(`observeRuntimeJob(${JSON.stringify(B)},true,selection)`),false);
  assert.equal(await f.run(`observeActivityJob(${JSON.stringify(A)},{...selection,render:{...selection.render}})`),false);
  assert.equal(f.requests.length,0);
});
test('Chinese activity copy exposes the same exact GET-only navigation', async () => {
  const f=fixture();f.run('state.language="zh-CN";renderActivity();');
  assert.match(f.html,/查看输出/);assert.match(f.html,/保留输出/);
  f.buttons[0].click();await settle(f);
  assert.equal(f.requests[0].options.method,'GET');
});
