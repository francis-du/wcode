---
layout: docs
title: 文档
nav_title: 概览
description: wcode 中文文档入口
lang: zh-CN
alternate: /docs/
permalink: /zh/docs/
---

# wcode 文档

这里是中文文档入口。官网只负责介绍产品与快速上手；协议细节、智能体接入、安全边界、质量模型和开发约束统一放在文档站，避免在首页堆叠参考手册。

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
- [v0.5.2 发布说明](releases/v0.5.2/) — 更干净的 MCP 调用、依赖感知并行 Agent 工作流、一键配置、stdio 人工授权与更严格的 Verification 治理。
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
