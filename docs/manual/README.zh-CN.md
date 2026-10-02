---
layout: docs
title: 文档
nav_title: 概览
description: wcode 中文文档入口
lang: zh-CN
alternate: /docs/
permalink: /zh/docs/
---

# 查看代码、管理任务、运行检查

**wcode 帮助编程助手读取项目、修改代码和运行检查。** 它通过 MCP 提供相关源码、代码关系和任务记录。修改文件前会核对文件哈希，检查结果会注明对应的代码版本。

使用流程是**理解项目 → 修改代码 → 审查变更 → 运行检查 → 查看结果 → 通过或阻止合并**。团队版 **wteam** 增加共享检查规则、审查记录和 GitHub 合并检查；完整团队流程仍需真实试点验收，不能把单项功能已实现当成整体已可交付。现状和实施计划见[商业产品审计](research-upgrades/#ai-变更验收商业架构2026-09-30)。

## 5 分钟理解 wcode

wcode 提供五类功能：

1. **理解（Understand）** — `agent_context` 返回与任务相关的源码、设计要求、代码关系、测试和修改位置。
2. **修改（Change）** — 文件操作受工作区目录、文件哈希、写入范围和权限限制。
3. **检查（Prove）** — `quick` 检查提供快速反馈；交付前仍需运行 `full` 完整检查。
4. **复用记录（Learn）** — 已验证的修改帮助定位相关代码；[失败记录](failure-memory/)提供纠错提示，但不能作为检查通过的依据。
5. **查看状态（Observe）** — 不用打开 IDE，也能查看项目结构、运行中的任务、代码变更、失败检查和当前版本的测试结果。

![wcode 工作流程](/assets/zh/engineering-loop.svg)

## 找到需要的界面

在 TUI 中按 **W** 打开当前项目的受保护项目状态，默认进入**工程架构**页。通过工作区选择器切换项目，通过语言和主题按钮调整显示。

| 界面 | 适合查看什么 |
| --- | --- |
| TUI 终端面板 | 连接状态、当前项目、Summary 总览、可选 Attention 问题、Tasks 任务、Providers 提供方和待授权请求。 |
| Setup Hub 设置中心 | 配置命令、只读预览、启动选项、性能预设和运行状态。 |
| 总览（Overview） | 项目概况、可操作的观测覆盖入口、工程流程、时间线、诊断和语言质量。 |
| 工程架构（Engineering architecture） | 系统与组件归属、设计依赖、实际观测关系和架构详情。 |
| 任务活动（Task activity） | 正在运行和排队的任务、执行耗时和资源占用。 |
| 验证证据（Verification evidence） | 当前版本的验证结果、就绪状态和已验证的仓库经验。 |
| 当前变更（Current changes） | 工作树变更、影响范围和需要执行的验证。 |
| 需求（Requirements） | 从需求到组件、代码、验证和证据的追溯关系。 |
| 项目文件（Project files） | 有界源码树和最大文件连接到分页源码检查，明确快照、SHA 和截断状态。 |
| 访问（Access） | 项目授权、可执行程序权限、精确操作和待处理请求。 |

检查已映射不代表已经运行，历史通过也可能属于旧版本。界面操作见[快速开始](getting-started/)，证据含义见[仓库理解与工程状态](software-intelligence/)。

## 从这里开始

| 你要做什么 | 文档 |
| --- | --- |
| 安装、启动并连接第一个仓库 | [快速开始](getting-started/) |
| 理解仓库、架构与工程状态 | [仓库理解与工程状态](software-intelligence/) |
| 接入本地编程智能体或云端连接器 | [智能体与 MCP 集成](code-agent-integrations/) |
| 理解工作区、命令和 OAuth 安全边界 | [安全模型](security/) |

## 核心概念

- [AI 变更验收商业审计](research-upgrades/#ai-变更验收商业架构2026-09-30) — 能力现状、统一验收架构、可信主体与执行凭据、外部 merge gate，以及 P0/P1/P2 计划；尚未完成的能力明确标记。

- [只读代码变更审查](change-inspection/) — 在当前变更内直接查看源码差异、符号、影响和证据，明确比较层与本次读取的快照身份。

- [不依赖 IDE 的工程观测](ide-independent-observatory/) — 官方产品对照、已实现的问题与源码导航、验证边界和剩余观测缺口。

- [变更验收](change-acceptance/) — 当前原生 Record、候选绑定检查、阻塞原因、历史与外部门禁边界。
- [本地验收 Policy](acceptance-policy/) — 原生预览、精确操作者批准、受保护代号历史及尚未完成的执行边界。
- [OSS / Commercial 边界](oss-boundary/) — 公开源码仓归属、单向合同、独立构建与许可证边界。
- [架构与模块边界](architecture-boundaries/) — `convention_status` 直接输出的文件/模块/目录/crate/仓库拆分信号与可执行规范。
- [产品范围](product-scopes/) — wcode 的产品能力与源码责任边界。
- [辅助编码流程](agentic-engineering/) — 短指令、按需上下文、并行执行与确定性验证的组合方式。
- [语言质量模型](language-quality/) — 全部 22 种索引语言共用一套能力矩阵：Syntax、真实初始化后的 Semantic、Format、Lint、Type、Static、Test、Security 与高级验证；缺口显式展示，不再用 Rust-centric Support Bit。
- [可维护性审查](maintainability-review/) — 结构增长信号、独立审查者与证据规则。

## 参考、运维与开发

- [检查历史评估](engineering-fitness/) — 不依赖模型的检索、上下文、编辑就绪与安全评估，区分正确性门禁与描述性性能测量。
- [技术论文](paper/) — 英文正文、中文原稿、指标定义、诊断结果与复现边界。

- [CLI 与 MCP 参考手册](reference/) — 命令、操作入口、传输方式与工具族的统一参考。
- [开发说明](development/) — 模块边界、运行时不变量、发布门禁与维护约束。
- [论文驱动的改进](research-upgrades/) — 纳入 0.6.2 发布准备的诊断上下文与依赖预览、论文原文和评估边界。
- [前沿工程](frontier-engineering/) — 选择性上下文执行、完整验证计划、新论文与可测的能力目标。
- [v0.9.0 发布说明](releases/v0.9.0/) — 统一问题观测、终端导航、安全源码分页、带范围的模型 Worker 协作与紧凑上下文。
- [v0.8.5 发布说明](releases/v0.8.5/) — 快照绑定的变更审查、精准代码到测试检索、Fitness 观测与 Observatory 交互修复。
- [v0.8.4 发布说明](releases/v0.8.4/) — Scope 隔离的并发 Writer、按授权 Grant 隔离的远程 Owner、统一 MCP Transport 协议策略与 300 轮发布验证。
- [v0.8.3 发布说明](releases/v0.8.3/) — 能力感知 MCP 发现、更紧凑的编辑就绪 Agent Context、隔离 Jev Policy、现代 Task 兼容与 300 轮发布验证。
- [v0.8.2 发布说明](releases/v0.8.2/) — 持久执行观测、结构化 Steering、更公平的代码图谱探索、资源隔离与 100 轮发布验证。
- [v0.8.1 发布说明](releases/v0.8.1/) — Jev Decision Plane 加固、更窄的结构化判断、严格 increase-only 权限、统一命名与更可靠的 WebUI 状态绑定。
- [v0.8.0 发布说明](releases/v0.8.0/) — Engineering Digital Twin、有界代码图谱与时间旅行、更快 Observatory、Ignore 感知扫描、可校准 Decision Plane 与任务优先 TUI。
- [v0.7.6 发布说明](releases/v0.7.6/) — 不依赖模型的 Check history、1K 编辑就绪上下文、多目标检索与对抗式发布验证。
- [v0.7.5 发布说明](releases/v0.7.5/) — 并发验证入口、入口信任代次校验、TUI 快捷键消歧与观测台响应式布局修复。
- [v0.7.4 发布说明](releases/v0.7.4/) — Workspace 会话级命令全授权、标准 MCP Image/Audio Content，以及按当前产品重做的文档/UI。
- [v0.7.3 发布说明](releases/v0.7.3/) — 有界开发命令默认自治、更完整的 Deno/Flutter 质量覆盖、更可靠的仓库智能 Cache，以及 WebUI/Verification 稳定性修复。
- [v0.7.2 发布说明](releases/v0.7.2/) — 更清晰的 Observatory／TUI／Setup 状态、模型高效工具发现、Agent Context 检索遥测与更精确的 Verification Mesh Target。
- [v0.7.1 发布说明](releases/v0.7.1/) — 稳定隧道抗抖动、项目实时切换、Semantic Auto 加固、减少开发授权摩擦并强化真实并行吞吐。
- [v0.7.0 发布说明](releases/v0.7.0/) — 重构 Project Status、加快项目状态加载、强化架构/证据工作流，并收紧发布契约。
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
