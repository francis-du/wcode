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
    implementation_files: 2, depends_on: ['component:two'], changed: true }],
    dependencies: [{ from: 'component:one', to: 'component:two', from_name: 'One', to_name: 'Two',
      desired: true, actual: false, status: 'unverified_actual', precision: 'syntax', blocking: false }],
    desired_edges: 1, observed_edges: 0, blocking_drift_edges: 0 },
  risk: { drift: { findings: [{ id: 'D-1', kind: 'runtime_drift', subject: 'C-1',
    risk_level: 'medium', message: 'm', affected_requirements: ['REQ-A'], paths: ['src/queue.rs'],
    deviation: { metric: 'wait', expected_max: 30000, observed: 45000, deviation_percent: 50,
      unit: 'ms', precision: 'runtime', revision_bound: false } }] } },
});

// The drift queue is a separate block below the matrix; a matrix filter must be
// judged on the matrix alone, not on the queue that still lists every kind.
const matrixOnly = html => html.slice(0, html.indexOf("DRIFT QUEUE"));

test('the matrix shows design, observed state and delta for each kind', () => {
  const s = sandbox();
  s.context.fixture = fixture();
  s.run('state.project=fixture;state.consistencyKind="all";state.consistencyState="all";renderConsistencyMatrix();');
  const html = s.node('#consistencySection').innerHTML;
  assert.match(html, /REQ-A/);
  assert.match(html, /component:one/);
  assert.match(html, /src\/queue\.rs/, 'the drift source path must be visible');
  assert.match(html, /declared, not observed/, 'an unverified declared dependency stays advisory');
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
  assert.match(html, /The suite passes/, 'the criterion statement must be shown');
  assert.match(html, /1 check/, 'the criterion verification references must be counted');
  assert.match(html, /no acceptance criterion references this check/,
    'an unreferenced check must not borrow a criterion');
  assert.match(html, /passed and current/);
  assert.match(html, /mapped but not executed/);
  assert.doesNotMatch(matrixOnly(html), /REQ-A/);
});

test('a requirement row links the drift to the convergence plan that claims it', () => {
  const s = sandbox();
  s.context.fixture = { ...fixture(),
    reconciliation: [{ id: 'PLAN-1', drift_ids: ['D-1'], implementation_tasks: [
      { id: 'T-1', kind: 'implementation', subject: 'Fix the queue', description: '', write_scopes: [], depends_on: [] }] },
      { id: 'PLAN-2', drift_ids: ['D-1'], implementation_tasks: [
        { id: 'T-2', kind: 'implementation', subject: 'Second lane', description: '', write_scopes: [], depends_on: [] },
        { id: 'T-3', kind: 'implementation', subject: 'Third lane', description: '', write_scopes: [], depends_on: [] }] }],
    reconciliation_execution: [{ execution: { plan_id: 'PLAN-1' },
      pending: 1, claimed: 0, completed: 0, failed: 0, converged: false }] };
  s.run('state.project=fixture;state.consistencyKind="requirement";state.consistencyState="all";renderConsistencyMatrix();');
  const html = s.node('#consistencySection').innerHTML;
  assert.match(html, /PLAN-1/);
  assert.match(html, /1 pending/);
  assert.match(html, /PLAN-2/);
  assert.match(html, /2 tasks/, 'a plan without recorded execution state falls back to its declared task count');
  assert.doesNotMatch(html, /no convergence plan references this drift/);
});

test('a drift that no plan claims says so instead of implying convergence', () => {
  const s = sandbox();
  s.context.fixture = { ...fixture(), reconciliation: [] };
  s.run('state.project=fixture;state.consistencyKind="requirement";state.consistencyState="all";renderConsistencyMatrix();');
  assert.match(s.node('#consistencySection').innerHTML, /no convergence plan references this drift/);
});

test('the drift queue separates the three drift kinds with their evidence fields', () => {
  const s = sandbox();
  s.context.fixture = { ...fixture(),
    risk: { drift: { truncated: true, findings: [
      { id: 'D-IMPL', kind: 'implementation_drift', risk_level: 'high', subject: 'REQ-A',
        message: 'Implementation trace is incomplete', affected_requirements: ['REQ-A'],
        paths: ['src/a.rs'], deviation: null },
      { id: 'D-DESIGN', kind: 'design_drift', risk_level: 'medium', subject: 'component:one',
        message: 'Actual state changed without a design change', affected_requirements: ['REQ-A'],
        paths: ['src/b.rs'], deviation: null },
      { id: 'D-RUNTIME', kind: 'runtime_drift', risk_level: 'low', subject: 'C-1',
        message: 'Observed bound exceeded', affected_requirements: [], paths: [],
        deviation: { metric: 'wait', expected_max: 30000, observed: 45000, deviation_percent: 50,
          unit: 'ms', precision: 'runtime', revision_bound: true } }] } } };
  s.run('state.project=fixture;state.consistencyDriftKind="all";renderConsistencyMatrix();');
  const html = s.node('#consistencySection').innerHTML;
  for (const id of ['D-IMPL', 'D-DESIGN', 'D-RUNTIME']) assert.match(html, new RegExp(id));
  assert.match(html, /implementation drift/);
  assert.match(html, /design drift/);
  assert.match(html, /runtime drift/);
  assert.match(html, /45,000 \/ 30,000 ms/, 'a deviation keeps its numerator, denominator and unit');
  assert.match(html, /revision-bound/, 'a revision-bound observation says so');
  assert.match(html, /truncated/, 'a truncated queue says so');
  assert.match(html, /data-consistency-drift-row="object:requirement:REQ-A"/);
  assert.match(html, /data-consistency-drift-row="object:component:component:one"/);
  assert.match(html, /data-consistency-drift-row="diagnosticsSection"/);
  s.run('state.consistencyDriftKind="design_drift";renderConsistencyMatrix();');
  const filtered = s.node('#consistencySection').innerHTML;
  assert.match(filtered, /D-DESIGN/);
  assert.doesNotMatch(filtered, /D-IMPL/);
  assert.doesNotMatch(filtered, /D-RUNTIME/);
});

