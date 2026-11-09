'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { sandbox, respond, flush } = require('./observatory.cjs');

const SESSION_ID = 'a'.repeat(64);
const OTHER_ID = 'b'.repeat(64);
const sessionsPayload = (sessions = [session()]) => ({ authority: 'local_operator', sessions });
const session = (patch = {}) => ({
  id: SESSION_ID,
  client_id: 'wcode-client',
  resource: 'https://example.com/mcp',
  access_count: 2,
  refresh_count: 1,
  access_expires_at_ms: 1_893_456_000_000,
  refresh_expires_at_ms: 1_893_456_000_000,
  ...patch,
});
function setup() {
  const s = sandbox(false, true, { fakeTimers: true });
  s.context.console = { ...console, warn() {} };
  return s;
}
function fail(request, status, body) {
  request.resolve({ ok: false, status, json: async () => body });
}
function loadListedSessions(s, sessions = [session()]) {
  s.context.sessions = sessionsPayload(sessions);
  s.run('state.sessions=sessions.sessions;state.sessionsLoaded=true;renderSessions();');
}
function armAndRevoke(s, id = SESSION_ID) {
  s.run(`armSessionRevoke(${JSON.stringify(id)})`);
  return s.run(`revokeSessionFromUi(${JSON.stringify(id)})`);
}

test('session listing shows bounded metadata and escapes untrusted fields', () => {
  const s = setup();
  loadListedSessions(s, [session({ client_id: '<script>client</script>' })]);
  const html = s.node('#sessionList').innerHTML;
  assert.match(html, /&lt;script&gt;client&lt;\/script&gt;/);
  assert.ok(!html.includes('<script>client</script>'));
  assert.ok(html.includes(`data-revoke-session="${SESSION_ID}"`));
  assert.ok(!html.includes('access_'), 'listings must never render token material');
  assert.ok(!html.includes('refresh_'), 'listings must never render token material');
  assert.ok(!html.includes('owner_'), 'listings must never render private grant owners');
  assert.ok(html.includes('2') && html.includes('1'), 'counts stay visible');
});

test('session status reflects the current time, not just the timestamps', () => {
  const s = setup();
  loadListedSessions(s, [session()]);
  assert.match(s.node('#sessionList').innerHTML, /Active/, 'a session whose tokens are ahead is active');
  loadListedSessions(s, [session({ refresh_expires_at_ms: 1 })]);
  assert.match(s.node('#sessionList').innerHTML, /Expired/, 'a dead refresh token ends the session');
  loadListedSessions(s, [session({ access_expires_at_ms: 1 })]);
  assert.match(s.node('#sessionList').innerHTML, /Access expired/,
    'an expired access token alone is not a dead session');
});

test('session revoke is confirmed explicitly and persists before success', async () => {
  const s = setup();
  loadListedSessions(s);
  const pending = armAndRevoke(s);
  await flush();
  assert.equal(s.requests.length, 1, 'only the confirmed revoke may be sent');
  assert.equal(s.requests[0].url, '/oauth/sessions/revoke');
  assert.equal(s.requests[0].options.method, 'POST');
  assert.equal(s.requests[0].options.headers['X-Wcode-UI-Token'], 'test-ui');
  assert.equal(JSON.parse(s.requests[0].options.body).session_id, SESSION_ID);
  respond(s.requests[0], { revoked: true, removed_access: 2, removed_refresh: 1 });
  await flush();
  respond(s.requests[1], sessionsPayload([]));
  await pending;
  assert.match(s.node('#sessionMessage').textContent, /Session revoked/);
  assert.match(s.node('#sessionMessage').textContent, /persisted/);
  assert.equal(s.run('state.sessions.length'), 0);
  assert.equal(s.run('state.sessionConfirm'), null);
});

test('unknown session revocation is not reported as success', async () => {
  const s = setup();
  loadListedSessions(s);
  const pending = armAndRevoke(s, OTHER_ID);
  await flush();
  respond(s.requests[0], { revoked: true, removed_access: 0, removed_refresh: 0 });
  await flush();
  respond(s.requests[1], sessionsPayload([session()]));
  await pending;
  assert.match(s.node('#sessionMessage').textContent, /No matching session/);
  assert.ok(!s.node('#sessionMessage').textContent.includes('persisted'));
  assert.equal(s.run('state.sessions.length'), 1, 'the listing must not claim a removal');
});

test('persistence failure keeps durable revocation unconfirmed', async () => {
  const s = setup();
  loadListedSessions(s);
  const pending = armAndRevoke(s);
  await flush();
  fail(s.requests[0], 500, { error: 'revocation_not_persisted' });
  await flush();
  respond(s.requests[1], sessionsPayload([session()]));
  await pending;
  const message = s.node('#sessionMessage').textContent;
  assert.match(message, /durable revocation was not confirmed/);
  assert.match(message, /restart may restore/);
  assert.ok(!message.includes('Session revoked'), 'a failed write is not success');
});

test('invalid session id is rejected without claiming revocation', async () => {
  const s = setup();
  loadListedSessions(s);
  const pending = armAndRevoke(s);
  await flush();
  fail(s.requests[0], 400, { error: 'invalid_session_id' });
  await flush();
  respond(s.requests[1], sessionsPayload([session()]));
  await pending;
  assert.match(s.node('#sessionMessage').textContent, /invalid session id/);
  assert.ok(!s.node('#sessionMessage').textContent.includes('persisted'));
});

test('malformed session listing fails closed', async () => {
  const s = setup();
  const pending = s.run('loadSessions()');
  await flush();
  respond(s.requests[0], { authority: 'local_operator', sessions: 'not-an-array' });
  await pending;
  assert.equal(s.run('state.sessionsLoaded'), false);
  assert.equal(s.run('state.sessions.length'), 0);
  assert.match(s.node('#sessionMessage').textContent, /Unable to update sessions/);
});

test('ablation: confirmation guard and operator token header are required', async () => {
  const s = setup();
  loadListedSessions(s);
  const unconfirmed = s.run(`revokeSessionFromUi(${JSON.stringify(SESSION_ID)})`);
  await flush();
  assert.equal(s.requests.length, 0, 'revoke without confirmation must not send a request');
  await unconfirmed;
  s.run(`armSessionRevoke(${JSON.stringify(SESSION_ID)})`);
  assert.equal(s.requests.length, 0, 'arming confirmation must not send a request');
  const pending = s.run('loadSessions()');
  await flush();
  assert.equal(s.requests[0].options.headers['X-Wcode-UI-Token'], 'test-ui');
  respond(s.requests[0], sessionsPayload([session()]));
  await pending;
  assert.equal(s.run('state.sessionsLoaded'), true);
});
