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

## 2. 配置本机 Coding Agent

```bash
cd /absolute/path/to/repository
wcode setup
```

交互式 `wcode setup` 第一项是**全局（推荐）**，第二项是**当前项目**。
全局模式只修改已验证的用户级 Host 配置，一次配置即可跨仓库使用；项目模式
把配置留在当前仓库。两种模式都只安装 `wcode mcp-stdio`，保留其他 Server，
未知 Schema 直接 Fail Closed。Binary 已内嵌 Canonical Skill 与 Plugin
Metadata，所以用户项目里不需要存在 `plugin/` 目录。需要预览时用
`wcode setup --dry-run`。

## 3. 启动并连接

```bash
wcode
```

当前目录就是默认 Workspace，因此正常使用不再需要 `--workspace "$PWD"`。
这条命令会启动本地 MCP、受保护 WebUI、OAuth、TUI 和已配置的公网连接。
根目录下的项目标记会自动成为可选择的 Subspace。

### 本机 Coding Agent

Agent 和 wcode 在同一台机器时优先用 stdio：

```bash
wcode mcp-stdio
```

MCP Host 启动进程时的当前目录就是默认 Workspace。stdio 不走 HTTP OAuth，
但仍使用同一套 Workspace、命令、路径、SHA、授权、验证和 Evidence 边界。

### 云端或 Web Connector

使用 wcode 显示的公网 `/mcp` 地址。兼容客户端会发现 OAuth 元数据，通过 PKCE/DCR 完成授权，并拿到绑定到该 Resource 的 Token。

三种传输的定位如下：

- 本地 MCP：stdio。
- 远程首选：`/mcp` 上的 Streamable HTTP + OAuth。
- 旧版远程兼容：`GET /sse` + `POST /message`，同样使用 OAuth。

三种方式共用 Harness 和 Workspace 策略；SSE 不提供匿名兼容路径。

正常 Runtime 会自动处理公网连接。Tunnel Provider 选择、稳定反向代理等高级选项统一放在 [CLI 与 MCP 参考手册](../reference/)；本机正常接入不需要先理解这些参数。

OAuth Client 注册和 Token 不按时间过期。wcode 按配置的 Workspace 根目录
把它们保存在用户状态目录中，下一次启动 wcode 时会重新载入。替换隧道通过
当前实例健康校验后可以继续原会话；授权始终留在请求实际进入的域名，未知或
已经失效的 Host 仍会被拒绝。

## 4. 需要时再初始化 Design State

连接后的 Agent 可以调用 `design_init`。初始化会写入 Project/Product 状态，
以及模块行数、测试位置、Design 引用同步三条基础约束；其他集合只有在有
实际内容时才创建，也不会覆盖已有 Design State。

本地查看：

```bash
wcode intelligence
wcode intelligence --check --json
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

`agent_context` 省略 `budget` 时会选择有界 Adaptive Budget，并可携带相关 Design State、Scope-aware Repo Map、Bounded Hot Source、SHA Edit Target、Related Test、Readiness 与显式 Parallelism Guidance。模型只应发送当前动作真正需要的 MCP 参数：默认 Workspace，以及服务端默认的 Path / Limit / Timeout / Budget 都应省略。先按依赖拆 Lane；Host 支持时，独立 Discovery、Read、Review 和 File-local Edit 用多个顶层 Tool Call 并发，真实依赖才串行。输入已经明确时优先用 `read_files`、`search_many`、`apply_file_edits`、`create_files`；`parallel_tools` 只用于参数很小的紧凑 Fan-out。普通定位继续走 `find_symbol` / `search_code`，只有 Readiness 需要更强跨文件关系时才调用 `semantic_navigation`。

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
- `P`：查看并确认 Full Access；它会把当前用户 Home 与其他可授权 Runtime
  能力显式放开，但 Protected Path、Symlink/Hard-link、No-shell 与 Filesystem
  Root 等硬边界继续保留。

受保护 WebUI 处理同一批请求，并把“可执行程序访问”和“精确仓库操作”
分开显示；批准一层不会自动放开另一层。

## 7. 常用运行模式

```bash
wcode --read-only
wcode --no-exec
wcode --no-semantic
wcode --no-monitor
wcode --open
```

除非任务确实需要，不要改变默认安全和资源姿态。高级 Transport / Resource 参数统一放在 [CLI 与 MCP 参考手册](../reference/)；Trust Boundary 控制见 [安全模型](../security/)。
