---
layout: docs
title: CLI 与 MCP 参考手册
description: wcode CLI、操作入口、传输方式与 MCP 能力的统一参考
lang: zh-CN
alternate: /docs/reference/
permalink: /zh/docs/reference/
---

# CLI 与 MCP 参考手册

这页是 wcode 日常操作的规范参考。概念解释放在专题文档里；这里主要回答“该运行什么命令、该调用哪个工具”。

## 启动与控制运行时

正常启动：

```bash
wcode --workspace "$PWD"
```

控制当前实例：

```bash
wcode restart
wcode stop
```

本地软件智能：

```bash
wcode --workspace "$PWD" intelligence
wcode --workspace "$PWD" intelligence --check --json
wcode --workspace "$PWD" verification
wcode --workspace "$PWD" verification --plan-id VP-...
```

同机编程智能体默认优先 stdio：

```bash
wcode --workspace /absolute/path/to/repository mcp-stdio
```

云端连接器使用运行时输出的受保护公网 `/mcp` 地址。旧客户端可以使用
`/sse`；第一条 SSE Event 会给出对应的 `/message?sessionId=...`。两种远程
传输都要求 OAuth 和 Origin 校验。

Agent 配置与插件导出：

```bash
wcode --workspace "$PWD" agent-plugin --install-all --dry-run [--json]
wcode --workspace "$PWD" agent-plugin --install-all [--json]
wcode --workspace "$PWD" agent-plugin --profile skill-only
wcode --workspace "$PWD" agent-plugin --profile local-stdio
wcode --workspace "$PWD" agent-plugin --profile remote-http --remote-url https://host/mcp
```

## 常用 CLI 参数

| 参数 | 用途 |
| --- | --- |
| `-w, --workspace <PATH>` | 暴露一个仓库根目录；只有任务确实跨仓时才重复指定。 |
| `-j, --max-parallel-tools <N>` | 覆盖自适应的有界并行上限。 |
| `--public-url https://…` | 使用稳定反向代理地址，而不是临时托管隧道。 |
| `--tunnel-provider auto\|cloudflare\|localhost-run\|pinggy\|tailscale` | 选择 HTTPS 隧道 Provider；`auto` 会在后台并发启动全部 Provider，保留所有通过验证的隧道，并对不可达的每 15 秒重试。 |
| `--read-only` | 移除模型侧文件修改能力。 |
| `--no-exec` | 禁止命令执行。 |
| `--no-semantic` | 关闭默认自动维护的第一方 LSP 索引，并禁止全部第一方 Semantic Provider 执行；满足 Hardened Profile 的 Provider 默认开启。 |
| `--open` | 启动后在浏览器打开 Setup Hub；链接始终展示在 TUI 与 setup 页面中。 |
| `--imessage-to <手机号或邮箱>` | 在 macOS 上通过本机 iMessage 推送每条可用隧道链接（setup、MCP、Web UI、配对码）。 |
| `--no-monitor` | 关闭实时终端面板。 |
| `--allow-sleep` | Serving 时不持有系统 idle-sleep assertion。 |
| `--allow-risky-exec` | 进程级预授权仓库感知执行；正常交互更推荐精确 Session 授权。 |

更宽的 Trust Boundary 参数只适合特殊部署，见 [安全模型](../security/)。

## TUI 快捷键

| 按键 | 动作 |
| --- | --- |
| `I` | 打开 Software Intelligence。 |
| `W` | 为当前 Workspace 打开受保护 Project Observatory。 |
| `O` | 重新打开 Setup Hub。 |
| `L` | 切换 TUI 语言。 |
| `+` | 添加 Workspace。 |
| `↑ / ↓` | 选择待授权请求。 |
| `Y / N` | 批准 / 拒绝当前请求。 |

## 推荐 MCP 工作流

正常编程任务：

```text
agent_context(goal, scopes=...)
  ↓
按 readiness / next_actions 执行
  ↓
只有需要更多源码时调用 symbol_context
  ↓
apply_edits 或 apply_file_edits
```

`agent_context` 省略 `budget` 时使用有界自适应预算。`workspace_info`、`scope_status`、`design_status`、`project_context`、`software_context`、`language_quality_status` 用于按需深入检查，不再作为每次编码的固定启动序列。

改动后：

