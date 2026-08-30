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

这条命令会启动本地 MCP、受保护 WebUI、OAuth、TUI 和已配置的公网隧道。

默认只暴露一个仓库根目录。只有一个任务确实需要多个仓库时再显式添加：

```bash
wcode \
  --workspace ~/Code/backend \
  --workspace ~/Code/frontend
```

根目录下的项目标记会成为可选 Subspace。例如从 `~/Code` 启动后，Agent
可以直接选择 `Rust/wcode`，不需要再注册一个手工重叠 Workspace。

## 3. 连接 Agent

已知的本地 Host 可以先预览、再写入项目配置：

```bash
wcode --workspace "$PWD" agent-plugin --install-all --dry-run
wcode --workspace "$PWD" agent-plugin --install-all
```

第一条不会写文件；第二条只合并已确认的 JSON/TOML Schema，保留其他
MCP server，并把需要手工处理的 Host 明确列出来。

### 本机 Coding Agent

Agent 和 wcode 在同一台机器时优先用 stdio：

```bash
wcode --workspace /absolute/path/to/repository mcp-stdio
```

stdio 不走 HTTP OAuth，但仍使用同一套 Workspace、命令、路径、SHA、授权、验证和 Evidence 边界。

### 云端或 Web Connector

使用 wcode 显示的公网 `/mcp` 地址。兼容客户端会发现 OAuth 元数据，通过 PKCE/DCR 完成授权，并拿到绑定到该 Resource 的 Token。

三种传输的定位如下：

- 本地 MCP：stdio。
- 远程首选：`/mcp` 上的 Streamable HTTP + OAuth。
- 旧版远程兼容：`GET /sse` + `POST /message`，同样使用 OAuth。

三种方式共用 Harness 和 Workspace 策略；SSE 不提供匿名兼容路径。

默认 `--tunnel-provider auto` 会在后台并发启动 Cloudflare Quick Tunnel、SSH 隧道 `localhost.run`、Pinggy 以及 Tailscale Funnel；面板立即渲染，每条通过实例级健康检查的隧道都会实时列出。错过首轮的 Provider 每 15 秒持续重试，单条隧道死亡只会独自重拉，不影响其余隧道。可用 `--tunnel-provider cloudflare|localhost-run|pinggy|tailscale` 强制指定。Quick Tunnel 的地址重启后可能变化；需要稳定入口时使用 `--public-url` 或 Tailscale Provider。

OAuth Client 注册和 Token 不按时间过期。wcode 按配置的 Workspace 根目录
把它们保存在用户状态目录中，执行 `wcode restart` 后会重新载入。替换隧道
通过当前实例健康校验后可以继续原会话；授权始终留在请求实际进入的域名，
未知或已经失效的 Host 仍会被拒绝。

## 4. 需要时再初始化 Design State

连接后的 Agent 可以调用 `design_init`。初始化会写入 Project/Product 状态，
以及模块行数、测试位置、Design 引用同步三条基础约束；其他集合只有在有
实际内容时才创建，也不会覆盖已有 Design State。

本地查看：

```bash
wcode --workspace "$PWD" intelligence
wcode --workspace "$PWD" intelligence --check --json
```

## 5. 让 Agent 先做正确的发现

改代码前先从一个紧凑入口开始：

```text
agent_context(goal, scopes=...)
  ↓
按 readiness / next_actions 执行
  ↓
只有被推荐的跨文件关系任务才调用 semantic_navigation
  ↓
只有缺更多源码时才调用 symbol_context
```

`agent_context` 省略 `budget` 时会选择有界 Adaptive Budget，并可携带相关 Design State、Scope-aware Repo Map、Bounded Hot Source、SHA Edit Target、Related Test 与 Readiness。普通定位继续用 `find_symbol` / `search_code`；当 Readiness 判断任务涉及 Syntax-only 的跨文件关系时，`semantic_navigation` 会复用 Warm LSP Session 查询 Reference、Caller、Callee 或 Implementation。只有任务确实需要更深发现时，再调用 `scope_status`、`design_status`、`project_context`、`software_context`、`language_quality_status`、`read_files` 或 `search_many`；只有独立操作已经明确时才使用 `parallel_tools`。

改完后默认：

```text
review_changes
verify_project
```

Change 或 Readiness 要求更深分析时，再补 Drift / Impact / Risk / Reconciliation / Evidence。真正的通过条件来自风险自适应 Verification 与 Evidence，不来自模型自己的“看起来没问题”。

## 6. 本地操作界面

TUI 主屏只保留连接状态、四项运行指标、Subspace 活动和 30 秒吞吐。
常用快捷键：

- `I`：打开 Intelligence 视图。
- `W`：打开当前 Workspace 的受保护 Project Observatory。
- `O`：重新打开 Setup Hub。
- `L`：手动切换界面语言。
- `+`：添加 Workspace。
- `↑/↓`：选择待授权请求。
- `Y/N`：批准或拒绝当前请求。

受保护 WebUI 处理同一批请求，并把“可执行程序访问”和“精确仓库操作”
分开显示；批准一层不会自动放开另一层。

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
