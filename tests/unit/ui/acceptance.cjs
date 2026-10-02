'use strict';
// These fixtures exercise the rendered public protocol, not the core acceptance evaluator.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const root = process.argv[2];
const { sandbox, project, respond, flush } = require('./observatory.cjs');
const hash = value => 'sha256:' + value.repeat(64);
const revision = { code: hash('a'), design: hash('b') };
function record() {
  return { schema_version: 1, id: 'CAR-ui-protocol', record_digest: hash('c'), workspace: 'A',
    revision: { ...revision }, git: { binding: { head_sha: 'd'.repeat(40) }, complete: true, authority: 'metadata_only' },
    policy: null, verification: null, plan: { id: 'VP-ui-protocol', verification_generation: 1 }, risk_level: 'high',
    state: 'blocked', partial: false, reasons: [{ code: 'required_check_failed', subject: 'native-check', action: 'inspect_failure' }],
    actions: ['inspect_failure'], checks: [{ id: 'native-check', signature: hash('e'), required: true,
      discovered: true, mapped: true, execution: 'executed', outcome: 'fail', freshness: 'current', level: 'full', required_level: 'full', evidence_ids: ['EV-native'] }],
    summary: { required: 1, discovered: 1, mapped: 1, executed: 1, passed: 0, failed: 1,
      skipped: 0, unavailable: 0, stale: 0, unknown: 0 }, evidence_ids: ['EV-native'] };
}
function fixture() {
  return { ...project(), git_review: { available: true }, code: { changed_files: 2 },
    proof: { revision_code: revision.code, revision_design: revision.design, current_evidence: 1, current_passed: 0,
      effective: { total: 1, failed: 1, passed: 0, inconclusive: 0, disagreed: 0,
        items: [{ id: 'EV-native', artifact_digest: 'artifact-native', subject: 'native-check', producer: 'native-runner', result: 'fail' }] } },
    acceptance: record() };
}
function inspected(p) {
  const binding = { repository: hash('1'), head_sha: 'd'.repeat(40), tree_sha: 'e'.repeat(40), dirty: true, index_fingerprint: hash('2') };
  p.acceptance.git = { base_sha: 'd'.repeat(40), target_sha: null, binding, complete: true, authority: 'metadata_only' };
  p.acceptance.inspection = { workspace: 'A', revision: { ...revision }, base_sha: 'd'.repeat(40), target_sha: null,
    git_binding: { ...binding }, producer: 'wcode/native-change-inspection/v1',
    symbols: [{ id: 'symbol:alpha', path: 'src/a.rs', name: 'alpha', provider: 'tree-sitter', precision: 'syntax',
      selection: 'file_membership', freshness: 'current', start_line: 1, end_line: 1 }],
    impacted_components: ['CMP-alpha'], impacted_requirements: ['REQ-alpha'], impacted_acceptance: ['AC-alpha'],
    mapped_verification: [{ owner: 'REQ-alpha', target: 'test_alpha', kind: 'test', resolved: true,
      provider: 'design-state', precision: 'declared', relation: 'declared_verification' }],
    impact: { graph_provider: 'wcode-composite', graph_precision: 'syntax', graph_truncated: false,
      transitive_callers: 3, public_api: false, security_boundary: true },
    risk: { level: 'high', precision: 'heuristic', findings_total: 1, findings: [{ id: 'RISK-alpha', category: 'security', level: 'high' }],
      bug_pattern_matches: 0, drift_findings: 0, truncated: false },
    coverage: { changed_paths_total: 2, graph_files_indexed: 2, symbols_observed: 1, symbols_returned: 1,
      mapped_paths_total: 1, unmapped_paths_total: 1, unmapped_paths: ['src/unmapped.rs'], uncovered_paths_total: 1,
      uncovered_paths: ['src/unmapped.rs'], verification_observed: 1, verification_returned: 1, components_observed: 1,
      requirements_observed: 1, acceptance_observed: 1, mappings_complete: false, graph_complete: true, totals_complete: true },
    unknown_reasons: ['symbol_body_delta_not_captured'], recommended_actions: ['inspect_failure'], truncated: false, complete: false };
}
function graphResponse(id = 'symbol:alpha', workspace = 'A') {
  return { workspace, repository_revision: { ...revision }, graph: {
    snapshot_id: 'GRAPH-native-protocol', captured_at_ms: 1, provider: 'wcode-composite', precision: 'syntax',
    query: id, mode: 'all', depth: 2, root_ids: [id], nodes: [{ node: { id, kind: 'function', label: 'alpha',
      attributes: { path: 'src/a.rs', language: 'rust' }, provenance: { precision: 'syntax', provider: 'tree-sitter', revision: revision.code } },
      distance: 0, upstream: false, downstream: false }], edges: [], precision_counts: { syntax: 1 }, upstream_nodes: 0, downstream_nodes: 0, truncated: false } };
}
function metadata(p) {
  p.acceptance.evidence = [{ id: 'EV-native', producer: 'native-executor', kind: 'verification', authority: 'native_verification',
    confidence: 'deterministic', revision: { ...revision }, timestamp_ms: 1, targets: ['native-check'],
    freshness: 'current', verification_relation: 'current_plan' }];
}
function prepared(change) {
  const s = sandbox(); s.context.fixture = fixture();
  if (change) change(s.context.fixture);
  s.run('state.project=fixture;renderAttention();');
  return s;
}
function summary(s) { return s.node('#statusSummary').innerHTML; }
async function main() {
  const results = [];
  async function test(name, fn) {
    try { await fn(); results.push({ name, passed: true }); }
    catch (error) { results.push({ name, passed: false, error: error.stack }); }
  }
  await test('acceptance is the initial page and supporting observations are progressive', () => {
    const s = sandbox(); assert.equal(s.run('state.workspaceTab'), 'overview');
    const html = fs.readFileSync(path.join(root, 'src/ui/intelligence_web/page.html'), 'utf8');
    assert.match(html, /data-workspace-tab="overview"[^>]*aria-selected="true"/);
    assert.match(html, /id="architectureSection" class="architecture-stage workspace-panel hidden"/);
    assert.ok(html.indexOf('id="statusSummary"') < html.indexOf('class="acceptance-supporting"'));
    assert.match(html, /<details class="acceptance-supporting">/);
  });
  await test('canonical failure controls acceptance independently from favorable evidence totals', () => {
    const s = prepared(p => { p.proof.current_passed = 999; p.proof.current_failed = 0; });
    assert.match(summary(s), /Acceptance blocked/);
    assert.match(summary(s), /native-check/); assert.match(summary(s), /required_check_failed/);
    assert.match(summary(s), /data-summary-action="proofSection"/);
    assert.doesNotMatch(summary(s), /Acceptance ready|Release confidence/);
  });
  await test('missing acceptance stays unknown even with current passing evidence', () => {
    const s = prepared(p => { delete p.acceptance; p.proof.current_passed = 999; });
    assert.match(summary(s), /Acceptance unknown/);
    assert.match(summary(s), /Evidence counts do not establish acceptance/);
  });
  await test('only a bound current canonical ready is displayed ready without running anything', () => {
    const s = prepared(p => { p.acceptance.state = 'ready'; p.acceptance.reasons = []; p.acceptance.actions = [];
      p.acceptance.checks[0].outcome = 'pass'; p.acceptance.summary.passed = 1; p.acceptance.summary.failed = 0; });
    assert.match(summary(s), /Acceptance ready/);
    assert.equal(s.requests.length, 0, 'painting a decision never executes or approves');
  });
  await test('all canonical nonready states remain explicit', () => {
    for (const [value, text] of [['blocked', 'Acceptance blocked'], ['needs_review', 'Review required'],
      ['incomplete', 'Acceptance incomplete'], ['stale', 'Acceptance stale']]) {
      const s = prepared(p => { p.acceptance.state = value; }); assert.ok(summary(s).includes(text));
    }
  });
  await test('invalid and unsupported records never certify readiness', () => {
    for (const mutate of [r => { r.schema_version = 2; }, r => { r.checks[0].outcome = 'success'; },
      r => { r.checks[0].signature = 'invented'; }, r => { r.summary.failed = -1; },
      r => { delete r.checks[0].mapped; }, r => { r.record_digest = 'not-a-digest'; }]) {
      const s = prepared(p => { p.acceptance.state = 'ready'; mutate(p.acceptance); });
      assert.match(summary(s), /Acceptance unknown/); assert.doesNotMatch(summary(s), /Acceptance ready/);
    }
  });
  await test('foreign workspace record is neither accepted nor disclosed', () => {
    const s = prepared(p => { p.acceptance.workspace = 'B'; p.acceptance.id = 'CAR-foreign-secret'; });
    assert.match(summary(s), /another workspace/); assert.doesNotMatch(summary(s), /CAR-foreign-secret/);
  });
  await test('code and design mismatches keep ready records stale', () => {
    for (const dimension of ['code', 'design']) {
      const s = prepared(p => { p.acceptance.state = 'ready'; p.acceptance.revision[dimension] = hash('f'); });
      assert.match(summary(s), /Acceptance stale/); assert.doesNotMatch(summary(s), /Acceptance ready/);
      assert.match(summary(s), /data-summary-action="refresh"/);
    }
  });
  await test('cached restoration failed refresh and background rebuild cannot present current ready', () => {
    for (const flag of ['state.syncError=true', 'state.fitnessSnapshotFromCache=true', 'state.project.snapshot_refreshing=true']) {
      const s = prepared(p => { p.acceptance.state = 'ready'; });
      s.run(flag + ';renderAttention();'); assert.match(summary(s), /Acceptance stale/);
    }
  });
  await test('partial inputs cannot turn a server ready label into complete acceptance', () => {
    const s = prepared(p => { p.acceptance.state = 'ready'; p.acceptance.partial = true; });
    assert.match(summary(s), /Acceptance incomplete/); assert.doesNotMatch(summary(s), /Acceptance ready/);
  });
  await test('check selection execution outcome and freshness remain distinct typed axes', () => {
    const s = prepared(p => { Object.assign(p.acceptance.checks[0], { signature: null, discovered: false,
      mapped: false, execution: 'unavailable', outcome: 'unknown', freshness: 'missing', evidence_ids: [] }); });
    assert.match(summary(s), /Signature unavailable/); assert.match(summary(s), /Not discovered/);
    assert.match(summary(s), /Not mapped/); assert.match(summary(s), />unavailable</);
    assert.match(summary(s), />unknown</); assert.match(summary(s), />missing</);
    assert.match(summary(s), /No linked receipt/);
  });
  await test('review stages and approval expose typed results without fabricating optional gates', () => {
    const s = prepared(p => { p.acceptance.verification = { deterministic_result: 'pass', human_approval: null,
      stage_results: { property: 'fail', mutation: 'inconclusive' }, queued: 2, claimed: 1, submitted: 3,
      reviewer_failures: 1, reviewer_inconclusive: 1, disagreements: 0 }; });
    assert.match(summary(s), /Review, stages and human approval/);
    assert.match(summary(s), /property/); assert.match(summary(s), />fail</);
    assert.match(summary(s), /Human approval result/); assert.match(summary(s), /2 reviews queued/);
    assert.match(summary(s), /Acceptance blocked/);
    s.context.fixture.acceptance.verification.human_approval = 'approved';
    s.run('renderAttention();'); assert.match(summary(s), /Acceptance unknown/);
  });
  await test('nullable reason subjects and quick versus full levels match the native public record', () => {
    const s = prepared(p => { p.acceptance.state = 'incomplete'; p.acceptance.reasons[0].subject = null;
      p.acceptance.checks[0].level = 'quick'; p.acceptance.checks[0].required_level = 'full'; });
    assert.match(summary(s), /Acceptance incomplete/); assert.doesNotMatch(summary(s), /Acceptance unknown/);
    assert.match(summary(s), /required check failed/);
    assert.match(summary(s), /Required: full · Reported: quick/);
  });
  await test('required check and reason presentation is bounded and discloses omissions', () => {
    const s = prepared(p => {
      p.acceptance.checks = Array.from({ length: 70 }, (_, i) => ({ ...p.acceptance.checks[0], id: 'check-' + i }));
      p.acceptance.summary.required = 70;
      p.acceptance.reasons = Array.from({ length: 13 }, (_, i) => ({ code: 'failed', subject: 'reason-' + i, action: 'inspect_failure' }));
    });
    assert.equal((summary(s).match(/<th scope="row">/g) || []).length, 64);
    assert.match(summary(s), /64 of 70 required checks shown/);
    assert.match(summary(s), /Additional recorded reasons/); assert.doesNotMatch(summary(s), /reason-12</);
  });
  await test('record subjects check IDs and evidence references escape executable markup', () => {
    const s = prepared(p => { p.acceptance.reasons[0].subject = '<script>bad</script>';
      p.acceptance.checks[0].id = '<img src=x onerror=bad>'; p.acceptance.checks[0].evidence_ids = ['EV-"><script>bad</script>']; });
    assert.match(summary(s), /&lt;script&gt;/); assert.doesNotMatch(summary(s), /<script>|<img/);
  });
  await test('linked evidence opens the exact bounded ledger record', () => {
    const s = prepared(); s.run('openAcceptanceEvidence("EV-native");');
    assert.equal(s.run('state.workspaceTab'), 'proof');
    assert.equal(s.run('state.selectedEvidenceKey'), 'artifact-native');
    assert.equal(s.run('state.selectedEvidenceReference'), '');
    assert.match(s.node('#proofSummary').innerHTML, /native-check/);
  });
  await test('missing and beyond-window evidence never substitutes an unrelated selected record', () => {
    for (const beyond of [false, true]) {
      const s = prepared(p => {
        if (beyond) p.proof.effective.items = Array.from({ length: 33 }, (_, i) => ({
          id: 'EV-' + i, artifact_digest: 'artifact-' + i, subject: 'check-' + i, result: 'pass' }));
      });
      s.run(beyond ? 'openAcceptanceEvidence("EV-32");' : 'openAcceptanceEvidence("EV-missing");');
      const html = s.node('#proofSummary').innerHTML;
      assert.match(html, /no substitute record is selected/);
      assert.match(html, /Select evidence to inspect/);
    }
  });
  await test('workspace clear removes evidence references', () => {
    const s = prepared(); s.run('openAcceptanceEvidence("EV-missing");clearWorkspaceView();');
    assert.equal(s.run('state.selectedEvidenceReference'), '');
  });
  await test('convergence uses the canonical decision instead of passing evidence counts', () => {
    const s = prepared(p => { delete p.acceptance; p.proof.current_evidence = 999; p.proof.current_passed = 999;
      Object.assign(p.proof.effective, { total: 999, passed: 999, failed: 0 }); });
    s.run('renderChangeConvergenceMap();');
    const html = s.node('#changeConvergenceMap').innerHTML;
    assert.match(html, /Acceptance unknown/); assert.doesNotMatch(html, /Release confidence<|>High</);
  });
  await test('all fixed next actions route to existing drilldowns and do not mutate', () => {
    const s = prepared();
    for (const action of ['capture_context', 'activate_policy', 'refresh_policy', 'capture_git', 'plan_verification',
      'run_verification', 'inspect_failure', 'request_review', 'request_human_approval', 'refresh_revision', 'resolve_discovery']) {
      s.context.action = action; assert.ok(s.run('acceptanceAction(action).target'));
    }
    s.context.action = 'https://untrusted.example';
    assert.equal(s.run('acceptanceAction(action).target'), '');
    assert.equal(s.requests.length, 0);
  });
  await test('failed transport retains the last decision only as stale', async () => {
    const s = prepared(p => { p.acceptance.state = 'ready'; });
    const run = s.run('refreshProject({reason:"manual",revision:{fingerprint:"new"}})');
    await flush(); respond(s.requests[0], { error: 'offline' }, false); await run;
    assert.match(summary(s), /Acceptance stale/); assert.doesNotMatch(summary(s), /Acceptance ready/);
  });
  await test('workspace A to B to A ignores an old ready response generation', async () => {
    const s = sandbox(); s.run('renderProject=()=>renderAttention();');
    const old = s.run('refreshProject({workspace:"A",reason:"manual",revision:{fingerprint:"old"}})'); await flush();
    const b = s.run('refreshProject({workspace:"B",reason:"manual",revision:{fingerprint:"b"}})'); await flush();
    const current = s.run('refreshProject({workspace:"A",reason:"manual",revision:{fingerprint:"new"}})'); await flush();
    const rows = s.requests.filter(item => item.url === '/intelligence/project');
    const latest = fixture(); respond(rows[2], latest); await current;
    const stale = fixture(); stale.acceptance.state = 'ready'; respond(rows[0], stale);
    const foreign = fixture(); foreign.workspace = 'B'; foreign.acceptance.workspace = 'B'; respond(rows[1], foreign);
    await Promise.all([old, b]);
    assert.equal(s.run('state.current'), 'A'); assert.match(summary(s), /Acceptance blocked/);
  });
  await test('native inspection shows bounded scope without inventing changed symbol bodies or executed mappings', () => {
    const s = prepared(inspected), html = summary(s);
    assert.match(html, /Inspect change scope/); assert.match(html, /data-acceptance-symbol="symbol:alpha"/);
    assert.match(html, /Symbols are members of changed files/); assert.match(html, /Resolved references are not executed tests or passing evidence/);
    assert.match(html, /Inspection coverage is partial/); assert.match(html, /src\/unmapped.rs/);
    assert.match(html, /RISK-alpha/); assert.match(html, /symbol_body_delta_not_captured/);
    assert.equal(s.run('acceptanceView().status'), 'blocked'); assert.equal(s.requests.length, 0);
  });
  await test('native inspection accepts ordinary uppercase digit and Unicode paths while rejecting unsafe components', () => {
    const validPaths = ['README.md', 'src/index.js', 'src/Test01.rs', 'src/café file.rs', 'src/目录.rs'];
    for (let code = 32; code < 127; code++) {
      if (![58, 92].includes(code)) validPaths.push('src/file' + String.fromCharCode(code) + '.rs');
    }
    for (const path of validPaths) {
      const s = prepared(p => { inspected(p); const data = p.acceptance.inspection;
        data.symbols[0].path = path; data.coverage.unmapped_paths = [path]; data.coverage.uncovered_paths = [path]; });
      assert.equal(s.run('acceptanceInspectionView().status'), 'current', path);
      assert.equal(s.run('acceptanceView().status'), 'blocked'); assert.equal(s.requests.length, 0);
    }
    for (const path of ['../private.rs', '/root.rs', 'src//a.rs', 'src/./a.rs', 'src/../a.rs',
      'C:/a.rs', 'src\\\\a.rs', 'src/' + String.fromCharCode(0) + '.rs', 'src/' + String.fromCharCode(31) + '.rs']) {
      const s = prepared(p => { inspected(p); p.acceptance.inspection.symbols[0].path = path; });
      assert.equal(s.run('acceptanceInspectionView().status'), 'unknown', path);
      assert.equal(s.requests.length, 0);
    }
  });
  await test('inspection identity errors hide foreign scope and never weaken canonical acceptance', async () => {
    for (const mutate of [row => { row.workspace = 'B'; }, row => { row.revision.code = hash('f'); },
      row => { row.git_binding.head_sha = 'f'.repeat(40); }, row => { row.base_sha = 'f'.repeat(40); },
      row => { row.producer = 'self-reported'; }, row => { row.symbols[0].path = '../private.rs'; },
      row => { row.symbols[0].selection = 'changed_body'; }, row => { row.coverage.symbols_returned = 999; }]) {
      const s = prepared(p => { inspected(p); mutate(p.acceptance.inspection); });
      assert.match(summary(s), /identity is unsupported/); assert.doesNotMatch(summary(s), /data-acceptance-symbol/);
      assert.equal(await s.run('openAcceptanceSymbol("symbol:alpha")'), false);
      assert.equal(s.requests.length, 0); assert.equal(s.run('acceptanceView().status'), 'blocked');
    }
  });
  await test('stale snapshot or stale symbol keeps historical inspection visible without initiating source reads', async () => {
    for (const change of ['state.syncError=true', 'state.project.acceptance.inspection.symbols[0].freshness="stale"']) {
      const s = prepared(inspected); s.run(change + ';renderAttention();');
      assert.match(summary(s), /alpha/); assert.doesNotMatch(summary(s), /data-acceptance-symbol/);
      assert.equal(await s.run('openAcceptanceSymbol("symbol:alpha")'), false); assert.equal(s.requests.length, 0);
    }
  });
  await test('inspection source navigation uses real graph and protected source response contracts', async () => {
    const s = prepared(inspected); const pending = s.run('openAcceptanceSymbol("symbol:alpha")'); await flush();
    assert.equal(s.requests.length, 1); const url = new URL(s.requests[0].url, 'http://fixture');
    assert.equal(url.pathname, '/intelligence/code-graph'); assert.equal(url.searchParams.get('node_id'), 'symbol:alpha');
    assert.equal(url.searchParams.get('expected_code_revision'), revision.code);
    respond(s.requests[0], graphResponse()); await flush();
    assert.equal(s.requests.length, 2); const sourceUrl = new URL(s.requests[1].url, 'http://fixture');
    assert.equal(sourceUrl.pathname, '/intelligence/code-source'); assert.equal(sourceUrl.searchParams.get('snapshot_id'), 'GRAPH-native-protocol');
    assert.equal(sourceUrl.searchParams.get('node_id'), 'symbol:alpha');
    respond(s.requests[1], { workspace: 'A', source: { snapshot_id: 'GRAPH-native-protocol', node_id: 'symbol:alpha', path: 'src/a.rs',
      provider: 'tree-sitter', precision: 'syntax', source_revision: revision.code, current_sha256: 'a'.repeat(64),
      start_line: 1, end_line: 1, total_lines: 1, focus_start_line: 1, focus_end_line: 1,
      content: 'fn alpha() {}', redacted: false, truncated: false, editable: false, line_ending: 'lf' } });
    assert.equal(await pending, true); assert.equal(s.run('state.codeGraphSource.current_sha256'), 'a'.repeat(64));
    assert.equal(s.run('state.codeGraphSource.content'), 'fn alpha() {}');
    assert.equal(s.run('state.codeGraphSource.editable'), false);
    assert.ok(s.requests.every(row => row.options.method === 'GET'));
  });
  await test('concurrent workspace, project revision or record refresh cannot continue stale inspection source reads', async () => {
    for (const mutation of ['state.workspaceEpoch++;state.current="B";state.current="A";',
      'state.project.proof.revision_code="new";', 'state.project.acceptance.record_digest="' + hash('f') + '";',
      'state.syncError=true;']) {
      const s = prepared(inspected); const pending = s.run('openAcceptanceSymbol("symbol:alpha")'); await flush();
      s.run(mutation); respond(s.requests[0], graphResponse()); await pending;
      assert.equal(s.requests.length, 1, 'old inspection graph must not initiate source');
      assert.equal(s.run('state.codeGraphSource'), null);
    }
  });
  await test('failed protected graph request keeps failure explicit and never falls through to arbitrary source', async () => {
    const s = prepared(inspected); const pending = s.run('openAcceptanceSymbol("symbol:alpha")'); await flush();
    respond(s.requests[0], { error: 'PRIVATE raw command output' }, false);
    assert.equal(await pending, false); assert.equal(s.requests.length, 1);
    assert.match(s.run('state.codeGraphError'), /unavailable|failed|retry|refresh/i);
    assert.doesNotMatch(s.run('state.codeGraphError'), /PRIVATE/);
  });
  await test('inspection metadata and canonical evidence escape repository text without upgrading mapping into proof', () => {
    const s = prepared(p => { inspected(p); p.acceptance.inspection.symbols[0].name = '<img onerror=bad>';
      p.acceptance.inspection.mapped_verification[0].target = '<script>bad</script>'; });
    assert.match(summary(s), /&lt;img/); assert.match(summary(s), /&lt;script/); assert.doesNotMatch(summary(s), /<img|<script>/);
    assert.equal(s.run('acceptanceView().status'), 'blocked');
  });
  await test('exact CAR evidence metadata remains inspectable when the bounded effective ledger omits its receipt', () => {
    const s = prepared(p => { metadata(p); p.proof.effective.items = []; }); s.run('openAcceptanceEvidence("EV-native");');
    const html = s.node('#proofSummary').innerHTML;
    assert.match(html, /Canonical evidence metadata/); assert.match(html, /native_verification/);
    assert.match(html, /native-executor/); assert.match(html, /no receipt outcome/);
    assert.match(html, new RegExp(revision.code)); assert.doesNotMatch(html, /Check results<|>Pass</);
    assert.equal(s.requests.length, 0);
  });
  await test('ambiguous or foreign CAR metadata never substitutes an exact evidence reference', () => {
    for (const mutate of [p => { p.acceptance.evidence.push({ ...p.acceptance.evidence[0] }); },
      p => { p.acceptance.workspace = 'B'; }, p => { p.acceptance.evidence[0].id = 'EV-other'; }]) {
      const s = prepared(p => { metadata(p); p.proof.effective.items = []; mutate(p); }); s.run('openAcceptanceEvidence("EV-native");');
      assert.doesNotMatch(s.node('#proofSummary').innerHTML, /Canonical evidence metadata|native-executor/);
      assert.match(s.node('#proofSummary').innerHTML, /no substitute record is selected/);
    }
  });
  await test('required check page controls expose every returned check and reset when the canonical record changes', () => {
    const s = sandbox(), buttons = [-64, 64].map(value => ({ dataset: { acceptanceCheckPage: String(value) },
      events: {}, addEventListener(name, fn) { this.events[name] = fn; }, focus() {} }));
    s.node('#statusSummary').querySelectorAll = selector => selector === '[data-acceptance-check-page]' ? buttons : [];
    s.context.fixture = fixture(); s.context.fixture.acceptance.checks = Array.from({ length: 70 }, (_, i) => ({
      ...record().checks[0], id: 'check-' + i }));
    s.context.fixture.acceptance.summary.required = 70;
    s.run('state.project=fixture;renderAttention();');
    assert.match(summary(s), /1–64 \/ 70/); assert.doesNotMatch(summary(s), /<code>check-69<\/code>/);
    buttons[1].events.click();
    assert.match(summary(s), /65–70 \/ 70/); assert.match(summary(s), /<code>check-69<\/code>/);
    assert.doesNotMatch(summary(s), /<code>check-0<\/code>/);
    assert.equal((summary(s).match(/<th scope="row">/g) || []).length, 6);
    buttons[0].events.click(); assert.match(summary(s), /1–64 \/ 70/);
    buttons[1].events.click(); s.context.nextDigest = hash('f'); s.run('state.project.acceptance.record_digest=nextDigest;renderAttention();');
    assert.match(summary(s), /1–64 \/ 70/); assert.equal(s.run('acceptanceView().status'), 'blocked');
    assert.equal(s.requests.length, 0);
  });
  console.log(JSON.stringify({ suite: 'acceptance-ui-protocol', results }, null, 2));
  assert.ok(results.every(item => item.passed), results.filter(item => !item.passed).map(item => item.name).join('\n'));
}

