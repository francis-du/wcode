'use strict';
// The change-convergence map is the change-story centerpiece: every card names
// an object, so navigating from it must land on that object rather than only
// revealing the section.
const assert = require('node:assert/strict');
const { test } = require('node:test');
const { sandbox, project } = require('./observatory.cjs');

const requirement = { id: 'REQ-A', title: 'A', intent: '', components: [], convergence: 'stable',
  dependency_alignment: [], acceptance: [], constraints: [], drift: [] };
function fixture(reviewAvailable = true) {
  return { ...project(), git_review: { available: reviewAvailable, reason: reviewAvailable ? 'available' : 'cached_snapshot' },
    changes: [{ path: 'a.rs', affected_components: ['component:one'], affected_requirements: ['REQ-A'] }],
    requirements: [requirement],
    architecture: { components: [{ id: 'component:one', name: 'One', responsibilities: [] }],
      dependencies: [], desired_edges: 0, observed_edges: 0, blocking_drift_edges: 0 } };
}

test('convergence cards name the object they navigate to and focus it', () => {
  const s = sandbox();
  s.context.fixture = fixture();
  s.run('state.project=fixture;renderChangeConvergenceMap();');
  const html = s.node('#changeConvergenceMap').innerHTML;
  assert.match(html, /data-convergence-target="architectureSection" data-convergence-id="component:one"/);
  assert.match(html, /data-convergence-target="requirementsSection" data-convergence-id="REQ-A"/);
  assert.match(html, /data-convergence-target="overviewSection" data-convergence-id="acceptance"/);
  assert.match(html, /data-convergence-target="overviewSection" data-convergence-id="attention"/);

  s.run('convergenceFocus("architectureSection","component:one")');
  assert.equal(s.run('state.selectedComponent'), 'component:one');
  assert.equal(s.run('state.architectureView'), 'components');
  s.run('state.selectedComponent="";state.selected="";convergenceFocus("requirementsSection","REQ-A")');
  assert.equal(s.run('state.selected'), 'REQ-A');
  assert.equal(s.run('state.filter'), 'all');
});

test('a convergence card never invents or selects an object the destination cannot show', () => {
  const s = sandbox();
  s.context.fixture = fixture();
  s.run('state.project=fixture;state.selectedComponent="";state.selected="";');
  s.run('convergenceFocus("architectureSection","component:missing")');
  assert.notEqual(s.run('state.selectedComponent'), 'component:missing');
  s.run('convergenceFocus("requirementsSection","REQ-MISSING")');
  assert.notEqual(s.run('state.selected'), 'REQ-MISSING');
});

test('an unavailable review does not let a card name an unobserved object', () => {
  const s = sandbox();
  s.context.fixture = fixture(false);
  s.run('state.project=fixture;renderChangeConvergenceMap();');
  const html = s.node('#changeConvergenceMap').innerHTML;
  assert.doesNotMatch(html, /data-convergence-id="component:one"/);
  assert.doesNotMatch(html, /data-convergence-id="REQ-A"/);
  assert.match(html, /<strong>Affected components<\/strong><b>—<\/b>/);
});
