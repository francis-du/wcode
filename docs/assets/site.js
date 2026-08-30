const pageIsChinese = document.documentElement.lang.toLowerCase().startsWith('zh');

const capabilityLabels = pageIsChinese ? {
  package: '插件包',
  skill: '通用 Skill',
  stdio: 'stdio',
  http: 'HTTP',
  sse: 'SSE',
  oauth: 'OAuth',
  auto: '一键安装',
  manual: '仅手工'
} : {
  package: 'Package',
  skill: 'Portable skill',
  stdio: 'stdio',
  http: 'HTTP',
  sse: 'SSE',
  oauth: 'OAuth',
  auto: 'Auto install',
  manual: 'Manual only'
};

const clientUi = pageIsChinese ? {
  auto: '可自动配置',
  manual: '需要手工配置',
  account: '账户内配置',
  configured: '已验证配置格式',
  documented: '厂商文档支持',
  version: '依版本而定',
  noResults: '没有匹配的 Host。',
  source: '厂商文档',
  evidence: '依据'
} : {
  auto: 'Auto-configurable',
  manual: 'Manual setup',
  account: 'Account setup',
  configured: 'Config shape tested',
  documented: 'Vendor documented',
  version: 'Version-dependent',
  noResults: 'No matching host.',
  source: 'Vendor documentation',
  evidence: 'Evidence'
};

const yes = true;
const no = false;
const varies = 'varies';

