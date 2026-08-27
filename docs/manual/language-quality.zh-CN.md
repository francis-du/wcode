---
layout: docs
title: 语言质量模型
description: wcode 的显式语言质量能力矩阵与 Provider 规则
lang: zh-CN
alternate: /docs/language-quality/
permalink: /zh/docs/language-quality/
---

# 语言质量模型

wcode 不用一个 `supported=true` 描述语言支持，因为“能解析”与“能做语义分析、Lint、类型检查、测试和安全扫描”完全不是一回事。

## 能力维度

每种检测到的语言分别报告：

- `syntax`：Tree-sitter 解析与导航；
- `semantic`：真实可运行的第一方 LSP Provider；
- `format`：只检查、不改写的格式 Provider；
- `lint`：Lint Provider；
- `type_check`：类型检查；
- `static_analysis`：静态分析；
- `test`：测试；
- `security`：安全扫描；
- `property`：Property Test；
- `mutation`：Mutation Test；
- `fuzz`：Fuzz；
- `runtime_canary`：运行时 Canary。

缺失维度必须显式显示为 Gap，不能因为 wcode“知道某个工具存在”就假装仓库已经启用它。

## Provider 选择

仓库声明或语言原生能力优先于 wcode 自己猜测。`language_quality_run` 只允许运行：

1. 当前仓库确实检测到；
2. Provider 已声明；
3. 程序可用；
4. 命令处于 check-only 模式；
5. 通过现有执行授权边界。

运行结果写入当前 Revision 的 Evidence。

## 语义精度

没有真实 Language Server 时继续使用 Tree-sitter syntax precision，不把语法索引包装成编译器级语义。

## 与 Verification 的关系

Language Quality Matrix 描述“当前有哪些能力”；Verification Plan 根据 Risk 决定“这次变更必须运行哪些能力”。两者不能合并成一个模糊的质量分数。
