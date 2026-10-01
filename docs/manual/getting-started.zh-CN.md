---
layout: docs
title: 快速开始
description: 安装、启动并把 wcode 接入一个代码仓库
lang: zh-CN
alternate: /docs/getting-started/
permalink: /zh/docs/getting-started/
---

# 先让一个仓库真正跑通 wcode

wcode 不是另一个 Coding Agent，而是现有 Agent 调用的本地仓库层：需要理解代码、确认跨文件关系、受控修改或证明当前 Revision 时，由 wcode 提供稳定的工程上下文与边界。

本地智能体只需三步：**安装 → `wcode setup` → 重新连接智能体**。智能体会启动自己的 stdio 进程，不必额外启动 HTTP 服务或公网隧道。云端 / 网页连接器才需要运行 `wcode` 服务并使用已验证的公网 MCP 地址。

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
把配置留在当前仓库。两种模式都为 `wcode mcp-stdio` 保存所选性能及限制性权限参数，保留其他 Server，
未知 Schema 直接 Fail Closed。Binary 已内嵌 Canonical Skill 与 Plugin
Metadata，所以用户项目里不需要存在 `plugin/` 目录。需要预览时用
`wcode setup --dry-run`。
要为已检测到的智能体保存快速预设，先用 `wcode setup --performance fast --dry-run`
预览，再运行 `wcode setup --performance fast`，最后重新连接智能体。
加上 `--project` 可把配置限制在当前仓库。

## 3. 启动并连接

云端 / 网页连接器，或需要独立 TUI / WebUI 时，运行下面的服务。
第 2 步配置好的本地智能体会自行启动 stdio，不必为了接入再开第二个服务。

```bash
wcode
```

当前目录就是默认 Workspace，因此日常使用不再需要 `--workspace "$PWD"`。
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

Runtime 默认会自动处理公网连接。Tunnel Provider 选择、稳定反向代理等高级选项统一放在 [CLI 与 MCP 参考手册](../reference/)；本机接入不需要先理解这些参数。

OAuth Client 注册继续持久保存，不设置时钟 TTL。Access Token 在一小时后过期；响应通过 `expires_in: 3600`、`Cache-Control: no-store` 和 `Pragma: no-cache` 明示生命周期。Refresh Token 的空闲 TTL 为 30 天，只有成功刷新轮换才重新计算。轮换移除同一 Grant 的旧 Access Token，其他 Owner 不受影响。过期、为零或未来签发时间失败关闭。状态按配置的 Workspace 根目录保存；重启与迁移保留原签发时间，不延长 Token 生命周期。替换隧道通过当前实例健康校验后，可继续仍有效的会话；授权始终留在请求实际进入的域名。

本地操作者可以通过 `GET /oauth/sessions` 检查会话，并通过 `POST /oauth/sessions/revoke` 撤销 `session_id`。这些管理 API 遵循现有 Host／Origin 检查并要求当前 `X-Wcode-UI-Token`，不接受普通 MCP Bearer 作为授权。会话列表不暴露凭据。成功撤销持久保存，重启不会恢复；存储错误会移除运行中的凭据，但不确认持久撤销。它们不是 RFC 7009 接口或 Team ACL／SSO；Team ACL 与 SSO 尚未实现。

## 4. 老项目逐步补充 Design

不需要先把整个仓库建模完，源码搜索、受控编辑和原生检查就能开始使用。全局 setup 只配置智能体；显式项目 setup（`wcode setup --project`，或交互选择“当前项目”）会在 Design 完全缺失时创建 `.wcode/project.yaml` 和空的 `.wcode/design/` 目录。项目名取目录名，描述为空，不写入 Policy，也不推测 Product 愿景、需求或组件映射。

`--dry-run` 只报告计划，不写文件。已有、部分存在或无效的 Design 都会保留并提示检查，不重置、不静默修复。启动 wcode 和普通只读工具不会进行这项初始化。仅有 `.wcode` 目录不代表 Design 完整、可追溯或 Acceptance 已通过。

