'use strict';
// Shipped renderers and graph/source protocol, with deterministic DOM and HTTP.
// This suite does not claim native browser layout or backend authorization coverage.
const assert = require('node:assert/strict');
const { test } = require('node:test');
const { sandbox, project, respond, flush } = require('./observatory.cjs');
const revision = { code: 'sha256:observed-code', design: 'sha256:observed-design' };
function reference(path, symbol, extra = {}) {
  return { target: symbol ? `${path}::${symbol}` : path, path, ...(symbol ? { symbol } : {}),
    resolved: true, provider: symbol ? 'tree-sitter' : 'filesystem', precision: symbol ? 'syntax' : 'deterministic', changed: false, ...extra };
}
function fixture() {
  const component = { id: 'component:engine', name: 'Engine', responsibilities: [], depends_on: [],
    implementation: [reference('src/mapped.rs', 'engine')], implementation_lines: 12, changed: false, changed_paths: [] };
  const requirement = { id: 'REQ-A', title: 'Inspect design implementation', intent: 'Connect mappings to files', priority: 'high',
    status: 'complete', convergence: 'stable', components: [component], constraints: [], drift: [],
    implementation_lines: 12, implementation_files: 1, implementation_symbols: 1,
    acceptance: [{ id: 'AC-A', title: 'Mapped test', statement: 'Inspect without running',
      verification: [reference('tests/mapped.rs', 'checks_engine'), reference('cargo.test', null, { provider: 'harness' })] }] };
  return { ...project(), requirements: [requirement], proof: { revision_code: revision.code, revision_design: revision.design },
    structure: { entries: [{ path: 'src/visible.rs', language: 'rust', lines: 10 }], largest_files: [], truncated: true },
    architecture: { components: [{ ...component, implementation_targets: ['src/mapped.rs::engine', 'src/not-observed.rs'],
      product_scopes: [], implementation_files: 1 }], dependencies: [] } };
}
function sourceHost(s, id) {
  const host = s.node(id); let markup = '', buttons = [];
  const decode = text => text.replace(/&quot;/g, '"').replace(/&#0?39;/g, "'").replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&');
  Object.defineProperty(host, 'innerHTML', { get: () => markup, set: value => {
    markup = value;
    buttons = [...value.matchAll(/<button\b([^>]*data-source-path="([^"]+)"[^>]*)>/g)].map(match => {
      const events = {};
      return { dataset: { sourcePath: decode(match[2]) }, disabled: /\sdisabled(?:\s|$)/.test(match[1]),
        addEventListener: (type, fn) => { events[type] = fn; }, click() { return events.click?.(); } };
    });
  }});
  host.querySelectorAll = selector => selector === '[data-source-path]' ? buttons : [];
  return { host, get buttons() { return buttons; } };
}
function prepared(surface = 'requirement') {
  const s = sandbox(false, true, { fakeTimers: true });
  const selector = { requirement: '#featureDetail', component: '#componentInspector', files: '#fileTree' }[surface];
  const view = sourceHost(s, selector);
  s.context.fixture = fixture();
  s.run('state.project=fixture;state.selected="REQ-A";state.selectedComponent="component:engine";');
  const render = surface === 'requirement' ? 'renderDetail()' : surface === 'component' ? 'renderComponentInspector()' : 'renderProjectStructure()';
  s.run(`state.workspaceTab=${JSON.stringify(surface === 'requirement' ? 'requirements' : surface === 'component' ? 'architecture' : 'files')};state.architectureView="components";${render};`);
  return { ...s, view, render };
}
function graph(path, extra = {}) {
  const id = `file:${path}`;
  return { snapshot_id: 'GRAPH-current', captured_at_ms: 1, provider: 'wcode-composite', precision: 'mixed',
    query: id, mode: 'all', depth: 2, root_ids: [id], nodes: [{ node: { id, kind: 'file', label: path,
      attributes: { path, language: 'rust' }, provenance: { provider: 'tree-sitter', precision: 'syntax', revision: 'sha256:' + 'a'.repeat(64) } },
      distance: 0, upstream: false, downstream: false }], edges: [], precision_counts: {}, upstream_nodes: 0, downstream_nodes: 0, truncated: false, ...extra };
}
function source(path, content = 'fn observed() {}') {
  return { workspace: 'A', source: { snapshot_id: 'GRAPH-current', node_id: `file:${path}`, path,
    provider: 'tree-sitter', precision: 'syntax', source_revision: 'sha256:' + 'a'.repeat(64), current_sha256: 'a'.repeat(64),
    start_line: 1, end_line: 1, total_lines: 1, focus_start_line: 1, focus_end_line: 1, content, redacted: false, truncated: false } };
}
async function finishObservation(s, path) {
  const initial = s.requests.length - 1, request = s.requests[initial];
  const url = new URL(request.url, 'http://fixture');
  assert.equal(request.options.method, 'GET');
  assert.equal(url.searchParams.get('node_id'), `file:${path}`);
  assert.equal(url.searchParams.get('expected_code_revision'), revision.code);
  assert.equal(url.searchParams.get('expected_design_revision'), revision.design);
  respond(request, { workspace: 'A', graph: graph(path), repository_revision: revision }); await flush();
  assert.equal(s.requests.length, initial + 2);
  const read = s.requests.at(-1), readUrl = new URL(read.url, 'http://fixture');
  assert.equal(read.options.method, 'GET'); assert.equal(readUrl.searchParams.get('snapshot_id'), 'GRAPH-current');
  assert.equal(readUrl.searchParams.get('node_id'), `file:${path}`);
  respond(read, source(path, '<script>not executable</script>')); await flush();
  assert.equal(s.run('state.codeGraphSource.path'), path);
  assert.match(s.node('#codeGraphInspector').innerHTML, /&lt;script&gt;not executable&lt;\/script&gt;/);
}

