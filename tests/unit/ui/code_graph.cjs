'use strict';
const assert=require('node:assert/strict');
const {sandbox,project,respond,flush}=require('./observatory.cjs');

const overview=(workspace='A',extra={})=>({
  snapshot_id:'GRAPH-overview',captured_at_ms:1,provider:'wcode-composite',precision:'mixed',
  files_considered:4,files_indexed:4,files_failed:0,scan_truncated:false,graph_truncated:false,
  total_nodes:8,total_edges:3,total_files:4,
  languages:{rust:2,'java-script':1,python:1},relation_counts:{calls:2,imports:1},
  nodes:[
    {id:'file:src/a.rs',label:'src/a.rs',path:'src/a.rs',language:'rust',symbols:3,degree:2},
    {id:'file:src/b.rs',label:'src/b.rs',path:'src/b.rs',language:'rust',symbols:2,degree:1},
    {id:'file:web/app.js',label:'web/app.js',path:'web/app.js',language:'java-script',symbols:2,degree:2},
    {id:'file:tools/x.py',label:'tools/x.py',path:'tools/x.py',language:'python',symbols:1,degree:1}
  ],
  edges:[
    {from:'file:src/a.rs',to:'file:src/b.rs',count:1,kinds:{calls:1},precision:{syntax:1}},
    {from:'file:web/app.js',to:'file:src/a.rs',count:1,kinds:{imports:1},precision:{syntax:1}}
  ],
  truncated:false,...extra
});
const searchResult=(query='alpha')=>({
  snapshot_id:'GRAPH-overview',captured_at_ms:1,provider:'wcode-composite',precision:'mixed',
  query,truncated:false,results:[
    {node:{id:'symbol:alpha',kind:'function',label:'alpha',attributes:{path:'src/a.rs',language:'rust'},provenance:{provider:'tree-sitter',precision:'syntax',revision:'rev-a'}},match_kind:'exact_label',score:0,relations:3},
    {node:{id:'file:src/a.rs',kind:'file',label:'src/a.rs',attributes:{path:'src/a.rs',language:'rust'},provenance:{provider:'tree-sitter',precision:'syntax',revision:'rev-a'}},match_kind:'path_contains',score:4,relations:2}
  ]
});
const focusGraph=(root='symbol:alpha',extra={})=>({
  snapshot_id:'GRAPH-overview',captured_at_ms:1,provider:'wcode-composite',precision:'mixed',
  query:root,mode:'all',depth:2,root_ids:[root],
  nodes:[
    {node:{id:root,kind:root.startsWith('file:')?'file':'function',label:root.startsWith('file:')?'src/a.rs':'alpha',attributes:{path:'src/a.rs',language:'rust',source_kind:root.startsWith('file:')?'file':'function'},provenance:{precision:'semantic',provider:'lsp:rust-analyzer',revision:'rev-sem'}},distance:0,upstream:false,downstream:false},
    {node:{id:'symbol:caller',kind:'function',label:'caller',attributes:{path:'src/b.rs',language:'rust'},provenance:{precision:'syntax',provider:'tree-sitter',revision:'rev-syntax'}},distance:1,upstream:true,downstream:false},
    {node:{id:'symbol:callee',kind:'function',label:'callee',attributes:{path:'src/a.rs',language:'rust'},provenance:{precision:'semantic',provider:'lsp:rust-analyzer',revision:'rev-sem'}},distance:1,upstream:false,downstream:true}
  ],
  edges:[
    {from:'symbol:caller',to:root,kind:'calls',provenance:{precision:'syntax',provider:'tree-sitter',revision:'rev-syntax'}},
    {from:root,to:'symbol:callee',kind:'calls',provenance:{precision:'semantic',provider:'lsp:rust-analyzer',revision:'rev-sem'}}
  ],
  precision_counts:{syntax:1,semantic:1},upstream_nodes:1,downstream_nodes:1,truncated:false,...extra
});

