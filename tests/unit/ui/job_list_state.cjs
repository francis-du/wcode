'use strict';
// Shipped bundle and its HTTP/list/render paths, with deterministic transport.
// No command is launched; each exact response may reconcile only retained IDs.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {sandbox, respond, flush} = require('./observatory.cjs');
const fs = require('node:fs');
const path = require('node:path');
const jobsSource = fs.readFileSync(path.resolve(__dirname, '../../../src/ui/intelligence_web/app/jobs.js'), 'utf8');
function withoutReconciliation(name, next) {
  const start = jobsSource.indexOf('async function ' + name + '(');
  const end = jobsSource.indexOf(next, start);
  assert.ok(start >= 0 && end > start, 'exact production function must be found');
  const body = jobsSource.slice(start, end).replace(/\r\n?/g, '\n');
  const transition = '    state.runtimeJobs = state.runtimeJobs.map(row => row.task_id === data.task_id\n'
    + '      ? { ...row, status: data.status, origin: data.origin, can_cancel: data.can_cancel } : row);\n';
  assert.equal(body.split(transition).length, 2, 'one reconciliation transition must be ablated');
  return body.replace(transition, '');
}

const A = 'TASK-' + '1'.repeat(20) + '-' + 'a'.repeat(32);
const B = 'TASK-' + '2'.repeat(20) + '-' + 'b'.repeat(32);
const stream = text => ({text, total_bytes:Buffer.byteLength(text), truncated:false, redacted:false});
const row = (id = A, patch = {}) => ({task_id:id, workspace:'A', tool:'run_command', origin:'ui',
  status:'working', can_cancel:true, created_at_ms:1, updated_at_ms:2, completion_is_not_success:true, ...patch});
const job = (patch = {}) => ({schema_version:1, kind:'command_job', task_id:A, workspace:'A', tool:'run_command',
  origin:'ui', status:'working', can_cancel:true, success:null, exit_code:null, error:null,
  completion_is_not_success:true, acceptance_ready:false, stdout:stream('retained output'), stderr:stream(''), ...patch});