test('consistency rows keep a visible keyboard path', () => {
  const s = sandbox();
  s.context.fixture = fixture();
  s.run('state.project=fixture;state.consistencyKind="all";state.consistencyState="all";renderConsistencyMatrix();');
  const html = s.node('#consistencySection').innerHTML;
  assert.match(html, /data-consistency-row="object:requirement:REQ-A" data-consistency-label="Open REQ-A" tabindex="0" role="button"/);
  assert.match(html, /data-consistency-drift-row="diagnosticsSection" data-consistency-drift-label="Open drift D-1" tabindex="0" role="button"/);
});

test('a requirement row carries the drift that names it, whatever the kind', () => {
  const s = sandbox();
  s.context.fixture = { ...fixture(),
    risk: { drift: { findings: [
      { id: 'D-DESIGN', kind: 'design_drift', risk_level: 'medium', subject: 'component:one',
        message: 'Actual state changed without a design change', affected_requirements: ['REQ-A'],
        paths: ['src/actual.rs'], deviation: null }] } } };
  s.run('state.project=fixture;state.consistencyKind="requirement";state.consistencyState="all";renderConsistencyMatrix();');
  const html = matrixOnly(s.node('#consistencySection').innerHTML);
  assert.match(html, /src\/actual\.rs/, 'the changed path behind the drift is visible');
  assert.match(html, /no convergence plan references this drift/);
});

test('the dependency observation states its coverage with counts, not percentages alone', () => {
  const s = sandbox();
  s.context.fixture = fixture();
  s.run('state.project=fixture;');
  const note = s.run('architectureObservationNote()');
  assert.match(note, /0 \/ 1/, 'design coverage keeps its numerator and denominator');
  assert.match(note, /1 \/ 1/, 'implementation coverage keeps its numerator and denominator');
  assert.match(note, /not applicable/, 'a zero observed denominator is not read as full consistency');
  s.context.fixture = { ...fixture(),
    architecture: { components: [], dependencies: [], desired_edges: 0, observed_edges: 0,
      aligned_edges: 0, blocking_drift_edges: 0, components_with_implementation: 0 } };
  s.run('state.project=fixture;');
  const empty = s.run('architectureObservationNote()');
  assert.match(empty, /no declared dependencies/, 'a zero denominator is not advertised as full coverage');
  assert.doesNotMatch(empty, /%/, 'no percentage is shown without the counts it came from');
});

test('a criterion no check references appears as a missing acceptance item', () => {
  const s = sandbox();
  s.context.fixture = { ...fixture(),
    design: { acceptance: { 'AC-GAP': { id: 'AC-GAP', title: 'Manual review',
      statement: 'A human reviews the change', verification: [] } } },
    proof: { acceptance: { checks: [] } } };
  s.run('state.project=fixture;state.consistencyKind="acceptance";state.consistencyState="all";renderConsistencyMatrix();');
  const html = s.node('#consistencySection').innerHTML;
  assert.match(html, /AC-GAP/);
  assert.match(html, /no verification ref/);
  assert.match(html, /no check references this criterion/);
});

test('component rows carry truncation and empty-sample boundaries', () => {
  const s = sandbox();
  s.context.fixture = { ...fixture(),
    architecture: { components: [{ id: 'component:one', name: 'One', responsibilities: ['a'],
      implementation_files: 2, depends_on: [], changed: true }],
      dependencies: [], desired_edges: 0, observed_edges: 0, blocking_drift_edges: 0,
      dependency_candidates: [], dependency_candidates_total: 5, dependency_candidates_truncated: true } };
  s.run('state.project=fixture;state.consistencyKind="component";state.consistencyState="all";renderConsistencyMatrix();');
  const html = s.node('#consistencySection').innerHTML;
  assert.match(html, /bounded sample/, 'a truncated candidate sample must be stated');
  assert.match(html, /No observed dependency edges/, 'an empty sample must not read as having no dependencies');
});

test('the matrix filters by kind and an empty match says so', () => {
  const s = sandbox();
  s.context.fixture = fixture();
  s.run('state.project=fixture;state.consistencyKind="constraint";state.consistencyState="all";renderConsistencyMatrix();');
  const kindOnly = s.node('#consistencySection').innerHTML;
  assert.match(kindOnly, /C-1/);
  assert.doesNotMatch(matrixOnly(kindOnly), /REQ-A/);
  s.run('state.consistencyKind="constraint";state.consistencyState="unobserved";renderConsistencyMatrix();');
  assert.match(s.node('#consistencySection').innerHTML, /No design object matches this filter./);
});
