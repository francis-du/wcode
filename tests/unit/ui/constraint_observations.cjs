'use strict';
// A designed runtime bound is only actionable next to the observation that
// applies to it. The panel keeps the designed bound, the observed value, the
// unit, the precision and the revision binding together, and an empty list must
// never read as "every bound holds".
const assert = require('node:assert/strict');
const { test } = require('node:test');
const { sandbox, project } = require('./observatory.cjs');

const finding = exceeded => ({
  id: 'DRIFT-1',
  kind: 'runtime_drift',
  risk_level: 'medium',
  subject: 'CONSTRAINT-PARALLEL-FIRST',
  message: 'Recent process admission exceeded the designed queue-wait bound.',
  affected_requirements: ['REQ-A'],
  paths: [],
  deviation: {
    metric: 'process_queue_wait',
    expected_max: 30000,
    observed: exceeded ? 45000 : 12000,
    deviation_percent: exceeded ? 50 : 0,
    unit: 'ms',
    precision: 'runtime',
    revision_bound: false,
  },
});

test('a constraint observation shows the designed bound next to the observed value', () => {
  const s = sandbox();
  s.context.fixture = { ...project(), risk: { drift: { findings: [finding(true)] } } };
  s.run('state.project=fixture;renderConstraintObservations();');
  const html = s.node('#constraintObservations').innerHTML;
  assert.match(html, /CONSTRAINT-PARALLEL-FIRST/);
  assert.match(html, /Exceeds bound/);
  assert.match(html, /process_queue_wait/);
  assert.match(html, /30,?000 ms/);
  assert.match(html, /45,?000 ms/);
  assert.match(html, /50%/);
  assert.match(html, /advisory only, not source-revision-bound/);
  assert.match(html, /REQ-A/);
});

test('an observation inside its bound is not reported as a violation', () => {
  const s = sandbox();
  s.context.fixture = { ...project(), risk: { drift: { findings: [finding(false)] } } };
  s.run('state.project=fixture;renderConstraintObservations();');
  const html = s.node('#constraintObservations').innerHTML;
  assert.match(html, /Within bound for this observation/);
  assert.doesNotMatch(html, /Exceeds bound/);
});

test('an empty observation list never reads as every bound holding', () => {
  const s = sandbox();
  s.context.fixture = { ...project(), risk: { drift: { findings: [] } } };
  s.run('state.project=fixture;renderConstraintObservations();');
  assert.match(
    s.node('#constraintObservations').innerHTML,
    /not proof that every designed bound holds/,
  );
});

test('a finding without a deviation records that no observation was kept', () => {
  const s = sandbox();
  const bare = finding(true);
  delete bare.deviation;
  s.context.fixture = { ...project(), risk: { drift: { findings: [bare] } } };
  s.run('state.project=fixture;renderConstraintObservations();');
  assert.match(s.node('#constraintObservations').innerHTML, /not recorded/);
});
