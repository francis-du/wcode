'use strict';
// Production bundle with a deterministic DOM/event adapter. Native WebKit
// separately checks actual viewport geometry, focus and scrolling.
const assert = require('node:assert/strict');
const {sandbox, project, respond, flush} = require('./observatory.cjs');
const A = 'TASK-' + '1'.repeat(20) + '-' + 'a'.repeat(32);
const B = 'TASK-' + '2'.repeat(20) + '-' + 'b'.repeat(32);
const decode = value => String(value).replace(/&(amp|lt|gt|quot|#39|#039);/g, (_, key) => ({amp:'&',lt:'<',gt:'>',quot:'"','#39':"'",'#039':"'"}[key]));

class Element {
  constructor(document, tag = 'div', attributes = '') {
    this.document = document; this.tagName = tag.toUpperCase(); this.attrs = {}; this.dataset = {};
    this.children = []; this.events = {}; this.value = ''; this.markup = ''; this.writes = 0;
    this.scrollTop = 0; this.scrollLeft = 0; this.scrollHeight = tag === 'pre' ? 6000 : 0;
    this.selectionStart = 0; this.selectionEnd = 0;
    for (const match of attributes.matchAll(/([\w:-]+)(?:="([^"]*)")?/g)) {
      this.attrs[match[1]] = decode(match[2] ?? '');
      if (match[1].startsWith('data-')) this.dataset[match[1].slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase())] = this.attrs[match[1]];
    }
    this.open = Object.hasOwn(this.attrs, 'open');
    this.disabled = Object.hasOwn(this.attrs, 'disabled');
    this.classList = {contains: name => (this.attrs.class || '').split(/\s+/).includes(name)};
  }
  set innerHTML(value) {
    this.markup = value; this.children = []; this.writes++;
    const stack = [{node:this, start:0}];
    for (const match of value.matchAll(/<\/?([a-z][\w-]*)\b([^>]*)>/gi)) {
      const closing = match[0].startsWith('</'), tag = match[1].toLowerCase();
      if (closing) {
        const frame = stack.at(-1);
        if (stack.length > 1 && frame.node.tagName.toLowerCase() === tag) {
          frame.node.markup = value.slice(frame.start, match.index); stack.pop();
        }
      } else {
        const node = new Element(this.document, tag, match[2]);
        stack.at(-1).node.children.push(node);
        if (!['input','br','hr','img','meta','link'].includes(tag)) stack.push({node, start:match.index + match[0].length});
      }
    }
  }
  get innerHTML() { return this.markup; }
  get textContent() { return decode(this.markup.replace(/<[^>]*>/g, '')); }
  getAttribute(key) { return this.attrs[key] ?? null; }
  setAttribute(key, value) { this.attrs[key] = String(value); }
  addEventListener(type, handler) { (this.events[type] ||= []).push(handler); }
  fire(type) { for (const handler of this.events[type] || []) handler({type, target:this}); }
  click() { this.fire('click'); }
  focus() { this.document.activeElement = this; }
  querySelectorAll(selector) {
    const attribute = /^\[([^=\]]+)(?:="([^"]*)")?\]$/.exec(selector), found = [];
    const visit = parent => { for (const node of parent.children) {
      if (attribute ? Object.hasOwn(node.attrs, attribute[1]) && (attribute[2] === undefined || node.attrs[attribute[1]] === attribute[2]) : node.tagName.toLowerCase() === selector) found.push(node);
      visit(node);
    }};
    visit(this); return found;
  }
  querySelector(selector) { return this.querySelectorAll(selector)[0] || null; }
}
const stream = (text, extra = {}) => ({text, total_bytes:Buffer.byteLength(text), truncated:false, redacted:false, ...extra});
function job(id = A, workspace = 'A', extra = {}) {
  return {schema_version:1, kind:'command_job', workspace, task_id:id, tool:'run_command', origin:'mcp', status:'working', can_cancel:false,
    success:null, exit_code:null, error:null, completion_is_not_success:true, acceptance_ready:false,
    stdout:stream('start\n[ERR] compile 海\n<script>not executable</script>\nprogress .* literal\n'),
    stderr:stream('warning\nerror at source\n'), ...extra};
}
function prepared(extra = {}) {
  const s = sandbox(false, true, {fakeTimers:true});
  for (const id of ['runtimeJobForm','runtimeJobList','runtimeJobDetail','runtimeJobMessage','runtimeJobOutputFilters','runtimeJobOutput']) s.nodes.set('#' + id, new Element(s.context.document));
  s.context.projectFixture = project(); s.run('state.project=projectFixture;state.workspaceTab="activity";clearRuntimeJobs();');
  const select = (id = A, patch = {}) => {
    s.context.jobFixture = job(id, s.run('state.current'), patch);
    s.context.retained = [A,B].map(task_id => ({task_id, workspace:s.run('state.current'), tool:'run_command', origin:'mcp', status:'working', can_cancel:false,
      created_at_ms:1, updated_at_ms:2, completion_is_not_success:true}));
    s.run('state.runtimeJobs=retained;state.runtimeJobsLoaded=true;state.runtimeJobSelected=jobFixture.task_id;state.runtimeJob=jobFixture;renderRuntimeJobs();');
  };
  const control = name => {const node = s.node('#runtimeJobOutputFilters').querySelector('[data-job-output-' + name + ']'); assert.ok(node, 'missing ' + name); return node;};
  const details = name => s.node('#runtimeJobOutput').querySelector('[data-job-output-stream-name="' + name + '"]');
  const output = name => details(name)?.querySelector('[data-job-output-text]');
  const search = text => {const input=control('query');input.value=text;input.fire('input');};
  const filter = name => {const input=control('stream');input.value=name;input.fire('change');};
  select(A, extra); control('case').checked=false;control('case').fire('change'); return {...s, select, control, details, output, search, filter};
}

