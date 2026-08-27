---
layout: docs
title: Agentic Engineering
description: wcode 的模型中立 Agent 执行与证据模型
lang: zh-CN
alternate: /docs/agentic-engineering/
permalink: /zh/docs/agentic-engineering/
---

# Agentic Engineering

wcode 把现代 Agent 编程当作执行架构问题，而不是“让模型拥有更多权限”。

## 核心模型

```text
Context Policy
  ↓
按需 Skill / Software Context
  ↓
独立 Worker
  ↓
确定性 Gate
  ↓
Revision-exact Evidence
  ↓
Convergence
```

## 1. 常驻指令只保留地图

仓库级常驻说明应当短，只告诉 Agent 从哪里发现 Design State、Product Scope、验证与约束。任务细节通过 `software_context`、`traceability_status`、`scope_status`、`language_quality_status` 和符号工具按需加载。

## 2. Skill 负责渐进式披露

Skill 可以指导 Agent 的工作顺序，但不能成为安全策略本身。必须执行的规则要放在 Workspace、Authorization、Verification 和 Evidence 机制里。

## 3. 独立工作可以并行

主机支持时，可以把互不依赖的研究、实现或 Review 分配给不同 Worker/Worktree。共享写入仍要经过 wcode 的 Scheduler 和 SHA Guard。

模型共识不是证明：多个 Reviewer 都说“通过”也不能清除确定性失败。

## 4. Verification 是真正的批准层

Risk 决定需要哪些确定性检查、独立 Reviewer、Property/Mutation/Fuzz/Runtime Stage 和 HumanApproval。

每个 Producer 的 Evidence 保留 Revision、Producer、Verdict 与 Provenance；过期 Revision 不算当前证明。

## 5. 目标是持续收敛

Agent 的任务不是完成一次 Chat，而是让 Actual State 持续向 Desired State 收敛，并留下可以再次验证的 Evidence。
