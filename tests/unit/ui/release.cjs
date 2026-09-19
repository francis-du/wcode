'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {sandbox, project, respond, flush} = require('./observatory.cjs');
const results = [];
async function test(name, run) {
  try { await run(); results.push({name, passed: true}); }
  catch (error) { results.push({name, passed: false, error: error.stack}); }
}
async function main() {
  await test('semantic success from an earlier A view cannot refresh a later A view', async () => {
    const s = sandbox();
    s.run('globalThis.refreshes=0;refreshProject=async()=>{refreshes++;return true;};');
    const pending = s.run('refreshSemantics()');
    await flush();
    s.run('state.current="B";clearWorkspaceView();state.current="A";clearWorkspaceView();');
    respond(s.requests[0], {runs: [], failures: []});
    await pending;
    assert.equal(s.run('refreshes'), 0);
    assert.equal(s.node('#refreshSemantic').disabled, false);
  });
  await test('obsolete semantic authorization errors cannot reopen the access panel', async () => {
    const s = sandbox();
    s.run('globalThis.accessReads=0;loadAccess=async()=>{accessReads++;};');
    const pending = s.run('refreshSemantics()');
    await flush();
    s.run('state.current="B";clearWorkspaceView();state.current="A";clearWorkspaceView();');
    respond(s.requests[0], {error: 'authorization required'}, false);
    await pending;
    assert.equal(s.run('state.semanticRefreshPending'), false);
    assert.equal(s.run('accessReads'), 0);
    assert.equal(s.run('accessPanelOpen()'), false);
  });
  await test('a pre-approval revision response cannot invalidate a fresh authorization list', async () => {
    const s = sandbox();
    s.context.fixture = project();
    s.run('state.project=fixture;state.revisionKey="same|||";state.accessLoaded=true;state.pendingValue=1;');
    const poll = s.run('pollRevision()');
    await flush();
    const approve = s.run('decideAuthorization("AUTH-release",true)');
    await flush();
    respond(s.requests[1], {pending: [], request: {status: 'approved'}});
    await flush();
    respond(s.requests[2], {allowed_commands: []});
    await approve;
    respond(s.requests[0], {workspace: 'A', fingerprint: 'same', pending_authorizations: 1});
    await poll;
    assert.equal(s.run('pendingCount()'), 0);
    assert.equal(s.run('state.accessLoaded'), true);
  });
  await test('server-side write failures require state reconciliation and never automatic replay', async () => {
    const s = sandbox();
    const pending = s.run('uiJson("/intelligence/commands","POST",{program:"cargo"})');
    const rejected = assert.rejects(pending, error => error.status === 503 && error.uncertain === true);
    await flush();
    respond(s.requests[0], {error: 'response failed after mutation'}, false);
    await rejected;
    assert.equal(s.requests.length, 1);
  });
  await test('explicit authorization rejection is not described as an uncertain write', async () => {
    const s = sandbox();
    const pending = s.run('uiJson("/intelligence/commands","POST",{program:"cargo"})');
    const rejected = assert.rejects(pending, error => error.status === 403 && !error.uncertain);
    await flush();
    s.requests[0].resolve({ok: false, status: 403, json: async () => ({error: 'forbidden'})});
    await rejected;
    assert.equal(s.requests.length, 1);
  });
  await test('current semantic refresh still refreshes the project exactly once', async () => {
    const s = sandbox();
    s.run('globalThis.refreshes=0;refreshProject=async()=>{refreshes++;return true;};');
    const pending = s.run('refreshSemantics()');
    await flush();
    respond(s.requests[0], {runs: [], failures: []});
    await pending;
    assert.equal(s.run('refreshes'), 1);
    assert.equal(s.node('#refreshSemantic').disabled, false);
  });
  await test('workspace-wide command authorization is single-flight and keeps delete separate', async () => {
    const s = sandbox();
    s.run('state.access={all_commands_authorized:false,allowed_commands:[]};state.accessLoaded=true;state.authorizations=[{id:"AUTH-c",kind:"command_access",workspace:"A"},{id:"AUTH-r",kind:"risky_execution",workspace:"A"},{id:"AUTH-d",kind:"destructive_delete",workspace:"A"}];renderAccess();');
    const first = s.run('toggleAllCommandsFromUi()');
    const second = s.run('toggleAllCommandsFromUi()');
    await flush();
    assert.equal(s.requests.length, 1);
    assert.equal(s.requests[0].url, '/intelligence/command-trust');
    assert.equal(s.requests[0].options.method, 'POST');
    respond(s.requests[0], {all_commands_authorized:true,allowed_commands:[],available_commands:[]});
    await Promise.all([first, second]);
    assert.equal(s.run('state.access.all_commands_authorized'), true);
    assert.equal(s.run('state.authorizations.length'), 1);
    assert.equal(s.run('state.authorizations[0].kind'), 'destructive_delete');
    assert.ok(s.node('#allCommandsStatus').textContent.includes('All commands authorized'));
  });
  await test('empty workspace mutation response is rejected before switching projects', async () => {
    const s = sandbox();
    s.node('#workspacePath').value = '/fixture/new';
    const pending = s.run('addWorkspaceFromUi()');
    await flush();
    respond(s.requests[0], {workspace: {id: ''}});
    await pending;
    assert.equal(s.requests.length, 1);
    assert.equal(s.node('#workspacePath').value, '/fixture/new');
    assert.equal(s.run('state.current'), 'A');
  });
  await test('malformed command mutation response does not replace access state or clear input', async () => {
    const s = sandbox();
    s.node('#commandCandidate').value = 'cargo';
    const pending = s.run('addCommandFromUi()');
    await flush();
    respond(s.requests[0], {});
    await pending;
    assert.equal(s.run('state.access'), null);
    assert.equal(s.node('#commandCandidate').value, 'cargo');
  });
  await test('malformed access reads fail closed before partial publication', async () => {
    const workspace = {id:'A',root:'/fixture/A',write_enabled:true,exec_enabled:true,all_commands_authorized:false,allowed_commands:[],available_commands:[]};
    const validWorkspace = {workspace,workspace_options:[{id:'A',root:'/fixture/A'}]};
    const variants = [
      {workspace:{...validWorkspace,workspace_options:'bad'},commands:{allowed_commands:[]},authorizations:{pending:[]}},
      {workspace:validWorkspace,commands:{allowed_commands:'bad'},authorizations:{pending:[]}},
      {workspace:validWorkspace,commands:{allowed_commands:[]},authorizations:{pending:[{id:'AUTH',kind:'command_access',workspace:'A'}]}}
    ];
    for (const variant of variants) {
      const s = sandbox();
      const pending = s.run('loadAccess()');
      await flush();
      for (const request of s.requests) {
        if (request.url.endsWith('/workspaces')) respond(request, variant.workspace);
        else if (request.url.endsWith('/commands')) respond(request, variant.commands);
        else respond(request, variant.authorizations);
      }
      assert.equal(await pending, false);
      assert.equal(s.run('state.access'), null);
      assert.equal(s.run('state.workspaceAccess'), null);
      assert.equal(s.run('state.accessLoaded'), false);
      assert.ok(!s.node('#authorizationList').innerHTML.includes('No pending authorizations'));
    }
  });

  const report = {suite: 'release-webui', results};
  fs.mkdirSync(path.join(process.argv[2], 'target'), {recursive: true});
  fs.writeFileSync(path.join(process.argv[2], 'target/wcode-release-webui.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report));
  assert.equal(results.length, 10);
  assert.ok(results.every(item => item.passed), 'release WebUI regressions failed');
}
main().catch(error => {console.error(error); process.exitCode = 1;});