function fixture(override) {
  const s = sandbox(false, true, {fakeTimers:true});
  if (override) s.run(override);
  s.run('state.workspaceTab="activity";clearRuntimeJobs();');
  return s;
}
async function discover(s, items = [row(), row(B)], truncated = false) {
  const pending = s.run('refreshRuntimeJobs()'); await flush();
  respond(s.requests.at(-1), {schema_version:1, kind:'command_job_list', workspace:'A',
    items, truncated, coverage:'retained_bounded', discovery_is_not_execution:true});
  assert.equal(await pending, true);
}
async function exact(s, value = job()) {
  const pending = s.run('observeRuntimeJob(' + JSON.stringify(value.task_id) + ')'); await flush();
  respond(s.requests.at(-1), value); return pending;
}
function filter(s, status, origin = 'all') {
  s.context.listStatus = status; s.context.listOrigin = origin;
  assert.equal(s.run('setRuntimeJobFilters(listStatus,listOrigin)'), true);
  return JSON.parse(s.run('JSON.stringify(runtimeJobsView())'));
}
async function statusOracle(override) {
  const s = fixture(override); await discover(s);
  const untouched = s.run('state.runtimeJobs[1]');
  await exact(s, job({status:'completed', can_cancel:false, success:true, exit_code:0}));
  assert.equal(s.run('state.runtimeJob.status'), 'completed');
  assert.equal(filter(s, 'completed').length, 1, 'EXACT_STATE_MUST_RECONCILE_RETAINED_ROW');
  assert.equal(filter(s, 'working').length, 1);
  assert.equal(s.run('state.runtimeJobs[1]'), untouched);
  assert.equal(s.requests.length, 2);
  assert.ok(s.requests.every(request => request.options.method === 'GET'));
  return s;
}
async function cancellationOracle(override) {
  const s = fixture(override); await discover(s); await exact(s);
  const pending = s.run('cancelRuntimeJob()'); await flush();
  const request = s.requests.at(-1);
  assert.equal(request.url, '/intelligence/jobs/' + A + '/cancel');
  assert.equal(request.options.method, 'POST');
  assert.deepEqual(JSON.parse(request.options.body), {});
  respond(request, job({status:'cancelled', can_cancel:false, success:false}));
  assert.equal(await pending, true);
  assert.equal(filter(s, 'cancelled').length, 1, 'CANCEL_RECEIPT_MUST_RECONCILE_RETAINED_ROW');
  assert.equal(filter(s, 'working').length, 1);
  assert.equal(s.run('state.runtimeJobs[0].can_cancel'), false);
  assert.equal(s.requests.length, 3);
  assert.equal(s.run('state.runtimeJob.acceptance_ready'), false);
}
test('accepted exact observations reconcile retained rows and all lifecycle filters', async () => {
  await statusOracle();
  for (const status of ['failed','cancelled','input_required','unknown']) {
    const s = fixture(); await discover(s);
    await exact(s, job({status, can_cancel:false, success:['failed','cancelled'].includes(status) ? false : null}));
    assert.equal(filter(s, status).length, 1, status);
    assert.equal(filter(s, 'working').length, 1, status);
    assert.equal(s.run('state.runtimeJobs[0].created_at_ms'), 1);
    assert.equal(s.run('state.runtimeJobs[0].updated_at_ms'), 2, 'exact response must not invent timestamps');
    assert.equal(s.run('state.runtimeJob.acceptance_ready'), false);
    assert.equal(s.requests.length, 2);
  }
});
test('a valid cancellation receipt reconciles only the existing exact retained row', async () => cancellationOracle());
test('exact observation of a selected ID outside the bounded list does not fabricate discovery', async () => {
  const s = fixture(); await discover(s); await exact(s);
  await discover(s, [row(B)], true);
  await exact(s, job({status:'completed', can_cancel:false, success:true, exit_code:0}));
  assert.equal(s.run('state.runtimeJob.task_id'), A);
  assert.deepEqual(JSON.parse(s.run('JSON.stringify(state.runtimeJobs.map(item => item.task_id))')), [B]);
  assert.equal(filter(s, 'completed').length, 0);
  assert.equal(s.run('state.runtimeJobsTruncated'), true);
  assert.equal(s.run('state.runtimeJobsLoaded'), true);
  assert.equal(s.requests.length, 4);
});
test('a newer retained list wins over older selected detail without sticky terminal states', async () => {
  for (const [previous, latest] of [['working','completed'], ['completed','failed']]) {
    const s = fixture(); await discover(s);
    await exact(s, job({status:previous, can_cancel:previous === 'working'}));
    await discover(s, [row(A, {status:latest, can_cancel:false})]);
    assert.equal(s.run('state.runtimeJob.status'), previous, 'detail remains its own older observation');
    s.run('renderRuntimeJobs();');
    assert.equal(filter(s, latest).length, 1);
    assert.equal(filter(s, previous).length, 0);
    assert.equal(s.requests.length, 3);
  }
});
test('pending discovery coalesces exact reads and an obsolete discovery cannot overwrite a newer terminal observation', async () => {
  const serial = fixture(); await discover(serial);
  const pending = serial.run('refreshRuntimeJobs()'); await flush();
  const sequence = serial.run('state.runtimeJobsSequence');
  assert.equal(await serial.run('observeRuntimeJob(' + JSON.stringify(A) + ')'), false);
  assert.equal(serial.requests.length, 2, 'a same-scope exact GET cannot pass an in-flight list GET');
  assert.equal(serial.run('state.runtimeJobsSequence'), sequence);
  respond(serial.requests.at(-1), {schema_version:1, kind:'command_job_list', workspace:'A',
    items:[row(A, {status:'completed', can_cancel:false})], truncated:false, coverage:'retained_bounded', discovery_is_not_execution:true});
  assert.equal(await pending, true);
  assert.equal(filter(serial, 'completed').length, 1);

  const s = fixture(); await discover(s);
  const oldPending = s.run('refreshRuntimeJobs()'); await flush(); const oldRequest = s.requests.at(-1);
  s.run('state.current="B";state.workspaceEpoch++;clearRuntimeJobs();state.current="A";state.workspaceEpoch++;clearRuntimeJobs();');
  await discover(s); await exact(s, job({status:'completed', can_cancel:false, success:true, exit_code:0}));
  respond(oldRequest, {schema_version:1, kind:'command_job_list', workspace:'A',
    items:[row()], truncated:false, coverage:'retained_bounded', discovery_is_not_execution:true});
  assert.equal(await oldPending, false);
  assert.equal(s.run('state.runtimeJob.status'), 'completed');
  assert.equal(filter(s, 'completed').length, 1);
  assert.equal(filter(s, 'working').length, 1, 'only the other current retained job remains working');
});
test('late exact and cancellation replies cannot reconcile a replacement scope or newer history', async () => {
  for (const cancel of [false, true]) {
    const s = fixture(); await discover(s);
    if (cancel) await exact(s);
    const pending = s.run(cancel ? 'cancelRuntimeJob()' : 'observeRuntimeJob(' + JSON.stringify(A) + ')');
    await flush(); const old = s.requests.at(-1);
    s.run('state.current="B";state.workspaceEpoch++;clearRuntimeJobs();state.current="A";state.workspaceEpoch++;clearRuntimeJobs();');
    await discover(s, [row(A, {status:'failed', can_cancel:false})]);
    respond(old, job({status:cancel ? 'cancelled' : 'completed', can_cancel:false, success:false}));
    assert.equal(await pending, false);
    assert.equal(filter(s, 'failed').length, 1);
    assert.equal(s.run('state.runtimeJob'), null);
    assert.equal(s.run('state.runtimeJobs[0].can_cancel'), false);
  }
});
test('malformed exact replies and unconfirmed cancellation do not alter retained rows or grant success', async () => {
  for (const cancel of [false, true]) for (const patch of [{workspace:'B'}, {task_id:B}, {kind:'foreign'}, {origin:'mcp', can_cancel:true}]) {
    const s = fixture(); await discover(s); if (cancel) await exact(s);
    const before = s.run('JSON.stringify(state.runtimeJobs)');
    const pending = s.run(cancel ? 'cancelRuntimeJob()' : 'observeRuntimeJob(' + JSON.stringify(A) + ')'); await flush();
    respond(s.requests.at(-1), job(patch));
    assert.equal(await pending, false);
    assert.equal(s.run('JSON.stringify(state.runtimeJobs)'), before);
    assert.equal(s.run('state.runtimeJob?.acceptance_ready === true'), false);
    if (cancel) {
      assert.equal(s.run('state.runtimeJobsUncertain'), true);
      assert.equal(await s.run('cancelRuntimeJob()'), false);
      assert.equal(s.requests.length, 3);
    }
  }
});

test('ablation of exact-observation reconciliation restores the wrong lifecycle filter', async () => {
  await statusOracle();
  const mutant = withoutReconciliation('observeRuntimeJob', 'async function cancelRuntimeJob(');
  for (const newline of ['\n', '\r\n']) {
    await assert.rejects(statusOracle(mutant.replace(/\r?\n/g, newline)),
      error => error.code === 'ERR_ASSERTION' && /EXACT_STATE_MUST_RECONCILE_RETAINED_ROW/.test(error.message));
  }
});
test('ablation of cancellation reconciliation restores the stale working row', async () => {
  await cancellationOracle();
  const mutant = withoutReconciliation('cancelRuntimeJob', 'function scheduleRuntimeJobObservation(');
  for (const newline of ['\n', '\r\n']) {
    await assert.rejects(cancellationOracle(mutant.replace(/\r?\n/g, newline)),
      error => error.code === 'ERR_ASSERTION' && /CANCEL_RECEIPT_MUST_RECONCILE_RETAINED_ROW/.test(error.message));
  }
});
