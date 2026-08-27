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

临时 Quick Tunnel 在重启后可能换地址；需要稳定入口时使用 `--public-url`。

## 4. 需要时再初始化 Design State

连接后的 Agent 可以调用 `design_init`。初始化保持稀疏：只创建 Project/Product 状态，不预建空的 Requirement、Component、Constraint、Acceptance、Decision 集合，也不会覆盖已有 Design State。

本地查看：

```bash
wcode --workspace "$PWD" intelligence
wcode --workspace "$PWD" intelligence --check --json
```

## 5. 让 Agent 先做正确的发现

改代码前推荐先调用：

```text
workspace_info
scope_status
design_status
project_context
software_context
```

根据 wcode 返回的 Product Scope 收窄 `software_context`，再进行大范围源码读取。批量发现优先用 `read_files` 和 `search_many`；只有在独立操作已经明确时才用 `parallel_tools`。

改完后依次看：

```text
review_changes
drift_status
impact_analysis
risk_status
verify_project
```

真正的通过条件来自风险自适应 Verification 与 Evidence，不来自模型自己的“看起来没问题”。

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
wcode --workspace "$PWD" --no-open
wcode --workspace "$PWD" --public-url https://mcp.example.com
```

除非任务确实需要，不要扩大默认信任边界。进一步配置见 [安全模型](../security/) 和 [Agent 与 MCP 集成](../code-agent-integrations/)。
