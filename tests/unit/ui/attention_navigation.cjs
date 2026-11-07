'use strict';
// Attention signals carry explicit identities (path, requirement). A signal
// whose destination cannot show that identity must resolve it through explicit
// data instead of dropping the investigation on an unfocused section.
const assert = require('node:assert/strict');
const { test } = require('node:test');
const { sandbox, project } = require('./observatory.cjs');

const requirement = { id: 'REQ-A', title: 'Workspace isolation', intent: '', components: ['component:one'],
  convergence: 'stable', dependency_alignment: [], acceptance: [], constraints: [], drift: [] };

function parseButtons(markup) {
  return [...markup.matchAll(/<button\b([^>]*)>/g)].map(match => {
    const dataset = {};
    for (const [, key, raw] of match[1].matchAll(/data-(summary-[a-z-]+)="([^"]*)"/g)) {
      dataset[key.replace(/-([a-z])/g, (_, character) => character.toUpperCase())] = raw;
    }
    return { dataset, events: {}, addEventListener(type, handler) { this.events[type] = handler; } };
  });
}

function attentionHost(s) {
  const host = s.node('#attention');
  let markup = host.innerHTML;
  let buttons = parseButtons(markup);
  Object.defineProperty(host, 'innerHTML', {
    get: () => markup,
    set: value => {
      markup = value;
      buttons = parseButtons(value);
    },
  });
  host.querySelectorAll = () => buttons;
  return { host, get buttons() { return buttons; } };
}

function fixture() {
  return { ...project(), git_review: { available: true, reason: 'available' },
    proof: { revision_code: 'code-1', revision_design: 'design-1', current_evidence: 1 },
    requirements: [requirement],
    architecture: { components: [{ id: 'component:one', name: 'One', responsibilities: [] }],
      dependencies: [], desired_edges: 0, observed_edges: 0, blocking_drift_edges: 0 },
    attention: { revision: { code: 'code-1', design: 'design-1' }, total: 2, items: [
      // The highest-severity signal is rendered into the status summary; the
      // drift signal has to be in the signal list for this contract.
      { id: 'failure', kind: 'evidence_failed', severity: 'critical', subject: 'Failing check',
        message: 'Current producer failed', provider: 'verification', precision: 'deterministic',
        section: 'proof' },
      { id: 'drift:1', kind: 'drift', severity: 'high', subject: 'REQ-A',
        message: 'implementation moved', provider: 'drift-analysis', precision: 'mixed',
        section: 'architecture', path: 'src/a.rs', requirement: 'REQ-A' } ] } };
}

test('a drift signal focuses the component that owns the affected requirement', () => {
  const s = sandbox();
  const view = attentionHost(s);
  s.context.fixture = fixture();
  s.run('state.project=fixture;renderAttention();');
  const signal = view.buttons.find(button => button.dataset.summaryAction === 'architectureSection'
    && button.dataset.summaryRequirement === 'REQ-A');
  assert.ok(signal, 'the drift signal must be rendered');
  assert.equal(typeof signal.events.click, 'function', 'the signal must be bound');
  signal.events.click();
  assert.equal(s.run('state.selectedComponent'), 'component:one',
    'the owning component must be selected, not just the section revealed');
  assert.equal(s.run('state.architectureView'), 'components');
});

test('a signal whose requirement cannot be resolved keeps the plain section reveal', () => {
  const s = sandbox();
  const view = attentionHost(s);
  s.context.fixture = { ...fixture(), requirements: [] };
  s.run('state.project=fixture;renderAttention();');
  const signal = view.buttons.find(button => button.dataset.summaryAction === 'architectureSection'
    && button.dataset.summaryRequirement === 'REQ-A');
  assert.ok(signal, 'the drift signal must be rendered');
  assert.equal(typeof signal.events.click, 'function', 'the signal must be bound');
  signal.events.click();
  assert.equal(s.run('state.selectedComponent'), '',
    'an unresolvable requirement must not select an invented component');
});
