'use strict';
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const {test}=require('node:test');
const {spawnSync}=require('node:child_process');
const root=path.resolve(__dirname,'../../..');
const audit=require(path.join(root,'tests/release_audit.cjs'));

function fixture(run) {
  fs.mkdirSync(path.join(root,'target'),{recursive:true});
  const directory=fs.mkdtempSync(path.join(root,'target/release-audit-test-'));
  function write(relative,content='before') {
    const filename=path.join(directory,relative);
    fs.mkdirSync(path.dirname(filename),{recursive:true});
    fs.writeFileSync(filename,content);
  }
  for(const area of ['src','tests','.wcode/design','docs/manual','plugin','.github/workflows','crates','examples','.cargo']) {
    fs.mkdirSync(path.join(directory,area),{recursive:true});
  }
  for(const name of ['Cargo.toml','Cargo.lock','marketplace.json','README.md','LICENSE','NOTICE','install.sh','install.ps1','.gitattributes','.gitignore','.wcode/project.yaml','.wcode/executors.yaml','.wcode/architecture.toml','.cargo/audit.toml']) write(name);
  try { return run(directory,write); }
  finally { fs.rmSync(directory,{recursive:true,force:true}); }
}

for(const filename of [
  'crates/core-types/src/lib.rs','crates/core-types/src/reports.rs','crates/core-types/Cargo.toml',
  'src/ui/font.woff2','src/embedded.bin','tests/fixture.py','tests/fixtures/runner',
  'install.sh','install.ps1','examples/git_publisher.rs','.cargo/config.toml',
  '.wcode/executors.yaml','.wcode/architecture.toml','.gitattributes','README.md','LICENSE',
]) {
  test(`audit input identity includes ${filename}`,()=>fixture((directory,write)=>{
    write(filename,'before');
    const before=audit.digest(directory);
    write(filename,'after!');
    assert.notEqual(audit.digest(directory).sha256,before.sha256,`Unchanged identity after ${filename} changed`);
  }));
}

test('audit input identity is stable for unchanged files',()=>fixture((directory,write)=>{
  write('src/main.rs','fn main() {}');
  assert.deepEqual(audit.digest(directory),audit.digest(directory));
}));
test('audit input identity rejects symbolic links',()=>fixture((directory,write)=>{
  write('src/actual.rs');
  if(process.platform==='win32') {
    // A directory junction exercises the same link rejection without requiring
    // Windows Developer Mode or the symbolic-link creation privilege.
    fs.symlinkSync(path.join(directory,'src'),path.join(directory,'tests/alias'),'junction');
  } else fs.symlinkSync('actual.rs',path.join(directory,'src/alias.rs'));
  assert.throws(()=>audit.digest(directory),/symlink|symbolic/i);
}));
test('audit input identity rejects hard-linked source aliases',()=>fixture((directory,write)=>{
  write('src/actual.rs');
  fs.linkSync(path.join(directory,'src/actual.rs'),path.join(directory,'src/alias.rs'));
  assert.throws(()=>audit.digest(directory),/link|alias/i);
}));
test('audit input identity ignores generated Python cache bytes',()=>fixture((directory,write)=>{
  write('tests/fixture.py');
  const before=audit.digest(directory);
  write('tests/__pycache__/fixture.cpython-313.pyc');
  assert.deepEqual(audit.digest(directory),before);
}));

test('directory changes remain blocking and report which metadata changed',()=>fixture((directory,write)=>{
  write('tests/unit/ui/fixture.cjs');
  const selected=path.join(directory,'tests/unit/ui');
  const readdir=fs.readdirSync;
  fs.readdirSync=function(filename,...args) {
    const entries=readdir.call(this,filename,...args);
    if(filename===selected) fs.utimesSync(selected,new Date(0),new Date(1000));
    return entries;
  };
  try { assert.throws(()=>audit.digest(directory),/directory changed.*tests[/\\]unit[/\\]ui.*mtimeNs/); }
  finally { fs.readdirSync=readdir; }
}));

