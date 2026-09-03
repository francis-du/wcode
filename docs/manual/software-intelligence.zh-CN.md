---
layout: docs
title: Software Intelligence 中文指南
description: wcode Software Intelligence Runtime 已实现能力与使用流程
lang: zh-CN
alternate: /docs/software-intelligence/
permalink: /zh/docs/software-intelligence/
---

# 别再让 Agent 每次会话都重新猜你的代码库

Coding Agent 写代码可以很快，但仍可能看错代码周围的系统。wcode 在仓库旁维护一份本地、可复用的软件模型，让不同 Agent 在修改前后都能回答四个实际问题：

1. **这个系统本来应该是什么样？** —— Requirement、Component、Constraint、Decision、Acceptance。
2. **它现在到底怎么工作？** —— Syntax 结构，加上真实 LSP Reference、Caller、Implementation，以及外部 Runtime / Compiler Relationship。
3. **这次修改会碰到什么？** —— Git-aware Impact、Product Scope、Drift、Public API / Security Signal 和 Maintainability Risk。
4. **凭什么相信改对了？** —— 确定性检查、语言原生验证、独立 Review，以及绑定当前 Revision 的 Evidence。

这就是“AI 能搜索仓库”和“**Agent 能据以推理的软件智能**”之间的区别。同一份状态通过 MCP、本地 CLI、TUI 和受保护 Project Observatory 提供，并且不会随着一次聊天结束而消失。

## 60 秒理解 wcode 的软件智能

```text
你正在使用的 Coding Agent
      ↓ 请求任务上下文
agent_context
      ↓ 聚焦代码 + Design + Test + Semantic Gap
Tree-sitter ── Warm LSP ── Design State ── Git Actual State
      ↓
Impact / Drift / Risk
      ↓ 受控编辑
Verification / Reviewer / Evidence
      ↓
Project Observatory + 持久 Workspace State
```

Project Observatory 把同一份模型变成人能直接读懂的视图：Desired State → Actual State → Change → Proof → Convergence，并把持久 Workspace State 留在会话之外。Software Graph 是保留 Provenance 的底层能力，不要求用户先看懂一张“球图”才能理解项目。

本地 `mcp-stdio`、远程 Streamable HTTP + OAuth 和旧版 SSE 共用一个 MCP
Core。`agent_context` 是紧凑编程入口；Design、Graph、Verification 工具
按任务需要再调用。插件导出复用 binary 内嵌的 canonical `plugin/` 源包，包含标准
`mcp.json`，不会携带凭据或隐式 Workspace。持久状态按 Workspace 隔离。
只有真实、Fresh 的 LSP Provider Fact 才标成 Semantic；否则明确保持
Tree-sitter Syntax Precision。

## 现在怎么用

日常仓库流程现在是：

```bash
wcode setup
wcode
```

当前目录就是默认 Workspace。不连接模型也可以直接查看本地 Runtime 状态：

```bash
wcode intelligence
wcode intelligence --check --json
wcode verification
wcode verification --plan-id VP-...
```

`intelligence --check` 会把只读状态面变成 fail-closed 的 CI / Release Gate：Design State 未初始化或无效、Requirement→Component / Design→Implementation / Acceptance→Verification 任一维度低于 100%、以及 Required Convention 出现 Error 时都会返回非零退出码。只有当 Design State 显式声明 `CONSTRAINT-PRODUCT-SCOPE-CANONICAL` 时，Product Scope 扫描截断或存在未映射受支持源码才会成为硬门槛；普通第三方仓库仍可查看 `scope_status`，但不会被强制套用 wcode 自身的 12 个 Product Scope 目录模型。JSON 会同时带出 Runtime 使用的 `scope_status` 与 `conventions`；Convention Warning 仍保持建议性质，不会阻断发布。

LSP Server 和 Stage Executor 都可能加载/执行仓库控制的配置或代码，因此不共用一条笼统的 Trust 规则。拥有 Hardened Profile 的第一方 LSP Server 默认通过独立有界 Lane 自动运行；当前首个 Auto Profile 是 `rust-analyzer`。`--no-semantic` 可以关闭全部第一方 LSP 执行。没有 Auto Profile 的 LSP Server 与 Stage Executor 仍属于显式 Trust Expansion：第一次尚未授权的精确操作会 fail closed 并生成本地 Authorization Request，可在 TUI 或受保护 WebUI 批准后重试；`--allow-risky-exec` 仍是这些非 Auto 操作更宽的进程级预授权方式：

```bash
wcode --no-semantic
wcode --allow-risky-exec verification --plan-id VP-... --execute-stages
```

在 TUI 中，按 `I` 会读取当前选中项目的智能分析，按 `C` 查看完整命令
清单，按 `W` 打开受保护的项目观测页。客户端连上以后，配对码仍会留在
页头，重连时不必再猜它藏在哪里。TUI 和 WebUI 处理同一批待授权请求，
并把“可执行程序访问”和“精确仓库操作”分开。

项目观测页的文件树来自当前有界软件图谱快照，会列出目录深度、最大文件
以及超过仓库 1,000 行上限的文件。索引达到安全上限时，页面会明确标记
“已截断”，浏览器不会另外再扫一遍磁盘。

