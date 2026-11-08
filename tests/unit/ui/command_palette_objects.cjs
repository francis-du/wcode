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
