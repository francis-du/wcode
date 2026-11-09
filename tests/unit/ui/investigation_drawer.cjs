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
  s.run('state.drawerJob="TASK-1";state.runtimeJob={status:"working",origin:"ui",stdout:{text:"line one\\nline two",total_bytes:18,truncated:false,redacted:false},stderr:{text:"",total_bytes:0,truncated:false,redacted:false}};renderInvestigationDrawer();');
  const logs = s.node('#drawerBody').innerHTML;
  assert.match(logs, /TASK-1/);
  assert.match(logs, /line two/, 'the retained output tail must be visible');
  assert.match(logs, /stdout/);
});

test('the drawer does not read an unread job list as empty', () => {
  const s = sandbox();
  s.context.fixture = fixtureWith();
  s.run('state.project=fixture;state.runtimeJobs=[];state.runtimeJobsLoaded=false;state.drawerSection="jobs";renderInvestigationDrawer();');
  assert.match(s.node('#drawerBody').innerHTML, /Reading retained jobs/);
  s.run('state.runtimeJobsLoaded=true;renderInvestigationDrawer();');
  assert.match(s.node('#drawerBody').innerHTML, /No retained jobs in this snapshot/);
});

test('the drawer keeps the investigation label in the URL hash', () => {
  const s = sandbox();
  s.context.fixture = fixtureWith();
  s.run('state.project=fixture;state.current="A";wireHistoryRestoration();renderInvestigationDrawer();');
  s.run('state.drawerLabel="Queue cleanup";syncInvestigationHash();');
  assert.match(s.history.lastUrl, /inv=Queue\+cleanup/, 'the label must be written to the hash');
  // The reader is driven by the hash the browser actually holds.
  s.run('location.hash="#view=files&workspace=A&inv=Queue+cleanup";');
  assert.equal(s.run('investigationHash().label'), 'Queue cleanup');
  s.run('state.drawerLabel=investigationHash().label;renderInvestigationDrawer();');
  assert.equal(s.run('document.getElementById("drawerLabel").value'), 'Queue cleanup');
});

test('the drawer collapses without losing the selected section', () => {
  const s = sandbox();
  s.context.fixture = fixtureWith();
  s.run('state.project=fixture;state.drawerSection="jobs";renderInvestigationDrawer();');
  s.run('state.drawerCollapsed=true;renderInvestigationDrawer();');
  assert.equal(s.run('state.drawerSection'), 'jobs');
  assert.equal(s.run('state.drawerCollapsed'), true);
});