Proof 只统计与当前代码和设计版本一致的 Evidence。本地 Agent 在项目工作目录中使用
`wcode mcp-stdio`，当前目录自动成为默认 Workspace；远程客户端首选 `/mcp`，旧客户端可以
使用 `/sse`。插件导出和一键 Host 配置见
[Agent 与 MCP 集成](../code-agent-integrations/)。

常规编码链路现在更短：

```text
agent_context(goal, scopes=...)
    ↓ readiness / next_actions / parallelism
独立 Lane ── 多个顶层 MCP Call 并发
    ↓ 只有真实依赖才串行
有界编辑 → review_changes → verify_project
    ↓
只有需要更深收敛分析时再进入 drift / risk / reconciliation / evidence
```

## 1. 安装 / 更新 wcode

安装最新 Release：

```bash
curl -fsSL https://raw.githubusercontent.com/francis-du/wcode/main/install.sh | sh
```

执行 `wcode update` 后，由终端或 MCP Host 在下一次启动时直接运行新版本。某些 MCP 客户端会缓存 `tools/list`，因此升级后可能还需要在客户端里重新连接或刷新一次 MCP 才能拿到新 Schema。

## 2. 给项目加入 Design State

Design-aware 工具会读取当前 Workspace 下的：

```text
.wcode/project.yaml
.wcode/design/
```

如果 Workspace 还没有初始化并且允许写入，可以直接让 Agent 调用：

```json
{
  "name": "my-service",
  "description": "Example service managed with wcode Design State."
}
```

对应 MCP 工具是 `design_init`。它会安全创建**稀疏的** `.wcode/` Desired State，已经存在的 Design 文件不会被覆盖。

wcode 自己已经在 Dogfood 这套格式。初始化时只创建真正有意义的 Project / Product：

```text
.wcode/
├── project.yaml
└── design/
    └── product.yaml
```

`requirements.yaml`、`components.yaml`、`constraints.yaml`、`acceptance.yaml`、`decisions.yaml` 不再为了凑 Schema 预先创建成空 `[]` 文件。只有真正出现对应 Desired State 时才创建集合文件。也支持拆成单文件目录：

```text
.wcode/design/requirements/
.wcode/design/components/
.wcode/design/constraints/
.wcode/design/acceptance/
.wcode/design/decisions/
```

### project.yaml

```yaml
schema_version: 1
name: my-service
description: Example service managed with wcode Design State.
```

### Requirement

```yaml
- schema_version: 1
  id: REQ-AUTH-001
  title: Refresh tokens rotate
  intent: Reusing an already-consumed refresh token must fail.
  priority: critical
  implemented_by:
    - component:auth
  acceptance:
    - AC-AUTH-001
  constraints:
    - CONSTRAINT-ROTATION
  risk:
    security: critical
```

### Component → Code 映射

```yaml
- schema_version: 1
  id: component:auth
  name: Authentication
  responsibilities:
    - issue and rotate refresh tokens
  constraints:
    - CONSTRAINT-ROTATION
  implementation:
    - kind: symbol
      path: src/integrations/auth.rs
      symbol: refresh_access_token
```

### Acceptance → Verification 映射

```yaml
- schema_version: 1
  id: AC-AUTH-001
  title: Refresh token reuse is rejected
  statement: A consumed refresh token cannot be exchanged twice.
  verification:
    - kind: test
      path: src/integrations/auth.rs
      symbol: tests::refresh_tokens_rotate_and_preserve_binding
    - kind: check
      id: rust-test
```

Design State 会校验 ID、跨对象引用以及代码/测试路径是否合法。

其中 Symbol / Test 的解析仍然来自 Tree-sitter，所以会明确标记为：

```text
provider = tree-sitter
precision = syntax
```

它不会伪装成编译器级类型、重载、宏展开或动态分派语义。

## 3. 推荐 Agent 工作流

较大修改也不再固定预加载一串 Broad Status Tool：

```text
1. agent_context(goal, scopes=...)
2. 按 readiness / next_actions / parallelism 执行；Host 支持时把独立 Lane 并发出去
3. 省略默认 / 可推导 MCP 参数；只有 Readiness 明确要求跨文件引用 / 调用 / 实现关系时才用 semantic_navigation，普通定位继续用 find_symbol / search_code
4. 只有缺源码时调用 symbol_context
5. apply_edits / apply_file_edits
6. review_changes
7. 只有任务需要时再运行 language_quality_run / drift / impact / risk
8. verify_project + 必需高级 Stage
9. 只有 Convergence / Proof 需要深入时再进入 evidence_status / reconciliation
```

`agent_context` 省略 `budget` 时使用有界自适应预算，组合相关 Design State、按 Scope 收窄的仓库地图排序、可用时的新鲜语义/运行时证据、有界热源、精确 SHA 编辑目标、关联测试、工作区提示、就绪度（Readiness）、确定性下一步动作与显式并行策略。模型侧 MCP 调用应省略默认 Workspace 以及服务端默认的 Path / Limit / Timeout / Budget。1,000 token 的下限模式优先保证直接可编辑；默认自适应档位会在任务模糊或跨模块时自动放大。`project_context`、`scope_status`、`design_status`、`traceability_status`、`software_context`、`language_quality_status` 与 Graph/Risk 工具保留为按需深入，而不是固定启动成本。

### `agent_context`

