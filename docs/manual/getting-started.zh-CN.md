---
layout: docs
title: 快速开始
description: 安装、启动并把 wcode 接入一个代码仓库
lang: zh-CN
alternate: /docs/getting-started/
permalink: /zh/docs/getting-started/
---

# 快速开始

这页只保留从现有仓库到可用 wcode Runtime 的最短路径。

## 1. 安装

macOS 与 Linux：

```bash
curl -fsSL https://raw.githubusercontent.com/francis-du/wcode/main/install.sh | sh
```

Windows PowerShell：

```powershell
irm https://raw.githubusercontent.com/francis-du/wcode/main/install.ps1 | iex
```

## 2. 在仓库中启动

```bash
cd /absolute/path/to/repository
wcode --workspace "$PWD"
```

正常启动会带起本地 MCP、受保护的 WebUI/Setup Hub、OAuth、终端监控，以及供云端 Connector 使用的公网入口。

默认只暴露一个仓库根目录。只有一个任务确实需要多个仓库时再显式添加：

```bash
wcode \
  --workspace ~/Code/backend \
  --workspace ~/Code/frontend
```

## 3. 选择连接方式

### 本机 Coding Agent

Agent 和 wcode 在同一台机器时优先用 stdio：

```bash
wcode --workspace /absolute/path/to/repository mcp-stdio
```

stdio 不走 HTTP OAuth，但仍使用同一套 Workspace、命令、路径、SHA、授权、验证和 Evidence 边界。

### 云端或 Web Connector

使用 wcode 显示的公网 `/mcp` 地址。兼容客户端会发现 OAuth 元数据，通过 PKCE/DCR 完成授权，并拿到绑定到该 Resource 的 Token。

默认 `--tunnel-provider auto` 会在后台并发启动 Cloudflare Quick Tunnel、SSH 隧道 `localhost.run`、Pinggy 以及 Tailscale Funnel；面板立即渲染，每条通过实例级健康检查的隧道都会实时列出。错过首轮的 Provider 每 15 秒持续重试，单条隧道死亡只会独自重拉，不影响其余隧道。可用 `--tunnel-provider cloudflare|localhost-run|pinggy|tailscale` 强制指定。Quick Tunnel 的地址重启后可能变化；需要稳定入口时使用 `--public-url` 或 Tailscale Provider。

## 4. 需要时再初始化 Design State

连接后的 Agent 可以调用 `design_init`。初始化保持稀疏：只创建 Project/Product 状态，不预建空的 Requirement、Component、Constraint、Acceptance、Decision 集合，也不会覆盖已有 Design State。

本地查看：

```bash
wcode --workspace "$PWD" intelligence
wcode --workspace "$PWD" intelligence --check --json
```

## 5. 让 Agent 先做正确的发现

改代码前的强默认路径现在只有一个紧凑入口：

```text
agent_context(goal, scopes=...)
  ↓
按 readiness / next_actions 执行
  ↓
只有缺更多源码时才调用 symbol_context
```

`agent_context` 省略 `budget` 时会选择有界 Adaptive Budget，并可携带相关 Design State、Scope-aware Repo Map、Bounded Hot Source、SHA Edit Target、Related Test 与 Readiness。只有任务确实需要更深发现时，再调用 `scope_status`、`design_status`、`project_context`、`software_context`、`language_quality_status`、`read_files` 或 `search_many`；只有独立操作已经明确时才使用 `parallel_tools`。

改完后默认：

```text
review_changes
verify_project
```

Change 或 Readiness 要求更深分析时，再补 Drift / Impact / Risk / Reconciliation / Evidence。真正的通过条件来自风险自适应 Verification 与 Evidence，不来自模型自己的“看起来没问题”。

## 6. 本地操作界面

TUI 快捷键保持简单：

- `I`：打开 Intelligence 视图。
- `W`：打开当前 Workspace 的受保护 Project Observatory。
- `O`：重新打开 Setup Hub。
- `L`：手动切换界面语言。
- `+`：添加 Workspace。
- `↑/↓`：选择待授权请求。
- `Y/N`：批准或拒绝当前请求。

受保护 WebUI 提供对应的项目与命令授权入口。

## 7. 常用运行模式

```bash
wcode --workspace "$PWD" --read-only
wcode --workspace "$PWD" --no-exec
wcode --workspace "$PWD" --no-monitor
wcode --workspace "$PWD" --open
wcode --workspace "$PWD" --tunnel-provider localhost-run
wcode --workspace "$PWD" --tunnel-provider pinggy
wcode --workspace "$PWD" --tunnel-provider tailscale
wcode --workspace "$PWD" --public-url https://mcp.example.com
```

除非任务确实需要，不要扩大默认信任边界。进一步配置见 [安全模型](../security/) 和 [Agent 与 MCP 集成](../code-agent-integrations/)。
