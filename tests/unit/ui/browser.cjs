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
const init=`\nstate.project=${data};state.current='A';state.autoRefresh=false;state.lastUpdated=Date.now();state.language='en';state.theme='dark';applyTheme();applyLanguage();renderProject(true);activateWorkspaceTab('proof');window.__layoutReady=true;`;
let html=read(base+'page.html').replace('<link rel="stylesheet" href="/intelligence/app.css">','<style>'+styles+'</style>').replace('<script defer src="/intelligence/app.js"></script>','');
html=html.replace('</body>',`<script>${(bundle.slice(0,bundle.indexOf(marker))+init).replace(/<\/script/gi,'<\\/script')}</script></body>`);
const out=path.join(root,'target/wcode-browser-fixture.html');fs.mkdirSync(path.dirname(out),{recursive:true});fs.writeFileSync(out,html);
console.log(JSON.stringify({suite:'browser-fixture',passed:true,fixture:out}));