日常编码优先调用 `agent_context`。它把过去多次启动发现合成一个有界、可直接编辑的上下文包：仓库地图排序同时使用任务相关性与 Software Graph 关系；新鲜的语义/运行时/确定性证据可以增强关系，过期的 Provider 事实自动回退语法精度。当任务明确涉及 Caller、Reference、Implementation、Rename Impact 或其他跨文件关系，而且当前图只有语法精度时，就绪度信息才会推荐 `semantic_navigation`；普通 Symbol 定位不承担这笔 LSP 成本。性能遥测放在 Tool Result 的 `_meta` 里，模型可见上下文只保留做决策真正需要的信息。

### Product Scope

wcode 现在有一份统一的产品能力 Scope Registry：`runtime`、`integrations`、`workspace`、`design`、`graph`、`semantics`、`traceability`、`risk`、`verification`、`evidence`、`reconciliation`、`experience`。`workspace_info` 和 `project_context` 会返回这份 Registry；`scope_status` 会把 Registry 应用到当前 Workspace，返回每个 Scope 的源码数量以及有界的未映射源码路径；`tools/list` 会给每个 Tool 的 `_meta.dev.wcode/productScopes` 附上所属 Scope；支持 MCP Resource 的客户端还可以读取 `wcode://runtime/product-scopes`。同一份实时 Scope Audit 也会进入本地 Intelligence Operator View。

Product Scope 描述的是 wcode 自身能力边界，不是模型厂商，也不会取代业务领域的自由 Semantic Scope。已知别名会被规范成 canonical scope；未知字符串仍作为自由业务 Scope 保留。`software_context.scopes` 对已识别 Product Scope 会真正收窄源码/符号导航；`semantic_query.scopes` 会过滤有 Scope 的 Semantic Fact，而没有 Scope 的 Fact 继续作为全局语义参与查询。详细映射见 [product-scopes.md](../product-scopes/)。

### `software_context`

当你的任务是从“业务行为 / Requirement / 子系统”出发，而不是已经知道具体文件名时，优先使用它。

示例：

```json
{
  "query": "workspace command security",
  "scopes": ["workspace"]
}
```

它会先规范可选的 `scopes`，再把 Query 拆成 Token 做相关性评分，并让 `budget` 真正控制返回量。已识别的 Product Scope 会把源码/符号导航收窄到对应 Source Root；如果没有传入可识别的 Product Scope，源码导航仍覆盖整个 Workspace。结果包括：

- Requirement / Component / Constraint
- Acceptance Criterion / Decision
- 带 title、summary、relations 的结构化 `design_items`
- Tree-sitter Symbol
- 当前 Runtime 已知 Risk
- 有界 Traceability Coverage
- `graph_context`：从 fresh Semantic / Runtime Provider Graph 中按任务文本、Semantic Expansion Token、已命中 Symbol Path 联合评分得到的有界一跳邻域，并保留 Provider / Precision

这样 Agent 可以直接拿到设计意图、符号以及真实语义/运行时关系，而不是先拿一串 ID 再回头读 YAML。

### `traceability_status`

它会尝试解析：

```text
Requirement
  → Component
  → File / Symbol
  → Acceptance Criterion
  → Test / Harness Check
```

Coverage 不会被压成一个总分，而是分别返回：

- Requirement → Component
- Design → Implementation
- Acceptance → Verification

### 22 种语言的 LSP 支持

`semantic_provider_status` 会扫描 Workspace，并对当前语法索引已经支持的全部 22 种语言报告真实 Provider 状态：Bash、C、C++、C#、CSS、Dart、Elixir、Go、HTML、Java、JavaScript、Lua、OCaml/Interface、PHP、Python、R、Ruby、Rust、Swift、TypeScript、TSX。

v0.5 把这一层收紧成显式 Compatibility Contract，不再把“Registry 里有一个 Provider 名字”当成支持。22 种索引语言每一种都必须恰好拥有一个 Canonical Launch Profile；Provider-specific Arguments 有单测锁定；而且每个 Canonical Profile 都会真实 Spawn 一个 stdio Mock LSP 并完成 `initialize` Handshake。Alternate 只保留真实可用的实现。Compatibility 与 Runtime Availability 仍然分离：Executable 没安装，或者真实 `initialize` 失败时，wcode 都不会宣称它 Runnable。`semantic_provider_status` 还会明确返回当前选中 Candidate 是否 Canonical，以及本机实际找到几个 Candidate。

| 语言 | Canonical LSP Launch Profile | 已安装时可用的 Alternate |
| --- | --- | --- |
| Bash | `bash-language-server start` | — |
| C / C++ | `clangd` | — |
| C# | `csharp-ls` | — |
| CSS | `vscode-css-language-server --stdio` | — |
| Dart | `dart language-server --protocol=lsp --client-id wcode --client-version <version>` | — |
| Elixir | ElixirLS `language_server.sh` / `language_server.bat`（同时识别发行版 `elixir-ls` Wrapper） | — |
| Go | `gopls serve` | — |
| HTML | `vscode-html-language-server --stdio` | — |
| Java | `jdtls -data <per-workspace-state-dir>` | — |
| JavaScript / TypeScript / TSX | `typescript-language-server --stdio` | — |
| Lua | `lua-language-server` | — |
| OCaml / Interface | `ocamllsp` | — |
| PHP | `phpactor language-server` | `intelephense --stdio` |
| Python | `pyright-langserver --stdio` | `pylsp` |
| R | `R --no-echo -e languageserver::run()` | — |
| Ruby | `ruby-lsp` | `solargraph stdio` |
| Rust | `rust-analyzer` | — |
| Swift | `sourcekit-lsp` | — |

