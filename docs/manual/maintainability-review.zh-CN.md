---
layout: docs
title: 可维护性审查
description: wcode 的结构可维护性 Review 与 Evidence 规则
lang: zh-CN
alternate: /docs/maintainability-review/
permalink: /zh/docs/maintainability-review/
---

# 可维护性审查

wcode 把可维护性当作发布批准问题，而不是“最后顺手格式化一下”。

## 两层信号

### 确定性结构信号

`review_changes` 可以从当前 Git 变更中测量：

- `maintainability-file-crossed-1k`：文件从 1000 行以下跨到 1000 行以上；
- `maintainability-concentrated-growth`：单文件净增长至少 400 行；
- `maintainability-cross-scope-churn`：至少跨 3 个 Product Scope 且总改动达到较大规模。

这些只是结构风险信号，不伪装成“自动理解了设计质量”。它们会进入正常 Risk Engine。

Convention Engine 的 2000 行生产模块阈值是仓库级结构告警，与本次变更跨越 1000 行的信号职责不同。

### 独立 Reviewer

Medium 及以上 Risk 的 Verification Plan 会包含独立 `maintainability` Job，需要 `maintainability_review` 能力。

Reviewer 重点检查：

- 是否可以用更少分支、Helper 或层级表达同一行为；
- 是否把特例散落到多个边界；
- 是否重复了已有 canonical helper；
- 是否引入没有必要的 Wrapper、Cast、Optionality 或状态复制；
- 文件继续增长是否说明责任边界需要拆分；
- 本可独立的工作是否被串行化；
- 相关状态更新是否应该原子化。

Correctness Pass 不能替代 Maintainability Pass。

## Evidence

结构审查结论必须和其他 Reviewer 一样保留 Revision、Producer、Verdict 与说明。当前源码变化后，旧 Revision 的 Pass 不再算当前证明。
