const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const root = process.argv[2] || path.resolve(__dirname, '../../..');
const source = [
  fs.readFileSync(path.join(root, 'src/ui/intelligence_web/app/code_graph_explorer.js'), 'utf8'),
  fs.readFileSync(path.join(root, 'src/ui/intelligence_web/app/features.js'), 'utf8'),
].join('\n');
const runtime = fs.readFileSync(path.join(root, 'src/ui/intelligence_web/app/runtime.js'), 'utf8');
const results = [];
function setup() {
  const host = {innerHTML:'', querySelectorAll:()=>[], querySelector:()=>null};
  const requests = [], navigation = [];
  const state = {current:'A', workspaceEpoch:1};
  const context = {state, q:()=>host, URLSearchParams, AbortController,
    localized:(en)=>en, esc:value=>String(value).replace(/[&<>"']/g, ch=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch])),
    requestFailureMessage:error=>error.message,
    uiJson:(url, method, body, options)=>new Promise((resolve,reject)=>requests.push({url,method,body,options,resolve,reject})),
    revealSection:id=>navigation.push({type:'reveal',id}),
    renderArchitecture:()=>navigation.push({type:'render'}),
    loadCodeGraph:async args=>{ navigation.push({type:'load',args}); return true; },
  };
  vm.createContext(context);
  vm.runInContext(source + '\nglobalThis.api={openChangeInspection,clearChangeInspection,invalidateChangeInspection,renderChangeInspector,changeDisplayRows,revealChangeSourceLine,openChangeSymbolInGraph,openChangeRelationNodeInGraph,openChangeSymbolImpact};', context);
  return {state,host,requests,navigation,api:context.api};
}
function view(overrides={}) {
  return {path:'main.rs', layer:'working', snapshot_id:'a'.repeat(64), head:'b'.repeat(40),
    index_fingerprint:'c'.repeat(64), worktree_sha256:'d'.repeat(64), kind:'unified_diff',
    content:'diff --git a/main.rs b/main.rs\n--- a/main.rs\n+++ b/main.rs\n@@ -3,2 +3,2 @@\n context\n-before\n+after\n',
    before_changed_ranges:[{start_line:4,end_line:4}], after_changed_ranges:[{start_line:4,end_line:4}],
    changed_ranges_truncated:false, after_source_matches_worktree:true, truncated:false, redacted:false, ...overrides};
}
function impact(overrides={}) {
  return {path:'main.rs',snapshot_id:'a'.repeat(64),source_sha256:'d'.repeat(64),source_state:'worktree',precision:'syntax',provider:'tree-sitter',
    mapping:'before_after_line_overlap',counterpart_basis:'qualified_name_kind_syntax',before_symbols_available:true,before_source_state:'head',before_source_sha256:'e'.repeat(64),before_unavailable_reason:null,
    before_symbols:[{node_id:'symbol:ts:before',name:'original',qualified_name:'original',kind:'function',start_line:4,end_line:4,counterpart_only:false,changed_ranges:[{start_line:4,end_line:4}]}],
    partial:false,unavailable_reason:null,
    after_symbols:[{node_id:'symbol:ts:abc',name:'changed',qualified_name:'changed',kind:'function',start_line:4,end_line:4,counterpart_only:false,changed_ranges:[{start_line:4,end_line:4}]}],...overrides};
}
function relationImpact(overrides={}) {
  return {path:'main.rs',snapshot_id:'a'.repeat(64),node_id:'symbol:ts:abc',source_sha256:'d'.repeat(64),
    provider:'tree-sitter+search',precision:'syntax',routing:'syntax-degraded',degraded:true,degraded_from:'lsp',
    reason:'language_server_unavailable',incoming_calls:[{path:'caller.rs',line:1,character:1,name:'caller',node_id:'symbol:ts:caller'}],
    references:[],implementations:[],search_matches:[{path:'caller.rs',line:1,source_sha256:'f'.repeat(64),text:'fn caller() { changed(); }'}],partial:false,...overrides};
}
function render(s, value, symbolImpact=null) {
  s.state.changeInspection = {path:value.path,layer:value.layer,data:value,symbolImpact,loading:false,error:''};
  s.api.renderChangeInspector();
}
async function test(name, fn) {
  try { await fn(); results.push({name,passed:true}); }
  catch(error) { results.push({name,passed:false,error:error.stack}); }
}
(async()=>{
  await test('hunk gutters distinguish old and new coordinates, not file headers', ()=>{
    const s=setup(), rows=s.api.changeDisplayRows(view());
    assert.equal(rows[1].before,''); assert.equal(rows[2].after,'');
    assert.equal(rows[4].before,3); assert.equal(rows[4].after,3);
    assert.equal(rows[5].before,4); assert.equal(rows[5].after,'');
    assert.equal(rows[6].before,''); assert.equal(rows[6].after,4);
  });
  await test('untracked source keeps literal plus/minus with source line numbers', ()=>{
    const s=setup(), rows=s.api.changeDisplayRows(view({kind:'untracked_source',content:'-source\n+source'}));
    assert.equal(rows[0].after,1); assert.equal(rows[1].after,2);
    assert.equal(rows[0].tone,'context');
  });
  await test('source, path and identity HTML are escaped', ()=>{
    const s=setup(); render(s,view({path:'<img src=x>.rs',content:'@@ -1 +1 @@\n-<script>old</script>\n+<script>new</script>',head:'<svg onload=x>'}));
    assert(!s.host.innerHTML.includes('<script>')); assert(!s.host.innerHTML.includes('<img'));
    assert(s.host.innerHTML.includes('&lt;script&gt;'));
    assert(s.host.innerHTML.includes('&lt;svg'));
  });
  await test('empty layer never claims the whole working tree is clean', ()=>{
    const s=setup(); render(s,view({content:''}));
    assert(s.host.innerHTML.includes('No differences in this selected layer'));
    assert(s.host.innerHTML.includes('does not mean every layer is clean'));
  });
  await test('binary and partial/redacted content remain explicit', ()=>{
    const s=setup(); render(s,view({kind:'binary',content:''}));
    assert(s.host.innerHTML.includes('Binary difference'));
    render(s,view({redacted:true,truncated:true}));
    assert(s.host.innerHTML.includes('not a complete source view'));
  });
  await test('large previews cap DOM rows and expose truncation', ()=>{
    const s=setup(); render(s,view({kind:'untracked_source',content:'line\n'.repeat(2000)}));
    assert.equal((s.host.innerHTML.match(/class="change-source-line /g)||[]).length,1500);
    assert(s.host.innerHTML.includes('Partial or redacted preview'));
  });
  await test('requests are explicit read-only workspace-scoped and path-encoded', async()=>{
    const s=setup(), p=s.api.openChangeInspection('a & b.rs');
    const request=s.requests[0], parsed=new URLSearchParams(request.url.split('?')[1]);
    assert.equal(parsed.get('path'),'a & b.rs'); assert.equal(request.method,'GET');
    assert.equal(request.options.workspace,'A'); assert.equal(request.body,undefined);
    request.resolve({workspace:'A',change:view({path:'a & b.rs'})}); await p;
    assert.equal(s.state.changeInspection.data.path,'a & b.rs');
  });
  await test('current changed-line overlap is rendered as syntax mapping, not semantic proof', async()=>{
    const s=setup(), p=s.api.openChangeInspection('main.rs');
    s.requests[0].resolve({workspace:'A',change:view(),symbol_impact:impact()}); await p;
    assert.equal(s.state.changeInspection.symbolImpact.after_symbols[0].name,'changed');
    assert.match(s.host.innerHTML,/Syntax-overlapping changed definitions/);
    assert.match(s.host.innerHTML,/unique same qualified-name \+ kind counterparts/);
    assert.match(s.host.innerHTML,/original/);
    assert.match(s.host.innerHTML,/changed/);
  });
  await test('syntax-overlap rows expose same-snapshot diff-line drilldown while counterpart-only rows stay display-only', ()=>{
    const s=setup(), value=view();
    const before={node_id:'symbol:ts:before',name:'original',qualified_name:'original',kind:'function',start_line:4,end_line:4,counterpart_only:false,changed_ranges:[{start_line:4,end_line:4}]};
    const after={node_id:'symbol:ts:after',name:'changed',qualified_name:'changed',kind:'function',start_line:4,end_line:4,counterpart_only:false,changed_ranges:[{start_line:4,end_line:4}]};
    render(s,value,impact({before_symbols:[before],after_symbols:[after]}));
    assert.match(s.host.innerHTML,/data-change-source-side="before"/);
    assert.match(s.host.innerHTML,/data-change-source-node="symbol:ts:before"/);
    assert.match(s.host.innerHTML,/data-change-source-side="after"/);
    assert.match(s.host.innerHTML,/data-change-source-line="4"/);
    let scrolled=0, focused=0;
    s.host.querySelector=selector => selector === '.change-source-line[data-change-before-line="4"]' ? {scrollIntoView:()=>{scrolled++;},focus:()=>{focused++;}} : null;
    assert.equal(s.api.revealChangeSourceLine('before','symbol:ts:before',4),true);
    assert.equal(scrolled,1); assert.equal(focused,1);
    assert.equal(s.api.revealChangeSourceLine('before','symbol:ts:before',99),false);
    const paired={...after,node_id:'symbol:ts:paired',counterpart_only:true,changed_ranges:[]};
    render(s,value,impact({after_symbols:[paired]}));
    assert(!s.host.innerHTML.includes('data-change-source-node="symbol:ts:paired"'));
  });
  await test('paired symbol signatures expose syntax-level before/after change without semantic overclaim', ()=>{
    const s=setup(), value=view();
    const before={node_id:'symbol:ts:before',name:'same',qualified_name:'same',kind:'function',start_line:4,end_line:6,counterpart_only:false,signature:'fn same(a: i32)',signature_redacted:false};
    const after={node_id:'symbol:ts:after',name:'same',qualified_name:'same',kind:'function',start_line:4,end_line:6,counterpart_only:false,signature:'fn same(a: i64)',signature_redacted:false};
    render(s,value,impact({before_symbols:[before],after_symbols:[after]}));
    assert.match(s.host.innerHTML,/Signature changed/);
    assert.match(s.host.innerHTML,/syntax signature only/);
    assert(!s.host.innerHTML.includes('semantic signature'));
  });
  await test('unmatched syntax identities stay path-local add/remove instead of rename inference', ()=>{
    const s=setup(), value=view();
    const before={node_id:'symbol:ts:before',name:'old_name',qualified_name:'old_name',kind:'function',start_line:4,end_line:4,counterpart_only:false,definition_change:'removed'};
    const after={node_id:'symbol:ts:after',name:'new_name',qualified_name:'new_name',kind:'function',start_line:4,end_line:4,counterpart_only:false,definition_change:'added'};
    render(s,value,impact({definition_change_basis:'qualified_name_kind_complete_syntax_outlines',before_symbols:[before],after_symbols:[after]}));
    assert.match(s.host.innerHTML,/Removed definition · syntax only/);
    assert.match(s.host.innerHTML,/Added definition · syntax only/);
    assert.match(s.host.innerHTML,/not semantic impact or rename proof/);
    assert(!s.host.innerHTML.includes('Renamed definition'));
  });
  await test('deleted after-state still shows exact before definitions without inventing current symbols', ()=>{
    const s=setup(), value=view();
    render(s,value,impact({source_state:null,source_sha256:null,unavailable_reason:'no_current_source',after_symbols:[]}));
    assert.match(s.host.innerHTML,/original/);
    assert.match(s.host.innerHTML,/After-state source is unavailable/);
    assert(!s.host.innerHTML.includes('data-change-symbol="symbol:ts:before"'));
  });
  await test('staged symbol mapping uses the exact index source instead of a newer worktree', async()=>{
    const s=setup(), p=s.api.openChangeInspection('main.rs','staged');
    const staged=view({layer:'staged',after_source_matches_worktree:false});
    const stagedImpact=impact({source_sha256:'f'.repeat(64),source_state:'index',before_source_state:'head',after_symbols:[{node_id:'symbol:ts:def',name:'staged_symbol',qualified_name:'staged_symbol',kind:'function',start_line:4,end_line:4,counterpart_only:false,changed_ranges:[{start_line:4,end_line:4}]}]});
    s.requests[0].resolve({workspace:'A',change:staged,symbol_impact:stagedImpact}); await p;
    assert.equal(s.state.changeInspection.data.after_source_matches_worktree,false);
    assert.equal(s.state.changeInspection.symbolImpact.source_state,'index');
    assert.match(s.host.innerHTML,/exact staged index snapshot/);
    assert.match(s.host.innerHTML,/staged_symbol/);
    assert(!s.host.innerHTML.includes('data-change-symbol="symbol:ts:def"'));
    assert.match(s.host.innerHTML,/not linked to the current graph/);
  });
  await test('changed symbol drilldown pins the captured repository revision and rejects stale or non-mapped syntax nodes', async()=>{
    const s=setup(), revision={code:'sha256:code',design:'sha256:design'};
    s.state.codeGraphSnapshot='GRAPH-old';
    s.state.changeInspection={repositoryRevision:revision,data:view(),symbolImpact:impact()};
    assert.equal(await s.api.openChangeSymbolInGraph('symbol:ts:abc'),true);
    assert.equal(s.state.codeGraphSnapshot,'');
    assert.equal(s.navigation.length,3);
    assert.equal(s.navigation[0].type,'reveal'); assert.equal(s.navigation[0].id,'codeGraphSection');
    assert.equal(s.navigation[1].type,'render');
    assert.equal(s.navigation[2].type,'load'); assert.equal(s.navigation[2].args.nodeId,'symbol:ts:abc');
    assert.deepEqual(s.navigation[2].args.repositoryRevision,revision);
    assert.equal(await s.api.openChangeSymbolInGraph('semantic:abc'),false);
    assert.equal(await s.api.openChangeSymbolInGraph('symbol:ts:not-mapped'),false);
    s.state.changeInspection.symbolImpact={...impact(),source_state:'index'};
    assert.equal(await s.api.openChangeSymbolInGraph('symbol:ts:abc'),false);
    s.state.changeInspection.symbolImpact=impact(); s.state.changeInspection.data=view({after_source_matches_worktree:false});
    assert.equal(await s.api.openChangeSymbolInGraph('symbol:ts:abc'),false);
    s.state.changeInspection.repositoryRevision=null;
    assert.equal(await s.api.openChangeSymbolInGraph('symbol:ts:abc'),false);
    assert.equal(s.navigation.length,3);
  });
  await test('changed-symbol action boundaries revalidate exact diff overlap instead of trusting mutable state', async()=>{
    const s=setup(), revision={code:'sha256:code',design:'sha256:design'};
    const malformed=impact({after_symbols:[{node_id:'symbol:ts:abc',name:'changed',qualified_name:'changed',kind:'function',start_line:4,end_line:4,counterpart_only:false,changed_ranges:[]}]});
    s.state.changeInspection={path:'main.rs',layer:'working',snapshotId:'a'.repeat(64),impactNodeId:'symbol:ts:abc',repositoryRevision:revision,data:view(),symbolImpact:malformed,relationImpact:relationImpact()};
    assert.equal(await s.api.openChangeSymbolInGraph('symbol:ts:abc'),false);
    assert.equal(s.navigation.length,0);
    const traced=s.api.openChangeSymbolImpact('symbol:ts:abc');
    assert.equal(s.requests.length,0);
    assert.equal(await traced,false);
    assert.equal(await s.api.openChangeRelationNodeInGraph('symbol:ts:caller'),false);
    assert.equal(s.navigation.length,0);
  });
  await test('in-flight impact response revalidates the changed-symbol mapping before it can populate relations', async()=>{
    const s=setup(), revision={code:'sha256:code',design:'sha256:design'};
    s.state.changeInspection={path:'main.rs',layer:'working',snapshotId:'a'.repeat(64),repositoryRevision:revision,data:view(),symbolImpact:impact()};
    const traced=s.api.openChangeSymbolImpact('symbol:ts:abc');
    assert.equal(s.requests.length,1);
    s.state.changeInspection.symbolImpact=impact({after_symbols:[{node_id:'symbol:ts:abc',name:'changed',qualified_name:'changed',kind:'function',start_line:4,end_line:4,counterpart_only:false,changed_ranges:[]}]});
    s.requests[0].resolve({workspace:'A',repository_revision:revision,impact:relationImpact()});
    assert.equal(await traced,false);
    assert.equal(s.state.changeInspection.relationImpact,null);
    assert(s.state.changeInspection.impactError);
  });
  await test('syntax impact caller drilldown validates the relation row instead of requiring the caller to be the changed symbol', async()=>{
    const s=setup(), revision={code:'sha256:code',design:'sha256:design'};
    s.state.changeInspection={path:'main.rs',snapshotId:'a'.repeat(64),impactNodeId:'symbol:ts:abc',repositoryRevision:revision,data:view(),symbolImpact:impact(),relationImpact:relationImpact()};
    assert.equal(await s.api.openChangeRelationNodeInGraph('symbol:ts:caller'),true);
    assert.equal(s.navigation.length,3);
    assert.equal(s.navigation[0].type,'reveal'); assert.equal(s.navigation[0].id,'codeGraphSection');
    assert.equal(s.navigation[1].type,'render');
    assert.equal(s.navigation[2].type,'load'); assert.equal(s.navigation[2].args.nodeId,'symbol:ts:caller');
    assert.deepEqual(s.navigation[2].args.repositoryRevision,revision);
    assert.equal(await s.api.openChangeRelationNodeInGraph('symbol:ts:not-returned'),false);
    s.state.changeInspection.relationImpact=relationImpact({snapshot_id:'f'.repeat(64)});
    assert.equal(await s.api.openChangeRelationNodeInGraph('symbol:ts:caller'),false);
    s.state.changeInspection.relationImpact=relationImpact({precision:'semantic',provider:'lsp:rust-analyzer',routing:'lsp',degraded:false,degraded_from:null,search_matches:[],incoming_calls:[{path:'caller.rs',line:1,character:1,name:'caller'}]});
    assert.equal(await s.api.openChangeRelationNodeInGraph('symbol:ts:caller'),false);
    assert.equal(s.navigation.length,3);
  });
  await test('changed symbol impact pins workspace snapshot and reports syntax fallback without semantic overclaim', async()=>{
    const s=setup(), revision={code:'sha256:code',design:'sha256:design'}; let p=s.api.openChangeInspection('main.rs');
    s.requests[0].resolve({workspace:'A',repository_revision:revision,change:view(),symbol_impact:impact()}); await p;
    const traced=s.api.openChangeSymbolImpact('symbol:ts:abc');
    assert.equal(s.requests.length,2);
    const parsed=new URLSearchParams(s.requests[1].url.split('?')[1]);
    assert.equal(parsed.get('path'),'main.rs'); assert.equal(parsed.get('layer'),'working');
    assert.equal(parsed.get('expected_snapshot'),'a'.repeat(64)); assert.equal(parsed.get('node_id'),'symbol:ts:abc');
    assert.equal(s.requests[1].options.workspace,'A');
    s.requests[1].resolve({workspace:'A',repository_revision:revision,impact:relationImpact({partial:true})});
    assert.equal(await traced,true);
    assert.match(s.host.innerHTML,/Syntax impact candidates/);
    assert.match(s.host.innerHTML,/not call relations, semantic proof, or verification proof/);
    assert.match(s.host.innerHTML,/Selected changed symbol/);
    assert.match(s.host.innerHTML,/Incoming callers/);
    assert.match(s.host.innerHTML,/References/);
    assert.match(s.host.innerHTML,/Implementations/);
    assert.match(s.host.innerHTML,/Exact search matches/);
    assert.match(s.host.innerHTML,/text mention, not a call relation/);
    assert.match(s.host.innerHTML,/Impact relations are bounded\/partial/);
    assert(!s.host.innerHTML.includes('Impact evidence'));
    assert.match(s.host.innerHTML,/caller.rs:1/);
    assert.match(s.host.innerHTML,/data-change-relation-node="symbol:ts:caller"/);
    assert.match(s.host.innerHTML,/Open in Code Graph · syntax node/);
    assert.match(s.host.innerHTML,/aria-label="Open caller.rs:1 syntax caller in revision-bound Code Graph"/);
    assert.match(source,/querySelectorAll\("\[data-change-relation-node\]"\)/);
    assert(source.includes('void openChangeRelationNodeInGraph(button.dataset.changeRelationNode)'));
    const spoofedSyntax=s.api.openChangeSymbolImpact('symbol:ts:abc');
    s.requests[2].resolve({workspace:'A',repository_revision:revision,impact:relationImpact({provider:'lsp:rust-analyzer',precision:'syntax',routing:'lsp',degraded:true,degraded_from:'lsp'})});
    assert.equal(await spoofedSyntax,false);
    assert.equal(s.state.changeInspection.relationImpact,null);
    const semantic=s.api.openChangeSymbolImpact('symbol:ts:abc');
    s.requests[3].resolve({workspace:'A',repository_revision:revision,impact:relationImpact({provider:'lsp:rust-analyzer',precision:'semantic',routing:'lsp',degraded:false,degraded_from:null,search_matches:[],
      incoming_calls:[{path:'caller.rs',line:1,character:1,name:'caller'}],references:[{path:'ref.rs',line:2,character:1,name:'original'}],implementations:[{path:'impl.rs',line:3,character:1,name:'changed_impl'}]})});
    assert.equal(await semantic,true);
    assert.match(s.host.innerHTML,/Semantic impact relations/);
    assert.match(s.host.innerHTML,/ref.rs:2/);
    assert.match(s.host.innerHTML,/impl.rs:3/);
    assert(!s.host.innerHTML.includes('Semantic impact evidence'));
    const mismatched=s.api.openChangeSymbolImpact('symbol:ts:abc');
    s.requests[4].resolve({workspace:'A',repository_revision:{code:'sha256:newer',design:'sha256:design'},impact:relationImpact({provider:'lsp:rust-analyzer',precision:'semantic',routing:'lsp',degraded:false,degraded_from:null,search_matches:[],incoming_calls:[],references:[],implementations:[]})});
    assert.equal(await mismatched,false);
    assert.equal(s.state.changeInspection.relationImpact,null);
    const contradictory=s.api.openChangeSymbolImpact('symbol:ts:abc');
    s.requests[5].resolve({workspace:'A',repository_revision:revision,impact:relationImpact({provider:'lsp:rust-analyzer',precision:'semantic',routing:'lsp',degraded:true,degraded_from:'lsp',
      incoming_calls:[{path:'caller.rs',line:1,character:1,name:'caller'}],references:[{path:'ref.rs',line:2,character:1,name:'original'}],implementations:[]})});
    assert.equal(await contradictory,false);
    assert.equal(s.state.changeInspection.relationImpact,null);
    const semanticSyntaxNode=s.api.openChangeSymbolImpact('symbol:ts:abc');
    s.requests[6].resolve({workspace:'A',repository_revision:revision,impact:relationImpact({provider:'lsp:rust-analyzer',precision:'semantic',routing:'lsp',degraded:false,degraded_from:null,search_matches:[],
      incoming_calls:[{path:'caller.rs',line:1,character:1,name:'caller',node_id:'symbol:ts:caller'}],references:[],implementations:[]})});
    assert.equal(await semanticSyntaxNode,false);
    assert.equal(s.state.changeInspection.relationImpact,null);
    const stringCoordinate=s.api.openChangeSymbolImpact('symbol:ts:abc');
    s.requests[7].resolve({workspace:'A',repository_revision:revision,impact:relationImpact({
      incoming_calls:[{path:'caller.rs',line:'1',character:1,name:'caller',node_id:'symbol:ts:caller'}]
    })});
    assert.equal(await stringCoordinate,false);
    assert.equal(s.state.changeInspection.relationImpact,null);
    const zeroCharacter=s.api.openChangeSymbolImpact('symbol:ts:abc');
    s.requests[8].resolve({workspace:'A',repository_revision:revision,impact:relationImpact({
      incoming_calls:[{path:'caller.rs',line:1,character:0,name:'caller',node_id:'symbol:ts:caller'}]
    })});
    assert.equal(await zeroCharacter,false);
    assert.equal(s.state.changeInspection.relationImpact,null);
    const syntaxAsSemantic=s.api.openChangeSymbolImpact('symbol:ts:abc');
    s.requests[9].resolve({workspace:'A',repository_revision:revision,impact:relationImpact({provider:'tree-sitter+search',precision:'semantic',routing:'lsp',degraded:false,degraded_from:null,search_matches:[],incoming_calls:[],references:[],implementations:[]})});
    assert.equal(await syntaxAsSemantic,false);
    assert.equal(s.state.changeInspection.relationImpact,null);
    const fakeSemantic=s.api.openChangeSymbolImpact('symbol:ts:abc');
    s.requests[10].resolve({workspace:'A',repository_revision:revision,impact:relationImpact({provider:'rust-analyzer',precision:'semantic',routing:'lsp',degraded:false,degraded_from:null,search_matches:[],incoming_calls:[],references:[],implementations:[]})});
    assert.equal(await fakeSemantic,false); assert.equal(s.state.changeInspection.relationImpact,null);
  });
  await test('staged snapshot symbols cannot request current-worktree impact', async()=>{
    const s=setup(), p=s.api.openChangeInspection('main.rs','staged');
    s.requests[0].resolve({workspace:'A',change:view({layer:'staged',after_source_matches_worktree:false}),
      symbol_impact:impact({source_state:'index',source_sha256:'f'.repeat(64)})}); await p;
    assert.equal(await s.api.openChangeSymbolImpact('symbol:ts:abc'),false);
    assert.equal(s.requests.length,1);
  });
  await test('impact request rejects symbol mappings from a different path snapshot or worktree bytes', async()=>{
    const revision={code:'sha256:code',design:'sha256:design'};
    for(const symbolImpact of [impact({path:'other.rs'}),impact({snapshot_id:'f'.repeat(64)}),impact({source_sha256:'e'.repeat(64)})]) {
      const s=setup();
      s.state.changeInspection={path:'main.rs',snapshotId:'a'.repeat(64),repositoryRevision:revision,data:view(),symbolImpact};
      const attempt=s.api.openChangeSymbolImpact('symbol:ts:abc');
      if(s.requests.length) s.requests[0].reject(new Error('unexpected impact request'));
      assert.equal(await attempt,false);
      assert.equal(s.requests.length,0);
    }
  });
  await test('foreign or stale impact response never repopulates the inspector', async()=>{
    const s=setup(), revision={code:'sha256:code',design:'sha256:design'}; let p=s.api.openChangeInspection('main.rs');
    s.requests[0].resolve({workspace:'A',repository_revision:revision,change:view(),symbol_impact:impact()}); await p;
    let traced=s.api.openChangeSymbolImpact('symbol:ts:abc');
    s.requests[1].resolve({workspace:'B',impact:relationImpact()}); assert.equal(await traced,false);
    assert.equal(s.state.changeInspection.relationImpact,null); assert(s.state.changeInspection.impactError);
    traced=s.api.openChangeSymbolImpact('symbol:ts:abc');
    s.requests[2].reject(Object.assign(new Error('stale'),{status:409})); assert.equal(await traced,false);
    assert.match(s.state.changeInspection.impactError,/Reload before tracing impact/);
  });
  await test('switching comparison layers pins the same captured snapshot', async()=>{
    const s=setup(); let p=s.api.openChangeInspection('main.rs');
    s.requests[0].resolve({workspace:'A',change:view()}); await p;
    p=s.api.openChangeInspection('main.rs','staged');
    assert(s.requests[1].url.includes('expected_snapshot='+'a'.repeat(64)));
    s.requests[1].resolve({workspace:'A',change:view({layer:'staged'})}); await p;
    assert.equal(s.state.changeInspection.data.layer,'staged');
  });
  await test('stale snapshot error removes old source rather than silently rebasing', async()=>{
    const s=setup(); let p=s.api.openChangeInspection('main.rs');
    s.requests[0].resolve({workspace:'A',change:view()}); await p;
    p=s.api.openChangeInspection('main.rs','unstaged');
    s.requests[1].reject(Object.assign(new Error('conflict'),{status:409})); await p;
    assert.equal(s.state.changeInspection.data,null);
    assert(s.host.innerHTML.includes('instead of mixing versions'));
    assert(!s.host.innerHTML.includes('change-source-line'));
  });
  await test('stale snapshot stays pinned across layer switches until explicit reload', async()=>{
    const s=setup(); let p=s.api.openChangeInspection('main.rs');
    s.requests[0].resolve({workspace:'A',change:view()}); await p;
    p=s.api.openChangeInspection('main.rs','unstaged');
    s.requests[1].reject(Object.assign(new Error('conflict'),{status:409})); await p;
    p=s.api.openChangeInspection('main.rs','staged');
    assert(s.requests[2].url.includes('expected_snapshot='+'a'.repeat(64)));
    s.requests[2].reject(Object.assign(new Error('conflict'),{status:409})); await p;
    const fresh=s.api.openChangeInspection('main.rs','staged',true);
    assert(!s.requests[3].url.includes('expected_snapshot'));
    s.requests[3].resolve({workspace:'A',change:view({layer:'staged',snapshot_id:'e'.repeat(64)})}); await fresh;
    assert.equal(s.state.changeInspection.data.snapshot_id,'e'.repeat(64));
  });
  await test('A to B to A late response cannot repopulate the inspector', async()=>{
    const s=setup(), first=s.api.openChangeInspection('main.rs');
    s.state.current='B'; s.state.workspaceEpoch++; s.api.clearChangeInspection();
    s.state.current='A'; s.state.workspaceEpoch++;
    const second=s.api.openChangeInspection('main.rs');
    s.requests[1].resolve({workspace:'A',change:view({content:'new snapshot'})}); await second;
    s.requests[0].resolve({workspace:'A',change:view({content:'old snapshot'})}); await first;
    assert.equal(s.state.changeInspection.data.content,'new snapshot');
  });
  await test('malformed or foreign response never displays source', async()=>{
    for(const result of [{workspace:'B',change:view()}, {workspace:'A',change:view({path:'foreign.rs'})},
      {workspace:'A',change:view({snapshot_id:'bad'})}, {workspace:'A',change:view({content:'x'.repeat(65537)})},
      {workspace:'A',change:view({before_changed_ranges:[{start_line:9,end_line:3}]})},
      {workspace:'A',change:view({after_changed_ranges:[{start_line:'4',end_line:'4'}]})},
      {workspace:'A',change:view({after_changed_ranges:Array.from({length:257},(_,i)=>({start_line:i+1,end_line:i+1}))})},
      {workspace:'A',change:view({changed_ranges_truncated:'no'})},
      {workspace:'A',change:view({after_source_matches_worktree:'yes'})},
      {workspace:'A',change:view({after_source_matches_worktree:false}),symbol_impact:impact()},
      {workspace:'A',change:view(),symbol_impact:impact({precision:'semantic'})},
      {workspace:'A',change:view(),symbol_impact:impact({before_source_state:'worktree'})},
      {workspace:'A',change:view(),symbol_impact:impact({after_symbols:[{...impact().after_symbols[0],start_line:'4'}]})},
      {workspace:'A',change:view(),symbol_impact:impact({after_symbols:[{...impact().after_symbols[0],changed_ranges:[]}]})},
      {workspace:'A',change:view(),symbol_impact:impact({after_symbols:[{...impact().after_symbols[0],start_line:8,end_line:12,changed_ranges:[{start_line:4,end_line:4}]}]})},
      {workspace:'A',change:view(),symbol_impact:impact({after_symbols:[{...impact().after_symbols[0],counterpart_only:true,changed_ranges:[{start_line:4,end_line:4}]}]})},
      {workspace:'A',change:view(),symbol_impact:impact({before_symbols:[{node_id:'semantic:bad',name:'x',qualified_name:'x',kind:'function',start_line:1,end_line:1}]})}]) {
      const s=setup(), p=s.api.openChangeInspection('main.rs'); s.requests[0].resolve(result); await p;
      assert.equal(s.state.changeInspection.data,null); assert(s.state.changeInspection.error);
    }
  });
  await test('refresh invalidates in-flight source, explicit reload takes a fresh snapshot', async()=>{
    const s=setup(), p=s.api.openChangeInspection('main.rs');
    s.api.invalidateChangeInspection();
    s.requests[0].resolve({workspace:'A',change:view()}); await p;
    assert.equal(s.state.changeInspection.data,null);
    const fresh=s.api.openChangeInspection('main.rs','working',true);
    assert(!s.requests[1].url.includes('expected_snapshot'));
    s.requests[1].resolve({workspace:'A',change:view()}); await fresh;
    assert(s.state.changeInspection.data);
  });
  await test('close aborts and late result cannot restore source', async()=>{
    const s=setup(), p=s.api.openChangeInspection('main.rs'); s.api.clearChangeInspection();
    assert(s.requests[0].options.signal.aborted);
    s.requests[0].resolve({workspace:'A',change:view()}); await p;
    assert.equal(s.state.changeInspection,null); assert.equal(s.host.innerHTML,'');
  });
  await test('captured repository revision shows only matching current proof and risk', async()=>{
    const s=setup(), code='sha256:code', design='sha256:design';
    s.state.project={
      repository_revision:{code,design},
      proof:{revision_code:code,revision_design:design,current_evidence:2,current_passed:1,current_failed:1,current_inconclusive:0,current_disagreed:0,current_verification_plans:1,current_verification_ready:0,current_verification_blocked:1,evidence_scan_truncated:false,acceptance:{total:2,mapped:2,executed:1,passed:1,fresh:1},effective:{total:2,passed:1,failed:1,inconclusive:0,disagreed:0,truncated:false,items:[{subject:'change:'+code,producer:'cargo-test',kind:'test',confidence:'high',result:'fail',timestamp_ms:2,summary:'failing regression'},{subject:'change:'+code,producer:'cargo-test',kind:'test',confidence:'high',result:'pass',timestamp_ms:1,summary:'targeted checks passed'}]}},
      risk:{workspace:'A',revision:{code,design},level:'high',risks:[{id:'RISK-1',subject:'component:intelligence-ui',category:'reliability',level:'high',summary:'stale evidence can mislead review',signals:['revision mismatch']}],bug_patterns:{precision:'heuristic-regex-candidate',patterns_scanned:4,matches:1,files:1,findings:[],truncated:false},drift:{workspace:'A',design_changed:false,implementation_changed:true,implementation_drift:1,design_drift:0,runtime_drift:0,findings:[],truncated:false}}
    };
    let p=s.api.openChangeInspection('main.rs');
    s.requests[0].resolve({workspace:'A',repository_revision:{code,design},change:view(),symbol_impact:impact()}); await p;
    assert.match(s.host.innerHTML,/Captured-revision verification/);
    assert.match(s.host.innerHTML,/1 passed/);
    assert.match(s.host.innerHTML,/1 failed/);
    assert.match(s.host.innerHTML,/Acceptance: 2\/2 mapped · 1 executed · 1 passed · 1 current-revision/);
    assert.match(s.host.innerHTML,/targeted checks passed/);
    assert.match(s.host.innerHTML,/Matched project risk/);
    assert.match(s.host.innerHTML,/high risk/);
    assert.match(s.host.innerHTML,/stale evidence can mislead review/);
    const stale=setup(); stale.state.project=s.state.project;
    p=stale.api.openChangeInspection('main.rs');
    stale.requests[0].resolve({workspace:'A',repository_revision:{code:'sha256:newer',design},change:view(),symbol_impact:impact()}); await p;
    assert.match(stale.host.innerHTML,/No matching current-version proof/);
    assert(!stale.host.innerHTML.includes('targeted checks passed'));
    assert(!stale.host.innerHTML.includes('stale evidence can mislead review'));
    const staleRisk=setup();
    staleRisk.state.project={...s.state.project,risk:{...s.state.project.risk,revision:{code:'sha256:older-risk',design}}};
    p=staleRisk.api.openChangeInspection('main.rs');
    staleRisk.requests[0].resolve({workspace:'A',repository_revision:{code,design},change:view(),symbol_impact:impact()}); await p;
    assert.match(staleRisk.host.innerHTML,/Matching project risk status is unavailable/);
    assert(!staleRisk.host.innerHTML.includes('stale evidence can mislead review'));
  });
  await test('captured proof fails closed when counts, items, or plan readiness contradict each other', async()=>{
    const code='sha256:code', design='sha256:design';
    const base={revision_code:code,revision_design:design,current_evidence:1,current_passed:1,current_failed:0,current_inconclusive:0,current_disagreed:0,current_verification_plans:1,current_verification_ready:1,current_verification_blocked:0,evidence_scan_truncated:false,acceptance:{total:1,mapped:1,executed:1,passed:1,fresh:1},effective:{total:1,passed:1,failed:0,inconclusive:0,disagreed:0,truncated:false,items:[{subject:'change:'+code,producer:'cargo-test',kind:'test',confidence:'high',result:'pass',timestamp_ms:1,summary:'trusted current proof'}]}};
    for (const proof of [
      {...base,effective:{...base.effective,total:2}},
      {...base,effective:{...base.effective,passed:0,failed:0}},
      {...base,effective:{...base.effective,items:[{...base.effective.items[0],result:'maybe'}]}},
      {...base,current_verification_ready:1,current_verification_blocked:1},
      {...base,acceptance:{...base.acceptance,total:0,mapped:1}},
      {...base,acceptance:{...base.acceptance,executed:2}},
      {...base,acceptance:{...base.acceptance,passed:2}},
      {...base,acceptance:{...base.acceptance,fresh:2}},
      {...base,acceptance:{...base.acceptance,total:'1'}},
    ]) {
      const s=setup();
      s.state.project={repository_revision:{code,design},proof,risk:{workspace:'A',revision:{code,design},level:'low',risks:[],bug_patterns:{matches:0,truncated:false},drift:{truncated:false}}};
      const p=s.api.openChangeInspection('main.rs');
      s.requests[0].resolve({workspace:'A',repository_revision:{code,design},change:view(),symbol_impact:impact()}); await p;
      assert.match(s.host.innerHTML,/No matching current-version proof/);
      assert(!s.host.innerHTML.includes('trusted current proof'));
      assert(!s.host.innerHTML.includes('Matched project risk'));
    }
  });
  await test('counterpart-only syntax rows are explicit and malformed counterpart metadata fails closed', async()=>{
    const s=setup(), value=view(), paired=impact({
      before_symbols:[{node_id:'symbol:ts:before',name:'changed',qualified_name:'changed',kind:'function',start_line:4,end_line:4,counterpart_only:false,definition_change:'modified'}],
      after_symbols:[{node_id:'symbol:ts:abc',name:'changed',qualified_name:'changed',kind:'function',start_line:8,end_line:12,counterpart_only:true,definition_change:'modified'}],
      definition_change_basis:'qualified_name_kind_complete_syntax_outlines'
    });
    render(s,value,paired);
    assert.match(s.host.innerHTML,/Paired counterpart · no changed-line overlap/);
    assert(!s.host.innerHTML.includes('data-change-symbol="symbol:ts:abc"'));
    assert(!s.host.innerHTML.includes('data-change-impact="symbol:ts:abc"'));
    s.state.changeInspection={path:'main.rs',snapshotId:value.snapshot_id,repositoryRevision:{code:'sha256:code',design:'sha256:design'},data:value,symbolImpact:paired};
    assert.equal(await s.api.openChangeSymbolInGraph('symbol:ts:abc'),false);
    assert.equal(await s.api.openChangeSymbolImpact('symbol:ts:abc'),false);
    const malformed=setup(), p=malformed.api.openChangeInspection('main.rs');
    malformed.requests[0].resolve({workspace:'A',change:value,symbol_impact:impact({after_symbols:[{node_id:'symbol:ts:abc',name:'changed',qualified_name:'changed',kind:'function',start_line:4,end_line:4,counterpart_only:'yes'}]})});
    await p;
    assert.equal(malformed.state.changeInspection.data,null);
    assert(malformed.state.changeInspection.error);
  });
  await test('real runtime transitions wire invalidation and cache clearing', ()=>{
    for(const name of ['restoreWorkspaceSnapshot','renderProjectPlaceholder','clearWorkspaceView']) {
      const start=runtime.indexOf('function '+name+'('); assert(start>=0);
      assert(runtime.slice(start,start+260).includes('clearChangeInspection()'),name);
    }
    const failure=runtime.indexOf('function showRefreshFailure(');
    assert(runtime.slice(failure,failure+260).includes('invalidateChangeInspection()'));
    const assertInvalidationOrder = text =>
      assert.match(text, /invalidateChangeInspection\(\);\r?\n    state\.project = data;/);
    assertInvalidationOrder(runtime);
    for (const newline of ['\n', '\r\n']) {
      assertInvalidationOrder(`invalidateChangeInspection();${newline}    state.project = data;`);
      assert.throws(() => assertInvalidationOrder(`state.project = data;${newline}    invalidateChangeInspection();`), assert.AssertionError);
      assert.throws(() => assertInvalidationOrder('    state.project = data;'), assert.AssertionError);
    }
    assert(source.includes('data-change-path='));
  });
  console.log(JSON.stringify({results}));
  if(results.some(result=>!result.passed)) process.exitCode=1;
})();
