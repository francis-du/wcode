---
layout: docs
title: Product Scope
description: wcode 的产品能力与源码责任边界
lang: zh-CN
alternate: /docs/product-scopes/
permalink: /zh/docs/product-scopes/
---

# wcode Product Scope

wcode 按产品行为组织 Control-plane 能力，而不是按通用 Backend Layer 划分。同一套 Scope 同时用于源码架构、Semantic Filter、Software Context Retrieval、MCP Tool Metadata、Project Context、Convention Governance 与 Agent Guidance。

## 规范 Scope

| Scope | 产品责任 | 主要源码根 |
| --- | --- | --- |
| `runtime` | Harness、调度/运行时控制、电源与进程协调 | `src/main.rs`, `src/runtime/`, `src/scopes/` |
| `integrations` | MCP、OAuth、Agent Plugin、Connector-facing Task/Resource/Prompt | `src/integrations/` |
| `workspace` | 安全文件原语、授权、Scheduler、Convention、命令策略 | `src/workspace/` |
| `design` | 结构化 Desired Software State 与校验 | `src/design/` |
| `graph` | Syntax Index、Composite Software Graph、Graph Persistence/Provider | `src/graph/` |
| `semantics` | 持久化 Semantic Registry 与第一方 LSP Adapter | `src/semantics/` |
| `traceability` | Requirement Traceability、Drift、Impact、Scoped Software Context、Project Architecture Projection | `src/intelligence/` 中除 Risk 外的 Traceability/Observatory 逻辑 |
| `risk` | Risk-adaptive Policy 与 Verification 深度 | `src/intelligence/risk.rs` |
| `verification` | Deterministic / Stage Verification 与 Blind Review Mesh | `src/verification/` |
| `evidence` | 带 Provenance 的持久化 Evidence | `src/evidence/` |
| `reconciliation` | Durable Convergence Plan 与依赖感知执行 | `src/reconciliation/` |
| `experience` | TUI / WebUI Operator Experience | `src/ui/` |

`main.rs` 负责启动组合，`src/scopes/mod.rs` 是规范 Registry。Product Scope 描述 wcode 自己；Semantic Fact 还可以额外携带 Freeform Business Scope。

## Runtime 行为

`software_context` 接受可选 `scopes`。识别出的 Product Scope Alias 会 Canonicalize，并把 Source / Symbol Navigation 收窄到对应 Source Root。Response 会返回规范 Scope，让 Agent 知道当前 Context 受哪个产品边界约束。

`semantic_query` 同样接受可选 `scopes`。有 Scope 的 Fact 必须与请求 Scope 重叠；Unscoped Fact 保持 Global。未知 Scope String 作为 Freeform Business Scope 保留，而不是被拒绝。

Convention Engine 会按 Architecture Domain 与 Product Scope 分类源码，报告 Root Rust Orphan、Unmapped Product Scope File、Language Naming Finding、Flat Domain Growth 与 Oversized Module，但不会自动改写仓库。

## MCP 与 Agent Discovery

每个 MCP Tool 都在 `_meta.dev.wcode/productScopes` 暴露 Product Scope。忽略 Custom Tool Metadata 的 Agent 仍可通过 MCP Resource `wcode://runtime/product-scopes` 获取同一模型。`scope_status` 把 Registry 应用到当前 Workspace，报告每个 Scope 的源码计数和有界 `unmapped_files`，因此 Scope 不只是 Discovery Metadata，也是 Architecture Governance 的一部分。TUI Intelligence Overlay 与受保护 `/intelligence/status` 使用同一审计结果；`/intelligence/scopes` 暴露当前 Focused Workspace 的 Scope Audit。

推荐 Agent Flow：

1. `agent_context(goal, scopes=...)` 作为日常编码主入口；
2. 只有需要 Scope Audit 时调用 `scope_status`，特别关注新增 Production Structure 前的 `unmapped_files`；
3. 只有需要完整 Desired State / Repository Guidance 时再调用 `design_status`、`project_context`、`traceability_status`；
4. 选择与行为对应的 Product Scope；
5. 需要更深 Context 时使用 `software_context(query, scopes=...)`；
6. 按需使用 `find_symbol`、`symbol_context`、Graph / Semantic / Traceability Tool；
7. 只通过 Workspace Primitive 与依赖感知 Scheduler 修改；
8. 根据 Change 运行 `review_changes`、Risk/Impact、Verification、Evidence 与 Reconciliation Gate。

## Scheduler 边界

`parallel_tools` 不是只读 Fan-out Helper。它复用 Scheduler 的 Resource Model：`reads`、`writes`、`creates`、`moves_from`、`moves_to`、`deletes`。独立工作可以 Fan-out；重叠资源按依赖排序。同文件 `apply_edits` 只有在使用同一个已观测 SHA、且编辑不重叠并能明确定位时才允许 Coalesce。

## Design State Contract

Design State 把这些 Scope 映射到真实 Component、Implementation Symbol 与 Acceptance Test。不要增加没有真实实现映射的 Future-only Component。Refactor 后 Traceability 仍必须可解析；物理移动文件时要同步更新 Design State Path。

## Scope 设计规则

- 优先按产品责任命名，不使用泛化技术层名称代替产品边界；
- 只保留一份规范 Registry，不在 MCP、Semantic、UI 或 Docs 复制 Alias Table；
- 新的一等 wcode 能力应映射到 Product Scope、Source Root、MCP Tool Metadata（若暴露）以及 Design State Component / Acceptance Chain；
- Business / Domain Semantic 保持 Freeform，不与 wcode Product Scope 混为一谈；
- Scope Filter 在适用处必须真实改变 Retrieval / Execution 行为，仅装饰性 Label 不够。
