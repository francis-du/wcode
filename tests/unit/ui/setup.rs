use super::*;
use std::process::Command;

#[test]
fn setup_page_escapes_labels_and_keeps_optional_configuration_progressive() {
    let page = render(
        "<img src=x onerror=alert(1)>{{script}}&\"'",
        2,
        32,
        "test-nonce",
    );
    assert!(!page.contains("<img src=x"));
    assert!(page.contains("&lt;img src=x onerror=alert(1)&gt;{{script}}&amp;&quot;&#39;"));
    assert_eq!(page.matches("<script nonce=\"test-nonce\">").count(), 1);
    assert_eq!(page.matches("<style nonce=\"test-nonce\">").count(), 1);
    for required in [
        "wcode setup",
        "wcode setup --dry-run",
        "wcode mcp-stdio --show-config",
        "<details class=\"card preferences\">",
        "<details class=\"card runtime\">",
        "data-copy=\"remote-endpoint\" disabled",
        "aria-live=\"polite\"",
        "data-i18n",
        "prefers-color-scheme:light",
        ":focus-visible",
        "viewport-fit=cover",
    ] {
        assert!(page.contains(required), "missing setup contract {required}");
    }
    for forbidden in [
        "x-wcode-ui-token",
        "--full-access",
        "--allow-risky-exec",
        "innerHTML",
        "setInterval(",
    ] {
        assert!(
            !page.contains(forbidden),
            "unexpected privileged or unsafe surface: {forbidden}"
        );
    }
}

#[test]
fn setup_interactions_handle_copy_language_visibility_and_failed_refresh() {
    let root = tempfile::tempdir().unwrap();
    let script = root.path().join("setup.cjs");
    std::fs::write(&script, SCRIPT).unwrap();
    let harness = r#"
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
class Element{
  constructor(){this.textContent='';this.value='';this.disabled=false;this.dataset={};this.events={};this.children=[];}
  addEventListener(event,handler){this.events[event]=handler;}
  replaceChildren(){this.children=[];} appendChild(child){this.children.push(child);} focus(){this.focused=true;}
}
const nodes={},ids=['remote-endpoint','copy-endpoint','remote-status','endpoints','mode','performance','launch-command','preview-command','preset-note','language','copy-status','setup-command','setup-preview-command','access','access-note','client-config'];
ids.forEach(id=>nodes[id]=new Element());nodes.mode.value='local';nodes.performance.value='balanced';nodes.access.value='standard';nodes['setup-command'].textContent='wcode setup';
nodes['copy-endpoint'].dataset.copy='remote-endpoint';const setupCopy=new Element();setupCopy.dataset.copy='setup-command';
const events={},timers=new Map(),requests=[],copied=[];let timerId=0,selected=false;
const document={hidden:false,documentElement:{},getElementById:id=>{assert.ok(nodes[id],id);return nodes[id];},
  querySelectorAll:selector=>selector==='[data-copy]'?[setupCopy,nodes['copy-endpoint']]:[],
  createElement:()=>new Element(),addEventListener:(event,handler)=>events[event]=handler,
  createRange:()=>({selectNodeContents:()=>{selected=true;}})};
const navigator={clipboard:{writeText:async text=>copied.push(text)}};
const context={document,navigator,URL,AbortController,Set,console,
  window:{getSelection:()=>({removeAllRanges(){},addRange(){}})},
  setTimeout:(handler,delay)=>{timers.set(++timerId,{handler,delay});return timerId;},
  clearTimeout:id=>timers.delete(id),
  fetch:(url,options)=>new Promise((resolve,reject)=>{
    assert.equal(url,'/setup/status');assert.equal(options.cache,'no-store');
    requests.push({resolve,reject});options.signal.addEventListener('abort',()=>reject(new Error('aborted')));
  })};
vm.runInNewContext(fs.readFileSync(process.argv[1],'utf8'),context);
const flush=()=>new Promise(setImmediate);
async function respond(data){requests.at(-1).resolve({ok:true,json:async()=>data});await flush();}
function poll(){const entry=[...timers].find(([,item])=>item.delay===6000);assert.ok(entry);timers.delete(entry[0]);entry[1].handler();}
(async()=>{
  assert.equal(requests.length,1);assert.equal(nodes['copy-endpoint'].disabled,true);
  assert.equal(nodes['launch-command'].textContent,'wcode mcp-stdio');
  await respond({ok:true,public_endpoint:'local-only'});assert.equal(nodes['copy-endpoint'].disabled,true);
  poll();await respond({ok:true,public_url_healthy:true,mcp_url:'https://example.test/mcp',tunnels:[]});
  assert.equal(nodes['copy-endpoint'].disabled,false);
  await nodes['copy-endpoint'].events.click();assert.equal(copied.at(-1),'https://example.test/mcp');
  nodes.language.events.click();assert.equal(document.documentElement.lang,'zh-CN');
  nodes.mode.value='remote';nodes.performance.value='fast';nodes.performance.events.change();
  assert.equal(nodes['launch-command'].textContent,'wcode --performance fast');
  assert.equal(nodes['preview-command'].textContent,'wcode --performance fast --show-config');
  assert.equal(nodes['setup-command'].textContent,'wcode setup --performance fast');
  assert.equal(nodes['setup-preview-command'].textContent,'wcode setup --performance fast --dry-run');
  nodes.access.value='inspect';nodes.access.events.change();
  assert.equal(nodes['setup-command'].textContent,'wcode setup --performance fast --read-only --no-exec --no-semantic');
  assert.deepEqual(JSON.parse(nodes['client-config'].textContent),{command:'wcode',args:['mcp-stdio','--performance','fast','--read-only','--no-exec','--no-semantic']});
  assert.ok(nodes['access-note'].textContent.includes('只读'));
  nodes.mode.value='local-http';nodes.mode.events.change();
  assert.equal(nodes['launch-command'].textContent,'wcode --no-tunnel --performance fast --read-only --no-exec --no-semantic');
  nodes.access.value='standard';nodes.access.events.change();
  navigator.clipboard=null;await setupCopy.events.click();assert.ok(selected);assert.ok(nodes['copy-status'].textContent.includes('手动复制'));
  poll();requests.at(-1).reject(new Error('offline'));await flush();
  assert.equal(nodes['copy-endpoint'].disabled,true);assert.ok(!nodes['remote-endpoint'].textContent.includes('example.test'));
  poll();const count=requests.length;document.hidden=true;events.visibilitychange();await flush();
  assert.equal(timers.size,0);assert.equal(requests.length,count);
  document.hidden=false;events.visibilitychange();assert.equal(requests.length,count+1);
  await respond({ok:true,public_endpoint:'local-only'});assert.equal(nodes['copy-endpoint'].disabled,true);
  document.hidden=true;events.visibilitychange();assert.equal(timers.size,0);
  console.log('setup interactions passed');
})().catch(error=>{console.error(error);process.exitCode=1;});
"#;
    let output = Command::new("node")
        .arg("-e")
        .arg(harness)
        .arg(&script)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("setup interactions passed"));
}

