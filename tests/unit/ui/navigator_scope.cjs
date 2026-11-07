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
