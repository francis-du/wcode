# Fast Coding Path：上下文对照记录

记录日期：2026-09-17。范围仅为 `Code/Rust/wcode` 的本地 Harness；不是 Kimi、Claude Code、Codex 的付费模型对跑，也不是原先六个业务 Case 的重测。

## 可复现命令

```text
cargo test --locked --lib fast_context_ -- --nocapture
cargo test --locked --lib hot_context_ -- --nocapture
cargo test --locked --lib fast_context_benchmark -- --ignored --nocapture
```

基准位于同目录 `fast_context.rs`，使用 36 个文件、4 个明确目标、3,000 个估算 token 的上下文上限。每种状态运行 12 个样本。先调用 `agent_context`；若尚缺某个完整目标正文，再实际调用 `symbol_context` 补齐，并累计所有响应的 JSON 字节。计数是本地 API 调用，不含 MCP 网络开销，不代表模型总工具调用或端到端写代码耗时。

`cold` 表示新建 Harness，**不表示清空操作系统文件缓存或全局语言配置**；`warm` 表示同一 Harness 已预热。计时不包括编译与 Harness 构造。p50 为排序后下标 n/2，p95 为最近秩法；12 个样本的 p95 等于最大样本。基准不以墙钟阈值阻断 CI。

## 本次观测

| 状态 / 指标 | 改动前 | 改动后 |
| --- | ---: | ---: |
| cold：首包完整目标正文 | 2/4（12 个样本一致） | 4/4（12 个样本一致） |
| warm：首包完整目标正文 | 2/4（12 个样本一致） | 4/4（12 个样本一致） |
| 补齐四个正文的本地调用数 | 3（所有样本） | 1（所有样本） |
| cold：累计响应字节，中位数 | 22,716 | 11,775 |
| warm：累计响应字节，中位数 | 22,213 | 11,567 |
| cold：p50，微秒 | 15,851 | 14,360 |
| cold：p95，微秒 | 17,133 | 20,912 |
| warm：p50，微秒 | 7,604 | 6,813 |
| warm：p95，微秒 | 8,567 | 6,873 |

**没有观察到全面的尾延迟改善：cold p95 变差。** 样本小，且共享工作树同时有检索/图优化，不能把时延变化单独归因于本补丁。确定的收获是这个 fixture 的正文覆盖、调用数和响应量；它们不能证明任意大仓、所有语言、实际业务正确率或三家模型的能力排名。

基准前 `agent_context.rs` SHA：`b1f696324e63840e12598bb48d34542a4371749c3b4d0939bddb0e360b105769`（只注册了失败测试，尚未实现优化）。基准后格式化版本：`9207d17cdddcdd1d5e9a5487255e34048c8384e795bf3865d629ec94f8cabe52`；新的 `src/graph/code_index/context.rs` SHA：`be5f27bf0c051782747fd0dc5911ec8fae6472e2b2862ad2eb3ebc4839f9e633`。这是未提交工作树的文件指纹，不是完整仓库版本证明。

## 回归与边界

先复现三个失败：四个明确函数只带两个正文；caller 扩展替换第二个明确目标；源码截断插入省略号并保留原来的 end_line。修改后这些用例通过。另覆盖默认自适应预算与固定预算，1/4 并行上限，1,000–6,000 token 紧预算，以及 640 个 UTF-8/CRLF/空文本/已有截断标记组合。组合数量不是工程迭代轮数，也不冒充 fuzz 或 mutation 执行证据。

`symbol_hot_context` 与完整 `symbol_context` 使用同一新鲜度校验，正文、符号、SHA 和调用名必须一致；热源码消费者不再先读取最多八个关联正文再丢弃，也不为直接正文重建已逐出的 AST。单独测试同长度、同 mtime 的源码变化，以及跨 Workspace symbol ID 拒绝。完整 `symbol_context` 仍保留原有关联上下文能力。

该记录只描述本次基准与定向回归，不代替完整构建、Clippy、Design State 或版本稳定性门禁的结果。
