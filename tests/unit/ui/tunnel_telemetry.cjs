'use strict';
// Tunnel health is bounded observation: a dashboard link must preserve the
// fragment credentials, a slow or malformed response must never publish fake
// health, and an obsolete response must not unlock a newer request.
const assert = require('node:assert/strict');
const { test } = require('node:test');
const { sandbox, project, respond, flush } = require('./observatory.cjs');

const healthyTunnel = (provider = 'fixture') => ({
  public_url_healthy: true,
  public_endpoint: 'ready',
  tunnels: [{ provider, role: 'active', state: 'verified', url: 'https://example.test' }],
});

test('every active tunnel is a dashboard link and it preserves fragment credentials', async () => {
  const s = sandbox(false, true, { fakeTimers: true, controlTunnels: true });
  const request = s.run('refreshTunnels()'); await flush();
  respond(s.requests[0], {
    public_url_healthy: true,
    public_endpoint: 'ready',
    tunnels: [
      { provider: 'primary', role: 'active', state: 'healthy', url: 'https://primary.example' },
      { provider: 'standby', role: 'active', state: 'verified', url: 'https://standby.example' },
    ],
  }); await request;
  const html = s.node('#tunnels').innerHTML;
  assert.ok(html.includes('href="https://primary.example/intelligence#token=test-ui&amp;workspace=A"'));
  assert.ok(html.includes('standby · active'));
  assert.ok(html.includes('href="https://standby.example'));
  assert.ok(html.includes('title="https://standby.example · active · verified'));
});

test('tunnel requests time out and allow a later retry', async () => {
  const s = sandbox(false, true, { fakeTimers: true, controlTunnels: true });
  const first = s.run('refreshTunnels()'); await flush();
  const request = s.requests[0], deadline = [...s.timers.values()].find(timer => timer.ms === 20000);
  assert.ok(deadline, 'health requests need a bounded deadline');
  request.options.signal.addEventListener('abort', () => request.reject(new DOMException('aborted', 'AbortError')), { once: true });
  deadline.fn(); await first;
  assert.equal(s.run('state.tunnelBusy'), false); assert.equal(s.run('state.tunnelSnapshot'), null);
  assert.ok(s.node('#tunnels').innerHTML.includes('unavailable'));
  const next = s.run('refreshTunnels()'); await flush(); respond(s.requests[1], healthyTunnel()); await next;
  assert.equal(s.run('state.tunnelSnapshot.public_url_healthy'), true); assert.equal(s.timers.size, 0);
  assert.equal(s.requests[1].options.cache, 'no-store');
});

test('old tunnel completion cannot overwrite or unlock a newer request', async () => {
  const s = sandbox(false, true, { fakeTimers: true, controlTunnels: true });
  const old = s.run('refreshTunnels()'); await flush();
  s.run('state.current="B";clearWorkspaceView({preserveDom:true});');
  const current = s.run('refreshTunnels()'); await flush();
  assert.equal(s.requests[0].options.signal.aborted, true);
  respond(s.requests[0], healthyTunnel('obsolete')); await old;
  assert.equal(s.run('state.tunnelBusy'), true); assert.equal(s.run('state.tunnelSnapshot'), null);
  await s.run('refreshTunnels()'); assert.equal(s.requests.length, 2);
  respond(s.requests[1], healthyTunnel('current')); await current;
  assert.equal(s.run('state.tunnelSnapshot.tunnels[0].provider'), 'current'); assert.equal(s.timers.size, 0);
});

test('hidden tabs cancel tunnel requests and their deadlines', async () => {
  const s = sandbox(false, true, { fakeTimers: true, controlTunnels: true });
  const request = s.run('refreshTunnels()'); await flush();
  s.context.document.hidden = true; s.events.visibilitychange();
  assert.equal(s.requests[0].options.signal.aborted, true); assert.equal(s.timers.size, 0);
  respond(s.requests[0], healthyTunnel('hidden')); await request;
  assert.equal(s.run('state.tunnelSnapshot'), null); assert.equal(s.run('state.tunnelBusy'), false);
  await s.run('refreshTunnels()'); assert.equal(s.requests.length, 1);
});