拥有 Alternate 的语言，在前台 Navigation 和手工 Semantic Refresh 两条路径里都会在 Canonical Provider 已安装但 Initialize 失败时尝试 Alternate。Alternate 仍然经过自己独立的 Trust Boundary；Refresh 成功切换后会在 `fallbacks` 中显式记录，绝不会把一个 Provider 的 Grant 偷偷扩大到另一个 Provider。

Runtime 会自动维护拥有显式 Hardened Profile 的 Provider。后台 Worker 只选择最具体的 Discovered Project Workspace，源码需要连续经过一个短暂稳定窗口才刷新，失败后做有界指数退避，并且每次真实刷新都必须先获取与 Model-facing Work 共用的 Global Harness Semaphore。Harness 还维护一个有容量上限的 Warm Session Pool，以 Workspace + LSP Server + 当前 Binary Identity 为 Key；后台索引和前台语义导航复用同一个活跃 Session。Coordinator 会周期性回收 Idle 且未被 Lease 的 Slot；容量驱逐绝不会删除正在使用的 Slot；全部 Slot 都 Busy 时 Fail Closed，而不是短暂超过进程数上限。LSP Binary Identity 变化时，如果旧 Slot 仍被 Lease，也会先要求当前请求结束，再允许替换。这样 Broad Root 与嵌套 Subspace 不会重复索引，也不会每次语义查询都重启 rust-analyzer。`semantic_provider_refresh` 继续保留为强制 Refresh Surface。

Warm Session 的 Document Sync 现在严格跟随 Server 返回的 `textDocumentSync` Contract：Numeric Full/Incremental 兼容形态按完整 Open 处理；Options 形态尊重 `openClose`；Full Change 发送整文档，Incremental Change 使用旧内容在已协商 UTF-8/UTF-16/UTF-32 Position Encoding 下计算合法 Replacement Range；None 不会硬发 Server 没声明支持的 Change；只有 Server 要求 Open/Close Sync 时才发送 `didClose`。Refresh 使用这条 Session 请求真实 hierarchical Document Symbol；Server 支持 Call Hierarchy / Implementation 时，只对高价值 Symbol 做有界 Relationship Expansion，不再为每个变量和字段浪费请求。第一方 LSP Node 携带 `source_sha256`，因此 Provider Status 会明确给出 `fresh / stale`；源码变化后 Stale Semantic Revision 自动退出 Software Graph、Impact、Reconciliation 和 `software_context.graph_context`。Graph Revision Key 仍由源码 Hash、Provider 二进制元数据和 Symbol 上限决定：输入未变时跳过 Graph 重建，但 Runtime 可以只 Warm 一次 Session，让后续 Semantic Query 不再承担启动成本。返回空符号集的 Server 也不会制造一个假的语义 Revision。

Auto Execution 是 Provider-specific Safety Profile，不是“LSP 全部免授权”。当前 `rust-analyzer` Profile 会拒绝解析到 Workspace 内的可执行文件，清理凭据与执行注入环境变量，并通过 Initialization Options 关闭 Build Script、Proc Macro、Cargo Auto Reload 与 Check-on-save。这会显著缩小默认执行面，但不是 OS Sandbox：LSP Server 仍可能读取项目元数据和配置。`--no-semantic` 是 Fail-closed Opt-out。检测到但没有 Auto Safety Profile 的 LSP Server，需要绑定 Workspace + Server + 当前 Binary Identity 的 `RiskyExecution` Grant；Refresh/Navigation 只能复用这一个已批准的 Warm Session，Executable 被替换以后旧 Grant 自动失效。只有操作者明确扩大整进程 Trust 时才使用 `--allow-risky-exec`。

没有可安全运行或已安装的 Provider 时，Tree-sitter 仍提供 `precision=syntax` 的基础图。SCIP / Compiler / Runtime 等其他 Provider 继续使用 `graph_provider_import`，两条路径共享同一个带 Provenance 的 Software Graph。

### `semantic_navigation`

`semantic_navigation` 专门解决纯文本搜索不完整的 Relationship 问题：需要语义解析的 Definition / Hover、Reference、Implementation、Incoming Caller、Outgoing Callee，或一组有界的 Impact 关系。优先传 `path + symbol`；wcode 先用 Tree-sitter 定位 Symbol，再把 1-based UTF-8 Byte Position 转成 LSP Server 协商出的 Position Encoding，因此 Agent 不需要自己算 UTF-16 Offset。已经掌握精确源码位置的调用方也可以直接传 `line + character`。

`intent` 决定实际发哪些 LSP Request：`definition`、`hover`、`references`、`incoming_calls`、`outgoing_calls`、`calls`、`implementations`、`impact`。其中 `impact` 偏向跨文件完整性，只查询 Reference、Incoming Caller 和 Implementation，而不是把所有 LSP 能力都扫一遍。Result 会把 `unsupported` 与 `failures` 分开：空 Relationship List 只表示“这个能力受支持、请求成功、没有匹配关系”；LSP Timeout/Error 会单独暴露，绝不会被当成 Negative Semantic Evidence。没有可信 LSP Server 时，Tool 明确返回 `precision=syntax` 与 `routing=tree_sitter_fallback`，不会伪装成 Semantic Precision。普通“这个 Symbol 在哪”仍然使用 `find_symbol` / `search_code`，让 LSP 成本只花在真正需要 Semantic Completeness 的任务上。

