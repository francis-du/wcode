'use strict';
// The project navigator only searches the snapshot this page already loaded.
// It must say what it covered instead of reading as a whole-repository search,
// and it must not imply coverage it does not have.
const assert = require('node:assert/strict');
const { test } = require('node:test');
const { sandbox, project } = require('./observatory.cjs');

function fixture(truncated = true) {
  return { ...project(),
    requirements: [{ id: 'REQ-A', title: 'Isolation', intent: '', components: [] }],
    changes: [{ path: 'src/changed.rs', status: 'modified', affected_components: [] }],
    structure: { entries: [{ path: 'src/a.rs', language: 'rust', lines: 10 }], largest_files: [], truncated },
    architecture: {
      subsystems: [{ id: 'sub:core', title: 'Core', purpose: '', component_ids: [] }],
      components: [{ id: 'component:one', name: 'One', responsibilities: [] }],
      dependencies: [], desired_edges: 0, observed_edges: 0, blocking_drift_edges: 0 } };
}

function scopeHost(s) {
  const search = s.node('#projectNavigator'), panel = s.node('#navigatorResults'),
    listbox = s.node('#navigatorListbox'), scope = s.node('#navigatorScope');
  let html = '', buttons = [];
  Object.defineProperty(listbox, 'innerHTML', {
    get: () => html,
    set: value => {
      html = value;
      buttons = [...value.matchAll(/<button id="([^"]+)"[^>]*data-nav-index="(\d+)"[^>]*aria-selected="([^"]+)"/g)]
        .map(match => ({
          id: match[1],
          dataset: { navIndex: match[2] },
          attrs: { 'aria-selected': match[3] },
          addEventListener() {},
          getAttribute(name) { return this.attrs[name]; },
        }));
    },
  });
  listbox.querySelectorAll = () => buttons;
  s.run('state.project=fixture;activateProjectNavigatorItem=item=>{};');
  search.value = 'one';
  s.run('renderProjectNavigator({open:true});');
  return { search, panel, listbox, scope, get buttons() { return buttons; } };
}

test('the navigator states that it searched only the loaded snapshot', () => {
  const s = sandbox();
  s.context.fixture = fixture();
  const host = scopeHost(s);
  assert.equal(host.panel.classList.contains('hidden'), false, 'the results panel must open');
  const text = host.scope.textContent;
  assert.match(text, /loaded snapshot/i, 'the scope note must name what was searched');
  assert.match(text, /1\s+system/, 'subsystems must be counted');
  assert.match(text, /1\s+component/, 'components must be counted');
  assert.match(text, /1\s+requirement/, 'requirements must be counted');
  assert.match(text, /1\s+change/, 'changes must be counted');
  assert.match(text, /1\s+file/, 'files must be counted');
});

test('a bounded snapshot marks the navigator scope as partial', () => {
  const s = sandbox();
  s.context.fixture = fixture(true);
  const host = scopeHost(s);
  assert.match(host.scope.textContent, /partial/i,
    'a truncated structure must not read as full repository coverage');
});

test('the navigator scope is localized without changing its meaning', () => {
  const s = sandbox();
  s.context.fixture = fixture(false);
  const host = scopeHost(s);
  s.run('state.language="zh-CN";renderProjectNavigator({open:true});');
  const text = host.scope.textContent;
  assert.match(text, /仅搜索已加载的快照/);
  assert.doesNotMatch(text, /partial/i, 'an untruncated snapshot must not claim partiality');
});

function objectNavigatorHost(s) {
  const host = s.node('#objectNavigator');
  let html = '', buttons = [];
  Object.defineProperty(host, 'innerHTML', {
    get: () => html,
    set: value => {
      html = value;
      buttons = [...value.matchAll(/data-object-index="(\d+)"/g)].map(match => ({
        dataset: { objectIndex: match[1] },
        handler: null,
        addEventListener(kind, handler) { this.handler = handler; },
      }));
    },
  });
  host.querySelectorAll = () => buttons;
  return host;
}

test('the left object navigator lists loaded objects and navigates on click', () => {
  const s = sandbox();
  s.context.fixture = fixture(true);
  const host = objectNavigatorHost(s);
  s.run('state.project=fixture;renderObjectNavigator();');
  const html = host.innerHTML;
  for (const label of ['REQ-A', 'component:one', 'src/changed.rs', 'src/a.rs']) {
    assert.ok(html.includes(label), `${label} must be reachable from the navigator`);
  }
  assert.doesNotMatch(html, /sub:core/, 'a subsystem grouping is not an object');
  s.context.opened = [];
  s.run('executeCommand=(command)=>opened.push(command);');
  for (const button of host.querySelectorAll()) button.handler();
  assert.deepEqual(s.context.opened, [
    'object:component:component:one',
    'object:requirement:REQ-A',
    'object:change:src/changed.rs',
    'object:file:src/a.rs',
  ]);
});

test('the right inspector summarizes the selected object and its stable id', () => {
  const s = sandbox();
  s.context.fixture = fixture(true);
  s.run('state.project=fixture;state.selected="REQ-A";renderObjectInspector();');
  const requirement = s.node('#objectInspector').innerHTML;
  assert.match(requirement, /REQ-A/);
  assert.match(requirement, /Priority/i);
  assert.match(requirement, /Convergence/i);
  s.run('state.selected="";state.selectedComponent="component:one";renderObjectInspector();');
  const component = s.node('#objectInspector').innerHTML;
  assert.match(component, /component:one/);
  assert.match(component, /Responsibilities/i);
  s.run('state.selectedComponent="";renderObjectInspector();');
  assert.match(s.node('#objectInspector').innerHTML, /Select an object/);
});

test('the right inspector names the selected code symbol and its provenance', () => {
  const s = sandbox();
  s.context.fixture = fixture(true);
  s.run('state.project=fixture;'
    + 'state.codeGraph={root_ids:["symbol:alpha"],'
    + 'nodes:[{node:{id:"symbol:alpha",label:"alpha",kind:"function",'
    + 'attributes:{path:"src/a.rs",source_kind:"function"},'
    + 'provenance:{provider:"tree-sitter",precision:"syntax",revision:"code:1"}},distance:1}],'
    + 'edges:[{from:"symbol:alpha",to:"symbol:beta",kind:"calls"}]};'
    + 'state.selectedCodeNode="symbol:alpha";renderObjectInspector();');
  const html = s.node('#objectInspector').innerHTML;
  assert.match(html, /Symbol/i);
  assert.match(html, /alpha/);
  assert.match(html, /src\/a\.rs/);
  assert.match(html, /tree-sitter/);
  assert.match(html, /syntax/);
  assert.doesNotMatch(html, /Select an object/);
});
