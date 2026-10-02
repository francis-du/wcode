'use strict';
const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {sandbox} = require('./observatory.cjs');
const root = path.resolve(__dirname, '../../..');
const featureData = () => ({activity:{available:true,recent:[],agent_context:{}},harness:{
  software_intelligence:{engineering_digital_twin:{modes:['calls','dependencies'],max_depth:4},decision_plane:{authority:'advisory_only',shadow_ab:true}},
  repository_scanning:{gitignore:true,dot_ignore:true},observatory:{background_single_flight_refresh:true}}});

function render(language, fixture) {
  const s=sandbox();s.context.fixture=fixture;s.context.language=language;
  s.run('state.language=language;state.activitySnapshot=fixture;renderActivity();');
  return s.node('#resourceStatus').innerHTML;
}
test('functional UI names do not contain release numbers or invented product terms',()=>{
  for(const name of ['src/ui/intelligence_web/page.html','src/ui/intelligence_web/app/i18n.js','src/ui/intelligence_web/app/overview.js','src/ui/monitor/shell.rs']){
    const source=fs.readFileSync(path.join(root,name),'utf8');
    assert.doesNotMatch(source,/WCode\s+v?\d+\.\d+\s+(?:control plane|控制平面)|工程数字孪生|交接血缘|后台单飞|工程适应度|From code to confidence/i,name);
  }
});
test('project and plugin descriptions use concrete function names',()=>{
  for(const name of ['.wcode/project.yaml','.wcode/design/product.yaml','marketplace.json','plugin/marketplace.json','plugin/plugin.json','plugin/.claude-plugin/plugin.json','plugin/.codex-plugin/plugin.json','plugin/.zcode-plugin/plugin.json']){
    const source=fs.readFileSync(path.join(root,name),'utf8');
    if(name.endsWith('.json'))assert.doesNotThrow(()=>JSON.parse(source),name);
    assert.doesNotMatch(source,/engineering[ -]control[ -]plane|shippable with evidence|repository-intelligence substrate|Vibe Coding/i,name);
  }
});
test('English capabilities use functional names and keep model checks separate',()=>{
  const html=render('en',featureData());
  for(const label of ['Repository features','Code relationships','Repository scanning','Background refresh','Model checks','Unknown'])assert.ok(html.includes(label),label);
  assert.doesNotMatch(html,/WCode 0\.8|Digital Twin|Jev decision runtime/);
});
test('Chinese capabilities do not expose English presentation labels',()=>{
  const html=render('zh-CN',featureData());
  for(const label of ['仓库功能','代码关系','仓库扫描','后台刷新','模型检查','仅提供建议','对比模式'])assert.ok(html.includes(label),label);
  assert.doesNotMatch(html,/comparison mode|advisory_only|工程数字孪生/);
});
test('absent or disabled runtime capabilities do not become advertised UI features',()=>{
  for(const harness of [{},{software_intelligence:{engineering_digital_twin:false,decision_plane:false},repository_scanning:false,observatory:false}]){
    const html=render('en',{activity:{available:true,recent:[]},harness});
    assert.doesNotMatch(html,/Repository features|Code relationships|Model checks/);
  }
});
test('presentation cleanup preserves the existing runtime field names',()=>{
  const source=fs.readFileSync(path.join(root,'src/ui/intelligence_web/app/overview.js'),'utf8');
  for(const key of ['engineering_digital_twin','decision_plane','repository_scanning','background_single_flight_refresh','repo_map_cache_hits'])assert.ok(source.includes(key),key);
});
