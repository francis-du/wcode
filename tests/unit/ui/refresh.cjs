'use strict';
const assert = require('node:assert/strict');
const {sandbox, project, respond, flush} = require('./observatory.cjs');
const results = [];
const test = async (name, fn) => {
  try { await fn(); results.push({name, passed: true}); }
  catch (error) { results.push({name, passed: false, error: error.stack}); }
};
function setup(authenticated = true) {
  const s = sandbox(false, authenticated, {fakeTimers: true});
  s.context.console = {...console, warn() {}};
  return s;
}
function http(request, status, body = {error: 'PRIVATE path/token/stack'}) {
  request.resolve({ok: false, status, json: async () => body});
}
function abortOnDeadline(request) {
  request.options.signal.addEventListener('abort', () => request.reject(new DOMException('aborted', 'AbortError')), {once: true});
}
async function main() {
  await test('401 provides reauthorization rather than network troubleshooting', async () => {
    for (const language of ['en', 'zh-CN']) {
      const s = setup(); s.context.language = language; s.run('state.language=language;');
      const refresh = s.run('refreshProject({reason:"manual"})'); await flush();
      http(s.requests[0], 401); assert.equal(await refresh, false);
      assert.ok(s.node('#syncState').textContent.includes('401'));
      const html = s.node('#architectureBlueprint').innerHTML;
      assert.ok(html.includes(language === 'en' ? 'wcode terminal' : 'wcode 终端'));
      assert.ok(!html.includes(language === 'en' ? 'Check the connection' : '请检查连接'));
      assert.ok(!html.includes('PRIVATE'));
    }
  });
  await test('403 preserves status and explains the trusted entry point', async () => {
    const s = setup(); const refresh = s.run('refreshProject({reason:"manual"})'); await flush();
    http(s.requests[0], 403, {error: 'untrusted_host'}); await refresh;
    assert.ok(s.node('#syncState').textContent.includes('403'));
    assert.ok(s.node('#architectureBlueprint').innerHTML.includes('wcode terminal'));
    assert.ok(!s.node('#architectureBlueprint').innerHTML.includes('Check the connection'));
  });
  await test('missing credentials explain how to reconnect without sending requests', async () => {
    const s = setup(false); assert.equal(await s.run('refreshProject({reason:"manual"})'), false);
    assert.equal(s.requests.length, 0);
    assert.match(s.node('#syncState').textContent, /authorization|connect/i);
    assert.ok(s.node('#architectureBlueprint').innerHTML.includes('wcode terminal'));
  });
  await test('project snapshots have a bounded budget longer than the server Git review', async () => {
    const s = setup(); const refresh = s.run('refreshProject({reason:"manual"})'); await flush();
    assert.ok([...s.timers.values()].some(timer => timer.ms === 120000));
    assert.ok(![...s.timers.values()].some(timer => timer.ms === 30000));
    respond(s.requests[0], project()); assert.equal(await refresh, true);
    assert.equal(s.timers.size, 0);
  });
  await test('timeouts remain distinguishable from cancellation and HTTP failures', async () => {
    const s = setup(); const refresh = s.run('refreshProject({reason:"manual"})'); await flush();
    abortOnDeadline(s.requests[0]); [...s.timers.values()][0].fn();
    assert.equal(await refresh, false);
    assert.match(s.node('#syncState').textContent, /timed out/i);
    assert.equal(s.run('state.syncFailure.code'), 'timeout');
    assert.equal(s.run('state.inFlight'), false);
    assert.equal(s.node('#refresh').disabled, false);
    assert.equal(s.timers.size, 0);
  });
  await test('successful HTTP responses containing HTML are response errors, not offline', async () => {
    const s = setup(); const refresh = s.run('refreshProject({reason:"manual"})'); await flush();
    s.requests[0].resolve({ok: true, status: 200, json: async () => {throw new SyntaxError('PRIVATE html');}});
    assert.equal(await refresh, false);
    assert.match(s.node('#syncState').textContent, /invalid response/i);
    assert.ok(!s.node('#architectureBlueprint').innerHTML.includes('PRIVATE'));
  });
  await test('wrong workspace payload is rejected without publishing another project', async () => {
    const s = setup(); const refresh = s.run('refreshProject({reason:"manual"})'); await flush();
    respond(s.requests[0], project('B')); assert.equal(await refresh, false);
    assert.equal(s.run('state.project'), null);
    assert.match(s.node('#syncState').textContent, /invalid response/i);
  });
  await test('a network failure has actionable connection guidance', async () => {
    const s = setup(); const refresh = s.run('refreshProject({reason:"manual"})'); await flush();
    s.requests[0].reject(new TypeError('Failed to fetch')); await refresh;
    assert.match(s.node('#syncState').textContent, /connection/i);
    assert.ok(s.node('#architectureBlueprint').innerHTML.includes('connection'));
  });
  await test('render failures do not certify new revisions or reject their error handler', async () => {
    const s = setup(); s.context.fixture = project();
    s.run('state.project=fixture;state.revisionKey="old|||";state.lastUpdated=123;state.lastChecked=124;cacheWorkspaceSnapshot();renderProject=()=>{throw new Error("PRIVATE render");};renderAttention=()=>{throw new Error("PRIVATE attention");};');
    const refresh = s.run('refreshProject({reason:"manual",revision:{fingerprint:"new"}})'); await flush();
    respond(s.requests[0], {...project(), product: 'new snapshot'});
    assert.equal(await refresh, false);
    assert.equal(s.run('state.project === fixture'), true);
    assert.equal(s.run('state.revisionKey'), 'old|||');
    assert.equal(s.run('state.lastUpdated'), 123);
    assert.equal(s.run('state.lastChecked'), 124);
    assert.equal(s.run('state.projectCache.get("A").revisionKey'), 'old|||');
    assert.match(s.node('#syncState').textContent, /render/i);
    assert.ok(!s.node('#architectureBlueprint').innerHTML.includes('PRIVATE'));
    assert.equal(s.node('.observatory-main').attrs['aria-busy'], 'false');
  });
  await test('obsolete failures cannot replace a newer success or its diagnostics', async () => {
    const s = setup();
    const old = s.run('refreshProject({reason:"manual"})'); await flush();
    const next = s.run('refreshProject({reason:"manual"})'); await flush();
    respond(s.requests[1], project()); assert.equal(await next, true);
    http(s.requests[0], 401); assert.equal(await old, false);
    assert.equal(s.run('state.syncError'), false);
    assert.equal(s.run('state.syncFailure'), null);
    assert.equal(s.node('#syncState').textContent, 'Snapshot up to date');
  });
  await test('revision failures retain the snapshot and expose their HTTP status', async () => {
    const s = setup(); s.context.fixture = project(); s.run('state.project=fixture;');
    const poll = s.run('pollRevision()'); await flush(); http(s.requests[0], 503); await poll;
    assert.equal(s.run('state.project === fixture'), true);
    assert.equal(s.requests.length, 1);
    assert.ok(s.node('#syncState').textContent.includes('503'));
    assert.equal(s.run('state.syncFailure.phase'), 'request');
  });
  await test('explicit cancellation does not become a refresh failure', async () => {
    const s = setup(); const refresh = s.run('refreshProject({reason:"manual"})'); await flush();
    abortOnDeadline(s.requests[0]); s.run('state.controller.abort();');
    assert.equal(await refresh, false);
    assert.equal(s.run('state.syncError'), false);
    assert.equal(s.run('state.syncFailure'), null);
    assert.equal(s.timers.size, 0);
  });
  console.log(JSON.stringify({suite: 'observatory-refresh', results}, null, 2));
  assert.equal(results.length, 12);
  assert.ok(results.every(item => item.passed), 'refresh regressions failed');
}
main().catch(error => {console.error(error); process.exitCode = 1;});
