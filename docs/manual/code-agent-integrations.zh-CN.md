---
layout: docs
title: Agent 与 MCP 集成
description: wcode 与本地 Coding Agent、云端 Connector、Skill 和 Plugin 的接入方式
lang: zh-CN
alternate: /docs/code-agent-integrations/
permalink: /zh/docs/code-agent-integrations/
---

# Code Agent、MCP、Skill 与 Plugin 集成

这页是 wcode 唯一维护的 Agent/Connector 技术接入指南。MCP 是可执行能力层；Skill 是可移植工作流说明层；Host 自带的 Subagent/Worktree 是隔离执行能力；真正必须执行的安全、Verification 与 Evidence 规则仍由 wcode Runtime 强制。

所有本地 Agent 都应显式指定 Repository Workspace，优先使用 stdio；Cloud/Web Connector 才使用公网 Streamable HTTP + OAuth。Plugin/Skill 不应携带 Hook、凭据、任意脚本或隐式扩大 Workspace 的逻辑。

## 1. 安装 wcode

macOS / Linux：

```bash
curl -fsSL https://raw.githubusercontent.com/francis-du/wcode/main/install.sh | sh
```

更新正在运行的 HTTP Runtime 后，可通过：

```bash
wcode restart
```

让当前实例进入完整重启路径。缓存 MCP Capability 的客户端可能需要重新连接。

## 2. 本地 Coding Agent：优先 stdio MCP

本地 Agent 使用：

```bash
wcode --workspace /absolute/path/to/repository mcp-stdio
```

持久化配置建议使用**绝对仓库路径**，避免 Host 或 Plugin 的 Working Directory 改变 Workspace。stdio 与 HTTP 共用同一 Harness、Workspace Policy、Software Intelligence、Task、Prompt、Resource 与 Tool 实现，只是不走公网 OAuth。

通用 stdio 配置形状如下；不同 Host 只是在配置文件位置和字段名上不同：

```json
{
  "command": "wcode",
  "args": ["--workspace", "/absolute/path/to/repository", "mcp-stdio"]
}
```

### 紧凑 Coding Path 与 Product Scope

正常 Coding 不要先加载整个 Control Plane。主路径是：

```text
agent_context(goal, scopes=...)
  ↓
symbol_context（仅 Readiness 要求更多源码时）
  ↓
apply_edits / apply_file_edits
  ↓
review_changes
  ↓
verify_project
```

`agent_context` 省略 `budget` 时使用有界 Adaptive Budget，并可返回相关 Design State、Scope-aware Repo Map、Fresh Semantic/Runtime Relationship、Bounded Hot Source、Exact SHA、Verification Reference、Readiness 与 Deterministic Next Actions。`workspace_info`、`scope_status`、`design_status`、`project_context`、`software_context`、`language_quality_status`、Graph/Risk/Reconciliation 工具只在任务需要时深入调用。

### Claude API MCP Connector 的延迟 Tool Loading

这是 **Claude API / MCP Connector** 的客户端优化，不是 wcode Server Flag，也不是 Claude Code CLI Setting。支持 `mcp_toolset` / Tool Search 的调用方可把大部分 wcode Tool 设为 Deferred，只保留核心 Coding Path 常驻：

```json
{
  "type": "mcp_toolset",
  "mcp_server_name": "wcode",
  "default_config": { "defer_loading": true },
  "configs": {
    "agent_context": { "defer_loading": false },
    "symbol_context": { "defer_loading": false },
    "apply_edits": { "defer_loading": false },
    "review_changes": { "defer_loading": false },
    "verify_project": { "defer_loading": false }
  }
}
```

其他 Host 如果有 Lazy/Deferred Tool Loading，也应采用相同原则；wcode 不会通过非标准 MCP 字段强迫客户端延迟加载。

## 3. 导出可移植 Skill / Agent Plugin

在目标 Repository 内生成：

```bash
wcode --workspace "$PWD" agent-plugin --output wcode-agent-plugin
```

核心布局：

```text
wcode-agent-plugin/
├── plugin.json
├── .claude-plugin/plugin.json
├── .zcode-plugin/plugin.json
├── marketplace.json
└── skills/wcode-software-intelligence/SKILL.md
```

导出包不包含通用 Hook、JS/TS/Python Executable、Credential，也不默认携带通用 `mcp.json`。原因是 Plugin Directory 不是 Source Repository，复制一个 Plugin 无法安全猜测哪个目录应成为 Workspace。ZCode 是当前例外：它提供 `${CLAUDE_PROJECT_DIR}`，因此生成 Manifest 可以声明 Workspace-scoped stdio MCP。

