---
layout: docs
title: 文档
nav_title: 概览
description: wcode 中文文档入口
lang: zh-CN
alternate: /docs/
permalink: /zh/docs/
---

# 让任何 Coding Agent 先看懂仓库，再动代码

**wcode 给 Coding Agent 补上通常缺失的四样东西：任务就绪的仓库上下文、真实的跨文件关系、受控的操作边界，以及与当前 Revision 绑定的验证证据。** 继续用你喜欢的 Agent；wcode 让它先理解、少改动、再证明，而不是每次从 grep、文件堆和聊天记录重新猜项目。

## 5 分钟理解 wcode

可以把 wcode 理解为包在 Coding Model 外面的五层基础设施：

1. **理解（Understand）** — `agent_context` 把任务感知检索、Design State、代码关系、测试、已验证历史和精确编辑目标合在一个有界 Context Pack 里。
2. **修改（Change）** — Workspace Policy 约束根目录、SHA、写入范围和权限，不让一次编码任务自然膨胀成整台机器的控制权。
3. **证明（Prove）** — 聚焦 quick 检查负责快速反馈；确定性的 full verification 仍是最终宽覆盖门禁。
4. **学习（Learn）** — 只有稳定且验证通过的修改才能进入本地 Experience Graph；失败、截断或不完整工作不会被学成“经验”。
5. **观测（Observe）** — Engineering Observatory 是项目数字孪生：不用打开 IDE，也能看到工程架构蓝图、实时工程流、Vibe Coding 变更链、设计偏离、来源、风险与当前版本证据。

检索也不是一套排名打天下：明确的 `trace→code`、`code→test`、`edit→ripple` 任务使用不同的有界启发式 Prior；多个意图同时出现时主动回退到 balanced context。精确目标和更新鲜、更强的 Semantic / Deterministic / Runtime Evidence 始终高于这些启发式。

## 从这里开始

| 你要做什么 | 文档 |
| --- | --- |
| 安装、启动并连接第一个仓库 | [快速开始](getting-started/) |
| 理解仓库、架构与工程状态 | [仓库理解与工程状态](software-intelligence/) |
| 接入本地编程智能体或云端连接器 | [智能体与 MCP 集成](code-agent-integrations/) |
| 理解工作区、命令和 OAuth 安全边界 | [安全模型](security/) |

## 核心概念

- [产品范围](product-scopes/) — wcode 的产品能力与源码责任边界。
- [智能体工程](agentic-engineering/) — 短指令、按需上下文、并行执行与确定性验证的组合方式。
- [语言质量模型](language-quality/) — 语法、语义、格式、Lint、类型、测试、安全与高级验证能力矩阵。
- [可维护性审查](maintainability-review/) — 结构增长信号、独立审查者与证据规则。

## 参考、运维与开发

- [CLI 与 MCP 参考手册](reference/) — 命令、操作入口、传输方式与工具族的统一参考。
- [开发说明](development/) — 模块边界、运行时不变量、发布门禁与维护约束。
- [论文驱动的改进](research-upgrades/) — 纳入 0.6.2 发布准备的诊断上下文与依赖预览、论文原文和评估边界。
- [前沿工程](frontier-engineering/) — 选择性上下文执行、完整验证计划、新论文与可测的能力目标。
- [v0.7.2 发布候选](releases/v0.7.2/) — 更清晰的 Observatory／TUI／Setup 状态、模型高效工具发现、Agent Context 检索遥测与更精确的 Verification Mesh Target。
- [v0.7.1 发布说明](releases/v0.7.1/) — 稳定隧道抗抖动、项目实时切换、Semantic Auto 加固、减少开发授权摩擦并强化真实并行吞吐。
- [v0.7.0 发布说明](releases/v0.7.0/) — 重构 Engineering Observatory、加快项目状态加载、强化架构/证据工作流，并收紧发布契约。
- [v0.6.2 发布说明](releases/v0.6.2/) — 资源感知并行、更简配置、更清楚的观测台与版本绑定的有效证据。
- [v0.6.1 发布说明](releases/v0.6.1/) — 完成驱动的并行调度、更少的重复上下文调用和任务优先 TUI。
- [发布版本](releases/) — 最新版本与按系列归档的完整历史。历史版本不再平铺到全局侧边栏，版本再多也不会让主导航膨胀。

## 推荐工作流

```text
agent_context(goal, scopes=...)
  ↓
按 readiness 执行；只有需要时再加载更深 Context
  ↓
只有跨文件关系任务才使用 semantic_navigation
  ↓
实现 / 编辑
  ↓
review_changes
  ↓
verify_project
  ↓
只有需要时再进入 drift / risk / evidence / reconciliation
```

文档中的命令、工具名、协议名和字段名保留其原始技术标识；说明文字本身按页面语言保持一致，不再在同一段正文中来回切换语言。
