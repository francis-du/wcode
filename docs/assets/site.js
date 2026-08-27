const pageIsChinese = document.documentElement.lang.toLowerCase().startsWith('zh');

const clients = [
  {
    name: 'Grok', mark: '✦', company: 'xAI', region: 'us', surface: 'chat',
    status: 'native', statusLabel: 'Native OAuth', free: true,
    tags: ['free', 'us', 'chat', 'native'],
    note: 'Custom MCP connectors are available to all Grok users. Public HTTPS is required; Streamable HTTP works through Cloudflare Quick Tunnel.',
    freeNote: 'Free Grok path available',
    source: 'https://docs.x.ai/grok/connectors', sourceLabel: 'xAI Connectors docs'
  },
  {
    name: 'Claude', mark: '◈', company: 'Anthropic', region: 'us', surface: 'chat',
    status: 'native', statusLabel: 'Native OAuth', free: true,
    tags: ['free', 'us', 'chat', 'native'],
    note: 'Remote custom MCP connectors are available across Free, Pro, Max, Team and Enterprise. Free accounts can add one custom connector.',
    freeNote: 'Free: 1 custom connector',
    source: 'https://support.claude.com/en/articles/11175166-get-started-with-custom-connectors-using-remote-mcp', sourceLabel: 'Claude Help Center'
  },
  {
    name: 'Mistral Vibe Work', mark: '◫', company: 'Mistral AI', region: 'global', surface: 'chat',
    status: 'native', statusLabel: 'Native OAuth', free: true,
    tags: ['free', 'chat', 'native'],
    note: 'Custom MCP Connectors auto-detect OAuth 2.1 and support Dynamic Client Registration. Free account owners are administrators by default.',
    freeNote: 'Free plan supports Connectors',
    source: 'https://docs.mistral.ai/vibe/work/connectors/mcp-connectors', sourceLabel: 'Mistral MCP Connectors'
  },
  {
    name: 'Qoder CLI', mark: 'Q', company: 'Alibaba / Qoder', region: 'china', surface: 'coding',
    status: 'native', statusLabel: 'Native OAuth', free: true,
    tags: ['free', 'china', 'coding', 'native'],
    note: 'Remote HTTP/SSE MCP with a complete OAuth 2.0 + PKCE + DCR implementation, metadata discovery and persisted tokens.',
    freeNote: 'Community / Free plan',
    source: 'https://docs.qoder.com/cli/sdk/mcp', sourceLabel: 'Qoder MCP integration'
  },
  {
    name: 'Cherry Studio', mark: '◇', company: 'CherryHQ', region: 'china', surface: 'chat',
    status: 'native', statusLabel: 'Native OAuth', free: true,
    tags: ['free', 'china', 'chat', 'native'],
    note: 'Desktop MCP host with SSE/Streamable HTTP and an OAuth callback flow in the current implementation. Keep the app current because MCP auth has evolved quickly.',
    freeNote: 'Open-source desktop client',
    source: 'https://github.com/CherryHQ/cherry-studio/blob/main/src/main/ai/mcp/McpRuntimeService.ts', sourceLabel: 'Cherry Studio MCP runtime'
  },
  {
    name: 'LM Studio', mark: '▣', company: 'LM Studio', region: 'us', surface: 'chat',
    status: 'native', statusLabel: 'Native OAuth', free: true,
    tags: ['free', 'us', 'chat', 'native'],
    note: 'Local-model desktop chat with local and remote MCP. OAuth-backed integrations open a browser and store tokens securely.',
    freeNote: 'Free local-model plan',
    source: 'https://lmstudio.ai/docs/integrations/mcp-remote', sourceLabel: 'LM Studio MCP integrations'
  },
  {
    name: 'Open WebUI', mark: '⌁', company: 'Open WebUI', region: 'global', surface: 'chat',
    status: 'native', statusLabel: 'Native OAuth', free: true,
    tags: ['free', 'chat', 'native'],
    note: 'Native Streamable HTTP MCP with OAuth 2.1 + DCR, resource indicators and per-chat authorization. Server registration is admin-only.',
    freeNote: 'Open-source / self-hosted',
    source: 'https://docs.openwebui.com/features/extensibility/mcp/', sourceLabel: 'Open WebUI MCP docs'
  },
  {
    name: 'LibreChat', mark: '◐', company: 'LibreChat', region: 'global', surface: 'chat',
    status: 'native', statusLabel: 'Native OAuth', free: true,
    tags: ['free', 'chat', 'native'],
    note: 'Self-hosted chat and agents can connect to remote MCP servers with OAuth/PKCE and dynamic client registration.',
    freeNote: 'Open-source / self-hosted',
    source: 'https://www.librechat.ai/docs/features/mcp', sourceLabel: 'LibreChat MCP docs'
  },
  {
    name: 'Gemini CLI', mark: 'G', company: 'Google', region: 'us', surface: 'coding',
    status: 'native', statusLabel: 'Native OAuth', free: true,
    tags: ['free', 'us', 'coding', 'native'],
    note: 'Remote HTTP/SSE MCP can discover OAuth from a 401 response, dynamically register a client, open a browser, and persist tokens.',
    freeNote: 'CLI is open source; model quota varies',
    source: 'https://github.com/google-gemini/gemini-cli/blob/main/docs/tools/mcp-server.md', sourceLabel: 'Gemini CLI MCP server docs'
  },
  {
    name: 'Cursor', mark: 'C', company: 'Anysphere', region: 'us', surface: 'coding',
    status: 'native', statusLabel: 'Native OAuth', free: true,
    tags: ['free', 'us', 'coding', 'native'],
    note: 'Remote SSE and Streamable HTTP both support OAuth. Cursor also supports static OAuth credentials when a server does not use DCR.',
    freeNote: 'Free/Hobby path; plan limits apply',
    source: 'https://cursor.com/docs/context/mcp', sourceLabel: 'Cursor MCP docs'
  },
  {
    name: 'Windsurf', mark: 'W', company: 'Cognition', region: 'us', surface: 'coding',
    status: 'native', statusLabel: 'Native OAuth', free: true,
    tags: ['free', 'us', 'coding', 'native'],
    note: 'Cascade supports stdio, Streamable HTTP and SSE, with OAuth support across transport types. Point HTTP servers at the /mcp endpoint.',
    freeNote: 'Free plan availability; limits apply',
    source: 'https://docs.windsurf.com/windsurf/cascade/mcp', sourceLabel: 'Windsurf MCP docs'
  },
  {
    name: 'VS Code', mark: 'V', company: 'Microsoft', region: 'us', surface: 'coding',
    status: 'native', statusLabel: 'Native OAuth', free: true,
    tags: ['free', 'us', 'coding', 'native'],
    note: 'Full MCP client with Streamable HTTP and OAuth. VS Code tries DCR for compatible authorization servers and supports newer CIMD flows as well.',
    freeNote: 'Editor is free; model choice varies',
    source: 'https://code.visualstudio.com/api/extension-guides/ai/mcp', sourceLabel: 'VS Code MCP developer guide'
  },
  {
    name: 'Dify', mark: 'D', company: 'LangGenius', region: 'china', surface: 'agent',
    status: 'native', statusLabel: 'Native OAuth', free: true,
    tags: ['free', 'china', 'native'],
    note: 'As an MCP client, Dify supports Streamable HTTP and an OAuth/PKCE/DCR flow. Self-hosted deployments need a public callback URL configured correctly.',
    freeNote: 'Community Edition self-hosted',
    source: 'https://github.com/langgenius/dify/discussions/37361', sourceLabel: 'Dify MCP OAuth discussion'
  },
  {
    name: 'TRAE', mark: 'T', company: 'ByteDance', region: 'china', surface: 'coding',
    status: 'transport', statusLabel: 'Transport', free: true,
    tags: ['free', 'china', 'coding'],
    note: 'TRAE can act as an MCP client over stdio, SSE and Streamable HTTP. Automatic MCP OAuth discovery was not verified in current public docs, so treat wcode auth as a compatibility test.',
    freeNote: 'Free coding client path',
    source: 'https://forum.trae.ai/', sourceLabel: 'TRAE official community'
  },
  {
    name: '扣子编程 / Coze', mark: '扣', company: 'ByteDance', region: 'china', surface: 'agent',
    status: 'manual', statusLabel: 'Manual OAuth', free: true,
    tags: ['free', 'china'],
    note: 'Coze can create a plugin from an HTTPS MCP URL at no Coze charge, but its OAuth setup expects pre-created client_id/client_secret and explicit endpoints instead of MCP DCR.',
    freeNote: 'Creating MCP plugin is free',
    source: 'https://docs.coze.cn/guides_create_a_plugin_based_on_mcp', sourceLabel: 'Coze MCP plugin guide'
  },
  {
    name: '腾讯元器', mark: '元', company: 'Tencent', region: 'china', surface: 'agent',
    status: 'transport', statusLabel: 'Transport', free: false,
    tags: ['china'],
    note: 'Tencent Yuanqi can add custom MCP servers by URL for Multi-Agent and workflow use. Public documentation does not establish automatic MCP OAuth discovery for wcode.',
    freeNote: 'Platform terms / quota vary',
    source: 'https://yuanqi.tencent.com/guide/plugin-market-integrate-mcp-plugin', sourceLabel: '腾讯元器 MCP guide'
  },
  {
    name: '腾讯云智能体开发平台', mark: '云', company: 'Tencent Cloud', region: 'china', surface: 'agent',
    status: 'transport', statusLabel: 'Transport', free: false,
    tags: ['china'],
    note: 'Supports SSE and streamableHttp MCP endpoints plus custom static headers. Native OAuth discovery is not documented, so an OAuth-capable gateway may be required.',
    freeNote: 'Cloud platform billing may apply',
    source: 'https://cloud.tencent.com/document/product/1759/117855', sourceLabel: '腾讯云 MCP tools docs'
  },
  {
    name: 'Roo Code', mark: 'R', company: 'Roo Code', region: 'global', surface: 'coding',
    status: 'transport', statusLabel: 'Transport', free: true,
    tags: ['free', 'coding'],
    note: 'Supports remote Streamable HTTP MCP, but native OAuth initiation has historically lagged behind transport support. An mcp-remote wrapper may be needed depending on the current release.',
    freeNote: 'Open-source coding agent',
    source: 'https://docs.roocode.com/features/mcp/using-mcp-in-roo', sourceLabel: 'Roo Code MCP docs'
  },
  {
    name: 'Cline', mark: 'Cl', company: 'Cline', region: 'us', surface: 'coding',
    status: 'transport', statusLabel: 'Transport', free: true,
    tags: ['free', 'us', 'coding'],
    note: 'Open-source coding agent with MCP support. Remote OAuth behavior is version-sensitive and is not as clearly documented as transport support, so verify the current build with wcode before relying on one-click auth.',
    freeNote: 'Open-source client; model cost varies',
    source: 'https://docs.cline.bot/mcp/mcp-overview', sourceLabel: 'Cline MCP overview'
  },
  {
    name: 'Kiro', mark: 'K', company: 'AWS', region: 'us', surface: 'coding',
    status: 'native', statusLabel: 'Native OAuth', free: true,
    tags: ['free', 'us', 'coding', 'native'],
    note: 'Remote HTTPS MCP supports browser OAuth and Dynamic Client Registration by default. Kiro has a perpetual Free tier with monthly credits.',
    freeNote: 'Kiro Free: $0 / 50 credits',
    source: 'https://kiro.dev/docs/mcp/configuration/', sourceLabel: 'Kiro MCP configuration'
  },
  {
    name: 'OpenCode', mark: 'O', company: 'OpenCode', region: 'global', surface: 'coding',
    status: 'native', statusLabel: 'Native OAuth', free: true,
    tags: ['free', 'coding', 'native'],
    note: 'Remote MCP automatically detects a 401, starts OAuth, attempts RFC 7591 Dynamic Client Registration, opens the browser, and persists tokens.',
    freeNote: 'Open-source coding agent',
    source: 'https://opencode.ai/docs/mcp-servers/', sourceLabel: 'OpenCode MCP docs'
  },
  {
    name: 'Kimi Code CLI', mark: 'K', company: 'Moonshot AI', region: 'china', surface: 'coding',
    status: 'native', statusLabel: 'OAuth capable', free: false,
    tags: ['china', 'coding', 'native'],
    note: 'Supports HTTP MCP plus browser-based OAuth authorization and token caching. Kimi Code model usage is tied to a paid Kimi membership or platform API access.',
    freeNote: 'Client available; Kimi Code usage is subscription-based',
    source: 'https://www.kimi.com/code/docs/en/kimi-code-cli/customization/mcp.html', sourceLabel: 'Kimi Code MCP docs'
  },
  {
    name: '阿里云百炼', mark: '百', company: 'Alibaba Cloud', region: 'china', surface: 'agent',
    status: 'transport', statusLabel: 'Transport', free: false,
    tags: ['china'],
    note: 'Agent and workflow applications can use custom remote MCP definitions with SSE/Streamable HTTP. Automatic MCP OAuth discovery for arbitrary external servers is not documented.',
    freeNote: 'Cloud service billing / trial terms apply',
    source: 'https://help.aliyun.com/zh/model-studio/custom-mcp', sourceLabel: '百炼 custom MCP docs'
  },
  {
    name: 'Qwen Code', mark: 'Qw', company: 'Alibaba / Qwen', region: 'china', surface: 'coding',
    status: 'native', statusLabel: 'Native OAuth', free: false,
    tags: ['china', 'coding', 'native'],
    note: 'The client supports remote MCP with OAuth discovery/DCR, but the old Qwen OAuth free model allowance was discontinued. Use another model/provider or BYOK.',
    freeNote: 'Client path available; model/provider required',
    source: 'https://github.com/QwenLM/qwen-code', sourceLabel: 'Qwen Code repository'
  },
  {
    name: 'ChatGPT', mark: '◎', company: 'OpenAI', region: 'us', surface: 'chat',
    status: 'native', statusLabel: 'Remote MCP', free: false,
    tags: ['us', 'chat', 'native'],
    note: 'Remote custom MCP is supported, but full write/modify MCP is currently for Business and Enterprise/Edu; Pro has more limited developer-mode MCP access. It is not a free-path recommendation.',
    freeNote: 'Full MCP requires paid workspace plans',
    source: 'https://help.openai.com/en/articles/12584461-developer-mode-and-full-mcp-connectors-in-chatgpt', sourceLabel: 'OpenAI MCP developer mode'
  }
];