### 当前 Code Agent Compatibility Matrix

| Code Agent | Native Package Surface | Portable Skill | Local MCP | 推荐 wcode 路径 |
| --- | --- | --- | --- | --- |
| Grok Build | Claude-compatible Plugin | Yes | `grok mcp add` | Generated Plugin + Project stdio MCP |
| ZCode | `.zcode-plugin/plugin.json` + Marketplace | Yes | Plugin Manifest / Settings | Generated ZCode Plugin；无需公网 Tunnel |
| Claude Code | `.claude-plugin/plugin.json` + Marketplace | Yes | `claude mcp add` | Generated Plugin + Project stdio MCP |
| OpenAI Codex | `.codex-plugin/plugin.json` / Skill | Yes | `codex mcp add` | Generated Skill + Project stdio MCP |
| Cursor | Agent Plugins v1 / `.cursor-plugin/plugin.json` | Yes | `.cursor/mcp.json` | Local Agent Plugin + stdio MCP |
| Gemini CLI | `gemini-extension.json` | Yes | `gemini mcp add` | Skill + Project stdio MCP |
| GitHub Copilot CLI | Agent Plugin / Marketplace | Yes | `/plugins` → MCP | Plugin/Skill + Dashboard MCP |
| VS Code + GitHub Copilot | Agent Plugins / Copilot / Claude Format | Yes | `.vscode/mcp.json` / `.mcp.json` | Local Plugin + Workspace stdio MCP |
| Cline | Executable Plugin + MCP | Experimental Skill | `cline mcp` | Skill + stdio MCP；不要把 VS Code Extension 等同于 Executable Plugin |
| Roo Code | Mode / Skill | Yes | `.roo/mcp.json` | Shared Skill + Project stdio MCP |
| OpenCode | Executable JS/TS Plugin | Yes | `opencode.json(c)` | Shared Skill + Project stdio MCP |
| Windsurf / Cascade | 无稳定 Agent Package Contract | 不声明 Native Skill Install | `mcp_config.json` | Project stdio MCP |
| Continue | Hub/Config/Rules | 不声明 Native Skill Install | `.continue/mcpServers/*.yaml` | Project MCP + Reviewed Rules |
| Kiro | Power / Agent Plugins v1 | Yes | `.kiro/settings/mcp.json` | Power + Project stdio MCP |
| Qoder CLI | `.qoder-plugin/plugin.json` + Marketplace | Yes | `.mcp.json` | Skill + Project stdio MCP |
| Kimi Code CLI | `kimi.plugin.json` | Yes | `.kimi-code/mcp.json` | Shared Skill + Project stdio MCP |
| Qwen Code | Native Extension / Agent Plugins v1 | Yes | `qwen mcp add` | Generated Plugin + Project stdio MCP |
| TRAE | MCP Surface 已公开，Plugin Contract 随版本变化 | Rules 依版本 | MCP Settings | Project stdio MCP；具体 Plugin Claim 需按当前 Build 核验 |

## 4. Grok Build / Grok Coding Agent

Grok Build 可复用 Claude-compatible Plugin 结构。优先安装生成的 `.claude-plugin` / Skill，并把 MCP 单独指向目标 Repository 的 `mcp-stdio`。不要让 Plugin Directory 本身变成 Workspace。

### MCP

Host 提供 `grok mcp add` 时，注册等价于通用 stdio 模板的命令；Workspace 路径必须是目标 Repository。

### Skill / Plugin

使用生成包中的 Skill 与 Claude-compatible Manifest。Skill 只负责工作流，不替代 wcode Authorization / Verification。

## 5. ZCode

ZCode 使用生成的 `.zcode-plugin/plugin.json` 与本地 Marketplace。由于 Host 提供 `${CLAUDE_PROJECT_DIR}`，Manifest 可以把 stdio MCP 安全绑定到当前项目，因此本地使用不需要 Public Tunnel。

## 6. Claude Code

Claude Code 可安装生成的 `.claude-plugin` / Skill，并用 `claude mcp add` 或等价项目设置注册 stdio MCP。CLI 本身不要套用前面的 Claude API `defer_loading` 配置；那是 API Connector 能力。

## 7. OpenAI Codex

Codex 使用生成的 Portable Skill，并通过 `codex mcp add` 或当前 Codex MCP 配置把 stdio Command 绑定到绝对 Repository Path。Native Marketplace Packaging 与 wcode Skill Export 是不同层，不要混为一谈。

