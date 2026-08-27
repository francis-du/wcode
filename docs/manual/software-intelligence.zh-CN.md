---
layout: docs
title: Software Intelligence 中文指南
description: wcode Software Intelligence Runtime 已实现能力与使用流程
lang: zh-CN
alternate: /docs/software-intelligence/
permalink: /zh/docs/software-intelligence/
---

# wcode Software Intelligence Runtime 中文指南

本文档说明 **当前已经实现** 的 Software Intelligence 能力，以及现在应该怎么用。

> **当前状态：Software Intelligence 已经是完整产品面。** 当前同时提供 MCP、本地 CLI、实时 TUI，以及带本机高熵 UI Token 的、以 Requirement/功能点为中心的 **Project Observatory**。WebUI 不再把通用 Software Graph 球图当主视图，而是按 Requirement 展开功能意图、组件设计、当前代码实现、设计依赖与代码依赖一致性、验收验证、代码统计、Git 改动映射和架构 Revision 历史。Software Graph 继续作为底层 Intelligence 能力/API。MCP 现在由同一套协议核心同时提供 Streamable HTTP + OAuth 和本地 `mcp-stdio`，并暴露 Tools、Prompts、Resources 以及按请求 Opt-in 的 MCP 2026 Durable Tasks。`agent-plugin` 可以导出一份不含 Hook/脚本/凭据的 Agent Skill / Agent Plugins 小包，供主流 Coding Agent 复用。Design/Semantic、Graph Provider Revision/History、Verification、Evidence、Reconciliation Plan 与执行状态仍按 Workspace 持久化。22 种语法索引语言继续共享第一方 LSP Semantic Provider Registry；只有真实、可用且 fresh 的 Provider Fact 才是 semantic precision，否则诚实回退到 syntax。Property / Mutation / Fuzz / Runtime-Canary 同样走统一跨语言 Executor Registry。

## 现在怎么用

启动方式没有变化：

```bash
wcode --workspace "$PWD"
```

不连接模型也可以直接查看本地 Runtime 状态：

```bash
wcode --workspace "$PWD" intelligence
wcode --workspace "$PWD" intelligence --check --json
wcode --workspace "$PWD" verification
wcode --workspace "$PWD" verification --plan-id VP-...
```

`intelligence --check` 会把只读状态面变成 fail-closed 的 CI / Release Gate：Design State 未初始化或无效、Requirement→Component / Design→Implementation / Acceptance→Verification 任一维度低于 100%、以及 Required Convention 出现 Error 时都会返回非零退出码。只有当 Design State 显式声明 `CONSTRAINT-PRODUCT-SCOPE-CANONICAL` 时，Product Scope 扫描截断或存在未映射受支持源码才会成为硬门槛；普通第三方仓库仍可查看 `scope_status`，但不会被强制套用 wcode 自身的 12 个 Product Scope 目录模型。JSON 会同时带出 Runtime 使用的 `scope_status` 与 `conventions`；Convention Warning 仍保持建议性质，不会阻断发布。

Language Server 和 Stage Executor 都可能加载/执行仓库控制的配置或代码，因此必须由操作者显式授权。正常交互式运行时，第一次尚未授权的精确操作会 fail closed 并生成本地 Authorization Request；操作者可以在 TUI 或受保护 WebUI 批准后重试，同一操作 Fingerprint 会获得当前 Session Grant。`--allow-risky-exec` 是更宽的进程级预授权方式：

```bash
wcode --workspace "$PWD" --allow-risky-exec intelligence --refresh-semantic
wcode --workspace "$PWD" --allow-risky-exec verification --plan-id VP-... --execute-stages
```

运行 TUI 时按 `I` 打开 Software Intelligence Overlay，按 `W` 打开受保护的 Project Observatory。待授权请求会直接出现在 TUI：用 ↑/↓ 选择一条，`Y` 只批准当前选中请求，`N` 只拒绝当前选中请求；WebUI 的 Access Panel 也能处理同一批 Pending Request，并管理运行时项目与命令授权。页面顶层按 wcode 自己的控制模型组织：**Desired State → Actual State → Change → Proof → Convergence**。Proof 只统计与当前 code+design Revision 精确一致的 Evidence，不会把历史失败或历史 Pass 混成“当前已验证”。每个 Requirement 明确标成 `stable`、`changing`、`needs_convergence` 或 `incomplete`；选中一个功能后，可以看到 Requirement → Component → 当前实现 → Acceptance/Verification、声明依赖与代码实时依赖的对照、Constraint/ADR、Convergence Blocker 和关联 Git 变更。下方继续显示源码统计、语言/Product Scope 分布、Risk 与 Graph Revision 时间线。Cloud/Web Connector 继续连接 `/mcp`；本地 Coding Agent 可以直接启动 `wcode --workspace <repo> mcp-stdio`。需要 Skill/Plugin 时用 `wcode --workspace <repo> agent-plugin` 导出。Grok、Claude、Cursor、Gemini、Copilot、Cline、Roo、OpenCode、Windsurf 的具体安装与诊断命令统一维护在 [Code Agent Integrations](../code-agent-integrations/)。