```text
review_changes
  ↓
drift_status + impact_analysis + risk_status
  ↓
language_quality_run / reconciliation_plan（按需）
  ↓
verification_plan + verify_project + 必需 Stage
  ↓
evidence_status
```

## MCP 能力地图

### Workspace 与项目发现

| Tool | 用途 |
| --- | --- |
| `workspace_info` | Workspace、权限、安全策略、调度能力、Product Scope Registry。 |
| `project_context` | 项目类型、仓库指导、推断检查、Convention Report。 |
| `scope_status` | Product Scope 映射与有界未映射源码。 |
| `convention_status` | 命名、Architecture Domain、过大模块和仓库结构 Finding。 |

### Desired State 与 Context

| Tool | 用途 |
| --- | --- |
| `design_init` | 稀疏初始化 Design State，不覆盖已有文件。 |
| `design_status` | 校验结构化 Desired State。 |
| `traceability_status` | Requirement → Component → implementation、Acceptance → verification 覆盖。 |
| `agent_context` | 编程主入口：自适应 / 显式 Token Budget、相关 Design、按任务收窄的 Repo Map、Hot Source、SHA 编辑目标、验证引用、Readiness 与下一步动作。 |
| `software_context` | 更深层的软件智能上下文，可按 Product Scope 收窄并包含 Graph Context。 |

### 源码导航与有界 I/O

| Tool | 用途 |
| --- | --- |
| `file_outline` / `find_symbol` / `symbol_context` | Tree-sitter 定义导航，明确保持 syntax precision。 |
| `search_code` / `search_many` | 精确仓库搜索；已知多个查询时优先批量形式。 |
| `read_file` / `read_files` | 有界 UTF-8 读取，并返回 SHA-256 编辑前置条件。 |
| `read_media` | Metadata-first 媒体检查；二进制内容需要客户端显式声明能力。 |
| `parallel_tools` | 对已知独立读/发现/写操作做路径资源感知并行。 |

### Workspace 修改

| Tool | 用途 |
| --- | --- |
| `apply_edits` / `write_file` / `replace_text` | 使用已观测 SHA 原子修改已有文件。 |
| `create_file` / `create_files` / `create_directory` | 不覆盖目标地创建内容。 |
| `move_path` / `move_paths` | 不覆盖目标地移动/重命名 Workspace 路径。 |
| `delete_path` | 经过精确一次性本地授权后删除一个文件或空目录。 |
| `run_command` | 无 Shell、策略校验执行；非默认 / 高风险操作继续需要授权。 |

### Graph、Semantic 与 Language Quality

| Tool | 用途 |
| --- | --- |
| `software_graph` | 构建/持久化带 Provider / Precision / Revision 的 Composite Graph。 |
| `graph_history` / `graph_query` / `graph_diff` | 查看 meaningful Graph Revision 与结构变化。 |
| `graph_provider_import` / `graph_provider_status` | 外部 SCIP / LSP / Compiler / Runtime Graph Fact。 |
| `semantic_status` / `semantic_query` | 持久化 Candidate / Confirmed / Retired Semantic Fact。 |
| `semantic_record` / `semantic_confirm` / `semantic_retire` | 人工治理的 Semantic Lifecycle。 |
| `semantic_provider_status` / `semantic_provider_refresh` | 查看第一方 LSP 可用性 / Auto Eligibility，或强制执行一次有界 Refresh。Hardened Provider 默认后台维护，未进入 Auto Profile 的 Provider 继续要求显式信任。 |
| `semantic_navigation` | 复用 Warm LSP Session，以 Symbol-first 方式查询 Definition/Hover、Reference、Incoming/Outgoing Call、Implementation 或跨文件 Impact。普通定位继续优先 Syntax/Search；无可信 Provider 时明确回退 Syntax。 |
| `language_quality_status` / `language_quality_run` | Syntax / Semantic / Format / Lint / Type / Static / Test / Security 能力矩阵及 check-only 执行。 |

### Change、Risk、Verification 与 Evidence

