'use strict';
// The command palette searches the requirements and architecture components the
// Observatory already loaded, and every result names its object kind before the
// object name so a view command cannot be mistaken for an object.
const assert = require('node:assert/strict');
const { test } = require('node:test');
const { sandbox, project } = require('./observatory.cjs');

const fixture = () => ({
  ...project(),
  requirements: [{ id: 'REQ-A', title: 'Workspace isolation', intent: '', components: [],
    convergence: 'stable', dependency_alignment: [], acceptance: [], constraints: [], drift: [] }],
  architecture: { components: [{ id: 'component:one', name: 'Engine One',
    responsibilities: ['first'], implementation_files: 2, depends_on: [] }],
    dependencies: [], desired_edges: 0, observed_edges: 0, blocking_drift_edges: 0 } });

// The behavior fixture models the palette list as markup, so the option lookup
// is fed from the rendered rows the same way the browser would.
function feedPaletteOptions(s) {
  const commands = () => [...s.node('#commandPaletteList').innerHTML
    .matchAll(/data-command="([^"]+)"/g)].map(match => match[1]);
  s.node('#commandPaletteList').querySelectorAll = () =>
    commands().map(command => ({ dataset: { command }, addEventListener() {} }));
  return commands;
}

test('the palette reaches a loaded requirement by title and selects it', () => {
  const s = sandbox();
  s.context.fixture = fixture();
  s.run('state.project=fixture;wireCommandPalette();');
  const search = s.node('#commandPaletteSearch');
  const commands = feedPaletteOptions(s);
  search.value = 'engine'; s.run('renderCommandPalette();');
  assert.ok(commands().includes('object:component:component:one'), 'a component must be reachable by name');
  assert.ok(s.node('#commandPaletteList').innerHTML.includes('Component'), 'the object kind must be shown');
  search.value = 'isolation'; s.run('renderCommandPalette();');
  assert.ok(commands().includes('object:requirement:REQ-A'), 'a requirement must be reachable by title');
  assert.ok(s.node('#commandPaletteList').innerHTML.includes('Requirement'), 'the requirement kind must be shown');
  search.events.keydown({ key: 'Enter', isComposing: false, preventDefault() {} });
  assert.equal(s.run('state.selected'), 'REQ-A', 'the selected requirement must be the one the palette named');
  assert.equal(s.run('state.workspaceTab'), 'requirements');
});

test('the palette reaches a loaded file and inspects its source', () => {
  const s = sandbox();
  s.context.__openedFiles = [];
  s.run('openRepositoryFile=(path)=>__openedFiles.push(path);');
  s.context.fixture = { ...fixture(),
    structure: { entries: [{ path: 'src/a.rs', name: 'a.rs', language: 'rust' }],
      line_limit: 1000, oversized_files: 0, truncated: false } };
  s.run('state.project=fixture;wireCommandPalette();');
  const search = s.node('#commandPaletteSearch');
  const commands = feedPaletteOptions(s);
  search.value = 'a.rs'; s.run('renderCommandPalette();');
  assert.ok(commands().includes('object:file:src/a.rs'), 'a file must be reachable by name');
  assert.ok(s.node('#commandPaletteList').innerHTML.includes('File'), 'the file kind must be shown');
  search.events.keydown({ key: 'Enter', isComposing: false, preventDefault() {} });
  assert.deepEqual(s.context.__openedFiles, ['src/a.rs'], 'the palette must inspect the named file');
});

test('the palette reaches an attention signal and runs its named destination', () => {
  const s = sandbox();
  s.context.__openedChanges = [];
  s.run('openChangeInspection=(path, layer, fresh)=>__openedChanges.push({ path, layer, fresh });');
  s.context.fixture = { ...fixture(),
    attention: { revision: { code: 'code-1', design: 'design-1' }, total: 1, items: [
      { id: 'risk:1', kind: 'risk', severity: 'high', subject: 'src/a.rs',
        message: 'Changed file needs inspection', provider: 'risk-analysis', precision: 'heuristic',
        section: 'changes', path: 'src/a.rs' }] } };
  s.run('state.project=fixture;wireCommandPalette();');
  const search = s.node('#commandPaletteSearch');
  const commands = feedPaletteOptions(s);
  search.value = 'inspection'; s.run('renderCommandPalette();');
  assert.ok(commands().includes('object:signal:risk:1'), 'a signal must be reachable by its message');
  assert.ok(s.node('#commandPaletteList').innerHTML.includes('Signal'), 'the signal kind must be shown');
  search.events.keydown({ key: 'Enter', isComposing: false, preventDefault() {} });
  assert.equal(s.context.__openedChanges.length, 1, 'the signal must run its named destination');
  assert.equal(s.context.__openedChanges[0].path, 'src/a.rs');
});