推荐完整链路：

```text
Design State
    ↓
design_status
    ↓
software_context / traceability_status
    ↓
实现或修改代码
    ↓
review_changes
    ↓
drift_status / impact_analysis / risk_status
    ↓
reconciliation_plan / verification_plan
    ↓
verify_project + 独立 Reviewer Job
    ↓
evidence_status
```

## 1. 安装 / 重启当前版本

在 wcode 仓库里安装当前代码：

```bash
cargo install --path .
```

如果 wcode 已经在运行，需要让正在使用的 MCP Server 加载新的 Tool Schema：

```bash
wcode restart
```

某些 MCP 客户端会缓存 `tools/list`，因此重启 wcode 后可能还需要在客户端里重新连接/刷新一次 MCP。

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

做一个比较大的修改时，建议 Agent 按下面顺序工作：

```text
1. workspace_info
2. scope_status
3. design_status
4. project_context
5. 涉及源码/质量门时调用 language_quality_status
6. 选择相关 Product Scope
7. software_context（需要时传 scopes）
8. 用 symbol/read/edit 工具实现
9. review_changes
10. drift_status
11. impact_analysis
12. risk_status
13. 对已声明的 check-only provider 运行 language_quality_run
14. 有缺口时 reconciliation_plan
15. verify_project + 必需的高级验证 Stage
16. evidence_status
```

你可以直接把下面这段发给连接到 wcode 的 Agent：

> 改代码前先调用 `workspace_info`、`scope_status`、`design_status`、`project_context`；涉及源码或质量门时再看 `language_quality_status`。先处理 `scope_status.unmapped_files` 和语言质量 gap，再选择 Product Scope 并进入 `software_context`。优先复用仓库已经声明的原生 formatter/linter/type/test/security 工具，不要为了统一风格强塞新工具。改完以后调用 `review_changes`、`drift_status`、`impact_analysis`、`risk_status`；只有 provider 被报告为 declared / available / check-only 时才运行 `language_quality_run`。有缺口就生成 `reconciliation_plan`，再执行 `verify_project` 与必需高级 Stage，最后查看 `evidence_status`。多个模型/子 Agent 一致也不能替代确定性 Proof。

`project_context` 已经会附带一份有界 Convention Report；`convention_status` 可以单独读取完整的有界结果，包括检测到的语言、语言约定、文件命名问题、Architecture Domain 分类、Product Scope 映射/缺口、未归类的根级源码文件、过大的源码模块、Rust Domain 扁平增长、统计数量和截断状态。

### Product Scope

wcode 现在有一份统一的产品能力 Scope Registry：`runtime`、`integrations`、`workspace`、`design`、`graph`、`semantics`、`traceability`、`risk`、`verification`、`evidence`、`reconciliation`、`experience`。`workspace_info` 和 `project_context` 会返回这份 Registry；`scope_status` 会把 Registry 应用到当前 Workspace，返回每个 Scope 的源码数量以及有界的未映射源码路径；`tools/list` 会给每个 Tool 的 `_meta.dev.wcode/productScopes` 附上所属 Scope；支持 MCP Resource 的客户端还可以读取 `wcode://runtime/product-scopes`。同一份实时 Scope Audit 也会进入本地 Intelligence Operator View。

Product Scope 描述的是 wcode 自身能力边界，不是模型厂商，也不会取代业务领域的自由 Semantic Scope。已知别名会被规范成 canonical scope；未知字符串仍作为自由业务 Scope 保留。`software_context.scopes` 对已识别 Product Scope 会真正收窄源码/符号导航；`semantic_query.scopes` 会过滤有 Scope 的 Semantic Fact，而没有 Scope 的 Fact 继续作为全局语义参与查询。详细映射见 [product-scopes.md](../product-scopes/)。

## 4. software_context

当你的任务是从“业务行为 / Requirement / 子系统”出发，而不是已经知道具体文件名时，优先使用它。