TUI Intelligence 会把 Installed `available`、Policy/Trust `launch-ready`、已真实 Initialize 的 `validated`、Runnable/Fresh 状态分开，再显示 Warm Session 数、已同步 Document 数、待授权与缺失 Server 数、Provider Start 次数和 Fresh/Stale 状态，可以直接判断问题发生在安装、权限、启动还是语义新鲜度。

### Language Quality Matrix

wcode 不再用一个 `supported=true` 描述语言能力。`language_quality_status` 复用同一套 22 语言 canonical surface，分别展示 Syntax、Semantic、Format、Lint、Type Check、Static Analysis、Test、Security、Property、Mutation、Fuzz、Runtime-Canary。仓库 Manifest、依赖、配置文件、package script 和语言原生项目结构决定“这个项目声明了什么”；已知生态工具可以作为候选出现，但没有 Repository declaration 就不会被当成项目质量策略。缺少可执行程序或质量维度会明确显示为 gap。

`language_quality_run` 只执行 Matrix 中 detected + declared + available + check-only 的 provider，并继续经过正常的 Repository-aware Authorization。这个通道没有 formatter/fixer 写模式，不会借“检查”偷偷修改源码。真实 Command Result 会转成 Verification Report 并记录为当前 code+design Revision Evidence，所以旧版本的 Pass 不会冒充当前版本已经验证。

目前 Registry 能识别 Rust / Go / Python / JS/TS/CSS/HTML / C/C++ / .NET / Java / Dart / Elixir / Bash / Lua / OCaml / PHP / R / Ruby / Swift 的主流原生或仓库声明质量链，但这不等于本机全部安装。实际状态以 `language_quality_status` 为准。详见 [language-quality.md](../language-quality/)。

### Graph History / Query / Diff

`software_graph` 会持久化并去重真正有变化的 Graph Snapshot。`graph_history` 查看历史，`graph_query` 查询某个 Snapshot / 邻域，`graph_diff` 可以显式比较两个 Revision，也可以默认比较最近两个 meaningful Snapshot。Node 用稳定 `node.id` 对齐；Edge 用 `from + to + kind + provider + precision` 对齐，Revision / Attributes 改动归为 `changed`，不会噪声式地报成“整条边删除后重建”。同一稳定 Edge Identity 下出现多条 Revision 时按 multiset 对齐，避免未来更复杂 SCIP / Runtime Provider 丢关系。Project Observatory 用这些 Snapshot 展示架构 Revision 时间线和最新 Node/Edge `+ / - / ~`；每个功能的 Actual Architecture 则在刷新时基于当前仓库重新生成。

### Change Intelligence

下面这组工具分析当前 Git Working Tree 与 Design State：

| Tool | 用途 |
| --- | --- |
| `drift_status` | 检测 Implementation Drift 与 Design Drift：Design 已声明但实现/验证链不完整，或 Desired State 改了而 Actual State 没有对应变化；以及反向的“实现变了、Design 没跟上”。当前 Drift 是保守启发式，不是编译器级程序等价。 |
| `impact_analysis` | 把变更路径映射到声明的 Component、Requirement、Acceptance、实现 Symbol、有界反向调用者、公共 API 与安全边界信号。有真实 LSP / Runtime Provider 事实时优先消费这些带 provenance 的边，否则退回 Tree-sitter 语法调用边；Provider / Precision / 截断状态始终显式。 |
| `risk_status` | 把 Git 变更审查、Traceability 缺口、Drift 与 Design 声明的风险合并为结构化 Risk，并生成风险自适应 Verification Profile。 |
| `reconciliation_plan` | 把 Design Change、Drift、Impact、Risk、Reconciliation Task、Change IR Intent 与 Verification Plan 组织成有界收敛计划并持久化。 |
| `reconciliation_status` / `reconciliation_history` | 重连 / 重启后重载或列出持久化计划。 |
| `reconciliation_execution_status` | 读取持久化、依赖感知的执行状态；Verification / Human Approval 任务只根据真实证据推进。 |
| `reconciliation_claim` / `reconciliation_submit` / `reconciliation_retry` | 认领可运行的设计/实现/审查任务，写入成败与 Evidence，显式重排队失败任务。源码修改本身仍走普通 wcode 编辑工具及其安全不变量。 |

Plan 生成后会进入**持久化 Reconciliation Execution 状态机**：执行者领取任务、提交结果，重启或换模型后仍可继续。

这组工具内部使用有界的 Git 变更审查路径，因此需要命令执行，`--no-exec` 下不可用。

`review_changes` 会报告三类可复查的结构信号：`maintainability-file-crossed-1k` 表示本次修改让一个未删除源码文件越过 1,000 行；`maintainability-concentrated-growth` 表示单个源码文件净增至少 400 行；`maintainability-cross-scope-churn` 表示源码变更覆盖至少 3 个 Product Scope，且总改动不少于 1,000 行。这些信号用于提醒审查，不直接替代设计判断。Convention Engine 也用 1,000 行边界检查整个仓库。详见 [maintainability-review.md](../maintainability-review/)。

## 4. Verification Mesh

`verification_plan` 会根据当前 Risk Level 生成验证策略，并创建 Blind Independent Reviewer Job。

当前确定性验证级别映射：