const inputBefore={sha256:'unchanged-input'};
const gitBefore={head:'unchanged-head',status_sha256:'unchanged-status'};
test('final audit scan preserves a successful unchanged snapshot',()=>{
  const result=audit.finalInputState(inputBefore,gitBefore,()=>({...inputBefore}),()=>({...gitBefore}));
  assert.equal(result.stable,true);
  assert.deepEqual(result.failures,[]);
  assert.deepEqual(result.after,inputBefore);
  assert.deepEqual(result.git_after,gitBefore);
});
test('a final scan failure is retained as failed data rather than losing all completed rounds',()=>{
  let gitRead=false;
  const result=audit.finalInputState(inputBefore,gitBefore,()=>{throw new Error('directory changed');},()=>{gitRead=true;return {...gitBefore};});
  assert.equal(result.stable,false);
  assert.equal(result.after,null);
  assert.equal(gitRead,true);
  assert.equal(result.failures[0].stage,'source_scan');
  assert.match(result.failures[0].message,/directory changed/);
});
test('unreadable or changed final Git state never turns completed rounds green',()=>{
  for(const readGit of [()=>{throw new Error('unavailable');},()=>({...gitBefore,head:'new-head'}),()=>({...gitBefore,status_sha256:'new-status'}),()=>null]) {
    const result=audit.finalInputState(inputBefore,gitBefore,()=>({...inputBefore}),readGit);
    assert.equal(result.stable,false);
  }
  const changed=audit.finalInputState(inputBefore,gitBefore,()=>({sha256:'different-input'}),()=>({...gitBefore}));
  assert.equal(changed.stable,false);
});
test('audit failure diagnostics stay bounded',()=>{
  const result=audit.finalInputState(inputBefore,gitBefore,()=>{throw new Error('x'.repeat(10000));},()=>({...gitBefore}));
  assert.equal(result.stable,false);
  assert.ok(result.failures[0].message.length<=2000);
});

