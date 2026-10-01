'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const { sandbox, respond, flush } = require('./observatory.cjs');
const { fixture } = require('./acceptance.cjs');
const id = 'TASK-' + '1'.repeat(20) + '-' + 'a'.repeat(32);
function row(extra = {}) { return { task_id: id, workspace: 'A', tool: 'run_command', status: 'working', origin: 'mcp', can_cancel: false, created_at_ms: 1, updated_at_ms: 2, completion_is_not_success: true, ...extra }; }
function list(extra = {}) { return { schema_version: 1, kind: 'command_job_list', workspace: 'A', items: [row()], truncated: false, coverage: 'retained_bounded', discovery_is_not_execution: true, ...extra }; }
function recovery(extra = {}) { return list({ kind: 'verification_task_list', items: [row({ tool: 'verify_project', origin: 'ui' })], ...extra }); }
function detail(extra = {}) { return { schema_version: 1, kind: 'command_job', task_id: id, workspace: 'A', tool: 'run_command', status: 'working', origin: 'mcp', can_cancel: false, success: null, exit_code: null, error: null, stdout: { text: '海\n<script>not HTML</script>', total_bytes: 28, truncated: true, redacted: false }, stderr: { text: '[REDACTED]', total_bytes: 10, truncated: false, redacted: true }, completion_is_not_success: true, acceptance_ready: false, ...extra }; }
function prepared(auth = true) {
  const s = sandbox(false, auth, { fakeTimers: true }); s.context.fixture = fixture();
  s.run('state.project=fixture;state.workspaceTab="activity";renderRuntimeJobs();'); return s;
}
async function discover(s, value = list()) { const p = s.run('refreshRuntimeJobs()'); await flush(); respond(s.requests.at(-1), value); return p; }
async function observe(s, value = detail()) { s.context.id = value.task_id; const p = s.run('observeRuntimeJob(id)'); await flush(); respond(s.requests.at(-1), value); return p; }
function status(request, code) { request.resolve({ ok: false, status: code, json: async () => ({ error: 'PRIVATE BACKEND DETAIL' }) }); }
async function nativeInterop(input) {
  const s = prepared(); s.context.input = input;
  s.run('state.current=input.workspace;clearRuntimeJobs();');
  if (input.jobs) {
    assert.equal(s.run('validRetainedTaskList(input.jobs, input.workspace)'), true);
    assert.equal(s.run('validRuntimeJob(input.job, input.workspace, input.job.task_id)'), true);
    s.run('state.runtimeJobs=input.jobs.items;state.runtimeJobsLoaded=true;state.runtimeJob=input.job;state.runtimeJobSelected=input.job.task_id;renderRuntimeJobs();');
    assert.ok(s.node('#runtimeJobDetail').innerHTML.includes(input.job.task_id));
  }
  if (input.recovery) {
    assert.equal(s.run('validRetainedTaskList(input.recovery,input.workspace,true)'), true);
    assert.equal(s.run('validVerificationReceipt(input.verification,{workspace:input.workspace,taskId:input.verification.task_id})'), true);
    s.run('entry=verificationEntry(true);entry.discovered=input.recovery.items;entry.task=input.verification;renderVerificationTask();');
    assert.ok(s.node('#verificationTask').innerHTML.includes(input.verification.task_id));
  }
  assert.equal(s.requests.length, 0, 'rendering serialized native records cannot execute anything');
  console.log(JSON.stringify({ passed: true, source: 'native-http-serializer', jobs: !!input.jobs, verification: !!input.recovery }));
}
async function main() {
  const results = [];
  async function test(name, fn) { try { await fn(); results.push({ name, passed: true }); } catch (error) { results.push({ name, passed: false, error: error.stack }); } }
  await test('rendering has no command launch and never sends automatic requests', async () => {
    const s = prepared(); assert.equal(s.requests.length, 0);
    assert.match(s.node('#runtimeJobForm').innerHTML, /data-jobs-refresh/);
    assert.doesNotMatch(s.node('#runtimeJobForm').innerHTML, /input|program|data-job-launch/);
    assert.match(s.node('#runtimeJobMessage').innerHTML, /not a complete process list/);
  });
  await test('missing token cannot discover jobs or verification tasks', async () => {
    const s = prepared(false); assert.equal(await s.run('refreshRuntimeJobs()'), false);
    assert.equal(await s.run('discoverVerificationTasks()'), false); assert.equal(s.requests.length, 0);
  });
  await test('manual bounded discovery keeps exact workspace headers and partial history', async () => {
    const s = prepared(); assert.equal(await discover(s, list({ truncated: true })), true);
    assert.equal(s.requests[0].url, '/intelligence/jobs'); assert.equal(s.requests[0].options.method, 'GET');
    assert.equal(s.requests[0].options.headers['X-Wcode-Workspace'], 'A'); assert.equal(s.requests[0].options.headers['X-Wcode-UI-Token'], 'test-ui');
    assert.match(s.node('#runtimeJobMessage').innerHTML, /truncated/); assert.match(s.node('#runtimeJobList').innerHTML, new RegExp(id));
  });
  await test('foreign malformed duplicate overbound and favorable discovery records are rejected', async () => {
    for (const value of [list({ workspace: 'B' }), list({ items: [row(), row()] }), list({ items: Array(33).fill(row()) }), list({ items: [row({ can_cancel: true })] }), list({ discovery_is_not_execution: false }), list({ items: [row({ task_id: '<script>' })] }), list({ items: [row({ status: 'passed' })] })]) {
      const s = prepared(); assert.equal(await discover(s, value), false); assert.equal(s.run('state.runtimeJobsLoaded'), false);
      assert.equal(s.run('state.runtimeJobs.length'), 0); assert.match(s.node('#runtimeJobMessage').innerHTML, /unavailable/);
    }
  });
  await test('real discovered ID selects a bounded escaped unknown-outcome log without changing Acceptance', async () => {
    const s = prepared(); await discover(s); assert.equal(await observe(s), true);
    assert.equal(s.requests[1].url, '/intelligence/jobs/' + id);
    const html = s.node('#runtimeJobDetail').innerHTML; assert.match(html, /&lt;script&gt;/); assert.doesNotMatch(html, /<script>|data-job-cancel/);
    assert.match(html, /Unknown/); assert.match(html, /truncated/); assert.match(html, /redacted/); assert.equal(s.run('acceptanceView().status'), 'blocked');
    assert.equal(await s.run('observeRuntimeJob("TASK-"+"2".repeat(20)+"-"+"b".repeat(32))'), false);
  });
  await test('MCP-owned jobs cannot POST cancellation', async () => {
    const s = prepared(); await discover(s); await observe(s); assert.equal(await s.run('cancelRuntimeJob()'), false);
    assert.equal(s.requests.filter(request => request.options.method === 'POST').length, 0);
  });
  await test('current UI-owned cancellation uses exact ID no actor no command and preserves failed outcome', async () => {
    const s = prepared(); await discover(s, list({ items: [row({ origin: 'ui', can_cancel: true })] }));
    await observe(s, detail({ origin: 'ui', can_cancel: true }));
    const p = s.run('cancelRuntimeJob()'); await flush(); const request = s.requests.at(-1);
    assert.equal(request.url, '/intelligence/jobs/' + id + '/cancel'); assert.deepEqual(JSON.parse(request.options.body), {});
    respond(request, detail({ origin: 'ui', status: 'cancelled', success: false, can_cancel: false })); assert.equal(await p, true);
    assert.equal(s.run('state.runtimeJob.status'), 'cancelled'); assert.equal(s.run('acceptanceView().status'), 'blocked');
  });
  await test('uncertain cancellation cannot repeat until an exact fresh status is read', async () => {
    const s = prepared(); await discover(s); await observe(s, detail({ origin: 'ui', can_cancel: true }));
    const p = s.run('cancelRuntimeJob()'); await flush(); s.requests.at(-1).reject(new Error('PRIVATE')); await p;
    assert.equal(s.run('state.runtimeJobsUncertain'), true); assert.equal(await s.run('cancelRuntimeJob()'), false);
    await discover(s); assert.equal(s.run('state.runtimeJobsUncertain'), true);
    await observe(s, detail({ origin: 'ui', can_cancel: true })); assert.equal(s.run('state.runtimeJobsUncertain'), false);
    assert.equal(s.requests.filter(request => request.options.method === 'POST').length, 1);
  });
  await test('malformed details withdraw old results and never turn missing output into success', async () => {
    for (const patch of [{ workspace: 'B' }, { acceptance_ready: true }, { can_cancel: true }, { stdout: { text: '海'.repeat(11000), total_bytes: 33000, truncated: false, redacted: false } }, { success: 'true' }, { stderr: null }]) {
      const s = prepared(); await discover(s); await observe(s, detail({ status: 'completed', success: true }));
      assert.equal(await observe(s, detail(patch)), false); assert.equal(s.run('state.runtimeJob'), null);
      assert.doesNotMatch(s.node('#runtimeJobDetail').innerHTML, /Reported success/);
    }
  });
  await test('bounded GET-only polling pauses when hidden or the poll budget is exhausted', async () => {
    const s = prepared(); await discover(s); await observe(s); assert.equal(s.timers.size, 1);
    const timer = [...s.timers.values()][0]; timer.fn(); await flush(); assert.equal(s.requests.at(-1).options.method, 'GET');
    respond(s.requests.at(-1), detail()); await flush();
    s.run('state.runtimeJobsPolls=120;scheduleRuntimeJobObservation();'); assert.equal(s.timers.size, 0);
    s.run('state.runtimeJobsPolls=0;document.hidden=true;scheduleRuntimeJobObservation();'); assert.equal(s.timers.size, 0);
    assert.equal(s.requests.filter(request => request.options.method === 'POST').length, 0);
  });
  await test('A to B to A workspace changes discard old discovery replies', async () => {
    const s = prepared(), p = s.run('refreshRuntimeJobs()'); await flush();
    s.run('state.current="B";clearWorkspaceView();state.current="A";clearWorkspaceView();');
    respond(s.requests[0], list()); assert.equal(await p, false); assert.equal(s.run('state.runtimeJobs.length'), 0);
    assert.doesNotMatch(s.node('#runtimeJobList').innerHTML, new RegExp(id));
  });
  await test('foreign cached session ownership clears output and drops in-flight replies', async () => {
    const s = prepared(); await discover(s); await observe(s);
    const p = s.run('observeRuntimeJob()'); await flush(); const request = s.requests.at(-1);
    s.run('state.runtimeJobsScope.owner="other-ui";renderRuntimeJobs();'); respond(request, detail()); assert.equal(await p, false);
    assert.equal(s.run('state.runtimeJob'), null); assert.equal(s.run('state.runtimeJobs.length'), 0);
  });
  await test('unavailable exact IDs show unknown without exposing server diagnostics or restarting', async () => {
    const s = prepared(); await discover(s); s.context.id = id;
    const p = s.run('observeRuntimeJob(id)'); await flush(); status(s.requests.at(-1), 404); assert.equal(await p, false);
    assert.match(s.node('#runtimeJobMessage').innerHTML, /unknown/); assert.doesNotMatch(s.node('#runtimeJobMessage').innerHTML, /PRIVATE/);
    assert.equal(s.requests.every(request => request.options.method === 'GET'), true);
  });
  await test('verification recovery discovers real UI IDs without selecting starting or clearing launch uncertainty', async () => {
    const s = prepared(); s.run('verificationEntry(true).uncertain=true');
    const p = s.run('discoverVerificationTasks()'); await flush(); assert.equal(s.requests[0].url, '/intelligence/verification/tasks');
    respond(s.requests[0], recovery({ truncated: true })); assert.equal(await p, true);
    assert.equal(s.run('verificationEntry().task'), null); assert.equal(s.run('verificationEntry().uncertain'), true);
    assert.match(s.node('#verificationTask').innerHTML, /data-verification-discovered/); assert.match(s.node('#verificationTask').innerHTML, /partial/);
    assert.equal(s.requests.length, 1); assert.equal(s.requests[0].options.method, 'GET');
  });
  await test('recovery rejects foreign owner tool workspace and malformed ID records', async () => {
    for (const value of [recovery({ workspace: 'B' }), recovery({ items: [row()] }), recovery({ items: [row({ tool: 'verify_project', origin: 'mcp' })] }), recovery({ items: [row({ tool: 'verify_project', origin: 'ui', task_id: 'guessed-id' })] })]) {
      const s = prepared(), p = s.run('discoverVerificationTasks()'); await flush(); respond(s.requests[0], value);
      assert.equal(await p, false); assert.equal(s.run('verificationEntry().discovered.length'), 0);
    }
  });
  await test('empty or failed recovery leaves lost launch unknown and never automatically repeats POST', async () => {
    for (const failed of [true, false]) {
      const s = prepared(); s.run('verificationEntry(true).uncertain=true'); const p = s.run('discoverVerificationTasks()'); await flush();
      if (failed) status(s.requests[0], 503); else respond(s.requests[0], recovery({ items: [] })); await p;
      assert.equal(s.run('verificationEntry().uncertain'), true); assert.equal(await s.run('startVerificationTask("full")'), false);
      assert.equal(s.requests.length, 1); assert.doesNotMatch(s.node('#verificationTask').innerHTML, /PRIVATE/);
    }
  });
  await test('token-scoped verification discovery cannot reveal a prior session cached task', async () => {
    const s = prepared(), p = s.run('discoverVerificationTasks()'); await flush(); respond(s.requests[0], recovery()); await p;
    s.run('verificationEntry().owner="other-ui";renderVerificationTask();'); assert.doesNotMatch(s.node('#verificationTask').innerHTML, new RegExp(id));
    assert.equal(s.run('verificationEntry()'), undefined);
  });
  await test('discovered task button reads exactly its server ID and never replays a lost launch', async () => {
    const s = prepared(), p = s.run('discoverVerificationTasks()'); await flush(); respond(s.requests[0], recovery()); await p;
    const button = s.node('#discoveredButton'); button.dataset.verificationDiscovered = id;
    const host = { querySelector: () => null, querySelectorAll: () => [button] };
    s.context.host = host; s.run('bindVerificationRecovery(host,verificationEntry());'); button.events.click(); await flush();
    assert.equal(s.requests.at(-1).url, '/intelligence/verification/' + id); assert.equal(s.requests.at(-1).options.method, 'GET');
    status(s.requests.at(-1), 404); await flush(); assert.equal(s.requests.filter(request => request.options.method === 'POST').length, 0);
  });
  console.log(JSON.stringify({ suite: 'native-jobs-ui', results }, null, 2));
  assert.ok(results.every(row => row.passed), results.filter(row => !row.passed).map(row => row.name).join('\n'));
}
const keepAlive = setInterval(() => {}, 1000);
(process.argv[3] ? nativeInterop(JSON.parse(fs.readFileSync(process.argv[3], 'utf8'))) : main())
  .catch(error => { console.error(error); process.exitCode = 1; }).finally(() => clearInterval(keepAlive));