```text
LOW      → quick
MEDIUM   → full
HIGH     → full
CRITICAL → full
```

Plan 还会包含：

- require_property
- require_mutation
- require_fuzz
- require_human_approval
- reviewer roles

Medium 及以上风险的 Plan 会额外加入一个盲审 `maintainability` Reviewer Job，Capability 是 `maintainability_review`。它会检查是否能通过删除分支/Helper/Layer 来简化实现、是否在堆叠零散 Special Case、逻辑是否留在 canonical Product Scope/Layer、1,000 行边界是否需要拆分，以及独立工作并行/相关状态原子化能否让结构更简单。Correctness Pass 不能代替这份结构审查。

Verification Plan 和 Reviewer Job 会按 Workspace 持久化，因此 wcode 重启或换模型后可以继续领取/提交同一个 Plan。

`verification_executor_status` 会返回跨语言 Executor Registry，并区分“已经注册”和“本机真实可执行”；`verification_execute_stages` 会运行当前 Stage 下**所有适用且真实可用**的 Executor，只跳过“这个 Producer 自己已经有最新 Pass Evidence”的 Runner，并把每次真实 Command Result 分别写成 Stage Evidence。`verification_status` 会保留每个 Producer 的最新结果，并按 `Fail > Disagree > Inconclusive > Pass` fail-closed 聚合，所以另一个晚到的 Pass 不能盖掉真实 Runner 的 Fail。CI 或其他外部系统仍然可以用 `verification_stage_submit` 提交真实 Verdict、Producer、Summary 与 Artifact Digest；Workspace Code Revision 变化后旧 Plan 仍会被 stale-revision blocker 阻止。

wcode 会自动发现一批常见生态，例如 Rust proptest/quickcheck/cargo-fuzz/cargo-mutants、Go Property/Fuzz、Python Hypothesis/mutmut、JS/TS fast-check/Stryker、Java jqwik/PIT、C# FsCheck/.NET Stryker、SwiftCheck/Muter、Elixir StreamData、Dart Glados、Ruby Rantly、PHP Eris/Infection、OCaml QCheck、R quickcheck。这些内置发现只是便捷适配器，不是封闭清单。对于其他框架和所有 22 种语言，都可以通过同一份 `.wcode/executors.yaml` 接入：

```yaml
schema_version: 1
executors:
  - id: service-canary
    stage: runtime_canary
    languages: [go]
    program: ./tools/check-canary
    args: [--environment, staging]
    cwd: .
    timeout_seconds: 60
```

配置 Executor 无 Shell 执行、沿用 Workspace canonical root / symlink 防护、对状态/UI 隐藏配置参数，并清理敏感环境和输出。没有进程级 `--allow-risky-exec` 时，第一次尚未授权的精确 Executor 操作会生成本地 `RuntimeExecutor` Authorization Request；TUI 或受保护 WebUI 批准后重试即可。需要整进程预授权时使用：

```bash
wcode --allow-risky-exec verification --plan-id VP-... --execute-stages
```

缺少可执行程序时只会报告 unavailable / missing executor，不会生成假的 Pass Evidence。

### MCP 2026 长任务 Tasks

在 MCP `2026-07-28` 下，wcode 已支持官方 `io.modelcontextprotocol/tasks` Extension，而且严格按**每个请求显式 opt-in**：客户端必须在该请求的 `_meta.io.modelcontextprotocol/clientCapabilities.extensions` 中声明 Tasks。当前只把两个明确的长耗时工具 Task 化：`semantic_provider_refresh` 和 `verification_execute_stages`；没有声明 Tasks 的客户端继续走原来的同步 `tools/call`，兼容行为不变。

Task Handle 返回前状态已经持久化；Owner 使用当前 OAuth `client_id` 的 SHA-256 Fingerprint，不保存原始 Bearer Token。`tasks/get` 轮询并在完成时返回原始 Tool Result；`tasks/update` 当前是 ack-only，因为这两个任务不会发 Input Request；`tasks/cancel` 先持久化 `cancelled`，再 Abort Worker，避免迟到的 Completed 覆盖取消。Task Store 有 Workspace 级容量上限，只会在创建新 Task 前回收 Terminal Task，Active Task 不会为了腾空间被删。如果 Runtime 在 Task 仍是 `working` 时被替换，下一次读取会把它标成 Failed，而不是假装 Worker 跨进程存活。

### Independent Reviewer Job

先创建：

```text
verification_plan
```

然后 Reviewer 调用：

```text
verification_claim
```

例如 Correctness Reviewer：

```json
{
  "reviewer": "reviewer-a",
  "capabilities": ["correctness_review"],
  "role": "correctness"
}
```

当前 Capability 名称包括：

```text
correctness_review
maintainability_review
architecture_review
security_review
adversarial_review
design_review
performance_review
compatibility_review
test_synthesis
```

第一轮是 Blind Review，领取 Job 的 Reviewer 不会看到其他 Reviewer 的提交内容。

提交结论：

```json
{
  "job_id": "VJ-00000001",
  "reviewer": "reviewer-a",
  "submission": {
    "verdict": "pass",
    "summary": "No correctness issue found.",
    "claims": ["The stale-write precondition is preserved."],
    "risks": [],
    "model": "provider/model/version"
  }
}
```

然后通过：

```text
verification_status
```

查看：

