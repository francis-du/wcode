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

## 从这里开始

| 你要做什么 | 文档 |
| --- | --- |
| 安装、启动并连接第一个仓库 | [快速开始](getting-started/) |
| 理解 wcode 的软件智能闭环 | [软件智能](software-intelligence/) |
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
- [v0.6 发布说明](releases/v0.6/) — Parallel-first Agent 执行、更强仓库上下文、LSP 语义理解与安全项目命令的精确授权。
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
