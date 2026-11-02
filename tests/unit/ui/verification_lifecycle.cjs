'use strict';
// Exercise the shipped bundle's navigation and visibility handlers; no native command runs.
const assert = require('node:assert/strict');
const { sandbox, respond, flush } = require('./observatory.cjs');
const { fixture } = require('./acceptance.cjs');
const { task } = require('./verification.cjs');

function prepared(change = {}) {
  const s = sandbox(false, true, { fakeTimers: true });
  s.context.fixture = fixture(); s.context.receipt = task(change);
  s.run('state.project=fixture;state.workspaceTab="proof";state.autoRefresh=false;'
    + 'entry=verificationEntry(true);entry.task=receipt;entry.expectedRevision=receipt.requested_revision;entry.polls=17;');
  return s;
}
function timer(s) {
  const value = [...s.timers.values()].find(item => item.ms === 2000);
  assert.ok(value, 'same-task observation timer is present'); return value;
}
function hide(s, hidden) {
  s.context.document.hidden = hidden;
  s.events.visibilitychange();
}
function onlyStatusReads(s, id = 'task-native-1') {
  assert.ok(s.requests.every(row => row.options.method === 'GET'
    && row.url === '/intelligence/verification/' + id), 'lifecycle only observes the retained exact ID');
}
async function main() {
  const results = [];
  async function test(name, fn) {
    try { await fn(); results.push({ name, passed: true }); }
    catch (error) { results.push({ name, passed: false, error: error.stack }); }
  }
  await test('leaving Proof pauses and invalidates an already queued verification timer', async () => {
    const s = prepared(); s.run('scheduleVerificationObservation(entry)'); const queued = timer(s);
    s.run('activateWorkspaceTab("overview")');
    assert.equal(s.timers.size, 0);
    queued.fn(); await flush(); assert.equal(s.requests.length, 0);
    assert.equal(s.run('entry.polls'), 17);
  });
  await test('returning to Proof resumes only the retained exact task without renewing its budget', async () => {
    const s = prepared();
    s.run('state.workspaceTab="overview";scheduleVerificationObservation(entry);activateWorkspaceTab("proof")');
    assert.equal(s.timers.size, 1); assert.equal(s.run('entry.polls'), 17);
    const queued = timer(s); s.timers.clear(); queued.fn(); await flush();
    onlyStatusReads(s); assert.equal(s.requests.length, 1);
    respond(s.requests[0], task()); await flush();
    assert.equal(s.run('entry.polls'), 18); assert.equal(s.timers.size, 1);
  });
  await test('visibility pauses queued reads and resumes Proof with the same remaining budget', async () => {
    const s = prepared(); s.run('scheduleVerificationObservation(entry)'); const queued = timer(s);
    hide(s, true); assert.equal(s.timers.size, 0);
    queued.fn(); await flush(); assert.equal(s.requests.length, 0);
    hide(s, false); assert.equal(s.timers.size, 1); assert.equal(s.run('entry.polls'), 17);
    const next = timer(s); queued.fn(); await flush();
    assert.equal(s.requests.length, 0, 'a prior callback cannot consume the replacement visible timer');
    assert.equal(timer(s), next);
    s.timers.clear(); next.fn(); await flush();
    onlyStatusReads(s); respond(s.requests[0], task()); await flush();
    assert.equal(s.run('entry.polls'), 18);
  });
  await test('a status reply settling while hidden resumes after visibility returns', async () => {
    const s = prepared(); const pending = s.run('observeVerificationTask()'); await flush();
    hide(s, true); respond(s.requests[0], task()); await pending;
    assert.equal(s.timers.size, 0); assert.equal(s.run('entry.polls'), 18);
    hide(s, false); assert.equal(s.timers.size, 1);
    const next = timer(s); s.timers.clear(); next.fn(); await flush();
    onlyStatusReads(s); respond(s.requests[1], task()); await flush();
    assert.equal(s.run('entry.polls'), 19);
  });
  await test('budget error approval busy and settled terminal states stay paused across lifecycle changes', () => {
    for (const setup of ['entry.polls=120', 'entry.error="Observation failed"',
      'entry.error="Cancellation was not confirmed"', 'entry.busy=true',
      'entry.task.status="input_required";entry.task.authorization_required=true',
      'entry.task.status="completed";entry.task.terminal=true;entry.refreshed=entry.task.task_id']) {
      const s = prepared(); s.run(setup);
      s.run('activateWorkspaceTab("overview");activateWorkspaceTab("proof")'); hide(s, true); hide(s, false);
      assert.equal(s.timers.size, 0, setup); assert.equal(s.requests.length, 0, setup);
    }
    const empty = sandbox(false, true, { fakeTimers: true });
    empty.run('state.autoRefresh=false;activateWorkspaceTab("proof")'); hide(empty, true); hide(empty, false);
    assert.equal(empty.run('state.verificationTasks.size'), 0); assert.equal(empty.requests.length, 0);
  });
  await test('foreign or replaced entries cannot schedule or use a replacement task through a queued callback', async () => {
    for (const setup of [
      'state.verificationTasks.set("A",{...entry,task:{...entry.task,task_id:"replacement"}})',
      'entry.task={...entry.task,task_id:"replacement"}',
      'entry.sequence++',
      'entry.owner="foreign-owner"']) {
      const s = prepared(); s.run('scheduleVerificationObservation(entry)'); const queued = timer(s);
      s.timers.clear(); s.run(setup); queued.fn(); await flush();
      assert.equal(s.requests.length, 0, setup);
    }
    const s = prepared(); s.run('state.verificationTasks.delete("A");scheduleVerificationObservation(entry)');
    assert.equal(s.timers.size, 0, 'detached entries cannot own timers');
  });
  await test('workspace A to B to A retains the task but rejects the old queued generation', async () => {
    const s = prepared(); s.run('scheduleVerificationObservation(entry)'); const old = timer(s);
    s.run('state.current="B";clearWorkspaceView();state.current="A";clearWorkspaceView();state.project=fixture;activateWorkspaceTab("proof")');
    assert.equal(s.run('entry.polls'), 17); assert.equal(s.timers.size, 1);
    old.fn(); await flush(); assert.equal(s.requests.length, 0);
    const current = timer(s); s.timers.clear(); current.fn(); await flush();
    onlyStatusReads(s); assert.equal(s.requests.length, 1);
    respond(s.requests[0], task()); await flush(); assert.equal(s.run('entry.polls'), 18);
  });
  await test('only an explicit status refresh renews an exhausted observation budget', async () => {
    const s = prepared(); s.run('entry.polls=120;activateWorkspaceTab("overview");activateWorkspaceTab("proof")');
    hide(s, true); hide(s, false); assert.equal(s.timers.size, 0); assert.equal(s.run('entry.polls'), 120);
    const pending = s.run('observeVerificationTask({manual:true})'); await flush();
    onlyStatusReads(s); respond(s.requests[0], task()); assert.equal(await pending, true);
    assert.equal(s.run('entry.polls'), 1); assert.equal(s.timers.size, 1);
  });
  console.log(JSON.stringify({ suite: 'native-verification-lifecycle', results }, null, 2));
  assert.equal(results.length, 8);
  assert.ok(results.every(row => row.passed), results.filter(row => !row.passed).map(row => row.name).join('\n'));
}
if (require.main === module) {
  const keepAlive = setInterval(() => {}, 1000);
  main().catch(error => { console.error(error); process.exitCode = 1; }).finally(() => clearInterval(keepAlive));
}
