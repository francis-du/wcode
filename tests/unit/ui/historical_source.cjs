'use strict';
// Bounded historical source inspection: functional regressions plus isolated
// ablations of the reported-revision bound. No production file is mutated.
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const root = process.argv[2] || path.resolve(__dirname, '../../..');
const sourcePath = path.join(root, 'src/ui/intelligence_web/app/historical_source.js');
const source = [
  fs.readFileSync(sourcePath, 'utf8'),
  fs.readFileSync(path.join(root, 'src/ui/intelligence_web/app/features_requirements.js'), 'utf8'),
  fs.readFileSync(path.join(root, 'src/ui/intelligence_web/app/features_changes.js'), 'utf8'),
].join('\n');
const digest = value => crypto.createHash('sha256').update(value).digest('hex');
const sourceDigest = digest(fs.readFileSync(sourcePath, 'utf8').replace(/\r\n?/g, '\n'));
const results = [];

const REVISION_A = 'a'.repeat(40);
const REVISION_B = 'b'.repeat(40);

function fixture() {
  return {
    workspace: 'A',
    revisions: [
      {revision: REVISION_A, short_revision: 'aaaaaaa', committed_at: '2026-11-01', subject: 'first recorded change'},
      {revision: REVISION_B, short_revision: 'bbbbbbb', committed_at: '2026-11-02', subject: 'second recorded change'},
    ],
    truncated: false,
  };
}
function sourcePayload(overrides = {}) {
  return {
    workspace: 'A',
    source: {
      path: 'main.rs', revision: REVISION_B, kind: 'source', content: 'fn before() {}\nfn after() {}',
      sha256: 'c'.repeat(64), start_line: 1, end_line: 2, total_lines: 2,
      redacted: false, truncated: false, line_ending: 'lf', ...overrides,
    },
  };
}
function environment(mutation) {
  const host = {innerHTML: '', querySelectorAll: () => [], querySelector: () => null};
  const requests = [];
  const state = {current: 'A', workspaceEpoch: 1, language: 'en', project: null};
  const context = {
    state, q: () => host, URLSearchParams, AbortController,
    localized: (en) => en,
    esc: value => String(value).replace(/[&<>"']/g, ch => ({'&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'}[ch])),
    requestFailureMessage: error => error.message,
    uiJson: (url, method, body, options) => new Promise((resolve, reject) => requests.push({url, method, body, options, resolve, reject})),
    num: value => String(value),
    pill: (label, cls = '') => `<span class="${cls}">${label}</span>`,
  };
  vm.createContext(context);
  let text = source;
  if (mutation) {
    text = mutation(text);
    assert.notEqual(text, source, 'an ablation must alter the production source snapshot');
  }
  vm.runInContext(text + '\nglobalThis.api={renderChangeInspector,loadHistoricalRevisions,openHistoricalSource,bindHistoricalSourcePanel,historicalState,validHistoricalRevisionList,validHistoricalSourceResponse,historicalRevisionId};', context);
  const current = {path: 'main.rs', layer: 'working', data: null, symbolImpact: null, loading: false, error: ''};
  state.changeInspection = current;
  return {state, current, host, requests, api: context.api};
}
function render(s) {
  s.api.renderChangeInspector();
  return s.host.innerHTML;
}
async function test(name, fn) {
  try { await fn(); results.push({name, passed: true}); }
  catch (error) { results.push({name, passed: false, error: error.stack}); }
}
function replaceOnce(text, before, after) {
  assert.equal(text.split(before).length, 2, 'ablation anchor must occur exactly once: ' + before);
  return text.replace(before, after);
}

(async () => {
  await test('revision list is backend-bounded and rendered as a selector, never free text', async () => {
    const s = environment(), html = render(s);
    assert.match(html, /Browse file history/);
    assert.match(html, /no revision expression is accepted/);
    const listed = s.api.loadHistoricalRevisions(s.current);
    assert.equal(s.requests.length, 1);
    assert.equal(s.requests[0].url, '/intelligence/revisions?limit=32');
    assert.equal(s.requests[0].method, 'GET');
    assert.equal(s.requests[0].options.workspace, 'A');
    assert.equal(s.requests[0].body, undefined);
    s.requests[0].resolve(fixture());
    await listed;
    const panel = render(s);
    assert.match(panel, /<select data-historical-revision>/);
    assert.equal(/data-historical-revision-input/.test(panel), false);
    assert.equal((panel.match(/<option value="/g) || []).length, 2);
    assert.match(panel, /bbbbbbb · 2026-11-02 · second recorded change/);
    assert.equal(s.current.historical.revisions.length, 2);
  });

  await test('historical read pins workspace, inspected path, exact revision and start line', async () => {
    const s = environment();
    s.current.historical = {revisions: fixture().revisions, revisionsLoading: false, revisionsError: '',
      revisionsController: null, revision: REVISION_B, source: null, sourceLoading: false, sourceError: '', controller: null};
    const pending = s.api.openHistoricalSource(s.current, REVISION_B, 1);
    assert.equal(s.requests.length, 1);
    const request = s.requests[0], params = new URLSearchParams(request.url.split('?')[1]);
    assert.equal(request.url.split('?')[0], '/intelligence/historical-source');
    assert.equal(request.method, 'GET');
    assert.equal(params.get('path'), 'main.rs');
    assert.equal(params.get('revision'), REVISION_B);
    assert.equal(params.get('start_line'), '1');
    assert.equal(request.options.workspace, 'A');
    assert.equal(request.body, undefined);
    request.resolve(sourcePayload());
    assert.equal(await pending, true);
    const html = render(s);
    assert.match(html, /fn before\(\) \{\}/);
    assert.match(html, /bbbbbbb/);
    assert.match(html, /sha256/);
    assert.match(html, /bound to the exact commit shown above/);
    assert.match(html, /data-historical-page="3"/);
  });

  await test('binary, redacted, directory and truncated revisions stay explicit', async () => {
    for (const [kind, message] of [
      ['binary', /Binary content at this revision/],
      ['redacted', /bounded Git output for this revision was redacted/],
      ['directory', /This path is a directory at the selected revision/],
      ['truncated', /exceeds the bounded historical read window/],
    ]) {
      const s = environment();
      s.current.historical = {revisions: fixture().revisions, revisionsLoading: false, revisionsError: '',
        revisionsController: null, revision: REVISION_B, source: null, sourceLoading: false, sourceError: '', controller: null};
      const pending = s.api.openHistoricalSource(s.current, REVISION_B, 1);
      s.requests[0].resolve(sourcePayload({
        kind, content: '', sha256: '', start_line: 1, end_line: 0, total_lines: 0,
        redacted: kind === 'redacted', truncated: kind === 'truncated',
      }));
      assert.equal(await pending, true);
      assert.match(render(s), message);
    }
  });

  await test('unknown, missing and protected revisions fail closed with distinct states', async () => {
    for (const [code, message] of [
      ['unknown_revision', /no longer available in this repository/],
      ['source_missing', /does not exist at the selected revision/],
      ['path_restricted', /restricted by the workspace protected-path policy/],
    ]) {
      const s = environment();
      s.current.historical = {revisions: fixture().revisions, revisionsLoading: false, revisionsError: '',
        revisionsController: null, revision: REVISION_B, source: null, sourceLoading: false, sourceError: '', controller: null};
      const pending = s.api.openHistoricalSource(s.current, REVISION_B, 1);
      s.requests[0].reject(Object.assign(new Error('HTTP 404'), {status: 404, code}));
      assert.equal(await pending, false);
      assert.equal(s.current.historical.source, null);
      assert.match(render(s), message);
    }
  });

  await test('a revision the backend never reported cannot be requested', async () => {
    const s = environment();
    s.current.historical = {revisions: fixture().revisions, revisionsLoading: false, revisionsError: '',
      revisionsController: null, revision: '', source: null, sourceLoading: false, sourceError: '', controller: null};
    assert.equal(await s.api.openHistoricalSource(s.current, 'f'.repeat(40), 1), false);
    assert.equal(await s.api.openHistoricalSource(s.current, 'HEAD~1', 1), false);
    assert.equal(s.requests.length, 0);
  });

  await test('stale, foreign or malformed responses never populate historical source', async () => {
    for (const payload of [
      sourcePayload({path: 'other.rs'}),
      sourcePayload({revision: REVISION_A}),
      sourcePayload({sha256: 'short'}),
      sourcePayload({kind: 'binary', content: 'x'}),
      sourcePayload({start_line: '1', end_line: 2}),
      {workspace: 'B', source: sourcePayload().source},
    ]) {
      const s = environment();
      s.current.historical = {revisions: fixture().revisions, revisionsLoading: false, revisionsError: '',
        revisionsController: null, revision: REVISION_B, source: null, sourceLoading: false, sourceError: '', controller: null};
      const pending = s.api.openHistoricalSource(s.current, REVISION_B, 1);
      s.requests[0].resolve(payload);
      assert.equal(await pending, false);
      assert.equal(s.current.historical.source, null);
      assert.match(render(s), /Invalid historical source response/);
    }
    const stale = environment();
    stale.current.historical = {revisions: fixture().revisions, revisionsLoading: false, revisionsError: '',
      revisionsController: null, revision: REVISION_B, source: null, sourceLoading: false, sourceError: '', controller: null};
    const pending = stale.api.openHistoricalSource(stale.current, REVISION_B, 1);
    stale.state.current = 'B';
    stale.state.workspaceEpoch += 1;
    stale.requests[0].resolve(sourcePayload());
    assert.equal(await pending, false);
    assert.equal(stale.current.historical.source, null);
    assert.equal(stale.current.historical.sourceLoading, false);
  });

  await test('paging stays inside the 240-line window and re-pins the same revision', async () => {
    const s = environment();
    s.current.historical = {revisions: fixture().revisions, revisionsLoading: false, revisionsError: '',
      revisionsController: null, revision: REVISION_B, source: null, sourceLoading: false, sourceError: '', controller: null};
    const first = s.api.openHistoricalSource(s.current, REVISION_B, 1);
    s.requests[0].resolve(sourcePayload({
      content: Array.from({length: 240}, (_, index) => 'line ' + index).join('\n'),
      start_line: 1, end_line: 240, total_lines: 500,
    }));
    assert.equal(await first, true);
    const html = render(s);
    assert.equal((html.match(/class="historical-source-line"/g) || []).length, 240);
    assert.match(html, /data-historical-page="241"/);
    const buttons = [];
    s.host.querySelectorAll = selector => selector === '[data-historical-page]'
      ? [
        {disabled: true, dataset: {historicalPage: '1'}, addEventListener: (name, handler) => buttons.push(handler)},
        {disabled: false, dataset: {historicalPage: '241'}, addEventListener: (name, handler) => buttons.push(handler)},
      ]
      : [];
    s.host.querySelector = () => null;
    s.api.bindHistoricalSourcePanel(s.current);
    assert.equal(buttons.length, 2);
    assert.equal(s.requests.length, 1);
    buttons[1]();
    assert.equal(s.requests.length, 2);
    const params = new URLSearchParams(s.requests[1].url.split('?')[1]);
    assert.equal(params.get('start_line'), '241');
    assert.equal(params.get('revision'), REVISION_B);
  });

  await test('closing history aborts in-flight work and clears the panel', async () => {
    const s = environment();
    s.current.historical = {revisions: fixture().revisions, revisionsLoading: false, revisionsError: '',
      revisionsController: null, revision: REVISION_B, source: null, sourceLoading: false, sourceError: '', controller: null};
    const pending = s.api.openHistoricalSource(s.current, REVISION_B, 1);
    let closeHandler = null;
    s.host.querySelector = selector => selector === '[data-historical-close]'
      ? {addEventListener: (name, handler) => { closeHandler = handler; }}
      : null;
    s.api.bindHistoricalSourcePanel(s.current);
    assert.equal(typeof closeHandler, 'function');
    closeHandler();
    assert.equal(s.requests[0].options.signal.aborted, true);
    assert.equal(s.current.historical.revisions, null);
    s.requests[0].resolve(sourcePayload());
    assert.equal(await pending, false);
    const html = render(s);
    assert.equal(/<option value="/.test(html), false);
    assert.equal(/fn before/.test(html), false);
    assert.match(html, /Browse file history/);
  });

  await test('production bundle keeps the historical source module in the shipped script', () => {
    const manifest = fs.readFileSync(path.join(root, 'src/ui/intelligence_web.rs'), 'utf8');
    assert.match(manifest, /include_str!\("intelligence_web\/app\/historical_source\.js"\)/);
    new vm.Script(source, {filename: 'historical-source-bundle.js'});
  });

  const ablations = [
    {
      id: 'reported-revision-membership',
      hypothesis: 'A revision the backend never reported cannot become a historical read.',
      mutation: value => replaceOnce(value,
        '|| !historical.revisions.some(row => historicalRevisionId(row.revision) === requested)) return false;',
        '|| false) return false;'),
      observe: s => {
        s.api.openHistoricalSource(s.current, 'f'.repeat(40), 1);
        return {invariant: s.requests.length === 0, request_count: s.requests.length};
      },
    },
    {
      id: 'selector-without-free-text',
      hypothesis: 'The revision selector offers only backend-reported revisions.',
      mutation: value => replaceOnce(value,
        '<select data-historical-revision>${options}</select>',
        '<input data-historical-revision-input value="${esc(historical.revision)}">'),
      observe: s => {
        const html = render(s);
        return {invariant: html.includes('<select data-historical-revision>') && !html.includes('data-historical-revision-input'),
          select: html.includes('<select data-historical-revision>'), free_text: html.includes('data-historical-revision-input')};
      },
    },
  ];
  for (const {id, hypothesis, mutation, observe} of ablations) {
    const baselineEnvironment = environment();
    baselineEnvironment.current.historical = {revisions: fixture().revisions, revisionsLoading: false,
      revisionsError: '', revisionsController: null, revision: REVISION_B, source: null,
      sourceLoading: false, sourceError: '', controller: null};
    const baseline = observe(baselineEnvironment);
    assert.equal(baseline.invariant, true, id + ': production baseline must satisfy the invariant');
    const ablatedEnvironment = environment(mutation);
    ablatedEnvironment.current.historical = {revisions: fixture().revisions, revisionsLoading: false,
      revisionsError: '', revisionsController: null, revision: REVISION_B, source: null,
      sourceLoading: false, sourceError: '', controller: null};
    const ablated = observe(ablatedEnvironment);
    assert.equal(ablated.invariant, false, id + ': isolated ablation must violate the invariant');
    const restoredEnvironment = environment();
    restoredEnvironment.current.historical = {revisions: fixture().revisions, revisionsLoading: false,
      revisionsError: '', revisionsController: null, revision: REVISION_B, source: null,
      sourceLoading: false, sourceError: '', controller: null};
    const restored = observe(restoredEnvironment);
    assert.equal(restored.invariant, true, id + ': fresh production baseline must recover');
    assert.deepEqual(restored, baseline, id + ': restoration must reproduce baseline observations');
    results.push({name: 'ablation: ' + id, passed: true, hypothesis, baseline, ablated, restored});
  }

  assert.equal(
    digest(fs.readFileSync(sourcePath, 'utf8').replace(/\r\n?/g, '\n')),
    sourceDigest,
    'production source changed while the diagnostic was running',
  );
  console.log(JSON.stringify({results}));
  if (results.some(result => !result.passed)) process.exitCode = 1;
})();
