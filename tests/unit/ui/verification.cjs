'use strict';
// Browser protocol behavior only; native execution/Acceptance remain server-owned.
const assert = require('node:assert/strict');
const { sandbox, respond, flush } = require('./observatory.cjs');
const { fixture } = require('./acceptance.cjs');
function task(change = {}) {
  const revision = fixture().acceptance.revision;
  return { schema_version: 1, kind: 'verification_task', task_id: 'task-native-1', workspace: 'A', tool: 'verify_project',
    status: 'working', status_message: 'Native checks queued', created_at_ms: 1, updated_at_ms: 2, ttl_ms: 300000,
    poll_interval_ms: 2000, terminal: false, result_available: false, completion_is_not_success: true,
    requested_revision: { ...revision }, current_revision: { ...revision }, freshness: 'current', git_freshness: 'current',
    is_error: false, report: null, acceptance_ready: false, authorization_required: false, error: null, next_action: 'poll_status', ...change };
}
function report(change = {}) {
  return { workspace: 'A', level: 'full', passed: false, checks_run: 1, checks_failed: 1, checks_reused: 0, elapsed_ms: 22,
    summary: 'One failure', skipped_checks: [], checks_total: 1, checks_truncated: false,
    checks: [{ id: 'native-check', phase: 1, success: false, execution: 'executed', reused: false, exit_code: 1,
      elapsed_ms: 22, command: 'native compiler', stdout_tail: '<script>unsafe</script>', stderr_tail: 'bounded diagnostic',
      output_truncated: true, evidence_id: 'EV-native' }], ...change };
}
function prepared(authenticated = true) {
  const s = sandbox(false, authenticated, { fakeTimers: true });
  s.context.fixture = fixture(); s.context.fixture.snapshot_revision = 'opaque-server-snapshot';
  s.run('state.project=fixture;state.workspaceTab="proof";renderVerificationTask();');
  return s;
}
function attach(s, value = task()) {
  s.context.receipt = value; s.run('entry=verificationEntry(true);entry.task=receipt;entry.expectedRevision=receipt.requested_revision;renderVerificationTask();');
}
function http(request, status) { request.resolve({ ok: false, status, json: async () => ({ error: 'PRIVATE command token/source body' }) }); }
async function launch(s, level = 'full', value = task()) {
  const promise = s.run('startVerificationTask("' + level + '")'); await flush();
  respond(s.requests.at(-1), value); await promise;
}
async function main() {
  const results = [];
  async function test(name, fn) {
    try { await fn(); results.push({ name, passed: true }); }
    catch (error) { results.push({ name, passed: false, error: error.stack }); }
  }
  await test('native Run is an explicit operator click, retains quick/full intent and never sends a command or owner', async () => {
    const s = prepared(); assert.equal(s.requests.length, 0); assert.match(s.node('#verificationTask').innerHTML, /data-verification-run="full"/);
    const promise = s.run('startVerificationTask("full")'); await flush();
    const request = s.requests[0]; assert.equal(request.url, '/intelligence/verification/run'); assert.equal(request.options.method, 'POST');
    assert.equal(request.options.headers['X-Wcode-UI-Token'], 'test-ui'); assert.equal(request.options.headers['X-Wcode-Workspace'], 'A');
    const body = JSON.parse(request.options.body);
    assert.deepEqual(body, { level: 'full', snapshot_revision: 'opaque-server-snapshot', revision: fixture().acceptance.revision,
      timeout_seconds: 1800, fail_fast: false });
    assert.equal(s.run('verificationEntry().busy'), true);
    assert.equal(await s.run('startVerificationTask("quick")'), false, 'double click must not create another task');
    respond(request, task()); assert.equal(await promise, true);
    assert.equal(s.run('verificationEntry().task.task_id'), 'task-native-1');
    assert.equal(s.requests.length, 1); assert.equal(s.run('acceptanceView().status'), 'blocked');
    const quick = prepared(); await launch(quick, 'quick');
    const quickBody = JSON.parse(quick.requests[0].options.body);
    assert.equal(quickBody.timeout_seconds, 600); assert.equal(quickBody.fail_fast, true);
  });
  await test('missing UI token readonly cached unconfirmed and stale snapshots cannot launch any task', async () => {
    const missing = prepared(false); assert.equal(await missing.run('startVerificationTask("full")'), false); assert.equal(missing.requests.length, 0);
    for (const code of ['state.syncError=true', 'state.fitnessSnapshotFromCache=true', 'state.project.snapshot_refreshing=true',
      'state.project.snapshot_unconfirmed=true', 'delete state.project.snapshot_revision',
      'state.project.git_observation={available:false,reason:"execution_disabled"}', 'state.project.workspace="B"']) {
      const s = prepared(); s.run(code + ';renderVerificationTask();');
      assert.equal(await s.run('startVerificationTask("full")'), false); assert.equal(s.requests.length, 0);
      assert.match(s.node('#verificationTask').innerHTML, /Refresh a current authorized snapshot/);
    }
  });
  await test('invalid foreign or favorable completion receipts are unknown and never establish ready', async () => {
    for (const mutate of [value => { value.workspace = 'B'; }, value => { value.schema_version = 2; },
      value => { value.tool = 'run_command'; }, value => { value.acceptance_ready = true; },
      value => { value.completion_is_not_success = false; }, value => { value.requested_revision.code = 'foreign'; },
      value => { value.terminal = true; }, value => { value.git_freshness = 'unknown'; }]) {
      const s = prepared(), value = task(); mutate(value);
      await launch(s, 'full', value); assert.equal(s.run('verificationEntry().task'), null);
      assert.equal(s.run('verificationEntry().uncertain'), true); assert.equal(s.run('acceptanceView().status'), 'blocked');
    }
  });
  await test('transport and bad launch responses retain unknown outcome, disable repeat POST and allow only status reconnect', async () => {
    const s = prepared(); const promise = s.run('startVerificationTask("full")'); await flush(); s.requests[0].reject(new Error('PRIVATE network body')); await promise;
    assert.equal(s.run('verificationEntry().uncertain'), true); assert.equal(await s.run('startVerificationTask("full")'), false);
    assert.match(s.node('#verificationTask').innerHTML, /launch outcome is unknown/); assert.doesNotMatch(s.node('#verificationTask').innerHTML, /PRIVATE/);
    const reconnect = s.run('observeVerificationTask({taskId:"task-native-1",manual:true})'); await flush();
    assert.equal(s.requests[1].url, '/intelligence/verification/task-native-1'); assert.equal(s.requests[1].options.method, 'GET');
    respond(s.requests[1], task()); assert.equal(await reconnect, true); assert.equal(s.run('verificationEntry().uncertain'), false);
    assert.equal(s.requests.filter(row => row.options.method === 'POST').length, 1);
  });
  await test('known permission or stale preflight failures never grant permission or automatically retry', async () => {
    for (const status of [401, 403, 409]) {
      const s = prepared(); const promise = s.run('startVerificationTask("full")'); await flush(); http(s.requests[0], status); await promise;
      assert.equal(s.run('verificationEntry().uncertain'), false); assert.equal(s.requests.length, 1);
      assert.doesNotMatch(s.node('#verificationTask').innerHTML, /PRIVATE/); assert.equal(s.run('acceptanceView().status'), 'blocked');
    }
  });
  await test('polling stays bounded, pauses for exact approval, hidden UI and budget without rerunning', async () => {
    const s = prepared(); await launch(s); assert.equal(s.timers.size, 1);
    const timer = [...s.timers.values()][0]; assert.ok(timer.ms >= 2000 && timer.ms <= 8000); s.timers.clear();
    timer.fn(); await flush(); assert.equal(s.requests[1].options.method, 'GET');
    respond(s.requests[1], task({ status: 'input_required', authorization_required: true }));
    await flush(); assert.equal(s.timers.size, 0); assert.match(s.node('#verificationTask').innerHTML, /Inspect approval/);
    assert.equal(s.requests.filter(row => row.options.method === 'POST').length, 1);
    s.run('verificationEntry().task.status="working";verificationEntry().task.authorization_required=false;verificationEntry().polls=120;scheduleVerificationObservation(verificationEntry());renderVerificationTask();');
    assert.equal(s.timers.size, 0); assert.match(s.node('#verificationTask').innerHTML, /Automatic observation paused/);
    s.run('verificationEntry().polls=0;document.hidden=true;scheduleVerificationObservation(verificationEntry());');
    assert.equal(s.timers.size, 0);
  });
  await test('terminal result fetch shows typed bounded diagnostics then force-refreshes canonical Evidence and Acceptance', async () => {
    const s = prepared(); attach(s);
    const promise = s.run('observeVerificationTask({result:true,manual:true})'); await flush();
    assert.equal(s.requests[0].url, '/intelligence/verification/task-native-1/result');
    respond(s.requests[0], task({ status: 'completed', terminal: true, result_available: true,
      report: report({ passed: true, checks_failed: 0 }), next_action: 'inspect_result_and_refresh_project' }));
    await flush(); assert.equal(s.requests[1].url, '/intelligence/project');
    assert.equal(s.requests[1].options.headers['X-Wcode-Prefer-Cached'], undefined, 'post-run Acceptance must be freshly captured');
    const current = fixture(); current.snapshot_revision = 'new-opaque-snapshot'; respond(s.requests[1], current);
    assert.equal(await promise, true);
    const html = s.node('#verificationTask').innerHTML;
    assert.match(html, /Passed/); assert.match(html, /task completion is not success|Task completion is not success/);
    assert.match(html, /&lt;script&gt;unsafe/); assert.doesNotMatch(html, /<script>/);
    assert.match(html, /data-acceptance-evidence="EV-native"/); assert.match(html, /Output is bounded and truncated/);
    assert.equal(s.run('acceptanceView().status'), 'blocked', 'reported pass must not override canonical blocker');
    assert.equal(s.requests.filter(row => row.options.method === 'POST').length, 0);
  });
  await test('result refresh failure retains historical Acceptance only as stale without claiming verification success', async () => {
    const s = prepared(); s.run('state.project.acceptance.state="ready";'); attach(s);
    const promise = s.run('observeVerificationTask({result:true})'); await flush();
    respond(s.requests[0], task({ status: 'completed', terminal: true, result_available: true, report: report() }));
    await flush(); http(s.requests[1], 503); await promise;
    assert.equal(s.run('acceptanceView().status'), 'stale'); assert.match(s.node('#verificationTask').innerHTML, /could not be refreshed/);
  });
  await test('nullable report fields and unavailable checks remain unknown, and skipped or truncated output stays visible', () => {
    const s = prepared(); s.context.receipt = task({ report: report({ skipped_checks: ['native-skip'], checks_total: 80, checks_truncated: true,
      checks: [{ id: null, phase: null, success: null, execution: null, reused: null, exit_code: null, elapsed_ms: null,
        command: null, stdout_tail: null, stderr_tail: null, evidence_id: null, output_truncated: false }] }) });
    assert.equal(s.run('validVerificationTaskReport(receipt.report,"A")'), true);
    const html = s.run('verificationReportHtml(receipt)'); assert.match(html, /Outcome unknown/); assert.match(html, /Output unavailable/);
    assert.match(html, /Skipped.*native-skip/); assert.match(html, /1 \/ 80/);
    assert.doesNotMatch(html, /Reported pass|EV-native/);
  });
  await test('malformed report and task identity never replaces the existing task or canonical decision', async () => {
    for (const mutate of [value => { value.task_id = 'other-task'; }, value => { value.report = report({ workspace: 'B' }); },
      value => { value.report = report({ checks: Array.from({ length: 65 }, () => report().checks[0]) }); },
      value => { value.report = report({ checks_failed: -1 }); }, value => { value.report = report({ checks: [{ ...report().checks[0], stdout_tail: 'x'.repeat(1025) }] }); }]) {
      const s = prepared(); attach(s); const promise = s.run('observeVerificationTask({result:true})'); await flush();
      const value = task({ status: 'completed', terminal: true, result_available: true }); mutate(value); respond(s.requests[0], value); await promise;
      assert.equal(s.run('verificationEntry().task.status'), 'working'); assert.equal(s.run('verificationEntry().result'), null);
      assert.equal(s.requests.length, 1); assert.equal(s.run('acceptanceView().status'), 'blocked');
    }
  });
  await test('cancellation is exact and a lost cancel reply requires status observation rather than assuming stopped', async () => {
    const s = prepared(); attach(s); const promise = s.run('cancelVerificationTask()'); await flush();
    assert.equal(s.requests[0].url, '/intelligence/verification/task-native-1/cancel'); assert.equal(s.requests[0].options.method, 'POST');
    assert.deepEqual(JSON.parse(s.requests[0].options.body), {}); http(s.requests[0], 503); await promise;
    assert.equal(s.run('verificationEntry().task.status'), 'working'); assert.match(s.node('#verificationTask').innerHTML, /Cancellation was not confirmed/);
    const status = s.run('observeVerificationTask({manual:true})'); await flush(); respond(s.requests[1], task()); await status;
    assert.equal(s.requests.filter(row => row.options.method === 'POST').length, 1);
  });
  await test('workspace A to B to A invalidates late launches and preserves uncertainty instead of allowing duplicate submission', async () => {
    const s = prepared(); const old = s.run('startVerificationTask("full")'); await flush();
    s.run('state.current="B";clearWorkspaceView();state.current="A";clearWorkspaceView();state.project=fixture;');
    respond(s.requests[0], task()); assert.equal(await old, false);
    assert.equal(s.run('verificationEntry().task'), null); assert.equal(s.run('verificationEntry().uncertain'), true);
    assert.equal(await s.run('startVerificationTask("full")'), false); assert.equal(s.requests.length, 1);
  });
  await test('late status from another view or replaced task cannot overwrite current owner state', async () => {
    const s = prepared(); attach(s); const old = s.run('observeVerificationTask()'); await flush();
    s.run('state.current="B";clearWorkspaceView();state.current="A";clearWorkspaceView();state.project=fixture;');
    s.context.freshTask = task({ task_id: 'new-task' }); s.run('verificationEntry().task=freshTask;');
    respond(s.requests[0], task({ status: 'completed', terminal: true, result_available: true })); assert.equal(await old, false);
    assert.equal(s.run('verificationEntry().task.task_id'), 'new-task'); assert.equal(s.requests.length, 1);
    assert.equal(s.run('acceptanceView().status'), 'blocked');
  });
  await test('foreign or missing reconnect cannot substitute task ownership or expose transport error text', async () => {
    const s = prepared(); const promise = s.run('observeVerificationTask({taskId:"foreign-task",manual:true})'); await flush(); http(s.requests[0], 404); await promise;
    assert.equal(s.run('verificationEntry().task'), null); assert.match(s.node('#verificationTask').innerHTML, /No task is substituted/);
    assert.doesNotMatch(s.node('#verificationTask').innerHTML, /PRIVATE/);
    assert.equal(await s.run('observeVerificationTask({taskId:"../private"})'), false); assert.equal(s.requests.length, 1);
  });
  await test('shipped Run button is wired to one protected verification launch and rendering itself stays read-only', async () => {
    const s = prepared(), button = { dataset: { verificationRun: 'quick' }, events: {},
      addEventListener(name, callback) { this.events[name] = callback; } };
    s.node('#verificationTask').querySelectorAll = selector => selector === '[data-verification-run]' ? [button] : [];
    s.run('invalidate("verificationTask");renderVerificationTask();'); assert.equal(s.requests.length, 0);
    assert.equal(typeof button.events.click, 'function'); button.events.click(); await flush();
    assert.equal(s.requests.length, 1); assert.equal(JSON.parse(s.requests[0].options.body).level, 'quick');
    respond(s.requests[0], task()); await flush();
    assert.equal(s.run('verificationEntry().task.task_id'), 'task-native-1'); assert.equal(s.run('verificationEntry().busy'), false);
  });
  await test('late terminal refresh cannot overwrite Acceptance after a new verification task starts in the same workspace', async () => {
    const s = prepared(); attach(s, task({ status: 'completed', terminal: true, result_available: true }));
    const old = s.run('refreshVerificationAcceptance(verificationEntry())'); await flush();
    assert.equal(s.requests[0].url, '/intelligence/project');
    const freshRun = s.run('startVerificationTask("full")'); await flush();
    respond(s.requests[1], task({ task_id: 'new-task' })); await freshRun;
    const stale = fixture(); stale.snapshot_revision = 'late-old-snapshot'; stale.acceptance.state = 'ready'; respond(s.requests[0], stale);
    assert.equal(await old, false); assert.equal(s.run('verificationEntry().task.task_id'), 'new-task');
    assert.equal(s.run('acceptanceView().status'), 'blocked'); assert.equal(s.run('state.inFlight'), false);
  });
  console.log(JSON.stringify({ suite: 'native-verification-webui', results }, null, 2));
  assert.ok(results.every(row => row.passed), results.filter(row => !row.passed).map(row => row.name).join('\n'));
}
if (require.main === module) {
  const keepAlive = setInterval(() => {}, 1000);
  main().catch(error => { console.error(error); process.exitCode = 1; }).finally(() => clearInterval(keepAlive));
}
module.exports = { task, report };