示例：

```json
{
  "query": "workspace command security",
  "intent": "modify",
  "budget": 12000,
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

## 5. traceability_status

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

### 22 语言 Semantic Provider

`semantic_provider_status` 会扫描 Workspace，并对当前语法索引已经支持的全部 22 种语言报告真实 Provider 状态：Bash、C、C++、C#、CSS、Dart、Elixir、Go、HTML、Java、JavaScript、Lua、OCaml/Interface、PHP、Python、R、Ruby、Rust、Swift、TypeScript、TSX。

Registry 会自动探测常见 LSP，例如 `clangd`、`csharp-ls`/OmniSharp、`gopls`、`jdtls`、`typescript-language-server`、`pyright`/`pylsp`、`rust-analyzer`、`sourcekit-lsp`、`ocamllsp`、`lua-language-server`、`ruby-lsp`，以及 PHP / Elixir / Dart / R / HTML / CSS / Bash Language Server。**语言支持**和**本机工具已安装**是两回事：没有找到可执行文件时会明确显示 unavailable / syntax fallback。

`semantic_provider_refresh` 使用有界 stdio LSP 请求真实 hierarchical Document Symbol；Server 声明支持 Call Hierarchy / Implementation 时再导入对应关系。第一方 LSP Node 会携带 `source_sha256`，因此 Provider Status 会明确给出 `fresh / stale`；源码变化后 stale semantic revision 会自动退出 Software Graph、Impact、Reconciliation 和 `software_context.graph_context`，直到刷新成功。刷新还会根据源码 Hash、Provider 二进制元数据和 Symbol 上限计算 Revision Cache Key；输入未变化时直接返回 `cached=true`，不会重复启动 Language Server。Server 没返回语义 Symbol 时不会制造一个假的 semantic revision。

Language Server 可能读取项目配置、插件、生成文件或构建元数据，所以刷新需要显式信任。没有开启进程级 `--allow-risky-exec` 时，尚未授权的 Refresh 会先生成本地 `RiskyExecution` Authorization Request；在 TUI 或受保护 WebUI 批准后重试同一操作即可。需要整进程预授权时再使用：

```bash
wcode --workspace "$PWD" --allow-risky-exec intelligence --refresh-semantic
```

既没有批准对应操作、也没有开启进程级预授权，或者本机没有对应 LSP 时，Tree-sitter 仍提供 `precision=syntax` 的基础图。SCIP / Compiler / Runtime 等其他 Provider 仍可以继续使用 `graph_provider_import`，两条路径共享同一个带 Provenance 的 Software Graph。

### Language Quality Matrix

wcode 不再用一个 `supported=true` 描述语言能力。`language_quality_status` 复用同一套 22 语言 canonical surface，分别展示 Syntax、Semantic、Format、Lint、Type Check、Static Analysis、Test、Security、Property、Mutation、Fuzz、Runtime-Canary。仓库 Manifest、依赖、配置文件、package script 和语言原生项目结构决定“这个项目声明了什么”；已知生态工具可以作为候选出现，但没有 Repository declaration 就不会被当成项目质量策略。缺少可执行程序或质量维度会明确显示为 gap。

`language_quality_run` 只执行 Matrix 中 detected + declared + available + check-only 的 provider，并继续经过正常的 Repository-aware Authorization。这个通道没有 formatter/fixer 写模式，不会借“检查”偷偷修改源码。真实 Command Result 会转成 Verification Report 并记录为当前 code+design Revision Evidence，所以旧版本的 Pass 不会冒充当前版本已经验证。

目前 Registry 能识别 Rust / Go / Python / JS/TS/CSS/HTML / C/C++ / .NET / Java / Dart / Elixir / Bash / Lua / OCaml / PHP / R / Ruby / Swift 的主流原生或仓库声明质量链，但这不等于本机全部安装。实际状态以 `language_quality_status` 为准。详见 [language-quality.md](../language-quality/)。

### Graph History / Query / Diff

`software_graph` 会持久化并去重真正有变化的 Graph Snapshot。`graph_history` 查看历史，`graph_query` 查询某个 Snapshot / 邻域，`graph_diff` 可以显式比较两个 Revision，也可以默认比较最近两个 meaningful Snapshot。Node 用稳定 `node.id` 对齐；Edge 用 `from + to + kind + provider + precision` 对齐，Revision / Attributes 改动归为 `changed`，不会噪声式地报成“整条边删除后重建”。同一稳定 Edge Identity 下出现多条 Revision 时按 multiset 对齐，避免未来更复杂 SCIP / Runtime Provider 丢关系。Project Observatory 用这些 Snapshot 展示架构 Revision 时间线和最新 Node/Edge `+ / - / ~`；每个功能的 Actual Architecture 则在刷新时基于当前仓库重新生成。

## 6. Change Intelligence

### drift_status

检测两类问题：

- **Implementation Drift**：Design 已经声明，但实现/验证链不完整，或者 Desired State 改了而 Actual State 没有对应变化。
- **Design Drift**：Design 映射过的实现发生变化，但当前变更中没有对应 Design State 变化。

当前 Drift 是保守式 heuristic，不是 compiler-grade program equivalence。

### impact_analysis

结合当前 Git Working Tree 与 Design State，返回：

- changed paths
- impacted components
- impacted requirements
- impacted acceptance criteria
- impacted symbols
- bounded transitive callers
- graph provider / precision / truncation
- public API signal
- security boundary signal
- risk level

当前会沿 Composite Software Graph 的 `Calls` / `RuntimeCalls` 反向传播到调用者：如果已经刷新真实 LSP / Runtime Provider，就优先消费这些带 semantic/runtime provenance 的边；没有对应 Provider 时继续使用 Tree-sitter syntax call edge。Impact 仍保持保守边界，不会把缺失的 type / overload / dynamic-dispatch 事实假装成已经解析。

### risk_status

把下面几类信号合并为结构化 Risk：

```text
Git change review
+ traceability gap
+ drift
+ design-declared risk
```

并生成对应的 Risk-Adaptive Verification Profile。

`review_changes` 现在还会产生三类确定性的 Maintainability Signal：`maintainability-file-crossed-1k` 表示本次修改把一个未删除源码文件从 1,000 行以下推到 1,000 行以上，严重级别为 high；`maintainability-concentrated-growth` 表示单个源码文件至少净增 400 行；`maintainability-cross-scope-churn` 表示源码变更横跨至少 3 个 canonical Product Scope，且总 churn 至少 1,000 行。它们只是可测量的结构信号，不会假装证明设计质量，但会进入正常 Risk Engine。Convention 中 2,000 行的 oversized-module 规则仍然是另一条仓库级信号。详见 [maintainability-review.md](../maintainability-review/)。

### reconciliation_plan

把当前状态组织成：

```text
Design Change
+ Drift
+ Impact
+ Risk
+ Reconciliation Task
+ Change IR Intent
+ Verification Plan
```

Plan 生成后会进入 **持久化 Reconciliation Execution 状态机**。通过 `reconciliation_execution_status` 查看依赖与进度，执行者使用 `reconciliation_claim` 领取当前可运行任务，通过 `reconciliation_submit` 写入成功/失败与 Evidence，失败任务可以用 `reconciliation_retry` 显式重排队。Verification / Human Approval Task 会根据真实 Verification/HumanApproval Evidence 自动推进。

源码修改本身仍然走 wcode 原有的 SHA-256 前置条件、原子写入和 Workspace 安全工具；Reconciliation 不会绕过这些边界偷偷执行无限制 Patch。Plan 和 Execution 都持久化到用户级 Workspace State，重启或换模型后仍可继续。

## 7. Verification Mesh

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

Verification Plan 和 Reviewer Job 会按 Workspace 持久化，因此 wcode 重启或换模型后可以继续领取/提交同一个 Plan。`verification_executor_status` 会返回跨语言 Executor Registry，并区分“已经注册”和“本机真实可执行”；`verification_execute_stages` 会运行当前 Stage 下**所有适用且真实可用**的 Executor，只跳过“这个 Producer 自己已经有最新 Pass Evidence”的 Runner，并把每次真实 Command Result 分别写成 Stage Evidence。`verification_status` 会保留每个 Producer 的最新结果，并按 `Fail > Disagree > Inconclusive > Pass` fail-closed 聚合，所以另一个晚到的 Pass 不能盖掉真实 Runner 的 Fail。CI 或其他外部系统仍然可以用 `verification_stage_submit` 提交真实 Verdict、Producer、Summary 与 Artifact Digest；Workspace Code Revision 变化后旧 Plan 仍会被 stale-revision blocker 阻止。

wcode 会自动发现一批常见生态，例如 Rust proptest/quickcheck/cargo-fuzz/cargo-mutants、Go Property/Fuzz、Python Hypothesis/mutmut、JS/TS fast-check/Stryker、Java jqwik/PIT、C# FsCheck/.NET Stryker、SwiftCheck/Muter、Elixir StreamData、Dart Glados、Ruby Rantly、PHP Eris/Infection、OCaml QCheck、R quickcheck。对于其他框架和所有 22 种语言，都可以通过同一份 `.wcode/executors.yaml` 接入：

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
wcode --workspace "$PWD" --allow-risky-exec verification --plan-id VP-... --execute-stages
```

