'use strict';
// A designed constraint is only actionable next to the observation that applies
// to it. The Design State supplies every declared constraint; a runtime drift
// finding supplies the bound, the observed value, the unit, the precision and
// the revision binding. A declared constraint with no finding must appear as
// unobserved rather than disappear, because absence of an observation is not
// evidence that the bound holds.
const assert = require('node:assert/strict');
const { test } = require('node:test');
const { sandbox, project } = require('./observatory.cjs');

const constraint = {
  schema_version: 1,
  id: 'CONSTRAINT-PARALLEL-FIRST',
  title: 'Parallel-first admission',
  statement: 'Recent process admission stays within the designed queue-wait bound.',
  applies_to: ['REQ-A'],
};

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

const fixtureWith = findings => ({
  ...project(),
  design: { constraints: { [constraint.id]: constraint } },
  risk: { drift: { findings } },
});

test('a declared constraint shows its designed bound next to the observed value', () => {
  const s = sandbox();
  s.context.fixture = fixtureWith([finding(true)]);
  s.run('state.project=fixture;renderConstraintObservations();');
  const html = s.node('#constraintObservations').innerHTML;
  assert.match(html, /CONSTRAINT-PARALLEL-FIRST/);
  assert.match(html, /Parallel-first admission/);
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
  s.context.fixture = fixtureWith([finding(false)]);
  s.run('state.project=fixture;renderConstraintObservations();');
  const html = s.node('#constraintObservations').innerHTML;
  assert.match(html, /Within bound for this observation/);
  assert.doesNotMatch(html, /Exceeds bound/);
});

test('a declared constraint with no observation is shown as unobserved, not omitted', () => {
  const s = sandbox();
  s.context.fixture = fixtureWith([]);
  s.run('state.project=fixture;renderConstraintObservations();');
  const html = s.node('#constraintObservations').innerHTML;
  assert.match(html, /CONSTRAINT-PARALLEL-FIRST/, 'the declared constraint must still appear');
  assert.match(html, /No observation recorded/);
  assert.match(html, /not recorded for this snapshot/);
  assert.doesNotMatch(html, /Exceeds bound/);
});

test('a finding without a deviation records that no observation was kept', () => {
  const s = sandbox();
  const bare = finding(true);
  delete bare.deviation;
  s.context.fixture = fixtureWith([bare]);
  s.run('state.project=fixture;renderConstraintObservations();');
  assert.match(s.node('#constraintObservations').innerHTML, /not recorded/);
});

test('a Design that declares no constraints says so instead of implying success', () => {
  const s = sandbox();
  s.context.fixture = { ...project(), risk: { drift: { findings: [] } } };
  s.run('state.project=fixture;renderConstraintObservations();');
  assert.match(
    s.node('#constraintObservations').innerHTML,
    /declares no constraints/,
  );
});
