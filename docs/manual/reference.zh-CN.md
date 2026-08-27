---
layout: docs
title: CLI 与 MCP Reference
description: wcode CLI、操作入口、Transport 与 MCP 能力的统一参考
lang: zh-CN
alternate: /docs/reference/
permalink: /zh/docs/reference/
---

# CLI 与 MCP Reference

这页是 wcode 日常操作的 canonical Reference。概念解释放在专题文档里；这里主要回答“该运行什么命令、该调用哪个 Tool”。

## 启动与控制 Runtime

正常启动：

```bash
wcode --workspace "$PWD"
```

控制当前实例：

```bash
wcode restart
wcode stop
```

本地 Software Intelligence：

```bash
wcode --workspace "$PWD" intelligence
wcode --workspace "$PWD" intelligence --check --json
wcode --workspace "$PWD" verification
wcode --workspace "$PWD" verification --plan-id VP-...
```

同机 Coding Agent 默认优先 stdio：

```bash
wcode --workspace /absolute/path/to/repository mcp-stdio
```

云端 / Web Connector 使用 Runtime 输出的受保护公网 `/mcp` 地址。

## 常用 CLI 参数

| 参数 | 用途 |
| --- | --- |
| `-w, --workspace <PATH>` | 暴露一个仓库根目录；只有任务确实跨仓时才重复指定。 |
| `-j, --max-parallel-tools <N>` | 覆盖自适应的有界并行上限。 |
| `--public-url https://…` | 使用稳定反向代理地址，而不是临时 Quick Tunnel。 |
| `--read-only` | 移除模型侧文件修改能力。 |
| `--no-exec` | 禁止命令执行。 |
| `--no-open` | 启动后不自动打开 Setup Hub。 |
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

大改动前：

```text
workspace_info
  ↓
scope_status + design_status + project_context
  ↓
language_quality_status（源码 / 质量任务）
  ↓
software_context(scopes=...)
  ↓
精确源码导航 / 编辑
```

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
| `software_context` | 带 Budget 的任务上下文，可按 Product Scope 收窄并包含 Graph Context。 |

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
| `semantic_provider_status` / `semantic_provider_refresh` | 第一方 LSP Provider 与有界 Semantic Refresh。 |
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

- **CommandAccess**：为一个 Workspace 授权裸可执行程序名。
- **RiskyExecution**：为当前 Session 授权一个精确仓库操作 Fingerprint。
- **RuntimeExecutor**：为一个精确高级验证 Executor 操作授权。
- **Destructive delete**：一次性授权，与可复用 Session Grant 分离。

Git Mutation 仍然很窄：只有显式 pathspec 的 `git add`、message-only `git commit -m ...`、非 Force / 非 Delete 的 `git push <remote> <refspec>` 可以进入精确授权。批准操作并不等于转发 SSH Agent、Token、Credential Helper 或任意 Git Config。

## 诊断

连接问题先看 TUI，再用 `/healthz` 分层排查 HTTP / Tunnel / OAuth。

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
