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
  const visible = clients.filter((client) => {
    const matchesFilter = activeFilter === 'all' || client.tags.includes(activeFilter);
    const economics = clientEconomics[client.name] || {};
    const haystack = `${client.name} ${client.company} ${client.note} ${client.statusLabel} ${economics.freeAccess || ''} ${economics.paidAccess || ''} ${economics.usage || ''}`.toLowerCase();
    return matchesFilter && (!query || haystack.includes(query));
  });

  if (!clientGrid) return;
  if (!visible.length) {
    clientGrid.innerHTML = '<div class="empty-state">No clients match this view. Try another filter or search term.</div>';
    return;
  }

  clientGrid.innerHTML = visible.map((client) => {
    const economics = clientEconomics[client.name] || {
      freeAccess: 'Not verified from current primary documentation.',
      paidAccess: 'Not verified from current primary documentation.',
      usage: 'No reliable MCP-specific billing statement found. Normal model/provider limits still apply.',
      evidence: [[client.sourceLabel || 'Primary source', client.source]]
    };
    const evidence = (economics.evidence || [[client.sourceLabel || 'Primary source', client.source]])
      .map(([label, url]) => `<a href="${url}" target="_blank" rel="noreferrer">${label} ↗</a>`)
      .join('');
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
        <span>${client.region === 'china' ? 'China' : client.region === 'us' ? 'US' : 'Global'}</span>
        <span>${client.surface === 'chat' ? 'AI chat' : client.surface === 'coding' ? 'Coding agent' : 'Agent platform'}</span>
        <span>${client.status === 'native' ? 'OAuth-first' : client.status === 'manual' ? 'Manual auth' : 'Remote HTTP'}</span>
      </div>
      <div class="economics">
        <div><b>Free</b><span>${economics.freeAccess}</span></div>
        <div><b>Paid</b><span>${economics.paidAccess}</span></div>
        <div><b>Usage & quota</b><span>${economics.usage}</span></div>
      </div>
      <div class="evidence"><b>Evidence</b><div>${evidence}</div></div>
      <div class="compat-footer">
        <span class="free-note ${client.free ? 'yes' : ''}">${client.free ? '● ' : '○ '}${client.freeNote}</span>
        <a class="docs-link" href="${client.source}" target="_blank" rel="noreferrer">Primary source ↗</a>
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
      <span>${source.mark}</span><div><b>${source.name}</b><small>${source.label}</small></div>
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
      button.textContent = 'Copied';
      setTimeout(() => { button.textContent = previous; }, 1300);
    } catch {
      button.textContent = 'Select';
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
