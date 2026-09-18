---
layout: docs
title: 论文驱动的改进
description: 论文与官方资料驱动的上下文检索、工具编排、模型 Host 效率、验证目标与实现边界。
lang: zh-CN
alternate: /docs/research-upgrades/
permalink: /zh/docs/research-upgrades/
---

# 论文驱动的改进

## 2026-09-17：从真实源码生成反例候选（工作树）

可选的 `review_changes(adversarial=true)` 现在通过 `candidate_search` 补充源码候选：最多检查六个不同的变更源码文件，以受控元信息筛选不超过 256 KiB 的文件，每文件最多匹配 32 个 AST 节点，最终最多推荐三个变异。复用真实 Tree-sitter 索引，不把注释和字符串里的文本当成代码。当前支持布尔字面值翻转和限定形状的简单比较运算符变异；例如 `count < 8` 还会生成符号绑定值 `7`、`8`、`9`，但变量是否对应输入参数、真实类型和值域仍需验证。边界生成使用检查算术，不会在有符号 64 位整数端点溢出。

每个候选保留源码 SHA、精确原文和替换文本、字节范围、行号及 `proposed-not-typechecked` 状态。安全信号明确指向的文件优先，其次使用静态成本启发式和稳定的路径／字节顺序；这不是实测缺陷概率或延迟预测。重复文件和输入顺序变化不会改变候选选择。范围是变更文件，不限于 diff 中改动的行；缺失、受限、过大、不支持、解析失败和所有输出截断均保持可见。无 AST 命中时还会核对文件级解析状态，不把空结果伪装成成功分析。Workspace、符号链接、脱敏和旧 SHA 保护继续保留。AST 工作在阻塞线程上持有真实工具配额，普通 Review 不扫描候选；非布尔值的 `adversarial` 参数直接拒绝，不再静默关闭对抗审查。