const clients = [
  {
    name: 'Claude Code', mark: 'Cl', tags: ['cli', 'auto'], mode: 'auto',
    package: yes, skill: yes, stdio: yes, http: yes, sse: yes, oauth: yes,
    evidence: 'configured', target: '.mcp.json',
    en: 'Merges the repository MCP entry and leaves every other server intact.',
    zh: '合并仓库内的 MCP 配置，已有服务器原样保留。',
    source: 'https://code.claude.com/docs/en/mcp'
  },
  {
    name: 'OpenAI Codex', mark: '◎', tags: ['cli', 'auto'], mode: 'auto',
    package: yes, skill: yes, stdio: yes, http: yes, sse: no, oauth: yes,
    evidence: 'configured', target: '.codex/config.toml',
    en: 'Adds one project-scoped mcp_servers.wcode table after the project is trusted.',
    zh: '在受信项目中加入一段 mcp_servers.wcode，不改用户级配置。',
    source: 'https://learn.chatgpt.com/docs/extend/mcp?surface=cli'
  },
  {
    name: 'GitHub Copilot CLI', mark: 'GH', tags: ['cli', 'auto'], mode: 'auto',
    package: no, skill: yes, stdio: yes, http: yes, sse: varies, oauth: varies,
    evidence: 'configured', target: '.mcp.json',
    en: 'Installed only when the CLI or an existing project MCP file is detected.',
    zh: '仅在检测到 CLI 或现有项目 MCP 文件时写入。',
    source: 'https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference'
  },
  {
    name: 'VS Code + Copilot', mark: 'VS', tags: ['ide', 'auto'], mode: 'auto',
    package: no, skill: yes, stdio: yes, http: yes, sse: no, oauth: yes,
    evidence: 'configured', target: '.vscode/mcp.json',
    en: 'Uses the workspace servers schema; extension-global state is not touched.',
    zh: '使用工作区 servers 格式，不碰扩展的全局状态。',
    source: 'https://code.visualstudio.com/docs/agents/reference/mcp-configuration'
  },
  {
    name: 'Cursor', mark: 'C', tags: ['ide', 'auto'], mode: 'auto',
    package: no, skill: yes, stdio: yes, http: yes, sse: yes, oauth: yes,
    evidence: 'configured', target: '.cursor/mcp.json',
    en: 'Writes the project file only; remote support still depends on the installed release.',
    zh: '只写项目配置；远程能力仍以当前安装版本为准。',
    source: 'https://docs.cursor.com/context/model-context-protocol'
  },
  {
    name: 'Gemini CLI', mark: 'G', tags: ['cli', 'auto'], mode: 'auto',
    package: no, skill: yes, stdio: yes, http: yes, sse: yes, oauth: yes,
    evidence: 'configured', target: '.gemini/settings.json',
    en: 'Merges mcpServers.wcode into the workspace settings file.',
    zh: '把 mcpServers.wcode 合并进工作区设置。',
    source: 'https://google-gemini.github.io/gemini-cli/docs/cli/tutorials.html'
  },
  {
    name: 'Qwen Code', mark: 'Qw', tags: ['cli', 'auto'], mode: 'auto',
    package: no, skill: yes, stdio: yes, http: yes, sse: yes, oauth: yes,
    evidence: 'configured', target: '.qwen/settings.json',
    en: 'Uses project settings and keeps host approval prompts in place.',
    zh: '使用项目设置，并保留 Host 自己的信任确认。',
    source: 'https://qwenlm.github.io/qwen-code-docs/en/users/features/mcp/'
  },
  {
    name: 'Kiro', mark: 'K', tags: ['ide', 'auto'], mode: 'auto',
    package: no, skill: yes, stdio: yes, http: yes, sse: varies, oauth: yes,
    evidence: 'configured', target: '.kiro/settings/mcp.json',
    en: 'Adds the server without auto-approving any tool.',
    zh: '只注册服务器，不自动批准任何工具。',
    source: 'https://kiro.dev/docs/mcp/configuration/'
  },
  {
    name: 'Qoder CLI', mark: 'Q', tags: ['cli', 'auto'], mode: 'auto',
    package: no, skill: yes, stdio: yes, http: yes, sse: yes, oauth: yes,
    evidence: 'configured', target: '.mcp.json',
    en: 'Shares the project MCP file and preserves unrelated servers.',
    zh: '复用项目 MCP 文件，其他服务器配置不会被覆盖。',
    source: 'https://docs.qoder.com/cli/mcp-reference'
  },
  {
    name: 'Cline', mark: 'Cl', tags: ['ide', 'manual'], mode: 'manual',
    package: no, skill: yes, stdio: yes, http: varies, sse: varies, oauth: varies,
    evidence: 'documented', target: 'Cline MCP settings',
    en: 'MCP state is application-level; use the settings screen or CLI.',
    zh: 'MCP 配置属于应用状态，请使用设置页或 CLI。',
    source: 'https://docs.cline.bot/getting-started/config'
  },
  {
    name: 'Kimi Code CLI', mark: 'Ki', tags: ['cli', 'manual'], mode: 'manual',
    package: no, skill: yes, stdio: yes, http: yes, sse: varies, oauth: yes,
    evidence: 'documented', target: 'Kimi MCP UI or CLI',
    en: 'No stable repository-local merge target was used; add the stdio entry manually.',
    zh: '目前不自动改写本地配置，请在 Kimi 的 MCP 设置中添加 stdio。',
    source: 'https://moonshotai.github.io/kimi-cli/en/customization/mcp.html'
  },
  {
    name: 'OpenCode', mark: 'O', tags: ['cli', 'auto'], mode: 'auto',
    package: no, skill: yes, stdio: yes, http: yes, sse: no, oauth: yes,
    evidence: 'configured', target: 'opencode.json',
    en: 'Detects the V1 or V2 MCP container before merging the local server.',
    zh: '先判断 V1 或 V2 MCP 容器，再合并本地服务器。',
    source: 'https://opencode.ai/v2/docs/mcp-servers/'
  },
  {
    name: 'Roo Code', mark: 'R', tags: ['ide', 'manual'], mode: 'manual',
    package: no, skill: yes, stdio: yes, http: yes, sse: varies, oauth: varies,
    evidence: 'documented', target: 'Workspace MCP settings',
    en: 'Use Roo workspace settings; automatic global-extension edits are intentionally avoided.',
    zh: '通过 Roo 的工作区设置添加；不会自动改全局扩展状态。',
    source: 'https://docs.roocode.com/features/mcp/using-mcp-in-roo'
  },
  {
    name: 'Continue', mark: 'Co', tags: ['ide', 'manual'], mode: 'manual',
    package: no, skill: yes, stdio: yes, http: varies, sse: varies, oauth: varies,
    evidence: 'version', target: 'Continue MCP settings',
    en: 'Common setups use YAML, so wcode does not risk a lossy rewrite.',
    zh: '常见配置是 YAML；无法可靠合并时只给手工步骤。'
  },
  {
    name: 'ZCode', mark: 'Z', tags: ['cli', 'manual'], mode: 'manual',
    package: yes, skill: yes, stdio: yes, http: varies, sse: varies, oauth: varies,
    evidence: 'version', target: 'ZCode marketplace',
    en: 'Install the exported package, then bind MCP to the source repository.',
    zh: '先安装导出的插件包，再把 MCP 绑定到源码仓库。'
  },
  {
    name: 'Grok Build', mark: '✦', tags: ['cli', 'manual'], mode: 'manual',
    package: no, skill: yes, stdio: yes, http: varies, sse: varies, oauth: varies,
    evidence: 'version', target: 'Project agent settings',
    en: 'The portable skill is safe to copy; MCP still needs an explicit repository binding.',
    zh: '通用 Skill 可以复制，MCP 仍需显式绑定当前仓库。'
  },
  {
    name: 'Windsurf', mark: 'W', tags: ['ide', 'manual'], mode: 'manual',
    package: no, skill: yes, stdio: yes, http: yes, sse: yes, oauth: yes,
    evidence: 'documented', target: 'Windsurf MCP settings',
    en: 'Use the MCP settings UI; no unverified repository schema is rewritten.',
    zh: '通过 MCP 设置界面添加，不改写未确认的项目 Schema。',
    source: 'https://docs.windsurf.com/windsurf/cascade/mcp'
  },
  {
    name: 'JetBrains / Junie', mark: 'JB', tags: ['ide', 'manual'], mode: 'manual',
    package: no, skill: yes, stdio: yes, http: varies, sse: varies, oauth: varies,
    evidence: 'version', target: 'IDE MCP settings',
    en: 'IDE and account state stay under JetBrains control.',
    zh: 'IDE 与账户状态继续由 JetBrains 自己管理。'
  },
  {
    name: 'Zed', mark: 'Z', tags: ['ide', 'manual'], mode: 'manual',
    package: no, skill: yes, stdio: yes, http: varies, sse: varies, oauth: varies,
    evidence: 'version', target: 'Zed project settings',
    en: 'Zed settings may be JSONC, so comments and unknown fields are never rewritten.',
    zh: 'Zed 配置可能是 JSONC；安装器不会破坏注释或未知字段。'
  },
  {
    name: 'TRAE', mark: 'T', tags: ['ide', 'manual'], mode: 'manual',
    package: no, skill: yes, stdio: yes, http: yes, sse: yes, oauth: varies,
    evidence: 'version', target: 'TRAE MCP settings',
    en: 'Transport support is not treated as proof of OAuth interoperability.',
    zh: '支持传输协议不等于已经验证 OAuth 互通。'
  },
  {
    name: 'CodeBuddy', mark: 'CB', tags: ['ide', 'manual'], mode: 'manual',
    package: no, skill: yes, stdio: yes, http: varies, sse: varies, oauth: varies,
    evidence: 'version', target: 'CodeBuddy MCP settings',
    en: 'Unknown schemas fail closed and remain untouched.',
    zh: '遇到未知 Schema 会停止，不会试着覆盖。'
  },
  {
    name: 'ChatGPT Web', mark: '◎', tags: ['web', 'account'], mode: 'account',
    package: no, skill: no, stdio: no, http: yes, sse: no, oauth: yes,
    evidence: 'documented', target: 'ChatGPT Connector settings',
    en: 'Paste the current HTTPS /mcp URL and finish OAuth in the browser.',
    zh: '在 Connector 设置中粘贴当前 HTTPS /mcp 地址并完成 OAuth。',
    source: 'https://help.openai.com/en/articles/12584461-developer-mode-and-full-mcp-connectors-in-chatgpt'
  },
  {
    name: 'Claude Web', mark: '◈', tags: ['web', 'account'], mode: 'account',
    package: no, skill: no, stdio: no, http: yes, sse: no, oauth: yes,
    evidence: 'documented', target: 'Claude Integrations',
    en: 'This is account state and cannot be installed safely from repository files.',
    zh: '这是账户级设置，不能从仓库文件安全代装。',
    source: 'https://support.claude.com/en/articles/11175166-get-started-with-custom-connectors-using-remote-mcp'
  },
  {
    name: 'Grok Web', mark: '✦', tags: ['web', 'account'], mode: 'account',
    package: no, skill: no, stdio: no, http: yes, sse: no, oauth: yes,
    evidence: 'documented', target: 'Grok Connectors',
    en: 'Add the current tunnel URL in Grok; wcode never stores the resulting token.',
    zh: '在 Grok 中添加当前隧道地址；授权 Token 不会写进 wcode 配置。',
    source: 'https://docs.x.ai/grok/connectors'
  },
  {
    name: 'Mistral', mark: 'M', tags: ['web', 'account'], mode: 'account',
    package: no, skill: no, stdio: no, http: yes, sse: varies, oauth: yes,
    evidence: 'documented', target: 'Mistral Connector settings',
    en: 'Use the account connector UI; no OAuth secret belongs in the repository.',
    zh: '通过账户 Connector 页面配置，仓库里不应出现 OAuth Secret。',
    source: 'https://docs.mistral.ai/vibe/work/connectors/mcp-connectors'
  }
];