缺少可执行程序时只会报告 unavailable / missing executor，不会生成假的 Pass Evidence。

### MCP 2026 长任务 Tasks

在 MCP `2026-07-28` 下，wcode 已支持官方 `io.modelcontextprotocol/tasks` Extension，而且严格按**每个请求显式 opt-in**：客户端必须在该请求的 `_meta.io.modelcontextprotocol/clientCapabilities.extensions` 中声明 Tasks。当前只把两个明确的长耗时工具 Task 化：`semantic_provider_refresh` 和 `verification_execute_stages`；没有声明 Tasks 的客户端继续走原来的同步 `tools/call`，兼容行为不变。

Task Handle 返回前状态已经持久化；Owner 使用当前 OAuth `client_id` 的 SHA-256 Fingerprint，不保存原始 Bearer Token。`tasks/get` 轮询并在完成时返回原始 Tool Result；`tasks/update` 当前是 ack-only，因为这两个任务不会发 Input Request；`tasks/cancel` 先持久化 `cancelled`，再 Abort Worker，避免迟到的 Completed 覆盖取消。Task Store 有 Workspace 级容量上限，只会在创建新 Task 前回收 Terminal Task，Active Task 不会为了腾空间被删。如果 Runtime 在 Task 仍是 `working` 时被替换，下一次读取会把它标成 Failed，而不是假装 Worker 跨进程存活。

