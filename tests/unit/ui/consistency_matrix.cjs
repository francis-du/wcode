'use strict';
// The design-consistency matrix combines existing design identities with their
// observed state and the delta between them. It must not invent a state, and a
// filter that matches nothing must say so rather than imply success.
const assert = require('node:assert/strict');
const { test } = require('node:test');
const { sandbox, project } = require('./observatory.cjs');

const fixture = () => ({
  ...project(),
  design: { constraints: { 'C-1': { id: 'C-1', title: 'Queue wait', statement: 'Bound', applies_to: ['REQ-A'] } } },
  requirements: [{ id: 'REQ-A', title: 'Isolation', intent: '', priority: 'high',
    components: ['component:one'], convergence: 'needs_convergence',
    dependency_alignment: [{ id: 'component:one', blocking: true }],
    acceptance: [{ id: 'AC-1' }], implementation_lines: 40, changed: true }],
  architecture: { components: [{ id: 'component:one', name: 'One', responsibilities: ['a'],
    implementation_files: 2, depends_on: [], changed: true }],
    dependencies: [], desired_edges: 0, observed_edges: 0, blocking_drift_edges: 0 },
  risk: { drift: { findings: [{ id: 'D-1', kind: 'runtime_drift', subject: 'C-1',
    risk_level: 'medium', message: 'm', affected_requirements: ['REQ-A'], paths: ['src/queue.rs'],
    deviation: { metric: 'wait', expected_max: 30000, observed: 45000, deviation_percent: 50,
      unit: 'ms', precision: 'runtime', revision_bound: false } }] } },
});

test('the matrix shows design, observed state and delta for each kind', () => {
  const s = sandbox();
  s.context.fixture = fixture();
  s.run('state.project=fixture;state.consistencyKind="all";state.consistencyState="all";renderConsistencyMatrix();');
  const html = s.node('#consistencySection').innerHTML;
  assert.match(html, /REQ-A/);
  assert.match(html, /component:one/);
  assert.match(html, /src\/queue\.rs/, 'the drift source path must be visible');
  assert.match(html, /C-1/);
  assert.match(html, /45,?000 \/ 30,?000 ms/);
  assert.match(html, /Needs attention/);
  assert.match(html, /Stable|Unobserved/);
});

test('acceptance rows keep mapped, executed, outcome and freshness independent', () => {
  const s = sandbox();
  s.context.fixture = { ...fixture(),
    design: { acceptance: { 'AC-1': { id: 'AC-1', title: 'Tests stay green',
        statement: 'The suite passes', verification: [{ kind: 'check', id: 'CHK-1' }] } } },
    proof: { acceptance: { checks: [
      { id: 'CHK-1', required: true, required_level: 'quick', level: 'quick',
        mapped: true, execution: 'executed', outcome: 'pass', freshness: 'current', evidence_ids: [] },
      { id: 'CHK-2', required: true, required_level: 'full', level: 'full',
        mapped: true, execution: 'unavailable', outcome: 'unknown', freshness: 'stale', evidence_ids: [] }] } } };
  s.run('state.project=fixture;state.consistencyKind="acceptance";state.consistencyState="all";renderConsistencyMatrix();');
  const html = s.node('#consistencySection').innerHTML;
  assert.match(html, /CHK-1/);
  assert.match(html, /CHK-2/);
  assert.match(html, /Tests stay green/, 'the referencing criterion must be named');
  assert.match(html, /no acceptance criterion references this check/,
    'an unreferenced check must not borrow a criterion');
  assert.match(html, /passed and current/);
  assert.match(html, /mapped but not executed/);
  assert.doesNotMatch(html, /REQ-A/);
});

test('a requirement row links the drift to the convergence plan that claims it', () => {
  const s = sandbox();
  s.context.fixture = { ...fixture(),
    reconciliation: [{ id: 'PLAN-1', drift_ids: ['D-1'], implementation_tasks: [
      { id: 'T-1', kind: 'implementation', subject: 'Fix the queue', description: '', write_scopes: [], depends_on: [] }] }],
    reconciliation_execution: { execution: { plan_id: 'PLAN-1' },
      pending: 1, claimed: 0, completed: 0, failed: 0, converged: false } };
  s.run('state.project=fixture;state.consistencyKind="requirement";state.consistencyState="all";renderConsistencyMatrix();');
  const html = s.node('#consistencySection').innerHTML;
  assert.match(html, /PLAN-1/);
  assert.match(html, /1 pending/);
  assert.doesNotMatch(html, /no convergence plan references this drift/);
});

test('a drift that no plan claims says so instead of implying convergence', () => {
  const s = sandbox();
  s.context.fixture = { ...fixture(), reconciliation: [] };
  s.run('state.project=fixture;state.consistencyKind="requirement";state.consistencyState="all";renderConsistencyMatrix();');
  assert.match(s.node('#consistencySection').innerHTML, /no convergence plan references this drift/);
});

test('the matrix filters by kind and an empty match says so', () => {
  const s = sandbox();
  s.context.fixture = fixture();
  s.run('state.project=fixture;state.consistencyKind="constraint";state.consistencyState="all";renderConsistencyMatrix();');
  const kindOnly = s.node('#consistencySection').innerHTML;
  assert.match(kindOnly, /C-1/);
  assert.doesNotMatch(kindOnly, /REQ-A/);
  s.run('state.consistencyKind="component";state.consistencyState="unobserved";renderConsistencyMatrix();');
  assert.match(s.node('#consistencySection').innerHTML, /No design object matches this filter./);
});