async function run(){
  const results=[];
  async function test(name,fn){
    try{await fn();results.push({name,passed:true});}
    catch(error){results.push({name,passed:false,error:error.stack});}
  }

  await test('opening Code Graph defaults to repository overview instead of an arbitrary focus',async()=>{
    const s=sandbox();s.context.fixture={...project(),history:[]};
    s.run('state.project=fixture;state.current="A";state.workspaceTab="architecture";state.architectureView="codegraph";maybeLoadCodeGraph();');
    await flush();
    assert.equal(s.requests.length,1);
    assert.match(s.requests[0].url,/view=overview/);
    assert.doesNotMatch(s.requests[0].url,/[?&]q=/);
    assert.equal(s.run('state.codeGraphView'),'overview');
  });

  await test('repository overview renders bounded repository metrics and language lanes',async()=>{
    const s=sandbox();s.run('state.current="A";state.codeGraphView="overview";');
    const pending=s.run('loadCodeGraphOverview()');await flush();
    respond(s.requests[0],{workspace:'A',overview:overview('A',{truncated:true,scan_truncated:true})});
    assert.equal(await pending,true);
    assert.match(s.node('#codeGraphSummary').innerHTML,/Repository overview/);
    assert.match(s.node('#codeGraphSummary').innerHTML,/4\/4/);
    assert.match(s.node('#codeGraphSummary').innerHTML,/bounded \/ truncated/);
    const html=s.node('#codeGraphMap').innerHTML;
    assert.match(html,/rust · 2/);assert.match(html,/java-script · 1/);assert.match(html,/python · 1/);
  });

  await test('repository overview repaint cache follows live language changes',async()=>{
    const s=sandbox();s.context.ov=overview();
    s.run('state.current="A";state.workspaceTab="architecture";state.architectureView="codegraph";state.codeGraphView="overview";state.codeGraphOverview=ov;state.codeGraphWorkspace="A";state.language="en";maybeLoadCodeGraph();');
    assert.match(s.node('#codeGraphSummary').innerHTML,/Repository overview/);
    s.run('state.language="zh-CN";maybeLoadCodeGraph();');
    assert.match(s.node('#codeGraphSummary').innerHTML,/仓库概览/);
  });

  await test('repository overview groups files by language instead of flattening polyglot symbols',async()=>{
    const s=sandbox();s.context.ov=overview();s.run('state.current="A";state.codeGraphView="overview";');
    const data=JSON.parse(s.run('JSON.stringify([...codeGraphOverviewLayout(ov).lanes])'));
    assert.deepEqual(data.map(x=>x.language),['java-script','python','rust']);
    assert.equal(data.find(x=>x.language==='rust').count,2);
  });

  await test('repository overview keeps existing node coordinates stable across refresh and reorder',async()=>{
    const s=sandbox();s.context.first=overview();s.context.second=overview('A',{nodes:[
      {id:'file:tools/x.py',label:'tools/x.py',path:'tools/x.py',language:'python',symbols:1,degree:1},
      {id:'file:src/b.rs',label:'src/b.rs',path:'src/b.rs',language:'rust',symbols:2,degree:1},
      {id:'file:web/app.js',label:'web/app.js',path:'web/app.js',language:'java-script',symbols:2,degree:2},
      {id:'file:src/a.rs',label:'src/a.rs',path:'src/a.rs',language:'rust',symbols:3,degree:2},
      {id:'file:src/new.rs',label:'src/new.rs',path:'src/new.rs',language:'rust',symbols:1,degree:0}
    ]});
    s.run('state.current="A";state.codeGraphView="overview";layoutA=codeGraphOverviewLayout(first);layoutB=codeGraphOverviewLayout(second);');
    for(const id of ['file:src/a.rs','file:src/b.rs','file:web/app.js','file:tools/x.py']){
      s.context.nodeId=id;
      assert.equal(s.run('layoutA.positions.get(nodeId).x'),s.run('layoutB.positions.get(nodeId).x'),id);
      assert.equal(s.run('layoutA.positions.get(nodeId).y'),s.run('layoutB.positions.get(nodeId).y'),id);
    }
  });

  await test('viewport scroll position is restored for the same graph key',async()=>{
    const s=sandbox();const viewport=s.node('#viewport');s.context.viewport=viewport;
    s.run('state.current="A";state.codeGraphView="overview";els.codeGraphMap.querySelector=()=>viewport;viewport.scrollLeft=180;viewport.scrollTop=90;captureCodeGraphViewport();viewport.scrollLeft=0;viewport.scrollTop=0;bindCodeGraphViewport();');
    assert.equal(viewport.scrollLeft,180);assert.equal(viewport.scrollTop,90);
  });

  await test('search returns candidates without silently replacing the rendered graph',async()=>{
    const s=sandbox();s.context.ov=overview();s.run('state.current="A";state.codeGraphView="overview";state.codeGraphOverview=ov;');
    s.node('#codeGraphSearch').value='alpha';
    const pending=s.run('searchCodeGraph()');await flush();
    assert.match(s.requests[0].url,/view=search/);assert.match(s.requests[0].url,/q=alpha/);
    respond(s.requests[0],{workspace:'A',search:searchResult('alpha')});
    assert.equal(await pending,true);
    assert.equal(s.run('state.codeGraphView'),'overview');
    assert.equal(s.run('state.codeGraph'),null);
    assert.equal(s.run('state.codeGraphSearchResults.length'),2);
    assert.match(s.node('#codeGraphSearchResults').innerHTML,/alpha/);
  });

  await test('choosing a search candidate binds focus to that exact node id',async()=>{
    const s=sandbox();s.run('state.current="A";');s.node('#codeGraphSearch').value='alpha';
    const search=s.run('searchCodeGraph()');await flush();respond(s.requests[0],{workspace:'A',search:searchResult('alpha')});await search;
    s.run('chooseCodeGraphSearchResult(0)');await flush();
    assert.equal(s.requests.length,2);
    assert.match(s.requests[1].url,/view=focus/);
    assert.match(s.requests[1].url,/node_id=symbol%3Aalpha/);
    assert.doesNotMatch(s.requests[1].url,/[?&]q=/);
  });

  await test('focused response renders only after the chosen root returns',async()=>{
    const s=sandbox();s.run('state.current="A";');s.node('#codeGraphSearch').value='alpha';
    const pending=s.run('loadCodeGraph({nodeId:"symbol:alpha",query:"alpha"})');await flush();
    respond(s.requests[0],{workspace:'A',graph:focusGraph()});
    assert.equal(await pending,true);
    assert.equal(s.run('state.codeGraphView'),'focus');
    assert.equal(s.run('state.selectedCodeNode'),'symbol:alpha');
    assert.match(s.node('#codeGraphMap').innerHTML,/alpha/);
    assert.match(s.node('#codeGraphMap').innerHTML,/caller/);
    assert.match(s.node('#codeGraphMap').innerHTML,/callee/);
  });

  await test('focused layout keeps existing node coordinates stable when response order changes',async()=>{
    const s=sandbox();s.context.g1=focusGraph();s.context.g2=focusGraph('symbol:alpha',{nodes:[
      focusGraph().nodes[2],focusGraph().nodes[0],focusGraph().nodes[1],
      {node:{id:'symbol:new',kind:'function',label:'newNode',attributes:{path:'src/c.rs'},provenance:{precision:'syntax',provider:'tree-sitter',revision:'r'}},distance:2,upstream:false,downstream:true}
    ]});
    s.run('state.current="A";state.codeGraphView="focus";state.codeGraphMode="all";state.codeGraphDepth=2;la=codeGraphLayout(g1);lb=codeGraphLayout(g2);');
    for(const id of ['symbol:alpha','symbol:caller','symbol:callee']){
      s.context.nodeId=id;
      assert.equal(s.run('la.positions.get(nodeId).x'),s.run('lb.positions.get(nodeId).x'),id);
      assert.equal(s.run('la.positions.get(nodeId).y'),s.run('lb.positions.get(nodeId).y'),id);
    }
  });

  await test('clearing search aborts search/focus state and returns to repository overview',async()=>{
    const s=sandbox();s.context.ov=overview();s.run('state.current="A";state.codeGraphView="focus";state.codeGraph=({});state.codeGraphQuery="old";state.selectedCodeNode="old";state.codeGraphOverview=ov;state.codeGraphWorkspace="A";state.codeGraphSearchController=new AbortController();state.codeGraphController=new AbortController();oldSearch=state.codeGraphSearchController;oldGraph=state.codeGraphController;clearCodeGraphSearchState();');
    assert.equal(s.run('oldSearch.signal.aborted'),true);assert.equal(s.run('oldGraph.signal.aborted'),true);
    assert.equal(s.run('state.codeGraphQuery'),'');assert.equal(s.run('state.selectedCodeNode'),'');
    assert.equal(s.run('state.codeGraphView'),'overview');
    assert.equal(s.run('state.codeGraphSearchResults.length'),0);
  });

  await test('Design State paths are rejected before graph search network traffic',async()=>{
    const s=sandbox();s.run('state.current="A";');s.node('#codeGraphSearch').value='.wcode/design/acceptance.yaml';
    assert.equal(await s.run('searchCodeGraph()'),false);
    assert.equal(s.requests.length,0);
    assert.match(s.run('state.codeGraphError'),/source-code entities/i);
  });

  await test('full screen is a graph workspace and closes the details rail by default',async()=>{
    const s=sandbox();s.run('setCodeGraphInspector(true);setCodeGraphFull(true);');
    assert.equal(s.run('state.codeGraphFull'),true);
    assert.equal(s.run('state.codeGraphInspectorOpen'),false);
    assert.equal(s.node('#codeGraphInspector').classes.has('hidden'),true);
    assert.equal(s.node('#codeGraphSection').classes.has('code-graph-fullscreen'),true);
    assert.equal(s.node('#codeGraphSection').attrs.role,'dialog');
    assert.equal(s.node('#codeGraphSection').attrs['aria-modal'],'true');
  });

  await test('details toggle really removes and restores the inspector rail',async()=>{
    const s=sandbox();s.run('setCodeGraphInspector(false)');
    assert.equal(s.node('#codeGraphInspector').classes.has('hidden'),true);
    assert.equal(s.node('#codeGraphInspector').attrs['aria-hidden'],'true');
    s.run('setCodeGraphInspector(true)');
    assert.equal(s.node('#codeGraphInspector').classes.has('hidden'),false);
    assert.equal(s.node('#codeGraphInspector').attrs['aria-hidden'],'false');
    assert.equal(s.node('#codeGraphInspectorToggle').attrs['aria-pressed'],'true');
  });

  await test('Escape exits Code Graph full screen before other overlays',async()=>{
    const s=sandbox();let prevented=false;s.run('setCodeGraphFull(true)');
    s.events.keydown({key:'Escape',preventDefault(){prevented=true;}});
    assert.equal(prevented,true);assert.equal(s.run('state.codeGraphFull'),false);
  });

  await test('leaving Code Graph view cannot strand the full-screen overlay',async()=>{
    const s=sandbox();s.run('setCodeGraphFull(true);state.architectureView="components";renderArchitecture();');
    assert.equal(s.run('state.codeGraphFull'),false);
  });

  await test('full-screen accessibility label follows live language changes',async()=>{
    const s=sandbox();s.run('setCodeGraphFull(true);state.language="zh-CN";applyLanguage();');
    assert.match(s.node('#codeGraphFull').attrs['aria-label'],/退出代码图谱全屏/);
    s.run('state.language="en";applyLanguage();');
    assert.match(s.node('#codeGraphFull').attrs['aria-label'],/Exit full screen code graph/);
  });

  await test('repository mode disables focus-only depth control and focus restores it',async()=>{
    const s=sandbox();s.run('setCodeGraphView("overview",{load:false})');
    assert.equal(s.node('#codeGraphDepth').disabled,true);
    s.run('setCodeGraphView("focus",{load:false})');
    assert.equal(s.node('#codeGraphDepth').disabled,false);
  });

  await test('workspace switch aborts stale graph work and starts a fresh repository overview',async()=>{
    const s=sandbox();s.context.fixtureA={...project('A'),history:[]};s.context.fixtureB={...project('B'),history:[]};
    s.run('state.project=fixtureA;state.current="A";state.workspaceTab="architecture";state.architectureView="codegraph";maybeLoadCodeGraph();');await flush();
    const first=s.requests[0];assert.match(first.url,/view=overview/);
    s.run('state.current="B";clearWorkspaceView();state.project=fixtureB;state.workspaceTab="architecture";state.architectureView="codegraph";maybeLoadCodeGraph();');await flush();
    assert.equal(first.options.signal.aborted,true);
    assert.equal(s.requests.length,2);assert.match(s.requests[1].url,/view=overview/);
    assert.equal(s.requests[1].options.headers['X-Wcode-Workspace'],'B');
  });

  await test('malformed repository overview fails closed',async()=>{
    const s=sandbox();s.run('state.current="A";');const pending=s.run('loadCodeGraphOverview()');await flush();
    respond(s.requests[0],{workspace:'A',overview:{snapshot_id:'G',nodes:[{id:'bad'}],edges:[],languages:{},relation_counts:{},files_considered:1,files_indexed:1,total_nodes:1,total_edges:0,total_files:1,truncated:false}});
    assert.equal(await pending,false);assert.equal(s.run('state.codeGraphOverview'),null);assert.match(s.run('state.codeGraphError'),/Invalid repository graph overview/);
  });

  await test('malformed search results fail closed without changing focus',async()=>{
    const s=sandbox();s.run('state.current="A";state.codeGraphView="overview";');s.node('#codeGraphSearch').value='alpha';
    const pending=s.run('searchCodeGraph()');await flush();
    respond(s.requests[0],{workspace:'A',search:{snapshot_id:'G',query:'alpha',truncated:false,results:[{node:{id:'x'}}]}});
    assert.equal(await pending,false);assert.equal(s.run('state.codeGraphSearchResults.length'),0);assert.equal(s.run('state.codeGraphView'),'overview');
    assert.match(s.run('state.codeGraphError'),/Invalid code graph search response/);
  });

  await test('malformed focused graph fails closed and preserves the previous rendered graph',async()=>{
    const s=sandbox();s.context.previous=focusGraph();s.run('state.current="A";state.codeGraph=previous;state.codeGraphView="focus";state.codeGraphWorkspace="A";');
    const pending=s.run('loadCodeGraph({nodeId:"symbol:alpha",query:"alpha"})');await flush();
    respond(s.requests[0],{workspace:'A',graph:{}});
    assert.equal(await pending,false);
    assert.equal(s.run('state.codeGraph.root_ids[0]'),'symbol:alpha');
    assert.match(s.run('state.codeGraphError'),/Invalid focused code graph response/);
  });

  await test('focused response cannot exceed the 140-node client safety bound',async()=>{
    const s=sandbox();s.context.g=focusGraph('symbol:alpha',{nodes:Array.from({length:141},(_,i)=>({node:{id:`n${i}`,kind:'function',label:`n${i}`,attributes:{path:'src/a.rs'},provenance:{precision:'syntax',provider:'tree-sitter',revision:'r'}},distance:i?1:0,upstream:false,downstream:i>0})),root_ids:['n0'],query:'n0',upstream_nodes:0,downstream_nodes:140,precision_counts:{syntax:1}});
    assert.equal(s.run('validCodeGraphResponse({graph:g})'),false);
  });

  await test('graph view keys isolate snapshots so historical layouts never contaminate latest',async()=>{
    const s=sandbox();s.run('state.current="A";state.codeGraphView="overview";state.codeGraphSnapshot="";k1=codeGraphViewKey();state.codeGraphSnapshot="GRAPH-old";k2=codeGraphViewKey();');
    assert.notEqual(s.run('k1'),s.run('k2'));assert.match(s.run('k2'),/GRAPH-old/);
  });

  await test('focused graph exposes semantic provenance instead of flattening LSP and syntax edges',async()=>{
    const s=sandbox();s.context.g=focusGraph();s.run('state.current="A";state.codeGraph=g;state.codeGraphView="focus";state.selectedCodeNode="symbol:alpha";renderCodeGraph();');
    assert.match(s.node('#codeGraphMap').innerHTML,/data-precision="semantic"/);
    assert.match(s.node('#codeGraphInspector').innerHTML,/lsp:rust-analyzer/);
    assert.match(s.node('#codeGraphMap').innerHTML,/tree-sitter/);
  });

  if(results.some(result=>!result.passed)){
    for(const result of results.filter(result=>!result.passed))console.error(result.name+'\n'+result.error);
    process.exitCode=1;
  }
  console.log(JSON.stringify({suite:'code-graph-webui',results},null,2));
  return results;
}
if(require.main===module)run();
module.exports={run};
