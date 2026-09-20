'use strict';
// Export the complete production page, CSS and renderer bundle, offline.
const fs=require('node:fs'), path=require('node:path'), assert=require('node:assert/strict');
const {project}=require('./observatory.cjs');
const root=path.resolve(process.argv[2]||'.');
const read=p=>fs.readFileSync(path.join(root,p),'utf8');
const base='src/ui/intelligence_web/';
const manifest=read('src/ui/intelligence_web.rs');
function manifestFiles(constant){
  const start=`pub(crate) const ${constant}: &str = concat!(`, from=manifest.indexOf(start);
  assert.ok(from>=0,`missing ${constant} manifest`);
  const to=manifest.indexOf('\n);',from);
  assert.ok(to>from,`unterminated ${constant} manifest`);
  const files=[...manifest.slice(from,to).matchAll(/include_str!\("([^"]+)"\)/g)].map(match=>path.join('src/ui',match[1]));
  assert.ok(files.length>0,`empty ${constant} manifest`);
  return files;
}
const styles=manifestFiles('INTELLIGENCE_CSS').map(read).join('\n');
const bundle=manifestFiles('INTELLIGENCE_JS').map(read).join('\n').replace(/\r\n?/g,'\n');
const marker='\napplyTheme();\napplyLanguage();\nstartObservatory();';
assert.ok(bundle.includes(marker));
const long='long_unbroken_test_identifier_'.repeat(12);
const fixture={...project(),workspace_options:[{id:'A'}],graph_precision:{primary:'syntax',providers:['tree-sitter',long]},
code:{languages:[{name:'rust',files:999999,lines:999999999},{name:long,files:12,lines:2}],product_scopes:[{name:long,files:999999999,lines:999999999},{name:'runtime',files:1,lines:0}],graph_truncated:true},
proof:{current_passed:200,effective:{total:40,passed:0,failed:40,truncated:true,items:Array.from({length:40},(_,i)=>({id:'e'+i,result:'fail',subject:long,producer:long,policy:long,summary:'<script>NOT_EXECUTED</script> '+long,timestamp_ms:1,stage:'deterministic'}))},acceptance:{total:35,fresh:0}},
adaptive_verification:{mode:'combined',base_quick_checks:6,planned_quick_checks:7,full_coverage_unchanged:true,focused_test:{command:'cargo test --locked '+long,island:long,phase:1,reason:long},cost_sentinel:{frontier:[{order:1,command:long,marginal_samples:12,marginal_failures:12,failure_rate_percent:100}],model:long,estimated_savings_ms:1234},cost_evaluation:{activation_state:'blocked',candidate_model:long,baseline_model:long}},
verified_learning:{available:true,records:0},
history:[{id:long,captured_at_ms:1,files_indexed:999999999,nodes:999999999,edges:999999999}],
latest_delta:{from_captured_at_ms:1,to_captured_at_ms:2,added_nodes:200,changed_nodes:42,removed_nodes:1,changed_paths:[long]},
activity:{available:true,active:1,queued:1,recent:[]}};
const data=JSON.stringify(fixture).replace(/</g,'\\u003c');
const longGraphLabel='extremely_long_code_graph_symbol_name_for_adversarial_layout_';
const codeGraphNodes=[
  ...Array.from({length:8},(_,i)=>({node:{id:'node:caller-'+i,kind:'function',label:(i===7?longGraphLabel.repeat(3):'caller_'+i),attributes:{path:'src/very/long/module/path/'+longGraphLabel+i+'.rs'},provenance:{precision:i%2?'semantic':'syntax',provider:i%2?'lsp':'tree-sitter',revision:'browser-up-'+i}},distance:i<4?1:2,upstream:true,downstream:false})),
  {node:{id:'node:focus',kind:'function',label:'renderProject',attributes:{path:'src/ui/intelligence_web/app/runtime.js'},provenance:{precision:'semantic',provider:'lsp',revision:'browser-semantic'}},distance:0,upstream:false,downstream:false},
  ...Array.from({length:8},(_,i)=>({node:{id:'node:callee-'+i,kind:'function',label:(i===6?longGraphLabel.repeat(2):'callee_'+i),attributes:{path:'src/ui/intelligence_web/app/'+longGraphLabel+i+'.js'},provenance:{precision:i%2?'syntax':'runtime',provider:i%2?'tree-sitter':'runtime',revision:'browser-down-'+i}},distance:i<4?1:2,upstream:false,downstream:true}))
];
const codeGraphEdges=[
  ...Array.from({length:8},(_,i)=>({from:'node:caller-'+i,to:'node:focus',kind:'calls',provenance:{precision:i%2?'semantic':'syntax',provider:i%2?'lsp':'tree-sitter',revision:'browser-up-edge-'+i}})),
  ...Array.from({length:8},(_,i)=>({from:'node:focus',to:'node:callee-'+i,kind:i%2?'calls':'runtime_calls',provenance:{precision:i%2?'syntax':'runtime',provider:i%2?'tree-sitter':'runtime',revision:'browser-down-edge-'+i}}))
];
const codeGraph=JSON.stringify({
  snapshot_id:'GRAPH-browser',captured_at_ms:Date.now(),provider:'wcode-composite',precision:'mixed',
  query:'renderProject',mode:'all',depth:2,root_ids:['node:focus'],nodes:codeGraphNodes,edges:codeGraphEdges,
  precision_counts:{syntax:8,semantic:4,runtime:4},upstream_nodes:8,downstream_nodes:8,truncated:false
}).replace(/</g,'\\u003c');
const codeGraphOverview=JSON.stringify({
  snapshot_id:'GRAPH-browser',captured_at_ms:Date.now(),provider:'wcode-composite',precision:'mixed',
  files_considered:6,files_indexed:6,files_failed:0,scan_truncated:false,graph_truncated:false,
  total_nodes:17,total_edges:16,total_files:6,
  languages:{rust:2,'java-script':3,python:1},relation_counts:{calls:12,imports:4},
  nodes:[
    {id:'file:src/lib.rs',label:'src/lib.rs',path:'src/lib.rs',language:'rust',symbols:4,degree:3},
    {id:'file:src/runtime.rs',label:'src/runtime.rs',path:'src/runtime.rs',language:'rust',symbols:3,degree:2},
    {id:'file:src/ui/app.js',label:'src/ui/app.js',path:'src/ui/app.js',language:'java-script',symbols:5,degree:4},
    {id:'file:src/ui/runtime.js',label:'src/ui/runtime.js',path:'src/ui/runtime.js',language:'java-script',symbols:4,degree:3},
    {id:'file:src/ui/graph.js',label:'src/ui/graph.js',path:'src/ui/graph.js',language:'java-script',symbols:3,degree:3},
    {id:'file:tools/check.py',label:'tools/check.py',path:'tools/check.py',language:'python',symbols:2,degree:1}
  ],
  edges:[
    {from:'file:src/ui/app.js',to:'file:src/ui/runtime.js',count:3,kinds:{calls:3},precision:{semantic:2,syntax:1}},
    {from:'file:src/ui/runtime.js',to:'file:src/lib.rs',count:2,kinds:{calls:1,imports:1},precision:{semantic:1,syntax:1}},
    {from:'file:src/ui/graph.js',to:'file:src/lib.rs',count:2,kinds:{calls:2},precision:{semantic:1,syntax:1}},
    {from:'file:src/lib.rs',to:'file:src/runtime.rs',count:1,kinds:{calls:1},precision:{syntax:1}},
    {from:'file:tools/check.py',to:'file:src/ui/graph.js',count:1,kinds:{imports:1},precision:{syntax:1}}
  ],
  truncated:false
}).replace(/</g,'\\u003c');
const init=`\nstate.project=${data};state.current='A';state.autoRefresh=false;state.lastUpdated=Date.now();state.language='en';state.theme='dark';state.codeGraph=${codeGraph};state.codeGraphOverview=${codeGraphOverview};state.codeGraphView='overview';state.codeGraphWorkspace='A';applyTheme();applyLanguage();renderProject(true);activateWorkspaceTab('proof');window.__layoutReady=true;`;
let html=read(base+'page.html').replace('<link rel="stylesheet" href="/intelligence/app.css">','<style>'+styles+'</style>').replace('<script defer src="/intelligence/app.js"></script>','');
html=html.replace('</body>',`<script>${(bundle.slice(0,bundle.indexOf(marker))+init).replace(/<\/script/gi,'<\\/script')}</script></body>`);
const out=path.join(root,'target/wcode-browser-fixture.html');fs.mkdirSync(path.dirname(out),{recursive:true});fs.writeFileSync(out,html);
console.log(JSON.stringify({suite:'browser-fixture',passed:true,fixture:out}));