// Plan/usage notes are deliberately separated from protocol compatibility.
// “No separate MCP fee documented” does not mean unlimited model usage: the host
// product, model provider, BYOK account, or self-hosted model still applies its own limits.
const clientEconomics = {
  'Grok': {
    freeAccess: 'Yes — Grok is free to start; custom MCP connectors have no paid-only restriction in the connector docs.',
    paidAccess: 'Yes — SuperGrok raises product limits.',
    usage: 'No separate MCP connector meter is documented. Grok conversations still follow product limits; paid users draw from a shared weekly usage pool.',
    evidence: [
      ['Connector support', 'https://docs.x.ai/grok/connectors'],
      ['Plans & weekly usage', 'https://docs.x.ai/grok/overview'],
      ['Usage pool FAQ', 'https://docs.x.ai/grok/faq']
    ]
  },
  'Claude': {
    freeAccess: 'Yes — Free supports one custom Remote MCP connector.',
    paidAccess: 'Yes — Pro, Max, Team and Enterprise support custom connectors.',
    usage: 'Connector/tool use is token-intensive and can reduce available context and usage allowance. Claude product surfaces share the same usage limit.',
    evidence: [
      ['Remote MCP availability', 'https://support.claude.com/en/articles/11175166-get-started-with-custom-connectors-using-remote-mcp'],
      ['Usage limits', 'https://support.claude.com/en/articles/11647753-how-do-usage-and-length-limits-work'],
      ['Tool access & context', 'https://support.claude.com/en/articles/13730515-manage-claude-s-tool-access']
    ]
  },
  'Mistral Vibe Work': {
    freeAccess: 'Yes — the account owner is administrator on Free, Pro and Student plans and can add Custom MCP Connectors.',
    paidAccess: 'Yes — paid/team plans can also use connectors subject to workspace controls.',
    usage: 'No separate MCP connector charge is documented. Model/task usage still follows the Vibe Work plan; connector calls add model context and work to the task.',
    evidence: [
      ['Custom MCP + OAuth 2.1/DCR', 'https://docs.mistral.ai/vibe/work/connectors/mcp-connectors'],
      ['Connector behavior', 'https://docs.mistral.ai/vibe/work/connectors']
    ]
  },
  'Qoder CLI': {
    freeAccess: 'Yes — Community/Free includes BYOK and limited basic-model messages.',
    paidAccess: 'Yes — Pro/Pro+/Ultra include monthly premium-model Credits.',
    usage: 'MCP itself has no separate documented charge. Agent/model requests consume Qoder limits/Credits, or the user’s own provider budget when using BYOK.',
    evidence: [
      ['Remote MCP + OAuth', 'https://docs.qoder.com/cli/sdk/mcp'],
      ['Plans & Credits', 'https://docs.qoder.com/account/pricing'],
      ['Community Edition / BYOK', 'https://qoder.com/blog/qoder-community']
    ]
  },
  'Cherry Studio': {
    freeAccess: 'Yes — open-source desktop client.',
    paidAccess: 'Client access is not tied to an AI subscription; configured model providers may charge separately.',
    usage: 'Cherry Studio does not provide the model budget. MCP tool schemas/results consume whatever context/tokens the selected model provider bills or limits.',
    evidence: [['MCP runtime implementation', 'https://github.com/CherryHQ/cherry-studio/blob/main/src/main/ai/mcp/McpRuntimeService.ts']]
  },
  'LM Studio': {
    freeAccess: 'Yes — local-model usage can avoid a hosted-model subscription entirely.',
    paidAccess: 'Optional hosted/provider usage depends on the model provider selected by the user.',
    usage: 'With local inference there is no external model subscription quota; MCP only adds local context/inference work. Hosted models follow their provider billing.',
    evidence: [
      ['Remote MCP integrations', 'https://lmstudio.ai/docs/integrations/mcp-remote'],
      ['LM Studio pricing', 'https://lmstudio.ai/pricing']
    ]
  },
  'Open WebUI': {
    freeAccess: 'Yes — self-hosted/open-source path.',
    paidAccess: 'Optional hosted infrastructure/model services can cost separately.',
    usage: 'Open WebUI does not create a model allowance. MCP traffic consumes context/tokens from the configured model backend; local models avoid provider quota.',
    evidence: [['Native MCP docs', 'https://docs.openwebui.com/features/extensibility/mcp/']]
  },
  'LibreChat': {
    freeAccess: 'Yes — self-hosted/open-source path.',
    paidAccess: 'Model/API provider costs are separate.',
    usage: 'No LibreChat MCP quota is documented; the selected model/API provider accounts for model tokens and rate limits.',
    evidence: [['LibreChat MCP docs', 'https://www.librechat.ai/docs/features/mcp']]
  },
  'Gemini CLI': {
    freeAccess: 'Client is open source; Google model quotas depend on the authenticated Gemini account/API tier.',
    paidAccess: 'Yes — paid/API tiers can provide higher model quotas.',
    usage: 'No separate MCP meter is documented. MCP tool context and calls are part of the model session and therefore subject to Gemini/API quotas.',
    evidence: [['Gemini CLI MCP docs', 'https://github.com/google-gemini/gemini-cli/blob/main/docs/tools/mcp-server.md']]
  },
  'Cursor': {
    freeAccess: 'A free/Hobby product tier exists; MCP availability is documented independently of model plan limits.',
    paidAccess: 'Yes — paid Cursor plans have larger agent/model allowances.',
    usage: 'No separate MCP connector fee is documented. Agent/model calls that use MCP still count under Cursor/model usage rules.',
    evidence: [
      ['Cursor MCP docs', 'https://cursor.com/docs/context/mcp'],
      ['Cursor pricing', 'https://cursor.com/pricing']
    ]
  },
  'Windsurf': {
    freeAccess: 'A free product tier is available; MCP transport/OAuth support is documented in Cascade.',
    paidAccess: 'Yes — paid tiers raise product/model allowances.',
    usage: 'No separate MCP connector meter is documented. Cascade/model activity remains subject to the user’s Windsurf plan limits.',
    evidence: [
      ['Windsurf MCP docs', 'https://docs.windsurf.com/windsurf/cascade/mcp'],
      ['Windsurf pricing', 'https://windsurf.com/pricing']
    ]
  },
  'VS Code': {
    freeAccess: 'The editor and MCP client are free; AI model entitlement depends on the chosen extension/provider.',
    paidAccess: 'Copilot or other hosted model subscriptions can be used separately.',
    usage: 'MCP itself has no VS Code subscription meter. The connected AI extension/provider determines token/request limits.',
    evidence: [['VS Code MCP guide', 'https://code.visualstudio.com/api/extension-guides/ai/mcp']]
  },
  'Dify': {
    freeAccess: 'Community Edition can be self-hosted.',
    paidAccess: 'Dify Cloud and model providers have their own plans.',
    usage: 'MCP connection itself is not documented as a separate meter. Model/API calls and cloud resources follow the configured provider/Dify plan.',
    evidence: [['Dify MCP OAuth discussion', 'https://github.com/langgenius/dify/discussions/37361']]
  },
  'TRAE': {
    freeAccess: 'A free client path exists, but current public docs do not clearly document wcode-style OAuth discovery.',
    paidAccess: 'Plan/model rules vary by TRAE offering and region.',
    usage: 'Not publicly specified for custom Remote MCP. Treat model/agent usage as plan-limited; wcode does not change those limits.',
    evidence: [['TRAE official community', 'https://forum.trae.ai/']]
  },
  '扣子编程 / Coze': {
    freeAccess: 'Creating an MCP plugin is documented as available without a separate plugin-creation charge.',
    paidAccess: 'Workspace/model usage can still be quota- or plan-based.',
    usage: 'The connector is not a model allowance. Bot/model execution remains subject to Coze workspace/model quotas; exact MCP-specific metering is not documented.',
    evidence: [['Coze custom MCP plugin guide', 'https://docs.coze.cn/guides_create_a_plugin_based_on_mcp']]
  },
  '腾讯元器': {
    freeAccess: 'Platform access/trials vary; no reliable public statement that arbitrary custom MCP is unlimited on a free tier.',
    paidAccess: 'Yes, subject to Tencent platform product terms.',
    usage: 'Custom MCP transport is documented, but MCP-specific billing is not. Model/agent/runtime quota can still apply.',
    evidence: [['腾讯元器 MCP guide', 'https://yuanqi.tencent.com/guide/plugin-market-integrate-mcp-plugin']]
  },
  '腾讯云智能体开发平台': {
    freeAccess: 'Cloud trials/quotas may exist; not treated as a guaranteed free path.',
    paidAccess: 'Yes — cloud model/runtime billing can apply.',
    usage: 'Streamable HTTP/SSE MCP itself is not documented as a separate allowance. Agent/model/cloud runtime usage follows Tencent Cloud billing.',
    evidence: [['腾讯云 MCP tools docs', 'https://cloud.tencent.com/document/product/1759/117855']]
  },
  'Roo Code': {
    freeAccess: 'Yes — open-source client.',
    paidAccess: 'Model/provider subscriptions or API keys are separate.',
    usage: 'Roo does not provide the model budget; MCP context/tool results consume the selected model provider’s quota/tokens.',
    evidence: [['Roo Code MCP docs', 'https://docs.roocode.com/features/mcp/using-mcp-in-roo']]
  },
  'Cline': {
    freeAccess: 'Yes — open-source client.',
    paidAccess: 'Model/provider usage is separate.',
    usage: 'No Cline MCP subscription meter. Tool context/results are processed by the selected model and therefore use that provider’s quota.',
    evidence: [['Cline MCP overview', 'https://docs.cline.bot/mcp/mcp-overview']]
  },
  'Kiro': {
    freeAccess: 'Yes — perpetual Kiro Free includes 50 credits.',
    paidAccess: 'Yes — paid tiers include 1,000–10,000 credits plus optional add-ons.',
    usage: 'Kiro says credits are consumed fractionally per request. MCP has no separate documented meter, so agent requests using MCP consume normal Kiro credits.',
    evidence: [
      ['MCP OAuth/DCR', 'https://kiro.dev/docs/mcp/configuration/'],
      ['Pricing', 'https://kiro.dev/pricing/'],
      ['Billing / credit consumption', 'https://kiro.dev/docs/billing/']
    ]
  },
  'OpenCode': {
    freeAccess: 'Yes — open-source client; BYOK/local/provider choice is independent.',
    paidAccess: 'Any cost comes from the model/provider selected by the user.',
    usage: 'OpenCode explicitly warns that MCP servers consume context space and can use many tokens. There is no separate OpenCode MCP charge.',
    evidence: [['Remote MCP + OAuth/DCR', 'https://dev.opencode.ai/docs/mcp-servers/']]
  },
  'Kimi Code CLI': {
    freeAccess: 'The CLI can be installed, but Kimi Code service entitlement is tied to Kimi membership/API access; it is not presented here as a free model path.',
    paidAccess: 'Yes — Kimi membership includes Kimi Code with plan-dependent quota.',
    usage: 'Kimi Code requests share membership quota and have a separate rolling/weekly limit. MCP itself has no separate meter; model work still consumes Kimi Code quota.',
    evidence: [
      ['MCP OAuth', 'https://moonshotai.github.io/kimi-cli/en/customization/mcp.html'],
      ['Kimi Code membership quota', 'https://www.kimi.com/code/docs/en/kimi-code/membership.html'],
      ['Membership pricing/usage', 'https://www.kimi.com/en/help/membership/membership-pricing']
    ]
  },
  '阿里云百炼': {
    freeAccess: 'Trials or promotional quotas may exist, but this is a cloud service rather than a guaranteed free path.',
    paidAccess: 'Yes — model/agent/cloud billing applies according to Model Studio terms.',
    usage: 'Custom MCP transport does not remove model or application charges. No separate arbitrary-MCP allowance is documented.',
    evidence: [['百炼 custom MCP docs', 'https://help.aliyun.com/zh/model-studio/custom-mcp']]
  },
  'Qwen Code': {
    freeAccess: 'The client is open source; bundled model entitlement is not assumed. BYOK/provider rules apply.',
    paidAccess: 'Can be used with paid model/provider access.',
    usage: 'MCP itself has no separate quota; selected model/API usage determines cost and limits.',
    evidence: [['Qwen Code repository', 'https://github.com/QwenLM/qwen-code']]
  },
  'ChatGPT': {
    freeAccess: 'No — custom full MCP is not a Free-plan feature.',
    paidAccess: 'Full read/write MCP: Business and Enterprise/Edu. Pro can connect read/fetch MCPs in developer mode.',
    usage: 'OpenAI does not document a separate MCP connector meter here. MCP runs inside ChatGPT conversations, so ordinary plan/model usage limits still apply.',
    evidence: [['OpenAI developer mode / MCP availability', 'https://help.openai.com/en/articles/12584461-developer-mode-and-full-mcp-connectors-in-chatgpt']]
  }
};