let activeFilter = 'all';
const clientGrid = document.getElementById('clientGrid');
const clientSearch = document.getElementById('clientSearch');
const filterButtons = [...document.querySelectorAll('.filter')];

function capabilityValue(value) {
  if (value === true) return ['yes', '✓'];
  if (value === false) return ['no', '—'];
  return ['varies', pageIsChinese ? '视版本' : 'Varies'];
}

function modeLabel(mode) {
  return clientUi[mode] || mode;
}

function renderCapability(key, value) {
  const [state, label] = capabilityValue(value);
  return `<div><b>${capabilityLabels[key]}</b><span class="cap-${state}">${label}</span></div>`;
}

function renderClients() {
  if (!clientGrid) return;
  const query = (clientSearch?.value || '').trim().toLowerCase();
  const visible = clients.filter((client) => {
    const filterMatch = activeFilter === 'all' || client.tags.includes(activeFilter);
    const searchMatch = !query || `${client.name} ${client.target}`.toLowerCase().includes(query);
    return filterMatch && searchMatch;
  });
  if (!visible.length) {
    clientGrid.innerHTML = `<div class="empty-state">${clientUi.noResults}</div>`;
    return;
  }
  clientGrid.innerHTML = visible.map((client) => {
    const note = pageIsChinese ? client.zh : client.en;
    const source = client.source ?
      `<a class="docs-link" href="${client.source}" target="_blank" rel="noreferrer">${clientUi.source} ↗</a>` : '';
    return `
      <article class="compat-card">
        <div class="compat-top">
          <div class="compat-name"><span class="compat-logo">${client.mark}</span><div><h3>${client.name}</h3><small>${client.target}</small></div></div>
          <span class="status-badge status-${client.mode}">${modeLabel(client.mode)}</span>
        </div>
        <p>${note}</p>
        <div class="compat-matrix">
          ${renderCapability('package', client.package)}
          ${renderCapability('skill', client.skill)}
          ${renderCapability('stdio', client.stdio)}
          ${renderCapability('http', client.http)}
          ${renderCapability('sse', client.sse)}
          ${renderCapability('oauth', client.oauth)}
          ${renderCapability('auto', client.mode === 'auto')}
          ${renderCapability('manual', client.mode !== 'auto')}
        </div>
        <div class="compat-footer"><span class="free-note">${clientUi[client.evidence]}</span>${source}</div>
      </article>`;
  }).join('');
}