const json={kind:'json',program:'swift',args:['tests/unit/ui/browser_webkit.swift']};
const browser=()=>({suite:'full-browser-adversarial',cases:2,expected_cases:2,total_cases:2,failed_cases:0,failures:0,runner_error:'',results:[{errors:[]},{errors:[]}]});
test('audit accepts a complete successful browser report',()=>{
  assert.equal(audit.casesFromOutput(json,JSON.stringify(browser())),2);
});
for(const [name,mutate] of [
  ['incomplete matrix',r=>{r.expected_cases=256;r.total_cases=256;}],
  ['runner failure',r=>{r.runner_error='deadline expired';}],
  ['contradictory failed summary',r=>{r.failed_cases=1;}],
  ['explicit failed result',r=>{r.results[0].passed=false;}],
  ['passed flag concealing errors',r=>{r.results[0]={passed:true,errors:['overflow']};}],
  ['misreported case count',r=>{r.cases=3;}],
  ['zero reported cases',r=>{r.cases=0;}],
  ['explicit failed top-level report',r=>{r.passed=false;}],
]) {
  test(`audit rejects ${name}`,()=>{
    const report=browser();mutate(report);
    assert.throws(()=>audit.casesFromOutput(json,JSON.stringify(report)));
  });
}
// Independent matrix oracle: a matching row count must not conceal repeated views.
const fullBrowserStep={...json,expected_cases:256,expected_suite:'full-browser-adversarial'};
function fullBrowserMatrix() {
  const results=[];
  const add=(widths,views)=>{
    for(const view of views) for(const width of widths) for(const language of ['en','zh-CN']) for(const theme of ['dark','light']) {
      const graph=view.startsWith('codegraph');
      results.push({scenario:view,width,language,theme,
        tab:graph||view.startsWith('architecture-')?'architecture':view,
        architectureView:graph?'codegraph':view==='architecture-components'?'components':view==='architecture-dependencies'?'graph':'blueprint',
        codeGraphView:view==='codegraph'?'overview':'focus',codeGraphFull:view==='codegraph-full',
        codeGraphInspectorOpen:view==='codegraph-source',fontStatus:'loaded',errors:[]});
    }
  };
  add([320,375,720,900,1024,1240,1280,1440,1461,1597,1676,1920],['proof','overview']);
  add([320,720,1024,1440],['codegraph','codegraph-full','codegraph-source','activity','changes','requirements','files','architecture-blueprint','architecture-components','architecture-dependencies']);
  return {suite:'full-browser-adversarial',cases:256,expected_cases:256,total_cases:256,
    failed_cases:0,failures:0,runner_error:'',results};
}
test('browser matrix accepts all distinct expected scenarios in any order',()=>{
  const report=fullBrowserMatrix();report.results.reverse();
  assert.equal(audit.casesFromOutput(fullBrowserStep,JSON.stringify(report)),256);
});
for(const [name,mutate] of [
  ['duplicate row replacing a missing scenario',r=>{r.results[1]={...r.results[0]};}],
  ['missing requested scenario',r=>{delete r.results[0].scenario;}],
  ['unexpected requested scenario',r=>{r.results[0].scenario='nonexistent';}],
  ['unknown language',r=>{r.results[0].language='fr';}],
  ['viewport never actually resized',r=>{r.results.find(x=>x.width===1920).width=320;}],
  ['fractional viewport',r=>{r.results[0].width+=0.5;}],
  ['wrong active tab',r=>{r.results[0].tab='files';}],
  ['wrong architecture subview',r=>{r.results.find(x=>x.scenario==='architecture-components').architectureView='blueprint';}],
  ['missing graph fullscreen observation',r=>{delete r.results.find(x=>x.scenario==='codegraph-full').codeGraphFull;}],
  ['fullscreen action had no effect',r=>{r.results.find(x=>x.scenario==='codegraph-full').codeGraphFull=false;}],
  ['graph overview silently rendered focus',r=>{r.results.find(x=>x.scenario==='codegraph').codeGraphView='focus';}],
  ['source inspector did not open',r=>{r.results.find(x=>x.scenario==='codegraph-source').codeGraphInspectorOpen=false;}],
  ['font geometry captured before loading',r=>{r.results[0].fontStatus='loading';}],
  ['missing font completion',r=>{delete r.results[0].fontStatus;}],
  ['different runner suite with matching counts',r=>{r.suite='some-other-suite';}],
]) {
  test(`browser matrix rejects ${name}`,()=>{
    const report=fullBrowserMatrix();mutate(report);
    assert.throws(()=>audit.casesFromOutput(fullBrowserStep,JSON.stringify(report)));
  });
}
const layoutStep={kind:'json',program:'swift',args:['tests/unit/ui/layout_webkit.swift'],expected_cases:12,expected_suite:'webkit-layout'};
const layoutMatrix=()=>({suite:'webkit-layout',failures:0,results:[375,720,900,1024,1240,1280,1440,1461,1500,1597,1676,1920].map(width=>({width,errors:[]}))});
test('layout matrix accepts all twelve observed widths',()=>{
  assert.equal(audit.casesFromOutput(layoutStep,JSON.stringify(layoutMatrix())),12);
});
for(const [name,mutate] of [
  ['duplicate observed width',r=>{r.results[1].width=r.results[0].width;}],
  ['missing observed width',r=>{delete r.results[0].width;}],
  ['unexpected viewport width',r=>{r.results[0].width=320;}],
  ['coerced string viewport',r=>{r.results[0].width='375';}],
  ['wrong suite',r=>{r.suite='full-browser-adversarial';r.cases=12;r.expected_cases=12;r.total_cases=12;r.failed_cases=0;r.runner_error='';}],
]) {
  test(`layout matrix rejects ${name}`,()=>{
    const report=layoutMatrix();mutate(report);
    assert.throws(()=>audit.casesFromOutput(layoutStep,JSON.stringify(report)));
  });
}