test('the palette reaches an evidence record and opens its inspector', () => {
  const s = sandbox();
  s.context.fixture = { ...fixture(),
    proof: { revision_code: 'code-1', revision_design: 'design-1',
      effective: { total: 1, failed: 0, passed: 1, inconclusive: 0, disagreed: 0,
        items: [{ id: 'ev-1', subject: 'cargo test', producer: 'cargo-nextest',
          result: 'pass', timestamp_ms: 1, stage: 'deterministic' }] } } };
  s.run('state.project=fixture;wireCommandPalette();');
  const search = s.node('#commandPaletteSearch');
  const commands = feedPaletteOptions(s);
  search.value = 'cargo test'; s.run('renderCommandPalette();');
  assert.ok(commands().includes('object:evidence:0'), 'an evidence record must be reachable by subject');
  assert.ok(s.node('#commandPaletteList').innerHTML.includes('Evidence'), 'the evidence kind must be shown');
  search.events.keydown({ key: 'Enter', isComposing: false, preventDefault() {} });
  assert.equal(s.run('state.selectedEvidenceKey'), 'ev-1');
  assert.equal(s.run('state.evidenceInspectorOpen'), true);
  assert.equal(s.run('state.workspaceTab'), 'proof');
});

test('the palette reaches a worklist task and filters the list to it', () => {
  const s = sandbox();
  s.context.fixture = { ...fixture(),
    execution: { worklist: { available: true, exists: true, revision: 1, items_included: true,
      counts: { pending: 1 },
      items: [{ id: 'TASK-1', title: 'Fix the leak', status: 'pending' }] } } };
  s.run('state.project=fixture;wireCommandPalette();');
  const search = s.node('#commandPaletteSearch');
  const commands = feedPaletteOptions(s);
  search.value = 'leak'; s.run('renderCommandPalette();');
  assert.ok(commands().includes('object:task:0'), 'a task must be reachable by title');
  assert.ok(s.node('#commandPaletteList').innerHTML.includes('Task'), 'the task kind must be shown');
  search.events.keydown({ key: 'Enter', isComposing: false, preventDefault() {} });
  assert.equal(s.run('state.worklistViewScope.query'), 'TASK-1');
  assert.equal(s.run('state.worklistViewScope.filter'), 'all');
  assert.equal(s.run('state.workspaceTab'), 'activity');
});

test('the palette reaches a symbol in the loaded code graph', () => {
  const s = sandbox();
  s.context.fixture = fixture();
  s.run('state.project=fixture;state.codeGraph={nodes:[{node:{id:"sym:1",label:"process_queue",kind:"function",attributes:{path:"src/a.rs",source_kind:"function"}}}],snapshot_id:"g1"};wireCommandPalette();');
  const search = s.node('#commandPaletteSearch');
  const commands = feedPaletteOptions(s);
  search.value = 'process_queue'; s.run('renderCommandPalette();');
  assert.ok(commands().includes('object:symbol:sym:1'), 'a symbol must be reachable by label');
  assert.match(s.node('#commandPaletteList').innerHTML, /Symbol/);
  search.events.keydown({ key: 'Enter', isComposing: false, preventDefault() {} });
  assert.equal(s.run('state.selectedCodeNode'), 'sym:1');
  assert.equal(s.run('state.architectureView'), 'codegraph');
});

test('a symbol result loads the graph focused on that node', () => {
  const s = sandbox();
  s.run('state.current="A";state.workspaceTab="architecture";state.architectureView="codegraph";state.codeGraphView="focus";state.selectedCodeNode="sym:1";state.codeGraph=null;state.codeGraphLoading=false;maybeLoadCodeGraph();');
  assert.equal(s.requests.length, 1, 'the graph load must be requested');
  assert.match(s.requests[0].url, /^\/intelligence\/code-graph\?/);
});

test('symbol search says when no code graph is loaded instead of returning nothing', () => {
  const s = sandbox();
  s.context.fixture = fixture();
  s.run('state.project=fixture;state.codeGraph=null;wireCommandPalette();');
  const search = s.node('#commandPaletteSearch');
  feedPaletteOptions(s);
  search.value = 'process_queue'; s.run('renderCommandPalette();');
  assert.match(s.node('#commandPaletteList').innerHTML, /needs the loaded code graph/);
});

test('the palette reaches a loaded component and opens it in the components view', () => {
  const s = sandbox();
  s.context.fixture = fixture();
  s.run('state.project=fixture;wireCommandPalette();');
  const search = s.node('#commandPaletteSearch');
  feedPaletteOptions(s);
  search.value = 'engine'; s.run('renderCommandPalette();');
  search.events.keydown({ key: 'Enter', isComposing: false, preventDefault() {} });
  assert.equal(s.run('state.selectedComponent'), 'component:one');
  assert.equal(s.run('state.architectureView'), 'components');
  assert.equal(s.run('state.workspaceTab'), 'architecture');
});