filterButtons.forEach((button) => {
  button.addEventListener('click', () => {
    activeFilter = button.dataset.filter || 'all';
    filterButtons.forEach((item) => item.classList.toggle('active', item === button));
    renderClients();
  });
});
clientSearch?.addEventListener('input', renderClients);
renderClients();

const sourceList = document.getElementById('sourceList');
if (sourceList) {
  const sources = clients.filter((client) => client.source);
  sourceList.innerHTML = sources.map((client, index) => `
    <a class="source-item" href="${client.source}" target="_blank" rel="noreferrer">
      <span>${client.mark}</span><div><b>${client.name}</b><small>${pageIsChinese ? `厂商依据 ${index + 1}` : `Vendor source ${index + 1}`}</small></div>
    </a>`).join('');
}

const tabs = [...document.querySelectorAll('.tab')];
const panels = [...document.querySelectorAll('.tab-panel')];
tabs.forEach((tab) => {
  tab.addEventListener('click', () => {
    const selected = tab.dataset.tab;
    tabs.forEach((item) => item.classList.toggle('active', item === tab));
    panels.forEach((panel) => panel.classList.toggle('active', panel.dataset.panel === selected));
  });
});

document.querySelectorAll('[data-copy]').forEach((block) => {
  const button = block.querySelector('.copy-button');
  if (!button) return;
  button.addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText(block.dataset.copy || '');
      const previous = button.textContent;
      button.textContent = pageIsChinese ? '已复制' : 'Copied';
      setTimeout(() => { button.textContent = previous; }, 1300);
    } catch {
      button.textContent = pageIsChinese ? '手动选择' : 'Select';
    }
  });
});

const root = document.documentElement;
const themeToggle = document.getElementById('themeToggle');
const savedTheme = localStorage.getItem('wcode-theme');
if (savedTheme === 'light' || savedTheme === 'dark') {
  root.dataset.theme = savedTheme;
} else if (window.matchMedia?.('(prefers-color-scheme: light)').matches) {
  root.dataset.theme = 'light';
}
themeToggle?.addEventListener('click', () => {
  root.dataset.theme = root.dataset.theme === 'light' ? 'dark' : 'light';
  localStorage.setItem('wcode-theme', root.dataset.theme);
});

const header = document.querySelector('.site-header');
function updateHeader() {
  header?.classList.toggle('scrolled', window.scrollY > 8);
}
window.addEventListener('scroll', updateHeader, { passive: true });
updateHeader();

const reduceMotion = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
if (reduceMotion || !('IntersectionObserver' in window)) {
  document.querySelectorAll('.reveal').forEach((node) => node.classList.add('visible'));
} else {
  const observer = new IntersectionObserver((entries) => {
    entries.forEach((entry) => {
      if (entry.isIntersecting) {
        entry.target.classList.add('visible');
        observer.unobserve(entry.target);
      }
    });
  }, { threshold: .08, rootMargin: '0px 0px -30px 0px' });
  document.querySelectorAll('.reveal').forEach((node) => observer.observe(node));
}
