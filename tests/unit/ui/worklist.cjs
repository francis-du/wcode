'use strict';
const assert = require('node:assert/strict');
const { sandbox, respond, flush } = require('./observatory.cjs');
const task = (id, status = 'pending', extra = {}) => ({ id, title: id, status, ...extra });
function prepared(items = [], extra = {}) {
  const s = sandbox(false, true, { fakeTimers: true });
  const host = s.node('#executionStatus');
  host.querySelector = selector => s.node(selector);
  s.context.fixture = { workspace: 'A', execution: { available: true, exists: false,
    worklist: { available: true, exists: true, revision: 1, items_included: true, items, ...extra } } };
  s.run('state.project=fixture;renderExecutionStatus();');
  return s;
}
const html = s => s.node('#executionStatus').innerHTML;
const ids = s => [...html(s).matchAll(/data-worklist-task="([^"]+)"/g)].map(match => match[1]);
function update(s, filter, query = '', page = 0) {
  s.context.nextView = { filter, query, page };
  return s.run('setWorklistView(nextView.filter,nextView.query,nextView.page)');
}
async function main() {
  const results = [];
  async function test(name, fn) { try { await fn(); results.push({ name, passed: true }); } catch (error) { results.push({ name, passed: false, error: error.stack }); } }
  await test('unfinished work stays visible when sixty completed reports precede it', () => {
    const archived = Array.from({ length: 60 }, (_, i) => task('archive-' + i, 'done', { result: { outcome: 'complete', summary: 'Old report' } }));
    const s = prepared([...archived, task('repair-ci', 'blocked'), task('build-ui', 'in_progress'), task('next-work'), task('future-work', 'future_state')]);
    for (const id of ['repair-ci', 'build-ui', 'next-work', 'future-work']) assert.match(html(s), new RegExp(id));
    assert.doesNotMatch(html(s), /data-worklist-task="archive-/);
    assert.match(html(s), /Unknown task state/);
  });
  await test('all observed tasks remain reachable through bounded pages', () => {
    const items = Array.from({ length: 25 }, (_, i) => task('task-' + i, i % 2 ? 'done' : 'pending'));
    const s = prepared(items), seen = [];
    for (let page = 0; page < 4; page++) {
      assert.equal(update(s, 'all', '', page), true);
      assert.ok(ids(s).length <= 8); seen.push(...ids(s));
    }
    assert.equal(new Set(seen).size, 25);
    assert.deepEqual([...new Set(seen)].sort(), items.map(item => item.id).sort());
    assert.equal(s.requests.length, 0);
  });
  await test('status and literal search filter retained tasks without changing their state', () => {
    const s = prepared([task('alpha', 'blocked', { note: 'Needs compiler adapter', write_paths: ['src/alpha.rs'] }), task('beta', 'done'), task('gamma', 'pending', { claim: { actor: 'reviewer', expired: true, expires_at_ms: 1 } })]);
    assert.equal(update(s, 'blocked', 'COMPILER'), true); assert.deepEqual(ids(s), ['alpha']);
    assert.equal(update(s, 'all', 'src/alpha.rs'), true); assert.deepEqual(ids(s), ['alpha']);
    assert.equal(update(s, 'open', 'reviewer'), true); assert.deepEqual(ids(s), ['gamma']);
    assert.equal(update(s, 'passed'), false); assert.equal(update(s, 'all', '', -1), false);
    assert.equal(s.run('fixture.execution.worklist.items[0].status'), 'blocked');
    assert.equal(s.requests.length, 0);
  });
  await test('dependencies resolve from the complete observation before filtering and paging', () => {
    const s = prepared([task('hidden-done', 'done'), ...Array.from({ length: 10 }, (_, i) => task('history-' + i, 'done')), task('dependent', 'pending', { depends_on: ['hidden-done'] })]);
    assert.deepEqual(ids(s), ['dependent']);
    assert.match(html(s), /Dependencies satisfied/);
    assert.doesNotMatch(html(s), /Reported runnable/);
  });
  await test('missing partial and malformed dependency observations stay distinct', () => {
    const s = prepared([task('dependent', 'pending', { depends_on: ['missing'] })]);
    assert.match(html(s), /Missing dependency/);
    s.run('fixture.execution.worklist.truncated=true;renderExecutionStatus();');
    assert.match(html(s), /Dependency not observed/);
    s.run('fixture.execution.worklist.items[0].depends_on="missing";renderExecutionStatus();');
    assert.match(html(s), /Dependency state unknown/);
    assert.doesNotMatch(html(s), /Dependencies satisfied|No dependencies/);
  });
  await test('reported runnable lanes require a current complete observation and never create actions', () => {
    const s = prepared([task('ready')], { runnable: ['ready'], truncated: false });
    assert.match(html(s), /Reported runnable/);
    s.run('state.snapshotStale=true;renderExecutionStatus();');
    assert.doesNotMatch(html(s), /Reported runnable/); assert.match(html(s), /Snapshot stale/);
    s.run('state.snapshotStale=false;state.syncError=true;renderExecutionStatus();');
    assert.doesNotMatch(html(s), /Reported runnable/);
    s.run('state.syncError=false;fixture.execution.worklist.truncated=true;renderExecutionStatus();');
    assert.doesNotMatch(html(s), /Reported runnable/);
    assert.equal(s.requests.length, 0);
  });
  await test('worker reports do not settle reopened tasks or manufacture verification', () => {
    const s = prepared([task('reopened', 'pending', { result: { actor: 'worker', outcome: 'complete', summary: 'Previous report', evidence: [{ id: 'E1' }] } })]);
    assert.deepEqual(ids(s), ['reopened']); assert.match(html(s), /Pending/);
    assert.match(html(s), /Worker reports complete/); assert.match(html(s), /independent verification/);
    assert.equal(s.run('fixture.execution.worklist.items[0].status'), 'pending');
  });
  await test('incomplete summaries never become an empty work queue', () => {
    for (const extra of [{ items_included: false }, { truncated: true }, { counts: { pending: 3, in_progress: 0, done: 0, blocked: 0 } }]) {
      const s = prepared([], extra);
      assert.match(html(s), /unknown|incomplete|Partial/i);
      assert.doesNotMatch(html(s), /No items in the observed Worklist/);
    }
  });
  await test('duplicate IDs and invalid task records cannot claim dependency satisfaction', () => {
    for (const items of [[task('same'), task('same', 'done')], [task(''), task('valid')], [null]]) {
      const s = prepared(items); assert.match(html(s), /Ownership state is unknown/);
      assert.doesNotMatch(html(s), /Dependencies satisfied|Reported runnable/);
    }
  });
  await test('empty search is different from empty complete or unavailable observations', () => {
    const s = prepared([task('one')]); update(s, 'open', 'no-such-task');
    assert.match(html(s), /No observed tasks match/); assert.doesNotMatch(html(s), /No items in the observed Worklist/);
    assert.match(html(prepared([])), /No items in the observed Worklist/);
    assert.match(html(prepared([], { available: false })), /Ownership state is unknown/);
  });
  await test('same-workspace revisions preserve filters while workspace and owner changes clear them', () => {
    const s = prepared(Array.from({ length: 20 }, (_, i) => task('task-' + i)));
    update(s, 'all', 'task', 2); s.run('fixture.execution.worklist.revision++;renderExecutionStatus();');
    assert.equal(s.run('worklistViewScope().query'), 'task');
    assert.equal(s.run('worklistViewScope().page'), 0);
    s.run('globalThis.oldScope=worklistViewScope();state.current="B";state.workspaceEpoch++;fixture.workspace="B";renderExecutionStatus();');
    assert.equal(s.run('worklistViewScope().query'), '');
    assert.equal(s.run('setWorklistView("done","old",1,oldScope)'), false);
    update(s, 'all', 'new'); s.run('worklistViewScope().owner="other";renderExecutionStatus();');
    assert.equal(s.run('worklistViewScope().query'), ''); assert.equal(s.requests.length, 0);
  });
  await test('real filter controls update the view and obsolete handlers are inert', () => {
    const s = prepared([task('one'), task('two', 'done')]);
    const select = s.node('[data-worklist-filter]'), search = s.node('[data-worklist-search]');
    assert.equal(typeof select.events.change, 'function');
    select.value = 'done'; select.events.change(); assert.deepEqual(ids(s), ['two']);
    search.value = 'not-there'; search.events.input(); assert.match(html(s), /No observed tasks match/);
    const oldChange = select.events.change;
    s.run('state.current="B";state.workspaceEpoch++;fixture.workspace="B";renderExecutionStatus();');
    select.value = 'done'; oldChange();
    assert.equal(s.run('worklistViewScope().filter'), 'open');
    assert.match(html(s), /aria-label=/); assert.equal(s.requests.length, 0);
  });
  await test('all public strings stay escaped and private claim tokens stay outside the view', () => {
    const s = prepared([task('safe', 'blocked', { title: '<img src=x onerror=alert(1)>', note: '<script>note</script>', depends_on: ['<dependency>'], write_paths: ['<path>'], claim: { actor: '<actor>', expired: true, expires_at_ms: 1, claim_id: 'PRIVATE_CLAIM' }, result: { outcome: 'complete', summary: '<script>result</script>', claim_id: 'PRIVATE_CLAIM' } })]);
    assert.match(html(s), /&lt;script&gt;note/); assert.match(html(s), /&lt;dependency&gt;/);
    assert.doesNotMatch(html(s), /<script>|<img|PRIVATE_CLAIM|claim_id/);
  });
  await test('historical observations retain inspection without active ownership or runnable claims', () => {
    const item = task('past', 'in_progress', { claim: { actor: 'worker', expired: false, expires_at_ms: Date.now() + 60000 } });
    const s = prepared([], { available: false, last_known: { available: true, exists: true, revision: 7, items: [item], runnable: ['past'] } });
    assert.match(html(s), /Historical ownership/); assert.match(html(s), /Last observed Worklist #7/);
    assert.doesNotMatch(html(s), /Reported runnable|>Claimed<\/span>/);
  });
  await test('clock expiry is re-evaluated even when the Worklist revision does not change', () => {
    const s = prepared([task('clock', 'in_progress', { claim: { actor: 'worker', expired: false, expires_at_ms: Date.now() + 60000 } })]);
    assert.match(html(s), />Claimed<\/span>/);
    s.context.futureNow = Date.now() + 120000;
    s.run('globalThis.Date=class extends Date { static now(){ return futureNow; } };renderExecutionStatus();');
    assert.match(html(s), /Lease expired/); assert.equal(s.run('fixture.execution.worklist.revision'), 1);
  });
  await test('native sparse counts omit zero states without becoming partial coverage', () => {
    const s = prepared([task('native')], { counts: { pending: 1 }, runnable: ['native'], truncated: false });
    assert.match(html(s), /Reported runnable/); assert.doesNotMatch(html(s), /Partial Worklist response/);
    for (const counts of [{ pending: null }, { done: 1 }, [], { pending: 1, other: 2 }]) {
      s.context.nextCounts = counts; s.run('fixture.execution.worklist.counts=nextCounts;renderExecutionStatus();');
      assert.match(html(s), /Partial Worklist response/); assert.doesNotMatch(html(s), /Reported runnable/);
    }
  });
  await test('a real revision failure withdraws runnable labels without a manual render', async () => {
    const s = prepared([task('ready')], { counts: { pending: 1 }, runnable: ['ready'], truncated: false });
    assert.match(html(s), /Reported runnable/);
    s.run('showRefreshFailure(new Error("fixture offline"));');
    assert.equal(s.run('state.syncError'), true); assert.doesNotMatch(html(s), /Reported runnable/);
  });
  await test('unchanged revision polling refreshes time-sensitive lease observations', async () => {
    const s = prepared([task('clock', 'in_progress', { claim: { actor: 'worker', expired: false, expires_at_ms: Date.now() + 60000 } })]);
    assert.match(html(s), />Claimed<\/span>/);
    s.context.futureNow = Date.now() + 120000;
    s.run('state.workspaceTab="activity";state.revisionKey=revisionKey({fingerprint:"same"});globalThis.Date=class extends Date { static now(){return futureNow;} };');
    const pending = s.run('pollRevision()'); await flush();
    respond(s.requests.at(-1), { workspace: 'A', fingerprint: 'same' }); await pending;
    assert.match(html(s), /Lease expired/);
    assert.equal(s.requests.filter(request => request.url === '/intelligence/project').length, 0);
  });
  await test('composition defers same-scope replacement until the committed search is available', () => {
    const s = prepared([task('work', 'pending', { title: '工程任务' })]);
    const search = s.node('[data-worklist-search]'), before = html(s);
    search.events.compositionstart(); search.value = '工程';
    search.events.input({ isComposing: true });
    s.run('fixture.execution.worklist.revision++;renderExecutionStatus();');
    assert.equal(html(s), before); assert.equal(s.run('worklistViewScope().query'), '');
    search.events.compositionend();
    assert.equal(s.run('worklistViewScope().query'), '工程'); assert.equal(s.run('worklistViewScope().composing'), false);
    assert.deepEqual(ids(s), ['work']);
  });
  console.log(JSON.stringify({ suite: 'engineering-work-queue', results }, null, 2));
  if (results.some(result => !result.passed)) process.exitCode = 1;
}
if (require.main === module) main().catch(error => { console.error(error); process.exitCode = 1; });
