'use strict';
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const {sandbox}=require('./observatory.cjs');
const root=path.resolve(process.argv[2]||'.');

function decodeAttribute(value){
  return value.replaceAll('&amp;','&').replaceAll('&quot;','"').replaceAll('&#39;',"'").replaceAll('&lt;','<').replaceAll('&gt;','>');
}
function run(){
  const results=[];
  function test(name,fn){
    try{fn();results.push({name,passed:true});}
    catch(error){results.push({name,passed:false,error:error.stack});}
  }

  test('browser Simplified Chinese preference is honored when no explicit choice was saved',()=>{
    const s=sandbox(false,true,{languages:['zh-CN','en-US']});
    assert.equal(s.run('state.language'),'zh-CN');
  });
  test('saved language overrides browser language',()=>{
    const s=sandbox(false,true,{languages:['zh-CN'],storage:{'wcode.ui.language':'en'}});
    assert.equal(s.run('state.language'),'en');
  });
  test('locale normalization does not silently serve Simplified Chinese to Hant-only browsers',()=>{
    const s=sandbox();
    assert.equal(s.run('initialLanguage(null,["zh-Hant-TW"])'),'en');
    assert.equal(s.run('initialLanguage(null,["zh-Hans-CN"])'),'zh-CN');
  });
  test('every marked WebUI string, placeholder and ARIA label has Simplified Chinese coverage',()=>{
    const page=fs.readFileSync(path.join(root,'src/ui/intelligence_web/page.html'),'utf8');
    const s=sandbox();
    const zh=JSON.parse(s.run('JSON.stringify(translations["zh-CN"])'));
    const keys=[...page.matchAll(/data-i18n(?:-placeholder|-aria-label|-title)?="([^"]+)"/g)]
      .map(match=>decodeAttribute(match[1]));
    const missing=[...new Set(keys.filter(key=>!Object.prototype.hasOwnProperty.call(zh,key)))].sort();
    assert.deepEqual(missing,[]);
    assert.ok(new Set(keys).size>=100,'static localization coverage unexpectedly shrank');
  });
  test('code-graph workbench copy is translated through the shared dictionary',()=>{
    const s=sandbox();
    assert.equal(s.run('translateKey("zh-CN","Code graph")'),'代码图谱');
    assert.equal(s.run('translateKey("zh-CN","Search code graph")'),'搜索代码图谱');
    assert.equal(s.run('translateKey("zh-CN","Code graph observatory")'),'代码图谱观测台');
  });
  test('document title has a Simplified Chinese translation',()=>{
    const s=sandbox();
    assert.equal(s.run('translateKey("zh-CN","wcode · Engineering Observatory")'),'wcode · 工程观测台');
  });
  test('static accessibility and input copy cannot bypass the i18n marker contract',()=>{
    const page=fs.readFileSync(path.join(root,'src/ui/intelligence_web/page.html'),'utf8');
    const missing=[];
    for(const match of page.matchAll(/<[^>]+>/g)){
      const tag=match[0];
      for(const [attribute,marker] of [['aria-label','data-i18n-aria-label'],['title','data-i18n-title'],['placeholder','data-i18n-placeholder']]){
        if(new RegExp('\\b'+attribute+'="[^"]+"').test(tag) &&
           !tag.includes(marker+'=') && !tag.includes('aria-hidden="true"')){
          missing.push(attribute+': '+tag);
        }
      }
    }
    assert.deepEqual(missing,[]);
  });
  test('static visible WebUI copy cannot remain as unmarked English text',()=>{
    const page=fs.readFileSync(path.join(root,'src/ui/intelligence_web/page.html'),'utf8');
    const bare=[];
    for(const match of page.matchAll(/>([^<>]+)</g)){
      const text=match[1].replace(/&nbsp;/g,' ').replace(/\s+/g,' ').trim();
      if(!text || /^[—–·→+\-\d.%/]+$/.test(text)) continue;
      const before=page.slice(Math.max(0,match.index-280),match.index+1);
      const open=before.match(/<[^>]+>$/)?.[0]||'';
      if(/data-i18n=/.test(open) || /^<(?:option|title|svg|path|circle|rect|kbd)\b/i.test(open)) continue;
      if(text==='中') continue;
      bare.push(text+' @ '+open);
    }
    assert.deepEqual(bare,[]);
  });

  const report={suite:'webui-i18n',results};
  console.log(JSON.stringify(report,null,2));
  assert.ok(results.every(item=>item.passed),results.filter(item=>!item.passed).map(item=>item.name+'\n'+item.error).join('\n'));
}
if(require.main===module)run();