| Tool | 用途 |
| --- | --- |
| `review_changes` | 有界 Git Review、numstat、Whitespace Check 与 Maintainability Signal。 |
| `drift_status` | 当前 Working Tree 的 Implementation / Design Drift。 |
| `impact_analysis` | Design Mapping + 有界 Reverse Call Impact。 |
| `risk_status` | 结构化 Risk 与风险自适应 Verification Profile。 |
| `reconciliation_plan` | 持久化 Desired → Actual 收敛计划。 |
| `reconciliation_status` / `reconciliation_history` | 跨重连 / 重启恢复计划。 |
| `reconciliation_execution_status` / `reconciliation_claim` / `reconciliation_submit` / `reconciliation_retry` | 持久化依赖感知收敛执行。 |
| `verification_plan` | 生成确定性 / Stage / Reviewer 验证要求。 |
| `verification_claim` / `verification_submit` | 独立盲审 Reviewer Job。 |
| `verification_executor_status` / `verification_execute_stages` | Property / Mutation / Fuzz / Runtime-Canary Runner。 |
| `verification_stage_submit` | 提交外部 Stage Verdict / Artifact Digest。 |
| `verification_approve` | Critical Plan 的显式 HumanApproval Evidence。 |
| `verification_status` / `verification_history` | Ready、Blocker、Stale Revision、Disagreement、Plan History。 |
| `verify_project` | 运行推断出的 quick/full 仓库质量门，并记录确定性 Evidence。 |
| `evidence_status` | 读取当前 Workspace 的持久化 Provenance-bearing Evidence。 |

## Precision 规则

Tree-sitter Fact 是 `precision=syntax`；真实 LSP Fact 才是 `precision=semantic`。Filesystem / Design / Runtime Provider 继续保留各自 Precision。Project Observatory 会直接显示当前 Provider / Precision，不把 syntax fallback 冒充成编译器级语义。

有界 syntax graph 里“没看到关系”并不等于关系不存在。弱精度下的 negative inference 只能是 advisory，不能直接变成 blocker。详见 [Software Intelligence](../software-intelligence/) 与 [语言质量](../language-quality/)。

## 授权模型

模型可以请求权限，但不能自批。待授权请求只能在本地 TUI 或 Token 保护的 WebUI 决策。

- **可执行程序访问（`CommandAccess`）**：为一个 Workspace 授权裸程序名。
- **精确仓库操作（`RiskyExecution`）**：为该 Workspace 和当前 Session 授权一组操作 Fingerprint。
- **RuntimeExecutor**：为一个精确高级验证 Executor 操作授权。
- **Destructive delete**：一次性授权，与可复用 Session Grant 分离。

Git Mutation 仍然很窄：只有显式 pathspec 的 `git add`、message-only `git commit -m ...`、非 Force / 非 Delete 的 `git push <remote> <refspec>` 可以进入精确授权。已批准 Push 可以通过 wcode 固定的非交互 SSH 命令使用当前 SSH Agent；Token 环境变量、Credential Helper、AskPass、任意 Git Config 以及 Force/Delete 形态继续阻断。

已知开发 CLI 采用命令级策略，而不是授权一个程序名后整套子命令都放开：`gh`、`just`、`task`、`uv`、`ruff`、`biome`、`deno`、`docker`、`kubectl`、`terraform`、`fd`、`jq`、`cmake`、`ninja`、`dotnet`、`mvn`、`gradle`、`swift`、`zig`、`pre-commit`、`act`。严格本地只读 / check-only 形态可直接运行；仓库构建/Runner、Docker/Kubernetes 数据访问和有界源码/远端写操作进入精确授权；Kubernetes Cluster Mutation、Terraform Apply/Destroy/State Secret、Gradle/Maven 发布、Host Toolchain 修改以及命令/文件加载绕过面继续阻断。Rust Full Verification 在仓库声明且本机安装 cargo-nextest 时优先使用 nextest，否则保留 `cargo test` 回退。

## 诊断

连接问题先看 TUI，再用 `/healthz` 分层排查 HTTP、隧道和 OAuth。同一组
配置的 Workspace 根目录会在重启后恢复 OAuth 注册与 Token；替换隧道通过
实例健康校验后可以继续该会话。Metadata 和授权页必须显示客户端实际使用
的 Host，未知或已经失效的 Host 仍会被拒绝。

仓库一致性检查：

```bash
wcode --workspace "$PWD" intelligence --check --json
```

实现质量应使用 `project_context` / `language_quality_status` 返回的仓库原生检查，最后执行 `verify_project`。

## 相关文档

- [快速开始](../getting-started/)
- [Software Intelligence](../software-intelligence/)
- [Agent 与 MCP 集成](../code-agent-integrations/)
- [安全模型](../security/)
- [语言质量](../language-quality/)
- [可维护性审查](../maintainability-review/)
