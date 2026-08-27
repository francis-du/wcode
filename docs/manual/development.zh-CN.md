---
layout: docs
title: 开发说明
description: wcode 的模块边界、运行时不变量与发布约束
lang: zh-CN
alternate: /docs/development/
permalink: /zh/docs/development/
---

# 开发说明

这页面向 wcode 自身的维护者。用户上手请从 [快速开始](../getting-started/) 开始。

## 模块边界

- `src/main.rs`：CLI 与启动组合。
- `src/runtime/`：Harness、项目上下文、Review、Verification 编排与运行时服务。
- `src/integrations/`：MCP、OAuth、Tasks、Prompts/Resources、Agent Plugin。
- `src/workspace/`：Workspace 隔离、文件原语、命令策略、人工授权、Scheduler、Convention。
- `src/design/`：结构化 Desired Software State。
- `src/graph/`：Tree-sitter Index、Software Graph、Provider/History。
- `src/semantics/`：Semantic Registry 与第一方 Provider。
- `src/intelligence/`：Traceability、Drift、Impact、Risk、Project Observatory Model。
- `src/verification/`：Verification Mesh、Language Quality、Stage Executor。
- `src/evidence/`：持久化 Evidence。
- `src/reconciliation/`：收敛计划与执行状态。
- `src/ui/`：TUI 与受保护 WebUI。

新生产模块必须落到规范 Product Scope，不要重新堆回仓库根目录或无责任边界的通用层。

## 并发不变量

真实 Tool 生命周期只有：

```text
request → queued → permit acquired → running → completed | failed
```

全局 Semaphore 是唯一并发容量 Gate。Composite Tool 等待子任务时不能占着父 Permit，否则低并发配置会死锁。

`parallel_tools` 在执行前建立路径资源依赖图；独立操作并发，读写冲突、父子目录、Move/Delete 等关系按依赖顺序执行。同文件 Edit 只有在同一 SHA 且范围不冲突时才能合并。

## 上下文与索引

- Project Context 必须有界并缓存。
- Tree-sitter Index 必须明确标记 `precision=syntax`。
- 完整 AST Cache 有上限；写入后必须失效相关记录。
- 搜索与源码扫描必须跳过构建目录、凭据和常见噪音目录。
- 不要为了“更智能”隐瞒 Provider Precision。

## 验证

修改后至少运行 quick Gate；发布级变更运行 full Gate：

```text
git diff --check
cargo check --locked
cargo fmt --check
cargo test --locked
cargo clippy --locked -- -D warnings
cargo build --release --locked
```

Risk 自适应 Verification、独立 Reviewer、Stage Evidence 与 HumanApproval 继续按各自 Producer 聚合，不能用单一 Pass 覆盖其他失败。

## 文档治理

官网只承担产品介绍与最短 Quick Start；所有会增长的 Reference、Agent 配置、安全规则、开发说明和 Release Notes 都统一放在 `docs/manual/`。

英文文档路径为 `/docs/`，中文路径为 `/zh/docs/`。中英文页面独立渲染、互相通过语言切换链接，不在同一正文中混排说明文字；旧 `/wiki/` 路径只保留兼容跳转。