让已连接的 Agent 每次补一个真实行为，不需要用户手写整套 YAML。Agent 应先展示草稿，对未知业务意图向你确认：

1. 让 Agent 读取 README、项目清单和 CI 配置，再用 `software_graph`、`file_outline`、`find_symbol` 检查实际代码，用 `project_context` 发现真实检查项。由你确认它提出的预期行为，未知意图明确保留，不把每个现有实现都倒推成需求。
2. 补一条 Requirement → Component → 实现路径/Symbol，以及 Requirement → Acceptance → 真实 Test/Check。映射就在这些记录中，没有独立的 `mappings.yaml` 格式。
3. 新文件用 `create_files`；已有文件先用 `read_files` 取得 SHA，再用 `apply_file_edits` 受控修改，没有 `design_update` 工具。完全未初始化的 Workspace 可以显式调用 `design_init` 创建更完整的 Product/核心约束骨架；项目 setup 已生成元数据后，应继续补充，不要再次初始化。
4. 先检查 `design_status`，再看 `traceability_status` 和 `drift_status`。`reconciliation_plan` 可以把缺口转成持久任务计划，但不会自动编辑或修复项目。之后审查变更并运行 `verify_project`；只有当前代码与 Design Revision 的实际 Evidence 能证明执行。

Agent 的 Rust 草稿可以采用下面的集合文件格式，业务行为、源码路径和测试 Symbol 必须换成已经检查过的真实对象。这是格式示例，不是已有的整仓 Design 自动生成功能：

```yaml
# .wcode/design/requirements.yaml
- schema_version: 1
  id: REQ-SESSION
  title: 拒绝过期会话
  intent: 过期会话不能访问服务。
  implemented_by: [component:session]
  acceptance: [AC-SESSION]

# .wcode/design/components.yaml
- schema_version: 1
  id: component:session
  name: 会话
  responsibilities: [验证会话有效期]
  implementation:
    - kind: file
      path: src/session.rs

# .wcode/design/acceptance.yaml
- schema_version: 1
  id: AC-SESSION
  title: 过期会话被拒绝
  statement: 过期会话回归测试拒绝访问。
  verification:
    - kind: test
      path: tests/session.rs
      symbol: rejects_expired_session
```

集合文件使用 YAML 列表；拆到 `design/requirements/`、`design/components/`、`design/acceptance/` 目录时，每个文件是单个对象。ID 必须唯一，引用必须可解析。已发现的 Symbol 可写为组件的 `{kind: symbol, path: src/session.rs, symbol: validate_session}`；只有实际发现对应检查时，Acceptance 才能声明 `{kind: check, id: rust-test}`。映射能解析不代表测试已经运行或通过。

项目 setup 还可能建议 Acceptance Policy 草稿；只有单独的交互 TTY 确认能写草稿，dry-run/JSON 不确认，已有 Policy 字段保持不变。可信激活仍需独立的原生预览与精确操作者批准；基础元数据初始化和安装确认都不会激活 Policy。

本地查看：

```bash
wcode intelligence
wcode intelligence --check --json
```

处理声明的 Design 与覆盖缺口后，再使用严格 `--check` 门禁；渐进接入中的不完整状态应如实保留。完整格式及验证流程见 [Software Intelligence](../software-intelligence/)。

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

`agent_context` 省略 `budget` 时会选择有界自适应预算，并可携带相关 Design State、按 Scope 收窄的仓库地图、有界热源、SHA 编辑目标、关联测试、就绪度（Readiness）与显式并行指引。模型只应发送当前动作真正需要的 MCP 参数：默认 Workspace，以及服务端默认的 Path / Limit / Timeout / Budget 都应省略。先按依赖拆 Lane；Host 支持时，独立 Discovery、Read、Review 和 File-local Edit 用多个顶层 Tool Call 并发，真实依赖才串行。输入已经明确时优先用 `read_files`、`search_many`、`apply_file_edits`、`create_files`；`parallel_tools` 只用于参数很小的紧凑 Fan-out。普通定位继续走 `find_symbol` / `search_code`，只有就绪度要求更强跨文件关系时才调用 `semantic_navigation`。