test('failed tunnel refresh clears stale healthy telemetry', async () => {
  const s = sandbox(false, true, { fakeTimers: true, controlTunnels: true });
  s.context.fixture = project(); s.context.health = healthyTunnel('obsolete');
  s.run('state.project=fixture;state.tunnelSnapshot=health;renderRuntimeTopology();');
  const request = s.run('refreshTunnels()'); await flush(); respond(s.requests[0], { error: 'offline' }, false); await request;
  assert.equal(s.run('state.tunnelSnapshot'), null); assert.ok(s.node('#tunnels').innerHTML.includes('unavailable'));
  assert.ok(s.node('#runtimeTopology').innerHTML.includes('telemetry unavailable'));
  assert.ok(!s.node('#runtimeTopology').innerHTML.includes('obsolete'));
});

test('malformed tunnel payloads are not published as current state', async () => {
  for (const payload of [null, [], { tunnels: 'not-an-array' }, { tunnels: [null] }]) {
    const s = sandbox(false, true, { fakeTimers: true, controlTunnels: true });
    const request = s.run('refreshTunnels()'); await flush(); respond(s.requests[0], payload); await request;
    assert.equal(s.run('state.tunnelSnapshot'), null); assert.equal(s.run('state.tunnelBusy'), false);
    assert.ok(s.node('#tunnels').innerHTML.includes('unavailable')); assert.equal(s.timers.size, 0);
  }
});

test('tunnel polling stays single-flight while the body is pending', async () => {
  const s = sandbox(false, true, { fakeTimers: true, controlTunnels: true });
  const first = s.run('refreshTunnels()'); await flush(); let complete;
  s.requests[0].resolve({ ok: true, status: 200, json: () => new Promise(resolve => { complete = resolve; }) }); await flush();
  await s.run('refreshTunnels()'); assert.equal(s.requests.length, 1);
  assert.ok([...s.timers.values()].some(timer => timer.ms === 20000));
  complete(healthyTunnel()); await first; assert.equal(s.run('state.tunnelBusy'), false); assert.equal(s.timers.size, 0);
});

test('unhealthy endpoints and unknown approval state are not green', async () => {
  const s = sandbox(); s.context.fixture = project();
  s.context.health = { ...healthyTunnel(), public_url_healthy: false }; s.context.health.tunnels[0].state = 'quarantined';
  s.run('state.project=fixture;state.tunnelSnapshot=health;renderRuntimeTopology();');
  const html = s.node('#runtimeTopology').innerHTML;
  assert.ok(html.includes('runtime-status-card warn"><span>Endpoint'));
  assert.ok(html.includes('runtime-status-card info"><span>OAuth &amp; MCP'));
  s.run('state.tunnelSnapshot.public_url_healthy=true;state.tunnelSnapshot.tunnels[0].state="verified";renderRuntimeTopology();');
  assert.ok(s.node('#runtimeTopology').innerHTML.includes('runtime-status-card good"><span>Endpoint'));
});

test('slow tunnel requests do not delay activity telemetry', async () => {
  const s = sandbox(false, true, { fakeTimers: true, controlTunnels: true });
  const tunnel = s.run('refreshTunnels()'); const activity = s.run('refreshActivity()'); await flush();
  const request = s.requests.find(item => item.url === '/intelligence/activity'); assert.ok(request);
  respond(request, { workspace: 'A', pending_authorizations: 0, activity: { available: true, active: 3, recent: [] } }); await activity;
  assert.equal(s.run('state.activitySnapshot.activity.active'), 3); assert.equal(s.run('state.tunnelBusy'), true);
  respond(s.requests.find(item => item.url === '/healthz'), healthyTunnel()); await tunnel;
});