async function main() {
  const results = [];
  async function test(name, fn) {try {await fn();results.push({name,passed:true});} catch(error) {results.push({name,passed:false,error:error.stack});}}
  await test('rendered output is read-only and keeps task selection and lifecycle separate from success', async () => {
    const s = prepared();
    assert.equal(s.requests.length, 0);
    const selected = s.node('#runtimeJobList').querySelector('[data-job-id="' + A + '"]');
    assert.equal(selected.getAttribute('aria-pressed'), 'true'); assert.equal(selected.classList.contains('selected'), true);
    assert.match(s.node('#runtimeJobDetail').innerHTML, /Unknown/); assert.doesNotMatch(s.node('#runtimeJobDetail').innerHTML, /Reported success|data-job-cancel/);
    for (const name of ['stdout','stderr']) {assert.equal(s.output(name).getAttribute('tabindex'), '0');assert.equal(s.output(name).getAttribute('role'), 'region');}
  });
  await test('literal search handles case, brackets, regex-looking text and Chinese without requests', async () => {
    const s = prepared();
    for (const [query, expected] of [['[err]','2: [ERR] compile 海'],['海','2: [ERR] compile 海'],['.*','4: progress .* literal']]) {
      s.search(query); assert.equal(s.output('stdout').textContent, expected);
      assert.match(s.node('#runtimeJobOutput').innerHTML, /1 matching lines/);
    }
    assert.equal(s.requests.length, 0);
  });
  await test('stream filtering preserves hidden-stream truncation and redaction boundaries', async () => {
    const s = prepared({stderr:stream('[REDACTED]',{truncated:true,redacted:true,total_bytes:80000})});
    s.filter('stdout'); assert.equal(s.details('stderr'), null);
    assert.match(s.node('#runtimeJobOutput').innerHTML, /Output was truncated/);
    assert.match(s.node('#runtimeJobOutput').innerHTML, /redacted before display and search/);
    s.filter('stderr'); assert.equal(s.details('stdout'), null); assert.equal(s.output('stderr').textContent, '[REDACTED]');
    assert.equal(s.requests.length, 0);
  });
  await test('search pages all matching lines with a 25-line rendered bound', async () => {
    const s = prepared({stdout:stream(Array.from({length:205}, (_,i) => 'hit ' + i).join('\n')),stderr:stream('hit other')});
    s.search('hit');
    assert.equal(s.output('stdout').textContent.split('\n').length, 25);
    assert.match(s.node('#runtimeJobOutput').innerHTML, /206 matching lines/);
    assert.match(s.node('#runtimeJobOutput').innerHTML, /Page 1 \/ 9/);
    assert.equal(s.run('runtimeJobOutputView().total_matches'),206);
    assert.doesNotMatch(s.output('stdout').textContent, /hit 204/);
  });
  await test('empty retained streams and empty search results do not claim absence outside the window', async () => {
    const s = prepared({stdout:stream(''),stderr:stream('unrelated',{truncated:true})});
    assert.match(s.node('#runtimeJobOutput').innerHTML, /No text retained for this stream/);
    s.search('missing'); assert.equal(s.output('stdout'), null);assert.equal(s.output('stderr'), null);
    assert.match(s.node('#runtimeJobOutput').innerHTML, /0 matching lines/);
    assert.match(s.node('#runtimeJobOutput').innerHTML, /does not rule out a match in omitted output/);
  });
  await test('HTML from retained output remains escaped before and after search', async () => {
    const s = prepared();
    assert.match(s.node('#runtimeJobOutput').innerHTML, /&lt;script&gt;/);
    assert.equal(s.node('#runtimeJobOutput').querySelector('script'), null);
    s.search('<script>'); assert.equal(s.output('stdout').textContent, '3: <script>not executable</script>');
    assert.equal(s.node('#runtimeJobOutput').querySelector('script'), null);assert.equal(s.requests.length, 0);
  });
  await test('search input stays focused with its caret while typing and a same-task refresh changes output', async () => {
    const s = prepared(), input = s.control('query'); input.focus();input.selectionStart=2;input.selectionEnd=2;
    s.search('er');assert.equal(s.control('query'),input);assert.equal(s.context.document.activeElement,input);
    s.run('state.runtimeJobsBusy=true;renderRuntimeJobs();');
    s.run('state.runtimeJob={...state.runtimeJob,stdout:{...state.runtimeJob.stdout,text:state.runtimeJob.stdout.text+"ERROR appended\\n"}};state.runtimeJobsBusy=false;renderRuntimeJobs();');
    assert.equal(s.control('query'),input);assert.equal(s.context.document.activeElement,input);assert.equal(input.selectionStart,2);assert.equal(input.selectionEnd,2);
    assert.equal(input.value,'er');assert.match(s.output('stdout').textContent,/ERROR appended/);
  });
  await test('poll rendering without changed output retains the existing log elements', async () => {
    const s = prepared(), stdout = s.output('stdout'), detail = s.details('stderr');stdout.scrollTop=137;detail.open=false;
    s.run('state.runtimeJobsBusy=true;renderRuntimeJobs();state.runtimeJobsBusy=false;renderRuntimeJobs();');
    assert.equal(s.output('stdout'),stdout);assert.equal(s.details('stderr'),detail);assert.equal(stdout.scrollTop,137);assert.equal(detail.open,false);
  });
  await test('changed output on the same task preserves scroll positions and folded streams', async () => {
    const s = prepared();s.output('stdout').scrollTop=137;s.output('stdout').scrollLeft=7;s.details('stderr').open=false;
    s.run('state.runtimeJob={...state.runtimeJob,stdout:{...state.runtimeJob.stdout,text:state.runtimeJob.stdout.text+"later\\n",total_bytes:300}};renderRuntimeJobs();');
    assert.equal(s.output('stdout').scrollTop,137);assert.equal(s.output('stdout').scrollLeft,7);assert.equal(s.details('stderr').open,false);
    assert.equal(s.requests.length,0);
  });
  await test('focused log keeps keyboard focus after new content replaces its element without stealing search focus', async () => {
    const s = prepared(), original = s.output('stdout');original.focus();original.scrollTop=91;
    s.run('state.runtimeJob={...state.runtimeJob,stdout:{...state.runtimeJob.stdout,text:state.runtimeJob.stdout.text+"new log data\\n"}};renderRuntimeJobs();');
    assert.notEqual(s.output('stdout'),original);assert.equal(s.context.document.activeElement,s.output('stdout'));assert.equal(s.output('stdout').scrollTop,91);
    const query=s.control('query');query.focus();query.selectionStart=0;query.selectionEnd=0;
    s.run('state.runtimeJob={...state.runtimeJob,stdout:{...state.runtimeJob.stdout,text:state.runtimeJob.stdout.text+"more data\\n"}};renderRuntimeJobs();');
    assert.equal(s.context.document.activeElement,query);assert.equal(s.control('query'),query);assert.equal(s.output('stdout').scrollTop,91);assert.equal(s.requests.length,0);
  });
  await test('changing filters resets reading position and clear search retains keyboard focus', async () => {
    const s = prepared();s.output('stdout').scrollTop=100;s.search('compile');assert.equal(s.output('stdout').scrollTop,0);
    s.output('stdout').scrollTop=50;s.filter('stdout');assert.equal(s.output('stdout').scrollTop,0);
    s.control('clear').click();assert.equal(s.control('query').value,'');assert.equal(s.context.document.activeElement,s.control('query'));assert.equal(s.requests.length,0);
  });
  await test('explicit jump-to-end affects only currently displayed streams', async () => {
    const s = prepared();s.filter('stdout');s.control('end').click();assert.equal(s.output('stdout').scrollTop,s.output('stdout').scrollHeight);
    assert.equal(s.details('stderr'),null);assert.equal(s.requests.length,0);
  });
  await test('new task resets search, stream, scroll and disclosure state', async () => {
    const s = prepared();s.search('compile');s.filter('stdout');s.output('stdout').scrollTop=100;s.details('stdout').open=false;
    s.select(B);assert.equal(s.control('query').value,'');assert.equal(s.control('stream').value,'all');
    assert.equal(s.output('stdout').scrollTop,0);assert.equal(s.details('stdout').open,true);assert.ok(s.details('stderr'));
  });
  await test('detached controls cannot change a replacement task view', async () => {
    const s = prepared(), old = Object.fromEntries(['query','stream','clear','end'].map(name => [name,s.control(name)]));
    s.select(B);s.search('warning');s.filter('stderr');const before=s.node('#runtimeJobOutput').innerHTML;
    old.query.value='compile';old.query.fire('input');old.stream.value='stdout';old.stream.fire('change');old.clear.click();old.end.click();
    assert.equal(s.control('query').value,'warning');assert.equal(s.control('stream').value,'stderr');assert.equal(s.output('stderr').scrollTop,0);
    assert.equal(s.node('#runtimeJobOutput').innerHTML,before);assert.equal(s.requests.length,0);
  });
  await test('stale owner scope and workspace A to B to A invalidate old output callbacks', async () => {
    for (const change of ['state.runtimeJobsScope.owner="obsolete";runtimeJobsScope();','state.current="B";state.workspaceEpoch++;clearRuntimeJobs();state.current="A";state.workspaceEpoch++;clearRuntimeJobs();']) {
      const s = prepared(), old = s.control('query');s.search('warning');s.run(change);s.select();
      old.value='compile';old.fire('input');assert.equal(s.control('query').value,'');assert.equal(s.requests.length,0);
    }
  });
  await test('selecting another job clears previous output while its exact GET is pending', async () => {
    const s = prepared();s.context.nextJob=B;const pending=s.run('observeRuntimeJob(nextJob)');await flush();
    assert.equal(s.requests.length,1);assert.equal(s.requests[0].options.method,'GET');assert.match(s.requests[0].url,new RegExp(B));
    assert.equal(s.node('#runtimeJobOutput').innerHTML,'');assert.equal(s.node('#runtimeJobOutputFilters').innerHTML,'');
    respond(s.requests[0],job(B,'A',{stdout:stream('new task output')}));assert.equal(await pending,true);assert.equal(s.output('stdout').textContent,'new task output');
  });
  await test('unavailable exact-task refresh removes stale output without a retry or rerun', async () => {
    const s = prepared();const pending=s.run('observeRuntimeJob()');await flush();
    respond(s.requests[0],{error:'private backend detail'},false);assert.equal(await pending,false);
    assert.equal(s.node('#runtimeJobOutput').innerHTML,'');assert.equal(s.node('#runtimeJobOutputFilters').innerHTML,'');
    assert.match(s.node('#runtimeJobMessage').innerHTML,/unknown/);assert.equal(s.requests.length,1);assert.equal(s.requests[0].options.method,'GET');
  });
  await test('query length is bounded and unrecognized stream options remain inert', async () => {
    const s = prepared();s.search('x'.repeat(400));assert.equal(s.control('query').value.length,256);
    s.filter('<script>');assert.equal(s.run('state.runtimeJobOutput.stream'),'all');assert.equal(s.requests.length,0);
  });
  await test('navigating away prevents retained output controls from changing view state', async () => {
    const s = prepared(), query = s.control('query'), end = s.control('end');s.run('state.workspaceTab="overview";');
    query.value='compile';query.fire('input');end.click();assert.equal(s.run('state.runtimeJobOutput.query'),'');assert.equal(s.output('stdout').scrollTop,0);assert.equal(s.requests.length,0);
  });
  await test('raw output retains CRLF, trailing terminators and Unicode without normalization', async () => {
    const text='海\r\nprogress\rupdate\r\n\r\n',s=prepared({stdout:stream(text)});
    assert.equal(s.output('stdout').textContent,text);s.search('海');assert.equal(s.output('stdout').textContent,'1: 海');
  });
  await test('changing job-list filters preserves the selected output view and scroll', async () => {
    const s=prepared();s.search('compile');s.output('stdout').scrollTop=70;s.run('setRuntimeJobFilters("failed","all");');
    assert.equal(s.control('query').value,'compile');assert.equal(s.output('stdout').scrollTop,70);assert.equal(s.run('state.runtimeJobSelected'),A);assert.equal(s.requests.length,0);
  });
  await test('clearing workspace view immediately removes prior workspace output and search controls', async () => {
    const s=prepared();s.run('state.current="B";clearWorkspaceView();');
    assert.equal(s.node('#runtimeJobOutput').innerHTML,'');assert.equal(s.node('#runtimeJobOutputFilters').innerHTML,'');
  });
  await test('match-case choice changes only literal local results and resets matching pagination', async () => {
    const s=prepared({stdout:stream(Array.from({length:30},()=> 'Error').join('\n')),stderr:stream('error')});
    s.search('error');assert.equal(s.run('runtimeJobOutputView().total_matches'),31);
    s.run('setRuntimeJobOutput("error","all",1);');assert.equal(s.run('runtimeJobOutputView().page'),1);
    const option=s.control('case');option.checked=true;option.fire('change');
    assert.equal(s.run('runtimeJobOutputView().total_matches'),1);assert.equal(s.run('runtimeJobOutputView().page'),0);
    const old=option;s.select(B);old.checked=false;old.fire('change');
    assert.equal(s.run('runtimeJobOutputScope().caseSensitive'),true);assert.equal(s.requests.length,0);
  });
  await test('composition keeps the uncommitted input and applies it only after composition ends', async () => {
    const s=prepared({stdout:stream('定位\noriginal')});s.search('original');
    const input=s.control('query');input.focus();input.fire('compositionstart');input.value='定位';input.fire('input');
    s.run('state.runtimeJob={...state.runtimeJob,stdout:{...state.runtimeJob.stdout,text:"定位 fresh\\noriginal"}};renderRuntimeJobs();');
    assert.equal(s.control('query'),input);assert.equal(input.value,'定位');assert.equal(s.run('runtimeJobOutputScope().query'),'original');
    input.fire('compositionend');assert.equal(s.run('runtimeJobOutputScope().query'),'定位');
    assert.equal(s.output('stdout').textContent,'1: 定位 fresh');assert.equal(s.context.document.activeElement,input);assert.equal(s.requests.length,0);
  });
  console.log(JSON.stringify({suite:'retained-job-output',results},null,2));
  assert.ok(results.length > 0 && results.every(row=>row.passed),results.filter(row=>!row.passed).map(row=>row.name).join('\n'));
}
if (require.main === module) main().catch(error=>{console.error(error);process.exitCode=1;});
module.exports = { prepared, Element, job, stream, main };