## 8. Multi-model / Independent Reviewer 怎么用

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

## 9. Evidence

现在 `verify_project` 完成后，会把确定性检查结果写入 Runtime Evidence。

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

因此 `wcode restart`、断开 MCP、切换 Model Executor 后，Software Intelligence 的持久状态仍可恢复。正在执行的 MCP `working` Task 不会伪装成跨进程继续运行：Runtime 被替换后下一次读取会标为 Failed。`Risk` 会基于最新 Design / Git / Code 重新计算；第一方 LSP Provider 会额外检查 Source Hash Freshness，stale Revision 不会进入新构建的 `software_graph`。

## 10. 当前高阶工具

### Desired State / Semantic / Graph

```text
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
wcode --workspace <PATH> intelligence
wcode --workspace <PATH> --allow-risky-exec intelligence --refresh-semantic
wcode --workspace <PATH> verification
wcode --workspace <PATH> --allow-risky-exec verification --plan-id VP-... --execute-stages
TUI: I = Intelligence, W = Project Observatory
Web: /intelligence（本机高熵 UI Token 保护 + Requirement-first Project Observatory）
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

## 11. 已实现能力与精度边界

### 已实现

- Structured Design State + `design_init`
- 统一的 12 个 Product Scope Registry：驱动源码架构、`workspace_info` / `project_context`、`software_context.scopes`、`semantic_query.scopes`、Convention Scope Mapping、MCP Tool `_meta.dev.wcode/productScopes` 和 `wcode://runtime/product-scopes` Resource，同时继续支持自由业务 Scope；`scope_status` 会审计当前 Workspace 的真实映射并返回有界的未映射源码路径
- 跨语言 Convention Policy 与仓库结构检查：`project_context` 会携带有界报告，也可用 `convention_status` 单独查看，其中包含 Product Scope Mapping Gap
- 独立 Workspace 读写任务的依赖感知调度：按路径冲突排序，并在同一文件、同一 SHA 且 Edit Range 不冲突时安全合并 `apply_edits`
- `delete_path` 的精确一次性人工授权：只能删除普通文件或空目录；文件删除要求当前 SHA-256，递归删除、Workspace Root、受保护路径、Symlink Alias、Hard-linked File 永久禁止
- 持久化 Semantic Registry；Conversation/Model 推断默认只能成为 Candidate，必须显式人工确认后才进入权威 Context
- Design + Tree-sitter + Fresh 第一方 LSP + External Provider Composite Software Graph
- 全部 22 种索引语言的统一 Semantic Provider Registry；真实 LSP Document Symbol / Call Hierarchy / Implementation 才会成为 `precision=semantic` 事实，并支持 Source Hash Freshness / Stale Exclusion / Revision Cache
- 可唯一判定的跨文件 Syntax Calls、Graph History / Query / Diff，以及外部 SCIP/Compiler/Runtime Provider Import Contract
- Requirement → Component → Code/Test Traceability
- Budget-aware / Semantic-aware Software Context，包含 provenance-bearing `graph_context` 语义/运行时邻域
- Drift、Transitive Impact、结构化 Risk，以及针对 1,000 行阈值跨越、单文件集中增长、跨 Product Scope 大规模 churn 的确定性 Maintainability Finding
- Risk-Adaptive Verification + 持久化 Blind Reviewer Job；Medium 及以上风险会加入独立 `maintainability_review`，并继续保留 Disagreement Evidence、HumanApproval Evidence、Verification History、Stale Revision Gate，以及 per-producer fail-closed Stage 聚合
- 跨语言 Property / Mutation / Fuzz / Runtime-Canary Executor Registry：常见生态自动发现 + 通用 `.wcode/executors.yaml` + 外部 Stage Evidence Adapter；自动执行会跑所有适用可用 Runner
- MCP 2026 Tasks：对 `semantic_provider_refresh` / `verification_execute_stages` 提供 per-request opt-in 的 Durable Task、Owner Scope、Get/Cancel 和同步兼容路径
- 持久化 Reconciliation Plan + dependency-aware Claim / Submit / Retry 执行状态机 + Reconciliation Evidence
- `wcode intelligence --refresh-semantic` / `wcode verification --execute-stages` CLI
- TUI Software Intelligence Overlay 与受保护 Project Observatory：Requirement Board、功能/组件设计、当前代码实现、Design ↔ Actual Dependency Alignment、Acceptance/Verification、Constraint/ADR、代码统计、Git 改动映射与架构 Revision 历史
- `read_media` 的 Capability-aware 多媒体边界：PNG/JPEG/GIF/WebP 可返回尺寸，常见音频与 MP4/WebM 可识别 Metadata；只有当前请求显式声明匹配的 `run.francis.wcode/media-content` Client Capability 时才返回 Image/Audio Content，能力未知或不支持时 Fail Closed，视频始终 Metadata-only
- 完整高阶 MCP Surface

