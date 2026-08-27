---
layout: docs
title: Product Scope
description: wcode 的产品能力与源码责任边界
lang: zh-CN
alternate: /docs/product-scopes/
permalink: /zh/docs/product-scopes/
---

# Product Scope

wcode 按产品行为划分能力，而不是按“Controller/Service/Util”这类通用技术层堆目录。同一套 Scope 同时用于源码架构、语义过滤、Software Context、MCP Tool Metadata、Project Context、Convention 和 Agent 指引。

## 12 个规范 Scope

| Scope | 责任 | 主要源码 |
| --- | --- | --- |
| `runtime` | 进程生命周期、Harness、全局运行时 | `src/main.rs`, `src/runtime/`, `src/scopes/` |
| `integrations` | MCP、OAuth、Agent Plugin、Connector | `src/integrations/` |
| `workspace` | 文件系统边界、授权、调度、Convention | `src/workspace/` |
| `design` | Desired Software State | `src/design/` |
| `graph` | Syntax Index、Software Graph、Provider/History | `src/graph/` |
| `semantics` | Semantic Registry 与 Provider | `src/semantics/` |
| `traceability` | Traceability、Drift、Impact、Context | `src/intelligence/` 相关模块 |
| `risk` | Risk Policy | `src/intelligence/risk.rs` |
| `verification` | Verification Mesh 与质量 Provider | `src/verification/` |
| `evidence` | 持久化 Evidence | `src/evidence/` |
| `reconciliation` | 收敛计划与执行 | `src/reconciliation/` |
| `experience` | TUI 与 WebUI | `src/ui/` |

## 对 Agent 有什么用

先调用 `scope_status` 看当前仓库的源码如何映射到 Scope，再把相关 Scope 传给 `software_context`。这样 Agent 可以先缩小源码导航范围，而不是一上来读取整个仓库。

```text
scope_status
  ↓
选择相关 Scope
  ↓
software_context(scopes=[...])
```

`semantic_query` 也支持 Scope Filter；没有 Scope 的 Semantic Fact 继续视为全局事实。

## Product Scope 不会扩大权限

Scope 是上下文与架构边界，不是权限声明。它不能扩大 Workspace、绕过命令授权、跳过 SHA 前置条件，也不能替代 Verification。

## 第三方仓库

第三方项目可以使用自己的业务 Scope。只有明确声明 wcode 自身 canonical Scope 约束的 Design State，才会把未映射 wcode Product Scope 当作 fail-closed 门禁。