test('requirements link implementation and acceptance test files omitted by the bounded tree', async () => {
  const s = prepared();
  assert.deepEqual(s.view.buttons.map(button => button.dataset.sourcePath), ['src/mapped.rs', 'tests/mapped.rs']);
  assert.match(s.view.host.innerHTML, /tree-sitter \/ syntax/);
  assert.match(s.view.host.innerHTML, /References resolved; execution, result and freshness are separate/);
  assert.equal(s.requests.length, 0, 'rendering must not fetch or execute');
  s.view.buttons[0].click(); await flush(); await finishObservation(s, 'src/mapped.rs');
});
test('opening a mapped acceptance test remains a revision-bound file read, never a verification run', async () => {
  const s = prepared(); s.view.buttons[1].click(); await flush(); await finishObservation(s, 'tests/mapped.rs');
  assert.equal(s.requests.length, 2); assert.ok(s.requests.every(request => request.options.method === 'GET'));
  assert.ok(s.requests.every(request => !/verification|acceptance/.test(request.url)));
});
test('component inspector links known mappings and explains unobserved references without guessing', async () => {
  const s = prepared('component'); assert.equal(s.view.buttons.length, 1);
  assert.match(s.view.host.innerHTML, /Source is unavailable in this observation/);
  s.view.buttons[0].click(); await flush(); await finishObservation(s, 'src/mapped.rs');
});
test('harness check IDs, mismatched locators and unobserved unresolved references cannot become source paths', () => {
  const s = prepared();
  for (const ref of [reference('cargo.test', null, { provider: 'harness' }), reference('../private.rs'),
    reference('/private/file.rs'), reference('C:\\private.rs'), reference('src/missing.rs', 'missing', { resolved: false }),
    reference('src/mapped.rs', 'engine', { target: 'src/other.rs::engine' })]) {
    s.context.candidate = ref;
    assert.doesNotMatch(s.run('designSourceAction(candidate)'), /data-source-path/);
  }
  assert.equal(s.run('repositorySourcePaths().has("cargo.test")'), false);
  assert.equal(s.requests.length, 0);
});
test('unresolved symbols can inspect independently observed files while remaining unresolved', () => {
  const s = prepared(); s.context.candidate = reference('src/visible.rs', 'not_found', { resolved: false });
  const html = s.run('implementationRows([candidate])');
  assert.match(html, /unresolved/); assert.match(html, /data-source-path="src\/visible.rs"/);
  assert.match(html, /a mapping is not execution proof/); assert.equal(s.requests.length, 0);
});
test('component locator matching prioritizes exact paths and rejects ambiguous symbol prefixes', () => {
  const s = prepared(); s.run('paths=new Set(["src/a.rs","src/a.rs::nested"]);');
  assert.equal(s.run('designSourceTargetPath("src/a.rs::nested",paths)'), 'src/a.rs::nested');
  assert.equal(s.run('designSourceTargetPath("src/a.rs::nested::target",paths)'), '');
  assert.equal(s.run('designSourceTargetPath("src/a.rs.extra::target",paths)'), '');
});
test('mapped target labels and path attributes are escaped in rendered actions', () => {
  const s = prepared(), path = 'src/<img onerror="bad">.rs';
  s.context.candidate = reference(path, 'symbol"><script>bad()</script>');
  s.run('state.project.requirements[0].components[0].implementation.push(candidate);');
  const html = s.run('implementationRows([candidate])');
  assert.match(html, /&lt;img/); assert.match(html, /&lt;script&gt;/);
  assert.doesNotMatch(html, /<img|<script>/); assert.equal(s.requests.length, 0);
});
test('unknown revision disables mapped actions; an obsolete owner scope rebinds unchanged markup', async () => {
  const s = prepared(), old = s.view.buttons[0];
  // The production page token is immutable. Model an obsolete captured owner,
  // without rewriting the shipped core or inventing a runtime token setter.
  s.run('sourceActionRenders.get("detail").owner="previous-session";' + s.render); const fresh = s.view.buttons[0];
  assert.notEqual(fresh, old); old.click(); await flush(); assert.equal(s.requests.length, 0);
  fresh.click(); await flush(); assert.equal(s.requests.length, 1);
  assert.equal(s.requests[0].options.headers['X-Wcode-UI-Token'], 'test-ui');
  respond(s.requests[0], {}, false); await flush();
  s.run('state.project.proof={};state.workspaceTab="requirements";' + s.render);
  assert.equal(s.view.buttons[0].disabled, true); s.view.buttons[0].click(); await flush(); assert.equal(s.requests.length, 1);
});
test('detached requirement, component and file controls cannot cross workspace, session, revision, or page scope', async () => {
  for (const surface of ['requirement', 'component', 'files']) {
    const key = { requirement: 'detail', component: 'componentInspector', files: 'fileTree' }[surface];
    for (const change of ['state.current="B"', 'state.workspaceEpoch++', `sourceActionRenders.get("${key}").owner="previous-session"`,
      'state.project.proof.revision_code="new-code"', 'state.project.proof.revision_design="new-design"',
      'state.workspaceTab="overview"', 'state.project={...state.project,workspace:"B"}']) {
      const s = prepared(surface), old = s.view.buttons[0]; s.run(change); old.click(); await flush();
      assert.equal(s.requests.length, 0, `${surface}: ${change}`);
    }
  }
});
test('detached controls cannot target a replacement selected requirement or component', async () => {
  for (const surface of ['requirement', 'component']) {
    const s = prepared(surface), old = s.view.buttons[0];
    s.run(surface === 'requirement' ? 'state.selected="REQ-B"' : 'state.selectedComponent="component:other"');
    old.click(); await flush(); assert.equal(s.requests.length, 0);
  }
  const s = prepared('component'), old = s.view.buttons[0]; s.run('state.architectureView="blueprint"');
  old.click(); await flush(); assert.equal(s.requests.length, 0);
});
test('A to B to A workspace epochs reject obsolete controls but current identical controls remain usable', async () => {
  const s = prepared(), old = s.view.buttons[0];
  s.run('state.current="B";state.workspaceEpoch++;state.current="A";state.workspaceEpoch++;' + s.render);
  old.click(); await flush(); assert.equal(s.requests.length, 0);
  s.view.buttons[0].click(); await flush(); await finishObservation(s, 'src/mapped.rs');
});
test('late graph and source responses cannot publish after epoch, revision or navigation changes', async () => {
  for (const stage of ['graph', 'source']) {
    for (const change of ['state.workspaceEpoch++',
      'state.project.proof.revision_code="new-code"', 'state.project.proof.revision_design="new-design"', 'state.workspaceTab="requirements"']) {
      const s = prepared(), pending = s.run('openRepositoryFile("src/mapped.rs")'); await flush();
      if (stage === 'source') {
        respond(s.requests[0], { workspace: 'A', graph: graph('src/mapped.rs'), repository_revision: revision }); await flush();
        assert.equal(s.requests.length, 2);
      }
      s.run(change);
      respond(s.requests.at(-1), stage === 'graph'
        ? { workspace: 'A', graph: graph('src/mapped.rs'), repository_revision: revision } : source('src/mapped.rs', 'obsolete source'));
      assert.equal(await pending, false, `${stage}: ${change}`);
      assert.equal(s.run('state.codeGraphSource'), null, `${stage}: ${change}`);
      if (stage === 'graph') { assert.equal(s.run('state.codeGraph'), null); assert.equal(s.requests.length, 1); }
    }
  }
});
test('a mapped file omitted by the graph remains visibly unavailable and never triggers a guessed source read', async () => {
  const s = prepared(), pending = s.run('openRepositoryFile("src/mapped.rs")'); await flush();
  respond(s.requests[0], { workspace: 'A', graph: graph('src/other.rs', { query: 'file:src/mapped.rs', truncated: true }), repository_revision: revision });
  assert.equal(await pending, false); assert.equal(s.requests.length, 1);
  assert.match(s.run('state.codeGraphError'), /absence here does not prove the file is missing/);
});
test('extensionless filesystem targets retain their exact identity even when the native path projection is empty', async () => {
  for (const path of ['Dockerfile', 'Makefile', 'LICENSE']) {
    const s = prepared(); s.context.candidate = reference(path, null, { path: '', symbol: path });
    s.run('state.project.requirements[0].components[0].implementation=[candidate];' + s.render);
    assert.equal(s.view.buttons[0].dataset.sourcePath, path);
    s.view.buttons[0].click(); await flush(); await finishObservation(s, path);
  }
});
