'use strict';
const assert=require('node:assert/strict');
const {sandbox,project,respond,flush}=require('./observatory.cjs');

async function run(){
  const results=[];
  async function test(name,fn){
    try{await fn();results.push({name,passed:true});}
    catch(error){results.push({name,passed:false,error:error.stack});}
  }

  await test('clean repository paints a searchable code-graph empty state instead of a blank canvas',async()=>{
    const s=sandbox();
    s.context.fixture={...project(),changes:[],history:[],architecture:{components:[],dependencies:[]},structure:{entries:[]}};
    s.run('state.project=fixture;state.current="A";state.workspaceTab="architecture";state.architectureView="codegraph";maybeLoadCodeGraph();');
    assert.match(s.node('#codeGraphMap').innerHTML,/Search the living code graph/);
    assert.equal(s.requests.filter(request=>request.url.startsWith('/intelligence/code-graph')).length,0);
  });

  await test('code-graph suggestions expose source code and never Design State documents',async()=>{
    const s=sandbox();
    s.context.fixture={...project(),changes:[{path:'.wcode/design/acceptance.yaml'},{path:'src/runtime.rs'}],history:[],architecture:{components:[],dependencies:[]},structure:{entries:[{path:'src/runtime.rs',language:'rust'}]}};
    s.run('state.project=fixture;state.current="A";state.codeGraph=null;state.codeGraphError="";renderCodeGraph();');
    const html=s.node('#codeGraphMap').innerHTML;
    assert.match(html,/data-code-graph-query/);
    assert.match(html,/src\/runtime\.rs/);
    assert.doesNotMatch(html,/\.wcode\/design\/acceptance\.yaml/);
    assert.match(html,/Start from observed project signals/);
  });

  await test('design-only changes do not become the automatic Code Graph focus',async()=>{
    const s=sandbox();
    s.context.fixture={...project(),changes:[{path:'.wcode/design/acceptance.yaml'}],history:[],architecture:{components:[{
      id:'component:webui',implementation_targets:['src/ui/app.js::renderWidget']
    }],dependencies:[]},structure:{entries:[]}};
    s.run('state.project=fixture;state.current="A";state.workspaceTab="architecture";state.architectureView="codegraph";maybeLoadCodeGraph();');
    await flush();
    const request=s.requests.find(item=>item.url.startsWith('/intelligence/code-graph?'));
    assert.ok(request);
    assert.match(request.url,/q=renderWidget/);
    assert.doesNotMatch(request.url,/acceptance/);
    assert.equal(s.node('#codeGraphSearch').value,'','automatic focus must not write into the user search box');
  });

  await test('manual Design State paths are rejected locally because Code Graph is code-only',async()=>{
    const s=sandbox();s.run('state.current="A";');
    s.node('#codeGraphSearch').value='.wcode/design/acceptance.yaml';
    assert.equal(await s.run('loadCodeGraph()'),false);
    assert.equal(s.requests.filter(item=>item.url.startsWith('/intelligence/code-graph?')).length,0);
    assert.match(s.run('state.codeGraphError'),/source-code entities/i);
  });

  await test('clean repository auto-focuses the first declared implementation and renders returned graph data',async()=>{
    const s=sandbox();
    s.context.fixture={...project(),changes:[],history:[],architecture:{components:[{
      id:'component:webui',name:'Web UI',implementation_targets:['src/ui/app.js::renderWidget']
    }],dependencies:[]},structure:{entries:[]}};
    s.run('state.project=fixture;state.current="A";state.workspaceTab="architecture";state.architectureView="codegraph";maybeLoadCodeGraph();');
    await flush();
    const request=s.requests.find(item=>item.url.startsWith('/intelligence/code-graph?'));
    assert.ok(request,'opening Code Graph must issue a real graph request');
    assert.match(request.url,/q=renderWidget/);
    assert.equal(s.node('#codeGraphSearch').value,'','automatic seed stays internal');
    assert.equal(request.options.headers['X-Wcode-Workspace'],'A');
    respond(request,{workspace:'A',graph:{
      snapshot_id:'GRAPH-test',captured_at_ms:1,provider:'wcode-composite',precision:'mixed',
      query:'renderWidget',mode:'all',depth:2,root_ids:['node:renderWidget'],
      nodes:[
        {node:{id:'node:caller',kind:'function',label:'caller',attributes:{path:'src/ui/caller.js'},provenance:{precision:'syntax',provider:'tree-sitter',revision:'rev-syntax'}},distance:1,upstream:true,downstream:false},
        {node:{id:'node:renderWidget',kind:'function',label:'renderWidget',attributes:{path:'src/ui/app.js'},provenance:{precision:'syntax',provider:'tree-sitter',revision:'rev-syntax'}},distance:0,upstream:false,downstream:false},
        {node:{id:'node:callee',kind:'function',label:'callee',attributes:{path:'src/ui/callee.js'},provenance:{precision:'semantic',provider:'lsp',revision:'rev-semantic'}},distance:1,upstream:false,downstream:true}
      ],
      edges:[
        {from:'node:caller',to:'node:renderWidget',kind:'calls',provenance:{precision:'syntax',provider:'tree-sitter',revision:'rev-syntax'}},
        {from:'node:renderWidget',to:'node:callee',kind:'calls',provenance:{precision:'semantic',provider:'lsp',revision:'rev-semantic'}}
      ],
      precision_counts:{syntax:1,semantic:1},upstream_nodes:1,downstream_nodes:1,truncated:false
    }});
    await flush();
    await flush();
    assert.match(s.node('#codeGraphMap').innerHTML,/renderWidget/);
    assert.match(s.node('#codeGraphMap').innerHTML,/caller/);
    assert.match(s.node('#codeGraphMap').innerHTML,/callee/);
    assert.match(s.node('#codeGraphSummary').innerHTML,/3 nodes/);
    assert.match(s.node('#codeGraphInspector').innerHTML,/tree-sitter/);
    assert.equal(s.run('state.codeGraphWorkspace'),'A');
  });

  await test('changed source path remains the highest-priority automatic code-graph focus',async()=>{
    const s=sandbox();
    s.context.fixture={...project(),changes:[{path:'src/changed.rs'}],history:[],architecture:{components:[{
      id:'component:webui',implementation_targets:['src/ui/app.js::renderWidget']
    }],dependencies:[]},structure:{entries:[]}};
    s.run('state.project=fixture;state.current="A";state.workspaceTab="architecture";state.architectureView="codegraph";maybeLoadCodeGraph();');
    await flush();
    const request=s.requests.find(item=>item.url.startsWith('/intelligence/code-graph?'));
    assert.ok(request);
    assert.match(request.url,/q=src%2Fchanged.rs/);
    assert.equal(s.node('#codeGraphSearch').value,'','changed-file seed must not become user input');
    request.options.signal?.throwIfAborted?.();
    respond(request,{workspace:'A',graph:{snapshot_id:'G',captured_at_ms:1,provider:'p',precision:'syntax',query:'src/changed.rs',mode:'all',depth:2,root_ids:['changed'],nodes:[{node:{id:'changed',kind:'file',label:'src/changed.rs',attributes:{path:'src/changed.rs'},provenance:{precision:'syntax',provider:'tree-sitter',revision:'rev-changed'}},distance:0,upstream:false,downstream:false}],edges:[],precision_counts:{},upstream_nodes:0,downstream_nodes:0,truncated:false}});
    await flush();
  });

  await test('auto-seeded graph can reload controls without populating the search box',async()=>{
    const s=sandbox();
    s.context.fixture={...project(),changes:[],history:[],architecture:{components:[{id:'a',implementation_targets:['src/a.rs::alpha']}],dependencies:[]},structure:{entries:[]}};
    s.run('state.project=fixture;state.current="A";state.workspaceTab="architecture";state.architectureView="codegraph";maybeLoadCodeGraph();');
    await flush();
    const first=s.requests.find(item=>item.url.startsWith('/intelligence/code-graph?'));
    respond(first,{workspace:'A',graph:{snapshot_id:'G',captured_at_ms:1,provider:'p',precision:'syntax',query:'alpha',mode:'all',depth:2,root_ids:['a'],nodes:[{node:{id:'a',kind:'function',label:'alpha',attributes:{path:'src/a.rs'},provenance:{precision:'syntax',provider:'tree-sitter',revision:'r'}},distance:0,upstream:false,downstream:false}],edges:[],precision_counts:{},upstream_nodes:0,downstream_nodes:0,truncated:false}});
    await flush();await flush();
    assert.equal(s.node('#codeGraphSearch').value,'');
    s.run('state.codeGraphDepth=3;reloadCodeGraphFromCurrentContext();');await flush();
    const second=s.requests.filter(item=>item.url.startsWith('/intelligence/code-graph?'))[1];
    assert.ok(second);assert.match(second.url,/node_id=a/);assert.match(second.url,/depth=3/);
    assert.equal(s.node('#codeGraphSearch').value,'');
  });

  await test('clearing the code-graph search does not resurrect the previous query',async()=>{
    const s=sandbox();
    s.run('state.current="A";state.codeGraphQuery="oldSymbol";state.codeGraph={nodes:[{node:{id:"old",label:"oldSymbol"}}],edges:[],root_ids:["old"]};els.codeGraphSearch.value="";');
    const loaded=await s.run('loadCodeGraph()');
    assert.equal(loaded,false);
    assert.equal(s.requests.filter(item=>item.url.startsWith('/intelligence/code-graph?')).length,0);
    assert.equal(s.run('state.codeGraph'),null);
    assert.equal(s.run('state.codeGraphError'),'');
  });

  await test('workspace switch starts the new code graph without waiting for an aborted old request',async()=>{
    const s=sandbox();
    s.context.fixtureA={...project('A'),changes:[],history:[],architecture:{components:[{id:'a',implementation_targets:['src/a.rs::alpha']}],dependencies:[]},structure:{entries:[]}};
    s.context.fixtureB={...project('B'),changes:[],history:[],architecture:{components:[{id:'b',implementation_targets:['src/b.rs::beta']}],dependencies:[]},structure:{entries:[]}};
    s.run('state.project=fixtureA;state.current="A";state.workspaceTab="architecture";state.architectureView="codegraph";maybeLoadCodeGraph();');
    await flush();
    const first=s.requests.find(item=>item.url.startsWith('/intelligence/code-graph?'));
    assert.ok(first);assert.match(first.url,/q=alpha/);
    s.run('state.current="B";clearWorkspaceView();state.project=fixtureB;state.workspaceTab="architecture";state.architectureView="codegraph";maybeLoadCodeGraph();');
    await flush();
    const graphRequests=s.requests.filter(item=>item.url.startsWith('/intelligence/code-graph?'));
    assert.equal(graphRequests.length,2,'new workspace must not be blocked by the old loading flag');
    const second=graphRequests[1];assert.match(second.url,/q=beta/);
    assert.equal(first.options.signal.aborted,true);
    respond(first,{workspace:'A',graph:{snapshot_id:'A',captured_at_ms:1,provider:'p',precision:'syntax',query:'alpha',mode:'all',depth:2,root_ids:['a'],nodes:[{node:{id:'a',kind:'function',label:'alpha',attributes:{},provenance:{precision:'syntax',provider:'tree-sitter',revision:'rev-a'}},distance:0,upstream:false,downstream:false}],edges:[],precision_counts:{},upstream_nodes:0,downstream_nodes:0,truncated:false}});
    await flush();
    assert.notEqual(s.run('state.codeGraphWorkspace'),'A');
    respond(second,{workspace:'B',graph:{snapshot_id:'B',captured_at_ms:1,provider:'p',precision:'syntax',query:'beta',mode:'all',depth:2,root_ids:['b'],nodes:[{node:{id:'b',kind:'function',label:'beta',attributes:{},provenance:{precision:'syntax',provider:'tree-sitter',revision:'rev-b'}},distance:0,upstream:false,downstream:false}],edges:[],precision_counts:{},upstream_nodes:0,downstream_nodes:0,truncated:false}});
    await flush();await flush();
    assert.equal(s.run('state.codeGraphWorkspace'),'B');
    assert.match(s.node('#codeGraphMap').innerHTML,/beta/);
    assert.equal(s.run('state.codeGraphQuery'),'beta');
  });

  await test('null code-graph payload fails closed instead of masquerading as an empty graph',async()=>{
    const s=sandbox();s.run('state.current="A";');s.node('#codeGraphSearch').value='target';
    const pending=s.run('loadCodeGraph()');await flush();respond(s.requests[0],{workspace:'A',graph:null});
    assert.equal(await pending,false);
    assert.equal(s.run('state.codeGraph'),null);
    assert.match(s.run('state.codeGraphError'),/invalid response/i);
  });

  await test('shape-less code-graph payload fails closed instead of reporting success',async()=>{
    const s=sandbox();s.run('state.current="A";');s.node('#codeGraphSearch').value='target';
    const pending=s.run('loadCodeGraph()');await flush();respond(s.requests[0],{workspace:'A',graph:{}});
    assert.equal(await pending,false);
    assert.equal(s.run('state.codeGraph'),null);
    assert.match(s.run('state.codeGraphError'),/invalid response/i);
  });

  await test('malformed nested code-graph nodes and edges fail closed before rendering',async()=>{
    const malformed=[
      {snapshot_id:'G',query:'q',mode:'all',depth:2,root_ids:['x'],nodes:[{}],edges:[],precision_counts:{}},
      {snapshot_id:'G',query:'q',mode:'all',depth:2,root_ids:['x'],nodes:[{node:{id:7,label:'x'}}],edges:[],precision_counts:{}},
      {snapshot_id:'G',query:'q',mode:'all',depth:2,root_ids:['x'],nodes:[{node:{id:'x',label:'x'}}],edges:[{from:'x'}],precision_counts:{}},
      {snapshot_id:'G',query:'q',mode:'all',depth:2,root_ids:[7],nodes:[{node:{id:'x',label:'x'}}],edges:[],precision_counts:{}},
      {snapshot_id:'G',query:'q',mode:'all',depth:2,root_ids:['x'],nodes:[{node:{id:'x',label:'x'}}],edges:[],precision_counts:{syntax:'bad'}}
    ];
    for(const graph of malformed){
      const s=sandbox();s.run('state.current="A";');s.node('#codeGraphSearch').value='target';
      const pending=s.run('loadCodeGraph()');await flush();respond(s.requests[0],{workspace:'A',graph});
      assert.equal(await pending,false);
      assert.equal(s.run('state.codeGraph'),null);
      assert.match(s.run('state.codeGraphError'),/invalid response/i);
    }
  });

  await test('deeply malformed code-graph entries clear partial graph state',async()=>{
    const malformed={snapshot_id:'G',captured_at_ms:1,provider:'p',precision:'syntax',query:'q',mode:'all',depth:2,root_ids:['x'],
      nodes:[{node:{id:'x',kind:'function',label:null,attributes:null,provenance:null},distance:'far',upstream:false,downstream:false}],
      edges:[],precision_counts:{},upstream_nodes:0,downstream_nodes:0,truncated:false};
    const s=sandbox();s.run('state.current="A";state.codeGraphWorkspace="old";state.selectedCodeNode="old";');s.node('#codeGraphSearch').value='target';
    const pending=s.run('loadCodeGraph()');await flush();respond(s.requests[0],{workspace:'A',graph:malformed});
    assert.equal(await pending,false);
    assert.equal(s.run('state.codeGraph'),null);
    assert.equal(s.run('state.codeGraphWorkspace'),'');
    assert.equal(s.run('state.selectedCodeNode'),'');
    assert.match(s.run('state.codeGraphError'),/invalid response/i);
  });

  await test('code-graph response cannot exceed the requested client safety bound',async()=>{
    for(const oversized of ['nodes','edges']){
      const s=sandbox();s.run('state.current="A";');s.node('#codeGraphSearch').value='target';
      const graph={snapshot_id:'G',captured_at_ms:1,provider:'p',precision:'syntax',query:'target',mode:'all',depth:2,root_ids:['n0'],
        nodes:Array.from({length:oversized==='nodes'?141:2},(_,index)=>({node:{id:'n'+index,kind:'function',label:'n'+index,attributes:{},provenance:{precision:'syntax',provider:'tree-sitter',revision:'rev-'+index}},distance:index?1:0,upstream:false,downstream:index>0})),
        edges:[],precision_counts:{},upstream_nodes:0,downstream_nodes:oversized==='nodes'?140:1,truncated:true};
      if(oversized==='edges') graph.edges=Array.from({length:421},()=>({from:'n0',to:'n1',kind:'calls',provenance:{precision:'syntax',provider:'tree-sitter',revision:'rev-edge'}}));
      const pending=s.run('loadCodeGraph()');await flush();respond(s.requests[0],{workspace:'A',graph});
      assert.equal(await pending,false);
      assert.equal(s.run('state.codeGraph'),null);
      assert.match(s.run('state.codeGraphError'),/invalid response/i);
    }
  });

  await test('malformed graph history entries are ignored without crashing the workbench',async()=>{
    const s=sandbox();s.context.fixture={...project(),history:[null,{}, {id:null,captured_at_ms:1},{id:'<bad>',captured_at_ms:1},{id:'invalid-date',captured_at_ms:'nope'}]};
    s.run('state.project=fixture;state.current="A";state.codeGraph=null;renderCodeGraph();');
    const html=s.node('#codeGraphSnapshot').innerHTML;
    assert.ok(html.includes('&lt;bad&gt;'));
    assert.ok(!html.includes('<bad>'));
    assert.ok(!html.includes('invalid-date'));
  });

  await test('code-graph response matches the complete serialized graph contract',async()=>{
    const base={snapshot_id:'G',captured_at_ms:1,provider:'p',precision:'mixed',query:'target',mode:'all',depth:2,root_ids:['root'],
      nodes:[{node:{id:'root',kind:'function',label:'target',attributes:{},provenance:{provider:'tree-sitter',precision:'syntax',revision:'rev-node'}},distance:0,upstream:false,downstream:false}],
      edges:[],precision_counts:{},upstream_nodes:0,downstream_nodes:0,truncated:false};
    const malformed=[
      {...base,captured_at_ms:'bad'},
      {...base,provider:''},
      {...base,precision:'bogus'},
      {...base,query:''},
      {...base,root_ids:['root','root']},
      {...base,upstream_nodes:-1},
      {...base,truncated:'yes'},
      {...base,nodes:[{...base.nodes[0],node:{...base.nodes[0].node,kind:'bogus'}}]},
      {...base,nodes:[{...base.nodes[0],node:{...base.nodes[0].node,provenance:{provider:'tree-sitter',precision:'syntax'}}}]},
      {...base,nodes:[{...base.nodes[0],distance:3}]}
    ];
    for(const graph of malformed){
      const s=sandbox();s.run('state.current="A";');s.node('#codeGraphSearch').value='target';
      const pending=s.run('loadCodeGraph()');await flush();respond(s.requests[0],{workspace:'A',graph});
      assert.equal(await pending,false);
      assert.equal(s.run('state.codeGraph'),null);
    }
  });

  await test('code-graph response must match the exact request semantics',async()=>{
    const variants=[
      {name:'query',mutate:graph=>{graph.query='other';}},
      {name:'mode',mutate:graph=>{graph.mode='calls';}},
      {name:'depth',mutate:graph=>{graph.depth=3;}},
      {name:'snapshot',snapshot:'SNAP-A',mutate:graph=>{graph.snapshot_id='SNAP-B';}}
    ];
    for(const variant of variants){
      const s=sandbox();
      const snapshot=variant.snapshot||'';
      s.context.fixture={...project(),history:snapshot?[{id:snapshot,captured_at_ms:1}]:[]};
      s.run('state.current="A";state.project=fixture;state.codeGraphMode="all";state.codeGraphDepth=2;state.codeGraphSnapshot='+JSON.stringify(snapshot)+';');
      s.node('#codeGraphSearch').value='target';
      const pending=s.run('loadCodeGraph()');await flush();
      const graph={snapshot_id:snapshot||'LATEST',captured_at_ms:1,provider:'p',precision:'syntax',query:'target',mode:'all',depth:2,root_ids:['root'],
        nodes:[{node:{id:'root',kind:'function',label:'target',attributes:{},provenance:{provider:'tree-sitter',precision:'syntax',revision:'rev-node'}},distance:0,upstream:false,downstream:false}],
        edges:[],precision_counts:{},upstream_nodes:0,downstream_nodes:0,truncated:false};
      variant.mutate(graph);
      respond(s.requests[0],{workspace:'A',graph});
      assert.equal(await pending,false,variant.name+' mismatch must fail closed');
      assert.equal(s.run('state.codeGraph'),null);
      assert.match(s.run('state.codeGraphError'),/invalid response/i);
    }
  });

  const report={suite:'code-graph-webui',results};
  console.log(JSON.stringify(report,null,2));
  assert.ok(results.every(item=>item.passed),results.filter(item=>!item.passed).map(item=>item.name+'\n'+item.error).join('\n'));
}
if(require.main===module)run().catch(error=>{console.error(error);process.exitCode=1;});