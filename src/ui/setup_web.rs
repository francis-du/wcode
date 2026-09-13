//! Public connection guide. This page generates commands; it never changes
//! configuration, starts a process, or carries an operator authorization token.

pub(crate) fn render(workspace: &str, roots: usize, slots: usize, nonce: &str) -> String {
    let values = [
        ("workspace", html_escape(workspace)),
        ("roots", roots.to_string()),
        ("slots", slots.to_string()),
        ("nonce", html_escape(nonce)),
        ("style", STYLE.to_owned()),
        ("script", SCRIPT.to_owned()),
        ("docs", html_escape(crate::DOCS_URL)),
        ("project", html_escape(crate::PROJECT_URL)),
        ("author", html_escape(crate::AUTHOR_URL)),
        ("chatgpt", html_escape(crate::CHATGPT_CONNECTOR_SETUP_URL)),
        ("claude", html_escape(crate::CLAUDE_CONNECTOR_SETUP_URL)),
        ("grok", html_escape(crate::GROK_CONNECTOR_SETUP_URL)),
        ("mistral", html_escape(crate::MISTRAL_CONNECTOR_SETUP_URL)),
    ];
    // Expand only the template, never a substituted workspace name or URL.
    let mut page = String::with_capacity(PAGE.len() + STYLE.len() + SCRIPT.len());
    let mut rest = PAGE;
    while let Some((before, tail)) = rest.split_once("{{") {
        page.push_str(before);
        let (key, after) = tail.split_once("}}").expect("closed static template token");
        let value = values
            .iter()
            .find(|(name, _)| *name == key)
            .expect("known template token");
        page.push_str(&value.1);
        rest = after;
    }
    page.push_str(rest);
    page
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

const PAGE: &str = r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<meta name="color-scheme" content="dark light"><meta name="theme-color" content="#0b0812">
<title>wcode · Connect & configure</title><style nonce="{{nonce}}">{{style}}</style></head>
<body><main>
<header><a class="brand" href="{{docs}}" target="_blank" rel="noreferrer"><img class="brand-logo" src="/intelligence/logo.svg" alt="wcode"></a><button id="language" type="button" aria-label="Switch language">中文</button></header>
<section class="intro"><span class="eyebrow" data-i18n="ONE PROJECT. YOUR CODING AGENT.">ONE PROJECT. YOUR CODING AGENT.</span>
<h1 data-i18n="Less setup. More building.">Less setup. More building.</h1>
<p data-i18n="Choose how your agent connects. Defaults are ready; permissions stay in your control.">Choose how your agent connects. Defaults are ready; permissions stay in your control.</p></section>
<div class="connections">
<section class="card local" aria-labelledby="local-title"><span class="badge" data-i18n="RECOMMENDED · SAME MACHINE">RECOMMENDED · SAME MACHINE</span>
<h2 id="local-title" data-i18n="Local coding agent">Local coding agent</h2>
<p data-i18n="Run this once in your terminal, choose the setup scope, then reconnect your agent.">Run this once in your terminal, choose the setup scope, then reconnect your agent.</p>
<div class="command"><code id="setup-command" tabindex="0">wcode setup</code><button type="button" data-copy="setup-command" data-i18n="Copy">Copy</button></div>
<p class="hint" data-i18n="The agent supplies its project directory. No URL, port or API key to enter.">The agent supplies its project directory. No URL, port or API key to enter.</p>
<details><summary data-i18n="Preview before configuring">Preview before configuring</summary><code id="setup-preview-command" class="example">wcode setup --dry-run</code><p class="hint" data-i18n="Shows proposed changes without writing files. Global installation still asks for confirmation.">Shows proposed changes without writing files. Global installation still asks for confirmation.</p></details>
</section>
<section class="card" aria-labelledby="remote-title"><span class="eyebrow" data-i18n="CLOUD OR WEB CLIENT">CLOUD OR WEB CLIENT</span>
<h2 id="remote-title" data-i18n="Remote connector">Remote connector</h2>
<p data-i18n="Add the verified MCP endpoint in your client, then complete its authorization flow.">Add the verified MCP endpoint in your client, then complete its authorization flow.</p>
<p id="remote-status" role="status" data-i18n="Checking public connectivity…">Checking public connectivity…</p>
<div class="command"><code id="remote-endpoint" tabindex="0" data-i18n="No verified public endpoint yet">No verified public endpoint yet</code><button id="copy-endpoint" type="button" data-copy="remote-endpoint" disabled data-i18n="Copy">Copy</button></div>
<div class="clients"><a href="{{chatgpt}}" target="_blank" rel="noreferrer">ChatGPT ↗</a><a href="{{claude}}" target="_blank" rel="noreferrer">Claude ↗</a><a href="{{grok}}" target="_blank" rel="noreferrer">Grok ↗</a><a href="{{mistral}}" target="_blank" rel="noreferrer">Mistral ↗</a></div>
<details><summary data-i18n="Other verified endpoints">Other verified endpoints</summary><div id="endpoints" aria-live="polite"></div></details>
<p class="hint" data-i18n="Localhost is not a cloud endpoint. Temporary tunnel addresses can change after reconnecting.">Localhost is not a cloud endpoint. Temporary tunnel addresses can change after reconnecting.</p></section>
</div>
<details class="card preferences"><summary data-i18n="Manual connection & performance">Manual connection & performance</summary>
<p data-i18n="Optional. Build a command for the next process; this does not change the running instance or save settings.">Optional. Build a command for the next process; this does not change the running instance or save settings.</p>
<div class="fields">
<div class="choice-field"><span class="choice-label" data-i18n="Connection">Connection</span><div class="segmented" role="group" aria-label="Connection"><button type="button" data-choice="mode" data-value="local" aria-pressed="true" data-i18n="Local agent (stdio)">Local agent (stdio)</button><button type="button" data-choice="mode" data-value="remote" aria-pressed="false" data-i18n="Remote service">Remote service</button><button type="button" data-choice="mode" data-value="local-http" aria-pressed="false" data-i18n="Local dashboard · no public tunnel">Local dashboard · no public tunnel</button></div><select id="mode" class="control-model" aria-hidden="true" tabindex="-1"><option value="local">Local agent (stdio)</option><option value="remote">Remote service</option><option value="local-http">Local dashboard · no public tunnel</option></select></div>
<div class="choice-field"><span class="choice-label" data-i18n="Performance">Performance</span><div class="segmented" role="group" aria-label="Performance"><button type="button" data-choice="performance" data-value="balanced" aria-pressed="true" data-i18n="Balanced · recommended">Balanced · recommended</button><button type="button" data-choice="performance" data-value="fast" aria-pressed="false" data-i18n="Fast · larger resource budget">Fast · larger resource budget</button><button type="button" data-choice="performance" data-value="light" aria-pressed="false" data-i18n="Light · smaller resource budget">Light · smaller resource budget</button></div><select id="performance" class="control-model" aria-hidden="true" tabindex="-1" aria-describedby="preset-note"><option value="balanced">Balanced · recommended</option><option value="fast">Fast · larger resource budget</option><option value="light">Light · smaller resource budget</option></select></div>
<div class="choice-field"><span class="choice-label" data-i18n="Access">Access</span><div class="segmented" role="group" aria-label="Access"><button type="button" data-choice="access" data-value="standard" aria-pressed="true" data-i18n="Standard · ask when needed">Standard · ask when needed</button><button type="button" data-choice="access" data-value="read-only" aria-pressed="false" data-i18n="Read-only · commands available">Read-only · commands available</button><button type="button" data-choice="access" data-value="inspect" aria-pressed="false" data-i18n="Inspect only · no commands">Inspect only · no commands</button></div><select id="access" class="control-model" aria-hidden="true" tabindex="-1" aria-describedby="access-note"><option value="standard">Standard · ask when needed</option><option value="read-only">Read-only · commands available</option><option value="inspect">Inspect only · no commands</option></select></div>
</div>
<p id="access-note" class="hint" role="status"></p>
<p class="hint" data-i18n="The setup command above follows these performance and access choices. Run it to save the launch options, then reconnect your agent.">The setup command above follows these performance and access choices. Run it to save the launch options, then reconnect your agent.</p>
<p id="preset-note" class="hint"></p><div class="command"><code id="launch-command" tabindex="0">wcode mcp-stdio</code><button type="button" data-copy="launch-command" data-i18n="Copy">Copy</button></div>
<p class="hint" data-i18n="In a local agent, use command wcode and arguments mcp-stdio. Optional preset arguments can follow it.">In a local agent, use command wcode and arguments mcp-stdio. Optional preset arguments can follow it.</p>
<details><summary data-i18n="Manual client fields">Manual client fields</summary><p class="hint" data-i18n="Local stdio server entry only. Merge it into your client's MCP settings; do not replace the entire configuration file.">Local stdio server entry only. Merge it into your client's MCP settings; do not replace the entire configuration file.</p><div class="command"><code id="client-config" tabindex="0"></code><button type="button" data-copy="client-config" data-i18n="Copy">Copy</button></div></details>
<p class="hint"><span data-i18n="Inspect resolved settings without starting:">Inspect resolved settings without starting:</span> <code id="preview-command">wcode mcp-stdio --show-config</code></p></details>
<details class="card runtime"><summary data-i18n="Current runtime details">Current runtime details</summary><dl><div><dt data-i18n="Default workspace">Default workspace</dt><dd>{{workspace}}</dd></div><div><dt data-i18n="Workspace roots">Workspace roots</dt><dd>{{roots}}</dd></div><div><dt data-i18n="Tool capacity">Tool capacity</dt><dd>{{slots}}</dd></div></dl><p class="hint" data-i18n="Presets never approve commands. Manage requests in the TUI or protected WebUI.">Presets never approve commands. Manage requests in the TUI or protected WebUI.</p></details>
<p id="copy-status" class="feedback" role="status" aria-live="polite"></p>
<noscript><p>JavaScript is disabled. Use <code>wcode setup</code> locally, or copy a verified public MCP URL from the TUI. 公网地址请从终端面板复制。</p></noscript>
<footer><a href="{{docs}}" target="_blank" rel="noreferrer" data-i18n="Documentation ↗">Documentation ↗</a><a href="{{project}}" target="_blank" rel="noreferrer" data-i18n="Source ↗">Source ↗</a><a href="{{author}}" target="_blank" rel="noreferrer">@francis-du ↗</a></footer>
</main><script nonce="{{nonce}}">{{script}}</script></body></html>"##;

const STYLE: &str = r#"
:root{color-scheme:dark;--bg:#0b0812;--panel:#151020;--surface:#120d1c;--raised:#1c1529;--text:#faf7ff;--soft:#cec5da;--muted:#a89db7;--line:#2f2739;--line2:#493d58;--accent:#8b7cff;--accentStrong:#665cff;--accent2:#f05aa6;--code:#120d1c;--glass:rgba(21,16,32,.74);--glass2:rgba(28,21,41,.86);--brand:linear-gradient(112deg,#665cff,#9b4dff 55%,#f43f8f);--fontSans:"Inter","Noto Sans SC","Noto Sans",sans-serif;--fontMono:"JetBrains Mono","Noto Sans Mono",monospace}
*{box-sizing:border-box}body{margin:0;min-height:100dvh;background:radial-gradient(760px 420px at 82% -10%,rgba(102,92,255,.14),transparent 70%),radial-gradient(600px 420px at -10% 55%,rgba(240,90,166,.06),transparent 72%),var(--bg);color:var(--text);font-family:var(--fontSans);font-size:16px;font-weight:450;line-height:1.58;font-synthesis:none;text-rendering:optimizeLegibility;-webkit-font-smoothing:antialiased;padding:max(22px,env(safe-area-inset-top)) max(20px,env(safe-area-inset-right)) max(20px,env(safe-area-inset-bottom)) max(20px,env(safe-area-inset-left))}
main{width:min(100%,1180px);margin:auto}header,footer,.brand{display:flex;align-items:center;gap:12px}header{justify-content:space-between;min-height:64px;padding:8px 4px 14px;border-bottom:1px solid var(--line)}.brand{gap:8px}.brand-logo{display:block;width:124px;height:auto;max-width:52vw;filter:drop-shadow(0 8px 22px rgba(102,92,255,.12))}
a{color:var(--soft);text-decoration:none}a:hover{color:var(--text)}button,select,summary,a{touch-action:manipulation}button,select{font:inherit;color:inherit;min-height:40px;border:1px solid var(--line);border-radius:12px;background:var(--glass2);padding:7px 12px;font-size:13px;font-weight:620}button,summary{cursor:pointer}button:hover,select:hover{border-color:var(--line2)}button:disabled{opacity:.45;cursor:not-allowed}:focus-visible{outline:1px solid var(--accent);outline-offset:2px}
.intro{margin:34px 0 22px}.eyebrow,.badge{font-size:12px;font-weight:720;letter-spacing:.09em;color:var(--muted)}.badge{color:var(--accent)}h1{font-size:clamp(34px,4.4vw,48px);line-height:1.08;letter-spacing:-.035em;margin:9px 0 8px;font-weight:720}h2{font-size:19px;line-height:1.3;margin:10px 0 7px}p{color:var(--muted);margin:8px 0}.connections{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:18px}.card{min-width:0;background:var(--glass);border:1px solid var(--line);border-radius:22px;padding:22px;backdrop-filter:blur(20px);box-shadow:0 14px 42px rgba(0,0,0,.12)}.local{border-color:color-mix(in srgb,var(--accent) 38%,var(--line));background:linear-gradient(145deg,color-mix(in srgb,var(--accent) 5%,var(--glass)),var(--glass))}.command{display:grid;grid-template-columns:minmax(0,1fr) auto;align-items:center;gap:8px;min-width:0;background:rgba(18,13,28,.66);border:1px solid var(--line);border-radius:14px;padding:7px 8px;margin:12px 0 8px}.command code{min-width:0;overflow-wrap:anywhere;white-space:pre-wrap;user-select:all}.command button{flex-shrink:0}.hint{font-size:12px;line-height:1.55}code{font:13px/1.5 var(--fontMono);font-variant-ligatures:none}.example{display:block;margin:10px 0;overflow-wrap:anywhere}summary{min-height:36px;align-content:center;font-size:13px;font-weight:650;color:var(--soft)}details[open]>summary{margin-bottom:6px}.clients{display:flex;flex-wrap:wrap;gap:7px 14px;font-size:13px}.clients a,footer a{display:inline-flex;align-items:center;min-height:34px}.preferences,.runtime{margin-top:14px;background:color-mix(in srgb,var(--glass) 56%,transparent);box-shadow:none}.fields{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:14px}.choice-field{position:relative;display:grid;gap:7px;min-width:0}.choice-label{font-size:13px;font-weight:650;color:var(--soft)}.segmented{display:grid;gap:3px;padding:3px;border:1px solid var(--line);border-radius:15px;background:color-mix(in srgb,var(--surface) 74%,transparent)}.segmented button{min-height:38px;border:0;border-radius:11px;background:transparent;color:var(--muted);text-align:left;box-shadow:none}.segmented button[aria-pressed="true"]{background:linear-gradient(180deg,color-mix(in srgb,var(--accentStrong) 30%,var(--raised)),color-mix(in srgb,var(--accentStrong) 14%,var(--raised)));color:var(--text);box-shadow:inset 0 0 0 1px color-mix(in srgb,var(--accent) 38%,transparent)}.control-model{position:absolute!important;width:1px!important;height:1px!important;min-height:0!important;padding:0!important;opacity:0!important;pointer-events:none!important;clip-path:inset(50%)!important}.endpoint{font:12px/1.6 var(--fontMono);overflow-wrap:anywhere;padding:7px 0;border-bottom:1px solid var(--line)}dl{margin:0}dl>div{display:flex;justify-content:space-between;gap:18px;padding:5px 0;border-bottom:1px solid var(--line)}dt{color:var(--muted)}dd{margin:0;overflow-wrap:anywhere;text-align:right}.feedback{min-height:20px;font-size:12px}footer{flex-wrap:wrap;justify-content:flex-end;font-size:12px;color:var(--muted);padding:14px 0}
@media(max-width:720px){.connections,.fields{grid-template-columns:1fr}.intro{margin-top:22px}.card{padding:13px}button,select,summary,.clients a,footer a{min-height:44px}.command button{min-height:38px}}
@media(max-width:440px){.command{grid-template-columns:1fr}.command button{width:100%}.card{padding:11px}dl>div{display:block}dd{text-align:left}}
@media(prefers-color-scheme:light){:root{color-scheme:light;--bg:#f8f6fc;--panel:#fff;--surface:#f6f2fb;--raised:#fff;--text:#21182d;--soft:#493d56;--muted:#62566f;--line:#ddd5e6;--line2:#b9aac8;--accent:#6846d6;--accentStrong:#5533bd;--accent2:#c22b78;--code:#f1edf8;--glass:rgba(255,255,255,.76);--glass2:rgba(255,255,255,.9);--brand:linear-gradient(112deg,#5533bd,#8d42e7 55%,#c22b78)}}
@media(prefers-reduced-motion:reduce){*{scroll-behavior:auto!important}}
"#;

const SCRIPT: &str = r#"
(function(){
'use strict';
function launchOptions(preset,access){
  var args=['fast','light'].includes(preset)?['--performance',preset]:[];
  if(access==='read-only'||access==='inspect')args.push('--read-only');
  if(access==='inspect')args.push('--no-exec','--no-semantic');
  return args;
}
function buildCommand(mode,preset,access){
  var base=mode==='remote'?['wcode']:mode==='local-http'?['wcode','--no-tunnel']:['wcode','mcp-stdio'];
  return base.concat(launchOptions(preset,access)).join(' ');
}
function buildSetup(preset,access){return ['wcode','setup'].concat(launchOptions(preset,access)).join(' ');}
function clientConfig(preset,access){return {command:'wcode',args:['mcp-stdio'].concat(launchOptions(preset,access))};}
function publicEndpoint(value){
  try{var url=new URL(value),host=url.hostname.toLowerCase();
    if(url.protocol!=='https:'||url.username||url.password||url.search||url.hash||url.pathname!=='/mcp')return null;
    if(host==='localhost'||host.endsWith('.localhost')||host==='[::1]'||host==='0.0.0.0'||host.startsWith('127.'))return null;
    return url.href;
  }catch(_){return null;}
}
function selectEndpoint(data){
  if(!data||data.ok!==true||data.public_endpoint==='local-only'||data.public_url_healthy===false)return null;
  var primary=data.public_url_healthy===true?publicEndpoint(data.mcp_url):null;
  if(primary)return primary;
  var tunnels=Array.isArray(data.tunnels)?data.tunnels:[];
  for(var i=0;i<Math.min(tunnels.length,8);i++){var endpoint=publicEndpoint(tunnels[i]&&tunnels[i].mcp_url);if(endpoint)return endpoint;}
  return null;
}
if(typeof module!=='undefined'&&module.exports)module.exports={buildCommand:buildCommand,buildSetup:buildSetup,clientConfig:clientConfig,publicEndpoint:publicEndpoint,selectEndpoint:selectEndpoint};
if(typeof document==='undefined')return;
var language='en';
var zh={
 'ONE PROJECT. YOUR CODING AGENT.':'当前项目，你熟悉的编程智能体。','Less setup. More building.':'少点配置，多写好代码。',
 'Choose how your agent connects. Defaults are ready; permissions stay in your control.':'选好接入方式即可开始。默认配置开箱可用，权限始终由你决定。',
 'RECOMMENDED · SAME MACHINE':'推荐 · 同一台电脑','Local coding agent':'本地编程智能体','Remote connector':'远程连接器','CLOUD OR WEB CLIENT':'云端或网页客户端',
 'Run this once in your terminal, choose the setup scope, then reconnect your agent.':'在终端运行一次，选择配置范围，然后重新连接智能体。',
 'Copy':'复制','The agent supplies its project directory. No URL, port or API key to enter.':'智能体自动传入项目目录。不用填写地址、端口或 API Key。',
 'Preview before configuring':'先预览，不修改','Shows proposed changes without writing files. Global installation still asks for confirmation.':'只展示将修改的文件，不写入配置；全局安装仍需明确确认。',
 'Add the verified MCP endpoint in your client, then complete its authorization flow.':'在客户端添加已验证的 MCP 地址，再完成授权。',
 'Checking public connectivity…':'正在检查公网连接…','No verified public endpoint yet':'暂无已验证的公网地址','Other verified endpoints':'其他已验证地址',
 'Localhost is not a cloud endpoint. Temporary tunnel addresses can change after reconnecting.':'本机地址不能用于云端连接器。临时隧道地址可能在重连后变化。',
 'Manual connection & performance':'手动接入与性能设置','Optional. Build a command for the next process; this does not change the running instance or save settings.':'可选：生成下次启动使用的命令。此操作不会更改当前实例，也不会保存配置。',
 'Local dashboard · no public tunnel':'本地面板 · 不开公网隧道','Access':'访问权限','Standard · ask when needed':'标准 · 必要时请求批准','Read-only · commands available':'文件只读 · 仍可运行命令','Inspect only · no commands':'仅查看 · 不运行命令',
 'File tools cannot write. Commands may still have side effects and retain their approval checks.':'文件工具不能写入；命令仍可能产生副作用，并保留批准检查。',
 'File tools are read-only; commands and language-server execution are disabled.':'文件工具只读，命令及语言服务器执行均已禁用。',
 'Normal coding capabilities; sensitive operations still require approval.':'保留正常编程能力；敏感操作仍需用户批准。',
 'The setup command above follows these performance and access choices. Run it to save the launch options, then reconnect your agent.':'上方安装命令会同步性能与权限选项。执行后会保存启动参数，重新连接智能体后生效。',
 'Manual client fields':'手动填写客户端配置','Local stdio server entry only. Merge it into your client\'s MCP settings; do not replace the entire configuration file.':'这里只是本地 stdio 服务条目，请合并到客户端 MCP 设置中，不要替换整个配置文件。',
 'Connection':'接入方式','Performance':'性能','Local agent (stdio)':'本地智能体（stdio）','Remote service':'远程服务','Balanced · recommended':'均衡 · 推荐','Fast · larger resource budget':'快速 · 较大资源预算','Light · smaller resource budget':'轻量 · 较小资源预算',
 'In a local agent, use command wcode and arguments mcp-stdio. Optional preset arguments can follow it.':'手动配置本地智能体时，命令填 wcode，参数填 mcp-stdio；可在参数后追加性能预设。',
 'Inspect resolved settings without starting:':'只查看最终配置，不启动服务：','Current runtime details':'当前运行信息','Default workspace':'默认工作区','Workspace roots':'工作区数量','Tool capacity':'工具容量',
 'Presets never approve commands. Manage requests in the TUI or protected WebUI.':'性能预设不会批准命令。请在终端面板或受保护 WebUI 管理授权请求。',
 'Documentation ↗':'文档 ↗','Source ↗':'源码 ↗','Verified public endpoint ready':'公网地址已验证，可用于连接',
 'Local-only mode · use a local agent':'当前仅本机模式，请使用本地智能体','Public endpoint unavailable · waiting for recovery':'公网入口不可用，等待运行时恢复',
 'Waiting for a verified public endpoint':'等待已验证的公网地址','Connection status unavailable · do not use stale endpoints':'连接状态暂不可用，请勿继续使用旧地址',
 'Copied.':'已复制。','Clipboard unavailable; text selected for manual copy.':'剪贴板不可用，已选中文本，请手动复制。',
 '32 tool slots · 512 MiB soft memory budget.':'32 个工具槽位，512 MiB 软内存预算。','64 tool slots · 1024 MiB soft memory budget.':'64 个工具槽位，1024 MiB 软内存预算。','16 tool slots · 256 MiB soft memory budget.':'16 个工具槽位，256 MiB 软内存预算。',
 'Hardware and resource limits still apply; this is not a promised speedup.':'仍受硬件与资源限制约束，不代表等比例提速。'
};
function t(text){return language==='zh'?(zh[text]||text):text;}
var timer=null,inflight=null,lastData=null,lastError=false;
var endpoint=document.getElementById('remote-endpoint'),copyEndpoint=document.getElementById('copy-endpoint');
function showHealth(data,failed){
  var selected=selectEndpoint(data);
  endpoint.textContent=selected||t('No verified public endpoint yet');
  copyEndpoint.disabled=!selected;
  var status=failed?'Connection status unavailable · do not use stale endpoints':selected?'Verified public endpoint ready':data&&data.public_endpoint==='local-only'?'Local-only mode · use a local agent':data&&data.public_url_healthy===false?'Public endpoint unavailable · waiting for recovery':'Waiting for a verified public endpoint';
  document.getElementById('remote-status').textContent=t(status);
  var list=document.getElementById('endpoints');list.replaceChildren();
  if(!selected)return;
  var seen=new Set();(Array.isArray(data.tunnels)?data.tunnels:[]).slice(0,8).forEach(function(item){
    var url=publicEndpoint(item&&item.mcp_url);if(!url||seen.has(url))return;seen.add(url);
    var row=document.createElement('div');row.className='endpoint';row.textContent=String(item.provider||'MCP')+' · '+url;list.appendChild(row);
  });
}
function updateCommand(){
  var mode=document.getElementById('mode').value,preset=document.getElementById('performance').value,access=document.getElementById('access').value;
  var command=buildCommand(mode,preset,access);document.getElementById('launch-command').textContent=command;
  var setup=buildSetup(preset,access);document.getElementById('setup-command').textContent=setup;
  document.getElementById('setup-preview-command').textContent=setup+' --dry-run';
  document.getElementById('client-config').textContent=JSON.stringify(clientConfig(preset,access),null,2);
  var accessNote=access==='inspect'?'File tools are read-only; commands and language-server execution are disabled.':access==='read-only'?'File tools cannot write. Commands may still have side effects and retain their approval checks.':'Normal coding capabilities; sensitive operations still require approval.';
  document.getElementById('access-note').textContent=t(accessNote);
  document.getElementById('preview-command').textContent=command+' --show-config';
  var note=preset==='fast'?'64 tool slots · 1024 MiB soft memory budget.':preset==='light'?'16 tool slots · 256 MiB soft memory budget.':'32 tool slots · 512 MiB soft memory budget.';
  document.getElementById('preset-note').textContent=t(note)+' '+t('Hardware and resource limits still apply; this is not a promised speedup.');
}
function translate(){
  document.documentElement.lang=language==='zh'?'zh-CN':'en';
  document.querySelectorAll('[data-i18n]').forEach(function(node){node.textContent=t(node.dataset.i18n);});
  document.getElementById('language').textContent=language==='zh'?'English':'中文';
  updateCommand();showHealth(lastData,lastError);
}
document.getElementById('language').addEventListener('click',function(){language=language==='zh'?'en':'zh';translate();});
function syncChoice(id){var model=document.getElementById(id);document.querySelectorAll('[data-choice="'+id+'"]').forEach(function(button){button.setAttribute('aria-pressed',button.dataset.value===model.value?'true':'false');});}
['mode','performance','access'].forEach(function(id){document.getElementById(id).addEventListener('change',function(){syncChoice(id);updateCommand();});syncChoice(id);});
document.querySelectorAll('[data-choice]').forEach(function(button){button.addEventListener('click',function(){var id=button.dataset.choice,model=document.getElementById(id);model.value=button.dataset.value;syncChoice(id);updateCommand();});});
document.querySelectorAll('[data-copy]').forEach(function(button){button.addEventListener('click',async function(){
  if(button.disabled)return;var target=document.getElementById(button.dataset.copy),feedback=document.getElementById('copy-status');
  try{if(!navigator.clipboard)throw new Error('clipboard unavailable');await navigator.clipboard.writeText(target.textContent);feedback.textContent=t('Copied.');}
  catch(_){var range=document.createRange();range.selectNodeContents(target);var selection=window.getSelection();if(selection){selection.removeAllRanges();selection.addRange(range);}target.focus();feedback.textContent=t('Clipboard unavailable; text selected for manual copy.');}
});});
function schedule(){clearTimeout(timer);timer=document.hidden?null:setTimeout(tick,6000);}
async function tick(){
  if(document.hidden||inflight)return;
  var controller=new AbortController();inflight=controller;var deadline=setTimeout(function(){controller.abort();},5000);
  try{var response=await fetch('/setup/status',{cache:'no-store',signal:controller.signal});if(!response.ok)throw new Error('health unavailable');var data=await response.json();if(data.ok!==true)throw new Error('invalid health');lastData=data;lastError=false;showHealth(data,false);}
  catch(_){lastData=null;lastError=true;showHealth(null,true);}
  finally{clearTimeout(deadline);inflight=null;schedule();}
}
document.addEventListener('visibilitychange',function(){clearTimeout(timer);timer=null;if(document.hidden){if(inflight)inflight.abort();}else if(!inflight){tick();}});
translate();tick();
})();
"#;

#[cfg(test)]
#[path = "../../tests/unit/ui/setup.rs"]
mod tests;