### 明确精度 / 集成边界

- 永远可用的内置代码索引仍是 Tree-sitter `precision=syntax`；第一方 LSP Adapter 只有在本机真实 Language Server 返回结果后才把对应事实标成 `precision=semantic`，外部 SCIP / Compiler / Runtime Provider 仍走带 `provider + precision + revision` 的 Import Contract。
- 全部 22 种语言共享同一套 Semantic Provider / Verification Executor 架构，但 wcode 不捆绑每一种第三方 LSP 和测试二进制；`semantic_provider_status` / `verification_executor_status` 会展示本机真实 availability，不把缺失程序算成已安装。
- Repository-aware LSP 刷新和 Property / Mutation / Fuzz / Runtime-Canary 执行都需要显式操作者信任：可以对精确操作在 TUI 或受保护 WebUI 做 Session 授权并重试，也可以用 `--allow-risky-exec` 做进程级预授权；两者都不是 OS Sandbox。
- 模型请求的命令程序使用按 Workspace 的 `CommandAccess`：少量安全命令默认预授权，其他合法的裸可执行程序名进入 Pending 列表，只有用户显式批准后才加入该 Workspace；Shell 解释器、带路径程序名、Workspace Escape 参数和受保护资源仍然硬拒绝。
- Reconciliation 可以持久化编排并跨模型继续执行，但实际源码修改仍走 wcode 的受限、原子、SHA-256 Guarded Edit Tool，不存在绕过安全边界的隐藏自动 Patch。
- 删除是单独的破坏性授权路径：第一次 `delete_path` 会创建精确的本地 Authorization Request，操作者在 TUI 或受保护 WebUI 中批准或拒绝；只有参数和目标完全匹配的重试才能消耗这一次性 Grant。

## 12. 直接 Dogfood wcode

当前 wcode 仓库本身已经包含：

```text
.wcode/project.yaml
.wcode/design/*.yaml
```

所以安装/重启当前版本以后，你可以直接让 Agent：

> 检查 wcode 自己的 Design State 和 Traceability。针对 workspace security 调用 `software_context`，再针对当前 Working Tree 调用 `drift_status`、`impact_analysis`、`risk_status`，生成 `reconciliation_plan`，执行推荐 Verification，最后把 `evidence_status` 展示出来。

这就是现在已经能跑通的 Software Intelligence Runtime Dogfood 路径。