const zhClientCopy = {
  'Grok': {
    statusLabel: '原生 OAuth',
    note: '所有 Grok 用户都可以使用自定义 MCP 连接器。需要公网 HTTPS；通过 Cloudflare Quick Tunnel 可使用 Streamable HTTP。',
    freeNote: '可使用免费 Grok',
    economics: {
      freeAccess: '是 — Grok 可以免费开始使用，连接器文档没有把自定义 MCP 限定为付费功能。',
      paidAccess: '是 — SuperGrok 会提高产品使用额度。',
      usage: '没有单独的 MCP 连接器计费项。Grok 对话仍受产品额度限制；付费用户使用共享的每周额度池。'
    }
  },
  'Claude': {
    statusLabel: '原生 OAuth',
    note: 'Free、Pro、Max、Team 和 Enterprise 都支持远程自定义 MCP 连接器；Free 账户可以添加 1 个自定义连接器。',
    freeNote: '免费版可添加 1 个连接器',
    economics: {
      freeAccess: '是 — Free 支持 1 个自定义远程 MCP 连接器。',
      paidAccess: '是 — Pro、Max、Team 和 Enterprise 都支持自定义连接器。',
      usage: '连接器和工具调用会占用较多 Token，可能减少可用上下文和使用额度；Claude 各产品界面共享使用限制。'
    }
  },
  'Mistral Vibe Work': {
    statusLabel: '原生 OAuth',
    note: '自定义 MCP 连接器可自动识别 OAuth 2.1，并支持动态客户端注册；Free 账户所有者默认具有管理员权限。',
    freeNote: '免费方案支持连接器',
    economics: {
      freeAccess: '是 — Free、Pro 和 Student 的账户所有者默认是管理员，可以添加自定义 MCP 连接器。',
      paidAccess: '是 — 付费或团队方案也可以在工作区策略允许时使用连接器。',
      usage: '没有单独的 MCP 连接器收费说明。模型和任务使用仍按 Vibe Work 方案计算，连接器调用会增加任务上下文和模型工作量。'
    }
  },
  'Qoder CLI': {
    statusLabel: '原生 OAuth',
    note: '支持远程 HTTP/SSE MCP，并实现 OAuth 2.0、PKCE、动态客户端注册、元数据发现和 Token 持久化。',
    freeNote: '社区版 / 免费方案',
    economics: {
      freeAccess: '是 — Community/Free 包含 BYOK 和有限的基础模型消息额度。',
      paidAccess: '是 — Pro、Pro+、Ultra 包含每月高级模型 Credits。',
      usage: 'MCP 本身没有单独收费说明。智能体和模型请求会消耗 Qoder 的额度/Credits；使用 BYOK 时则消耗用户自己的模型提供商预算。'
    }
  },
  'Cherry Studio': {
    statusLabel: '原生 OAuth',
    note: '桌面 MCP Host 支持 SSE/Streamable HTTP，当前实现包含 OAuth 回调流程。MCP 认证变化较快，建议保持客户端为最新版本。',
    freeNote: '开源桌面客户端',
    economics: {
      freeAccess: '是 — 开源桌面客户端。',
      paidAccess: '客户端本身不绑定 AI 订阅；配置的模型提供商可能单独收费。',
      usage: 'Cherry Studio 不提供模型额度。MCP 工具 Schema 和结果会占用所选模型提供商计费或限制的上下文与 Token。'
    }
  },
  'LM Studio': {
    statusLabel: '原生 OAuth',
    note: '本地模型桌面对话客户端，同时支持本地和远程 MCP。使用 OAuth 的集成会打开浏览器，并安全保存 Token。',
    freeNote: '免费本地模型方案',
    economics: {
      freeAccess: '是 — 使用本地模型可以完全不依赖托管模型订阅。',
      paidAccess: '可选的托管模型或提供商费用取决于用户选择的模型服务。',
      usage: '本地推理没有外部模型订阅额度；MCP 只增加本地上下文和推理工作。托管模型仍按对应提供商计费。'
    }
  },
  'Open WebUI': {
    statusLabel: '原生 OAuth',
    note: '原生支持 Streamable HTTP MCP、OAuth 2.1、动态客户端注册、资源指示器和按对话授权；服务器注册仅管理员可操作。',
    freeNote: '开源 / 可自托管',
    economics: {
      freeAccess: '是 — 可自托管的开源方案。',
      paidAccess: '可选的托管基础设施或模型服务可能单独收费。',
      usage: 'Open WebUI 不提供模型额度。MCP 流量会消耗所配置模型后端的上下文和 Token；本地模型可避免外部提供商额度。'
    }
  },
  'LibreChat': {
    statusLabel: '原生 OAuth',
    note: '自托管聊天和智能体可以通过 OAuth/PKCE 与动态客户端注册连接远程 MCP 服务器。',
    freeNote: '开源 / 可自托管',
    economics: {
      freeAccess: '是 — 可自托管的开源方案。',
      paidAccess: '模型或 API 提供商费用另计。',
      usage: '没有文档说明 LibreChat 自身存在 MCP 配额；模型 Token 和速率限制由所选模型/API 提供商计算。'
    }
  },
  'Gemini CLI': {
    statusLabel: '原生 OAuth',
    note: '远程 HTTP/SSE MCP 可以从 401 响应发现 OAuth，动态注册客户端、打开浏览器，并持久化 Token。',
    freeNote: 'CLI 开源；模型额度按账户而定',
    economics: {
      freeAccess: '客户端开源；Google 模型额度取决于已登录的 Gemini 账户或 API 等级。',
      paidAccess: '是 — 付费/API 等级可以提供更高模型额度。',
      usage: '没有单独的 MCP 计量项。MCP 工具上下文和调用属于模型会话，因此受 Gemini/API 额度限制。'
    }
  },
  'Cursor': {
    statusLabel: '原生 OAuth',
    note: '远程 SSE 和 Streamable HTTP 都支持 OAuth；如果服务器不使用动态客户端注册，Cursor 也支持静态 OAuth 凭据。',
    freeNote: '有免费/Hobby 方案；受额度限制',
    economics: {
      freeAccess: '存在免费/Hobby 产品方案；MCP 支持与模型方案额度分别说明。',
      paidAccess: '是 — Cursor 付费方案提供更高的智能体和模型额度。',
      usage: '没有单独的 MCP 连接器费用。使用 MCP 的智能体/模型请求仍按 Cursor 和模型使用规则计算。'
    }
  },
  'Windsurf': {
    statusLabel: '原生 OAuth',
    note: 'Cascade 支持 stdio、Streamable HTTP 和 SSE，并在多种传输上支持 OAuth；HTTP 服务器应指向 /mcp 端点。',
    freeNote: '有免费方案；受额度限制',
    economics: {
      freeAccess: '有免费产品方案；Cascade 文档明确支持 MCP 传输和 OAuth。',
      paidAccess: '是 — 付费方案提高产品和模型额度。',
      usage: '没有单独的 MCP 连接器计量项。Cascade 和模型活动仍受 Windsurf 方案限制。'
    }
  },
  'VS Code': {
    statusLabel: '原生 OAuth',
    note: '完整 MCP 客户端，支持 Streamable HTTP 和 OAuth。VS Code 会对兼容授权服务器尝试动态客户端注册，也支持更新的 CIMD 流程。',
    freeNote: '编辑器免费；模型选择另计',
    economics: {
      freeAccess: '编辑器和 MCP 客户端免费；AI 模型权益取决于所选扩展或提供商。',
      paidAccess: '可以另外使用 Copilot 或其他托管模型订阅。',
      usage: 'MCP 本身没有 VS Code 订阅计量项；连接的 AI 扩展或模型提供商决定 Token 和请求限制。'
    }
  },
  'Dify': {
    statusLabel: '原生 OAuth',
    note: '作为 MCP 客户端时支持 Streamable HTTP，以及 OAuth/PKCE/动态客户端注册流程；自托管部署需要正确配置公网回调地址。',
    freeNote: '社区版可自托管',
    economics: {
      freeAccess: 'Community Edition 可以自托管。',
      paidAccess: 'Dify Cloud 和模型提供商各有独立方案。',
      usage: 'MCP 连接没有单独计量说明；模型/API 调用和云资源按配置的提供商或 Dify 方案计算。'
    }
  },
  'TRAE': {
    statusLabel: '仅传输',
    note: '可通过 stdio、SSE 和 Streamable HTTP 作为 MCP 客户端。当前公开文档未确认自动 MCP OAuth 发现，因此与 wcode 的认证兼容性需要实际验证。',
    freeNote: '有免费编程客户端方案',
    economics: {
      freeAccess: '存在免费客户端路径，但当前公开文档没有清晰说明与 wcode 类似的 OAuth 自动发现。',
      paidAccess: '方案和模型规则会随 TRAE 产品与地区变化。',
      usage: '自定义远程 MCP 的具体计量没有公开说明；模型和智能体使用仍受平台方案限制，wcode 不会改变这些额度。'
    }
  },
  '扣子编程 / Coze': {
    statusLabel: '手动 OAuth',
    note: '可以从 HTTPS MCP 地址创建插件且不收取插件创建费用，但 OAuth 配置要求预先创建 client_id/client_secret 并显式填写端点，而不是使用 MCP 动态客户端注册。',
    freeNote: '创建 MCP 插件免费',
    economics: {
      freeAccess: '文档说明创建 MCP 插件本身不单独收费。',
      paidAccess: '工作区和模型使用仍可能受到额度或套餐限制。',
      usage: '连接器不等于模型额度。机器人和模型执行仍受 Coze 工作区/模型额度限制；没有公开单独的 MCP 计量规则。'
    }
  },
  '腾讯元器': {
    statusLabel: '仅传输',
    note: '可通过 URL 添加自定义 MCP 服务器用于多智能体和工作流；公开文档没有证明会为 wcode 自动执行 MCP OAuth 发现。',
    freeNote: '平台条款 / 额度可能变化',
    economics: {
      freeAccess: '平台访问和试用会变化；没有可靠公开说明表明任意自定义 MCP 在免费层无限可用。',
      paidAccess: '是 — 具体取决于腾讯元器产品条款。',
      usage: '公开文档说明了自定义 MCP 传输，但没有单独的 MCP 计费规则；模型、智能体和运行资源仍可能受额度限制。'
    }
  },
  '腾讯云智能体开发平台': {
    statusLabel: '仅传输',
    note: '支持 SSE 和 Streamable HTTP MCP 端点以及自定义静态 Header。公开文档未说明原生 OAuth 发现，因此可能需要具备 OAuth 能力的网关。',
    freeNote: '可能产生云平台费用',
    economics: {
      freeAccess: '可能存在云试用或赠送额度，但不视为稳定的免费路径。',
      paidAccess: '是 — 云模型和运行资源可能产生费用。',
      usage: 'Streamable HTTP/SSE MCP 没有单独额度说明；智能体、模型和云运行资源按腾讯云计费规则计算。'
    }
  },
  'Roo Code': {
    statusLabel: '仅传输',
    note: '支持远程 Streamable HTTP MCP，但原生 OAuth 发起能力曾落后于传输支持；根据当前版本，可能仍需要 mcp-remote 包装器。',
    freeNote: '开源编程智能体',
    economics: {
      freeAccess: '是 — 开源客户端。',
      paidAccess: '模型提供商订阅或 API Key 费用另计。',
      usage: 'Roo 不提供模型额度；MCP 上下文和工具结果会消耗所选模型提供商的额度和 Token。'
    }
  },
  'Cline': {
    statusLabel: '仅传输',
    note: '开源编程智能体，支持 MCP。远程 OAuth 行为受版本影响，公开文档没有像传输支持那样明确，因此依赖一键认证前应使用当前版本实测。',
    freeNote: '开源客户端；模型费用另计',
    economics: {
      freeAccess: '是 — 开源客户端。',
      paidAccess: '模型提供商费用另计。',
      usage: 'Cline 没有单独的 MCP 订阅计量；工具上下文和结果由所选模型处理，因此会消耗该提供商额度。'
    }
  },
  'Kiro': {
    statusLabel: '原生 OAuth',
    note: '远程 HTTPS MCP 默认支持浏览器 OAuth 和动态客户端注册；Kiro 提供长期 Free 方案并按月提供 Credits。',
    freeNote: 'Kiro Free：$0 / 50 Credits',
    economics: {
      freeAccess: '是 — 长期 Kiro Free 包含 50 Credits。',
      paidAccess: '是 — 付费方案包含 1,000–10,000 Credits，并可选购附加额度。',
      usage: 'Kiro 按请求按比例消耗 Credits。没有单独的 MCP 计量项，因此使用 MCP 的智能体请求仍消耗普通 Kiro Credits。'
    }
  },
  'OpenCode': {
    statusLabel: '原生 OAuth',
    note: '远程 MCP 会自动识别 401、启动 OAuth、尝试 RFC 7591 动态客户端注册、打开浏览器并持久化 Token。',
    freeNote: '开源编程智能体',
    economics: {
      freeAccess: '是 — 开源客户端；BYOK、本地模型或提供商选择彼此独立。',
      paidAccess: '费用来自用户选择的模型或提供商。',
      usage: 'OpenCode 明确提示 MCP 服务器会占用上下文并可能消耗大量 Token；没有单独的 OpenCode MCP 收费。'
    }
  },
  'Kimi Code CLI': {
    statusLabel: '支持 OAuth',
    note: '支持 HTTP MCP、浏览器 OAuth 授权和 Token 缓存。Kimi Code 模型使用与付费 Kimi 会员或平台 API 权益绑定。',
    freeNote: '客户端可用；Kimi Code 服务需会员/API',
    economics: {
      freeAccess: 'CLI 可以安装，但 Kimi Code 服务权益绑定 Kimi 会员或 API，不作为免费模型路径推荐。',
      paidAccess: '是 — Kimi 会员包含 Kimi Code，额度随方案变化。',
      usage: 'Kimi Code 请求共享会员额度，并有独立滚动/每周限制。MCP 没有单独计量项，模型工作仍消耗 Kimi Code 额度。'
    }
  },
  '阿里云百炼': {
    statusLabel: '仅传输',
    note: '智能体和工作流应用可以使用 SSE/Streamable HTTP 的自定义远程 MCP；公开文档没有说明会对任意外部服务器自动执行 MCP OAuth 发现。',
    freeNote: '可能有试用；云服务按规则计费',
    economics: {
      freeAccess: '可能有试用或活动额度，但这是云服务，不视为稳定的免费路径。',
      paidAccess: '是 — 模型、智能体和云服务按百炼相关条款计费。',
      usage: '自定义 MCP 传输不会免除模型或应用费用；没有公开单独的任意 MCP 使用额度。'
    }
  },
  'Qwen Code': {
    statusLabel: '原生 OAuth',
    note: '客户端支持远程 MCP 的 OAuth 发现和动态客户端注册；旧的 Qwen OAuth 免费模型额度已经停止，需使用其他模型/提供商或 BYOK。',
    freeNote: '客户端可用；模型/提供商另配',
    economics: {
      freeAccess: '客户端开源；不假设附带模型权益，需按 BYOK 或提供商规则使用。',
      paidAccess: '可以连接付费模型或提供商。',
      usage: 'MCP 本身没有单独额度；成本和限制由所选模型/API 决定。'
    }
  },
  'ChatGPT': {
    statusLabel: '远程 MCP',
    note: '支持远程自定义 MCP；完整读写/修改 MCP 当前面向 Business 和 Enterprise/Edu，Pro 的开发者模式 MCP 能力更有限，因此不作为免费路径推荐。',
    freeNote: '完整 MCP 需要付费工作区方案',
    economics: {
      freeAccess: '否 — 自定义完整 MCP 不是 Free 方案能力。',
      paidAccess: '完整读写 MCP：Business 与 Enterprise/Edu；Pro 可在开发者模式连接较受限的读取/获取型 MCP。',
      usage: '这里没有单独的 MCP 连接器计量说明。MCP 在 ChatGPT 对话内运行，因此仍受普通方案和模型使用限制。'
    }
  }
};