复用已经发现的工具 Schema 和 `agent_context` 已提供的上下文；`project_context` 不是第二个必需启动调用。并行 Lane 数是受运行时上限约束的建议，不代表写操作一定独立。在紧凑 `parallel_tools` 批次中，后续任务在自身前置任务完成后即可启动，不再等整层结束；失败依赖会跳过后续分支，独立工作继续执行。

改完后默认：

```text
review_changes
verify_project
```

Change 或 Readiness 要求更深分析时，再补 Drift / Impact / Risk / Reconciliation / Evidence。真正的通过条件来自风险自适应 Verification 与 Evidence，不来自模型自己的“看起来没问题”。

## 6. 本地操作界面

TUI 主屏优先展示连接状态和 Subspace 活动，移除了重复的 OVERVIEW 面板。
较矮终端使用紧凑顶栏；仅在有近期流量且空间充足时显示 30 秒吞吐。
常用快捷键：

- `I`：打开仓库理解视图。
- `W`：打开当前 Workspace 的受保护 Engineering Observatory。
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

### 浏览工程观测台

先在工作区选择器中选中具体项目。默认的**工程架构**页从系统地图开始，组件和依赖详情可在该视图中继续展开。

| 页签 | 回答什么问题 |
| --- | --- |
| 总览 | 哪些情况需要关注，最近发生了什么？ |
| 工程架构 | 代码归哪些系统和组件负责，它们有什么关系？ |
| 任务活动 | 什么任务正在运行或排队，耗时与内存用在哪里？ |
| 验证证据 | 当前版本有哪些证据，还有哪些问题未解决？ |
| 当前变更 | 哪些文件变了，会影响什么，需要哪些检查？ |
| 需求 | 需求如何对应到实现和验证证据？ |
| 项目文件 | 当前快照包含哪些源码，哪些文件最大？ |

语言和主题按钮在顶栏，**访问**按钮打开项目与命令权限面板。窄屏下可横向滚动页签栏，查看其余页面。

理解数字前，先看状态标签：

- **已映射（Mapped）**表示验证引用可以解析；**已执行（Executed）**表示存在符合要求的执行证据。
- **已通过（Passed）**描述所观测版本的有效结果；**当前版本（Fresh）**表示证据与当前代码和设计一致。
- **语法（Syntax）**表示 Tree-sitter 关系；语义精度需要真实运行的分析器和匹配的源码版本。
- **已截断（Truncated）**表示有界视图并不完整。快照不可用或 Git 状态未知，不代表仓库为空。

页面看起来未更新时，先检查当前工作区和刷新状态，再点**立即刷新**。暂停**自动刷新**会停止自动更新，包括已排队的后台快照重建；暂停时仍可手动刷新，即使先显示缓存，也会继续完成这次更新。**已显示缓存 · 后台刷新…**表示画面仍在更新，不代表当前版本的验证已经通过。页面隐藏时暂停轮询；开启自动刷新后，返回页面会继续更新。运行时更换后如果授权失效，从 TUI 按 **W** 重新打开当前受保护地址。

## 7. 常用运行模式

用 `wcode help-all` 查看所有支持的 CLI 命令和参数，包括普通帮助隐藏的高级选项。
`wcode help-all setup` 聚焦单个命令；`wcode help-all --json` 输出机器可读目录。
查看帮助不会启动服务或执行被查询命令。

默认保持均衡预算。需要更多容量时用 `wcode --performance fast`，需要较小
预算时用 `wcode --performance light`；`wcode --show-config` 只展示最终配置，
不启动服务。智能体配置也可以使用 `wcode mcp-stdio --performance fast`。
预算、单项覆盖和生效方式见 [CLI 与 MCP 参考手册](../reference/)。

```bash
wcode --read-only
wcode --no-exec
wcode --no-semantic
wcode --no-monitor
wcode --open
```

除非任务确实需要，不要改变默认安全和资源姿态。高级 Transport / Resource 参数统一放在 [CLI 与 MCP 参考手册](../reference/)；Trust Boundary 控制见 [安全模型](../security/)。
