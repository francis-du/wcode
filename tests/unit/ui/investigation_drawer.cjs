'use strict';
// The bottom panel keeps the problems, jobs, logs and evidence being tracked
// visible while the main view changes. Each section is a compact projection of
// facts the views already own; it must not invent a state of its own.
const assert = require('node:assert/strict');
const { test } = require('node:test');
const { sandbox } = require('./observatory.cjs');
const { fixture } = require('./acceptance.cjs');

const fixtureWith = overrides => ({
  ...fixture(),
  attention: { revision: { code: 'code-1', design: 'design-1' }, total: 1, items: [
    { id: 'a1', kind: 'evidence_failed', severity: 'critical', subject: 'Failing check',
      message: 'Current producer failed', provider: 'verification', precision: 'deterministic',
      section: 'proof' }] },
  ...overrides,
});

test('the drawer shows the current problems and the evidence summary', () => {
  const s = sandbox();
  s.context.fixture = fixtureWith();
  s.run('state.project=fixture;renderInvestigationDrawer();');
  assert.match(s.node('#drawerBody').innerHTML, /Failing check/);
  s.run('state.drawerSection="evidence";renderInvestigationDrawer();');
  const evidence = s.node('#drawerBody').innerHTML;
  assert.match(evidence, /current-version evidence/);
  assert.match(evidence, /blocked/, 'the canonical acceptance state must be shown');
});

test('the drawer lists retained jobs and keeps a selection for logs', () => {
  const s = sandbox();
  s.context.fixture = fixtureWith();
  s.run('state.project=fixture;state.runtimeJobs=[{task_id:"TASK-1",status:"working",origin:"ui"}];state.drawerSection="jobs";renderInvestigationDrawer();');
  assert.match(s.node('#drawerBody').innerHTML, /TASK-1/);
  s.run('state.drawerSection="logs";renderInvestigationDrawer();');
  assert.match(s.node('#drawerBody').innerHTML, /Select a job/,
    'logs must not invent output before a job is chosen');
  s.run('state.drawerJob="TASK-1";state.runtimeJob={status:"working",origin:"ui"};renderInvestigationDrawer();');
  assert.match(s.node('#drawerBody').innerHTML, /TASK-1/);
});

test('the drawer collapses without losing the selected section', () => {
  const s = sandbox();
  s.context.fixture = fixtureWith();
  s.run('state.project=fixture;state.drawerSection="jobs";renderInvestigationDrawer();');
  s.run('state.drawerCollapsed=true;renderInvestigationDrawer();');
  assert.equal(s.run('state.drawerSection'), 'jobs');
  assert.equal(s.run('state.drawerCollapsed'), true);
});