test('fixture generation is not counted as a browser test',()=>{
  assert.equal(audit.casesFromOutput({kind:'json'},JSON.stringify({passed:true,fixture:'local.html'})),0);
});
test('audit does not use nested Rust test output to conceal a zero-test selection',()=>{
  const stdout='test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n'+
    'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 300 filtered out; finished in 0.00s\n';
  assert.throws(()=>audit.casesFromOutput({kind:'rust'},stdout),/zero|test|result/i);
});
test('audit counts only the outer Rust test summary',()=>{
  const stdout='test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n'+
    'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 300 filtered out; finished in 0.00s\n';
  assert.equal(audit.casesFromOutput({kind:'rust'},stdout),1);
});
test('audit pins expected browser coverage even without optional summary fields',()=>{
  assert.throws(()=>audit.casesFromOutput({kind:'json',expected_cases:256},JSON.stringify({results:[{errors:[]}]})));
  assert.throws(()=>audit.casesFromOutput({kind:'json',expected_cases:256},JSON.stringify({passed:true})));
});
test('audit rejects a nested success followed by a failed outer Rust summary',()=>{
  assert.throws(()=>audit.casesFromOutput({kind:'rust'},
    'test result: ok. 1 passed; 0 failed;\ntest result: FAILED. 0 passed; 1 failed;\n'));
});
test('audit exact Rust selector cannot silently widen',()=>{
  assert.throws(()=>audit.casesFromOutput({kind:'rust',args:['--exact']},'test result: ok. 2 passed; 0 failed;\n'));
});
test('audit options default to all 300 rounds',()=>{
  assert.deepEqual(audit.parseOptions([]),{start:1,end:300,require_clean:false,help:false});
  assert.deepEqual(audit.parseOptions(['--rounds=48-49','--require-clean']),{start:48,end:49,require_clean:true,help:false});
});
for(const args of [['--rounds=0-300'],['--rounds=1-301'],['--rounds=2-1'],['--rounds=1-1','--rounds=2-2'],['--require-clean','--require-clean'],['--rouds=1-2'],['--help','--rounds=1-1']]) {
  test(`audit rejects invalid or ambiguous options ${args.join(' ')}`,()=>assert.throws(()=>audit.parseOptions(args)));
}
test('audit help performs no Cargo lookup or report generation',()=>fixture(directory=>{
  const result=spawnSync(process.execPath,[path.join(root,'tests/release_audit.cjs'),'--help'],{
    cwd:directory,env:{...process.env,PATH:''},encoding:'utf8',timeout:5000,
  });
  assert.equal(result.status,0,result.stderr);
  assert.match(result.stdout,/Usage:/);
  assert.ok(!fs.existsSync(path.join(directory,'target')));
}));
test('unknown audit options fail before any Cargo lookup',()=>fixture(directory=>{
  const result=spawnSync(process.execPath,[path.join(root,'tests/release_audit.cjs'),'--rouds=1-2'],{
    cwd:directory,env:{...process.env,PATH:''},encoding:'utf8',timeout:5000,
  });
  assert.equal(result.status,1);
  assert.match(result.stderr,/Unknown audit option/);
  assert.doesNotMatch(result.stderr,/spawnSync cargo/);
}));
test('audit detects optional build configuration appearing and disappearing',()=>fixture((directory,write)=>{
  const absent=audit.digest(directory);
  write('.cargo/config.toml');
  assert.notEqual(audit.digest(directory).sha256,absent.sha256);
  fs.unlinkSync(path.join(directory,'.cargo/config.toml'));
  assert.deepEqual(audit.digest(directory),absent);
}));
test('audit bounded reads reject oversized binary inputs',()=>fixture((directory,write)=>{
  write('src/oversized.bin');
  fs.truncateSync(path.join(directory,'src/oversized.bin'),8*1024*1024+1);
  assert.throws(()=>audit.digest(directory),/byte bound/);
}));
test('audit rejects a symlinked ancestor of an optional config',()=>fixture(directory=>{
  fs.renameSync(path.join(directory,'.cargo'),path.join(directory,'cargo-original'));
  fs.symlinkSync(path.join(directory,'cargo-original'),path.join(directory,'.cargo'),process.platform==='win32'?'junction':'dir');
  assert.throws(()=>audit.digest(directory),/symlink/);
}));