## 8. Cursor

Cursor 可本地加载 Agent Plugins v1 / Cursor Plugin，并在 `.cursor/mcp.json` 配置 stdio MCP。共享写操作仍走 wcode Workspace/Scheduler/SHA Boundary；Cursor Worktree/Subagent 只负责隔离推理与执行上下文。

## 9. Gemini CLI

Gemini CLI 使用 Skill / Extension Guidance，并通过 `gemini mcp add` 或当前项目 MCP 设置注册 stdio。不要把 Host Hook 当成 wcode 强制 Gate；真实 Gate 仍在 Harness / Evidence。

## 10. GitHub Copilot CLI

安装生成的 Plugin 或 Skill，并从 Copilot CLI 的 Plugin/MCP 管理界面注册 Project stdio MCP。GitHub Remote 操作如果通过 wcode `run_command` 执行，仍受 `git` / `gh` 的命令级安全策略。

## 11. Cline

Cline 同时有 MCP 和自己的 Executable Plugin Surface。wcode 默认只推荐 **Skill + stdio MCP**，不生成可执行 Cline Plugin；这样不会把脚本、Hook 或 Credential 隐藏进 Plugin 包。

## 12. Roo Code

Roo Code 使用共享 Agent Skill，并在 `.roo/mcp.json` 注册 Project stdio MCP。Mode/Rule 可以引用 Skill，但不应复制一份独立 wcode 安全策略。

## 13. OpenCode

OpenCode 使用共享 Skill，并在 `opencode.json` / `opencode.jsonc` 配置 MCP。wcode 不需要生成 Executable JS/TS Plugin 才能提供核心能力。

## 14. Windsurf / Cascade

在 `mcp_config.json` 注册 Project stdio MCP。当前指南不把 Windsurf 描述成拥有稳定的 Portable Agent Skill/Plugin Install Contract；Workflow Guidance 如需复制，应保持 Reviewed、Non-executable。

## 15. Continue

通过 `.continue/mcpServers/*.yaml` 或当前 Continue MCP 配置注册 Project MCP；可在 `.continue/rules/` 放经过 Review 的简短 Workflow Guidance，但不要复制整套 wcode Control Plane。

## 16. Kiro

Kiro 可把生成 Skill/Plugin 作为 Power 使用，并在 `.kiro/settings/mcp.json` 单独配置 Project stdio MCP。Power 负责 Guidance，MCP 负责执行。

## 17. Qoder CLI

Qoder 使用 Skill + `.mcp.json` / 当前 MCP 配置。只有在需要 Marketplace Distribution 时才额外做 Qoder Wrapper；正常 Repository Coding 不需要多一层可执行 Plugin。

## 18. Kimi Code CLI

使用共享 Skill，并在 `.kimi-code/mcp.json` 注册 Project stdio MCP。Kimi-specific Wrapper 是可选分发层，不是 wcode Runtime 必需组件。

## 19. Qwen Code

可安装/链接生成 Agent Plugin，并使用 `qwen mcp add` 或当前 MCP 设置注册 Project stdio MCP。

## 20. VS Code + GitHub Copilot

在 `.vscode/mcp.json` 或 `.mcp.json` 注册 Project stdio MCP；可同时注册生成的 Agent Plugin / Skill。Workspace 应指向真实 Repository，而不是 Extension/Plugin Installation Directory。

## 21. TRAE

TRAE 已有 MCP 接入面，但 Rules/Plugin Contract 会随产品版本变化。本指南只稳定声明 Project stdio MCP 路径；任何 Native Plugin 兼容性 Claim 都应按当前 Client Build 与 Primary Documentation 重新核验。

## 22. Cloud/Web Connector：Grok Web

Cloud/Web Connector 不能启动本机 stdio Child Process，因此使用 Runtime 输出的受保护公网 `/mcp`：

```text
https://<public-host>/mcp
```

默认 Managed Tunnel 会在 Cloudflare、`localhost.run`、Pinggy 之间做实例级 Health-verified Fallback；长期稳定部署更推荐 `--public-url https://...`。

### OAuth Validation

Remote Connector 使用 Streamable HTTP + OAuth。PKCE、Protected Resource Metadata、Resource Binding、DCR、Redirect 与 Origin Policy 仍由 wcode 控制；Tunnel 只提供 Reachability，不是 Authorization。

本地 Coding Agent 不要为了“统一配置”强行绕公网；同机场景优先 stdio。

