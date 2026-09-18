# 论文证据与复现记录

日期：2026-09-18。工作区：`Code/Rust/wcode`。仓库操作通过 wcode connector 完成。本轮只新增论文文件。

## 1 规范报告

正文只使用下列一次诊断结果，不混用第一次专项测试的延迟或其他历史分数。

```text
JSON: target/engineering-fitness-1789665000441866000-37653.json
Markdown: target/engineering-fitness-1789665000441866000-37653.md
时间: 2026-09-18T02:10:00.441866+09:00
JSON 字节数: 597957
JSON SHA-256:
b117266f8c33cebcef26cb2fc9d56a5bf50b48f604d67ffab1944a16b4807ffc
Markdown SHA-256:
d6377ea365b839f0da80cb6bd058aee8ffdd53b1f9846ddd4f12dae8f1009c23
```

时间由报告文件名的纳秒时间戳换算。`snapshot.json` 是人工核对的精选字段摘录，不是上述原始 JSON 的副本。重新评分和逐样本分析需要完整原始报告；原报告留在仓库 target，未在附件中伪装成完整制品。

## 2 环境与指纹

| 字段 | 报告值 |
| --- | --- |
| wcode_version | 0.7.5 |
| git_head | 181ec689f1db21840038ab2fe179dd3f567ba899 |
| worktree_dirty | true |
| profile | debug |
| os / arch | macos / aarch64 |
| rustc | rustc 1.98.1 (48a229cea 2026-09-01) |
| available_parallelism / harness_slots | 10 / 4 |
| contract_version / JSON schema_version | 3 / 2 |
| case_count / samples_per_cell | 60 / 1 |
| completed_query_attempts | 360 |
| model_calls / query_errors / warmup_errors | 0 / 0 / 0 |
| source_stable_during_run | true |

```text
corpus_sha256:
83cc3eb2b0ddeb2386f0ead9703f0e546688d0f0d9e8c171f9db6633a6c0461e

evaluator_sha256:
a0cf6c25d2fe982bc84994a89ccd18f913c2b90e7dda97dc3c51dcf3fe18c0ca

test_binary_sha256:
5a6ed3688d8b4ada0945462ee5783d5f3416b29e13acb5197df27295211b5d2c

source_snapshot_before == source_snapshot_after:
14d646e7ad0286482292b618047a07944935ada25f6c9850f699dab245ac91cd
```

源码快照覆盖实现声明的 src、tests、.wcode/design 及若干构建输入。前后相同不证明编译读取了完全相同输入，也不排除修改后恢复。二进制另有 SHA，但哈希不能恢复未提交源码。机器型号、OS 补丁、隔离负载与 CPU/RSS 未完整记录，不作硬件性能主张。

## 3 本轮实际执行

先执行：

```sh
cargo test --locked --lib engineering_fitness_ -- --nocapture --test-threads=1
```

结果：40 passed，0 failed，2 ignored，860 filtered out。测试 50.16s，编译阶段报告 36.82s。九项控制通过。两个 ignored 是需要显式调用的诊断快照与七重复试验，不能写为已运行。

随后执行：

```sh
cargo test --locked --lib engineering_fitness_diagnostic_snapshot -- --ignored --nocapture --test-threads=1
```

结果：1 passed，0 failed，907 filtered out。测试 24.60s，编译阶段报告 31.07s。此命令生成正文规范报告。

两次命令之间存在其他会话的源码修改与重新编译，过滤测试数不同；因此分别报告，不合并为同一冻结二进制或全项目测试通过。正文未使用第一次矩阵的延迟。

未执行：release 七重复试验、外部基线、机制消融、端到端模型实验、全项目 full verification 或发布流程。

## 4 分母与计算

每预算/阶段为 60 次尝试，59 个可回答任务，58 个可写且有必要证据任务，98 个必要身份。1K：85 身份、67 正文、97 当前 SHA，28 任务具备全部输入。2K/4K：98、98、98、58。查询错误与超预算响应均为零。

1K 自然语言子集 12 场景：身份 100%，正文 50%，输入 33.3%；影响子集 4 场景：100%、50%、0%；关系子集 8 场景：75%、50%、0%；缺陷相关子集 10 场景：55%、55%、10%。分项百分比来自原始 Markdown，主表精确字段来自命令打印的 JSON summary。

```text
cold 2K 相对 4K 序列化字节减少：
(12045.266666666666 - 7330.116666666667)
/ 12045.266666666666 = 0.39145252076887736
```

约 39.15%，不是实际模型 token 或 API 费用节省。预热 180 次不计入 360 次测量。

## 5 主张与源码定位

| 来源 | 路径或函数 | 支持范围 |
| --- | --- | --- |
| S1 | README.md；Cargo.toml | 系统定位、工作流、版本字段与操作边界 |
| S2 | docs/manual/engineering-fitness.md；设计 fitness requirement | 协议、开发语料与未测量维度 |
| S3 | src/runtime/harness/agent_context.rs：agent_context、truncate_source_body、finalize_agent_context | 并行组装、原文与预算约束，不是最优性证明 |
| S4 | fitness/corpus.rs、controls.rs、checks.rs 与专项反例 | Gold、控制及退化保护，不是隐藏留出 |
| S5 | fitness/scoring.rs：delivered、complete_body、ndcg、score | 独立计分，不是补丁正确率 |
| S6 | fitness/report.rs、delivery.rs、breakdown.rs | 分母、字节并集、缓存定义、指纹与分项 |
| S7 | 本文规范 JSON/Markdown 与命令回执 | 此次观察，不是干净提交的完整复现 |
| S8 | src/runtime/harness/quality/verification_run.rs；src/evidence/experience.rs | 验证与共变提示实现，未评独立收益 |

表内 fitness/ 指 tests/unit/runtime/harness/fitness/。并发开发会使行号和当前源码漂移，使用路径、函数名与实验指纹核对；不要假定当前文件必然等于历史编译输入。

## 6 复跑与正式制品

在所选 wcode 工作区运行第 3 节命令。新报告由 FITNESS_SNAPSHOT 给出，不覆盖原始报告。已有 release 入口如下，但本文没有执行：

```sh
cargo test --release --locked --lib engineering_fitness_trial -- --ignored --nocapture --test-threads=1
```

此入口每格七样本，共 2520 次测量及 180 次预热；重复不是独立任务。七样本最近秩 p95 等于最大值，不宜据此制定严肃的尾延迟阈值。

正式制品应冻结经审查的源码及未提交文件、依赖锁、评分器、构建记录与二进制、机器环境、原始报告和结果生成脚本。当前未导出其他会话所有修改，不自动向外部服务发布源码。

## 7 文献与写作声明

文献核实于原始 arXiv 页面与 MCP 官方规范。第 7 篇是 2026-09-16 更新的预印本，未称为同行评审论文。相关工作只比较问题与方法，不把不同基准或模型的成绩拼成排名。

本稿由 AI 辅助整理与撰写，实验数字来自实际命令。作者、单位、投稿格式和最终学术责任由维护者确认。model_calls=0 仅描述 Fitness 测量不调用模型 API，不代表开发或写作未使用 AI。