- queued / claimed / submitted
- reviewer failures / inconclusive
- disagreements
- 当前 Change Subject 的 deterministic verification result
- blockers
- 最终 `ready` 状态

如果不同 Reviewer 的 Verdict 发生冲突，wcode 会把“存在争议”本身记录成：

```text
EvidenceResult::Disagree
```

而不是简单用多数票把争议盖掉。

## 5. Evidence

现在 `verify_project` 完成后，会把确定性检查结果写入 Runtime Evidence。声明验证引用被实际执行到的 Acceptance，同样会获得对应证据。

例如：

- compiler / cargo check
- static analysis / clippy / format / diff check
- integration test
- acceptance criterion 对应的验证结果

Reviewer Submit 也会形成 Model Review Evidence，并记录：

- producer
- model
- design revision
- code revision
- verification policy
- result
- confidence
- timestamp

读取：

```json
{
  "subject": "AC-AUTH-001",
  "limit": 50
}
```

也可以不传 `subject`，查看当前 Workspace 最近的 Evidence。

### Persistence Model

下面这些状态已经按 Workspace 持久化到 wcode 的用户级 State 目录，不写进 Git 仓库：

- Evidence：有界不可变 Record
- Verification Plan / Reviewer Job：不可变 Snapshot
- Semantic Fact：Candidate / Confirmed / Retired 不可变 Revision
- External Graph Provider：按 Provider 保存最新有 provenance 的 Revision
- Software Graph：有界历史 Snapshot，可通过 `graph_history` / `graph_query` 查询，并通过 `graph_diff` 直接比较结构变化
- Reconciliation Plan：不可变 Plan Artifact
- Reconciliation Execution：可恢复的 Task 状态 Snapshot
- MCP Tasks：有界不可变 Task Snapshot；Owner 绑定 OAuth Client Fingerprint

这些存储都不会修改 Git 仓库，也不要求 Workspace 可写。

因此进程重启、断开 MCP、切换 Model Executor 后，Software Intelligence 的持久状态仍可恢复。正在执行的 MCP `working` Task 不会伪装成跨进程继续运行：Runtime 被替换后下一次读取会标为 Failed。`Risk` 会基于最新 Design / Git / Code 重新计算；第一方 LSP Provider 会额外检查 Source Hash Freshness，stale Revision 不会进入新构建的 `software_graph`。

## 6. 当前 MCP Tool Surface

### Desired State / Semantic / Graph

```text
agent_context
design_init
design_status
traceability_status
software_context
semantic_status / semantic_query
semantic_record / semantic_confirm / semantic_retire
semantic_provider_status / semantic_provider_refresh
software_graph
graph_provider_import / graph_provider_status
graph_history / graph_query / graph_diff
```

### Change Intelligence / Reconciliation

```text
review_changes
drift_status
impact_analysis
risk_status
reconciliation_plan
reconciliation_status / reconciliation_history
reconciliation_execution_status
reconciliation_claim / reconciliation_submit / reconciliation_retry
```

### Verification / Evidence

```text
verification_plan
verification_claim / verification_submit
verification_executor_status / verification_execute_stages
verification_stage_submit
verification_approve
verification_status / verification_history
verify_project
evidence_status
```

### 本地产品界面

```text
# 在仓库目录中运行；当前目录就是默认 Workspace
wcode intelligence
wcode --no-semantic                       # 完全关闭第一方 LSP
wcode intelligence --refresh-semantic     # 强制一次 Refresh
wcode verification
wcode --allow-risky-exec verification --plan-id VP-... --execute-stages
TUI: I = 当前项目分析, C = 完整命令清单, W = 项目观测页
Web: /intelligence（本机高熵 UI Token 保护；包含架构、文件树和大文件视图）
```

### 原有底层工具

```text
workspace_info
scope_status
project_context
convention_status
search_code / search_many
file_outline
find_symbol
symbol_context
read_file / read_files
read_media  # 默认只读 Metadata；Image/Audio Payload 需要当前请求显式声明 run.francis.wcode/media-content
path_info
parallel_tools
replace_text / write_file / apply_edits / apply_file_edits
create_file / create_files / create_directory
move_path / move_paths
delete_path
run_command
```

## 7. 已实现能力与精度边界

### 已实现

