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

这里是中文文档入口。官网只负责介绍产品与快速上手；协议细节、Agent 接入、安全边界、质量模型和开发约束统一放在文档站，避免在首页堆叠参考手册。

## 从这里开始

| 你要做什么 | 文档 |
| --- | --- |
| 安装、启动并连接第一个仓库 | [快速开始](getting-started/) |
| 理解 wcode 的软件智能闭环 | [软件智能](software-intelligence/) |
| 接入本地 Coding Agent 或云端 Connector | [Agent 与 MCP 集成](code-agent-integrations/) |
| 理解工作区、命令和 OAuth 安全边界 | [安全模型](security/) |

## 核心概念

- [Product Scope](product-scopes/) — wcode 的产品能力与源码责任边界。
- [Agentic Engineering](agentic-engineering/) — 短指令、按需上下文、并行执行与确定性验证的组合方式。
- [语言质量模型](language-quality/) — 语法、语义、格式、Lint、类型、测试、安全与高级验证能力矩阵。
- [可维护性审查](maintainability-review/) — 结构增长信号、独立 Reviewer 与 Evidence 规则。

## Reference、运维与开发

- [CLI 与 MCP Reference](reference/) — 命令、操作入口、Transport 与 Tool Family 的统一参考。
- [开发说明](development/) — 模块边界、运行时不变量、发布门禁与维护约束。
- [v0.3.0 发布说明](releases/v0.3.0/) — 0.3 系列的产品形态与关键变化。

## 推荐工作流

```text
workspace_info
  ↓
scope_status + design_status + project_context
  ↓
software_context
  ↓
实现 / 编辑
  ↓
review_changes + drift_status + impact_analysis + risk_status
  ↓
verification + evidence
  ↓
reconciliation
```

文档中的命令、工具名、协议名和字段名保留其原始技术标识；说明文字本身按页面语言保持一致，不再在同一段正文中来回切换语言。