#[test]
fn setup_commands_and_endpoint_selection_run_in_javascript() {
    let root = tempfile::tempdir().unwrap();
    let script = root.path().join("setup.cjs");
    std::fs::write(&script, SCRIPT).unwrap();
    let harness = r#"
const assert=require('node:assert/strict');
const ui=require(process.argv[1]);
for(const mode of ['local','remote']){
  const base=mode==='local'?'wcode mcp-stdio':'wcode';
  assert.equal(ui.buildCommand(mode,'balanced'),base);
  for(const preset of ['fast','light'])assert.equal(ui.buildCommand(mode,preset),base+' --performance '+preset);
  assert.equal(ui.buildCommand(mode,'fast; rm -rf anything'),base);
}
for(const preset of ['balanced','fast','light']){
  const suffix=preset==='balanced'?'':' --performance '+preset;
  assert.equal(ui.buildSetup(preset,'standard'),'wcode setup'+suffix);
  assert.equal(ui.buildSetup(preset,'inspect'),'wcode setup'+suffix+' --read-only --no-exec --no-semantic');
  assert.equal(ui.buildCommand('local-http',preset,'read-only'),'wcode --no-tunnel'+suffix+' --read-only');
  assert.equal(ui.clientConfig(preset,'inspect').command,'wcode');
  assert.deepEqual(ui.clientConfig(preset,'inspect').args,ui.buildCommand('local',preset,'inspect').split(' ').slice(1));
}
assert.equal(ui.buildSetup('fast; injected','--full-access'),'wcode setup');
for(const value of ['http://example.test/mcp','https://localhost/mcp','https://127.0.0.1/mcp',
  'https://[::1]/mcp','https://user:password@example.test/mcp','https://example.test/mcp?key=x',
  'https://example.test/mcp#token=x','javascript:alert(1)','not a URL'])assert.equal(ui.publicEndpoint(value),null);
assert.equal(ui.selectEndpoint({ok:true,mcp_url:'http://127.0.0.1:8765/mcp',public_endpoint:'local-only'}),null);
const tunnel={provider:'test',mcp_url:'https://example.test/mcp'};
assert.equal(ui.selectEndpoint({ok:true,tunnels:[tunnel]}),tunnel.mcp_url);
assert.equal(ui.selectEndpoint({ok:true,public_url_healthy:false,tunnels:[tunnel]}),null);
assert.equal(ui.selectEndpoint({ok:true,public_url_healthy:null,mcp_url:tunnel.mcp_url}),null);
assert.equal(ui.selectEndpoint({ok:true,public_url_healthy:true,mcp_url:tunnel.mcp_url}),tunnel.mcp_url);
assert.equal(ui.selectEndpoint({ok:false,tunnels:[tunnel]}),null);
assert.equal(ui.selectEndpoint(null),null);
console.log('setup behavior passed');
"#;
    let output = Command::new("node")
        .arg("-e")
        .arg(harness)
        .arg(&script)
        .output()
        .expect("node is required for the setup JavaScript contract");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("setup behavior passed"));
}
