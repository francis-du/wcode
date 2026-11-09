'use strict';
// The production-capability matrix keeps every existing engineering surface
// reachable from the home status block. Fitness and the language providers had
// no surface entry, so both could only be found by scrolling the overview page.
const assert = require('node:assert/strict');
const { test } = require('node:test');
const { sandbox, project } = require('./observatory.cjs');
const { fixture } = require('./acceptance.cjs');

test('the home status surface reaches Fitness and the language providers', () => {
  const s = sandbox();
  s.context.fixture = {
    ...project(),
    fitness: { benchmark: { available: true, status: 'current', partial: false,
      latest: { case_count: 60, profile: 'synthetic' } } },
    language_quality: { languages: [
      { language: 'rust', detected_files: 120, gaps: ['lint'] },
      { language: 'go', detected_files: 8, gaps: [] }] },
  };
  s.run('state.project=fixture;renderObservationCoverage();');
  const html = s.node('#observationCoverage').innerHTML;
  assert.match(html, /data-summary-action="fitnessSection"/);
  assert.match(html, /data-summary-action="qualitySection"/);
  assert.match(html, /60 cases/);
  assert.match(html, /2 languages/);
  assert.match(html, /1 gaps/);
});

test('browser back restores the previous view', () => {
  const s = sandbox();
  s.run('state.current="A";wireHistoryRestoration();activateWorkspaceTab("files");');
  assert.equal(s.history.length, 1, 'the first activation replaces the load entry');
  s.run('activateWorkspaceTab("proof");activateWorkspaceTab("changes");');
  assert.equal(s.run('state.workspaceTab'), 'changes');
  s.history.back();
  assert.equal(s.run('state.workspaceTab'), 'proof', 'back must restore the previous view');
  s.history.back();
  assert.equal(s.run('state.workspaceTab'), 'files');
});

test('a history entry recorded for another workspace is ignored', () => {
  const s = sandbox();
  s.run('state.current="A";wireHistoryRestoration();activateWorkspaceTab("files");');
  s.events.popstate({ state: { wcodeView: 'proof', wcodeWorkspace: 'B' } });
  assert.equal(s.run('state.workspaceTab'), 'files',
    'a foreign workspace entry must not move this workspace');
});

test('switching views keeps the selection even when the filter hides it', () => {
  const s = sandbox();
  s.context.fixture = { ...fixture(),
    requirements: [{ id: 'REQ-A', title: 'Isolation', intent: '', priority: 'high', status: 'active',
      components: [], convergence: 'stable', dependency_alignment: [], acceptance: [], constraints: [],
      decisions: [], drift: [], implementation_lines: 1, changed: false }] };
  s.run('state.project=fixture;state.current="A";');
  s.run('state.filter="changed";state.selectedFilter="changed";state.selected="REQ-A";activateWorkspaceTab("files");activateWorkspaceTab("requirements");');
  assert.equal(s.run('state.selected'), 'REQ-A', 'a view switch must not drop the selection');
  assert.match(s.node('#requirements').innerHTML, /outside the current filter/);
});

test('a shared hash restores the view it names', () => {
  const s = sandbox(false, true, { hash: '#view=proof&ws=A' });
  s.run('state.current="A";');
  assert.equal(s.run('initialViewFromHash()'), 'proof');
});

test('a hash for another workspace is ignored on load', () => {
  const s = sandbox(false, true, { hash: '#view=proof&ws=B' });
  s.run('state.current="A";');
  assert.equal(s.run('initialViewFromHash()'), 'overview');
});

test('an external hash change restores that view for this workspace', () => {
  const s = sandbox(false, true, { hash: '#view=files&ws=A' });
  s.run('state.current="A";wireHistoryRestoration();state.workspaceTab="proof";');
  s.run('location.hash="#view=changes&ws=A";');
  s.events.hashchange();
  assert.equal(s.run('state.workspaceTab'), 'changes');
});

test('an external hash change for another workspace is ignored', () => {
  const s = sandbox(false, true, { hash: '#view=files&ws=A' });
  s.run('state.current="A";wireHistoryRestoration();state.workspaceTab="proof";');
  s.run('location.hash="#view=changes&ws=B";');
  s.events.hashchange();
  assert.equal(s.run('state.workspaceTab'), 'proof');
});

test('a shared hash can name an object directly', () => {
  const s = sandbox(false, true, { hash: '#view=requirements&workspace=A&o=object:requirement:REQ-A' });
  s.run('state.project={workspace:"A",requirements:[{id:"REQ-A",title:"Isolation",intent:"",priority:"high",status:"active",components:[],convergence:"stable",dependency_alignment:[],acceptance:[],constraints:[],decisions:[],drift:[],implementation_files:0,implementation_symbols:0,implementation_lines:1}]};');
  s.run('state.current="A";activateWorkspaceTab(initialViewFromHash());executeCommand(investigationHash().object);');
  assert.equal(s.run('state.workspaceTab'), 'requirements');
  assert.equal(s.run('state.selected'), 'REQ-A');
});

test('selecting an object rewrites the hash without adding a history step', () => {
  const s = sandbox();
  s.run('state.current="A";wireHistoryRestoration();activateWorkspaceTab("files");');
  const before = s.history.length;
  s.run('state.selected="REQ-A";renderTabPanels("files");');
  assert.equal(s.history.length, before, 'an object selection must not add a history step');
  assert.match(s.history.lastUrl, /o=object%3Arequirement%3AREQ-A/,
    'the hash must name the selected object');
});

test('the home status surface reaches the structured risk list', () => {
  const s = sandbox();
  s.context.fixture = { ...project(), risk: { risks: [
    { id: 'R-1', category: 'coverage', level: 'high', summary: 'Thin verification' },
    { id: 'R-2', category: 'drift', level: 'low', summary: 'Minor drift' }] } };
  s.run('state.project=fixture;renderObservationCoverage();');
  const html = s.node('#observationCoverage').innerHTML;
  assert.match(html, /data-summary-action="diagnosticsSection"/);
  assert.match(html, /2 structured risks/);
  assert.match(html, /Needs attention/);
});

test('an unavailable benchmark and no detected language are reported as unavailable', () => {
  const s = sandbox();
  s.context.fixture = project();
  s.run('state.project=fixture;renderObservationCoverage();');
  const html = s.node('#observationCoverage').innerHTML;
  assert.match(html, /data-summary-action="fitnessSection"/);
  assert.match(html, /No benchmark report available/);
  assert.match(html, /data-summary-action="qualitySection"/);
  assert.match(html, /No detected language files/);
});

test('a partial benchmark is not presented as a current observation', () => {
  const s = sandbox();
  s.context.fixture = { ...project(),
    fitness: { benchmark: { available: true, status: 'historical', partial: true,
      latest: { case_count: 12, profile: 'synthetic' } } } };
  s.run('state.project=fixture;renderObservationCoverage();');
  const html = s.node('#observationCoverage').innerHTML;
  assert.match(html, /Historical or partial/);
});
