'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const root = process.argv[2] || path.resolve(__dirname, '../../..');
const manifest=fs.readFileSync(path.join(root,'src/ui/intelligence_web.rs'),'utf8');
function manifestFiles(constant,folder,extension){
  const start=`pub(crate) const ${constant}: &str = concat!(`,from=manifest.indexOf(start);
  assert.ok(from>=0,`missing ${constant} manifest`);
  const to=manifest.indexOf('\n);',from);assert.ok(to>from,`unterminated ${constant} manifest`);
  const files=[...manifest.slice(from,to).matchAll(/include_str!\("([^"]+)"\)/g)]
    .map(match=>match[1]).filter(file=>file.startsWith(folder+'/')&&file.endsWith(extension))
    .map(file=>file.slice((folder+'/').length,-extension.length));
  assert.ok(files.length>0,`empty ${constant} manifest`);return files;
}
const APP_FILES=manifestFiles('INTELLIGENCE_JS','intelligence_web/app','.js');
const STYLE_FILES=manifestFiles('INTELLIGENCE_CSS','intelligence_web/styles','.css');
function productionBundle(){return APP_FILES.map(file=>fs.readFileSync(path.join(root,'src/ui/intelligence_web/app',file+'.js'),'utf8')).join('');}
class Element {
  constructor(){ this.innerHTML=''; this.textContent=''; this.value=''; this.checked=true; this.disabled=false; this.dataset={}; this.attrs={}; this.events={}; this.classes=new Set(); this.classList={contains:x=>this.classes.has(x),toggle:(x,on)=>on?this.classes.add(x):this.classes.delete(x),add:x=>this.classes.add(x),remove:x=>this.classes.delete(x)}; }
  setAttribute(k,v){this.attrs[k]=v;}
  removeAttribute(k){delete this.attrs[k];}
  getAttribute(k){return this.attrs[k]??null;}
  addEventListener(k,v){this.events[k]=v;}
  querySelector(){return new Element();}
  querySelectorAll(){return [];}
  focus(){} blur(){} scrollIntoView(){} closest(){return null;}
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
  const requests=[],timers=new Map(),events={},listeners=new Map();let timerId=0;
  const storage={...(options.storage||{})};
  const languages=options.languages||['en-US'];
  const context={console,URL,URLSearchParams,AbortController,Date,Intl,Map,Set,Promise,JSON,Number,String,Math,Error,DOMException,
    navigator:{languages,language:languages[0]||'en-US'},
    location:{hash:authenticated?'#token=test-ui&workspace=A':'#workspace=A'},localStorage:{getItem(key){if(storageBlocked)throw new Error('storage denied');return storage[key]??null;},setItem(key,value){if(storageBlocked)throw new Error('storage denied');storage[key]=String(value);}},
    document:{hidden:false,documentElement:{dataset:{},classList:{toggle(){}},setAttribute(){}},querySelector:node,querySelectorAll:()=>[],addEventListener:(name,handler)=>{const callbacks=listeners.get(name)||[];callbacks.push(handler);listeners.set(name,callbacks);events[name]=event=>{for(const callback of callbacks)callback(event);};},getElementById:id=>node('#'+id)},
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
function navigatorFixture(s){
  const search=s.node('#projectNavigator'),results=s.node('#navigatorResults');let html='',buttons=[];
  Object.defineProperty(results,'innerHTML',{get:()=>html,set:value=>{
    html=value;
    buttons=[...value.matchAll(/<button id="([^"]+)"[^>]*data-nav-index="(\d+)"[^>]*aria-selected="([^"]+)"/g)].map(match=>{
      const button=new Element();button.id=match[1];button.dataset.navIndex=match[2];
      button.setAttribute('aria-selected',match[3]);button.click=()=>button.events.click?.();return button;
    });
  }});
  results.querySelectorAll=()=>buttons;
  s.context.navFixture={...project(),architecture:{components:[
    {id:'one',name:'Engine One',responsibilities:['first']},
    {id:'two',name:'Engine Two',responsibilities:['second']},
  ]}};
  s.context.activated=[];s.run('state.project=navFixture;activateProjectNavigatorItem=item=>activated.push(item.id);');
  search.value='engine';s.run('renderProjectNavigator({open:true});');
  return {search,results,get buttons(){return buttons;}};
}
async function run(){
  const results=[];
  async function test(name,fn){try{await fn();results.push({name,passed:true});}catch(error){results.push({name,passed:false,error:error.stack});}}
  await test('workspace page preferences restore only the seven supported pages',async()=>{
    for(const tab of ['overview','architecture','activity','proof','changes','requirements','files']){
      const s=sandbox(false,true,{storage:{'wcode.ui.page':tab}});
      assert.equal(s.run('state.workspaceTab'),tab);
      s.run('activateWorkspaceTab("files");');
      assert.equal(s.run('readPreference("wcode.ui.page")'),'files');
    }
    assert.equal(sandbox(false,true,{storage:{'wcode.ui.page':'<script>unknown</script>'}}).run('state.workspaceTab'),'overview');
    const denied=sandbox(true);denied.run('activateWorkspaceTab("changes");');assert.equal(denied.run('state.workspaceTab'),'changes');
  });
  await test('workspace identity and browser title follow selection without retaining another root or revision',async()=>{
    const s=sandbox();s.context.fixture={...project(),project:'<script>Project</script>',root:'/private/project',proof:{revision_code:'sha256:current-source'}};
    s.run('state.project=fixture;activateWorkspaceTab("files");');
    assert.equal(s.node('#workspaceName').textContent,'<script>Project</script>');
    assert.equal(s.node('#workspaceRoot').textContent,'/private/project');
    assert.match(s.context.document.title,/Project files.*<script>Project<\/script>/);
    s.run('state.current="B";clearWorkspaceView();');
    assert.equal(s.node('#workspaceName').textContent,'B');
    assert.equal(s.node('#workspaceRoot').textContent,'');assert.equal(s.node('#workspaceRevision').textContent,'');
  });
  await test('sync failures remain visible in workspace context and language changes refresh the page heading',async()=>{
    const s=sandbox();s.run('activateWorkspaceTab("proof");setSync("error","Snapshot unavailable");');
    assert.equal(s.node('#snapshotState').textContent,'Snapshot unavailable');assert.equal(s.node('#snapshotState').dataset.state,'error');
    s.run('state.language="zh-CN";applyLanguage();');assert.equal(s.node('#workspaceTitle').textContent,'验证证据');
  });
  await test('model ownership is unknown for older snapshots and remains visible without Execution',async()=>{
    const s=sandbox();s.context.fixture={...project(),execution:{available:true,exists:false}};s.run('state.project=fixture;renderExecutionStatus();');
    assert.match(s.node('#executionStatus').innerHTML,/Ownership state is unknown/);assert.doesNotMatch(s.node('#executionStatus').innerHTML,/Unclaimed|No items in the observed Worklist/);
    s.context.fixture.execution.worklist={available:true,exists:true,revision:2,items:[{id:'lane',title:'Source lane',write_paths:['src/a.rs'],claim:{actor:'worker',base_revision:1,claimed_at_ms:1,expires_at_ms:Date.now()+60000,expired:false}}]};
    s.run('state.project=fixture;renderExecutionStatus();');assert.match(s.node('#executionStatus').innerHTML,/Source lane/);assert.match(s.node('#executionStatus').innerHTML,/Claimed/);
  });
  await test('worker ownership uses public scopes and labels reports independently from verification',async()=>{
    const s=sandbox(),secret='CLAIM_PRIVATE_SECRET';s.context.fixture={...project(),execution:{available:true,exists:true,phase:'executing',checkpoint:{},worklist:{available:true,exists:true,revision:8,claim_id:secret,items:[
      {id:'source',title:'<script>source</script>',write_paths:['src/a.rs','src/b.rs'],claim:{actor:'<worker>',base_revision:6,claimed_at_ms:1,expires_at_ms:Date.now()+60000,expired:false,claim_id:secret},result:{id:'result',actor:'reviewer',outcome:'complete',summary:'<script>reported</script>',reported_at_ms:2,repository_revision:{code:'code'},evidence:[{id:'E1'}],proof_status:'current_references_only',claim_id:secret}}
    ]}}};s.run('state.project=fixture;renderExecutionStatus();');
    const html=s.node('#executionStatus').innerHTML;assert.match(html,/&lt;worker&gt;/);assert.match(html,/src\/a.rs.*src\/b.rs/);assert.match(html,/Worker reports complete/);assert.match(html,/independent verification/);assert.match(html,/1 Evidence references/);
    assert.doesNotMatch(html,/<script>|CLAIM_PRIVATE_SECRET|claim_id|current_references_only/);
  });
  await test('worker ownership is bounded and keeps expired claims and stale snapshots explicit',async()=>{
    const s=sandbox();s.context.fixture={...project(),execution:{available:true,exists:false,worklist:{available:true,exists:true,revision:9,items:Array.from({length:12},(_,i)=>({id:'lane-'+i,title:'Lane '+i,write_paths:[],claim:{actor:'worker-'+i,base_revision:1,claimed_at_ms:1,expires_at_ms:1,expired:true}}))}}};
    s.run('state.project=fixture;state.snapshotStale=true;renderExecutionStatus();');const html=s.node('#executionStatus').innerHTML;
    assert.equal((html.match(/class="execution-worker"/g)||[]).length,8);assert.match(html,/Lease expired/);assert.match(html,/Bounded ownership view.*8 \/ 12/);assert.match(html,/Snapshot stale/);assert.match(html,/Read-only lane/);assert.doesNotMatch(html,/Lane 11/);
  });
  await test('malformed worker projections fail closed while known empty Worklists stay explicit',async()=>{
    const s=sandbox();for(const worklist of [{available:false},{available:true,exists:true,items:'bad'},{available:true,exists:true,items:[null]}]){
      s.context.fixture={...project(),execution:{available:true,exists:false,worklist}};s.run('state.project=fixture;renderExecutionStatus();');assert.match(s.node('#executionStatus').innerHTML,/Ownership state is unknown/);
    }
    s.context.fixture.execution.worklist={available:true,exists:false,items:[]};s.run('state.project=fixture;renderExecutionStatus();');assert.match(s.node('#executionStatus').innerHTML,/No durable Worklist has been created/);
    s.context.fixture.execution.worklist={available:true,exists:true,revision:1,items:[]};s.run('state.project=fixture;renderExecutionStatus();');assert.match(s.node('#executionStatus').innerHTML,/No items in the observed Worklist/);assert.doesNotMatch(s.node('#executionStatus').innerHTML,/Ownership state is unknown/);
  });
  await test('Worklist revision keys distinguish known state from unknown without changing legacy signals',async()=>{
    const s=sandbox();assert.equal(s.run('revisionKey({fingerprint:"same"})'),'same|||');
    const key=signal=>{s.context.signal=signal;return s.run('revisionKey({fingerprint:"same",worklist_revision:signal})');};
    assert.equal(key({available:true,exists:true,revision:8}),'same||||worklist:1:8');
    assert.notEqual(key({available:true,exists:true,revision:8}),key({available:true,exists:true,revision:9}));
    assert.equal(key({available:true,exists:false,revision:8}),'same||||worklist:0:0');
    for(const unknown of [null,{available:false},{available:true,exists:true,revision:-1},{available:true,exists:true,revision:"8"}]){
      assert.equal(key(unknown),'same||||worklist:unknown');assert.notEqual(key(unknown),key({available:true,exists:false,revision:0}));
    }
  });
  await test('Worklist-only changes refresh live ownership once while unchanged cached snapshots remain cheap',async()=>{
    const s=sandbox(false,true,{fakeTimers:true});const signal={workspace:'A',fingerprint:'same',graph_signal:'graph',proof_revision:'proof',engineering_revision:'journal',worklist_revision:{available:true,exists:true,revision:8}};
    s.context.signal=signal;s.context.fixture={...project(),execution:{available:true,exists:false,worklist:{available:true,exists:true,revision:8,items:[]}}};
    s.run('state.project=fixture;state.revisionKey=revisionKey(signal);renderProject=()=>{};renderAttention=()=>{};');
    signal.worklist_revision.revision=9;const first=s.run('pollRevision()');await flush();respond(s.requests[0],signal);await flush();
    assert.equal(s.requests[1].url,'/intelligence/project');respond(s.requests[1],{...project(),snapshot_cache:'cached',snapshot_revision:'same|graph|proof|journal|worklist:1:9',execution:{available:true,exists:false,worklist:{available:true,exists:true,revision:9,items:[{id:'new',title:'Claimed lane',write_paths:[]}]}}});await first;
    assert.equal(s.run('state.project.execution.worklist.revision'),9);assert.equal(s.run('state.revisionKey'),'same|graph|proof|journal|worklist:1:9');
    const second=s.run('pollRevision()');await flush();respond(s.requests.at(-1),signal);await second;
    assert.equal(s.requests.filter(request=>request.url==='/intelligence/project').length,1,'the cache base key must not cause repeated heavy refreshes');
    signal.worklist_revision={available:true,exists:false,revision:0};const removed=s.run('pollRevision()');await flush();respond(s.requests.at(-1),signal);await flush();
    assert.equal(s.requests.at(-1).url,'/intelligence/project');respond(s.requests.at(-1),{...project(),snapshot_cache:'cached',snapshot_revision:'same|graph|proof|journal|worklist:0:0',execution:{available:true,exists:false,worklist:{available:true,exists:false,revision:0,items:[]}}});await removed;
    assert.equal(s.run('state.project.execution.worklist.exists'),false);
  });
  await test('an unavailable Worklist preserves same-workspace observations only as historical ownership',async()=>{
    const s=sandbox();s.context.fixture={...project(),execution:{available:true,exists:false,worklist:{available:true,exists:true,revision:8,items:[{id:'lane',title:'Historical lane',write_paths:['src/a.rs'],claim:{actor:'worker',expired:false,expires_at_ms:Date.now()+60000}}]}}};
    s.run('state.project=fixture;state.workspaceTab="activity";renderProject=()=>renderExecutionStatus();renderAttention=()=>{};');
    for(let i=0;i<2;i++){
      const refresh=s.run('refreshProject({revision:{fingerprint:"same",worklist_revision:null}})');await flush();
      respond(s.requests.at(-1),{...project(),snapshot_cache:'cached',snapshot_revision:'same||||worklist:unknown',execution:{available:true,exists:false,worklist:{available:false}}});await refresh;
      assert.equal(s.run('state.project.execution.worklist.available'),false);assert.equal(s.run('state.project.execution.worklist.last_known.revision'),8);
      assert.equal(s.run('state.project.execution.worklist.last_known.last_known'),undefined,'historical metadata must not grow on repeated failures');
      const html=s.node('#executionStatus').innerHTML;assert.match(html,/Ownership state is unknown/);assert.match(html,/Historical lane/);assert.match(html,/Historical ownership/);assert.match(html,/Last observed Worklist #8/);assert.doesNotMatch(html,/>Claimed<\/span>/);
    }
    const recovered=s.run('refreshProject({revision:{fingerprint:"same",worklist_revision:{available:true,exists:true,revision:9}}})');await flush();
    respond(s.requests.at(-1),{...project(),execution:{available:true,exists:false,worklist:{available:true,exists:true,revision:9,items:[]}}});await recovered;
    assert.equal(s.run('state.project.execution.worklist.last_known'),undefined);assert.doesNotMatch(s.node('#executionStatus').innerHTML,/Historical lane/);
  });
  await test('historical ownership never crosses a Workspace switch',async()=>{
    const s=sandbox();s.context.fixture={...project(),execution:{available:true,exists:false,worklist:{available:true,exists:true,revision:8,items:[{id:'lane',title:'Private A lane'}]}}};
    s.run('state.project=fixture;state.workspaceTab="activity";renderProject=()=>renderExecutionStatus();renderAttention=()=>{};');
    const refresh=s.run('refreshProject({workspace:"B",reason:"manual"})');await flush();
    respond(s.requests.at(-1),{...project('B'),execution:{available:true,exists:false,worklist:{available:false}}});await refresh;
    assert.equal(s.run('state.current'),'B');assert.equal(s.run('state.project.execution.worklist.available'),false);assert.equal(s.run('state.project.execution.worklist.last_known'),undefined);
    assert.match(s.node('#executionStatus').innerHTML,/Ownership state is unknown/);assert.doesNotMatch(s.node('#executionStatus').innerHTML,/Private A lane/);
  });
  await test('shared attention drives the primary action and keeps urgent signals visible',async()=>{
    const s=sandbox();s.context.fixture={...project(),proof:{revision_code:'code-1',revision_design:'design-1',current_evidence:1},attention:{revision:{code:'code-1',design:'design-1'},total:3,items:[
      {id:'failure',kind:'verification_failure',severity:'high',subject:'Failing check',message:'Current producer failed',provider:'verification',precision:'deterministic',section:'proof'},
      {id:'size',kind:'oversized_source',severity:'high',subject:'src/a.rs',message:'Decompose source',provider:'structure',precision:'source',section:'files',path:'src/a.rs'},
      {id:'coverage',kind:'partial_coverage',severity:'info',subject:'Partial scan',message:'Bounded data',provider:'scan',precision:'syntax',section:'diagnostics'}
    ]}};
    s.context.fixture.attention.items.push({id:'provider',kind:'provider_gap',severity:'medium',subject:'Missing quality provider',message:'Unavailable provider',provider:'language-quality',precision:'none',section:'quality'});s.context.fixture.attention.total=4;
    s.run('state.project=fixture;renderAttention();');
    const summary=s.node('#statusSummary').innerHTML,attention=s.node('#attention').innerHTML;
    assert.match(summary,/Failing check/);assert.match(summary,/data-summary-action="proofSection"/);
    assert.match(attention,/data-summary-path="src\/a.rs"/);assert.match(attention,/structure · source/);assert.match(attention,/data-summary-action="qualitySection"/,'provider gaps must reveal quality');
    assert.ok(attention.indexOf('src/a.rs')<attention.indexOf('<details'),'urgent source issue must be outside collapsed observations');
    assert.match(summary,/code-1/);assert.doesNotMatch(attention,/Working-tree status is unknown/,'shared projection owns repository attention');
  });
  await test('shared attention revision disagreement and stale snapshots demand a real refresh',async()=>{
    const s=sandbox();s.context.fixture={...project(),proof:{revision_code:'current'},attention:{revision:{code:'old'},total:0,items:[],partial:true,partial_reasons:['scan bounded']}};
    s.run('state.project=fixture;state.syncError=true;renderAttention();');
    assert.match(s.node('#statusSummary').innerHTML,/Snapshot is stale/);
    assert.match(s.node('#statusSummary').innerHTML,/data-summary-action="refresh"/);
    assert.match(s.node('#attention').innerHTML,/Attention revision does not match/);
    assert.match(s.node('#attention').innerHTML,/Partial attention coverage/);
    const html=s.node('#observationCoverage').innerHTML;
    assert.equal((html.match(/>Stale<\/span>/g)||[]).length,4);
    assert.match(html,/Unavailable, not idle/);
    s.run('clearWorkspaceView();');assert.equal(s.node('#observationCoverage').innerHTML,'','old observation cards must not cross workspace switches');
  });
  await test('command center includes source changes and requirements navigation',async()=>{
    const s=sandbox();const commands=JSON.parse(s.run('JSON.stringify(commandActions())'));
    for(const command of ['files','changes','requirements'])assert.ok(commands.some(item=>item[0]===command));
    s.run('executeCommand("files");');assert.equal(s.run('state.workspaceTab'),'files');
  });
  await test('production concatenated bundle parses as one script',async()=>{new vm.Script(productionBundle(),{filename:'intelligence-app.js'});});
  await test('runtime fixture strips bootstrap after CRLF checkout',async()=>{const source='function ready(){}\r\napplyTheme();\r\napplyLanguage();\r\nstartObservatory();';assert.equal(stripRuntimeBootstrap(source),'function ready(){}');});
  await test('storage denial cannot blank the dashboard',async()=>{const s=sandbox(true);assert.ok(s.run('state.language'));});
  await test('background refresh stays silent and never commandeers the manual refresh button',async()=>{
    const s=sandbox();s.run('renderProject=()=>{};renderAttention=()=>{};setSync("ok","steady");');
    const refresh=s.run('refreshProject({reason:"auto"})');await flush();
    assert.equal(s.node('#refresh').disabled,false);
    assert.equal(s.node('#syncState').textContent,'steady','auto refresh must not flash loading state');
    respond(s.requests.find(request=>request.url==='/intelligence/project'),project());await refresh;
    assert.equal(s.node('#refresh').disabled,false);
    s.run('setManualRefreshBusy(true)');assert.equal(s.node('#refresh').disabled,true);assert.equal(s.node('#refresh').attrs['aria-busy'],'true');
    s.run('setManualRefreshBusy(false)');assert.equal(s.node('#refresh').disabled,false);assert.equal(s.node('#refresh').attrs['aria-busy'],'false');
  });
  await test('missing proof and unavailable Git are not green success',async()=>{const s=sandbox();s.context.fixture=project();s.run('state.project=fixture;renderStats();renderAttention();');assert.ok(!s.node('#attention').innerHTML.includes('attention-item good'),'no evidence must not produce all-clear');assert.ok(!s.node('#stats').innerHTML.includes('>clean<'),'unavailable review must not look clean');assert.equal(s.run('architectureData().evidence_coverage_percent'),0,'empty architecture evidence must remain zero rather than a fake 100%');});
  await test('runtime drift stays separate from structural drift and exposes deviation percent',async()=>{const s=sandbox();s.context.fixture={...project(),risk:{drift:{findings:[{kind:'runtime_drift',deviation:{deviation_percent:722.1}}]}}};s.run('state.project=fixture;');assert.equal(s.run('runtimeDriftSummary(fixture).maxDeviation'),722.1);const signals=JSON.parse(s.run('JSON.stringify(attentionSignals())'));assert.ok(signals.some(item=>item.title.includes('runtime drift')&&item.detail.includes('722')));assert.equal(s.run('architectureData().observed_drift_percent'),0,'runtime drift must not be folded into dependency drift');});
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
  await test('Jev runtime shows checkpoints and metered call cost after observation',async()=>{const s=sandbox();s.context.fixture={activity:{available:true,recent:[],agent_context:{}},harness:{software_intelligence:{decision_plane:{authority:'advisory_only',shadow_ab:true}}}};s.run('state.activitySnapshot=fixture;renderActivity();');assert.ok(s.node('#resourceStatus').innerHTML.includes('Model checks'));assert.ok(s.node('#resourceStatus').innerHTML.includes('Unknown'));s.context.fixture.activity.agent_context.decision_runtime={jev:{checkpoint:'verification_failure',status:'active',model:'jev-latest',authority:'increase_only_assist',question_set:{id:'wcode.runtime_checkpoint',version:1},baseline_next_action:'fix_then_retry',candidate_next_action:'semantic_navigation',guidance:['jev:prefer_semantic_navigation'],comparison:{shared_signals:7,choice_disagreements:1,safety_policy_violations:0,shape_mismatches:0},call:{request_bytes:4096,response_bytes:1024,elapsed_ms:240,tokens:{input:1024,output:256,total:1280,source:'byte_estimate'}},calls:{observed:4,successful:4,by_checkpoint:{agent_context:1,post_edit_review:1,verification_failure:2},metered:4,request_bytes:16384,response_bytes:4096,avg_elapsed_ms:300,tokens:{input:4096,output:1024,total:5120,source:'byte_estimate'}},observed_ago_ms:42}};s.run('state.activitySnapshot=fixture;renderActivity();');const html=s.node('#resourceStatus').innerHTML;assert.ok(html.includes('Active'));assert.ok(html.includes('jev-latest'));assert.ok(html.includes('wcode.runtime_checkpoint@1'));assert.ok(html.includes('fix_then_retry → semantic_navigation'));assert.ok(html.includes('jev:prefer_semantic_navigation'));assert.ok(html.includes('verification failure'));assert.ok(html.includes('agent context 1'));assert.ok(html.includes('1,024 in / 256 out'));assert.ok(html.includes('4,096 in / 1,024 out'));assert.ok(html.includes('byte estimate'));});
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
      const deferred=[...s.timers.values()].find(timer=>timer.ms===900);assert.ok(deferred);
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
    const deferred=[...s.timers.values()].find(timer=>timer.ms===900);assert.ok(deferred);
    s.context.document.hidden=true;deferred.fn();await flush();assert.equal(s.requests.length,1);
  });
  await test('paused auto refresh cannot start deferred snapshot rebuilds',async()=>{
    for(const response of [{workspace:'A',snapshot_pending:true},{...project(),snapshot_cache:'stale-while-revalidate'}]){
      const s=sandbox(false,true,{fakeTimers:true});s.run('renderProject=()=>{};renderAttention=()=>{};');
      const refresh=s.run('refreshProject({reason:"auto",preferCached:true})');await flush();
      respond(s.requests[0],response);await refresh;
      const deferred=[...s.timers.values()].find(timer=>timer.ms===900);assert.ok(deferred);
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
      const deferred=[...s.timers.values()].find(timer=>timer.ms===900);assert.ok(deferred);
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
      const epoch=s.run('state.workspaceEpoch'),deferred=[...s.timers.values()].find(timer=>timer.ms===900);assert.ok(deferred);
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
    const deferred=[...s.timers.values()].find(timer=>timer.ms===900);assert.ok(deferred);
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
    assert.equal(s.run('state.revisionKey'),null);assert.ok([...s.timers.values()].some(timer=>timer.ms===900));
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
  await test('workspace snapshot cache refreshes recency before bounded eviction',async()=>{
    const s=sandbox();
    for(let index=0;index<8;index++){
      s.context.fixture={...project('W'+index),marker:'W'+index};
      s.run('state.current="W'+index+'";state.project=fixture;cacheWorkspaceSnapshot();');
    }
    s.run('state.current="W0";clearWorkspaceView();');
    assert.equal(s.run('restoreWorkspaceSnapshot("W0")'),true);
    s.context.fixture={...project('W8'),marker:'W8'};
    s.run('state.current="W8";state.project=fixture;cacheWorkspaceSnapshot();');
    assert.equal(s.run('state.projectCache.has("W0")'),true);
    assert.equal(s.run('state.projectCache.has("W1")'),false);
    assert.equal(s.run('state.projectCache.size'),8);
  });
  await test('workspace access keeps long project paths inside the authorized-project column',async()=>{
    const s=sandbox();s.context.accessFixture={workspace:{id:'A',root:'/fixture/A',write_enabled:true,exec_enabled:true,allowed_commands:[]},workspace_options:[{id:'Code/Other/francis.run/themes/hugo-shortcode-gallery',root:'/Users/francis/Code/Other/francis.run/themes/hugo-shortcode-gallery/<unsafe>'}]};
    s.run('state.workspaceAccess=accessFixture;state.access={allowed_commands:["cargo"],all_commands_authorized:false};state.accessLoaded=true;state.authorizations=[];renderAccess();');
    const html=s.node('#workspaceList').innerHTML;
    assert.ok(html.includes('workspace-chip-root'));assert.ok(html.includes('&lt;unsafe&gt;'));assert.ok(!html.includes('<unsafe>'));
    const dataCss=fs.readFileSync(path.join(root,'src/ui/intelligence_web/styles/data.css'),'utf8');
    const shellCss=fs.readFileSync(path.join(root,'src/ui/intelligence_web/styles/shell.css'),'utf8');
    assert.match(dataCss,/\.workspace-chip\{display:grid;[^}]*max-width:100%;[^}]*min-width:0;/);
    assert.match(dataCss,/\.workspace-chip-root\{[^}]*overflow-wrap:anywhere;/);
    assert.match(dataCss,/\.workspace-list\{display:grid;[^}]*align-content:start;/);
    assert.match(shellCss,/\.access-panel\{[^}]*width:min\(1040px,/);
  });
  await test('system map surfaces dependency flow change and drift signals on the primary canvas',async()=>{
    const s=sandbox();
    const base=project();
    s.context.fixture={...base,proof:{...base.proof,current_evidence:4},architecture:{...base.architecture,blocking_drift_edges:1,components:[],dependencies:[],subsystems:[
      {id:'agent-runtime',title:'Agent Runtime',purpose:'Coordinates agent execution',layer:3,component_ids:[],components:2,implementation_files:12,requirements:3,changed_components:1,blocking_drift_edges:1,designed_depends_on:['core-platform'],observed_depends_on:['Core Platform']},
      {id:'core-platform',title:'Core Platform',purpose:'Shared runtime and graph services',layer:0,component_ids:[],components:3,implementation_files:21,requirements:5,changed_components:0,blocking_drift_edges:0,depends_on:[]},
      {id:'core-platform-shadow',title:'Core Platform',purpose:'Same display title must remain ambiguous',layer:0,component_ids:[],components:1,implementation_files:2,requirements:1,changed_components:0,blocking_drift_edges:0,depends_on:[]}
    ]}};
    s.run('state.project=fixture;state.selectedSubsystem="";renderArchitectureBlueprint();');
    const html=s.node('#architectureBlueprint').innerHTML;
    assert.match(html,/system-map-kpis/);assert.match(html,/Architecture flow/);assert.match(html,/Dependency paths/);
    assert.match(html,/Agent Runtime/);assert.match(html,/Core Platform/);assert.match(html,/Changed subsystems/);assert.match(html,/Blocking drift/);
    assert.match(html,/data-subsystem-card="agent-runtime"/);assert.match(html,/→ Core Platform/);
    assert.equal((html.match(/class="system-map-relation /g)||[]).length,1,'ID/title aliases must not duplicate the same subsystem dependency');
  });

  await test('code graph renders real SVG nodes and directed edges instead of lane cards',async()=>{
    const s=sandbox();s.context.fixture=project();s.context.graphFixture={
      snapshot_id:'snap-1',captured_at_ms:1,provider:'tree-sitter',precision:'syntax',query:'root',mode:'all',depth:2,root_ids:['root'],
      nodes:[
        {node:{id:'up',kind:'function',label:'upstream',attributes:{path:'src/up.rs'},provenance:{provider:'tree-sitter',precision:'syntax',revision:'r1'}},distance:1,upstream:true,downstream:false},
        {node:{id:'root',kind:'function',label:'root',attributes:{path:'src/root.rs'},provenance:{provider:'tree-sitter',precision:'syntax',revision:'r1'}},distance:0,upstream:false,downstream:false},
        {node:{id:'down',kind:'function',label:'downstream',attributes:{path:'src/down.rs'},provenance:{provider:'tree-sitter',precision:'syntax',revision:'r1'}},distance:1,upstream:false,downstream:true}
      ],
      edges:[
        {from:'up',to:'root',kind:'calls',provenance:{provider:'tree-sitter',precision:'syntax',revision:'r1'}},
        {from:'root',to:'down',kind:'calls',provenance:{provider:'tree-sitter',precision:'syntax',revision:'r1'}}
      ],
      upstream_nodes:1,downstream_nodes:1,truncated:false,precision_counts:{syntax:3}
    };
    s.run('state.project=fixture;state.codeGraphView="focus";state.codeGraph=graphFixture;state.selectedCodeNode="root";renderCodeGraph();');
    const html=s.node('#codeGraphMap').innerHTML;
    assert.ok(html.includes('<svg class="code-graph-diagram"'));assert.ok(html.includes('marker-end="url(#codeGraphArrow)"'));
    assert.ok(html.includes('data-code-node="root"'));assert.ok(html.includes('class="code-graph-edge-path selected"'));assert.ok(!html.includes('code-graph-lanes'));
    const positions=JSON.parse(s.run('JSON.stringify([...codeGraphLayout(graphFixture).positions.entries()].map(([id,p])=>[id,p.x]))'));
    const xs=Object.fromEntries(positions);assert.ok(xs.up<xs.root&&xs.root<xs.down,'upstream, focus and downstream must occupy ordered graph bands');
  });
  await test('hidden pages cancel in-flight code graph work and clear its loading state',async()=>{
    const s=sandbox();s.run('state.current="A";');s.node('#codeGraphSearch').value='target';
    const pending=s.run('loadCodeGraph()');await flush();const request=s.requests[0];
    request.options.signal.addEventListener('abort',()=>request.reject(new DOMException('aborted','AbortError')),{once:true});
    s.context.document.hidden=true;s.events.visibilitychange();await pending;
    assert.equal(request.options.signal.aborted,true);
    assert.equal(s.run('state.codeGraphController'),null);
    assert.equal(s.run('state.codeGraphLoading'),false);
  });
  await test('malformed activity cannot remain published after a render failure',async()=>{
    const s=sandbox();s.context.fixture={...project(),activity:{available:true,recent:[]}};
    s.context.previous={workspace:'A',marker:'previous',activity:{available:true,recent:[]},resources:{}};
    s.run('state.project=fixture;state.activitySnapshot=previous;state.activityUpdated=77;');
    const pending=s.run('refreshActivity()');await flush();
    respond(s.requests[0],{workspace:'A',marker:'bad',activity:{available:true,recent:{length:1},active:0,queued:0,completed:0,failed:0},resources:{}});await pending;
    assert.equal(s.run('state.activitySnapshot.marker'),'previous');
    assert.equal(s.run('state.activityUpdated'),77);
    assert.equal(s.run('state.activityError'),true);
  });
  await test('malformed tunnel fields fail closed instead of publishing fake health',async()=>{
    const variants=[
      {provider:{bad:true},role:'active',state:'verified',url:'https://example.test'},
      {provider:'x',role:[],state:'verified',url:'https://example.test'},
      {provider:'x',role:'active',state:'verified',url:'https://example.test',retry_in_seconds:-1}
    ];
    for(const tunnel of variants){
      const s=sandbox(false,true,{fakeTimers:true,controlTunnels:true});
      const pending=s.run('refreshTunnels()');await flush();
      respond(s.requests[0],{public_url_healthy:true,public_endpoint:'ready',tunnels:[tunnel]});await pending;
      assert.equal(s.run('state.tunnelSnapshot'),null);
      assert.ok(s.node('#tunnels').innerHTML.includes('unavailable'));
    }
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
  await test('startup wires command palette controls once and supports localized filtering',async()=>{
    const s=sandbox();let inputBindings=0;
    const search=s.node('#commandPaletteSearch'),listen=search.addEventListener.bind(search);
    search.addEventListener=(type,handler)=>{if(type==='input')inputBindings++;listen(type,handler);};
    s.run('activateWorkspaceTab=()=>{};refreshTunnels=async()=>{};activityTick=async()=>{};refreshProject=async()=>{};scheduleProject=()=>{};');
    await s.run('startObservatory()');await s.run('startObservatory()');
    assert.equal(inputBindings,1);
    s.run('state.language="zh-CN";setCommandPalette(true);');
    assert.equal(s.node('#commandPalette').attrs['aria-hidden'],'false');
    search.value='刷新项目';search.events.input();
    assert.ok(s.node('#commandPaletteList').innerHTML.includes('刷新项目'));
    assert.ok(!s.node('#commandPaletteList').innerHTML.includes('打开架构'));
    search.events.keydown({key:'Escape',preventDefault(){},stopPropagation(){}});
    assert.equal(s.node('#commandPalette').attrs['aria-hidden'],'true');
    assert.equal(search.value,'');
  });
  await test('failure placeholders wire retry actions and preserve workspace and filters',async()=>{
    const s=sandbox(),buttons=[];
    for(const id of ['#statusSummary','#architectureGraph']){
      const button=new Element();button.dataset.summaryAction='refresh';buttons.push(button);
      s.node(id).querySelectorAll=selector=>selector==='[data-summary-action]'?[button]:[];
    }
    s.context.fixture=project();
    s.run('state.project=fixture;state.filter="incomplete";state.autoRefresh=false;renderProject=()=>{};renderAttention=()=>{};renderProjectPlaceholder(true);');
    for(const button of buttons)assert.equal(typeof button.events.click,'function');
    let refreshed;
    s.node('#refresh').click=()=>{refreshed=s.node('#refresh').events.click();};
    await buttons[1].events.click();await flush();
    assert.equal(s.requests.length,1);assert.equal(s.requests[0].url,'/intelligence/revision');
    respond(s.requests[0],{workspace:'A',fingerprint:'retry'});await flush();
    const snapshot=s.requests.find(request=>request.url==='/intelligence/project');
    assert.ok(snapshot);assert.equal(snapshot.options.headers['X-Wcode-Workspace'],'A');
    respond(snapshot,project());
    for(const request of s.requests.filter(request=>request.url==='/intelligence/activity'))respond(request,{workspace:'A',activity:{available:true,recent:[]}});
    await refreshed;
    assert.equal(s.run('state.current'),'A');assert.equal(s.run('state.filter'),'incomplete');
    assert.equal(s.run('state.syncError'),false);assert.equal(s.node('#refresh').disabled,false);
  });
  await test('command palette ignores composition keys until text is committed',async()=>{
    const s=sandbox(),search=s.node('#commandPaletteSearch'),items=['overview','architecture'].map(command=>Object.assign(new Element(),{dataset:{command}}));
    s.node('#commandPaletteList').querySelectorAll=()=>items;
    s.context.executed=[];s.run('wireCommandPalette();setCommandPalette(true);executeCommand=command=>executed.push(command);');
    for(const key of ['Enter','ArrowDown','Escape']){
      const event={key,isComposing:true,preventDefault(){throw new Error('composition must keep native input handling');},stopPropagation(){}};
      search.events.keydown(event);s.events.keydown(event);
      assert.equal(s.run('state.commandPaletteOpen'),true);
    }
    search.events.keydown({key:'Enter',keyCode:229,preventDefault(){throw new Error('legacy composition must remain native');}});
    assert.deepEqual(s.context.executed,[]);assert.equal(s.run('state.commandPaletteIndex'),0);
    search.events.keydown({key:'Enter',isComposing:false,preventDefault(){}});
    assert.deepEqual(s.context.executed,['overview']);
  });
  await test('Escape closes only the command palette from its input or another control',async()=>{
    const s=sandbox();s.run('wireCommandPalette();state.codeGraphFull=true;setCommandPalette(true);');
    const event={key:'Escape',stopped:false,preventDefault(){},stopPropagation(){this.stopped=true;}};
    s.node('#commandPaletteSearch').events.keydown(event);
    if(!event.stopped)s.events.keydown(event);
    assert.equal(s.run('state.commandPaletteOpen'),false);
    assert.equal(s.run('state.codeGraphFull'),true,'the underlying fullscreen graph must stay open');
    s.run('setCommandPalette(true);');
    s.events.keydown({key:'Escape',preventDefault(){},stopPropagation(){}});
    assert.equal(s.run('state.commandPaletteOpen'),false,'Escape from the close button must dismiss the palette');
    assert.equal(s.run('state.codeGraphFull'),true);
  });
  await test('closing the command palette restores focus and cancels delayed focus',async()=>{
    const s=sandbox(),frames=[],search=s.node('#commandPaletteSearch');let paletteFocus=0,returnedFocus=0;
    const origin={isConnected:true,focus(){returnedFocus++;s.context.document.activeElement=origin;}};
    s.context.document.activeElement=origin;
    s.context.requestAnimationFrame=callback=>frames.push(callback);
    search.focus=()=>{paletteFocus++;s.context.document.activeElement=search;};
    s.run('setCommandPalette(true);setCommandPalette(false);');
    frames.splice(0).forEach(callback=>callback());
    assert.equal(paletteFocus,0,'a closed palette must not regain focus on the next frame');
    s.run('setCommandPalette(true);');
    frames.splice(0).forEach(callback=>callback());
    assert.equal(s.context.document.activeElement,search);
    s.run('setCommandPalette(false);');
    assert.equal(s.context.document.activeElement,origin);
    assert.equal(returnedFocus,2);
    s.run('setCommandPalette(false);');assert.equal(returnedFocus,2,'repeated close must not steal focus');
    origin.isConnected=false;let fallbackFocus=0;
    s.node('#projectNavigator').focus=()=>{fallbackFocus++;};
    s.run('setCommandPalette(true);setCommandPalette(false);');
    assert.equal(fallbackFocus,1,'a removed trigger must fall back to project navigation');
  });
  await test('project navigation preserves composition and scopes Escape to its search',async()=>{
    const s=sandbox(),nav=navigatorFixture(s);
    s.run('state.codeGraphFull=true;');
    for(const key of ['Enter','ArrowDown','Escape']){
      nav.search.events.keydown({key,isComposing:true,preventDefault(){throw new Error('composition must stay native');}});
      assert.deepEqual(s.context.activated,[]);assert.equal(nav.search.value,'engine');
    }
    nav.search.events.keydown({key:'Enter',keyCode:229,preventDefault(){throw new Error('legacy composition must stay native');}});
    assert.deepEqual(s.context.activated,[]);
    nav.search.events.keydown({key:'Enter',preventDefault(){}});
    assert.deepEqual(s.context.activated,['one']);
    const escape={key:'Escape',stopped:false,preventDefault(){this.defaultPrevented=true;},stopPropagation(){this.stopped=true;}};
    nav.search.events.keydown(escape);if(!escape.stopped)s.events.keydown(escape);
    assert.equal(nav.search.value,'');assert.equal(s.run('state.codeGraphFull'),true);
  });
  await test('dismissed navigator results cannot activate or reopen on background refresh',async()=>{
    const s=sandbox(),nav=navigatorFixture(s);
    s.events.click({target:{closest:()=>null}});
    assert.equal(nav.results.classList.contains('hidden'),true);
    nav.search.events.keydown({key:'Enter',preventDefault(){}});
    assert.deepEqual(s.context.activated,[],'hidden results must not activate');
    s.run('renderProjectNavigator();');
    assert.equal(nav.results.classList.contains('hidden'),true,'background refresh must preserve dismissal');
    assert.equal(nav.search.getAttribute('aria-expanded'),'false');
    nav.search.events.input();
    assert.equal(nav.results.classList.contains('hidden'),false,'new input intentionally reopens search');
  });
  await test('navigator refresh keeps selection semantics and current item identities',async()=>{
    const s=sandbox(),nav=navigatorFixture(s);
    nav.search.events.keydown({key:'ArrowDown',preventDefault(){}});
    assert.equal(nav.search.getAttribute('aria-activedescendant'),'navigator-result-1');
    s.run('renderProjectNavigator();');
    assert.equal(nav.search.getAttribute('aria-activedescendant'),'navigator-result-1','ARIA must follow the actual selected row');
    s.context.navFixture.architecture.components[0].id='one-new';
    s.run('renderProjectNavigator();');
    nav.search.events.keydown({key:'Home',preventDefault(){}});
    nav.search.events.keydown({key:'Enter',preventDefault(){}});
    assert.deepEqual(s.context.activated,['one-new'],'same visible text must not retain a stale item ID');
  });
  await test('global shortcuts ignore held toggle keys and keep palette focus scoped',async()=>{
    const s=sandbox();let navigatorFocus=0;
    s.node('#projectNavigator').focus=()=>{navigatorFocus++;};
    const chord=repeat=>({key:'k',ctrlKey:true,repeat,preventDefault(){}});
    s.events.keydown(chord(false));assert.equal(s.run('state.commandPaletteOpen'),true);
    s.events.keydown(chord(true));assert.equal(s.run('state.commandPaletteOpen'),true);
    s.context.document.activeElement={tagName:'BUTTON'};
    s.events.keydown({key:'/',preventDefault(){throw new Error('background shortcut must stay inactive');}});
    assert.equal(navigatorFocus,0);
    s.events.keydown(chord(false));assert.equal(s.run('state.commandPaletteOpen'),false);
    navigatorFocus=0;s.events.keydown({key:'/',preventDefault(){}});assert.equal(navigatorFocus,1);
  });
  await test('command palette keeps Tab navigation inside the visible dialog',async()=>{
    const s=sandbox(),first=s.node('#closeCommandPalette'),middle=s.node('#commandPaletteSearch'),last=new Element();
    const controls=[first,middle,last];s.node('#commandPalette').querySelectorAll=()=>controls;
    controls.forEach(control=>{control.focus=()=>{s.context.document.activeElement=control;};});
    s.run('wireCommandPalette();setCommandPalette(true);');let prevented=0;
    s.context.document.activeElement=last;
    s.node('#commandPalette').events.keydown({key:'Tab',preventDefault(){prevented++;}});
    assert.equal(s.context.document.activeElement,first);
    s.node('#commandPalette').events.keydown({key:'Tab',shiftKey:true,preventDefault(){prevented++;}});
    assert.equal(s.context.document.activeElement,last);
    s.context.document.activeElement=middle;
    s.node('#commandPalette').events.keydown({key:'Tab',preventDefault(){prevented++;}});
    assert.equal(prevented,2,'interior Tab navigation remains native');
  });
  await test('delayed access-panel focus cannot target a closed panel or cover the command palette',async()=>{
    const s=sandbox(),frames=[];let accessFocus=0;
    s.context.requestAnimationFrame=callback=>frames.push(callback);
    s.node('#closeAccess').focus=()=>{accessFocus++;};
    s.run('setAccessPanel(true);setAccessPanel(false);');
    frames.splice(0).forEach(callback=>callback());assert.equal(accessFocus,0);
    s.run('setAccessPanel(true);setCommandPalette(true);');
    frames.splice(0).forEach(callback=>callback());assert.equal(accessFocus,0);
  });
  const view=sandbox();
  const components=[['Runtime','Task scheduling','Schedule independent work and retain real capacity through cancellation.'],['Runtime','Context retrieval','Locate relevant source and retain exact edit preconditions.'],['Integrations','MCP transports','Serve one tool runtime across local and remote clients.'],['Integrations','Agent setup','Configure supported coding agents without replacing unrelated settings.'],['Workspace','File operations','Read and edit bounded files with SHA-checked atomic writes.'],['Workspace','Command execution','Run approved commands and retain timeout diagnostics.'],['Intelligence','Verification','Keep checks and evidence bound to the code revision.'],['Intelligence','Software graph','Map component relationships with explicit provider precision.']].map(([scope,name,purpose],i)=>({id:'component:'+i,name,product_scopes:[scope],responsibilities:[purpose],implementation_targets:['src/example/module_'+i+'.rs'],implementation_files:3+i,implementation_lines:250+i*50,requirements:[],depends_on:i?['component:0']:[],changed:i===1||i===5,changed_paths:i===1?['src/example/module_1.rs']:[]}));
  view.context.fixture={...project(),project:'wcode',root:'/example/wcode',pending_authorizations:2,git_review:{available:true,reason:'available'},code:{changed_files:12,source_files:246,source_lines:48190,languages:[],product_scopes:[]},proof:{current_evidence:7,current_passed:5,current_failed:2,current_inconclusive:0,current_verification_plans:1,current_verification_ready:0,current_verification_blocked:1,revision_code:'sha256:example-current-code',revision_design:'sha256:example-current-design',acceptance:{total:33,mapped:33,executed:28,passed:26,fresh:21}},architecture:{components,dependencies:components.slice(1).map((c,i)=>({from:c.id,to:'component:0',from_name:c.name,to_name:'Task scheduling',status:i===3?'unverified_actual':'aligned',desired:true,actual:i!==3,precision:'syntax',blocking:false})),desired_edges:7,observed_edges:6,aligned_edges:6,blocking_drift_edges:0,components_with_implementation:8,observed_drift_percent:0,evidence_coverage_percent:85.7,implementation_coverage_percent:100},workspace_options:[{id:'A',root:'/example/wcode'}]};
  view.context.activity={workspace:'A',pending_authorizations:2,activity:{available:true,active:3,queued:4,orchestration:1,completed:124,failed:3,recent:[{id:132,tool:'agent_context',status:'running',slot_counted:true,wait_ms:6,run_ms:173},{id:131,tool:'cargo test',status:'running',slot_counted:true,wait_ms:450,run_ms:12450},{id:130,tool:'find_symbol',status:'queued',slot_counted:true,wait_ms:42},{id:129,tool:'verify_project',status:'failed',slot_counted:false,wait_ms:0,run_ms:21800}]},resources:{limits:{child_queue:{active:2,limit:2,waiting:3},probe_queue:{active:1,limit:4,waiting:0},resident_memory_bytes:224*1048576}}};
  view.run('state.project=fixture;state.activitySnapshot=activity;state.lastUpdated=Date.now();');
  const snapshots={};
  for(const language of ['en','zh-CN']){view.context.fixtureLanguage=language;view.run('state.language=fixtureLanguage;renderProject(true);');snapshots[language]=[...view.nodes].filter(([id])=>id.startsWith('#')).map(([id,node])=>({id:id.slice(1),html:node.innerHTML,text:node.textContent,value:node.value,attrs:node.attrs}));}
  const dictionary=view.run('JSON.stringify(translations)');
  const styles=STYLE_FILES.map(file=>fs.readFileSync(path.join(root,'src/ui/intelligence_web/styles',file+'.css'),'utf8')).join('\n');
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
if(require.main===module){
  const keepAlive=setInterval(()=>{},1000);
  run()
    .catch(error=>{console.error(error);process.exitCode=1;})
    .finally(()=>clearInterval(keepAlive));
}
module.exports={sandbox,project,respond,flush,stripRuntimeBootstrap};
