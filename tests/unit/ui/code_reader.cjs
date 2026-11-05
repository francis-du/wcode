'use strict';
// Shipped code-reader renderers and revision guards, with deterministic DOM and HTTP.
// This suite does not claim native browser layout or backend authorization coverage.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const root = process.argv[2] || path.resolve(__dirname, '../../..');
const APP = file => fs.readFileSync(path.join(root, 'src/ui/intelligence_web/app', file), 'utf8');
const results = [];
const flush = () => new Promise(setImmediate);

function productionSource() {
  return ['code_reader.js', 'change_ranges.js', 'features_requirements.js', 'features_changes.js', 'code_graph.js', 'design_sources.js', 'structure.js']
    .map(APP).join('\n');
}
function escapeHtml(value) {
  return String(value ?? '—').replace(/[&<>"']/g, ch => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  }[ch]));
}
function makeHost() {
  const host = {
    innerHTML: '', textContent: '', value: '', disabled: false, onclick: null,
    classes: new Set(), attrs: {},
    classList: {
      add: name => host.classes.add(name),
      remove: name => host.classes.delete(name),
      toggle: (name, on) => (on ? host.classes.add(name) : host.classes.delete(name)),
      contains: name => host.classes.has(name),
    },
    setAttribute(name, value) { host.attrs[name] = String(value); },
    getAttribute(name) { return host.attrs[name] ?? null; },
    addEventListener() {}, focus() {}, scrollIntoView() {}, blur() {},
    querySelector() { return null; }, querySelectorAll() { return []; },
  };
  return host;
}
function projectFixture() {
  return {
    workspace: 'A',
    proof: { revision_code: 'sha256:code', revision_design: 'sha256:design' },
    repository_revision: { code: 'sha256:code', design: 'sha256:design' },
    structure: { entries: [{ path: 'src/a.rs', language: 'rust', lines: 3 }], largest_files: [], truncated: false },
    requirements: [{
      id: 'REQ-A', title: 'Requirement A', intent: 'read the mapped file', priority: 'high', status: 'complete',
      convergence: 'stable', components: [{
        id: 'component:a', name: 'A', responsibilities: [], depends_on: [], implementation_lines: 3, changed: true,
        changed_paths: ['src/a.rs'],
        implementation: [{ target: 'src/a.rs::symbol', path: 'src/a.rs', symbol: 'symbol', resolved: true, provider: 'tree-sitter', precision: 'syntax', changed: true }],
      }, {
        id: 'component:a-qualified', name: 'A qualified', responsibilities: [], depends_on: [], implementation_lines: 1, changed: false,
        changed_paths: [],
        implementation: [{ target: 'src/a.rs::Impl::method', path: 'src/a.rs', symbol: 'Impl::method', resolved: true, provider: 'tree-sitter', precision: 'syntax' }],
      }], acceptance: [{ id: 'AC-A', title: 'Acceptance', statement: 'read only', verification: [] }],
      constraints: [], drift: [], implementation_lines: 3, implementation_files: 1, implementation_symbols: 1,
    }],
    changes: [{ path: 'src/a.rs', status: 'modified', additions: 1, deletions: 1, affected_requirements: ['REQ-B'], scope: 'source' }],
  };
}
function graphFixture() {
  const provenance = { provider: 'tree-sitter', precision: 'syntax', revision: 'sha256:' + 'a'.repeat(64) };
  return {
    snapshot_id: 'GRAPH-1', captured_at_ms: 1, provider: 'wcode-composite', precision: 'mixed',
    query: 'file:src/a.rs', mode: 'all', depth: 2, root_ids: ['file:src/a.rs'],
    nodes: [
      { node: { id: 'file:src/a.rs', kind: 'file', label: 'src/a.rs', attributes: { path: 'src/a.rs', language: 'rust' }, provenance }, distance: 0, upstream: false, downstream: false },
      { node: { id: 'symbol:1', kind: 'function', label: 'symbol', attributes: { path: 'src/a.rs', name: 'symbol', qualified_name: 'symbol', symbol_kind: 'function', source_kind: 'function', language: 'rust', range: { start_line: 1, end_line: 3 } }, provenance }, distance: 1, upstream: false, downstream: false },
      { node: { id: 'symbol:2', kind: 'function', label: 'other', attributes: { path: 'src/other.rs', name: 'other', qualified_name: 'other', symbol_kind: 'function', source_kind: 'function', language: 'rust', range: { start_line: 4, end_line: 6 } }, provenance }, distance: 1, upstream: false, downstream: false },
      { node: { id: 'symbol:3', kind: 'function', label: 'Impl::method', attributes: { path: 'src/a.rs', name: 'method', qualified_name: 'Impl::method', symbol_kind: 'function', source_kind: 'function', language: 'rust', range: { start_line: 2, end_line: 2 } }, provenance }, distance: 1, upstream: false, downstream: false },
    ],
    edges: [], precision_counts: {}, upstream_nodes: 0, downstream_nodes: 0, truncated: false,
  };
}
function sourceFixture(overrides = {}) {
  return {
    workspace: 'A',
    source: {
      snapshot_id: 'GRAPH-1', node_id: 'file:src/a.rs', path: 'src/a.rs', provider: 'tree-sitter', precision: 'syntax',
      source_revision: 'sha256:' + 'a'.repeat(64), current_sha256: 'a'.repeat(64),
      start_line: 1, end_line: 3, total_lines: 3, focus_start_line: 1, focus_end_line: 3,
      content: 'line one\nline two\nline three', redacted: false, truncated: false, line_ending: 'lf', editable: false,
      ...overrides,
    },
  };
}
function changeFixture(overrides = {}) {
  return {
    path: 'src/a.rs', layer: 'working', snapshot_id: 'b'.repeat(64), head: 'c'.repeat(40),
    index_fingerprint: 'd'.repeat(64), worktree_sha256: 'a'.repeat(64), kind: 'unified_diff',
    content: 'diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@\n line one\n-line two\n+line two changed\n line three\n',
    before_changed_ranges: [{ start_line: 2, end_line: 2 }], after_changed_ranges: [{ start_line: 2, end_line: 2 }],
    changed_ranges_truncated: false, after_source_matches_worktree: true, truncated: false, redacted: false,
    ...overrides,
  };
}
function setup({ ablate = null } = {}) {
  let text = productionSource();
  if (ablate) {
    text = ablate(text);
    assert.notEqual(text, productionSource(), 'an ablation must alter the production source snapshot');
  }
  const hosts = new Map();
  const host = id => {
    if (!hosts.has(id)) hosts.set(id, makeHost());
    return hosts.get(id);
  };
  const requests = [], navigation = [];
  const state = {
    current: 'A', workspaceEpoch: 1, codeReader: null, codeGraph: null, codeGraphError: '',
    selected: '', filter: 'all', workspaceTab: 'requirements', architectureView: 'blueprint',
    rendered: new Map(), project: projectFixture(),
  };
  const context = {
    state, console, URLSearchParams, AbortController, token: 'ui-token',
    els: {
      codeReaderPanel: host('#codeReaderPanel'), codeReaderPath: host('#codeReaderPath'), codeReaderBody: host('#codeReaderBody'),
      codeReaderClose: host('#codeReaderClose'), codeReaderReload: host('#codeReaderReload'),
      search: host('#reqSearch'), detail: host('#featureDetail'), requirements: host('#requirements'), reqCount: host('#reqCount'),
      fileTree: host('#fileTree'), fileSearch: host('#fileSearch'), fileSearchStatus: host('#fileSearchStatus'),
      structureSummary: host('#structureSummary'), largeFiles: host('#largeFiles'),
    },
    document: { querySelectorAll: () => [], getElementById: id => host(id.startsWith('#') ? id : `#${id}`) },
    esc: escapeHtml, localized: en => en, num: value => String(Number(value || 0)),
    pill: (label, cls = '') => `<span class="pill ${cls}">${escapeHtml(label)}</span>`,
    statusLabel: value => String(value || '').replaceAll('_', ' '),
    statusClass: value => ({ complete: 'good', aligned: 'good', stable: 'good', modified: 'warn', untracked: 'info' }[value] || 'info'),
    unit: (value, singular, plural) => `${Number(value || 0)} ${Number(value) === 1 ? singular : plural}`,
    t: key => key, changeNums: item => `+${Number(item.additions || 0)}/-${Number(item.deletions || 0)}`,
    uiJson: (url, method, body, options) => new Promise((resolve, reject) => requests.push({ url, method, body, options, resolve, reject })),
    requestFailureMessage: error => error.message,
    revealSection: id => navigation.push({ type: 'reveal', id }),
    invalidate: () => {},
    setHtml: (key, node, html, bind) => { node.innerHTML = html; bind?.(); return true; },
    observationStamp: () => ({ workspace: 'A', view: 1, mutation: 0, sequence: 1 }),
    observationCurrent: () => true,
  };
  vm.createContext(context);
  vm.runInContext(text + '\nglobalThis.api={openCodeReader,closeCodeReader,clearCodeReader,invalidateCodeReader,renderCodeReader,reloadCodeReader,codeReaderSourceAction,bindCodeReaderActions,codeReaderChangeOverlays,codeReaderRequirements,codeReaderGotoLine,openCodeReaderRequirement,changeTable,implementationRows,renderProjectStructure,repositorySourcePaths,renderDetail};', context);
  const loadGraph = async () => { state.codeGraph = graphFixture(); return true; };
  return { state, host, requests, navigation, api: context.api, loadGraph };
}
async function openAndResolve(s, { source = sourceFixture(), change = changeFixture(), revision } = {}) {
  const pending = s.api.openCodeReader('src/a.rs', { loadGraph: s.loadGraph });
  await flush();
  assert.equal(s.requests.length, 2, 'reader must request source and change once');
  assert.match(s.requests[0].url, /\/intelligence\/code-source\?/);
  assert.match(s.requests[1].url, /\/intelligence\/change-detail\?/);
  s.requests[0].resolve(source);
  s.requests[1].resolve({ workspace: 'A', repository_revision: revision || { code: 'sha256:code', design: 'sha256:design' }, change });
  assert.equal(await pending, true);
  await flush();
  return s;
}
async function test(name, fn) {
  try { await fn(); results.push({ name, passed: true }); }
  catch (error) { results.push({ name, passed: false, error: error.stack }); }
}
(async () => {
  await test('reader renders paged line-numbered source, change map, requirements and identity', async () => {
    const s = setup();
    await openAndResolve(s);
    const html = s.host('#codeReaderBody').innerHTML;
    assert.match(html, /Read-only source/);
    assert.match(html, /data-code-reader-line="1"/);
    assert.match(html, /data-code-reader-line="2"/);
    assert.match(html, /data-code-reader-line="3"/);
    assert.match(html, /code-reader-line context focus" data-code-reader-line="1"/);
    assert.match(html, /code-reader-line added focus" data-code-reader-line="2"/);
    assert.match(html, /data-reader-goto="2"/);
    assert.match(html, /Removed baseline lines/);
    assert.match(html, /data-reader-requirement="REQ-A"/);
    assert.match(html, /data-reader-requirement="REQ-B"/);
    assert.match(html, /Captured snapshot identity/);
    assert.match(html, /sha256:code/);
    assert.doesNotMatch(html, /contenteditable/i);
    assert.doesNotMatch(html, /data-code-edit/);
  });
  await test('the reader exposes the three change layers and marks the active one', async () => {
    const s = setup();
    await openAndResolve(s);
    const layers = s.host('#codeReaderLayers').innerHTML;
    assert.match(layers, /data-code-reader-layer="working"/);
    assert.match(layers, /data-code-reader-layer="staged"/);
    assert.match(layers, /data-code-reader-layer="unstaged"/);
    assert.match(layers, /data-code-reader-layer="working" aria-pressed="true"/);
    assert.match(layers, /data-code-reader-layer="staged" aria-pressed="false"/);
  });
  await test('change overlay distinguishes added, context and removed baseline lines', async () => {
    const s = setup();
    await openAndResolve(s);
    const html = s.host('#codeReaderBody').innerHTML;
    assert.match(html, /code-reader-line added focus" data-code-reader-line="2"/);
    assert.match(html, /code-reader-line context focus" data-code-reader-line="1"/);
    assert.match(html, /code-reader-line context focus" data-code-reader-line="3"/);
    assert.match(html, /code-reader-line removed/);
    assert.match(html, /line two/);
    const overlays = s.api.codeReaderChangeOverlays(changeFixture(), sourceFixture().source);
    assert.equal(overlays.overlays.get(2), 'added');
    assert.equal(overlays.overlays.get(1), 'context');
    assert.deepEqual([...overlays.removed].map(row => row.line), [2]);
  });
  await test('untracked source marks every source line as added without inventing a baseline', async () => {
    const s = setup();
    const untracked = changeFixture({
      kind: 'untracked_source', before_changed_ranges: [], after_changed_ranges: [{ start_line: 1, end_line: 3 }],
      content: 'line one\nline two\nline three',
    });
    await openAndResolve(s, { change: untracked });
    const html = s.host('#codeReaderBody').innerHTML;
    assert.match(html, /code-reader-line added focus" data-code-reader-line="1"/);
    assert.match(html, /code-reader-line added focus" data-code-reader-line="2"/);
    assert.match(html, /code-reader-line added focus" data-code-reader-line="3"/);
    assert.doesNotMatch(html, /Removed baseline lines/);
  });
  await test('binary change stays explicit and never claims a text overlay', async () => {
    const s = setup();
    await openAndResolve(s, { change: changeFixture({ kind: 'binary', content: '' }) });
    const html = s.host('#codeReaderBody').innerHTML;
    assert.match(html, /Binary difference; text preview is unavailable/);
    assert.doesNotMatch(html, /data-reader-goto/);
  });
  await test('invalid path, missing revision and missing graph node fail closed without source requests', async () => {
    const invalidPath = setup();
    assert.equal(await invalidPath.api.openCodeReader('../secret.rs', { loadGraph: invalidPath.loadGraph }), false);
    assert.equal(invalidPath.requests.length, 0);
    assert.equal(invalidPath.state.codeReader, null);

    const noRevision = setup();
    noRevision.state.project.proof = {};
    assert.equal(await noRevision.api.openCodeReader('src/a.rs', { loadGraph: noRevision.loadGraph }), false);
    assert.equal(noRevision.requests.length, 0);
    assert.match(noRevision.host('#codeReaderBody').innerHTML, /Source revision unavailable/);

    const missingNode = setup();
    missingNode.loadGraph = async () => { missingNode.state.codeGraph = { ...graphFixture(), nodes: [] }; return true; };
    assert.equal(await missingNode.api.openCodeReader('src/a.rs', { loadGraph: missingNode.loadGraph }), false);
    assert.equal(missingNode.requests.length, 0);
    assert.equal(missingNode.state.codeReader.missing, true);
    assert.match(missingNode.host('#codeReaderBody').innerHTML, /not available in the current bounded graph/);
    assert.match(missingNode.host('#codeReaderBody').innerHTML, /absence here does not prove the file is missing/);
  });
  await test('workspace and epoch guards reject late responses without publishing source', async () => {
    const changes = {
      workspace: s => { s.state.current = 'B'; },
      epoch: s => { s.state.workspaceEpoch++; },
    };
    for (const [name, apply] of Object.entries(changes)) {
      const s = setup();
      const pending = s.api.openCodeReader('src/a.rs', { loadGraph: s.loadGraph });
      await flush();
      apply(s);
      s.requests[0].resolve(sourceFixture());
      s.requests[1].resolve({ workspace: 'A', repository_revision: { code: 'sha256:code', design: 'sha256:design' }, change: changeFixture() });
      assert.equal(await pending, false, name);
      assert.equal(s.state.codeReader.source, null, name);
      assert.doesNotMatch(s.host('#codeReaderBody').innerHTML, /data-code-reader-line/, name);
    }
  });
  await test('a changed repository revision keeps the reader explicitly stale instead of silently rebasing', async () => {
    const s = setup();
    const pending = s.api.openCodeReader('src/a.rs', { loadGraph: s.loadGraph });
    await flush();
    s.state.project.proof.revision_code = 'sha256:newer';
    s.requests[0].resolve(sourceFixture());
    s.requests[1].resolve({ workspace: 'A', repository_revision: { code: 'sha256:code', design: 'sha256:design' }, change: changeFixture() });
    await pending;
    s.api.renderCodeReader();
    assert.ok(s.state.codeReader.source, 'stale source stays visible but marked');
    assert.match(s.host('#codeReaderBody').innerHTML, /earlier project revision/);
  });
  await test('foreign or malformed source/change responses never populate the reader', async () => {
    const cases = [
      { source: { workspace: 'B', source: sourceFixture().source } },
      { source: sourceFixture({ ...sourceFixture().source, path: 'other.rs' }) },
      { source: sourceFixture({ ...sourceFixture().source, current_sha256: 'b'.repeat(64) }) },
      { source: sourceFixture({ ...sourceFixture().source, content: 'too many\nrows' }) },
      { change: changeFixture({ path: 'other.rs' }) },
      { change: changeFixture({ snapshot_id: 'bad' }) },
      { change: changeFixture({ content: 'x'.repeat(65537) }) },
      { change: changeFixture({ after_changed_ranges: [{ start_line: 9, end_line: 3 }] }) },
      { change: changeFixture({ changed_ranges_truncated: 'no' }) },
      { change: changeFixture({ after_source_matches_worktree: 'yes' }) },
    ];
    for (const broken of cases) {
      const s = setup();
      const pending = s.api.openCodeReader('src/a.rs', { loadGraph: s.loadGraph });
      await flush();
      s.requests[0].resolve(broken.source || sourceFixture());
      s.requests[1].resolve({ workspace: 'A', repository_revision: { code: 'sha256:code', design: 'sha256:design' }, change: broken.change || changeFixture() });
      await pending;
      const label = JSON.stringify(broken).slice(0, 80);
      if (broken.source) assert.equal(s.state.codeReader.source, null, label);
      else assert.equal(s.state.codeReader.change, null, label);
      assert.ok(s.state.codeReader.sourceError || s.state.codeReader.changeError, label);
    }
  });
  await test('requirement links and entry-point actions navigate back to the requirement detail', async () => {
    const s = setup();
    await openAndResolve(s);
    assert.equal(s.api.openCodeReaderRequirement('REQ-A'), true);
    assert.equal(s.state.selected, 'REQ-A');
    assert.equal(s.state.filter, 'all');
    assert.equal(s.state.codeReader, null);
    assert.deepEqual(s.navigation, [{ type: 'reveal', id: 'requirementsSection' }]);
    const reference = { target: 'src/a.rs::symbol', path: 'src/a.rs', symbol: 'symbol', resolved: true, provider: 'tree-sitter', precision: 'syntax' };
    const action = s.api.codeReaderSourceAction(reference, s.api.repositorySourcePaths());
    assert.match(action, /data-reader-path="src\/a.rs"/);
    assert.match(action, /Read source/);
    assert.equal(s.api.codeReaderSourceAction({ target: 'cargo.test', path: 'cargo.test', provider: 'harness' }, s.api.repositorySourcePaths()), '');
    const table = s.api.changeTable([{ path: 'src/a.rs', status: 'modified', additions: 1, deletions: 1, affected_requirements: ['REQ-A'] }], false);
    assert.match(table, /data-change-path="src\/a.rs"/);
    assert.match(table, /data-reader-path="src\/a.rs"/);
    assert.match(table, /Open in code reader/);
    s.api.renderProjectStructure();
    const tree = s.host('#fileTree').innerHTML;
    assert.match(tree, /data-reader-path="src\/a.rs"/);
    assert.match(tree, /tree-file-reader/);
  });
  await test('requirement change table renders bounded ranges with a jump target', () => {
    const s = setup();
    const table = s.api.changeTable([{
      path: 'src/a.rs', status: 'modified', additions: 2, deletions: 1,
      before_changed_ranges: [{ start_line: 10, end_line: 11 }],
      after_changed_ranges: [{ start_line: 12, end_line: 13 }],
      changed_ranges_truncated: false,
    }], true);
    assert.match(table, /L10–11 → L12–13/);
    assert.match(table, /data-change-path="src\/a.rs"/);
    assert.match(table, /data-change-layer="working"/);
    assert.match(table, /data-change-focus-line="12"/);
    assert.match(table, /data-label="Changed lines"/);
  });
  await test('requirement change table keeps unavailable and truncated ranges explicit', () => {
    const s = setup();
    const unavailable = s.api.changeTable([{ path: 'src/a.rs', status: 'modified' }], true);
    assert.match(unavailable, /ranges not retained/);
    assert.doesNotMatch(unavailable, /L0|→ L0/);
    const truncated = s.api.changeTable([{
      path: 'src/b.rs', status: 'modified',
      before_changed_ranges: [{ start_line: 1, end_line: 2 }],
      after_changed_ranges: [{ start_line: 1, end_line: 2 }],
      changed_ranges_truncated: true,
    }], true);
    assert.match(truncated, /ranges truncated/);
    assert.match(truncated, /L1–2 → L1–2/);
  });
  await test('symbol outline lists this file\'s observed definitions and links their requirements', async () => {
    const s = setup();
    await openAndResolve(s);
    const html = s.host('#codeReaderBody').innerHTML;
    assert.match(html, /Symbols in this file/);
    assert.match(html, /data-reader-symbol-line="1"/);
    assert.match(html, /code-reader-symbol-name">symbol</);
    assert.match(html, /code-reader-symbol-kind">function</);
    assert.match(html, /code-reader-symbol-range">L1–L3/);
    assert.match(html, /code-reader-symbol-name">Impl::method</);
    assert.match(html, /code-reader-symbol-range">L2–L2/);
    assert.doesNotMatch(html, /code-reader-symbol-name">other</, 'a foreign path must not enter this outline');
    assert.match(html, /code-reader-symbol-reqs[\s\S]*data-reader-requirement="REQ-A"/);
    const chips = html.match(/data-reader-requirement="REQ-A"/g) || [];
    assert.equal(chips.length, 3, 'the file requirement row plus both mapped symbols carry the requirement');
  });
  await test('symbol outline stays an explicit observation gap when the graph carries no symbol', async () => {
    const s = setup();
    s.loadGraph = async () => {
      s.state.codeGraph = { ...graphFixture(), nodes: graphFixture().nodes.slice(0, 1) };
      return true;
    };
    await openAndResolve(s);
    const html = s.host('#codeReaderBody').innerHTML;
    assert.match(html, /No symbol in this file is present in the current bounded graph/);
    assert.match(html, /absence here does not prove the file defines none/);
    assert.doesNotMatch(html, /data-reader-symbol-line/);
  });
  await test('symbol outline marks only definitions whose lines the captured change overlaps', async () => {
    const touched = setup();
    await openAndResolve(touched);
    assert.match(touched.host('#codeReaderBody').innerHTML, /code-reader-symbol changed/);

    const untouched = setup();
    await openAndResolve(untouched, {
      change: changeFixture({
        before_changed_ranges: [{ start_line: 9, end_line: 9 }],
        after_changed_ranges: [{ start_line: 9, end_line: 9 }],
      }),
    });
    const html = untouched.host('#codeReaderBody').innerHTML;
    assert.doesNotMatch(html, /code-reader-symbol changed/);
    assert.match(html, /data-reader-symbol-line="1"/, 'the outline stays available without a changed marker');
  });
  await test('malformed or non-definition symbol nodes fail closed without entering the outline', async () => {
    const s = setup();
    const provenance = { provider: 'tree-sitter', precision: 'syntax', revision: 'sha256:' + 'a'.repeat(64) };
    s.loadGraph = async () => {
      s.state.codeGraph = {
        ...graphFixture(),
        nodes: [
          ...graphFixture().nodes,
          { node: { id: 'symbol:bad-range', kind: 'function', label: 'bad range', attributes: { path: 'src/a.rs', name: 'bad range', qualified_name: 'bad range', source_kind: 'function', range: { start_line: 0, end_line: 2 } }, provenance }, distance: 1, upstream: false, downstream: false },
          { node: { id: 'symbol:bad-kind', kind: 'file', label: 'bad kind', attributes: { path: 'src/a.rs', name: 'bad kind', qualified_name: 'bad kind', source_kind: 'file', range: { start_line: 1, end_line: 2 } }, provenance }, distance: 1, upstream: false, downstream: false },
          { node: { id: 'symbol:no-name', kind: 'function', label: '', attributes: { path: 'src/a.rs', source_kind: 'function', range: { start_line: 1, end_line: 2 } }, provenance }, distance: 1, upstream: false, downstream: false },
        ],
      };
      return true;
    };
    await openAndResolve(s);
    const html = s.host('#codeReaderBody').innerHTML;
    assert.doesNotMatch(html, /bad range|bad kind|no-name/);
    assert.match(html, /data-reader-symbol-line="1"/);
  });
  await test('symbol outline warns when the loaded graph snapshot is truncated', async () => {
    const s = setup();
    s.loadGraph = async () => {
      s.state.codeGraph = { ...graphFixture(), truncated: true };
      return true;
    };
    await openAndResolve(s);
    const html = s.host('#codeReaderBody').innerHTML;
    assert.match(html, /graph snapshot is truncated/);
    assert.match(html, /may omit definitions/);
    assert.match(html, /data-reader-symbol-line="1"/, 'the outline still renders');

    const complete = setup();
    await openAndResolve(complete);
    assert.doesNotMatch(complete.host('#codeReaderBody').innerHTML, /graph snapshot is truncated/);
  });
  await test('ablation: removing the changed-line overlap removes the marker but keeps the outline', async () => {
    const anchor = 'range.start_line <= symbol.endLine && range.end_line >= symbol.startLine';
    const baseline = setup();
    await openAndResolve(baseline);
    assert.match(baseline.host('#codeReaderBody').innerHTML, /code-reader-symbol changed/);

    const ablated = setup({
      ablate: text => {
        assert.equal(text.split(anchor).length, 2, 'ablation anchor must occur exactly once');
        return text.replace(anchor, 'false');
      },
    });
    await openAndResolve(ablated);
    const after = ablated.host('#codeReaderBody').innerHTML;
    assert.doesNotMatch(after, /code-reader-symbol changed/);
    assert.match(after, /data-reader-symbol-line="1"/);
    assert.match(after, /code-reader-symbol-name">symbol</);
    assert.match(after, /data-reader-requirement="REQ-A"/);
  });
  await test('requirement detail keeps an unavailable working-tree review as an explicit unknown', async () => {
    const s = setup();
    s.state.selected = 'REQ-A';
    s.state.project.git_review = { available: false, reason: 'cached_snapshot' };
    s.api.renderDetail();
    const unknown = s.host('#featureDetail').innerHTML;
    assert.match(unknown, /change state unknown/);
    assert.doesNotMatch(unknown, /no mapped implementation change/);
    assert.match(unknown, /mapped changes are unknown/);

    s.state.project.git_review = { available: true, reason: 'available' };
    s.api.renderDetail();
    const known = s.host('#featureDetail').innerHTML;
    assert.match(known, /no mapped implementation change/);
    assert.doesNotMatch(known, /change state unknown/);
  });
  await test('change table discloses the requirement chips it caps', () => {
    const s = setup();
    const ids = ['REQ-1', 'REQ-2', 'REQ-3', 'REQ-4', 'REQ-5', 'REQ-6', 'REQ-7', 'REQ-8'];
    const capped = s.api.changeTable([{ path: 'src/a.rs', status: 'modified', affected_requirements: ids }], false);
    assert.equal((capped.match(/data-req=/g) || []).length, 6, 'the table caps the chips');
    assert.match(capped, /\+2 more/, 'the capped count stays explicit');

    const exact = s.api.changeTable([{ path: 'src/a.rs', status: 'modified', affected_requirements: ids.slice(0, 6) }], false);
    assert.doesNotMatch(exact, /more/, 'six requirements need no overflow note');

    const none = s.api.changeTable([{ path: 'src/a.rs', status: 'modified' }], false);
    assert.match(none, /—/, 'no requirements stays an em dash');
  });
  await test('ablation: removing change-row parsing removes the overlay while source rendering remains', async () => {
    const anchor = 'for (const row of changeDisplayRows(change))';
    const baseline = setup();
    await openAndResolve(baseline);
    const before = baseline.host('#codeReaderBody').innerHTML;
    assert.match(before, /code-reader-line added focus" data-code-reader-line="2"/);
    assert.match(before, /data-reader-goto="2"/);

    const ablated = setup({
      ablate: text => {
        assert.equal(text.split(anchor).length, 2, 'ablation anchor must occur exactly once');
        return text.replace(anchor, 'for (const row of [])');
      },
    });
    await openAndResolve(ablated);
    const after = ablated.host('#codeReaderBody').innerHTML;
    assert.doesNotMatch(after, /code-reader-line added/);
    assert.doesNotMatch(after, /code-reader-line context/);
    assert.doesNotMatch(after, /Removed baseline lines/);
    assert.match(after, /data-reader-goto="2"/, 'the bounded range map remains independent of diff-row parsing');
    assert.match(after, /data-code-reader-line="1"/);
    assert.match(after, /data-code-reader-line="2"/);
    assert.match(after, /Read-only source/);
  });
  console.log(JSON.stringify({ suite: 'code-reader', results }));
  if (results.some(result => !result.passed)) process.exitCode = 1;
})();
