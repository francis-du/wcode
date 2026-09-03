---
layout: docs
title: Agentic Engineering 模型
description: wcode 的模型中立 Agent 执行与证据架构
lang: zh-CN
alternate: /docs/agentic-engineering/
permalink: /zh/docs/agentic-engineering/
---

# Agentic Engineering 模型

wcode 把现代 Agent 编程当作执行架构问题，而不是“给模型更多权限”。这套模型刻意保持厂商中立：不同 Host 可以拥有不同的 Rules、Skills、Subagent、Worktree、Hook、Plugin 或 MCP 能力，而 wcode 保持仓库状态、授权、Verification 与 Evidence 语义稳定。

## wcode 模型

```text
紧凑 Context Policy
  ↓
agent_context / 按需 Skill
  ↓
隔离 Worker
  ↓
有界 Workspace 修改
  ↓
确定性 Gate
  ↓
Revision-exact Evidence
  ↓
Convergence
```

### 1. Context Policy：先给地图

常驻指令应保持短小，只说明 Workspace / 安全边界、Desired State、Product Scope、推荐编程路径和权威验证工具。详细架构、Requirement 历史、源码正文、语言工具和 Verification 状态只在任务需要时加载。

正常编程时，`agent_context` 是主入口。它的有界自适应 Context Pack 可以包含相关 Design State、按 Scope 收窄的 Repo Map、少量 Hot Source、编辑 SHA 前置条件、相关测试、Readiness、确定性的 Next Actions 与显式 Parallelism Strategy。MCP 调用保持最小：默认 Workspace，以及服务端默认的 Path / Limit / Timeout / Budget 都不要传，除非任务确实需要覆盖。只有 Pack 明确需要更多上下文时，再使用 `symbol_context`、`software_context`、`scope_status`、`language_quality_status` 或更深的 Graph / Traceability 工具。

### 2. Skill：渐进披露，不是隐藏执行

可移植的 wcode Agent Skill 只有指令，不包含 Hook、凭据、脚本，也不会隐式扩大 Workspace。Skill 可以说明什么时候调用哪些能力，但 Prompt 里写了“永远执行 X”并不等于真正强制执行。必须执行的规则应落在 Workspace Policy、Authorization、Harness Verification 或 Evidence Readiness 中。

### 3. 隔离 Worker：并行独立推理

Subagent、Worktree、类似隔离上下文以及多个顶层 MCP Call 都适合做独立仓库调研、替代实现分析、测试生成、安全审查和可维护性审查。执行前先按依赖拆 Lane；能降低延迟或上下文互相干扰的独立 Lane 应并发，只有真实 Data / Path Dependency 才串行。输入已经明确时优先用 `read_files`、`search_many`、`apply_file_edits`、`create_files` 等 Bulk Primitive。嵌套 `parallel_tools` 只用于紧凑 Fan-out，避免 Host 的 Tool Call 展示重新变成长串递归 JSON。

Worker 不应绕过 wcode 的路径资源 Scheduler 与 SHA 前置条件并发修改共享或相互依赖的状态。相关状态在部分应用会让系统更难推理时应保持原子更新。多个模型意见一致仍然只是模型证据，不是确定性证明。

### 4. Deterministic Gate：强制策略必须在 Prompt 外

必须执行的控制放在确定性机制中：

- Workspace Root / Path 保护；
- 精确的 Destructive / Risky Operation 授权；
- No-shell Command Policy；
- SHA 编辑前置条件；
- Product Scope 与 Convention 检查；
- `review_changes` 可维护性信号；
- Verification Plan Readiness 与 Revision Matching；
- Stage Evidence 与 HumanApproval。

这样 wcode 才能保持模型中立：更强模型可以提升执行质量，但不会成为策略事实来源。

### 5. Evidence：证明必须绑定精确 Revision

Compiler Check、Static Analysis、Test、Language Quality Provider、独立 Review、高级 Verification Stage 和人工批准都会产生带 Provenance 的 Evidence。Evidence 只对它证明的 code+design Revision 有意义；历史 Green 结果仍然只是历史，而不是当前证明。

### 6. Convergence：目标不是“代码生成完成”

控制目标是：

```text
Desired State → Actual State → Change → Proof → Convergence
```

Agent 可以提出或实现 Change。wcode 判断 Implementation 是否可追踪、Observed Architecture 是否与 Design State 对齐、当前 Revision 有哪些 Proof，以及还剩哪些未收敛项。

## Host 集成

Cursor、Claude Code、Codex、Copilot、Gemini、Grok 等 Host 可以拥有不同的 Rules、Skills、Subagent、Worktree、Hook、Plugin 与 MCP 能力。可移植契约是：

- 使用 Host 自己的 Sandbox / Worktree / Isolation 能力；
- 用 wcode Skill 作为简短 Control-plane 地图；
- 通过 MCP 做结构化仓库操作与 Intelligence；
- 保留 Host / Operator 的批准边界；
- 当 wcode 是 Editing Harness 时，通过受保护 Workspace 工具修改源码；
- 对必须跨模型变化或 Runtime 重启存活的结论，使用 Verification / Evidence。

Host 具体接入方式见 [Agent 与 MCP 集成](../code-agent-integrations/)。

## Language-aware Agent 工作流

进行较大源码修改前：

1. 调用 `agent_context(goal, scopes=...)`，通常不手工传 Budget；
2. 按 `readiness`、`next_actions` 与 `readiness.parallelism` 执行；Host 支持时把独立 Lane 拆成多个顶层调用并发；
3. 省略默认 / 可推导 MCP 参数，`find_symbol` / `search_code` 继续作为低成本定位路径；只有 Readiness 判断任务需要跨文件 Reference、Caller、Implementation、Rename Impact 等语义关系时才调用 `semantic_navigation`；
4. 只有需要更多源码时才调用 `symbol_context`；
5. 只有任务确实需要时再调用 `language_quality_status`、`scope_status`、`design_status`、`traceability_status` 或更深 Graph Context。

修改后：

1. `review_changes`；
2. 按需执行 Drift / Impact / Risk；
3. 按需运行仓库声明且 Check-only 的 `language_quality_run`；
4. `verify_project`；
5. 必需的 Property / Mutation / Fuzz / Runtime Stage 与独立 Review；
6. 检查当前 Revision 的 `evidence_status` 与 Convergence 状态。

这是一条 Guidance 路径；真正 Enforcement Boundary 仍然是 Workspace Policy、Authorization、Harness Verification 与 Verification Mesh。

## 研究影响

wcode 采用的是可长期复用的公开模式，而不是某一家 Host 的配置语法：

- 紧凑 Repo Map 与 Task-specific Context；
- Task-adaptive Code Navigation：普通定位走低成本 Syntax/Search，跨文件完整关系走 Warm Semantic Session；
- Agent Skills 与 Progressive Disclosure；
- 隔离 Subagent / Worktree 并行独立工作；
- Deterministic Hook、Policy 与 Verification Gate；
- MCP 等结构化 Tool Protocol；
- 明确的 Review / Test / Evidence 闭环。

Host 产品会持续变化，wcode 应周期性复核 Primary Documentation；内部不变量保持稳定：模型指引可以替换，确定性的 Repository State、Policy 与 Evidence 不可以。
