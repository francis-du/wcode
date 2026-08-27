---
layout: docs
title: 可维护性审查
description: wcode 的结构可维护性 Review 与 Evidence 规则
lang: zh-CN
alternate: /docs/maintainability-review/
permalink: /zh/docs/maintainability-review/
---

# 可维护性审查策略

wcode 把可维护性当作批准问题，而不是最后的 Cosmetic Cleanup。这套策略吸收结构化 Code Review 中“优先删除复杂度、检查职责边界、避免分支和 Helper 膨胀”的有效思想，并把它落到 Design State、Product Scope、Risk、Verification 和 Evidence 模型中，而不是依赖某个外部 Skill 才成立。

## 两层 Review

### 确定性结构信号

`review_changes` 可以从当前 Git Change 中测量事实，而不假装理解设计意图：

- `maintainability-file-crossed-1k`：本次修改把一个源码文件从 1000 行以下推到 1000 行以上。这是强烈的拆分信号，不是全局统一的文件大小禁令。
- `maintainability-concentrated-growth`：单个源码文件净新增至少 400 行，需要检查是否把多个职责集中到同一位置，或遗漏了更简单的模型。
- `maintainability-cross-scope-churn`：源码变更跨至少 3 个规范 Product Scope 且总变更达到至少 1000 行，需要检查 Ownership、Dependency Direction，以及独立关注点是否应拆分。

这些 Finding 进入正常 Risk Engine。高严重度的 Maintainability Finding 可以像其他 Architecture Risk 一样提高 Verification 深度。

Convention Engine 与之分离。它的 Rust Production Module 2000 行阈值是仓库级 Oversized Module 信号；这里的 1000 行规则专门描述“本次 Change 穿过此前尚未超过的边界”。

## 独立 Maintainability Reviewer

Medium 及以上 Risk 的 Verification Plan 包含盲审 `maintainability` Reviewer Job，Capability 为 `maintainability_review`。不同模型 Provider 使用同一套批准 Rubric：

1. **先删除复杂度。** 优先寻找能保持行为、同时减少 Branch、Helper、Mode 或 Layer 的重构，而不是把复杂度搬家。
2. **阻止 Spaghetti 增长。** 到处分散的 Feature Check 与 Special-case Condition 往往说明缺少更明确的 Model、Policy、State Machine、Helper 或 Module Ownership。
3. **保持 Canonical Ownership。** Feature Logic 应属于正确 Product Scope / Layer，优先复用规范 Helper，不要创建第二份局部实现。
4. **让边界显式。** 质疑没有必要的 Wrapper、Cast、Optionality、Silent Fallback 和松散 Contract，它们可能隐藏真正 Invariant。
5. **质疑文件膨胀。** 从 1000 行以下跨到以上需要强结构理由，否则应拆分。
6. **简化 Orchestration。** 独立工作在流程更清晰时应 Fan-out；相关 State Transition 在部分应用会增加推理成本时应原子化。
7. **优先高置信 Finding。** 结构回归与明显遗漏的 Simplification 优先于 Naming / Formatting Nit。

## 批准标准

行为正确是必要条件，但不是充分条件。存在明显结构回归、Canonical Helper 重复、可避免的 Special-case Branching、无正当理由的 1000 行跨越，或明显能实质删除复杂度的更简单设计时，Maintainability Reviewer 不应 Pass。

Reviewer 必须提交真实 Verification Evidence。Correctness Reviewer 不能替代 Maintainability Reviewer；确定性的 Growth Signal 也不能替代模型审查。它们是不同 Evidence Type，拥有不同 Precision。

## 开发流程

编辑前用 Product Scope 和 `agent_context` / `software_context` 找到 Canonical Ownership。实现中优先复用已有 Helper 与直接模型，并在相关状态需要一致时保持原子更新。编辑后：

```text
review_changes
  → drift_status / impact_analysis / risk_status
  → verification_plan
  → required independent maintainability review
  → deterministic / stage verification
  → evidence_status
```

如果 Maintainability Review 找到了明显更简单的实现，应先修结构，再围绕一个已经知道更难维护的设计继续堆 Verification Evidence。
