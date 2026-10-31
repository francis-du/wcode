'use strict';
// Functional ablations of the shipped renderer, isolated to fresh VMs.
// No production file, task, command, ownership or Evidence is mutated.
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { sandbox, project } = require('./observatory.cjs');
const root = process.argv[2] || path.resolve(__dirname, '../../..');
const sourcePath = path.join(root, 'src/ui/intelligence_web/app/worklist.js');
const source = fs.readFileSync(sourcePath, 'utf8').replace(/\r\n?/g, '\n');
const digest = value => crypto.createHash('sha256').update(value).digest('hex');

function replaceOnce(text, before, after) {
  assert.equal(text.split(before).length, 2, 'ablation anchor must occur exactly once: ' + before);
  return text.replace(before, after);
}
function fixture() {
  const done = Array.from({ length: 8 }, (_, i) => ({
    id: 'done-' + i, title: 'Historical task ' + i, status: 'done', write_paths: [],
  }));
  const open = [
    { id: 'open-0', title: 'Current engineering work', status: 'pending', depends_on: ['done-0'], write_paths: [] },
    { id: 'open-1', title: 'Blocked engineering work', status: 'blocked', depends_on: [], write_paths: [] },
  ];
  return { available: true, exists: true, revision: 7, items: done.concat(open),
    counts: { pending: 1, in_progress: 0, blocked: 1, done: 8 }, runnable: ['open-0'] };
}
function environment(mutation) {
  const s = sandbox(false, true, { fakeTimers: true });
  if (mutation) {
    const changed = mutation(source);
    assert.notEqual(changed, source, 'an ablation must alter the production source snapshot');
    // The production constant already exists; only replace function definitions.
    const functions = replaceOnce(changed,
      'const worklistFilters = ["open", "all", "pending", "in_progress", "blocked", "done", "unknown"];\n', '');
    vm.runInContext(functions, s.context, { filename: 'worklist-isolated-ablation.js' });
  }
  s.context.fixture = fixture();
  s.context.projection = { ...project(), execution: { available: true, exists: false, worklist: s.context.fixture } };
  s.run('state.project = projection;');
  return s;
}
function render(s) {
  const html = s.run('worklistOwnershipHtml(fixture)');
  return { html, ids: [...html.matchAll(/data-worklist-task="([^"]+)"/g)].map(match => match[1]) };
}
function openVisibility(mutation) {
  const s = environment(mutation), view = render(s);
  const openIds = view.ids.filter(id => id.startsWith('open-'));
  return { invariant: openIds.includes('open-0') && openIds.includes('open-1'),
    observed_tasks: 10, relevant_open_tasks: 2, visible_tasks: view.ids.length,
    visible_open_tasks: openIds.length, visible_ids: view.ids };
}
function fullObservationDependencies(mutation) {
  const s = environment(mutation), view = render(s);
  const row = view.html.match(/data-worklist-task="open-0"[\s\S]*?<\/article>/)?.[0] || '';
  const satisfied = row.includes('Dependencies satisfied');
  const runnable = row.includes('Reported runnable');
  const dependencyVisible = view.ids.includes('done-0');
  return { invariant: satisfied && runnable && !dependencyVisible,
    prerequisite_observed: true, prerequisite_visible: dependencyVisible,
    dependency_satisfied: satisfied, reported_runnable: runnable };
}
function resetBoundaries(mutation) {
  const boundaries = ['workspace', 'owner', 'epoch'].map(boundary => {
    const s = environment(mutation);
    s.run('setWorklistView("done", "Historical task", 0);');
    assert.equal(s.run('worklistViewScope().filter'), 'done', 'old filter must be established');
    if (boundary === 'workspace') {
      s.run('state.current = "B";');
      s.context.projection = { ...project('B'), execution: { available: true, exists: false, worklist: s.context.fixture } };
      s.run('state.project = projection;');
    } else if (boundary === 'owner') {
      s.run('state.worklistViewScope.owner = "previous-ui-owner";');
    } else {
      s.run('state.workspaceEpoch += 1;');
    }
    const view = render(s), currentFilter = s.run('worklistViewScope().filter');
    return { boundary, reset_to_open: currentFilter === 'open',
      visible_open_tasks: view.ids.filter(id => id.startsWith('open-')).length };
  });
  return { invariant: boundaries.every(row => row.reset_to_open && row.visible_open_tasks === 2),
    boundaries_preserved: boundaries.filter(row => row.reset_to_open && row.visible_open_tasks === 2).length,
    total_boundaries: boundaries.length, boundaries };
}
function observationFreshness(mutation) {
  const current = render(environment(mutation)).html.includes('Reported runnable');
  const observations = ['stale', 'partial', 'historical'].map(kind => {
    const s = environment(mutation);
    if (kind === 'stale') s.run('state.snapshotStale = true;');
    if (kind === 'partial') s.context.fixture.truncated = true;
    if (kind === 'historical') {
      s.context.fixture = { available: false, last_known: s.context.fixture };
    }
    const html = render(s).html;
    return { kind, reported_runnable: html.includes('Reported runnable') };
  });
  return { invariant: current && observations.every(row => !row.reported_runnable),
    current_runnable_visible: current,
    uncertain_observations_suppressed: observations.filter(row => !row.reported_runnable).length,
    total_uncertain_observations: observations.length, observations };
}

const definitions = [
  {
    id: 'open-work-default-priority',
    hypothesis: 'Open work remains visible when eight completed records precede it.',
    mutation: value => replaceOnce(
      replaceOnce(value, 'filter: "open", query: "", page: 0', 'filter: "all", query: "", page: 0'),
      'filtered.sort((a, b) => rank[worklistTaskState(a)] - rank[worklistTaskState(b)]);',
      '// Ablation: legacy insertion order before the first-eight limit.'),
    observe: openVisibility,
  },
  {
    id: 'full-observation-dependencies',
    hypothesis: 'A completed prerequisite outside the filtered page still resolves.',
    mutation: value => replaceOnce(value,
      'dependencies = worklistDependencies(item, observation), claim = item.claim;',
      'dependencies = worklistDependencies(item, { ...observation, byId: new Map(view.visible.map(row => [row.id, row])) }), claim = item.claim;'),
    observe: fullObservationDependencies,
  },
  {
    id: 'workspace-owner-epoch-reset',
    hypothesis: 'Old filters cannot hide open work after the view identity changes.',
    mutation: value => replaceOnce(value,
      'if (!scope || scope.workspace !== state.current || scope.owner !== token || scope.epoch !== state.workspaceEpoch) {',
      'if (!scope) {'),
    observe: resetBoundaries,
  },
  {
    id: 'uncertain-runnable-suppression',
    hypothesis: 'Stale, partial and historical observations cannot report current runnable work.',
    mutation: value => replaceOnce(value,
      'current: !stale && !observation.historical && !observation.partial',
      'current: true'),
    observe: observationFreshness,
  },
];
const results = definitions.map(({ id, hypothesis, mutation, observe }) => {
  const baseline = observe();
  assert.equal(baseline.invariant, true, id + ': production baseline must satisfy the invariant');
  const ablated = observe(mutation);
  assert.equal(ablated.invariant, false, id + ': isolated ablation must violate the invariant');
  const restored = observe();
  assert.equal(restored.invariant, true, id + ': fresh production baseline must recover');
  assert.deepEqual(restored, baseline, id + ': restoration must reproduce baseline observations');
  return { id, hypothesis, baseline, ablated, restored, passed: true };
});
assert.equal(
  digest(fs.readFileSync(sourcePath, 'utf8').replace(/\r\n?/g, '\n')),
  digest(source),
  'production source changed while the diagnostic was running',
);
console.log(JSON.stringify({
  kind: 'isolated-functional-ablation',
  source: 'src/ui/intelligence_web/app/worklist.js',
  source_sha256: digest(source),
  cases: results,
  passed: results.length,
  total: definitions.length,
  limits: 'Synthetic renderer fixtures only; no browser, model-quality, latency, token-cost or production benchmark claim.',
}, null, 2));
