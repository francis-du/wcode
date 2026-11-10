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

test('the investigation link follows the selected requirement', () => {
  const requirement = (id, changed) => ({ id, title: id, intent: '', priority: 'high', status: 'active',
    components: [], convergence: 'stable', dependency_alignment: [], acceptance: [], constraints: [],
    decisions: [], drift: [], implementation_lines: 1, changed });
  const s = sandbox();
  s.context.fixture = { ...fixture(), requirements: [requirement('REQ-A', false), requirement('REQ-B', true)] };
  s.run('state.project=fixture;state.current="A";state.filter="all";renderRequirements();');
  s.run('activateWorkspaceTab("requirements");');
  assert.match(s.history.lastUrl, /o=object%3Arequirement%3AREQ-B/,
    'the requirement the view selects names the link');
  s.run('activateRequirement("REQ-A");');
  assert.equal(s.run('state.selected'), 'REQ-A');
  assert.match(s.history.lastUrl, /o=object%3Arequirement%3AREQ-A/,
    'selecting another requirement rewrites the link');
  assert.doesNotMatch(s.history.lastUrl, /REQ-B/, 'the previous object leaves the link');
});

test('a runtime drift signal keeps its percentage next to the values it came from', () => {
  const finding = deviation => ({ id: 'D-1', kind: 'runtime_drift', risk_level: 'high',
    subject: 'C-1', message: 'bound exceeded', affected_requirements: [], paths: [], deviation });
  const s = sandbox();
  s.context.fixture = { ...fixture(), risk: { drift: { findings: [finding(
    { metric: 'queue_wait_ms', expected_max: 100, observed: 250, deviation_percent: 150,
      unit: 'ms', precision: 'runtime', revision_bound: false })] } } };
  s.run('state.project=fixture;');
  const drift = JSON.parse(s.run('JSON.stringify(attentionSignals())'))
    .find(item => item.title.includes('runtime drift'));
  assert.match(drift.detail, /150%/);
  assert.match(drift.detail, /250 \/ 100 ms/, 'the peak keeps its numerator, denominator and unit');

  s.context.fixture = { ...fixture(), risk: { drift: { findings: [finding({ deviation_percent: 40 })] } } };
  s.run('state.project=fixture;');
  const uncomparable = JSON.parse(s.run('JSON.stringify(attentionSignals())'))
    .find(item => item.title.includes('runtime drift'));
  assert.match(uncomparable.detail, /without a comparable value/);
  assert.doesNotMatch(uncomparable.detail, /0 \/ 0/, 'absent values must not render as zero');
});

test('the context budget shows its tokens, not a percentage alone', () => {
  const s = sandbox();
  s.context.fixture = { ...fixture(),
    activity: { available: true, recent: [], completed: 0, failed: 0,
      agent_context: { calls: 3, model_tokens: 4096, budget_tokens: 8192 } } };
  s.run('state.project=fixture;state.activitySnapshot=fixture;renderActivity();');
  const html = s.node('#resourceStatus').innerHTML;
  assert.match(html, /4,096 \/ 8,192/, 'the budget keeps its numerator and denominator');
  assert.match(html, /50%/, 'the derived percentage still appears');
});

test('the home status carries the work queue counts', () => {
  const s = sandbox();
  s.context.fixture = { ...fixture(), execution: { available: true, exists: true,
    checkpoint: { open_items: 4, blocked_items: 2, done_items: 9, runnable: ['lane-a'] } } };
  s.run('state.project=fixture;renderObservationCoverage();');
  const html = s.node('#observationCoverage').innerHTML;
  assert.equal((html.match(/class="observation-surface"/g) || []).length, 8,
    'the drilldown count stays at the contracted eight');
  assert.match(html, /4 open · 2 blocked/, 'unfinished and blocked counts are visible at home');
  assert.match(html, /data-summary-action="activitySection"/, 'the entry opens the task progress');
  s.run('state.language="zh-CN";renderObservationCoverage();');
  assert.match(s.node('#observationCoverage').innerHTML, /4 项未完成 · 2 项阻塞/,
    'the counts keep their Simplified Chinese form');
  s.context.fixture = { ...fixture(), execution: { available: false } };
  s.run('state.language="en";state.project=fixture;renderObservationCoverage();');
  assert.doesNotMatch(s.node('#observationCoverage').innerHTML, /open · /,
    'an unread checkpoint adds no counts');
});

