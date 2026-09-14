---
layout: docs
title: 论文驱动的改进
description: 论文与官方资料驱动的上下文检索、工具编排、模型 Host 效率、验证目标与实现边界。
lang: zh-CN
alternate: /docs/research-upgrades/
permalink: /zh/docs/research-upgrades/
---

# 论文驱动的改进

## 状态与范围

本页改动已纳入 [v0.6.2 发布准备](../releases/v0.6.2/)，不代表 v0.6.1 自动获得这些能力。需要使用新构建启动运行时并刷新工具 Schema。尤其不能向没有声明 `dry_run` 的旧运行时发送这个字段：忽略未知参数不等于安全预览。

实现扩展现有 `agent_context` 与 `parallel_tools`，不新增模型后端、嵌入服务、向量数据库、生产依赖或自动权限授予。论文于 2026-09-10 核查，覆盖提交于 2026-09-08 的研究；这是针对当前问题的选取，不是穷尽式文献综述。

## 研究取舍

| 论文原文 | 对 wcode 有参考价值的发现 | 实现取舍与限制 |
| --- | --- | --- |
| [Agent Retrieval Bench](https://arxiv.org/abs/2607.24882)，2026-07-27 | 仓库上下文获取应独立于补丁成功率评估，不同检索信号适合不同方法。 | 增加文件与行号定位，分别测试源码命中、缺失位置和预算限制。不宣称复现其基准，也不把堆栈帧视为已证明的根因。 |
| [Authority Is Not a String / CapScope](https://arxiv.org/abs/2609.08371)，2026-09-08 | 模型上下文之外的能力检查可以限制仓库文本和工具输出诱导的操作。 | 定位信息和预览只是数据，不是授权。预览不消费或授予权限。这不等于实现了 CapScope 的逐智能体能力系统，也不意味着免疫提示注入。 |
| [The Complexity Trap](https://arxiv.org/abs/2508.21433)，2025-08-29，修订于 2025-10-27 | 在论文的 SWE-agent 实验中，简单的观测遮蔽与模型摘要相比具有竞争力。 | 使用有界原文片段，而非增加摘要模型。本轮没有实现对话轨迹遮蔽，也不把论文中的成本降幅套用到 wcode。 |
| [Do Context Files Help Coding Agents?](https://arxiv.org/abs/2607.27250)，2026-07-28 | 一个小样本双智能体消融实验没有检测到上下文文件带来的正确率改善，结论受统计检验能力限制。 | 保持必需指令简短，按需提供更丰富的上下文。这不能证明仓库指令没有价值。 |
| [LLMCompiler](https://arxiv.org/abs/2312.04511)，ICML 2024；首次提交于 2023-12-07 | 分离计划、任务调度与执行，可实现遵守依赖的并行调用。 | 将已有调度图以零子任务执行的形式提供出来。不照搬其加速倍数，也不再叠加一个模型规划器。 |

## 2026-09-15 追加调研：检索精度与模型／Host 效率

v0.7.2 继续保持控制面 Model-neutral，但会主动优化当前 Coding Model 与 Host 真正使用的接口。取舍以能力而不是厂商为中心：Host 可以支持 Deferred Tool Search、Prompt Cache、Native Parallel Call，也可以一个都不支持；仓库语义、授权和验证绝不会因为某个模型品牌字符串而改变。

| 原始资料 | 当前信号 | wcode 取舍 |
| --- | --- | --- |
| [Agent Retrieval Bench](https://arxiv.org/abs/2607.24882)，2026-07-27 | 没有一种检索方法在所有 Coding Task 上都占优；预算内 Context Yield 与下一步真正需要的文件应独立于最终补丁评估。 | 保持 `agent_context` 自适应和任务路由，展示 candidate→delivery 效率，并优先精确定位／关系，不靠简单放大 Context。 |
| [ContextBench](https://arxiv.org/abs/2602.05892)，2026-02-05 | Coding Agent 往往过度检索，而且“探索过”与“真正使用”的上下文之间存在明显差距。 | 保持有界 Context Pack，不把第二次全仓库 dump 变成必需步骤；衡量最终交付给模型的上下文，而不是只追 Recall。 |
| [CORE-Bench](https://arxiv.org/abs/2606.11864)，2026-06-10 | Agentic Repository Retrieval 与孤立代码片段搜索明显不同。 | 保留 Repository State、issue→edit 与 broader-context 路由，不用一个通用 Embedding Query 替代全部检索。 |
| [Anthropic Advanced Tool Use](https://www.anthropic.com/engineering/advanced-tool-use) | 当工具目录较大时，Deferred Tool Discovery 能降低 Tool Definition Context，并改善工具选择。 | 仅把少量 wcode 核心工具标为 `dev.wcode/preloadRecommended=true`；专业工具按需发现。这只是通用 `_meta` 建议，不形成 Anthropic 专属依赖。 |
| [OpenAI Codex Agent Loop](https://openai.com/index/unrolling-the-codex-agent-loop/) 与 [Agents API](https://openai.com/index/introducing-the-agents-api/) | 精确稳定的 Prompt／Tool Prefix 有利于 Cache 复用；Tool Search 与 Programmatic Orchestration 可以减少不必要的工具上下文。 | 保持 Tool 顺序与 Server Instructions 确定性，严格控制目录体积，使用一个稳定的核心 Preload 集，而不是每个模型生成一份目录。 |
| [Gemini Context Caching](https://ai.google.dev/gemini-api/docs/caching) | 重复稳定 Prefix 能提高 Implicit Cache 命中机会。 | 静态 Instructions 与 Tool Definition 保持稳定；动态仓库状态留在 Tool Result 和 Agent Context 中，不烘焙进定义。 |

`preloadRecommended` 不代表工具结果可以缓存、调用天然幂等或无需授权。只读／破坏性／幂等语义仍使用标准 MCP Annotation，wcode Runtime 继续执行自己的 Policy Check。忽略这条 Hint 的旧 Host 仍获得相同的确定性完整目录和行为。

同一版本也会保守收敛 Verification Mesh 的 Advanced Stage Target。High／Critical Plan 优先使用已经有确定性风险归因的源码；CSS／HTML 展示源码除非显式 Advanced Executor 选择它们，否则不会制造语言级 Property／Mutation／Fuzz Cross-product。Full Deterministic Verification 与 Security／Adversarial Review 不减少。缺少真实 Rust Mutation／Fuzz Executor 时继续显式暴露 Gap；普通 `cargo test` 绝不会冒充 Mutation 或 Fuzz Evidence。

## 诊断上下文

在现有 `agent_context` 查询中包含明确位置，例如 `error[E0308] at src/runtime/harness/context_budget.rs:33:9`。支持 `file:line`、`file:line:column`、`file#Lline`，以及支持的源码、配置、文档文件名。接受反斜杠分隔的相对路径和无空白的引用标记；这不是覆盖所有语言堆栈语法的完整解析器。

最多处理四个不同定位点。带行号的位置在文件和语法大纲 SHA 一致时，优先选择包含该行的最小语法定义。无法解析语法的文件和纯文件定位保留确定性的文件目标，不虚构符号。片段从给出的行开始，最多读取十三行，初始上限为 1,600 个字符，现有 Token 预算可以继续裁剪。文件 SHA 与片段 SHA 必须一致，语法事实不会被标成编译器语义。

`retrieval` 字段提供 `strategy`、`resolved`、`anchors` 和指引。定位状态区分 `resolved`、`unavailable`、`outside_boundary`、`invalid_location`、`changed_during_read`。明确位置不可用时，不会把无关词法命中提升为可编辑目标。没有位置的普通符号查询继续使用现有检索路径。堆栈帧是检查位置，不是已经证明的根因。

路径仍经过 Workspace 保护。父目录穿越、受保护文件和符号链接不会因路径规范化而获得权限；绝对位置必须属于所选根目录。不通过后缀猜测其他 CI 检出路径。新请求重新获取原文和 SHA，不把旧缓存当成编辑许可。

## 依赖预览

确认运行时 Schema 声明此字段后，向 `parallel_tools` 传入 `dry_run: true` 和计划执行的任务。预览复用真实预检、编辑合并规则和实际根目录依赖图，但在派发任何子任务之前返回。即使执行额度已满也能返回，不排队执行命令，也不创建授权请求。

响应包含 `execution: "dependency-preview"` 与 `tasks_executed: 0`，并提供从零开始的任务 `index`、`depends_on`、`coalesced_into`、路径数量、`waves`、`initial_ready`、并发边界和合并数量。使用数字索引，避免回显任意 ID、文件正文或负载中的秘密。分层表示依赖结构，不是整层执行屏障；实际执行仍由前置任务完成驱动。

`authorization_checked: false` 和 `file_preconditions_checked: false` 很重要：预览不能证明写入获批、文件存在、SHA 匹配，也不代表所有工具专属参数均有效。真实执行会重新建立计划并检查状态。预览不推断资源模型中不存在的逻辑依赖。`dry_run: false` 或省略字段保持普通执行；错误的字段类型在子任务执行前拒绝。

只有依赖不确定时才需要预览，不应将它变成每次任务额外必调的步骤。依赖已明确时，仍优先使用独立的顶层并行调用和已知输入的批量工具。

## 基于实际使用的流程改进

明确的操作请求（`git commit`、`git status`、`提交`、`全量检查`）在没有指定 Product Scopes 时走紧凑操作上下文，不再扫描无关符号或 Design State；`inspect commit` 这样的源码问题仍使用原检索路径。没有实际构建完整上下文作对照时，不虚报节省比例。

验证会完成当前独立阶段，但出现失败后不再启动后续阶段。`skipped_checks` 不计入执行数量，也不会生成通过证据。需要完整诊断时，给 `verify_project` 设置 `fail_fast: false`。大段成功日志保留有界测试汇总和尾部；失败日志保留原来的较大诊断额度。省略内容由 `output_truncated` 明确标注。

常见 Git 查询（分支、标签列表、当前分支和远端 URL）沿用只读策略；明确的分支创建与切换、轻量标签、带说明的附注标签，以及 `git restore --staged -- <paths>` 改走精确用户批准，不再永久拒绝。这不是开放任意 Git：强制替换或删除、宽泛路径、Shell、辅助执行与配置重定向、受保护路径不属于这些受支持形式。错误参数和无效工作目录在生成无用批准请求之前就被拒绝。

TUI 展示选中请求的 ID、工作区和换行后的详情，使用 `PgUp`/`PgDn` 翻阅长说明，批准按钮保持可见；原来的请求 ID 绑定和遮挡保护保留。客户端不支持表单询问时，仍须到 TUI 或受保护 WebUI 批准，修改仓库不能凭空增加客户端能力。批量命令一次批准方案已评估，但不包含在本次实现中。

这些取舍参考了 [Anthropic 的高级工具使用](https://www.anthropic.com/engineering/advanced-tool-use) 中的按任务选择工具，以及 [AgenTRIM](https://arxiv.org/abs/2601.12449)（2026-08-30 修订）中保留有效能力的运行时控制。这里只做有边界的工程适配，不宣称复现整套系统，也不移用其基准数字。

本地验收样例要求：故意设置无效 Cargo 清单时，快速失败模式执行两项而完整诊断执行五项；合成成功日志缩减超过 80% 且保留测试汇总；操作型上下文不超过 4,000 字节。这些是可执行测试的验收阈值，不是端到端智能体提速承诺；只有测试真实完成后，才能判定当前版本达标。

## 审查加固

Git 提交和附注标签的说明参数，只有在完整命令形式校验通过后才按纯文本处理。说明中提到 `../migration` 或 `.env` 并不会读取这些路径，因此可以请求精确用户批准。真实路径参数、从文件加载说明的选项和控制字符仍保留检查；识别出说明文本不等于批准执行。

`verify_project` 显式传入的级别、超时和快速失败参数在派发前校验，不再把错误值悄悄替换成默认值。禁用执行时，操作上下文返回 `workspace_exec_disabled`，不再建议执行命令。窄屏授权弹窗使用精简按钮，40×10 字符窗口也能同时显示批准与拒绝。

验收项要求的部分检查未执行时，证据标记为尚无定论；已有失败仍保持失败。单个语言质量检查只生成自身检查证据，不生成整项目验证通过或通用测试验收通过。旧版曾生成的整项目语言质量记录不参与确定性验证门禁聚合。

确定性结果按生产者和验证策略分别取最新记录：后来的快速检查通过不能清除之前的全量失败，不同生产者的失败也不能互相覆盖。同一生产者后来完成的全量检查包含快速检查，因此可以替代其更早的快速结果。时间戳相同时遇到冲突按失败关闭处理。上述规则仍绑定计划的精确代码与 Design 版本。

## 命令超时诊断

普通命令与仓库执行器复用一个结果收集器。命令超时现在返回失败的 `CommandResult`，保留已捕获并脱敏的标准输出和错误输出，不再直接丢弃诊断。`timed_out` 表示命令执行期限，而不是 HTTP 传输超时。`output_incomplete` 表示管道读取失败或清理期结束时仍未读完；该值为 false 只说明已产生的输出读完了，不代表命令完成了原定工作。超时或捕获不完整时，即使清理期间观察到退出码零，`success` 也不能为 true。

收集器统一持有两个管道读取任务，取消请求不会使读取任务脱离管理。进程清理和随后管道排空分别最多等待两秒。失败结果携带 `retry_guidance`，要求先核对效果再重试；现有授权、进程组管理、输出脱敏和执行额度继续生效。不增加回滚、自动重放命令或恰好执行一次的保证。设计参考 [AWS Builders' Library](https://aws.amazon.com/builders-library/making-retries-safe-with-idempotent-APIs/) 对“响应丢失”和“没有产生副作用”的区分。

回归样例只在临时工作区执行合成 Rust 程序，检查超时诊断及既有效果保留、取消后不发生延迟写入、大量双路输出不会死锁或无限增长、普通退出行为兼容，以及超时输出仍经过脱敏。底层日志仍按每路最多 256 KiB 保留前缀，本次没有实现头尾同时保留；超出上限的末尾诊断仍可能省略。进程被终止不等于远端副作用已经撤销。

## 并发与进程队列

`SLOTS` 表示工具准入额度的占用，不等于 CPU 核心使用量；`PEAK` 是启动以来同时占用工具额度的峰值。一次批量读取或编辑即使内部处理多个文件，外层仍是一个工具。没有独立任务排队时，占用低不能证明调度器拖慢吞吐；不要篡改计数或制造无用任务来填满槽位。

前台 CPU 工作线程数现在取可用硬件并行度、八个线程、内存推导上限与请求并行度中的最小值；后台 CPU 目标不变。阻塞线程池上限为 64，避免 32 个独立阻塞请求被暗中的十六线程上限卡住。批量文件修改使用共享、有界的 I/O 池，默认 512 MiB 预算下为十六个线程，文件系统等待不再占用 CPU 索引池。读取仍使用 CPU 池：扩大热读取并发在本地配对实验中变慢，已撤回该路径的改动。这些是容量限制，不代表等比例提速，也不是对子孙进程内存的硬隔离。

明确、固定形式的 Git 状态和差异检查使用独立检查队列，根据内存、CPU 和工具额度提供一到四个名额。其他命令和仓库执行器保留原有重型进程限制，默认预算下为两个。命令策略、用户批准、进程监管和资源压力准入仍执行；更换队列不能批准修改或辅助程序选项。外层工具槽位耗尽时，仍可能在进入进程队列之前等待。

资源信息提供 `child_queue`、`probe_queue`，包含 `active`、`limit`、`waiting`、累计 `waits`、`total_wait_ms` 和 `max_wait_ms`。占用表示已保留的名额，不表示 CPU 正在运行这些进程；取消等待会清理计数。宽屏 TUI 展示 `PROC`、`GIT` 和内层合计 `Q`，与工具槽位及峰值分开。不增加授权开关，不自动重启实例。

回归对照检查旧上限下十六个与新上限下三十二个阻塞任务实际启动、重型名额全部占用时真实 Git 检查仍可完成，以及取消、关闭队列和资源压力边界；文件批处理对照保持 SHA 校验与逐文件错误结果。性能样例记录真实线程数量和原始耗时，测试构建的结果不等于生产、模型或网络提速。需要以新构建启动运行时，这些改动才会影响当前面板。

## 一致性检索与饱和处理

`symbol_context` 在返回签名、位置和调用关系前，对比正文 SHA 与索引版本。发现不一致就使旧记录失效，最多补取一次；符号身份改变或文件持续变化时明确报错。文件大小与修改时间相同，不代表内容相同。这里保证返回的单文件上下文一致，不保证整个仓库的原子快照，也不保证响应后文件永远不变。

冷启动的 `ensure_indexed`、单关键词和多关键词搜索，共享按工作区根目录与文件区分的构建任务。等待者不持有全局索引状态锁或 CPU 名额，取得构建权后重新检查缓存，复用已经成功的构建；热缓存命中保持原有快路径。最多登记 256 个独立活跃构建，回收空闲项，不能为了腾位置移除正在使用的项。单文件、路径前缀失效与强力内存清理都会更新正在构建任务的发布版本，防止旧结果稍后重新写入。读取失败或仅用于协调的空锁发生异常，不会永久阻塞后续构建。这是消除重复工作，不是增量 Tree-sitter 解析或跨请求源码快照存储。

执行进程的 MCP 工具（`run_command`、`language_quality_run`）和项目验证检查，在占用总工具槽位之前先取得执行准入名额。总量为 32 时，这类请求最多占用 28 个总槽位，为非命令工具留下四个余量；非命令工具仍可使用全部 32 个。单槽位配置保留一个可用名额。调用方取消后，已经开始的阻塞工作仍持有两种名额直到结束；排队取消会释放预留。先到先服务沿用 [Tokio 信号量语义](https://docs.rs/tokio/latest/tokio/sync/struct.Semaphore.html)。这不保证其他工具、CPU 或内存饱和时的固定延迟，也不绕过用户批准。

执行样例在 `target/wcode-index-sharing.json` 记录真实构建次数与耗时，在 `target/wcode-admission.json` 记录 32 个命令请求排队时、真实进程内 MCP 读取耗时。测试检查目标不变、版本拒绝、不同文件独立、构建表容量与回收、旧结果晚到、队列顺序和名额恢复。这些是本地测试构建诊断，不是模型或网络基准；文件由测试重新生成，本身不等于发布证据。

## 验证与限制

回归测试覆盖 1,000/1,400/4,000 Token 预算下的报错行保留、非代码文件、缺失与被拦截位置、可移植位置语法、SHA 变化、执行额度耗尽、编辑合并、无效预览参数以及原执行行为不变。测试使用本地样例，不是 SWE-bench、Agent Retrieval Bench 或模型质量评测。

先运行 `review_changes`，再运行 `verify_project(level="full")`。Rust 全量检查包含 `cargo clippy --locked --all-targets -- -D warnings`，让测试代码获得与 CI 相同的 Clippy 覆盖。应以实际测试报告和绑定版本的证据为准；本文本身不是某个构建通过的证明。

不宣称通用延迟、Token 成本或修复成功率提升。在代表性仓库上分别测量上下文命中、工具往返次数、负载大小和耗时后，才能给出提速数值。发布这些变更前仍需新运行时冒烟测试与跨平台 CI。