- Structured Design State + `design_init`
- 统一的 12 个 Product Scope Registry：驱动源码架构、`workspace_info` / `project_context`、`software_context.scopes`、`semantic_query.scopes`、Convention Scope Mapping、MCP Tool `_meta.dev.wcode/productScopes` 和 `wcode://runtime/product-scopes` Resource，同时继续支持自由业务 Scope；`scope_status` 会审计当前 Workspace 的真实映射并返回有界的未映射源码路径
- 跨语言 Convention Policy 与仓库结构检查：`project_context` 会携带有界报告，也可用 `convention_status` 单独查看，其中包含 Product Scope Mapping Gap
- 独立 Workspace 读写任务的依赖感知调度：按路径冲突排序，并在同一文件、同一 SHA 且 Edit Range 不冲突时安全合并 `apply_edits`
- `delete_path` 的精确一次性人工授权：只能删除普通文件或空目录；文件删除要求当前 SHA-256，递归删除、Workspace Root、受保护路径、Symlink Alias、Hard-linked File 永久禁止
- 持久化 Semantic Registry；Conversation/Model 推断默认只能成为 Candidate，必须显式人工确认后才进入权威 Context
- Design + Tree-sitter + Fresh 第一方 LSP + External Provider Composite Software Graph
- 全部 22 种索引语言的第一方 LSP 支持；真实 LSP Document Symbol / Call Hierarchy / Implementation 才会成为 `precision=semantic` 事实，并支持 Source Hash Freshness / Stale Exclusion / Revision Cache
- 可唯一判定的跨文件 Syntax Calls、Graph History / Query / Diff，以及外部 SCIP/Compiler/Runtime Provider Import Contract
- Requirement → Component → Code/Test Traceability
- 低上下文成本的 `agent_context`：自适应预算、按 Scope 收窄的仓库地图、Revision 感知缓存、多查询单趟搜索、有界热源、新鲜语义/运行时排序、精确 SHA 目标、工作区提示、就绪度与下一步动作；更深层仍保留预算感知 / 语义感知的 `software_context`
- Drift、Transitive Impact、结构化 Risk，以及针对 1,000 行阈值跨越、单文件集中增长、跨 Product Scope 大规模 churn 的确定性 Maintainability Finding
- Risk-Adaptive Verification + 持久化 Blind Reviewer Job；Medium 及以上风险会加入独立 `maintainability_review`，并继续保留 Disagreement Evidence、HumanApproval Evidence、Verification History、Stale Revision Gate，以及 per-producer fail-closed Stage 聚合
- 跨语言 Property / Mutation / Fuzz / Runtime-Canary Executor Registry：常见生态自动发现 + 通用 `.wcode/executors.yaml` + 外部 Stage Evidence Adapter；自动执行会跑所有适用可用 Runner
- MCP 2026 Tasks：对 `semantic_provider_refresh` / `verification_execute_stages` 提供 per-request opt-in 的 Durable Task、Owner Scope、Get/Cancel 和同步兼容路径
- 持久化 Reconciliation Plan + dependency-aware Claim / Submit / Retry 执行状态机 + Reconciliation Evidence
- `wcode intelligence --refresh-semantic` / `wcode verification --execute-stages` CLI
- TUI 项目分析、完整命令清单和持续显示的配对码；受保护的项目观测页包含架构总览、设计与实现依赖对照、组件与需求详情、验收和验证、约束与决策、受限文件树、大文件与 1,000 行越界提示、代码统计、Git 改动映射和架构版本历史
- `read_media` 的 Capability-aware 多媒体边界：PNG/JPEG/GIF/WebP 可返回尺寸，常见音频与 MP4/WebM 可识别 Metadata；只有当前请求显式声明匹配的 `run.francis.wcode/media-content` Client Capability 时才返回 Image/Audio Content，能力未知或不支持时 Fail Closed，视频始终 Metadata-only
- 完整高阶 MCP Surface

### 明确精度 / 集成边界

- 永远可用的内置代码索引仍是 Tree-sitter `precision=syntax`；第一方 LSP Adapter 只有在本机真实 LSP Server 返回结果后才把对应事实标成 `precision=semantic`，外部 SCIP / Compiler / Runtime Provider 仍走带 `provider + precision + revision` 的 Import Contract。
- 全部 22 种语言共享同一套 LSP / Verification Executor 架构，但 wcode 不捆绑每一种第三方 LSP 和测试二进制；`semantic_provider_status` / `verification_executor_status` 会展示本机真实 availability，不把缺失程序算成已安装。
- 拥有 Hardened Profile 的第一方 LSP Server 可以通过有界 LSP Lane 自动刷新，并可用 `--no-semantic` 关闭；没有 Profile 的 LSP Refresh 与 Property / Mutation / Fuzz / Runtime-Canary 继续要求显式操作者信任，`--allow-risky-exec` 仍是进程级预授权；这些机制都不是 OS Sandbox。
- Model-facing Command Execution 对内置 Development CLI Catalog 使用 Command-specific Policy，对有界 Repository/Remote Operation 使用精确 `RiskyExecution` Fingerprint。Repository Mutation 只允许显式 Path 的 `git add`、Message-only `git commit`、显式 Remote+Ref 的 Non-force `git push` 进入授权；批准后的 SSH Push 只通过固定非交互 SSH 命令使用当前 SSH Agent。Force/Delete/Reset/Restore、Shell Interpreter、Credential-bypass Surface、Workspace Escape 与 Protected Resource 继续阻断。
- Reconciliation 可以持久化编排并跨模型继续执行，但实际源码修改仍走 wcode 的受限、原子、SHA-256 Guarded Edit Tool，不存在绕过安全边界的隐藏自动 Patch。
- 删除是单独的破坏性授权路径：第一次 `delete_path` 会创建精确的本地 Authorization Request，操作者在 TUI 或受保护 WebUI 中批准或拒绝；只有参数和目标完全匹配的重试才能消耗这一次性 Grant。

## 8. 直接 Dogfood wcode

当前 wcode 仓库本身已经包含：

```text
.wcode/project.yaml
.wcode/design/*.yaml
```

所以安装/重启当前版本以后，你可以直接让 Agent：

> 针对要修改的 wcode 行为先调用 `agent_context`，按 Readiness / Next Actions 通过受保护 Workspace Tool 编辑，执行 `review_changes` 与 `verify_project`；只有还需要更深 Convergence 分析时，再进入 Drift / Risk / Reconciliation / Evidence Tool。

这就是现在已经能跑通的 Software Intelligence Runtime Dogfood 路径。
