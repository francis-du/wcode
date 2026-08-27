---
layout: docs
title: Agent 与 MCP 集成
description: wcode 与本地 Coding Agent、云端 Connector、Skill 和 Plugin 的接入方式
lang: zh-CN
alternate: /docs/code-agent-integrations/
permalink: /zh/docs/code-agent-integrations/
---

# Agent 与 MCP 集成

wcode 把“怎么接入”拆成三层，不再把 Plugin、Skill 和 MCP 混为一谈。

## MCP：真正的执行能力层

本机 Coding Agent 优先使用 stdio：

```bash
wcode --workspace /absolute/path/to/repository mcp-stdio
```

云端或 Web Connector 使用 wcode 显示的 Streamable HTTP `/mcp` 地址，并通过 OAuth 完成认证。

HTTP 与 stdio 最终进入同一套 Workspace、命令策略、SHA 写入保护、Software Intelligence、Verification 与 Evidence Runtime。

## Skill：工作流说明层

Skill 负责告诉 Agent 如何发现 Workspace、Design State、Product Scope、上下文和验证能力。Skill 本身不授予文件、Shell、网络或凭据权限。

推荐的最小工作流：

```text
workspace_info
scope_status
design_status
project_context
software_context
```

修改后：

```text
review_changes
drift_status
impact_analysis
risk_status
verify_project
```

## Plugin：客户端分发容器

导出可移植能力包：

```bash
wcode --workspace "$PWD" agent-plugin --output wcode-agent-plugin
```

导出的包只包含声明式 Metadata、README 和 Skill，不携带 Hook、脚本、凭据或隐式 Workspace。客户端支持哪种 Plugin/Extension/Power Manifest 由客户端自己决定。

## 为什么本地 Agent 优先 stdio

- 不需要公网隧道。
- 不需要浏览器 OAuth。
- Workspace 由启动命令中的绝对路径固定。
- 文件与命令安全规则和 HTTP 模式一致。

## 为什么云端 Connector 需要公网地址

Grok、Claude、ChatGPT 等云端产品从厂商基础设施发起请求，无法访问你的 `localhost`。公网入口只负责网络可达，真正的权限仍由 OAuth 和 wcode 的 Workspace/Authorization Policy 决定。

## 常见错误

### 把 `/mcp` URL 填进 Marketplace

Marketplace 通常需要 Git 仓库、目录或 Marketplace Manifest；`/mcp` 是 MCP Server 地址，两者不是同一个输入。

### `fetch failed` 或 Connector 不可用

按顺序检查：

1. 本地 Agent 是否应该改用 stdio；
2. 当前公网 `/mcp` URL 是否仍有效；
3. Quick Tunnel 是否在重启后换了地址；
4. 客户端是否重新执行了 Discovery、DCR 与 OAuth；
5. Redirect URI、PKCE、Resource Binding 和 Origin 是否精确匹配。

不要通过关闭 OAuth、扩大 Workspace 或全局自动批准来掩盖握手问题。

## 授权请求

模型请求的非默认裸可执行程序名，以及需要仓库信任的精确操作，都可以进入按 Workspace 隔离的 Pending Authorization 列表。操作者在 TUI 或受保护 WebUI 选择性批准，模型不能自我授权。

## 供应商配置

不同 Agent 的配置字段和安装命令变化很快。英文版页面保留更长的逐供应商配置与来源清单；中文页保持稳定的接入模型、诊断顺序和安全边界，避免把短期厂商 UI 文案复制到多个地方。
