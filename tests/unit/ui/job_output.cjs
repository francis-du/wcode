'use strict';
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { sandbox, project, respond, flush } = require('./observatory.cjs');
const idA = 'TASK-' + '1'.repeat(20) + '-' + 'a'.repeat(32);
const idB = 'TASK-' + '2'.repeat(20) + '-' + 'b'.repeat(32);
const stream = (text, extra = {}) => ({ text, total_bytes: Buffer.byteLength(text), truncated: false, redacted: false, ...extra });
function row(id = idA) {
  return { task_id: id, workspace: 'A', tool: 'run_command', status: 'completed', origin: 'mcp',
    can_cancel: false, created_at_ms: 1, updated_at_ms: 2, completion_is_not_success: true };
}
function listing(ids = [idA]) {
  return { schema_version: 1, kind: 'command_job_list', workspace: 'A', items: ids.map(row),
    truncated: false, coverage: 'retained_bounded', discovery_is_not_execution: true };
}
function detail(out = 'ordinary output\nneedle stdout', err = 'needle stderr', extra = {}) {
  return { schema_version: 1, kind: 'command_job', task_id: idA, workspace: 'A', tool: 'run_command',
    status: 'completed', origin: 'mcp', can_cancel: false, success: null, exit_code: null, error: null,
    stdout: stream(out), stderr: stream(err), completion_is_not_success: true, acceptance_ready: false, ...extra };
}
function prepared() {
  const s = sandbox(false, true, { fakeTimers: true });
  s.context.projectFixture = project();
  s.run('state.project=projectFixture;state.workspaceTab="activity";renderRuntimeJobs();');
  return s;
}
async function discover(s, ids = [idA]) {
  const pending = s.run('refreshRuntimeJobs()'); await flush();
  respond(s.requests.at(-1), listing(ids)); assert.equal(await pending, true);
}
async function observe(s, value = detail()) {
  s.context.nextId = value.task_id;
  const pending = s.run('observeRuntimeJob(nextId)'); await flush();
  respond(s.requests.at(-1), value); return pending;
}
async function ready(value = detail(), ids = [idA]) {
  const s = prepared(); await discover(s, ids); assert.equal(await observe(s, value), true); return s;
}
function search(s, query, selected = 'all', page = 0) {
  s.context.searchQuery = query; s.context.searchStream = selected; s.context.searchPage = page;
  return s.run('setRuntimeJobOutput(searchQuery,searchStream,searchPage)');
}
function view(s) { return JSON.parse(s.run('JSON.stringify(runtimeJobOutputView())')); }
function rows(s) { return view(s).rows.map(({ stream, line, text }) => ({ stream, line, text })); }
function remember(s) { s.run('globalThis.previousOutputView=runtimeJobOutputScope();'); }
function oldViewRejected(s) {
  assert.equal(s.run('setRuntimeJobOutput("old","all",0,previousOutputView)'), false);
}
function unchangedRequests(s, count) {
  assert.equal(s.requests.length, count, 'local output inspection must not send a request');
  assert.equal(s.requests.filter(request => request.options.method === 'POST').length, 0);
}
async function runAblations() {
  const root = process.argv[2] || path.resolve(__dirname, '../../..');
  const sourcePath = path.join(root, 'src/ui/intelligence_web/app/jobs.js');
  const source = fs.readFileSync(sourcePath, 'utf8').replace(/\r\n?/g, '\n');
  const digest = text => crypto.createHash('sha256').update(text).digest('hex');
  function replaceOnce(text, before, after) {
    assert.equal(text.split(before).length, 2, 'ablation anchor must occur once: ' + before);
    return text.replace(before, after);
  }
  async function isolated(mutation, value, ids = [idA]) {
    const s = prepared();
    if (mutation) {
      let changed = mutation(source);
      changed = replaceOnce(changed, 'const runtimeJobStatuses = ["all", "working", "input_required", "completed", "failed", "cancelled", "unknown"];\n', '');
      changed = replaceOnce(changed, 'const runtimeJobOrigins = ["all", "ui", "mcp"];\n', '');
      vm.runInContext(changed, s.context, { filename: 'job-output-isolated-ablation.js' });
    }
    await discover(s, ids); assert.equal(await observe(s, value), true); return s;
  }
  const cases = [
    {
      id: 'literal-output-search',
      mutation: text => replaceOnce(text, 'text.includes(view.query)', 'new RegExp(view.query).test(text)'),
      observe: async mutation => {
        const s = await isolated(mutation, detail('literal .* token\nunrelated\n海', 'error .*\nnoise'));
        search(s, '.*'); const current = view(s);
        return { invariant: current.total_matches === 2 && current.rows.every(row => row.text.includes('.*')),
          observed_lines: current.total_lines, matching_lines: current.total_matches };
      },
    },
    {
      id: 'exact-task-owner-output-scope',
      mutation: text => replaceOnce(text, 'if (!previous || previous.owner !== owner || previous.task !== task) {', 'if (!previous) {'),
      observe: async mutation => {
        const boundaries = [];
        for (const boundary of ['task', 'owner']) {
          const s = await isolated(mutation, detail(), [idA, idB]);
          search(s, 'needle'); remember(s);
          if (boundary === 'task') {
            assert.equal(await observe(s, detail('new task', '', { task_id: idB })), true);
          } else {
            s.run('state.runtimeJobsScope.owner="previous-owner";renderRuntimeJobs();');
            await discover(s); assert.equal(await observe(s, detail('new owner output', '')), true);
          }
          const current = view(s);
          const rejected = s.run('setRuntimeJobOutput("old","all",0,previousOutputView)') === false;
          boundaries.push({ boundary, available: current.available, query_cleared: current.query === '', stale_control_rejected: rejected });
        }
        const passed = boundaries.filter(row => row.available && row.query_cleared && row.stale_control_rejected).length;
        return { invariant: passed === 2, preserved_boundaries: passed, total_boundaries: 2, boundaries };
      },
    },
    {
      id: 'retained-output-coverage-explanation',
      mutation: text => {
        const start = text.indexOf('function runtimeJobOutputCoverage(view) {');
        const end = text.indexOf('function runtimeJobOutputHtml(view) {', start);
        assert.ok(start >= 0 && end > start, 'coverage function boundaries must exist');
        return text.slice(0, start) + 'function runtimeJobOutputCoverage(view) { return ""; }\n' + text.slice(end);
      },
      observe: async mutation => {
        const s = await isolated(mutation, detail('visible', '[REDACTED]', {
          stdout: stream('visible', { truncated: true, total_bytes: 999 }),
          stderr: stream('[REDACTED]', { redacted: true }),
        }));
        search(s, 'not-retained'); const html = s.node('#runtimeJobDetail').innerHTML, current = view(s);
        const explained = [/omitted text is not searched/, /hidden text is not searched/,
          /relative to each retained fragment/, /streams have no shared time order/].filter(pattern => pattern.test(html)).length;
        return { invariant: current.total_matches === 0 && current.limited && explained === 4,
          matching_lines: current.total_matches, limited: current.limited, explicit_coverage_facts: explained, required_coverage_facts: 4 };
      },
    },
  ];
  const results = [];
  for (const item of cases) {
    const baseline = await item.observe();
    assert.equal(baseline.invariant, true, item.id + ': baseline must satisfy the invariant');
    const ablated = await item.observe(item.mutation);
    assert.equal(ablated.invariant, false, item.id + ': isolated ablation must violate the invariant');
    const restored = await item.observe();
    assert.equal(restored.invariant, true, item.id + ': restored baseline must pass');
    assert.deepEqual(restored, baseline, item.id + ': restoration must reproduce baseline observations');
    results.push({ id: item.id, baseline, ablated, restored, passed: true });
  }
  assert.equal(digest(fs.readFileSync(sourcePath, 'utf8').replace(/\r\n?/g, '\n')), digest(source), 'production source changed during ablation');
  return { source_sha256: digest(source), passed: results.length, total: cases.length, results,
    limits: 'Synthetic production-renderer fixtures; no production mutation, browser, model-quality, latency or token-cost claim.' };
}
async function main() {
  const results = [];
  async function test(name, fn) {
    try { await fn(); results.push({ name, passed: true }); }
    catch (error) { results.push({ name, passed: false, error: error.message }); }
  }
  await test('empty query preserves the original retained streams and adds only local search controls', async () => {
    const s = await ready(), count = s.requests.length, current = view(s);
    assert.equal(current.available, true); assert.equal(current.query, ''); assert.deepEqual(current.rows, []);
    const html = s.node('#runtimeJobDetail').innerHTML;
    assert.match(html, /ordinary output/); assert.match(html, /needle stdout/); assert.match(html, /needle stderr/);
    assert.match(html, /data-job-output-query/); assert.match(html, /data-job-output-stream/);
    assert.doesNotMatch(html, /data-job-launch|name="program"/); unchangedRequests(s, count);
  });
  await test('metacharacters are literal and Unicode matches remain case-sensitive', async () => {
    const s = await ready(detail('Error .*[tag] $value \\path\n海🙂 X\nERROR literal', 'Error .*[tag]\n海🙂 error'));
    const count = s.requests.length;
    for (const [query, expected] of [['.*', 2], ['[tag]', 2], ['$value', 1], ['\\path', 1], ['海🙂', 2], ['ERROR', 1], ['error', 1]]) {
      assert.equal(search(s, query), true); assert.equal(view(s).total_matches, expected, query);
      assert.ok(rows(s).every(row => row.text.includes(query)), query);
    }
    unchangedRequests(s, count);
  });
  await test('stdout and stderr use independent retained line numbers across CRLF', async () => {
    const s = await ready(detail('head\r\nneedle first\r\nmiddle\r\nneedle last', 'needle error\nother'));
    search(s, 'needle');
    assert.deepEqual(rows(s), [
      { stream: 'stdout', line: 2, text: 'needle first' },
      { stream: 'stdout', line: 4, text: 'needle last' },
      { stream: 'stderr', line: 1, text: 'needle error' },
    ]);
    assert.equal(view(s).total_matches, 3); assert.equal(view(s).total_lines, 6);
  });
  await test('empty streams and CR-only trailing lines retain fragment-relative counting', async () => {
    const s = await ready(detail('head\rneedle\r', ''));
    search(s, 'needle', 'stdout');
    assert.equal(view(s).total_lines, 3);
    assert.deepEqual(rows(s), [{ stream: 'stdout', line: 2, text: 'needle' }]);
    search(s, 'needle', 'stderr'); assert.equal(view(s).total_lines, 0); assert.equal(view(s).total_matches, 0);
    assert.deepEqual(rows(s), []);
  });
  await test('stream selection cannot invent a match across stream boundaries', async () => {
    const s = await ready(detail('needle out\nleft', 'right\nneedle err')), count = s.requests.length;
    search(s, 'needle', 'stdout'); assert.deepEqual(rows(s).map(row => row.stream), ['stdout']);
    search(s, 'needle', 'stderr'); assert.deepEqual(rows(s).map(row => row.stream), ['stderr']);
    search(s, 'leftright'); assert.equal(view(s).total_matches, 0); assert.deepEqual(rows(s), []);
    unchangedRequests(s, count);
  });
  await test('multiple occurrences on one retained line count as one matching line', async () => {
    const s = await ready(detail('needle needle needle\nno match', 'needle'));
    search(s, 'needle'); assert.equal(view(s).total_matches, 2); assert.equal(rows(s).length, 2);
  });
  await test('pagination visits every matching line once and clamps an oversized page', async () => {
    const s = await ready(detail(Array.from({ length: 58 }, (_, i) => 'hit out ' + i).join('\n'),
      Array.from({ length: 3 }, (_, i) => 'hit err ' + i).join('\n'))), count = s.requests.length;
    const visited = [];
    for (let page = 0; page < 3; page++) {
      assert.equal(search(s, 'hit', 'all', page), true);
      const current = view(s);
      assert.equal(current.page, page); assert.equal(current.pages, 3);
      assert.equal(current.total_lines, 61); assert.equal(current.total_matches, 61);
      assert.equal(current.rows.length, page < 2 ? 25 : 11);
      visited.push(...current.rows.map(row => row.stream + ':' + row.line));
    }
    assert.equal(new Set(visited).size, 61);
    assert.equal(search(s, 'hit', 'all', 99), true); assert.equal(view(s).page, 2);
    unchangedRequests(s, count);
  });
  await test('changing query or stream returns to the first matching page', async () => {
    const out = Array.from({ length: 61 }, (_, i) => 'hit ' + i).join('\n');
    const s = await ready(detail(out, out));
    search(s, 'hit'); search(s, 'hit', 'all', 2); assert.equal(view(s).page, 2);
    search(s, 'it', 'all', 2); assert.equal(view(s).page, 0);
    search(s, 'it', 'all', 2); assert.equal(view(s).page, 2);
    search(s, 'it', 'stdout', 2); assert.equal(view(s).page, 0); assert.equal(view(s).total_lines, 61);
  });
  await test('query and page validation reject malformed input without altering an established search', async () => {
    const s = await ready(detail('x'.repeat(256), 'none'));
    assert.equal(search(s, 'x'.repeat(256)), true); assert.equal(view(s).total_matches, 1);
    const before = view(s);
    for (const [query, selected, page] of [
      ['x'.repeat(257), 'all', 0], ['a\nb', 'all', 0], ['a\rb', 'all', 0], ['a\0b', 'all', 0],
      [null, 'all', 0], [4, 'all', 0], ['x', 'combined', 0], ['x', '<script>', 0],
      ['x', 'all', -1], ['x', 'all', 1.5], ['x', 'all', Infinity], ['x', 'all', '1'],
    ]) {
      assert.equal(search(s, query, selected, page), false);
      assert.deepEqual(view(s), before);
    }
  });
  await test('search text and matching output remain escaped rather than executable markup', async () => {
    const s = await ready(detail('<script>alert("unsafe")</script>\na&b', '<img src=x onerror=alert(1)>'));
    search(s, '<script>');
    assert.equal(view(s).total_matches, 1);
    const html = s.node('#runtimeJobDetail').innerHTML;
    assert.match(html, /&lt;script&gt;/); assert.doesNotMatch(html, /<script>|<img src=x/);
    search(s, 'a&b'); assert.equal(view(s).total_matches, 1);
    assert.match(s.node('#runtimeJobDetail').innerHTML, /a&amp;b/);
  });
  await test('zero matches retain truncation redaction and limited-coverage explanations', async () => {
    const s = await ready(detail('visible', '[REDACTED]', {
      stdout: stream('visible', { truncated: true, total_bytes: 999 }),
      stderr: stream('[REDACTED]', { redacted: true }),
    }));
    search(s, 'not-retained'); const current = view(s), html = s.node('#runtimeJobDetail').innerHTML;
    assert.equal(current.total_matches, 0); assert.deepEqual(current.rows, []);
    assert.equal(current.limited, true); assert.equal(current.truncated, true); assert.equal(current.redacted, true);
    assert.match(html, /retained/i); assert.match(html, /truncated/i); assert.match(html, /redacted/i);
    assert.doesNotMatch(html, /complete (log|output)|all output (has been )?searched/i);
  });
  await test('coverage flags and line totals describe only the selected stream', async () => {
    const s = await ready(detail('visible\nsecond', '[REDACTED]', {
      stdout: stream('visible\nsecond', { truncated: true, total_bytes: 999 }),
      stderr: stream('[REDACTED]', { redacted: true }),
    }));
    search(s, 'absent', 'stdout');
    assert.equal(view(s).total_lines, 2); assert.equal(view(s).truncated, true); assert.equal(view(s).redacted, false);
    search(s, 'absent', 'stderr');
    assert.equal(view(s).total_lines, 1); assert.equal(view(s).truncated, false); assert.equal(view(s).redacted, true);
    await observe(s, detail('complete visible', '[REDACTED]', { stderr: stream('[REDACTED]', { redacted: true }) }));
    search(s, 'absent', 'stdout');
    assert.equal(view(s).limited, false); assert.equal(view(s).truncated, false); assert.equal(view(s).redacted, false);
  });
  await test('selecting another exact task withdraws old search before the new response arrives', async () => {
    const s = await ready(detail(), [idA, idB]); search(s, 'needle'); remember(s);
    s.context.nextId = idB; const pending = s.run('observeRuntimeJob(nextId)'); await flush();
    assert.equal(view(s).available, false); assert.deepEqual(view(s).rows, []); oldViewRejected(s);
    respond(s.requests.at(-1), detail('new task line', '', { task_id: idB })); assert.equal(await pending, true);
    assert.equal(view(s).query, ''); assert.equal(view(s).page, 0); assert.equal(view(s).stream, 'all');
    search(s, 'new task'); assert.equal(view(s).total_matches, 1); oldViewRejected(s);
  });
  await test('same-task fresh output keeps the query and clamps the page to new retained results', async () => {
    const s = await ready(detail(Array.from({ length: 61 }, (_, i) => 'hit ' + i).join('\n'), ''));
    search(s, 'hit', 'stdout'); search(s, 'hit', 'stdout', 2); assert.equal(view(s).page, 2);
    assert.equal(await observe(s, detail(Array.from({ length: 31 }, (_, i) => 'hit changed ' + i).join('\n'), '')), true);
    const current = view(s);
    assert.equal(current.query, 'hit'); assert.equal(current.stream, 'stdout');
    assert.equal(current.total_matches, 31); assert.equal(current.page, 1); assert.equal(current.rows.length, 6);
  });
  await test('workspace owner and observation-epoch changes invalidate old output controls', async () => {
    for (const boundary of ['workspace', 'owner', 'epoch']) {
      const s = await ready(); search(s, 'needle'); remember(s);
      if (boundary === 'workspace') s.run('state.current="B";');
      else if (boundary === 'owner') s.run('state.runtimeJobsScope.owner="previous-owner";');
      else s.run('state.workspaceEpoch += 1;');
      s.run('renderRuntimeJobs();'); oldViewRejected(s);
      assert.equal(view(s).available, false); assert.deepEqual(view(s).rows, []); assert.equal(view(s).query, '');
    }
  });
  await test('failed exact-task refresh withdraws old matches and invalidates the old view', async () => {
    const s = await ready(); search(s, 'needle'); remember(s);
    const pending = s.run('observeRuntimeJob()'); await flush();
    s.requests.at(-1).reject(new Error('PRIVATE ERROR')); assert.equal(await pending, false);
    assert.equal(view(s).available, false); assert.deepEqual(view(s).rows, []); assert.equal(view(s).query, '');
    oldViewRejected(s); assert.doesNotMatch(s.node('#runtimeJobDetail').innerHTML, /needle stdout|PRIVATE ERROR/);
  });
  await test('invalid detail cannot leave prior matching output visible', async () => {
    const s = await ready(); search(s, 'needle'); remember(s);
    assert.equal(await observe(s, detail('forged', '', { workspace: 'B' })), false);
    assert.equal(view(s).available, false); assert.deepEqual(view(s).rows, []); assert.equal(view(s).query, '');
    oldViewRejected(s);
  });
  await test('discovery failure withdraws current search rather than interpreting it as an empty log', async () => {
    const s = await ready(); search(s, 'needle'); remember(s);
    const pending = s.run('refreshRuntimeJobs()'); await flush();
    s.requests.at(-1).resolve({ ok: false, status: 503, json: async () => ({ error: 'PRIVATE' }) });
    assert.equal(await pending, false); assert.equal(view(s).available, false); assert.deepEqual(view(s).rows, []);
    assert.equal(view(s).query, ''); oldViewRejected(s);
    assert.match(s.node('#runtimeJobMessage').innerHTML, /unavailable/);
  });
  await test('list filtering preserves the exact selected output search without requests or cancellation rights', async () => {
    const s = await ready(); search(s, 'needle', 'stderr'); const count = s.requests.length;
    assert.equal(s.run('setRuntimeJobFilters("failed","ui")'), true);
    assert.equal(view(s).query, 'needle'); assert.equal(view(s).stream, 'stderr');
    assert.deepEqual(rows(s), [{ stream: 'stderr', line: 1, text: 'needle stderr' }]);
    assert.doesNotMatch(s.node('#runtimeJobDetail').innerHTML, /data-job-cancel/); unchangedRequests(s, count);
  });
  const ablations = await runAblations();
  const report = { suite: 'native-job-output-search', passed: results.filter(row => row.passed).length, total: results.length, results, ablations };
  console.log(JSON.stringify(report, null, 2));
  assert.equal(report.passed, report.total, results.filter(row => !row.passed).map(row => row.name).join('\n'));
  return report;
}
if (require.main === module) {
  const keepAlive = setInterval(() => {}, 1000);
  main().catch(error => { console.error(error.message); process.exitCode = 1; }).finally(() => clearInterval(keepAlive));
}
module.exports = { prepared, row, listing, detail, stream, discover, observe, ready, search, view, rows, main };
