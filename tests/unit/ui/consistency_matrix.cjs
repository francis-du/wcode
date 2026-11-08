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
    risk_level: 'medium', message: 'm', affected_requirements: ['REQ-A'], paths: [],
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
  assert.match(html, /C-1/);
  assert.match(html, /45,?000 \/ 30,?000 ms/);
  assert.match(html, /Needs attention/);
  assert.match(html, /Stable|Unobserved/);
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