const clientUi = pageIsChinese ? {
  empty: '没有符合当前筛选条件的客户端。请切换筛选项或搜索关键词。',
  regionChina: '中国', regionUs: '美国', regionGlobal: '全球',
  surfaceChat: 'AI 对话', surfaceCoding: '编程智能体', surfaceAgent: '智能体平台',
  authNative: 'OAuth 优先', authManual: '手动认证', authRemote: '远程 HTTP',
  free: '免费', paid: '付费', usage: '使用与额度', evidence: '依据',
  primarySource: '官方来源 ↗', source: '官方来源'
} : {
  empty: 'No clients match this view. Try another filter or search term.',
  regionChina: 'China', regionUs: 'US', regionGlobal: 'Global',
  surfaceChat: 'AI chat', surfaceCoding: 'Coding agent', surfaceAgent: 'Agent platform',
  authNative: 'OAuth-first', authManual: 'Manual auth', authRemote: 'Remote HTTP',
  free: 'Free', paid: 'Paid', usage: 'Usage & quota', evidence: 'Evidence',
  primarySource: 'Primary source ↗', source: 'Primary source'
};

function localizedClient(client) {
  if (!pageIsChinese) {
    return { ...client, economics: clientEconomics[client.name] || {} };
  }
  const localized = zhClientCopy[client.name] || {};
  return {
    ...client,
    ...localized,
    economics: { ...(clientEconomics[client.name] || {}), ...(localized.economics || {}) }
  };
}