test('the acceptance policy binding shows its scope and expiry', () => {
  const s = sandbox();
  const base = fixture();
  s.context.fixture = { ...base, acceptance: { ...base.acceptance, state: 'ready', policy: {
    snapshot_digest: `sha256:${'a'.repeat(64)}`, generation: 2, current_generation: 2, expires_at_ms: 1,
    selection: { policy_id: 'POL-1', policy_version: 3, docs_only: true, matched_rules: ['r1', 'r2'] } } } };
  s.run('state.project=fixture;renderAcceptanceSummary(null);');
  const html = s.node('#statusSummary').innerHTML;
  assert.match(html, /POL-1/);
  assert.match(html, /Policy version 3/, 'the policy version is part of its identity');
  assert.match(html, /2 matched rules/, 'the scope says how many rules matched');
  assert.match(html, /docs only/);
  assert.match(html, /expired/, 'an expiry in the past is marked as expired');
});

test('a pending authorization shows when it was requested', () => {
  const s = sandbox();
  s.run('state.accessLoaded=true;state.authorizations=[{id:"AUTH-1",workspace:"A",kind:"command_access",summary:"authorize command: git",program:"git",created_at_ms:1700000000000}];renderAccess();');
  const html = s.node('#authorizationList').innerHTML;
  assert.match(html, /authorize command: git/);
  assert.match(html, /requested/, 'the request time is visible so a stale request can be judged');
});

test('the investigation link waits for a workspace', () => {
  const s = sandbox(false, true, { hash: '' });
  s.run('state.current="";state.workspaceTab="overview";syncInvestigationHash();');
  assert.equal(s.history.lastUrl, '', 'no workspace means no link to share');
  s.run('state.current="A";syncInvestigationHash();');
  assert.match(s.history.lastUrl, /workspace=A/, 'the link appears once a workspace is known');
});

test('clearing the workspace resets the system map controls too', () => {
  const s = sandbox();
  s.context.fixture = fixture();
  s.run('state.project=fixture;state.current="A";setSystemMapScale(1.2);setSystemMapFull(true);');
  assert.equal(s.run('els.systemMapZoomValue.textContent'), '120%');
  s.run('clearWorkspaceView();');
  assert.equal(s.run('els.systemMapZoomValue.textContent'), '100%',
    'the zoom readout follows the reset scale');
  assert.equal(s.run('els.systemMapFit.getAttribute("aria-pressed")'), 'true',
    'the fit button follows the reset fit state');
  assert.equal(s.run('els.systemMapFull.textContent'), 'Open full map',
    'the full-map button must not offer to exit a map that is no longer full');
  assert.equal(s.run('els.systemMapFull.getAttribute("aria-pressed")'), 'false');
});

test('clearing the workspace drops the fullscreen code graph', () => {
  const s = sandbox();
  s.context.fixture = fixture();
  s.run('state.project=fixture;state.current="A";setCodeGraphFull(true);setCodeGraphInspector(true);');
  assert.equal(s.run('els.codeGraphSection.classList.contains("code-graph-fullscreen")'), true);
  s.run('clearWorkspaceView();');
  assert.equal(s.run('state.codeGraphFull'), false,
    'a fullscreen graph must not cover the next workspace');
  assert.equal(s.run('els.codeGraphSection.classList.contains("code-graph-fullscreen")'), false);
  assert.equal(s.run('els.codeGraphFull.getAttribute("aria-pressed")'), 'false');
  assert.equal(s.run('state.codeGraphInspectorOpen'), false,
    'the inspector belongs to the graph being cleared');
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

test('a change without retained diff counts stays unknown instead of a measured zero', () => {
  const s = sandbox();
  const change = (path, additions, deletions) => ({
    path, status: 'modified', category: 'source', scope: 'source',
    additions, deletions, staged: false, unstaged: true, untracked: false,
    affected_requirements: [], affected_components: [], changed_ranges_truncated: false,
  });
  s.context.fixture = { ...project(), changes: [
    change('assets/logo.png', null, null),
    change('src/a.rs', 2, 1),
    change('src/partial.rs', 4, null),
  ] };
  s.run('state.project=fixture;renderChanges();');
  const html = s.node('#changes').innerHTML;
  assert.doesNotMatch(html, /\+0\s*-0/, 'an unretained diff must not render as a measured zero');
  assert.match(html, /<span class="change-num add">\+2<\/span> <span class="change-num remove">-1<\/span>/,
    'retained counts keep their exact values');
  assert.match(html, /<span class="change-num add">\+4<\/span> <span class="change-num unknown">-\?<\/span>/,
    'a half-known diff keeps the unknown side unknown');
  const unknown = (html.match(/class="change-num unknown"/g) || []).length;
  assert.equal(unknown, 2, 'the binary change and the unknown deletion side each stay unknown');
});
