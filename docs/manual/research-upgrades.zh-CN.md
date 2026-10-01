---
layout: docs
title: 论文驱动的改进
description: 论文与官方资料驱动的上下文检索、工具编排、模型 Host 效率、验证目标与实现边界。
lang: zh-CN
alternate: /docs/research-upgrades/
permalink: /zh/docs/research-upgrades/
---

# 论文驱动的改进

## 2026-09-28：按具体证据筛选关联测试（工作树）

筛选标准：原始论文或维护中的实现、有具体仓库任务评估或可检查代码、机制与 wcode 相关，并能写出可证伪的本地验收。这里不是跨模型排行榜，论文的成功率和成本不能直接套给 wcode。

| 来源 | 有用证据与限制 | wcode 的取舍 |
| --- | --- | --- |
| [SWE-agent，NeurIPS 2024](https://arxiv.org/abs/2405.15793) | 评估面向 Agent 的仓库导航、编辑、执行接口；结果依赖其模型、工具和任务。 | 保留有界、可编辑的接口，衡量 Agent 实际收到的内容，不再增加 Agent 循环。 |
| [Agentless，2024 修订论文](https://arxiv.org/abs/2407.01489) 与[官方实现](https://github.com/OpenAutoCoder/Agentless/blob/main/README_swebench.md) | 分层定位、修复、回归与复现验证提供了具体的简单基线；Python SWE-bench Lite 结果不能证明 Rust 或多语言效果。 | 定位到具体符号，同时保留故障复现与既有回归。 |
| [SWE-bench，ICLR 2024](https://arxiv.org/abs/2310.06770) | 真实 Issue/Patch 与可执行评估要求区分检索质量和问题解决率；原始语料为 Python。 | 本地合成上下文检查保持诊断性质，不冒充模型任务成功率。 |
| [SWE-ContextBench，2026 预印本](https://arxiv.org/abs/2602.08316) | 研究经验复用，但相关摘要与自主检索是不同条件；Oracle 提供相关经验不等于自动选择可靠。 | 保留经过验证、按意图筛选的经验，不存放无筛选的整段轨迹；迁移效果还需留出评测。 |
| [Aider 仓库地图实现](https://github.com/Aider-AI/aider/blob/main/aider/repomap.py) | Tree-sitter 与图排序提供预算内上下文；图中心性或处于测试文件中，不等于与任务相关。 | 复用现有图，收紧测试候选的具体证据要求。 |
| [mini-SWE-agent](https://github.com/SWE-agent/mini-swe-agent) | 小型 Agent 核心适合作为简单性基线；宣传分数随模型和评估设置变化。 | 本次不新增规划器、模型依赖或固定多 Agent 编排。 |

本次复现的问题是关联测试的候选准入，而不是缺少 PageRank：即使已经找到精确源码目标，任意测试路径中的符号仍可成为新的图遍历起点。初始夹具包含一个目标、一个真实关联测试和四十个无关测试；旧实现在 4,000 预算下返回十二项仓库地图，其中十项是无关测试，相关测试排在第九位。

修复保留没有精确目标时按测试路径探索的能力。已有精确目标时，候选需要有界图可达性、既有 Design/已验证经验，或落在该符号内部的目标文本命中。关联测试请求复用原有有界补充搜索，在同一次遍历收集命中行，检查符号范围与源码版本。因此宏断言和按目标命名的回归候选仍可召回，同文件的其他测试不再整体入选。纯文本候选明确标记 `test_target_text_match`，不生成调用边，也不证明执行或覆盖率。注释、字符串、同一行内的结构仍可能产生启发式命中；部分、脱敏、截断、过期观察不能证明覆盖完整。

后续反例在排序靠前的文档中放入 700 次目标词，旧的结果前缀因此漏掉后面的宏测试。内部上下文搜索现在于同一次扫描中，为有界范围内的匹配文件保留各目标词的首次命中：先保证查询词覆盖，再放入跨文件代表行，最后补充剩余前缀。沿用既有扫描、保留片段和响应字节上限；普通搜索排序与分页保持原样。中英、冷暖、单目标及双目标回归均能找回后面的测试，同时排除无关测试。同文件的其他命中或过多匹配文件仍可能被省略，因此始终是明确标记截断的样本，不是穷尽测试发现。

进一步审查复现了三个边界缺陷：JSX/TSX 的 `.test`/`.spec` 文件丢失纯文本候选；解析器与查询计划允许八个明确目标，但精确符号发现提前丢弃第四个之后的目标；补充扫描只查询四个目标时，没有标明其余目标被省略。现已在原有八查询上限内保留精确目标，统一 JSX/TSX 测试路径规则，并让补充扫描的目标省略保持可见。冷暖回归覆盖四、五、八个目标以及四种 JSX/TSX 文件名。补充扫描仍最多查询四个目标，不宣称对八个目标的关联测试全部召回。

验证命令：`cargo test --locked --lib context_search_`、`cargo test --locked code_to_test_ -- --nocapture`、既有 repo-map/fitness 回归，以及全仓门禁。测试覆盖中英查询、冷暖缓存、宏/命名候选、同文件噪声、过期/脱敏/无效搜索行、Design/经验锚点和无精确目标的探索。搜索回归还覆盖稀有目标、结果去重、每文件仅扫描读取一次、公共分页不变、原始 SHA、脱敏、长行裁剪、无法读取和过大文件。测试定义不等于已经执行，实际结果以当前运行记录为准。

下一项可测工作：冻结跨仓库留出的 Issue 集，基线与候选使用同一模型、预算和环境，分别报告补丁成功率、首个正确文件、交付的无关上下文、工具/读取成本与延迟。当前合成夹具不能证明整体百分比提升。在该比较暴露具体缺口前，不继续堆叠检索引擎和记忆机制。隧道、资源预算、授权及发布行为保持原样。


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

搜索按匹配行去重，保留 `queries` 来源、原文件 `sha256`、逐模式匹配行数，以及独立的 `scan_truncated`、`results_truncated`、`failed_files`、`skipped_files`、`coverage_complete`。计数是命中行数，不是正则出现次数，也不是已证明的 Bug 数量。结果优先保留每个模式的代表样本，再按路径／行号补齐。`offset` 与 `next_offset` 分页读取当前文件；`auto_page=true` 会把单次有界结果预算提升到 2,000，普通调用仍保持较小的省流量页。跨请求发生修改可能改变结果，因此不宣称仓库原子快照。预算用尽且没有可继续位置时，必须缩小范围，不能声称全仓已经排除问题。

`search_syntax` 的仓库级默认扫描上限从原来的 1,000 文件提升到 50,000 文件；默认跳过 Comment AST Node，`include_comments=true` 可恢复。Go 语法命中还会输出守卫证据和有界 Bug Pattern 信号。`scan_patterns` 保留文本正则模式，并默认跳过纯注释行／注释块；同时支持 `preset=go_common_bugs` 或单个 `pattern`，用 AST 验证 `nil_deref`、`err_swallowed`、`index_mismatch`、`empty_test`、`unguarded_subscript`。Pattern 会在占用结果预算之前过滤；只要 `coverage_complete=false`，调用方仍不能宣称全仓不存在该问题。

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

Agent Context 在最终定容前也会去掉可由完整当前源码重建的 repo-map `signature`，但只限同一精确符号已有完整、未脱敏源码，且源码 SHA 与已交付文件 SHA 一致。这里只去重元数据，不删除 repo-map 行、排序原因或关系；陈旧、脱敏、截断或身份不匹配的条目继续保留。整条无关系 repo-map 行仍只会在真正预算压力下经过既有安全裁剪器让位。

紧预算下缩短源码体之前，压缩器会把恢复符号原始行号所增加的序列化字节也计入缩减额度。因此补回跟进读取所需的坐标不会造成负向压缩进度；SHA、原始坐标和截断标记仍然保留。

MCP `tools/list` 目录不再重复输出由规范工具 `name` 机械派生的显示 `title`。工具名、精简描述、输入 Schema、annotations 与 Product Scope 元数据保持不变，完整且与任务无关的协议目录仍然可用。这里减少的是真实序列化 Host 目录字节，不冒充 Host 只加载任务 manifest；Provider/模型输入、编排上下文和失败重试的 Token 仍是独立的 E2E 缺测。

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

明确、固定形式的 Git 状态和差异检查使用独立检查队列。其他命令和仓库执行器进入按内存与 CPU 推导的重型进程队列；Balanced 的 512 MiB 配置当前提供 4 个重型进程名额。重型命令执行准入另外限制为重型进程容量，因此超出的命令会先在 EXEC 门口等待，不占 Global Tool Permit；32 个外层 Tool 槽位不会再先放 28 个命令进来，然后全部挤在 4 个进程名额后面。命令策略、用户批准、进程监管和资源压力准入仍执行；更换队列不能批准修改或辅助程序选项。

资源信息提供 `child_queue`、`probe_queue`，包含 `active`、`limit`、`waiting`、累计 `waits`、`total_wait_ms`、生命周期 `max_wait_ms`，并额外记录最近一次等待及其距今时间。占用表示已保留的名额，不表示 CPU 正在运行这些进程；取消等待会清理计数。Runtime Drift 只使用最近且超过 5 秒准入上限的等待，生命周期峰值仅保留为描述性历史。宽屏 TUI 展示 `PROC`、`GIT` 和内层合计 `Q`，与工具槽位及峰值分开。不增加授权开关，不自动重启实例。

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

## AI 变更验收商业架构（2026-09-30）

这是经过审计的目标设计和实施计划，不代表商业工作流已经交付。源码基线为 `ce698df`，另有保留的未提交源码检查、TUI JobView 和 Web Jobs 骨架。已通过的 `c7a3d3f` CI 与审计只证明此前那次提交。本计划不授权发版、打 tag、计费或托管部署。

### A. 当前能力地图

成熟度定义：**完整**表示所述有界契约已经实现；**部分**表示已有可用实现，但仍有实质缺口；**底层缺入口**表示引擎已存在，但缺少所需产品入口；**未实现**表示审计中未找到实现；**不应该实现**表示不属于本产品的目标范围。下表所有成熟度都描述审计基线。

| 能力 | 当前实现 | 成熟度 | 可复用组件 | 缺口 | 商业重要性 |
| --- | --- | --- | --- | --- | --- |
| 工程控制平面定位 | README、Product State、仓库智能手册 | 部分 | README / ProjectDesign / Product Scopes | 以 AI 变更验收为主线，同时保持 Agent 中立 | 核心 |
| 理解 → 修改 → 检查 | MCP 工作流 prompts、agent_context、受保护写入、变更审查 | 完整 | ToolHarness / Workspace | 仅完成这些操作，还不会产生 Change Acceptance Record | 核心 |
| 产品责任归属 | 12 个 Product Scopes、源码／测试归属及门禁 | 完整 | scopes / convention_status | 复用现有 Scopes，不另建一套验收归属分类 | 核心 |
| Design State | 需求、组件、约束、ADR、AcceptanceCriterion | 完整 | DesignState / AcceptanceCriterion | 验收条件的 verification 引用只是映射，不是已执行证明 | 核心 |
| Software Graph | 语法图、快照、有界影响遍历 | 完整 | CodeIndex / SoftwareGraphSnapshot | 保留语法／Provider 精度和部分覆盖状态 | 核心 |
| 语义导航 | 可选 Provider 能力、来源与新鲜度 | 部分 | semantic_navigation / graph_provider_store | 可用性取决于已安装 Provider，不能推断语义确定性 | 高 |
| Traceability | 需求／组件／实现／AC 映射 | 完整 | RequirementTrace / TraceResolutionSnapshot | 结构覆盖与执行覆盖必须分开 | 核心 |
| 风险 | 绑定版本的启发式风险档案与升级策略 | 部分 | Risk / VerificationProfile | 风险是建议性上下文，不能消除确定性失败 | 核心 |
| 变更检查 | 暂存／未暂存／未跟踪 diff、文件／符号／影响／源码 | 完整 | ChangeReviewReport / worktree status | 有界且感知快照；新产生的未提交编辑仍需最终门禁 | 核心 |
| 验证发现 | ProjectContext、CheckSpec、语言／语言岛／原生检查发现 | 完整 | ProjectProfile / CheckSpec | 策略必须选择精确的必需检查 ID 与执行级别 | 核心 |
| 验证执行 | verify_project、有界无 shell 执行器、失败／跳过／复用报告 | 完整 | ToolHarness::verify_project / VerificationReport | 核心执行器保留跳过与失败信息，并检测版本变化 | 核心 |
| 精确 AC 测试执行 | analysis 映射与生成的 AC Evidence | 部分 | VerificationRef / execution receipt | 当前通用测试检查可能暗示其他映射测试也已运行 | 关键 |
| 计划强度强制执行 | VerificationPlan、VerificationStatus、执行下限 | 部分 | VerificationPlan / Execution floor | 汇总 quick Pass 不能证明完整计划的每项检查都已执行 | 关键 |
| 阶段自动化 | Property／Mutation／Fuzz 适配器与阶段目标 | 部分 | StageExecutorRegistry / stage targets | 缺少工具时仍是自动化缺口；外部报告需要可信执行回执 | 高 |
| Evidence 账本 | 版本、类型、producer、置信度、目标、有界持久化 | 部分 | Evidence / evidence_store | producer 字符串／摘要不是经过认证的执行身份 | 关键 |
| Evidence 新鲜度 | Code + Design hash、有效／当前聚合 | 完整 | Revision / latest_current | 验收封装还需绑定 Git head／base／tree 与策略 | 关键 |
| 人工批准 | verification_approve 与冻结的协调快照 | 部分 | AuthorizationManager / plan digest | 调用者提供的 confirmed／approver 不能认证真人 | 关键 |
| 独立审查 | 角色任务、领取、审查提交、分歧 | 部分 | VerificationJob / ReviewerRole | 客户端名称可伪造；blind 标志没有结果访问隔离 | 高 |
| Reconciliation | 期望状态 drift、冻结计划、执行与就绪门禁 | 部分 | ReconciliationPlan / ApprovedPlanSnapshot | 先强化权限与覆盖，再复用门禁 | 核心 |
| 本地发布门禁 | Design、Scope、Convention 与结构覆盖检查 | 完整 | release_gate / Scope / Convention | 不是客户合并门禁；映射覆盖不能证明测试已运行 | 高 |
| Change Acceptance Record | 现有 Evidence／Status／Review 输入 | 未实现 | VerificationStatus / Evidence / Risk | 缺少统一、不可变、可解释且绑定版本的决策模型 | 关键 |
| 项目验收策略 | Design acceptance、风险档案、检查引用、运行时策略 | 底层缺入口 | ProjectDesign / AcceptanceCriterion / Risk | 版本化的确定性选择、可信策略基线与例外规则 | 关键 |
| GitHub 外部合并门禁 | 自身 CI、有界 GitHub 读取与仓库工作流 | 未实现 | Git reader / existing CI | 绑定提交的发布器、PR 事件、必需 App Check 与过期处理 | 关键 |
| 其他 Git Provider | Provider 中立的核心 | 未实现 | Provider-neutral core | 现在定义适配器契约，之后再做 GitLab／Bitbucket | 后续 |
| 团队组织与项目 | Workspace 注册表 | 未实现 | Workspaces registry | 组织／成员／项目身份与有范围的授权 | 高 |
| 高风险团队角色 | 命令／风险执行／删除授权 | 底层缺入口 | AuthorizationManager | 把 Owner／Admin／Developer／Reviewer／Viewer 映射到真实操作 | 高 |
| 工程日志 | 有界、以追加为主的里程碑 JSON | 部分 | Engineering milestones / journal | 缺少已验证 actor、策略／例外事件、链／检查点和审计导出 | 高 |
| OAuth 连接 | Access／Refresh grants、客户端绑定、Refresh 轮换 | 部分 | AuthState / PublicEndpoints / auth tokens | 没有时间到期／撤销／会话 UI；Refresh 后旧 Access 仍有效 | 关键 |
| Workspace 访问 | 根目录／路径保护与只读强制执行 | 完整 | Workspace / Workspaces / fs_safety | 单一操作者选择根目录；OAuth 客户端没有按根目录的主体 ACL | 关键 |
| 命令边界 | 直接 argv、策略形态、授权、进程限制 | 完整 | CommandResult / run_command | 普通仓库构建／脚本以宿主用户运行，不是 OS 租户隔离 | 关键 |
| Full Access／宽权限沙箱 | macOS Seatbelt／Linux bubblewrap 的宽权限命令通道 | 部分 | WorkspaceSecurity / authorization | 不隔离普通构建／LSP 或共享权限状态 | 关键 |
| 受保护路径／脱敏 | fs_safety、环境／内容／Header 脱敏 | 部分 | fs_safety / redaction | 运行时权限状态需要保护；脱敏不是写入隔离 | 关键 |
| 远程 MCP／隧道 | OAuth、端点来源、本地探测与 Provider 恢复 | 部分 | AppState / AuthState / tunnel health | 当前详细匿名健康信息会披露根目录／启动配置 | 关键 |
| LSP 执行 | 能力探测、有界子进程、清理后的环境 | 完整 | semantic providers / command execution | 已安装 LSP 进程仍保有宿主用户访问权 | 高 |
| Evidence 存储隔离 | 用户状态、权限、有界记录 | 部分 | workspace_state_directory / Evidence | 同用户脚本可修改本地权限状态，不防篡改 | 关键 |
| 工程观测台 | Attention、变更／源码桥、Evidence 检查器、图谱 | 部分 | ProjectObservatory / ProjectAttentionView | 默认架构页，没有统一的当前变更决定与操作 | 核心 |
| Acceptance → 文件 → 符号 | 类型化 AC path／symbol／provider 引用与源码桥 | 底层缺入口 | FeatureAcceptanceView / source bridge | AC 行不可操作；全程保留所选变更／版本 | 核心 |
| TUI | Attention／Proof／Agents／Provider 视图与 Web 跳转 | 部分 | TaskMonitor / console | 没有统一验收摘要；JobView 适配器尚未接入 | 高 |
| Web Jobs | 空容器与 State／CSS 骨架 | 未实现 | TaskRuntime / TaskRecord | 没有后端／API／模块，不是已交付任务台 | 延后 |
| Setup 连接 | 配置预览／合并、Agent Setup Hub、受保护配置 | 完整 | Setup / agent_install | 公开 Setup 是连接指南，不是批准权限来源 | 核心 |
| 首次验收 onboarding | 原生发现与 Setup 基础能力 | 底层缺入口 | ProjectProfile / setup planning | Dry-run 建议 → 确认 → 首个绑定版本的结果 | 核心 |
| Agent 集成 | Provider 中立 MCP、插件／配置、implement／review／verify prompts | 部分 | MCP / agent_plugin / agent_install | 配置测试不代表各宿主版本 OAuth E2E；缺少验收入口 | 核心 |
| 多 Agent 工作 | Worklist CAS、有范围的领取、私有租约 Token、有界结果 | 完整 | Worklist / writer lease / task claim | Agent 由 Host 创建；工作报告不会成为验证 Evidence | 高 |
| 上下文效率 | 有界上下文、渐进式 Schema、紧凑确认响应 | 完整 | Agent Context / tool manifest | 字节／4 估算不能证明实际计费 Token 节省或模型成功率 | 高 |
| 试点指标／导出 | Engineering Fitness 与里程碑基础能力 | 底层缺入口 | Engineering Fitness / journal | 验收计数、缺失／过期发现和实测耗时 | 高 |
| 产品方遥测 | 本地工程观测 | 未实现 | Local runtime observation | 可选的显式 Schema 与 opt-in 接收端，不默认上传源码 | 后续 |
| 发布流水线／测试 | 三平台 CI、原生浏览器、对抗性验证产物 | 完整 | GitHub Actions / release contracts | 每项结果只适用于其精确 SHA；当前未要求发布 | 核心 |
| 自建 Agent／IDE／聊天／计费／远程 shell | 验收不需要这些能力 | 不应该实现 | No component needed | 保留有用的 OSS 检查／编辑，停止扩张无关产品界面 | 无 |

审计来源：[验证运行时](../../src/intelligence/runtime/design.rs)、[analysis](../../src/intelligence/analysis.rs)、[验证协议](../../src/verification/mod.rs)、[Evidence](../../src/evidence/mod.rs)、[Evidence 存储](../../src/evidence/store.rs)、[日志](../../src/evidence/journal.rs)、[授权](../../src/workspace/authorization.rs)、[OAuth](../../src/integrations/auth/mod.rs)、[运行时](../../src/integrations/mcp/mod.rs)、[命令执行](../../src/workspace/operations/execution.rs)、[Setup](../../src/app/setup.rs)、[发布门禁](../../src/intelligence/release_gate.rs)，以及当前 Design／Worklist／工具报告。发现来自源码调用链观察，不代表已经完成漏洞利用测试。

### B. 目标产品架构

首个商业场景是 **AI Change Acceptance（AI 变更验收）**。目标结果是让 AI 生成的代码具备有证据支持的交付条件。现有编码 Agent 始终可替换。产品闭环为 **理解 → 修改 → 检查 → 验证 → 证据 → 接受／阻止**。

| 层 | 职责 | 权限与权威边界 |
| --- | --- | --- |
| 现有 Apache-2.0 OSS 核心 | Workspace、Design、Graph、Risk、受保护变更、真实验证、Evidence、Reconciliation | 保留现有边界、保护和确定性失败优先规则 |
| OSS 验收层 | 强化现有 VerificationStatus；确定性策略选择；Change Acceptance 投影／历史／导出 | 基于现有计划与证据的唯一规范引擎，不另建执行器或前端放行算法 |
| 团队层 | 已验证 actor、组织／项目成员、策略变更、人工决定、共享历史／审计 | 项目范围的权限；策略要求时明确分离作者与审查者 |
| 集成层 | 可信执行回执、GitHub App 发布器、PR 版本同步 | Provider 适配器不能削弱核心决定；凭据留在不可信工作进程之外 |
| 可选托管层 | 共享协调、运行管理、身份联合与可选指标 | 显式部署／数据契约；实用的本地 OSS 验收不依赖它 |

需求视图中的 `stable` 表示结构收敛，不表示 Accepted。`passed/fresh` 汇总计数可能包含历史记录，不能用于决定当前验收。Reconciliation 与 Execution 继续使用同一个经过强化的验证决定。

策略是对现有项目 Design acceptance、verification 引用和风险约束的版本化扩展，引用已有组件／需求／AC／检查身份。不引入任意可执行策略 hooks 或第二套需求数据库。客户策略从可信、已批准的基线选择，不能被正在评估的 PR 悄悄削弱。

### C. 数据模型与确定性决策契约

以下是目标契约，不是当前可用 API。

| 模型 | 复用／新增 | 必需内容与不变量 |
| --- | --- | --- |
| Acceptance | 在现有 ChangeReviewReport、VerificationStatus、Risk、Evidence 上新增封装 | Schema／ID／Project／Workspace／Repository，base／head／tree，未提交代码 hash + Design hash，策略版本／摘要，捕获时间，已验证 actor／producer／可选 Agent 身份，有界变更／影响／精度，验证项、Evidence 引用、人工决定与最终决定 |
| Policy | 扩展项目 Design State | Version／ID／digest，针对路径／组件／需求／AC 的选择器，精确必需检查／阶段／级别，人工审查／风险规则，显式 docs-only 规则，允许的例外，不可变核心约束；排序后的确定性匹配 |
| Actor | 新增经过认证的权限类型 | ID、kind（operator／agent／integration／system）、认证来源、组织／项目权限、可选的已验证 Host 身份；producer 展示文字始终不构成权限 |
| Organization | 新增团队模型 | ID、成员／角色、项目以及策略／集成归属；继续支持本地单一操作者 |
| Project | 给 Workspace 身份扩展团队绑定 | 稳定 ID、规范仓库身份、根目录、可信策略来源、部署模式、成员与集成绑定；别名不授予跨项目访问 |
| Audit Event | 复用日志存储机制，扩展类型化审计流 | 事件 ID／时间、actor、project、精确版本、操作／结果、Evidence／Acceptance／Policy 引用、例外原因、前一事件／本事件摘要和导出检查点；显式保留缺口 |
| Integration | 新增 Provider 适配器模型 | Provider／Installation／Repository 绑定、能力范围、凭据引用、到期／撤销状态、可信回执权限、投递／幂等状态；记录中不保存原始密钥 |

每个验证项区分互相独立的事实：
- **选择：** required、discovered、mapped，以及映射精度／Provider。
- **执行：** not_executed、running、completed、skipped、unavailable；检查 ID、执行器／命令回执、范围、开始／结束时间和退出状态。
- **结果：** pass、fail、inconclusive、unknown。命令执行完成不自动等于测试通过。
- **新鲜度：** current、stale、unbound；Code、Design、Git 目标与策略关系。
- **权限来源：** internal executor、authenticated integration、operator、self_reported、legacy_unknown。

测试路径／符号映射不是执行回执。输出尾部和通用 `cargo test` 检查名不能证明某个测试运行过，尤其当测试被筛选、忽略、跳过，或者来自另一语言岛时。命令级成功可以满足显式的命令级要求；针对具体测试的要求需要精确的执行器覆盖。

Evidence 保留 producer、kind／type、revision、scope／targets、timestamp、freshness、source／artifact digest、precision／confidence、verification／check 关联，以及经过认证的回执权限来源。缺少权限来源的历史记录保持为 `legacy_unknown`，迁移不能升级其权威性。Evidence 数量限制／截断保持显式。

人工决定为 approved、rejected、needs_review、exception_approved，与测试结果分开。操作者的一次性授权绑定 Workspace／服务器实例、计划摘要、Code + Design 版本、策略、决定和陈述摘要，并具备到期与防重放机制。仅有 MCP 客户端确认和 OAuth 客户端身份，不能证明真人参与。Full Access 永远不授予 HumanDecision 权限。例外必须写明豁免的策略规则与理由；不能把失败测试变成通过，也不能豁免核心身份／版本／授权约束。

最终决定包含 `status`、`blocking_reasons`、`warnings`、`required_actions`、`evidence_summary`、`verification_summary`、`risk_summary` 与完整版本身份。即使某个原因决定主状态，其他原因仍全部保留：
1. 候选版本／策略不匹配 → **stale**。
2. 当前确定性失败、必需审查被拒绝或违反核心约束 → **blocked**。
3. 必需执行／回执／发现不可用，或身份／覆盖不完整 → **incomplete**。
4. 确定性要求已满足，但缺少经过认证的人工／独立审查 → **needs_review**。
5. 所有必需条件均由当前、具备权威来源的输入满足 → **ready**。

引擎在评估前捕获输入，并在持久化或发布前重新检查。即使源码字节相同，Git 提交变化也使旧 Acceptance 失效。本地未提交变更的验收绑定内容 hash，不能授权一个尚未提交的 GitHub SHA。有界扫描不完整时，不能因为遗漏而产生 ready。Evidence 读取的损坏、超限或缺失必须显式报告；最新 Verification 快照不可因读取失败而静默回退到旧批准状态。CAR 评估需要完整性来源，不能把被丢弃的记录解释成没有失败。

### D. 威胁模型

| 威胁 | 当前问题 | 必需缓解措施／失败行为 |
| --- | --- | --- |
| Agent 绕过 | Prompt／Skill 遵循只是建议 | 必需的外部 Git Check；核心决定保持权威性 |
| 过期版本 | 只有代码 hash 不能识别 Git head | Base／head／tree + Code／Design／Policy 绑定，前后检查与过期记录 |
| 伪造批准 | confirmed=true 与任意 approver | 操作者签发的一次性授权、经过认证的 actor、拒绝 MCP 自我批准 |
| 伪造验证 | 任意 producer／verdict／digest／targets | 内部或经过认证的回执、精确检查／测试覆盖；自报结果仅供参考 |
| 隐藏确定性失败 | 同 producer 后续 Pass 可替换 Fail | 认证 producer 与回执；只有真实可信的重新执行可替代旧结果 |
| 伪独立审查 | 同一客户端领取多个名称／角色 | 不透明且绑定所有者的领取、结果可见性规则、策略职责分离 |
| 策略降级 | Agent 修改被评估变更中的策略／工作流 | 已批准的基线策略摘要；策略变更需要授权审查 |
| 集成被攻破 | 发布器凭据或重放回调 | 最小权限 App、已验证投递、Repository／Project／SHA 绑定、幂等与撤销 |
| 跨项目访问 | OAuth 客户端当前可访问暴露的根目录 | 已验证主体 → 项目 ACL；不能把当前根目录注册表宣传为团队 RBAC |
| 凭据泄露 | 详细公开健康信息、日志／状态／构建环境 | 最小公开探测、经过认证的诊断、脱敏与受保护凭据引用 |
| 命令逃逸 | 构建／LSP 以宿主用户执行 | 如实描述本地信任模型；共享部署使用独立 Worker UID／容器／VM |
| Evidence 篡改 | 同用户本地状态与未签名 JSON | 独立权限存储、可信回执校验、链式／带检查点导出；损坏时 incomplete |
| 集成中断 | Check 发布丢失或 head 查询过期 | 门禁维持 pending／failing，展示重试操作；不回退为成功 |
| 重启／并发 | 授权到期、head 改变或记录写入中断 | 原子写入、一次性／重启失效、有界队列和恢复测试 |
| 共享部署 | 一个进程／OS 用户共享根目录与权限状态 | 在具备真实项目 ACL + Worker 隔离前，每个运行时只承载一个租户／操作者边界 |

Workspace 路径隔离不是 OS 沙箱。当前宽权限命令沙箱不隔离普通构建或 LSP 进程。本地模式假设 OS 账号和仓库工具链可信。团队托管运行时需要显式租户／项目身份与隔离 Worker；专属部署使用自己的权限状态和凭据。

不能宣称“源码永不离开设备”：通过 MCP 返回的源码可能进入所选 Host／模型；配置的远程 MCP／隧道客户端可以收到源码；可选 Provider 与 Git／CI 集成有各自的数据流。Evidence／导出可能包含源码派生的路径、摘要或输出。应记录数据目的地和用户配置的保留策略。

审计采用追加机制，通过链式摘要和可信检查点提供实际的篡改可察觉能力，不属于法律意义上的不可变。同用户攻击者可以重写没有外部锚点的链。有界保留／导出必须披露历史缺失，不能默默宣称完整。

### E. UX 与外部合并流程

1. **Setup：** `wcode setup` 检测仓库／语言／原生测试／CI／Git Provider／Agent，展示 dry-run 策略建议与数据目的地，仅应用已确认配置。
2. **Change：** 开发者使用现有编码 Agent。wcode 选择明确的 base 与候选版本，展示未提交／干净状态和部分覆盖。
3. **Acceptance：** 认证后的首页打开 Current Change 和一个决定：状态、必需项的 passed／missing／skipped／stale、风险、原因与下一步操作。
4. **Blocked／Inspect：** 选择阻塞原因 → 关联 AC／检查 → Evidence 或缺少的回执 → 变更文件 → 符号，全程保留 Project／Revision／Breadcrumb。
5. **Verify：** 通过现有有界执行器运行精确的必需验证，展示真实 running／completed／skipped／unavailable，不把映射当作运行。
6. **Human Review：** 操作者查看冻结的候选版本、策略和证据，批准／拒绝，或明确请求允许的例外。该操作不更新测试结果。
7. **Ready：** 条件满足时生成一条新的不可变记录，历史决定仍可检查。
8. **Merge：** Git 适配器验证实时 PR head 及可信策略／记录绑定，发布相应 Check，再由分支保护强制执行。

高级图谱、指标、执行与 Provider 详情继续通过渐进式披露提供。复用现有 Attention、Evidence 检查器与源码桥。不要增加通用图表或第二个命令终端来代替验收操作。

**GitHub P0 设计：** 可信 GitHub App 发布器在 PR Worker 之外持有 `checks:write`。Check 绑定 `head_sha` 和 Record／Policy 摘要。分支保护要求由预期 App 发布的指定 Check。只有内部 **ready** 才产生 success；blocked／incomplete／stale／needs_review 永不发布 neutral、skipped 或 success。缺少必需检查在内部判为失败，即使 GitHub 本身可以接受 neutral／skipped 结论。

处理 PR opened／reopened／synchronize、发布器重试与 head 变化。启用合并队列时，支持 `merge_group`，把它的合成合并 SHA 作为独立候选。必需门禁任务不使用路径／提交跳过过滤器，即使上游失败也运行，并明确检查失败／缺失的依赖。区分 PR head 与合成测试合并提交，不能把某一版本的回执改标为另一版本的证明。

不可信 PR 代码拿不到发布器凭据，也不能修改已批准策略或可信 wcode 二进制。高权限 `pull_request_target`／`workflow_run` 发布器不能 checkout 并执行 PR 代码。Worker 提供的产物在执行权限来源与版本绑定得到验证前都不可信；自行编写的 hash 或 Pass JSON 不够。本地同用户产物不能宣传为具有对抗安全性的托管回执。

一手资料：[受保护分支](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)、[必需检查行为](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks)、[Checks API](https://docs.github.com/en/rest/checks/runs)、[Actions 安全](https://docs.github.com/en/actions/reference/security/secure-use)。这些资料用于设计适配器；核心引擎保持 Git Provider 中立。

### F. 实施计划

每项任务都列出目的、责任 Scope、依赖、位置、验收条件、验证与实质风险。以下均为计划任务，列在这里不代表已完成。

**P0 — 真实的付费试点路径**

- **P0-A 权限来源强化。** 目的：阻止 Agent 自我批准与伪造外部证明。责任：verification／evidence／workspace／integrations。依赖：本次审计。位置：现有 MCP dispatch、AuthorizationManager、Verification／Evidence 运行时。验收：MCP 布尔值／producer 字符串不能生成具有权威性的人工或确定性证明；操作者授权必须精确、可到期、仅用一次。验证：冒名、all-command 绕过、重放、过期计划与重启失败场景。风险：现有宽松客户端需要显式迁移，不能默默丢弃其报告。
- **P0-B 精确验证覆盖。** 目的：区分 mapped／executed／skipped／failed。责任：verification／traceability。依赖：P0-A 权限来源语义。位置：analysis、Verification 运行时与定向测试。验收：通用测试 Pass 不能证明某个映射测试；quick 不能满足要求 full 的计划；必需检查失败／跳过时阻止推进。验证：筛选／忽略／跨语言／缺失／复用／fail-fast／full 级别场景。风险：保守处理的历史 Evidence 变为 incomplete，不能变为 ready。
- **P0-C 版本化策略与候选身份。** 目的：确定性选择要求。责任：design／verification／workspace。依赖：A+B。位置：Design State 扩展、现有检查发现与 Git 读取器。验收：策略摘要／版本、可信基线、docs-only／认证／支付规则、精确 base／head／tree／未提交状态绑定；部分扫描阻止推进。验证：规则排序、策略降级、相同内容的新 SHA 与并发编辑。风险：基线版本不明确时必须显式展示。
- **P0-D 规范 Acceptance Record。** 目的：解释为何该版本可以推进。责任：verification／evidence／reconciliation。依赖：C。位置：共享状态投影、有界存储／历史／导出。验收：状态／原因／操作／来源、不可变记录摘要与前后保护；所有消费者共用一个决定。验证：旧 Evidence／新版本、负面结果、截断／损坏与中断写入。风险：有界历史不能暗示完整生命周期审计。
- **P0-E 操作者验收 UX。** 目的：形成连续的决定到源码流程。责任：experience／integrations。依赖：D+A。位置：现有 TUI／WebUI、受保护的窄操作路由。验收：当前变更／状态／必需项失败，以及可操作的 Evidence／Check／File／Symbol 导航；经过认证的人工决定仍单独处理。验证：unknown／stale／跨 Workspace／迟到响应／权限／键盘／原生浏览器场景。风险：公开 Setup 与 OAuth Agent 身份都不是批准通道。
- **P0-F 引导式 Setup dry-run。** 目的：无需空 YAML 即可得到首条记录。责任：experience／runtime／integrations／design。依赖：C+D。位置：现有 Setup／发现／模板。验收：检测仓库／技术栈／CI／Provider／Agent，精确策略预览、显式确认、保留无关配置。验证：不写入的 dry-run、仓库歧义／工具不可用／只读与配置合并。风险：发现能力不代表所有 Host 已连接。
- **P0-G GitHub 外部门禁。** 目的：在 Agent 配合之外强制执行验收。责任：integrations／verification／workspace。依赖：A–D+F。位置：窄 Provider 适配器 + 可信发布器／Setup 契约。验收：必需 App Check 绑定精确 SHA，synchronize 使旧结果失效，只有 ready 才 success，不进行高权限的不可信 checkout，也不向不可信代码提供凭据。验证：过期 head、neutral／skipped、伪造产物、中断、重试、撤销凭据与策略降级。风险：必须配好安装／仓库权限与安全 Worker 边界。
- **P0-H 可部署性与最小审计。** 目的：解释试点信任／数据边界与决定。责任：workspace／evidence／integrations。依赖：A+D+G。位置：OAuth 生命周期、公开健康接口、受保护状态、类型化决定事件／导出与部署文档。验收：到期／撤销／会话可见性、最小匿名探测、有界授权、已验证 actor、版本／策略决定历史。验证：到期／撤销／轮换后的访问、容量饱和、团队模式跨根目录拒绝、损坏／恢复／导出缺口。风险：不能把同用户本地运行时称作多租户隔离。
- **P0-I 真实试点演示与验收。** 目的：展示可强制执行的商业价值。责任：verification／experience／integrations。依赖：A–H。位置：隔离的真实仓库／PR、真实原生验证与下载的 Check／审计记录。验收：现有 Agent 修改仓库 → 必需检查缺失 → 真实 PR 被阻止 → 真实验证执行 → 当前 Evidence → 新 Record → 真实 PR ready；保留前后两份记录与精确 SHA。验证：全量套件 + 过期／并发／绕过／授权／集成中断／重启的对抗场景。风险：不能 mock 核心决定，不能用历史 CI 代替，未观测到真实 Git 平台结果前不能宣称完成。

**P1 — 团队运行与商业验证**

- **P1-A 组织／角色。** 目的：共享责任。责任：workspace／integrations。依赖：P0-H。位置：主体／项目注册表与受保护操作者 API。验收：Owner／Admin 管理项目／集成；策略编辑与例外需要显式权限；Reviewer 可审查、Viewer 可检查、Developer 可验证；按项目 ACL。验证：禁止的角色操作／跨项目／自我审查。风险：不要对无害读取套用一刀切 RBAC。
- **P1-B 共享策略／历史／审计。** 目的：解释过去的放行／阻止决定。责任：design／evidence。依赖：P1-A+P0-D。位置：现有持久化／导出 + 链／检查点。验收：actor／policy／revision／exception 谱系、有界保留与显式缺口、导出校验。验证：篡改／损坏／缺失／检查点／重启。风险：不宣称法律意义上的不可变。
- **P1-C 审查者归属与可见性。** 目的：真实独立审查。责任：verification／integrations。依赖：P1-A+P0-A。位置：现有 claim／job／status 协议。验收：私有、绑定所有者的租约；不能冒充审查者；获得资格前过滤 blind 输出；作者／审查者分离。验证：伪造名称、窃取公开 ID、重复 actor、到期与未授权结果读取。风险：Host 身份必须经过验证。
- **P1-D 试点指标。** 目的：让客户比较有实测依据。责任：evidence／experience。依赖：P0-D+H。位置：有界聚合／导出。验收：acceptance／blocked／missing／stale／review／exception 计数，带分母／时间窗口的原始实测验证耗时与验收周期。验证：重复／幂等事件、历史缺失、不含源码负载。风险：不得编造节省工时／Token／线上缺陷数。
- **P1-E 运行管理与集成。** 目的：支持可靠续约。责任：integrations／runtime。依赖：P0-G+H。位置：凭据轮换／撤销、投递重试／健康、备份／导出／导入／支持手册。验收：安全可恢复的中断、有界队列、可见集成状态。验证：撤销密钥、长期中断、重试、恢复。风险：密钥处理留在 Agent Worker 之外。

**P2 — 试点之后的可选规模化**

- **P2-A 可选托管协调／企业认证。** 目的：减轻共享部署负担。责任：integrations／workspace。依赖：P1-A/B/E。位置：可选服务适配器与部署模型。验收：SSO／联合认证的已验证 actor、租户 Worker／状态隔离、显式数据契约。验证：租户逃逸／凭据范围／恢复。风险：OSS 不强制依赖云。
- **P2-B GitLab／Bitbucket／CI 适配器。** 目的：跨 Provider 复用核心验收。责任：integrations。依赖：已证明的 P0-G 适配器契约。位置：Provider 专用版本／发布器适配器。验收：相同的 SHA／Policy／Failure／Receipt 不变量。验证：Provider 过期／中断／分支策略绕过。风险：以实测客户需求决定优先级。
- **P2-C 保护隐私的可选遥测。** 目的：测量产品 onboarding／运行。责任：runtime／evidence。依赖：P1-D。位置：显式、可配置的事件接收端。验收：文档说明 opt-in 与禁用；仅记录 Setup／Repository／Agent／Acceptance／Block／Verification／Stale／Review／Exception／Gate 事件，不默认上传源码／文件／命令输出／Evidence。验证：负载白名单与禁用／离线运行。风险：聚合标识符仍可能敏感。

每个实施阶段都执行：`review_changes` → `verify_project quick` → 相关定向测试；P0 完成时运行全量与负面／对抗检查。实际外部门禁得到观测前，不向用户宣称 ready。

### 商业边界与当前 Worklist

Apache-2.0 本地核心继续实用，不施加人为商业限制。团队价值来自协调、治理、共享证据与运行便利。这里不规划 Stripe、CRM、云 IDE、自建模型、任意远程 shell、移动 App 或大型企业控制台。

保留未完成的源码检查／编辑器工作和 JobView／Web Jobs 骨架。它们是辅助 Inspect／Change 工作，不是商业流程已存在的证据。保留现有 Worklist 历史，追加商业 P0 依赖链，并延后不兼容的 IDE 扩展，不删除已有工作。发布任务仍因用户“不发版”的指令而阻塞。

审计完成意味着能力与缺口已分类。P0 完成需要上述真实演示。文档、计数器、语法映射、Agent 报告或过去干净的 CI，都不是新版本的验收 Evidence。

### 首个实施阶段记录

首个底座阶段强化现有 Verification、Evidence 与操作者边界：精确必需命令回执和最低验证级别、保守处理命名测试映射、缓存来源校验、完整版本／计划绑定的人工单次授权、仅作自报的 MCP 阶段报告、可到期／撤销的 OAuth 会话与受保护运行时状态。同时拒绝不完整／未绑定的版本身份、冲突 Evidence ID、损坏或超限的权威记录，以及会抹掉当前原生失败的危险回收。Verification 快照带单调持久化序号，代码／Design 版本仍单独绑定；迟到旧快照或最新同代冲突不能成为恢复后的决定。现有 TUI JobView 已接入真实持久化 MCP 命令任务，日志有界并脱敏，保留真实失败结果，取消操作绑定所有者和 Workspace。

这些改动复用 OSS 核心，验证结果在 Worklist 中绑定当前代码与 Design 版本记录。尚未实现 Change Acceptance Record、已批准的项目 Acceptance Policy、Git SHA／base／tree 身份、精确逐测试事件适配器、可信 CI 回执、已验证团队 actor、例外流程、外部门禁、团队部署隔离或审计谱系。任务观测属于 Inspect／Verify 辅助工作；规范 Acceptance UX 与 Web Jobs 仍待实现。P0 必须完整实现并观测上述真实 PR 流程才算完成。

### Commit 感知输入与 Policy 草案

下一阶段为原生项目验证与语言质量 Evidence 增加有界执行 Git 身份：私有 repository／Workspace 范围摘要、完整 HEAD／tree 对象 ID、index 指纹和 dirty 状态。静态复用和执行中合并都纳入此身份，并校验实际来源回执；执行期间观测到 commit 或 index 变化会拒绝发布 Evidence。缺少 Git 绑定的旧 Evidence 保留 unknown。现有 Code／Design 内容守卫仍必需：dirty 不是内容摘要，前后探测也不是原子文件系统快照。

既有 `review_changes` 默认保留当前工作树响应。显式传入 `base_revision` 才返回独立的基线改动元数据；`target_revision` 默认 `HEAD`，也可为 `worktree`。干净当前 commit 的检查保留重命名两侧路径和文件模式。未知、截断、拒绝、dirty commit 或非当前目标捕获均为 incomplete 并报告错误。其 `metadata_only` 来源不能批准基线或产生 Acceptance。

仓库发现现在报告有界完整性、问题数量和原因标签。指纹与解析共用本次捕获的输入字节；无效、不可读或超限输入不能静默成为完整检查清单。扫描不完整会给 quick／full 验证增加一个未满足的确定性要求，同时保留实际执行的独立检查。

`ProjectDesign` 增加可选、带版本的 typed Acceptance Policy 草案、摘要和确定性规则累积。显式 docs-only 选择要求完整 old／new 路径、已知普通且不可执行的文件模式以及允许的 Markdown 范围；Agent 指令和 Skill Markdown 保留默认要求。任意嵌入式 Markdown 仍需可信图谱提供 risk floor。未来激活前，必须从完整可信输入解析检查 ID 与组件／需求映射。

Evidence 区分 native verification、native stage、local operator、self-reported 和 legacy unknown 来源。通用 Agent 提交不能满足原生阶段或人工要求；自报 review 不能覆盖原生失败。解析草案、记录 Git 元数据或检查来源标签，都不代表激活 Policy。

本阶段定向与完整验证结果在 Worklist 中绑定当前代码与 Design 版本记录。已批准 Policy 激活、规范 Change Acceptance Record、Git 绑定的阶段／人工决定、可信外部 merge check、Team actor 和真实试点 PR 演示仍待完成。运行中的 MCP 进程需要明确升级／重启才能提供新编译行为；本地源码测试不能证明已部署。
