---
layout: docs
title: 不依赖 IDE 的工程观测
description: 官方产品调研、当前工作台范围与仍需补齐的观测能力。
lang: zh-CN
alternate: /docs/ide-independent-observatory/
permalink: /zh/docs/ide-independent-observatory/
---

# 不打开 IDE 也能观测工程工作

wcode 的目标是让操作者理解仓库、查看 Agent 活动、定位问题、审查变更并判断证据，而不必打开 IDE。首先要把已有信息连到源码和下一步动作。单纯增加状态卡片，无法完成这条操作路径。

这份 2026-09-30 的调研对照了有代表性的编辑器、Git 工具、CI 系统、终端工作台和可观测性平台的官方文档。它是能力对照，不代表穷尽了所有产品、复现了性能基准，也不表示 wcode 已经替代 IDE 的全部功能。

## 官方产品提供的参考

下表的产品行为来自所链接的一手资料。对应的 wcode 改进是工程判断，不沿用其他产品的性能数字。

| 官方来源 | 已确认的产品行为 | 对 wcode 的启发 |
| --- | --- | --- |
| [VS Code 代码导航](https://code.visualstudio.com/docs/editing/editingevolved) | 快速文件与符号导航、面包屑、定义和引用预览，以及关联源码的问题列表。 | 从工程信号进入文件、符号和具体行时保留上下文，并明确区分语法与实时语义精度。 |
| [JetBrains Problems](https://www.jetbrains.com/help/idea/problems-tool-window.html) | 按严重程度筛选，跳到源码，预览选中问题的上下文。 | 可选的问题列表应展示原因、生产者、观测时间和相关入口。 |
| [VS Code 测试](https://code.visualstudio.com/docs/debugtest/testing) | 由语言或测试扩展提供发现、状态、详细输出、源码导航和覆盖率。 | 区分已发现或已映射的测试与已执行结果；只有真实执行器提供数据后才展示逐测试结果。 |
| [VS Code 终端 Shell 集成](https://code.visualstudio.com/docs/terminal/shell-integration) | 检测工作目录和命令边界，展示退出状态并导航命令输出。 | 命令检查器需要程序、参数、cwd、排队与运行时间、退出或超时状态，以及有界脱敏输出。 |
| [Zed Agent 审查](https://zed.dev/docs/ai/agent-panel#reviewing-changes) 与 [Lazygit](https://github.com/jesseduffield/lazygit#features) | Zed 按文件和差异块审查；Lazygit 支持行或差异块选择、筛选和提交比较。 | 复用 wcode 现有比较层和快照绑定的源码差异查看；Git 写操作继续使用独立的策略检查。 |
| [GitHub 状态检查](https://docs.github.com/en/pull-requests/reference/status-checks) 与 [工作流日志](https://docs.github.com/en/actions/how-tos/monitor-workflows/use-workflow-run-logs) | 检查生命周期与最终结论分开，问题可标注源码，日志按任务和步骤搜索。 | 将未来 CI/PR 投影绑定到仓库、提交、执行尝试、生产者和平台。跳过与执行通过必须分开。 |
| [OpenTelemetry 日志关联](https://opentelemetry.io/docs/specs/otel/logs/) 与 [Grafana 链路导航](https://grafana.com/docs/grafana/latest/visualizations/explore/trace-integration/) | 时间、trace/span 和资源身份关联信号；链路节点可以打开相关日志。 | 先关联 Execution、工具任务、命令、验证和 Evidence 身份，再完善时间线。外部应用遥测需要独立的接入契约。 |
| [K9s 命令](https://k9scli.io/topics/commands/) | 工作台内可发现键盘导航、上下文切换、资源筛选、日志和详细描述。 | TUI 的问题、任务和提供方视图应采用一致的选择、筛选、详情和快捷键。 |

## 多 Agent 与 token 的市场取舍

| 一手来源 | 已确认的行为 | 本轮采用的原则 |
| --- | --- | --- |
| [Claude Code Agent teams](https://code.claude.com/docs/en/agent-teams) | 独立会话通过共享任务和领取机制协作；额外上下文及协调增加 token 开销。 | 只并行足够大且独立的工作，串行执行同文件修改；交接范围、版本和简短结果。 |
| [OpenCode Agents](https://opencode.ai/v2/docs/agents) | 主 Agent 使用子会话执行专项任务，子 Agent 有自己的权限和模型配置。 | 宿主启动模型，wcode 协调有租约的任务；领取不会授予文件权限。 |
| [Cursor Worktrees](https://prod.cursor.com/docs/configuration/worktrees) 与 [Cloud Agents](https://cursor.com/docs/cloud-agent) | 独立 checkout 或远程 VM 隔离 Agent 的文件和运行环境，提供可检查的产物。 | 共享工作区用明确不重叠范围；需要环境隔离时使用已有宿主 Worktree，检查合并后的版本。 |
| [OpenAI prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching) | 缓存复用提示前缀；实际效果由模型、请求和缓存计量决定。 | 工具目录保持稳定，任务路由在上下文内渐进披露；字节下降不等于计费或缓存收益。 |

`worklist_claim` 与 `worklist_submit` 给模型提供标准领取和提交入口。租约、依赖、当前版本和范围冲突保护协作；主 Agent 对组合结果独立验证。多 Agent 提高并行度与减少总 token 是两个需要分别测量的目标。

领取、续租和提交回复携带 Worklist 摘要，明确标记 `items_included: false`，保留当前版本、状态计数和可运行任务。当前领取项仍在 `handoff.item`，提交结果仍在 `result`。需要完整保留清单时读取 `worklist_status`（`items_included: true`）。这样避免每次单任务交接重复无关历史，不删除持久化任务，也不改变归属和证明规则。单任务交接在上下文预算和行动规划之前移除全局 Worklist 与 Execution 进度，避免无关任务错误要求子任务重复并行。待处理指令、强制验证级别和重新规划要求仍保留，普通协调者的 Agent Context 仍提供 Worklist 发现。交接上下文在精简后重新计算字节数和字节/4的 token 估算。序列化回复字节只是本地测量，不代表提供方 token 或计费节省。

## 已有基础

当前观测台已经展示架构与 Code Graph、任务活动、持久化 Execution、验证证据、变更影响、需求、语言提供方覆盖和有界文件树，也已有命令面板和项目导航。TUI 已有运行时与端点状态、工作区、资源压力、任务活动、工程摘要和授权界面。

[变更检查](../change-inspection/) 已经提供只读的工作树、暂存和未暂存比较，改前与改后语法映射、变更行定位，以及从图节点进入受保护源码窗口。源码身份、仓库版本、精度、脱敏和截断都是这些视图的一部分。新增文件浏览入口复用这些边界，进入受保护的源码导航。

这些信息的证据强度不同。架构声明表达意图，语法关系仍是语法，实时语言提供方可能提供语义，检查输出是执行数据，而版本绑定 Evidence 按自己的规则提供证明。放在同一个工作台里，也必须保留这种区分。

## 本轮实现范围

下列改进已在当前工作树实现，验证仍在进行。本文不代表全量检查、运行验收或发布已经完成。

| 界面 | 已实现的改进 | 边界 |
| --- | --- | --- |
| 共享项目状态 | 将已有策略、漂移、证据、验证、提供方和结构信号投影为有界问题与待处理列表。 | 说明来源、版本或新鲜度与覆盖不完整的情况，不生成虚构的编译器诊断或证明。 |
| TUI | Summary 总览、Attention 问题、Tasks 任务、Agents 协作和 Providers 提供方支持逐行选择与上下文详情。 | 实时数据变化时，选择仍绑定到当前工作区与记录身份；窄屏保留可读控件。 |
| WebUI | 可操作的观测覆盖条与可选待处理条目通向现有工作台视图。 | 导航不会执行检查、批准请求或将工作标记完成。 |
| 项目文件 | 可交互的文件树与最大文件条目连接受保护源码检查，支持每页 240 行并校验快照与 SHA。 | 读取有界且校验版本；受保护、缺失、过期、脱敏和二进制内容明确呈现。 |
| macOS Menu Bar | `wcode menu-bar` 读取 HTTP/MCP runtime 与 `wcode mcp-stdio` 共同发布的受保护本机 runtime-presence；`wcode menu-bar --json` 输出同一份可移植有界投影。 | 首版只观测 runtime/MCP/任务计数以及 partial/unknown；不保存 UI/OAuth token、owner、命令参数、源码路径或原始诊断，也不能批准、执行、取消或建立 Verification/Acceptance。 |

操作者路径是：选择工作区 → 选择待处理问题 → 查看详情 → 打开相关源码、活动、提供方、变更或证据视图 → 通过 Agent 或已授权操作解决原因 → 检查新证据。TUI、WebUI 与 Menu Bar 可以采用不同布局，但应解释同一份底层运行时事实。

Menu Bar 是独立 accessory UI 进程，不是第二套 wcode daemon，也不会作为普通 Dock App 抢前台；原生状态项只在 macOS 主线程事件循环已经运行后创建。HTTP/MCP 与 stdio 进程在受保护的本机 authority state root 下发布短生命周期心跳；超过活动窗口的记录不会继续算在线。菜单直接展示 transport 数量、最新 runtime 版本与 Workspace ID、最近 MCP 活动、任务/队列数量和覆盖状态。损坏、别名、超量或覆盖不完整都保持 partial/unknown。非 macOS 平台目前还没有原生托盘，但 `wcode menu-bar --json` 保留跨平台状态契约，供后续 Windows/Linux tray 复用。

## 后续优先级与取舍

1. **先验证关联上下文的问题入口。** 复用已有数据生产者与源码边界，能直接减少切换成本。保留稳定记录身份、键盘焦点、覆盖不完整提示和真实空状态。刷新失败不等于没有问题。
2. **补齐结果与输出检查。** 只读命令和检查查看器应支持搜索保留的输出，展示流、生产者、命令身份、生命周期、时间与遗漏标记。测试适配器再补充测试身份、失败位置和真实覆盖率。重放命令与查看输出是不同操作。
3. **接入提交绑定的外部检查。** CI/PR 观测要区分本地改动、提交 SHA、检查套件、任务、步骤、执行尝试、平台和结论。认证和轮询限制属于集成范围；上一次成功不能证明当前工作树。
4. **关联运行信号。** 先统一已有执行、任务和证据身份。应用 traces、logs、metrics 和 profiles 需要明确采集、保留、脱敏、资源身份与时钟处理。wcode 运行健康度不能代替应用可观测性。
5. **按真实需求加入调试与交互终端协议。** 断点、单步、栈帧、监视变量和局部变量需要调试会话及相应适配器。终端会话需要进程与 PTY 生命周期、输入归属、取消和跨平台验证，无法从普通命令执行推断。

## 仍然存在的缺口

本轮没有建立完整的调试器界面、断点与变量监视流程、任意交互终端会话、外部 CI/PR 日志浏览器或外部链路接入。全面的测试发现、逐测试结果历史、覆盖率可视化、完整历史源码浏览和安全的 Git 冲突解决也仍需后续工作。

文件树和图谱是有界观测。可见快照里没有某个文件，不代表它不存在；已映射测试不代表已执行；历史通过可能过期；缺失提供方不代表诊断干净。所有界面继续受 Workspace、保护路径、命令授权、OAuth 和版本规则约束。

## 验收标准

操作者应能辨认当前工作区与新鲜度，选择真实问题，理解生产者与原因，进入相关上下文，并判断哪些证据属于当前版本。键盘与窄屏流程、工作区切换、过期响应、刷新失败、提供方不可用和数据截断，都需要对实际实现验证。

使用项目验证门禁，并检查原生 TUI 渲染和 WebUI 交互。文档与静态检查不能单独证明端到端可用性、跨平台行为，或所有流程都不再需要 IDE。
