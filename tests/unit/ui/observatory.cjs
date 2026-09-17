'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const root = process.argv[2];
const APP_FILES=['core','access','overview','architecture','engineering','features','quality','structure','runtime'];
function productionBundle(){return APP_FILES.map(file=>fs.readFileSync(path.join(root,'src/ui/intelligence_web/app',file+'.js'),'utf8')).join('');}
class Element {
  constructor(){ this.innerHTML=''; this.textContent=''; this.value=''; this.checked=true; this.disabled=false; this.dataset={}; this.attrs={}; this.events={}; this.classes=new Set(); this.classList={contains:x=>this.classes.has(x),toggle:(x,on)=>on?this.classes.add(x):this.classes.delete(x),add:x=>this.classes.add(x),remove:x=>this.classes.delete(x)}; }
  setAttribute(k,v){this.attrs[k]=v;}
  removeAttribute(k){delete this.attrs[k];}
  addEventListener(k,v){this.events[k]=v;}
  querySelector(){return new Element();}
  querySelectorAll(){return [];}
  focus(){} scrollIntoView(){} closest(){return null;}
}
function stripRuntimeBootstrap(source){
  const normalized=source.replace(/\r\n?/g,'\n');
  const bootstrap='\napplyTheme();\napplyLanguage();\nstartObservatory();';
  const index=normalized.indexOf(bootstrap);
  assert.notEqual(index,-1,'runtime bootstrap marker must stay discoverable in the behavior fixture');
  return normalized.slice(0,index);
}
function sandbox(storageBlocked=false,authenticated=true,options={}){
  const nodes=new Map();
  const node=id=>{if(!nodes.has(id))nodes.set(id,new Element());return nodes.get(id);};
  node('#accessPanel').classes.add('hidden');
  const requests=[],timers=new Map(),events={};let timerId=0;
  const context={console,URL,URLSearchParams,AbortController,Date,Intl,Map,Set,Promise,JSON,Number,String,Math,Error,DOMException,
    location:{hash:authenticated?'#token=test-ui&workspace=A':'#workspace=A'},localStorage:{getItem(){if(storageBlocked)throw new Error('storage denied');return null;},setItem(){if(storageBlocked)throw new Error('storage denied');}},
    document:{hidden:false,documentElement:{dataset:{},classList:{toggle(){}},setAttribute(){}},querySelector:node,querySelectorAll:()=>[],addEventListener:(name,handler)=>{events[name]=handler;},getElementById:id=>node('#'+id)},
    window:{matchMedia:()=>({matches:false,addEventListener(){}}),addEventListener(){}},requestAnimationFrame:fn=>fn(),queueMicrotask,
    setTimeout:(fn,ms)=>{if(options.fakeTimers){timers.set(++timerId,{fn,ms});return timerId;}const timer=setTimeout(fn,ms);timer.unref();return timer;},clearTimeout:id=>options.fakeTimers?timers.delete(id):clearTimeout(id),
    fetch:(url,requestOptions={})=>url==='/healthz'&&!options.controlTunnels
      ? Promise.resolve({ok:true,status:200,json:async()=>({public_endpoint:'pending',tunnels:[]})})
      : new Promise((resolve,reject)=>requests.push({url,options:requestOptions,resolve,reject}))};
  vm.createContext(context);
  for(const file of APP_FILES){
    let source=fs.readFileSync(path.join(root,'src/ui/intelligence_web/app',file+'.js'),'utf8');
    source=file==='runtime'?stripRuntimeBootstrap(source):source.replace(/\r\n?/g,'\n');
    vm.runInContext(source,context,{filename:file+'.js'});
  }
  return {context,nodes,requests,timers,events,run:code=>vm.runInContext(code,context),node};
}
const flush=()=>new Promise(setImmediate);
function respond(request,data,ok=true){request.resolve({ok,status:ok?200:503,json:async()=>data});}
const project=(workspace='A')=>({workspace,project:workspace,root:'/fixture/'+workspace,design_valid:true,workspace_options:[],requirements:[],code:{changed_files:0},proof:{current_evidence:0,acceptance:{total:0,mapped:0,executed:0,passed:0,fresh:0}},convergence:{},coverage:{},architecture:{components:[],dependencies:[],desired_edges:0,observed_edges:0,blocking_drift_edges:0},git_review:{available:false,reason:'execution_disabled'}});
async function run(){
  const results=[];
  async function test(name,fn){try{await fn();results.push({name,passed:true});}catch(error){results.push({name,passed:false,error:error.stack});}}
  await test('production concatenated bundle parses as one script',async()=>{new vm.Script(productionBundle(),{filename:'intelligence-app.js'});});
  await test('runtime fixture strips bootstrap after CRLF checkout',async()=>{const source='function ready(){}\r\napplyTheme();\r\napplyLanguage();\r\nstartObservatory();';assert.equal(stripRuntimeBootstrap(source),'function ready(){}');});
  await test('storage denial cannot blank the dashboard',async()=>{const s=sandbox(true);assert.ok(s.run('state.language'));});
  await test('missing proof and unavailable Git are not green success',async()=>{const s=sandbox();s.context.fixture=project();s.run('state.project=fixture;renderStats();renderAttention();');assert.ok(!s.node('#attention').innerHTML.includes('attention-item good'),'no evidence must not produce all-clear');assert.ok(!s.node('#stats').innerHTML.includes('>clean<'),'unavailable review must not look clean');assert.equal(s.run('architectureData().evidence_coverage_percent'),0,'empty architecture evidence must remain zero rather than a fake 100%');});
  await test('failed refresh does not acknowledge the new revision',async()=>{const s=sandbox();s.context.fixture=project();s.run('state.project=fixture;state.revisionKey="old|graph|proof|";renderProject=()=>{};renderAttention=()=>{};');const first=s.run('pollRevision()');await flush();respond(s.requests[0],{workspace:'A',fingerprint:'new',graph_revision:'graph',proof_revision:'proof',pending_authorizations:0});await flush();const req=s.requests.find(r=>r.url==='/intelligence/project');assert.ok(req);respond(req,{error:'offline'},false);await first;assert.equal(s.run('state.revisionKey'),'old|graph|proof|');const second=s.run('pollRevision()');await flush();respond(s.requests.at(-1),{workspace:'A',fingerprint:'new',graph_revision:'graph',proof_revision:'proof'});await flush();assert.equal(s.requests.at(-1).url,'/intelligence/project');respond(s.requests.at(-1),project());await second;});
  await test('out-of-order workspace response never replaces the selected project',async()=>{const s=sandbox();s.run('renderProject=()=>{};renderAttention=()=>{};');const a=s.run('refreshProject({workspace:"A",reason:"manual"})');await flush();const b=s.run('refreshProject({workspace:"B",reason:"manual"})');await flush();for(const req of s.requests.filter(r=>r.url==='/intelligence/revision'))respond(req,{workspace:req.options.headers['X-Wcode-Workspace'],fingerprint:'r'});await flush();const pending=s.requests.filter(r=>r.url==='/intelligence/project');for(const req of pending.filter(r=>r.options.headers['X-Wcode-Workspace']==='B'))respond(req,project('B'));await flush();for(const req of pending.filter(r=>r.options.headers['X-Wcode-Workspace']==='A'))respond(req,project('A'));await Promise.all([a,b]);assert.equal(s.run('state.current'),'B');assert.equal(s.run('state.project.workspace'),'B');});
  await test('old access results do not cross workspace boundaries',async()=>{const s=sandbox();const access=s.run('loadAccess()');await flush();s.run('state.current="B";state.accessLoaded=false;');for(const req of s.requests)respond(req,req.url.endsWith('authorizations')?{pending:[{id:'AUTH-A',workspace:'A'}]}:{allowed_commands:['private-A']});await access;assert.equal(s.run('state.accessLoaded'),false);assert.equal(s.run('state.authorizations.length'),0);});
  await test('component with no observed dependencies is not marked aligned',async()=>{const s=sandbox();assert.notEqual(s.run('architectureNodeTone({id:"empty",changed:false},[])'),'aligned');});
  await test('quoted operation arguments are not silently split',async()=>{const s=sandbox();assert.deepEqual(JSON.parse(s.run('JSON.stringify(parseOperationArgs(\'["commit","-m","two words",""]\'))')),['commit','-m','two words','']);assert.throws(()=>s.run('parseOperationArgs(\'commit -m "two words"\')'));});
  await test('requirement filtering also changes its selected detail',async()=>{const s=sandbox();s.context.fixture={...project(),requirements:[{id:'first',title:'First',intent:'',components:[],convergence:'stable'},{id:'second',title:'Second',intent:'',components:[],convergence:'incomplete'}]};s.run('state.project=fixture;state.selected="first";state.filter="incomplete";renderRequirements();');assert.equal(s.run('state.selected'),'second');});
  await test('graph metadata signal takes precedence over the compatible graph revision id',async()=>{const s=sandbox();assert.notEqual(s.run('revisionKey({fingerprint:"same",graph_revision:"GRAPH-same",graph_signal:"before",proof_revision:"same"})'),s.run('revisionKey({fingerprint:"same",graph_revision:"GRAPH-same",graph_signal:"after",proof_revision:"same"})'));});
  await test('evidence-only changes have a different revision key',async()=>{const s=sandbox();assert.notEqual(s.run('revisionKey({fingerprint:"same",graph_revision:"same",proof_revision:"before"})'),s.run('revisionKey({fingerprint:"same",graph_revision:"same",proof_revision:"after"})'));});
  await test('engineering-journal-only changes have a different revision key',async()=>{const s=sandbox();assert.notEqual(s.run('revisionKey({fingerprint:"same",graph_revision:"same",proof_revision:"same",engineering_revision:"before"})'),s.run('revisionKey({fingerprint:"same",graph_revision:"same",proof_revision:"same",engineering_revision:"after"})'));});
  await test('an obsolete failed response cannot make a newer snapshot stale',async()=>{const s=sandbox();s.run('renderProject=()=>{};renderAttention=()=>{};');const first=s.run('refreshProject({reason:"manual",revision:{fingerprint:"first"}})');await flush();const second=s.run('refreshProject({reason:"manual",revision:{fingerprint:"second"}})');await flush();respond(s.requests[1],project());await second;respond(s.requests[0],{error:'old failure'},false);await first;assert.equal(s.run('state.syncError'),false);assert.ok(s.run('state.revisionKey').startsWith('second|'));});
  await test('missing UI credentials never send protected requests',async()=>{const s=sandbox(false,false);await assert.rejects(s.run('uiJson("/intelligence/project","GET",undefined,{workspace:"A"})'),/authorize access/);assert.equal(s.requests.length,0);});
  await test('approval double-click emits only one mutation and a failed reread never replays it',async()=>{const s=sandbox();s.run('state.accessLoaded=true;');const first=s.run('decideAuthorization("AUTH-test",true)');const second=s.run('decideAuthorization("AUTH-test",true)');await flush();assert.equal(s.requests.length,1);respond(s.requests[0],{pending:[],request:{status:'approved'}});await flush();assert.equal(s.requests[1].options.method,'GET');respond(s.requests[1],{error:'read failed'},false);await Promise.all([first,second]);assert.equal(s.requests.filter(r=>r.options.method==='POST').length,1);assert.ok(s.node('#authorizationMessage').textContent.includes('Authorization approved'));assert.equal(s.run('state.accessBusy'),false);});
  await test('activity output escapes task labels and unknown telemetry is not idle',async()=>{const s=sandbox();s.run('renderActivity()');assert.ok(s.node('#activity').innerHTML.includes('unavailable'));s.context.fixture={workspace:'A',activity:{available:true,recent:[{id:1,tool:'<img src=x>',status:'running',slot_counted:true,wait_ms:9,run_ms:11}],completed:0,failed:0}};s.run('state.activitySnapshot=fixture;renderActivity();');assert.ok(s.node('#activity').innerHTML.includes('&lt;img src=x&gt;'));assert.ok(!s.node('#activity').innerHTML.includes('<img src=x>'));});
  await test('an independently observed revision cannot certify an earlier project snapshot',async()=>{
    const s=sandbox(false,true,{fakeTimers:true});s.run('renderProject=()=>{};renderAttention=()=>{};');
    const refresh=s.run('refreshProject({reason:"manual"})');await flush();
    const speculative=s.requests.find(request=>request.url==='/intelligence/revision');
    respond(s.requests.find(request=>request.url==='/intelligence/project'),project());await refresh;
    assert.equal(s.run('state.revisionKey'),null);
    if(speculative){respond(speculative,{workspace:'A',fingerprint:'newer-than-snapshot'});await flush();}
    assert.equal(s.run('state.revisionKey'),null,'a separately fetched revision is not snapshot provenance');
    const poll=s.run('pollRevision()');await flush();respond(s.requests.at(-1),{workspace:'A',fingerprint:'newer-than-snapshot'});await flush();
    assert.equal(s.requests.at(-1).url,'/intelligence/project');respond(s.requests.at(-1),project());await poll;
    assert.equal(s.run('state.revisionKey'),'newer-than-snapshot|||');
  });
  await test('obsolete cached-refresh callbacks cannot abort a newer manual refresh',async()=>{
    for(const response of [{workspace:'A',snapshot_pending:true},{...project(),snapshot_cache:'stale-while-revalidate'}]){
      const s=sandbox(false,true,{fakeTimers:true});s.run('renderProject=()=>{};renderAttention=()=>{};');
      const old=s.run('refreshProject({reason:"manual",revision:{fingerprint:"old"}})');await flush();respond(s.requests[0],response);await old;
      const deferred=[...s.timers.values()].find(timer=>timer.ms===0);assert.ok(deferred);
      const current=s.run('refreshProject({reason:"manual",revision:{fingerprint:"current"}})');await flush();
      const request=s.requests.at(-1),count=s.requests.length;deferred.fn();await flush();
      assert.equal(s.requests.length,count,'old callback must not start another project request');
      assert.equal(request.options.signal.aborted,false);respond(request,project());await current;
      assert.equal(s.run('state.revisionKey'),'current|||');
    }
  });
  await test('hidden pages do not start delayed snapshot rebuilds',async()=>{
    const s=sandbox(false,true,{fakeTimers:true});s.run('renderProject=()=>{};renderAttention=()=>{};');
    const old=s.run('refreshProject({reason:"manual",revision:{fingerprint:"old"}})');await flush();respond(s.requests[0],{workspace:'A',snapshot_pending:true});await old;
    const deferred=[...s.timers.values()].find(timer=>timer.ms===0);assert.ok(deferred);
    s.context.document.hidden=true;deferred.fn();await flush();assert.equal(s.requests.length,1);
  });
  await test('paused auto refresh cannot start deferred snapshot rebuilds',async()=>{
    for(const response of [{workspace:'A',snapshot_pending:true},{...project(),snapshot_cache:'stale-while-revalidate'}]){
      const s=sandbox(false,true,{fakeTimers:true});s.run('renderProject=()=>{};renderAttention=()=>{};');
      const refresh=s.run('refreshProject({reason:"auto",preferCached:true})');await flush();
      respond(s.requests[0],response);await refresh;
      const deferred=[...s.timers.values()].find(timer=>timer.ms===0);assert.ok(deferred);
      s.run('state.autoRefresh=false;');deferred.fn();await flush();
      assert.equal(s.requests.length,1,'pausing auto refresh must suppress its queued rebuild');
    }
  });
  await test('explicit refresh completes a cached response even while auto refresh is paused',async()=>{
    for(const reason of ['manual','initial']){
      const s=sandbox(false,true,{fakeTimers:true});s.run('state.autoRefresh=false;renderProject=()=>{};renderAttention=()=>{};');
      s.context.refreshReason=reason;
      const refresh=s.run('refreshProject({reason:refreshReason,preferCached:true})');await flush();
      respond(s.requests[0],{...project(),snapshot_cache:'stale-while-revalidate'});await refresh;
      const deferred=[...s.timers.values()].find(timer=>timer.ms===0);assert.ok(deferred);
      deferred.fn();await flush();assert.equal(s.requests.length,2);
      respond(s.requests[1],project());await flush();assert.equal(s.run('state.inFlight'),false);
    }
  });
  await test('an omitted workspace resolves before deferred snapshot rebuilds',async()=>{
    for(const response of [{workspace:'A',snapshot_pending:true},{...project(),snapshot_cache:'stale-while-revalidate'}]){
      const s=sandbox(false,true,{fakeTimers:true});s.run('state.current="";renderProject=()=>{};renderAttention=()=>{};');
      const refresh=s.run('refreshProject({reason:"initial",preferCached:true})');await flush();
      respond(s.requests[0],response);await refresh;
      assert.equal(s.run('state.current'),'A');
      const epoch=s.run('state.workspaceEpoch'),deferred=[...s.timers.values()].find(timer=>timer.ms===0);assert.ok(deferred);
      deferred.fn();await flush();assert.equal(s.requests.length,2);
      assert.equal(s.requests[1].options.headers['X-Wcode-Workspace'],'A');
      assert.equal(s.run('state.workspaceEpoch'),epoch,'resolving the default must not switch back to an empty workspace');
      respond(s.requests[1],project());await flush();assert.equal(s.run('state.project.workspace'),'A');
    }
  });
  await test('a deferred default-workspace rebuild cannot follow a later project selection',async()=>{
    const s=sandbox(false,true,{fakeTimers:true});s.run('state.current="";renderProject=()=>{};renderAttention=()=>{};');
    const refresh=s.run('refreshProject({reason:"initial",preferCached:true})');await flush();
    respond(s.requests[0],{...project(),snapshot_cache:'stale-while-revalidate'});await refresh;
    const deferred=[...s.timers.values()].find(timer=>timer.ms===0);assert.ok(deferred);
    const current=s.run('refreshProject({workspace:"B",reason:"manual"})');await flush();
    const request=s.requests.at(-1),count=s.requests.length;deferred.fn();await flush();
    assert.equal(s.requests.length,count);assert.equal(request.options.signal.aborted,false);
    respond(request,project('B'));await current;assert.equal(s.run('state.project.workspace'),'B');
  });
  await test('manual refresh preserves an earlier safe revision baseline without an extra probe',async()=>{
    const s=sandbox(false,true,{fakeTimers:true});s.run('state.revisionKey="known|||";renderProject=()=>{};renderAttention=()=>{};');
    const refresh=s.run('refreshProject({reason:"manual"})');await flush();
    assert.equal(s.requests.length,1);assert.equal(s.requests[0].url,'/intelligence/project');respond(s.requests[0],project());await refresh;
    assert.equal(s.run('state.revisionKey'),'known|||');
  });
  await test('a stale cache response cannot acknowledge a supplied current revision',async()=>{
    const s=sandbox(false,true,{fakeTimers:true});s.run('renderProject=()=>{};renderAttention=()=>{};');
    const refresh=s.run('refreshProject({reason:"manual",revision:{fingerprint:"new"}})');await flush();
    respond(s.requests[0],{...project(),snapshot_cache:'stale-while-revalidate'});await refresh;
    assert.equal(s.run('state.revisionKey'),null);assert.ok([...s.timers.values()].some(timer=>timer.ms===0));
  });
  await test('language quality coverage requires a runnable provider',async()=>{
    const s=sandbox();s.context.language={providers:[
      {id:'blocked',capability:'lint',covers:[],declared:true,available:true,runnable:false,check_only:true},
      {id:'script',capability:'test',covers:[],declared:true,available:true,runnable:true,check_only:false}
    ]};
    assert.equal(s.run('qualityProviders(language,"lint").length'),0);
    assert.ok(s.run('qualityCell(language,"lint")').includes('not runnable'));
    assert.ok(s.run('qualityCell(language,"test")').includes('discovery only'));
    s.run('language.providers[0].runnable=true;language.providers[0].external_advisory_data=true;');
    assert.equal(s.run('qualityProviders(language,"lint").length'),1);
    const covered=s.run('qualityCell(language,"lint")');
    assert.ok(covered.includes('covered'));assert.ok(covered.includes('advisory data'));
  });
  const healthyTunnel = (provider='fixture') => ({public_url_healthy:true,public_endpoint:'ready',tunnels:[{provider,role:'active',state:'verified',url:'https://example.test'}]});
  await test('every active tunnel is a dashboard link and it preserves fragment credentials',async()=>{
    const s=sandbox(false,true,{fakeTimers:true,controlTunnels:true});
    const request=s.run('refreshTunnels()');await flush();
    respond(s.requests[0],{public_url_healthy:true,public_endpoint:'ready',tunnels:[
      {provider:'primary',role:'active',state:'healthy',url:'https://primary.example'},
      {provider:'standby',role:'active',state:'verified',url:'https://standby.example'}
    ]});await request;
    const html=s.node('#tunnels').innerHTML;
    assert.ok(html.includes('href="https://primary.example/intelligence#token=test-ui&amp;workspace=A"'));
    assert.ok(html.includes('standby · active'));
    assert.ok(html.includes('href="https://standby.example'));
    assert.ok(html.includes('title="https://standby.example · active · verified'));
  });
  await test('tunnel requests time out and allow a later retry',async()=>{
    const s=sandbox(false,true,{fakeTimers:true,controlTunnels:true});
    const first=s.run('refreshTunnels()');await flush();
    const request=s.requests[0],deadline=[...s.timers.values()].find(timer=>timer.ms===20000);
    assert.ok(deadline,'health requests need a bounded deadline');
    request.options.signal.addEventListener('abort',()=>request.reject(new DOMException('aborted','AbortError')),{once:true});
    deadline.fn();await first;
    assert.equal(s.run('state.tunnelBusy'),false);assert.equal(s.run('state.tunnelSnapshot'),null);
    assert.ok(s.node('#tunnels').innerHTML.includes('unavailable'));
    const next=s.run('refreshTunnels()');await flush();respond(s.requests[1],healthyTunnel());await next;
    assert.equal(s.run('state.tunnelSnapshot.public_url_healthy'),true);assert.equal(s.timers.size,0);
    assert.equal(s.requests[1].options.cache,'no-store');
  });
  await test('old tunnel completion cannot overwrite or unlock a newer request',async()=>{
    const s=sandbox(false,true,{fakeTimers:true,controlTunnels:true});
    const old=s.run('refreshTunnels()');await flush();
    s.run('state.current="B";clearWorkspaceView({preserveDom:true});');
    const current=s.run('refreshTunnels()');await flush();
    assert.equal(s.requests[0].options.signal.aborted,true);
    respond(s.requests[0],healthyTunnel('obsolete'));await old;
    assert.equal(s.run('state.tunnelBusy'),true);assert.equal(s.run('state.tunnelSnapshot'),null);
    await s.run('refreshTunnels()');assert.equal(s.requests.length,2);
    respond(s.requests[1],healthyTunnel('current'));await current;
    assert.equal(s.run('state.tunnelSnapshot.tunnels[0].provider'),'current');assert.equal(s.timers.size,0);
  });
  await test('hidden tabs cancel tunnel requests and their deadlines',async()=>{
    const s=sandbox(false,true,{fakeTimers:true,controlTunnels:true});
    const request=s.run('refreshTunnels()');await flush();
    s.context.document.hidden=true;s.events.visibilitychange();
    assert.equal(s.requests[0].options.signal.aborted,true);assert.equal(s.timers.size,0);
    respond(s.requests[0],healthyTunnel('hidden'));await request;
    assert.equal(s.run('state.tunnelSnapshot'),null);assert.equal(s.run('state.tunnelBusy'),false);
    await s.run('refreshTunnels()');assert.equal(s.requests.length,1);
  });
  await test('failed tunnel refresh clears stale healthy telemetry',async()=>{
    const s=sandbox(false,true,{fakeTimers:true,controlTunnels:true});s.context.fixture=project();s.context.health=healthyTunnel('obsolete');
    s.run('state.project=fixture;state.tunnelSnapshot=health;renderRuntimeTopology();');
    const request=s.run('refreshTunnels()');await flush();respond(s.requests[0],{error:'offline'},false);await request;
    assert.equal(s.run('state.tunnelSnapshot'),null);assert.ok(s.node('#tunnels').innerHTML.includes('unavailable'));
    assert.ok(s.node('#runtimeTopology').innerHTML.includes('telemetry unavailable'));
    assert.ok(!s.node('#runtimeTopology').innerHTML.includes('obsolete'));
  });
  await test('malformed tunnel payloads are not published as current state',async()=>{
    for(const payload of [null,[],{tunnels:'not-an-array'},{tunnels:[null]}]){
      const s=sandbox(false,true,{fakeTimers:true,controlTunnels:true});
      const request=s.run('refreshTunnels()');await flush();respond(s.requests[0],payload);await request;
      assert.equal(s.run('state.tunnelSnapshot'),null);assert.equal(s.run('state.tunnelBusy'),false);
      assert.ok(s.node('#tunnels').innerHTML.includes('unavailable'));assert.equal(s.timers.size,0);
    }
  });
  await test('tunnel polling stays single-flight while the body is pending',async()=>{
    const s=sandbox(false,true,{fakeTimers:true,controlTunnels:true});
    const first=s.run('refreshTunnels()');await flush();let complete;
    s.requests[0].resolve({ok:true,status:200,json:()=>new Promise(resolve=>{complete=resolve;})});await flush();
    await s.run('refreshTunnels()');assert.equal(s.requests.length,1);
    assert.ok([...s.timers.values()].some(timer=>timer.ms===20000));
    complete(healthyTunnel());await first;assert.equal(s.run('state.tunnelBusy'),false);assert.equal(s.timers.size,0);
  });
  await test('unhealthy endpoints and unknown approval state are not green',async()=>{
    const s=sandbox();s.context.fixture=project();s.context.health={...healthyTunnel(),public_url_healthy:false};s.context.health.tunnels[0].state='quarantined';
    s.run('state.project=fixture;state.tunnelSnapshot=health;renderRuntimeTopology();');
    const html=s.node('#runtimeTopology').innerHTML;
    assert.ok(html.includes('runtime-status-card warn"><span>Endpoint'));
    assert.ok(html.includes('runtime-status-card info"><span>OAuth &amp; MCP'));
    s.run('state.tunnelSnapshot.public_url_healthy=true;state.tunnelSnapshot.tunnels[0].state="verified";renderRuntimeTopology();');
    assert.ok(s.node('#runtimeTopology').innerHTML.includes('runtime-status-card good"><span>Endpoint'));
  });
  await test('slow tunnel requests do not delay activity telemetry',async()=>{
    const s=sandbox(false,true,{fakeTimers:true,controlTunnels:true});
    const tunnel=s.run('refreshTunnels()');const activity=s.run('refreshActivity()');await flush();
    const request=s.requests.find(item=>item.url==='/intelligence/activity');assert.ok(request);
    respond(request,{workspace:'A',pending_authorizations:0,activity:{available:true,active:3,recent:[]}});await activity;
    assert.equal(s.run('state.activitySnapshot.activity.active'),3);assert.equal(s.run('state.tunnelBusy'),true);
    respond(s.requests.find(item=>item.url==='/healthz'),healthyTunnel());await tunnel;
  });
  await test('truncated file snapshots never claim complete line-limit success',async()=>{
    for(const language of ['en','zh-CN']){
      const s=sandbox();s.context.fixture={...project(),structure:{entries:[{path:'a.rs',lines:10,language:'rust'}],largest_files:[],oversized_files:0,truncated:true}};
      s.context.fixtureLanguage=language;
      s.run('state.language=fixtureLanguage;state.project=fixture;renderProjectStructure();');
      const summary=s.node('#structureSummary').innerHTML;
      assert.ok(!summary.includes('pill good'),'partial data cannot certify the repository');
      assert.ok(summary.includes(language==='en'?'No oversized files in snapshot':'当前快照未发现超长文件'));
      assert.ok(summary.includes(language==='en'?'1 files shown':'展示 1 个文件'));
    }
  });
  await test('empty structure snapshots do not claim line-limit success',async()=>{
    const s=sandbox();s.context.fixture={...project(),structure:{entries:[],largest_files:[],oversized_files:0,truncated:false}};
    s.run('state.project=fixture;renderProjectStructure();');
    assert.ok(!s.node('#structureSummary').innerHTML.includes('pill good'));
    assert.ok(s.node('#fileTree').innerHTML.includes('No source files'));
  });
  await test('complete structure snapshots retain real success and generated exemptions',async()=>{
    const s=sandbox();s.context.fixture={...project(),structure:{entries:[{path:'l10n/app_localizations.dart',lines:1200,language:'dart',generated:true,over_limit:false}],largest_files:[{path:'l10n/app_localizations.dart',lines:1200,language:'dart',generated:true,over_limit:false}],line_limit:1000,oversized_files:0,truncated:false}};
    s.run('state.project=fixture;renderProjectStructure();');
    assert.ok(s.node('#structureSummary').innerHTML.includes('pill good'));
    assert.ok(s.node('#fileTree').innerHTML.includes('generated'));
    assert.ok(s.node('#largeFiles').innerHTML.includes('line limit exempt'));
  });
  await test('file filtering reveals nested matches and escapes displayed paths',async()=>{
    const s=sandbox();s.context.fixture={...project(),structure:{entries:[
      {path:'src/deep/Parser.rs',language:'rust',lines:12},
      {path:'src/deep/other.rs',language:'rust',lines:20},
      {path:'src/deep/<parser>.rs',language:'rust',lines:30}
    ]}};
    s.run('state.project=fixture;els.fileSearch.value="PARSER";renderProjectStructure();');
    const html=s.node('#fileTree').innerHTML;
    assert.ok(html.includes('Parser.rs'));assert.ok(!html.includes('other.rs'));
    assert.ok(html.includes('&lt;parser&gt;.rs'));assert.ok(!html.includes('<parser>'));
    assert.equal((html.match(/class="tree-directory" open/g)||[]).length,2);
    assert.equal(s.node('#fileSearchStatus').textContent,'2 matching files');
    s.run('els.fileSearch.value="missing";renderProjectStructure();');
    assert.ok(s.node('#fileTree').innerHTML.includes('No matching files.'));
    s.run('clearWorkspaceView();');assert.equal(s.node('#fileSearch').value,'');
    assert.equal(s.node('#fileSearchStatus').textContent,'');
  });
  await test('icon controls retain accessible names across language and theme changes',async()=>{
    const s=sandbox();s.run('state.language="zh-CN";state.theme="light";applyLanguage();');
    for(const id of ['#refresh','#refreshSemantic','#manage','#projectNavigator','#fileSearch']){
      assert.ok(s.node(id).attrs['aria-label'],id+' needs a name when its visual label is hidden');
    }
    assert.ok(s.node('#theme').attrs['aria-label'].includes('浅色'));
    assert.equal(s.node('#autoRefresh').attrs['aria-label'],'自动刷新：开启');
    s.run('state.autoRefresh=false;applyAutoRefreshControl();');
    assert.equal(s.node('#autoRefresh').attrs['aria-label'],'自动刷新：暂停');
  });
  await test('server diagnostics stay in logs instead of access or semantic UI',async()=>{
    const s=sandbox();
    s.context.rawServerError=Object.assign(new Error('HTTP 503 · /Users/private/repository panic backtrace'),{status:503});
    const message=s.run('requestFailureMessage(rawServerError)');
    assert.ok(!message.includes('/Users/private'));
    assert.ok(!message.includes('panic backtrace'));
    assert.ok(message.includes('WCode') || message.includes('日志'));
  });
  await test('initial refresh failure replaces loading with useful guidance without raw diagnostics',async()=>{
    const s=sandbox();s.context.console={...console,warn(){}};
    const refresh=s.run('refreshProject({reason:"manual"})');await flush();
    respond(s.requests[0],{error:'internal_private_path_and_stack'},false);await refresh;
    assert.equal(s.node('#syncState').textContent,'Server error · HTTP 503');
    const html=s.node('#architectureBlueprint').innerHTML;
    assert.ok(html.includes('connection-state'));assert.ok(html.includes('use Refresh'));
    assert.ok(!html.includes('internal_private_path_and_stack'));assert.ok(!html.includes('loading-state'));
    assert.equal(s.node('#refresh').disabled,false);
    assert.equal(s.node('.observatory-main').attrs['aria-busy'],'false');
  });
  const view=sandbox();
  const components=[['Runtime','Task scheduling','Schedule independent work and retain real capacity through cancellation.'],['Runtime','Context retrieval','Locate relevant source and retain exact edit preconditions.'],['Integrations','MCP transports','Serve one tool runtime across local and remote clients.'],['Integrations','Agent setup','Configure supported coding agents without replacing unrelated settings.'],['Workspace','File operations','Read and edit bounded files with SHA-checked atomic writes.'],['Workspace','Command execution','Run approved commands and retain timeout diagnostics.'],['Intelligence','Verification','Keep checks and evidence bound to the code revision.'],['Intelligence','Software graph','Map component relationships with explicit provider precision.']].map(([scope,name,purpose],i)=>({id:'component:'+i,name,product_scopes:[scope],responsibilities:[purpose],implementation_targets:['src/example/module_'+i+'.rs'],implementation_files:3+i,implementation_lines:250+i*50,requirements:[],depends_on:i?['component:0']:[],changed:i===1||i===5,changed_paths:i===1?['src/example/module_1.rs']:[]}));
  view.context.fixture={...project(),project:'wcode',root:'/example/wcode',pending_authorizations:2,git_review:{available:true,reason:'available'},code:{changed_files:12,source_files:246,source_lines:48190,languages:[],product_scopes:[]},proof:{current_evidence:7,current_passed:5,current_failed:2,current_inconclusive:0,current_verification_plans:1,current_verification_ready:0,current_verification_blocked:1,revision_code:'sha256:example-current-code',revision_design:'sha256:example-current-design',acceptance:{total:33,mapped:33,executed:28,passed:26,fresh:21}},architecture:{components,dependencies:components.slice(1).map((c,i)=>({from:c.id,to:'component:0',from_name:c.name,to_name:'Task scheduling',status:i===3?'unverified_actual':'aligned',desired:true,actual:i!==3,precision:'syntax',blocking:false})),desired_edges:7,observed_edges:6,aligned_edges:6,blocking_drift_edges:0,components_with_implementation:8,observed_drift_percent:0,evidence_coverage_percent:85.7,implementation_coverage_percent:100},workspace_options:[{id:'A',root:'/example/wcode'}]};
  view.context.activity={workspace:'A',pending_authorizations:2,activity:{available:true,active:3,queued:4,orchestration:1,completed:124,failed:3,recent:[{id:132,tool:'agent_context',status:'running',slot_counted:true,wait_ms:6,run_ms:173},{id:131,tool:'cargo test',status:'running',slot_counted:true,wait_ms:450,run_ms:12450},{id:130,tool:'find_symbol',status:'queued',slot_counted:true,wait_ms:42},{id:129,tool:'verify_project',status:'failed',slot_counted:false,wait_ms:0,run_ms:21800}]},resources:{limits:{child_queue:{active:2,limit:2,waiting:3},probe_queue:{active:1,limit:4,waiting:0},resident_memory_bytes:224*1048576}}};
  view.run('state.project=fixture;state.activitySnapshot=activity;state.lastUpdated=Date.now();');
  const snapshots={};
  for(const language of ['en','zh-CN']){view.context.fixtureLanguage=language;view.run('state.language=fixtureLanguage;renderProject(true);');snapshots[language]=[...view.nodes].filter(([id])=>id.startsWith('#')).map(([id,node])=>({id:id.slice(1),html:node.innerHTML,text:node.textContent,value:node.value,attrs:node.attrs}));}
  const dictionary=view.run('JSON.stringify(translations)');
  const styles=['theme','shell','features','data','architecture','engineering','structure','responsive'].map(file=>fs.readFileSync(path.join(root,'src/ui/intelligence_web/styles',file+'.css'),'utf8')).join('\n');
  let html=fs.readFileSync(path.join(root,'src/ui/intelligence_web/page.html'),'utf8').replace('<link rel="stylesheet" href="/intelligence/app.css">','<style>'+styles+'</style>').replace('<script defer src="/intelligence/app.js"></script>','');
  const script='const snapshots='+JSON.stringify(snapshots)+',translations='+dictionary+';window.preview=(language,theme)=>{document.documentElement.lang=language;document.documentElement.dataset.theme=theme;for(const item of snapshots[language]){const node=document.getElementById(item.id);if(!node)continue;if(item.html)node.innerHTML=item.html;else if(item.text)node.textContent=item.text;for(const [k,v] of Object.entries(item.attrs))node.setAttribute(k,v);}document.querySelectorAll("[data-i18n]").forEach(node=>node.textContent=translations[language]?.[node.dataset.i18n]||node.dataset.i18n);const languageNode=document.querySelector("#language strong");if(languageNode)languageNode.textContent=language==="zh-CN"?"EN":"中";const themeNode=document.getElementById("theme");if(themeNode){themeNode.dataset.themeState=theme;themeNode.setAttribute("aria-pressed",String(theme!=="system"));}document.getElementById("syncState").textContent=language==="en"?"Example data · layout fixture":"示例数据 · 布局验收";document.getElementById("syncDot").className="sync-dot ok";};preview("en","light");';
  html=html.replace('</body>','<script>'+script+'</script></body>');
  fs.mkdirSync(path.join(root,'target'),{recursive:true});
  // Export actual renderer HTML and shipped CSS for offline layout inspection.
  // The fixture has example data, no credential and no network-capable script.
  let staticHtml=html.replace(/<script>[\s\S]*?<\/script>/g,'');
  const visible=new Set(['stats','statusSummary','attention','componentCards','componentInspector','activity','resourceStatus','proofSummary','componentCount','precisionBadge','lastUpdated']);
  for(const item of snapshots.en.filter(item=>visible.has(item.id))){
    const pattern=new RegExp('(<([a-z]+)[^>]*\\bid="'+item.id+'"[^>]*>)[\\s\\S]*?(</\\2>)');
    staticHtml=staticHtml.replace(pattern,(_,open,tag,close)=>open+(item.html||item.text)+close);
  }
  staticHtml=staticHtml.replace('>Connecting</strong>','>Example data · layout fixture</strong>');
  fs.writeFileSync(path.join(root,'target/wcode-observatory-preview.txt'),require('node:zlib').gzipSync(staticHtml).toString('base64'));
  const report={suite:'observatory-behavior',results};
  fs.mkdirSync(path.join(root,'target'),{recursive:true});
  fs.writeFileSync(path.join(root,'target/wcode-observatory-behavior.json'),JSON.stringify(report,null,2));
  console.log(JSON.stringify(report,null,2));
  assert.ok(results.every(r=>r.passed),results.filter(r=>!r.passed).map(r=>r.name).join('\n'));
}
if(require.main===module)run().catch(error=>{console.error(error);process.exitCode=1;});
module.exports={sandbox,project,respond,flush,stripRuntimeBootstrap};
