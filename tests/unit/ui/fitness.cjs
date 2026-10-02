'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const root = process.argv[2];
const source = fs.readFileSync(path.join(root, 'src/ui/intelligence_web/app/quality.js'), 'utf8');
const runtime = fs.readFileSync(path.join(root, 'src/ui/intelligence_web/app/runtime.js'), 'utf8');
const page = fs.readFileSync(path.join(root, 'src/ui/intelligence_web/page.html'), 'utf8');
const escape = text => String(text ?? '').replace(/[&<>"']/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[char]);
function harness() {
  const state = { current: 'A', language: 'en', syncError: false, project: null };
  const els = { fitnessObservatory: { innerHTML: '' }, fitnessBenchmark: { innerHTML: '' } };
  const context = { state, els, Number, String, Math, Intl,
    esc: escape, localized: (en, zh) => state.language === 'zh-CN' ? zh : en,
    t: value => value, statusLabel: value => value,
    pill: (label, tone) => `<span class="pill ${tone}">${escape(label)}</span>`,
    time: value => escape(new Date(value).toISOString()),
    setHtml: (_, element, html) => { element.innerHTML = html; },
  };
  vm.createContext(context);
  vm.runInContext(source, context, { filename: 'quality.js', timeout: 1000 });
  return { state, context, els, run: code => vm.runInContext(code, context, { timeout: 1000 }),
    render: project => { state.project = project; context.renderFitnessObservatory(); return els.fitnessObservatory.innerHTML; } };
}
function fixture() {
  return { workspace: 'A', proof: { revision_code: 'sha256:a', revision_design: 'sha256:d' },
    fitness: { schema_version: 1, available: true, revision: { code: 'sha256:a', design: 'sha256:d' },
      sampled_records: 4, retained_records: 512, unbound_records: 0, stale_records: 0,
      duplicate_records: 0, conflicting_events: 0, partial: false, history_truncated: false,
      window_start_ms: 1, window_end_ms: 4, observed_at_ms: 5,
      current: [{ tool: 'verify_project', verification_level: 'full', samples: 4,
        succeeded: 1, partial: 1, blocked: 1, failed: 1, success_rate: .25,
        p50_ms: 2.5, p95_ms: 4, trend: null }], history: [] } };
}
const results = [];
function test(name, fn) { try { fn(); results.push({ name, passed: true }); } catch (error) { results.push({ name, passed: false, error: String(error.stack) }); } }
test('missing and unavailable snapshots never infer success', () => {
  const h = harness();
  assert.match(h.render({ workspace: 'A' }), /unavailable/);
  const data = fixture(); data.fitness.available = false;
  const html = h.render(data); assert.match(html, /unavailable/); assert.doesNotMatch(html, /25%|100%/);
});
test('empty observations retain unknown rates and explicit measurement boundaries', () => {
  const h = harness(), data = fixture(); data.fitness.current = [];
  const html = h.render(data); assert.match(html, /No usable observations/);
  assert.match(html, /not a performance score/); assert.match(html, /explicit Check history reports/);
  assert.doesNotMatch(html, /100%/); assert.equal(h.run('fitnessRate(0, 0)'), 'Not measured');
  assert.equal(h.run('fitnessNumber(null)'), '—'); assert.equal(h.run('fitnessNumber(NaN)'), '—');
});
test('all recorded outcome kinds stay in the visible denominator', () => {
  const html = harness().render(fixture());
  assert.match(html, /25% \(1\/4\)/); assert.match(html, /1 \/ 1 \/ 1/);
  assert.match(html, /2\.5 \/ 4/); assert.match(html, /Insufficient comparable samples/);
  assert.match(html, /not all tool calls/); assert.match(html, /Not a release gate/);
});
test('mismatched code or design revisions cannot supply current rates', () => {
  for (const field of ['code', 'design']) {
    const data = fixture(); data.fitness.revision[field] = 'old';
    const html = harness().render(data); assert.match(html, /mismatched/); assert.doesNotMatch(html, /25%/);
  }
});
test('partial and cached snapshots suppress directional trend claims', () => {
  for (const mode of ['partial', 'syncError', 'cache']) {
    const h = harness(), data = fixture(), row = data.fitness.current[0];
    Object.assign(row, { samples: 6, succeeded: 3, partial: 1, blocked: 1, failed: 1,
      trend: { earlier_samples: 3, recent_samples: 3, success_rate_delta_pp: 33.3, p50_delta_ms: -2 } });
    if (mode === 'partial') data.fitness.partial = true;
    if (mode === 'syncError') h.state.syncError = true;
    if (mode === 'cache') data.snapshot_cache = 'stale-while-revalidate';
    const html = h.render(data); assert.doesNotMatch(html, /33\.3 pp/);
    assert.match(html, /Partial retained window|Cached or mismatched snapshot/);
  }
});
test('trends require enough comparable samples and finite deltas', () => {
  const h = harness(), data = fixture(), row = data.fitness.current[0];
  Object.assign(row, { samples: 6, succeeded: 3, partial: 1, blocked: 1, failed: 1,
    trend: { earlier_samples: 3, recent_samples: 3, success_rate_delta_pp: 33.3, p50_delta_ms: -2 } });
  assert.match(h.render(data), /\+33\.3 pp · -2 ms · 3 → 3/);
  assert.match(h.render(data), /do not establish a regression or statistical significance/);
  row.trend.success_rate_delta_pp = Infinity;
  assert.match(h.render(data), /Insufficient comparable samples/);
});
test('malformed counts are excluded instead of becoming a percent', () => {
  for (const count of [null, -1, NaN, 65]) {
    const data = fixture(); data.fitness.current[0].samples = count;
    const html = harness().render(data); assert.match(html, /Invalid or mismatched rows/); assert.doesNotMatch(html, /25%/);
  }
});
test('labels and history are escaped and revision history stays bounded', () => {
  const data = fixture(); data.fitness.current[0].tool = '<img src=x onerror=bad>';
  data.fitness.history = Array.from({ length: 20 }, (_, index) => ({ revision: { code: `<revision-${index}>`, design: 'd' }, samples: 1, last_at_ms: 1 }));
  const html = harness().render(data);
  assert.match(html, /&lt;img src=x onerror=bad&gt;/); assert.doesNotMatch(html, /<img/);
  assert.match(html, /&lt;revision-15&gt;/); assert.doesNotMatch(html, /revision-16/);
});
test('workspace switching clears previous observations and Chinese copy is available', () => {
  const h = harness(); assert.match(h.render(fixture()), /25%/);
  h.state.current = 'B'; assert.doesNotMatch(h.render(fixture()), /25%/);
  h.state.current = 'A'; h.state.language = 'zh-CN';
  assert.match(h.render(fixture()), /未绑定版本/); assert.match(h.render(fixture()), /不是发布门禁/);
});
test('production render and placeholder paths include the Fitness overview panel', () => {
  assert.match(page, /id="fitnessSection"[^>]+data-workspace-panel="overview"/);
  assert.match(page, /id="fitnessObservatory"/);
  assert.match(runtime, /case "overview":\s*renderFitnessObservatory\(\)/);
  assert.ok((runtime.match(/renderFitnessObservatory\(\)/g) || []).length >= 2);
  assert.match(runtime, /"revisions", "fitnessBenchmark", "fitnessObservatory", "languageQuality"/);
  assert.match(source, /role="region"[^>]+aria-label=[\s\S]*tabindex="0"/);
});
function withBenchmark() {
  const data = fixture();
  data.fitness.benchmark = { available: true, status: 'current', partial: false, invalid_reports: 0,
    duplicate_reports: 0, report_count: 1, history: [], latest: {
      case_count: 60, samples_per_case: 1, profile: 'debug', os: 'macos', arch: 'aarch64', model_calls: 0,
      controls_passed: 11, controls_total: 11, revision_before: { code: 'sha256:a', design: 'sha256:d' },
      revision_after: { code: 'sha256:a', design: 'sha256:d' }, source_snapshot_before: 'f', source_snapshot_after: 'f',
      source_stable_during_run: true, corpus_sha256: 'c', evaluator_sha256: 'e', test_binary_sha256: 'b',
      rows: [{ budget: 1000, phase: 'cold', required_count: 10, required_hits: 9, complete_body_hits: 8,
        fresh_sha_hits: 9, edit_input_eligible: 8, edit_input_ready: 6, ranking_attempts: 9,
        mean_ndcg_at_10: .8, noise_samples: 9, mean_non_gold_fraction: .1,
        p50_us: 20000, p95_us: 50000, attempts: 10, query_errors: 1, over_budget: 0, warmup_errors: 0 }],
    } };
  return data;
}
test('benchmark measurements do not depend on journal availability', () => {
  const h = harness(), data = withBenchmark(); data.fitness.available = false;
  assert.match(h.render(data), /observations unavailable/);
  const html = h.els.fitnessBenchmark.innerHTML;
  assert.match(html, /80% \(8\/10\)/); assert.match(html, /75% \(6\/8\)/);
  assert.match(html, /20 \/ 50/); assert.match(html, /n=9/);
  assert.match(html, /not model token usage/); assert.match(html, /not an external holdout/);
});
test('stale benchmark source and unavailable summaries are never current results', () => {
  const h = harness(), data = withBenchmark(); data.fitness.benchmark.latest.revision_after.code = 'old';
  h.render(data); assert.match(h.els.fitnessBenchmark.innerHTML, /Historical, partial or unconfirmed/);
  assert.doesNotMatch(h.els.fitnessBenchmark.innerHTML, />Current source observation</);
  data.fitness.benchmark = { available: true, status: 'not_measured', latest: null };
  h.render(data); assert.match(h.els.fitnessBenchmark.innerHTML, /never runs a benchmark automatically/);
  assert.doesNotMatch(h.els.fitnessBenchmark.innerHTML, /80%|100%/);
});
test('benchmark null costs remain unknown and a workspace switch clears them', () => {
  const h = harness(), data = withBenchmark();
  data.fitness.benchmark.latest.rows[0].p50_us = null;
  data.fitness.benchmark.latest.rows[0].p95_us = null;
  h.render(data); assert.match(h.els.fitnessBenchmark.innerHTML, /— \/ —/);
  h.state.current = 'B'; h.render(data);
  assert.doesNotMatch(h.els.fitnessBenchmark.innerHTML, /80%/);
  assert.match(h.els.fitnessBenchmark.innerHTML, /unavailable/);
});
test('restored client snapshots suppress current labels and directional trends', () => {
  const h = harness(), data = withBenchmark(), row = data.fitness.current[0];
  Object.assign(row, { samples: 6, succeeded: 3, partial: 1, blocked: 1, failed: 1,
    trend: { earlier_samples: 3, recent_samples: 3, success_rate_delta_pp: 33.3, p50_delta_ms: -2 } });
  h.state.fitnessSnapshotFromCache = true;
  assert.doesNotMatch(h.render(data), /33\.3 pp/);
  assert.doesNotMatch(h.els.fitnessBenchmark.innerHTML, />Current source observation</);
  assert.match(runtime, /state\.project = cached\.project;\s*state\.fitnessSnapshotFromCache = true/);
  assert.match(runtime, /state\.project = data; state\.current = data\.workspace;\s*state\.fitnessSnapshotFromCache = false/);
});
test('the production refresh failure handler invalidates already painted Fitness status', () => {
  const h = harness(), data = withBenchmark(), row = data.fitness.current[0];
  Object.assign(row, { samples: 6, succeeded: 3, partial: 1, blocked: 1, failed: 1,
    trend: { earlier_samples: 3, recent_samples: 3, success_rate_delta_pp: 33.3, p50_delta_ms: -2 } });
  assert.match(h.render(data), /33\.3 pp/);
  assert.match(h.els.fitnessBenchmark.innerHTML, />Current source observation</);
  const from = runtime.indexOf('function showRefreshFailure('), to = runtime.indexOf('function renderProjectPlaceholder(', from);
  assert.ok(from >= 0 && to > from);
  Object.assign(h.context, { console, setSync() {}, renderAttention() {}, renderLive() {},
    refreshFailureCopy: () => ({ title: 'Connection failed', detail: 'Unavailable' }),
    renderProjectPlaceholder: () => { throw new Error('A request error with an existing snapshot must not erase its historical data'); } });
  h.els.syncState = { title: '', parentElement: { setAttribute() {} } };
  vm.runInContext(runtime.slice(from, to), h.context, { filename: 'runtime-failure.js', timeout: 1000 });
  h.context.showRefreshFailure({ code: 'network' });
  assert.equal(h.state.syncError, true);
  assert.doesNotMatch(h.els.fitnessObservatory.innerHTML, /33\.3 pp/);
  assert.match(h.els.fitnessObservatory.innerHTML, /Cached or mismatched/);
  assert.doesNotMatch(h.els.fitnessBenchmark.innerHTML, />Current source observation</);
});
console.log(JSON.stringify({ results }, null, 2));
process.exitCode = results.every(result => result.passed) ? 0 : 1;