[Hypothesis 状态测试](https://hypothesis.readthedocs.io/en/latest/stateful.html) 将动作生成、前置条件和独立模型／不变量分开；[cargo-mutants](https://mutants.rs/) 通过变异寻找测试缺口。本实现借鉴这些边界，但不是新增自动执行引擎：始终明确 `executed=false`、`oracle_required=true` 和 syntax 精度。变异只能在隔离副本中、非空基线通过后进行类型检查；编译失败、零测试和超时不是成功杀死变异，存活变异仍须检查可达性与等价性。没有候选或杀死一个变异，都不能证明整体正确或关闭无关 QA 问题。

运行 `cargo test --locked --lib harness_quality -- --nocapture` 及 `review_changes_adversarial_mode_attaches_non_evidence_questions` 测试。冻结的 Rust 样例实际编译并运行一项通过的基线测试，再使用生成的边界值与独立预期值表让 `<` 改为 `<=` 的变异失败，原源码 Workspace 保持不变。这不等于在用户项目上自动执行了 Mutation Stage。Rust／Go／TypeScript／Python 四语言回归验证 AST 候选提取，不代表四语言编译通过。任意参数合成、类型和值域推断、并发交错生成及自动实验裁决不在本批实现范围内。

## 2026-09-17：把对抗 QA 做成可证伪协议，而不是自我信心（工作树）

本次工作树为 `review_changes` 增加 `adversarial=true` 模式：它复用同一次确定性变更审查，生成有界、无模型的反向质疑层。核心规则很简单：问题可以揭露假设，但不能由提出问题的一方自行“证明关闭”。OpenAI 的 [Harness engineering](https://openai.com/index/harness-engineering/) 把额外 Agent Review 与反馈闭环视为工程基础设施，而不是相信第一次输出的理由；[Codex Security](https://openai.com/index/why-codex-security-doesnt-include-sast/) 强调验证代码里的防线是否真的保证系统依赖的安全性质，而不只是识别熟悉的代码形状。Self-Refine 说明反馈/修订在部分任务上可能改善模型输出，但 [How Much LLM Does a Self-Revising Agent Actually Need?](https://arxiv.org/abs/2604.07236) 也提醒：增加 LLM 修订并不会单调提高结果。因此 wcode 把质疑协议外置，并把证明留在 Critique Loop 之外。

该工具最多生成十二个唯一的可证伪问题。基础问题会反问产品验收、隐藏影响面、当前 Revision 的证据新鲜度和负路径；源码与测试一起修改时会质疑测试是否过拟合；source-without-test、安全敏感、Manifest、删除测试、生成物、超大变更、未跟踪文件和 Review 截断等确定性信号会触发更针对性的问题。每个问题都包含正在被质疑的 Claim、反例问题、当前确定性 Signal、关闭它所需的 Evidence，以及可用于取证的 wcode 工具。

整个包明确标记为 `challenge-packet-not-evidence`。它不会被持久化成 Pass，不会抬高语义精度，也不能覆盖确定性失败。每个问题还会带一个有界 `counterexample_experiment`：实验类型、相关变更目标、可证伪假设、具体失败条件、可执行该实验的工具或 Verification Stage，以及真正能关闭问题的证据类型。这些只是可执行假设，不代表 Mutation／Property／Fuzz／Runtime 已经运行。对于确实需要模型审查的变更，它只桥接到现有 Verification Mesh：创建 Verification Plan，再以 `adversarial` 角色领取独立盲审任务。该审查仍是独立 Evidence Producer；确定性 Verification、Property/Mutation/Fuzz/Runtime Stage 和 HumanApproval 继续遵循原有 fail-closed 规则。工作树干净时不生成问题，避免把“无限自我反思”变成每次任务都必须执行的固定循环。

可运行 `cargo test --locked --lib adversarial_qa_ -- --nocapture`。确定性回归检查 Non-Evidence 契约、基础质疑类别、Review Finding 的针对性问题、去重、硬上限和干净工作树静默。它们只衡量质疑协议本身，不代表模型一定能正确回答所有问题，也不宣称重复 Critique 能在编码任务上超过 Kimi、Claude Code 或 Codex。

## 2026-09-17：诊断格式直接进入安全编辑（工作树）

现有 `agent_context` 查询现在可识别 Python 的 `File "src/worker file.py", line 37`、编译器的 `src/worker.ts(23,7)` / `src/service.cs(23,7)`，以及 PHP 风格的 `in src/handler.php on line 29`。带引号的路径保留空格与 Unicode，也支持括号包住整条带引号的 `file:line:column`。这是复用现有源码/SHA 入口的输入归一化，不新增工具，也不把堆栈位置当作已经证明的根因。

行号标记只绑定同一物理诊断行；重复位置在原有四个锚点上限之前去重。已识别格式中的非法坐标保持无效，不再静默读取第一行；URL、未闭合引号和超长引用片段不会再产生本地文件名后缀。没有 URL 解码、Shell 执行或权限放宽。Workspace 边界、受保护路径、符号链接拒绝、源码脱敏与旧 SHA 拒绝继续执行。解析器只覆盖说明中的有界格式，不宣称兼容所有语言的堆栈语法；未知格式仍需提供明确位置。

可运行 `cargo test --locked --lib trace_format_ -- --nocapture`。独立测试位于 `tests/unit/runtime/harness/diagnostic_formats.rs`，12 项回归覆盖 72 种 Python 帧组合、编译器/PHP 格式、引号/括号嵌套、非法坐标、去重/上限及路径边界。本地 Harness 样例分别在 1,000/1,400/4,000 预算下只调用一次 `agent_context`，拿到第 37 行和源码 SHA，从该响应直接执行安全编辑，再确认旧 SHA 被拒绝。这验证检索到编辑的协议，不是实际运行 Python，也不是远程 MCP 延迟或模型编码能力排名。

## 2026-09-17：全局图预算与构建开销（工作树）

2026-08-03 提交的 [DyRetriever](https://arxiv.org/abs/2608.01927) 提供按需获取依赖、避免每次构建完整静态图的参考；[Agent Retrieval Bench](https://arxiv.org/abs/2607.24882) 将仓库检索与补丁成功率分开评估。本次保留 wcode 确定性的语法图和现有预算，不引入论文中的模型驱动构图，也不移用其加速数字。

两项回归先复现了逐文件优先级的缺陷：第一个优先文件的无关定义会耗尽预算，挤掉后续文件的精确目标；同文件里位于目标之前的大量调用者也会挤掉目标定义。现在先在整个图中为精确目标分配预算，再选择直接调用者，最后补充其他定义。测试覆盖不同文件顺序、6,200 个干扰定义与 5,000 符号图预算，以及目标数量超过预算的情况。上限不变，被省略的定义仍明确标记为截断。

内部语法构建器直接追加已经去重的关系，在返回前统一验证完整图，避免每加入一条边就遍历不断增长的关系列表。通用图插入 API 保持原有校验。补图合并使用包含端点、关系类型和完整来源信息的临时集合，保留顺序与独立证据，拒绝重叠节点的来源版本冲突，并在返回前继续检查非法端点和自环。这只保证重叠源码不混用版本，不保证全仓原子快照。

手动复现：`cargo test --release --locked --lib graph_build_release_cost -- --ignored --nocapture`。样本包含 500／5,000 个定义，预热两次后保留十一组暖缓存数据；序列化在计时外完成，每组输出均与首组逐字节核对。报告中位数、p95、图尺寸和关系数量，不代表模型推理、冷启动全仓发现、MCP 网络延迟或补丁成功率；耗时不作为 CI 成败阈值。`tests/unit/graph/budget_perf.md` 保存本地前后记录：5,000 定义构图的 p50／p95 从 82.281／83.585 毫秒变为 10.629／12.263 毫秒，节点／关系数量与序列化字节数一致。两次测量使用共享工作树上的不同构建，不是同进程交替执行的配对实验。

## 2026-09-17：精确目标排序与图复用（工作树）

本节是尚未发布的源码工作。[Aider 仓库地图](https://aider.chat/docs/repomap.html) 提供图排序与有界上下文的参考；[SWE-Explore](https://arxiv.org/abs/2606.07297) 在固定行数预算下评估相关代码区域的排序；[Agent Retrieval Bench](https://arxiv.org/abs/2607.24882) 将上下文获取与补丁成功率分开评估。因此本轮分别检查排序、覆盖和开销，不宣称复现这些论文的数据集，也不宣称模型基准分数提升。

一个 14 符号星形调用图复现了问题：被多处调用的辅助函数排在查询明确指定的函数之前。现在精确目标明确优先于图中心性，其余排序保留已有分数、直接检索种子优先级、限定名和路径，并用规范符号 ID 稳定决胜。评审／报错帧中已经给出的文件仍在选择前排除。采用 [Rust 标准库选择算法](https://doc.rust-lang.org/std/primitive.slice.html#method.select_nth_unstable_by)，先划出前 K 项，再只排序这一小段，将选择工作从全量排序降为 O(n + k log k)。回归测试逐项对照同一修正后规则的全排序结果，覆盖空集合、同分、逆序及数量边界。

无须补充关系图时直接借用原始快照，仅真正补图时才创建可修改的图。上游指纹、新鲜度检查和截断报告均保留；指针一致性测试防止这条路径再次退化为全图复制。

复现命令为 `cargo test --release --locked --lib repo_rank_release_cost_comparison -- --ignored --nocapture`。基准预热 3 次，保留 31 次采样；候选缓冲区在计时外准备，交替执行两种选择算法并核对返回 ID 顺序。两种算法使用相同的修正后比较规则，隔离算法开销与排序质量变化。2026-09-17 的本地样本环境为 Rust 1.98.1、aarch64-apple-darwin：

| 候选数量；保留 16 项 | 全排序 p50／p95（微秒） | Top-K p50／p95（微秒） |
| --- | --- | --- |
| 128 | 34.833／35.625 | 15.750／16.709 |
| 6,000 | 1,306.583／2,205.125 | 299.875／546.083 |
| 12,000 | 2,066.041／2,219.583 | 471.917／505.417 |

另一个完整图样本包含 5,000 个节点：原复制开销 p50 为 1,230.417 微秒、p95 为 1,298.084 微秒；无须补图的路径返回原快照。样本按实际符号上限生成，并断言没有截断，不通过关闭必要恢复来制造免复制结果。超过运行时图上限的候选数量仅测试选择器，不承诺运行时交付同等数量符号。这些是本地阶段级测量，不是 Agent 端到端延迟、网络性能、内存分配统计、代码生成质量或 Kimi／Claude／Codex 排名；耗时不作为 CI 成败阈值。

## 2026-09-17：对照主流 Coding Agent 的代码读写优化（工作树）

本节是源码工作，不代表已经发布。需要用新构建启动运行时并刷新工具目录；当前连接中的旧进程可能仍声明精确子串搜索 Schema。

对比以能力为依据：[Kimi 官方工具](https://www.kimi.com/code/docs/en/kimi-code-cli/reference/tools.html) 提供基于 ripgrep 的正则搜索、输出模式、分页与陈旧读取保护；[Claude Code 子代理](https://code.claude.com/docs/en/sub-agents) 隔离探索上下文；[Codex 提示指南](https://developers.openai.com/cookbook/examples/gpt-5/codex_prompting_guide) 强调批量读取已知文件和明确的补丁接口。这些是官方工具／流程属性，不是同题、同预算实测的模型正确率或生成速度排名。

`search_code` 接受字符串或最多 32 项查询数组。单字符串默认自动模式：精确匹配没有命中时才采用 token-AND 回退；数组默认精确匹配。精确与回退候选在一次遍历中读取同一份文件字节，不再重扫。显式模式保留 `regex`、`tokens_all`、`tokens_any`。`search_many` 默认精确、`scan_patterns` 默认正则，不会静默对数组启用 auto。正则按行匹配并保留行首／行尾语义；本轮没有加入跨行正则或通用 glob 过滤。

搜索按匹配行去重，保留 `queries` 来源、原文件 `sha256`、逐模式匹配行数，以及独立的 `scan_truncated`、`results_truncated`、`failed_files`、`skipped_files`、`coverage_complete`。计数是命中行数，不是正则出现次数，也不是已证明的 Bug 数量。结果优先保留每个模式的代表样本，再按路径／行号补齐。`offset` 与 `next_offset` 分页读取当前文件，跨请求发生修改可能改变结果，因此不宣称仓库原子快照。预算用尽且没有可继续位置时，必须缩小范围，不能声称全仓已经排除问题。

`output_mode` 支持 `content`、`files_with_matches`、`count_matches`；后两种不返回源码正文，均保留路径、SHA 和匹配行数。`scan_patterns` 将重叠上下文合并到文件级 `context_lines`。搜索结果的 SHA 可以直接作为现有安全编辑工具的前置条件，减少为了取 SHA 再读文件的调用；这不免除判断修改所需的上下文检查。脱敏或裁剪正文不能当作完整原文使用。扫描沿用受保护路径与源码读取检查，并保留文件数量、字节量、正则编译、结果保留和响应预算。

多处编辑在校验所有原始范围后，一次拼接输出缓冲区，不再每替换一处就搬移后续正文。唯一锚点、原始行范围、重叠拒绝、输出尺寸与陈旧 SHA 拒绝均保留。AST 搜索也附带源码 SHA，并显式报告提前结束与读取失败，避免伪报完整覆盖。

可通过 `cargo test --release --locked --lib competitive_io_benchmark -- --ignored --nocapture` 复现本地基准，需要已安装 `rg`。基准使用 768 文件、三个模式，与 ripgrep 实际返回的路径／行号集合逐项对照；比较批量与逐条查询，记录正文／仅文件结果的 JSON 字节数，并在同一份约 1 MiB 文档上比较 128 处编辑。搜索采样 15 次、编辑采样 31 次，交替执行顺序，报告暖缓存中位数与 p95。普通测试套件默认忽略这个环境相关基准，显式运行才构成独立的实测记录。它不测模型推理、远程 MCP 延迟或任务通过率；两者的安全检查和输出格式不同，因此原生 ripgrep 用时只是参照，不是完全相同工作量的比较。

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

## 2026-09-15 多语言质量追加调研

这一轮不再把 Rust 习惯硬套给所有生态，而是逐项核对当前官方契约。Dart 官方把 `dart analyze` 定义为 Static Analysis，而同一 Analyzer 也负责 Lint 与 Type-system Diagnostic，因此一次真实 Analyzer Run 可以诚实覆盖 Lint／Type／Static，而不应该重复执行三次。Prettier 明确把 `--check` 定义成不写源码的 CI Check，`--write` 才是修改模式；Vitest 的 `vitest run` 是单次非 Watch；Jest 的 `--runInBand` 是有界串行 Test Run。Biome 有独立 Formatter/Linter Switch，因此 Registry 使用不同 Format/Lint Check，不再把同一条宽泛 `biome check` 跑两遍。Standard Ruby 默认 `standardrb` 只报告问题，修复需要 `--fix`；R Startup 官方文档说明默认可能加载 `.Renviron` / `.Rprofile`，而 styler 的 `dry="fail"` 明确不写文件，因此内置 R Quality/LSP Command 统一使用 `--vanilla`，R Formatter Gate 使用 styler Dry-fail。

主要资料：[Dart analyze](https://dart.dev/tools/dart-analyze)、[Dart Analysis/Lints](https://dart.dev/tools/analysis)、[Prettier CLI](https://prettier.io/docs/cli)、[Vitest CLI](https://vitest.dev/guide/cli)、[Jest CLI](https://jestjs.io/docs/30.0/cli)、[Biome CLI](https://biomejs.dev/reference/cli/)、[Standard Ruby](https://github.com/standardrb/standard)、[R Startup](https://www.stat.ethz.ch/R-manual/R-devel/library/base/html/Startup.html) 与 [styler `style_pkg`](https://styler.r-lib.org/reference/style_pkg.html)。

同一轮还收紧 Advanced Stage 的真实性：内置 Property Discovery 现在要求框架声明 + 对应语言源码真实使用；JS/TS fast-check 使用固定 Vitest/Jest Runner，任意 `test`、`mutation`、`mutate` Package Script 都不能生成 Advanced Evidence。由于 JS/TS Stryker 的 Repository Config 本身可以执行 JavaScript，它保持显式 Executor 配置。这里刻意选择“真实 Gap”，而不是宽泛但不可证明的绿色 Coverage。

## 诊断上下文

2026-09-17 的源码更新识别定位点周围的明确中文标点，例如 `修复：src/worker.rs:120，检查：src/model.rs#L9`。诊断片段与直接符号片段统一保留原始 UTF-8 前缀和实际返回行号；截断通过元数据表达，不再插入省略号。未脱敏的 `read_file` 片段保留内部 LF、CRLF 与混合换行，仍沿用省略最后一个行终止符的既有约定。脱敏内容继续携带 `redacted: true`，不得当作原始字节直接编辑。

即使预算很小，保留的片段仍包含路径、已有符号 ID、SHA、精度来源、行号和脱敏状态。可编辑状态要求非空、未脱敏的正文与直接目标及可写文件 SHA 对应；无关或陈旧正文不能单独让目标变成 ready。多目标只交付部分正文时明确给出提醒，不宣称覆盖全部目标。这些改进不授予权限、不证明报错根因、不自动修复测试失败，也不代替最终验证。

使用 `cargo test --locked --lib diagnostic_context_ -- --nocapture` 复现确定性回归：包括标点与路径边界、小预算元数据、脱敏、LF/CRLF/混合换行/EOF，以及把返回的诊断片段直接用于 SHA 保护编辑后拒绝旧 SHA。这是本地正确性测试，不是模型对跑或端到端延迟测量。

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

## 输入绑定缓存与可信刷新基线

2026-09-15 的本地后续迭代参考了 [Anthropic 基于评测的工具设计](https://www.anthropic.com/engineering/writing-tools-for-agents)，以及 2026-06-08 提交的 [Less Context, Better Agents](https://arxiv.org/html/2606.10209v1)。后者研究企业报销流程，不是仓库修复；不能因此给所有编程模型强制套用五次调用的窗口。wcode 保留精确源码与 SHA 上下文，先评测自身检索行为，再考虑模型专用策略。本次不宣称完成模型微调、远端模型评测或论文基准复现。

已验证经验的激活缓存现在绑定完整规范化历史，包括轨迹和检索意图，以及当前经过工作区保护的文件存在性快照。只比较记录数量与末条版本，无法发现早期记录被替换或文件被删除。缓存键与时序重放使用同一份准备好的快照；输入未变时复用缓存决策，相同输入的并发未命中只执行一次重放。存在性检查改用 Workspace 元数据，不再仅为丢弃 SHA 而读取整份源码。元数据不是验证证据，真实编辑和源码 SHA 检查保持独立。

浏览器只确认项目请求之前已观测到的版本，不使用独立迟到的响应为快照背书。手动刷新可以保留此前安全的基线；没有基线的首个快照立即显示，随后成功的版本轮询会保守地再构建一次。过期缓存响应清空基线。延迟重建绑定请求与工作区代次，隐藏页面不启动重建。[MDN 的取消文档](https://developer.mozilla.org/en-US/docs/Web/API/AbortController/abort) 说明了网络请求与响应体的取消范围；代次检查则进一步阻止已经完成的旧工作改写当前 UI。上述规则不代表获得了整个仓库的原子快照。

回归样例覆盖末条不变的历史改写、删除、目录和符号链接替换、文件恢复、轨迹与意图变化、缓存重放复用、六线程请求重叠、提前确认版本，以及过期和隐藏页面的刷新回调。`target/wcode-experience-membership.json` 对 96 条记录、十二个各 512 KiB 的文件执行五次本地对照，并断言准备好的路径一致。耗时仅代表受保护的文件存在性检查，不代表端到端智能体或模型性能。当前已运行的实例仍需使用新构建启动才能获得改动；本地验证不授权发布。

## 验证与限制

回归测试覆盖 1,000/1,400/4,000 Token 预算下的报错行保留、非代码文件、缺失与被拦截位置、可移植位置语法、SHA 变化、执行额度耗尽、编辑合并、无效预览参数以及原执行行为不变。测试使用本地样例，不是 SWE-bench、Agent Retrieval Bench 或模型质量评测。

先运行 `review_changes`，再运行 `verify_project(level="full")`。Rust 全量检查包含 `cargo clippy --locked --all-targets -- -D warnings`，让测试代码获得与 CI 相同的 Clippy 覆盖。应以实际测试报告和绑定版本的证据为准；本文本身不是某个构建通过的证明。

不宣称通用延迟、Token 成本或修复成功率提升。在代表性仓库上分别测量上下文命中、工具往返次数、负载大小和耗时后，才能给出提速数值。发布这些变更前仍需新运行时冒烟测试与跨平台 CI。