## 23. 聊天与云端客户端矩阵

第 3 节的矩阵覆盖的是能操作仓库的 coding agent。聊天与云端客户端要按另一种方式分组：可拉起 stdio 的桌面端、接受自定义远程 MCP URL 的客户端、以及只能挂目录、够不到私有 wcode 的客户端。以下结论基于 2026 年 8 月的厂商公开文档；"未找到"表示没有公开文档，不代表永远不会支持。

国际客户端：

| 客户端 | stdio | 远程 URL | OAuth | wcode 路径 |
| --- | --- | --- | --- | --- |
| Claude Code | 支持 | 支持 | 支持 | stdio 或远程；一等公民 |
| claude.ai 网页 Connectors | 不支持 | 支持 | 支持 | 仅远程；期望 search/fetch 工具 |
| Claude Desktop | 支持 | 部分 | 部分 | stdio 配置文件；远程请用 Claude Code |
| ChatGPT Developer Mode / Apps SDK | 仅桌面版 | 支持（beta） | 支持 | 仅远程；强制 search/fetch，高风险标记 |
| Gemini CLI | 支持 | 支持 | 支持 | stdio 或远程 |
| gemini.google.com 消费版 | 不支持 | 仅目录 | — | 无法连接 |
| Gemini Enterprise / API | 不支持 | 支持 | 支持 | 仅远程 |
| VS Code + GitHub Copilot | 支持 | 支持 | 支持 | stdio 或远程 |
| GitHub Copilot CLI / coding agent | 支持 | 支持 | 支持 | 仓库 `.mcp.json` stdio |
| Cursor | 支持 | 支持 | 支持 | stdio 或远程 |
| Windsurf / Cascade | 支持 | 支持 | 支持 | stdio 或远程 |
| JetBrains AI Assistant / Junie | 支持 | 支持 | 支持 | stdio 或远程 |
| Zed | 支持 | 支持 | 需桥接 | stdio 或远程 |
| Cline / Roo Code | 支持 | 支持 | 需桥接 | stdio |
| OpenCode | 支持 | 仅 bearer | 不支持 | stdio |
| Continue | 支持 | 部分 | 不支持 | stdio |
| Grok 自定义连接器（grok.com） | 不支持 | 支持 | 支持 | 仅远程 |
| Grok Build | 支持 | 支持 | 支持 | stdio；见第 4 节 |
| Mistral Le Chat | 不支持 | 支持 | 有认证、细节未核实 | 仅远程 |
| Perplexity | macOS 应用 | 支持 | 未核实 | 仅远程 |
| Microsoft 365 Copilot | 不支持 | 管理员目录 | — | 无法连接 |
| Notion AI / Slack AI / Discord / Replit / Cody | 不支持 | 未找到 | — | 无法连接 |
| Neovim（avante / codecompanion） | 支持 | 部分 | 需桥接 | stdio |

国内客户端：

| 客户端 | stdio | 远程 URL | OAuth | wcode 路径 |
| --- | --- | --- | --- | --- |
| ZCode（智谱） | 支持 | 支持 | 支持 | 生成的 Plugin；见第 5 节 |
| Kimi Code CLI（月之暗面） | 支持 | 支持 | 支持 | stdio 或远程 |
| Qwen Code CLI（阿里） | 支持 | 支持 | 支持 | stdio 或远程 |
| CodeBuddy（腾讯） | 支持 | 支持 | 支持 | stdio 或远程 |
| Qoder CLI（阿里） | 支持 | 支持 | 未核实 | stdio；见第 17 节 |
| Trae（字节） | 支持 | SSE | 未核实 | stdio；见第 21 节 |
| Comate Zulu（百度） | 支持 | SSE | 未核实 | stdio |
| 千问 App / 通义桌面版 | 不支持 | 以市场为主 | 未核实 | 仅目录 |
| MiniMax Agent / 纳米 AI 工具箱 / 星火智能体平台 | 不支持 | 有配置入口 | 未核实 | 仅平台或目录 |
| 元宝 / 豆包电脑版 / 文心网页版 | 不支持 | 未找到 | — | 无法连接 |
| DeepSeek 官方客户端 | 不支持 | 不支持 | — | 在其他客户端里把 DeepSeek 当模型用 |
| 商汤小浣熊 / 阶跃 / Monica | 未找到 | 未找到 | — | 无法连接 |

