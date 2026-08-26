---
layout: wiki
title: WIKI
description: wcode documentation, Code Agent setup, and troubleshooting index
lang: zh-CN
permalink: /wiki/
---

# WIKI

这是面向使用者的入口页。它帮助你选择正确的接入方式，但不复制各厂商
经常变化的安装命令。完整命令、配置文件、诊断步骤和官方来源统一维护在
[Code Agent 集成指南](code-agent-integrations/)。

## 先判断你在连接什么

| 使用场景 | 应选方式 | wcode 入口 |
| --- | --- | --- |
| 本机 CLI、IDE 或 Coding Agent | stdio MCP | `wcode --workspace /absolute/repo mcp-stdio` |
| Grok、Claude 等云端 Connector | Streamable HTTP + OAuth | wcode TUI 显示的公网 `/mcp` URL |
| 只需要复用工作流说明 | Agent Skill | `skills/wcode-software-intelligence/SKILL.md` |
| 客户端支持可安装能力包 | Agent Plugin / Extension / Power | `wcode agent-plugin` 导出的目录 |

本地 Code Agent 优先使用 stdio。只有客户端运行在厂商服务器上、无法访问
你的本机进程时，才需要公网 URL、Quick Tunnel 和 OAuth。

## Plugin、Skill 和 MCP 不是一回事

- **Plugin / Extension / Power** 是客户端的安装与分发容器。
- **Skill** 是按需加载的工作流说明，本身不授予文件、Shell 或网络权限。
- **MCP** 提供真正可调用的 wcode Tools、Prompts 和 Resources。
- 一个 Plugin 可以包含 Skill 和 MCP，但不同客户端使用不同 manifest 和
  marketplace；不能把 `/mcp` URL 填进 marketplace 输入框。

## Code Agent 快速入口

详细配置按同一顺序集中在主指南：

- [Grok Build](code-agent-integrations/#4-grok-build--grok-coding-agent)、[ZCode](code-agent-integrations/#5-zcode)、[Claude Code](code-agent-integrations/#6-claude-code)、[OpenAI Codex](code-agent-integrations/#7-openai-codex)
- [Cursor](code-agent-integrations/#8-cursor)、[Gemini CLI](code-agent-integrations/#9-gemini-cli)、[GitHub Copilot CLI](code-agent-integrations/#10-github-copilot-cli)、[Cline](code-agent-integrations/#11-cline)
- [Roo Code](code-agent-integrations/#12-roo-code)、[OpenCode](code-agent-integrations/#13-opencode)、[Windsurf](code-agent-integrations/#14-windsurf--cascade)、[Continue](code-agent-integrations/#15-continue)
- [Kiro](code-agent-integrations/#16-kiro)、[Qoder CLI](code-agent-integrations/#17-qoder-cli)、[Kimi Code CLI](code-agent-integrations/#18-kimi-code-cli)、[Qwen Code](code-agent-integrations/#19-qwen-code)
- [VS Code + GitHub Copilot](code-agent-integrations/#20-vs-code--github-copilot)、[TRAE](code-agent-integrations/#21-trae)
- [Grok Web Custom Connector](code-agent-integrations/#22-cloudweb-connector-grok-web)

## 最常见的连接错误

### `Failed to fetch marketplace: 401 Unauthorized`

这通常表示把受 OAuth 保护的 `/mcp` 地址当成了 marketplace。Marketplace
输入框需要 GitHub 仓库、Git URL、本地目录或真正的 `marketplace.json`；
MCP 地址应填写到客户端的 MCP Server 设置。

### `fetch failed` 或 `connector unavailable`

先确认连接类型：

1. 本地客户端检查 stdio command、绝对 Workspace 路径和可执行文件路径。
2. 云端 Connector 检查当前公网 `/mcp` URL；Quick Tunnel 重启后 URL 会变化。
3. 旧授权失败过时，删除连接并重新添加，让客户端重新执行 Discovery、DCR
   和 OAuth。
4. 不要通过关闭 OAuth、扩大 Workspace 或全局自动批准来掩盖握手错误。

## 云端 Connector 的统一合约

Grok、Claude、ChatGPT、Mistral 等远程客户端共享同一套 wcode 服务端行为，
不会为某个品牌维护一套放宽的鉴权或 Workspace 逻辑。连接应同时满足：

- MCP 协议版本和必要请求头协商成功；
- Protected Resource Metadata 与 Authorization Server Metadata 可达；
- OAuth Authorization Code、PKCE、DCR 和 Redirect URI 精确匹配；
- Authorization Code、Access Token 和 Refresh Token 绑定同一个 `/mcp` Resource；
- 浏览器请求通过 Origin 校验；
- Tools、Prompts、Resources、Tasks 和 Product Scope Discovery 使用同一运行时；
- HTTP 与 stdio 最终进入同一套 Workspace、命令、SHA、Evidence 和人工授权边界。

排查 OAuth/DCR 时按 Resource Identity、Discovery、Redirect URI、PKCE、
Resource Binding、DCR Metadata、Origin、Protocol Headers 的顺序检查。日志不得
输出 Access Token、Refresh Token、PKCE Verifier 或其他凭据。

## 安全默认值

- Workspace 使用仓库绝对路径，不要指向主目录或文件系统根目录。
- 本地接入优先 stdio，不为本地 Agent 暴露公网隧道。
- 安装 Plugin 前检查 hooks、脚本、MCP command 和凭据处理。
- 保留客户端审批机制；wcode 的 Workspace 防护不能替代客户端权限控制。

## 其他文档

这些文档有独立职责，因此保留为单独页面，并从这里统一进入：

- [Software Intelligence](software-intelligence/) / [中文指南](software-intelligence-zh-cn/)
- [Agentic Engineering Model](agentic-engineering/)
- [Product Scopes](product-scopes/)
- [Language Quality](language-quality/)
- [Maintainability Review](maintainability-review/)
- [v0.3.0 Release Notes](releases/v0.3.0/)
- [开发者说明](development/)

需要复制配置时，请进入
[完整集成指南](code-agent-integrations/)，不要从 README、网站介绍文案或旧聊天
记录复制可能过时的厂商命令。