const sourceExtras = [
  { name: 'MCP 2026-07-28 specification', mark: 'M', url: 'https://modelcontextprotocol.io/specification/2026-07-28', label: 'Protocol + authorization baseline' },
  { name: 'Grok tunneling guidance', mark: '✦', url: 'https://docs.x.ai/grok/connectors/custom-mcp-tunneling', label: 'Cloudflare Quick Tunnel + Streamable HTTP' },
  { name: 'Qoder Community Edition', mark: 'Q', url: 'https://qoder.com/blog/qoder-community', label: 'Free / BYOK availability' },
  { name: 'LM Studio pricing', mark: '▣', url: 'https://lmstudio.ai/pricing', label: 'Free local-model plan' }
];

const clientGrid = document.getElementById('clientGrid');
const clientSearch = document.getElementById('clientSearch');
const filterButtons = [...document.querySelectorAll('.filter')];
let activeFilter = 'all';

function statusClass(status) {
  return {
    native: 'status-native',
    transport: 'status-transport',
    manual: 'status-manual'
  }[status] || 'status-none';
}

function renderClients() {
  const query = (clientSearch?.value || '').trim().toLowerCase();
  const visible = clients.map(localizedClient).filter((client) => {
    const matchesFilter = activeFilter === 'all' || client.tags.includes(activeFilter);
    const economics = client.economics || {};
    const haystack = `${client.name} ${client.company} ${client.note} ${client.statusLabel} ${economics.freeAccess || ''} ${economics.paidAccess || ''} ${economics.usage || ''}`.toLowerCase();
    return matchesFilter && (!query || haystack.includes(query));
  });

  if (!clientGrid) return;
  if (!visible.length) {
    clientGrid.innerHTML = `<div class="empty-state">${clientUi.empty}</div>`;
    return;
  }

  clientGrid.innerHTML = visible.map((client) => {
    const economics = client.economics || {
      freeAccess: pageIsChinese ? '当前官方文档未核实。' : 'Not verified from current primary documentation.',
      paidAccess: pageIsChinese ? '当前官方文档未核实。' : 'Not verified from current primary documentation.',
      usage: pageIsChinese ? '没有可靠的 MCP 单独计费说明；仍按模型或提供商的一般限制执行。' : 'No reliable MCP-specific billing statement found. Normal model/provider limits still apply.',
      evidence: [[client.sourceLabel || clientUi.source, client.source]]
    };
    const evidence = (economics.evidence || [[client.sourceLabel || clientUi.source, client.source]])
      .map(([label, url], index) => `<a href="${url}" target="_blank" rel="noreferrer">${pageIsChinese ? `官方来源 ${index + 1}` : `${label} ↗`}</a>`)
      .join('');
    const regionLabel = client.region === 'china' ? clientUi.regionChina : client.region === 'us' ? clientUi.regionUs : clientUi.regionGlobal;
    const surfaceLabel = client.surface === 'chat' ? clientUi.surfaceChat : client.surface === 'coding' ? clientUi.surfaceCoding : clientUi.surfaceAgent;
    const authLabel = client.status === 'native' ? clientUi.authNative : client.status === 'manual' ? clientUi.authManual : clientUi.authRemote;
    return `
    <article class="compat-card" data-tags="${client.tags.join(' ')}">
      <div class="compat-top">
        <div class="compat-name">
          <div class="compat-logo" aria-hidden="true">${client.mark}</div>
          <div><h3>${client.name}</h3><small>${client.company}</small></div>
        </div>
        <span class="status-badge ${statusClass(client.status)}">${client.statusLabel}</span>
      </div>
      <p>${client.note}</p>
      <div class="compat-tags">
        <span>${regionLabel}</span>
        <span>${surfaceLabel}</span>
        <span>${authLabel}</span>
      </div>
      <div class="economics">
        <div><b>${clientUi.free}</b><span>${economics.freeAccess}</span></div>
        <div><b>${clientUi.paid}</b><span>${economics.paidAccess}</span></div>
        <div><b>${clientUi.usage}</b><span>${economics.usage}</span></div>
      </div>
      <div class="evidence"><b>${clientUi.evidence}</b><div>${evidence}</div></div>
      <div class="compat-footer">
        <span class="free-note ${client.free ? 'yes' : ''}">${client.free ? '● ' : '○ '}${client.freeNote}</span>
        <a class="docs-link" href="${client.source}" target="_blank" rel="noreferrer">${clientUi.primarySource}</a>
      </div>
    </article>
  `;
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
  const economicsSources = clients.flatMap((client) =>
    (clientEconomics[client.name]?.evidence || []).map(([label, url]) => ({
      name: client.name,
      mark: client.mark,
      url,
      label
    }))
  );
  const sources = [
    ...clients.map(({ name, mark, source, sourceLabel }) => ({ name, mark, url: source, label: sourceLabel })),
    ...economicsSources,
    ...sourceExtras
  ];
  const unique = [...new Map(sources.map((source) => [source.url, source])).values()];
  sourceList.innerHTML = unique.map((source) => `
    <a class="source-item" href="${source.url}" target="_blank" rel="noreferrer">
      <span>${source.mark}</span><div><b>${pageIsChinese ? source.name.replace('MCP 2026-07-28 specification', 'MCP 2026-07-28 规范').replace('Grok tunneling guidance', 'Grok 隧道指南').replace('Qoder Community Edition', 'Qoder 社区版').replace('LM Studio pricing', 'LM Studio 价格') : source.name}</b><small>${pageIsChinese ? clientUi.source : source.label}</small></div>
    </a>
  `).join('');
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