国内 MCP 市场（魔搭广场、千帆、百炼、腾讯云、火山引擎、Trae 市场、纳米 AI 工具箱）分发的是 MCP 服务器，不是私有 wcode 的安装通道。

### 在已验证的远程客户端上注册 wcode

stdio 命令在各家完全一致，差别只在注册入口。

```bash
claude mcp add wcode -- wcode --workspace /绝对路径/仓库 mcp-stdio
claude mcp add --transport http wcode https://<公网主机>/mcp
gemini mcp add --transport http wcode https://<公网主机>/mcp
qwen  mcp add --transport http wcode https://<公网主机>/mcp
kimi  mcp add --transport http --auth oauth wcode https://<公网主机>/mcp
```

- **claude.ai Connectors**：设置 → Connectors → Add connector → 填公网 URL → OAuth。把它当作检索客户端；它期望服务端提供 `search`/`fetch`。
- **ChatGPT**：设置 → Connectors → Advanced → Developer Mode → 添加公网 URL。强制要求服务端 `search`/`fetch`，会禁用 Memory 并标记高风险。
- **Gemini / Qwen / Kimi CLI**：上面的命令；远程 URL 触发 OAuth 流，也可改用静态 header。Kimi 的 token 存于 `~/.kimi/mcp-oauth/`；配置在 `~/.qwen/settings.json` 或 `~/.kimi/mcp.json`。
- **CodeBuddy**：IDE 的 MCP 标签页或 `.codebuddy/settings.json`；远程首连有文档化的 OAuth。
- **Cursor / Windsurf / JetBrains / Zed**：同一条 stdio 命令，分别写入 `.cursor/mcp.json`、`~/.codeium/windsurf/mcp_config.json`、JetBrains 的 MCP 设置页、或 Zed 的 `context_servers`。远程 HTTP 可用但 OAuth 成熟度不一；Zed 可能需要 `mcp-remote` 桥。
- **不支持 OAuth 的客户端**：在 wcode 前加一层带静态认证的受信反向代理，或换客户端。

### 路径选择

- 单机编码：stdio + 仓库绝对路径。不需要隧道、OAuth 或公网暴露。
- 远程或网页客户端：托管隧道或稳定的 `--public-url`，注册 `https://<公网主机>/mcp` 并完成 OAuth。
- 仅目录型客户端（元宝、豆包、文心网页版、纳米 AI、gemini.google.com、M365 Copilot）：够不到私有 wcode；不要为此削弱 wcode 的安全边界。

## 24. 所有 Agent 共用的安全清单

- Workspace 必须显式，持久化配置优先绝对 Repository Path；
- 本地 Agent 优先 stdio，Cloud/Web 才使用 Public `/mcp`；
- Skill/Plugin 不携带 Credential、Shell Hook 或隐藏 Executable；
- `agent_context` 是正常 Coding 主入口，不固定预加载整个 Control Plane；
- Source Write 使用 SHA-guarded Workspace Primitive；
- Risky Command / Remote Mutation 只在精确操作层授权；模型不能批准自己的请求；
- Verification / Evidence 必须匹配当前 Revision；Model Consensus 不替代 Deterministic Proof；
- Host 自己的 Sandbox、Worktree、Subagent 是 Isolation Helper，不是 wcode Security Boundary 的替代品。

## 25. 为什么 wcode 默认不生成 Vendor-specific Executable Plugin

可执行 Plugin 往往意味着 Host-specific Runtime、Hook、脚本、Credential 或 Working-directory 语义。把这些打进一个“通用插件”会扩大 Trust Boundary，并让安全事实分叉成多份。

wcode 因此把 Portable Layer 限制为 Skill/Manifest，把执行能力统一放在 MCP，把真实 Repository 访问统一放在 Workspace Policy，把强制批准统一放在 Authorization/Verification/Evidence。只有 Host 提供明确、安全、可移植的 Declarative Contract 时才增加薄 Wrapper。

## Primary References Checked for This Compatibility Guide

Compatibility Claim 会随 Host 演进。维护这页时应优先检查每个 Host 的 Primary Documentation，并分别验证：

- Native Package / Skill Surface；
- Local stdio MCP；
- Remote MCP / OAuth；
- Workspace / Project Path Semantics；
- 是否支持 Tool Search / Lazy Loading；
- 是否真实做过 End-to-end Verification。

不要把“有 MCP”“能装 Skill”“有 Plugin Marketplace”“实际与 wcode 端到端验证过”压成一个 `supported=true`。英文与中文页面必须同步更新同一 Compatibility Matrix 与安全结论。