function nativeContract(inputPath) {
  const bytes = fs.readFileSync(inputPath);
  assert.ok(bytes.length <= 2 * 1024 * 1024, 'native protocol fixture must be bounded');
  const snapshots = JSON.parse(bytes.toString('utf8'));
  assert.equal(snapshots.length, 2);
  const seen = new Set();
  for (const snapshot of snapshots) {
    const record = snapshot.acceptance;
    assert.equal(record.producer, 'wcode/native-acceptance/v1');
    assert.ok(!Object.hasOwn(record.revision, 'complete'), 'Rust Revision has Code/Design only');
    const s = sandbox(); s.context.nativeSnapshot = snapshot;
    s.run('state.current=nativeSnapshot.workspace;state.project=nativeSnapshot;renderAttention();');
    assert.equal(s.run('acceptanceView().status'), record.state, 'real native state must survive UI projection');
    assert.match(s.node('#statusSummary').innerHTML, new RegExp(s.run('acceptanceStateLabel(acceptanceView().status)')));
    for (const row of record.checks.filter(row => row.required)) {
      assert.ok(s.node('#statusSummary').innerHTML.includes(row.id), 'native required check must be visible');
    }
    assert.equal(s.run('state.project.acceptance.record_digest'), record.record_digest);
    assert.ok(record.inspection, 'native Harness must supply actual bound inspection');
    assert.equal(s.run('acceptanceInspectionView().status'), 'current', 'actual native inspection must survive its strict UI boundary');
    assert.match(s.node('#statusSummary').innerHTML, /Inspect change scope/);
    for (const row of record.evidence || []) {
      s.context.nativeEvidenceId = row.id;
      assert.equal(s.run('acceptanceEvidenceMetadata(nativeEvidenceId)?.id'), row.id, 'native evidence metadata must preserve exact ID');
    }
    assert.equal(s.requests.length, 0, 'rendering must not verify or approve');
    seen.add(record.state);
  }
  assert.deepEqual([...seen].sort(), ['incomplete', 'ready']);
  console.log(JSON.stringify({suite: 'acceptance-native-serializer', snapshots: 2, passed: true}));
}

if (require.main === module) {
  const keepAlive = setInterval(() => {}, 1000);
  (process.argv[3] ? Promise.resolve().then(() => nativeContract(process.argv[3])) : main())
    .catch(error => { console.error(error); process.exitCode = 1; }).finally(() => clearInterval(keepAlive));
}
module.exports = { record, fixture };
