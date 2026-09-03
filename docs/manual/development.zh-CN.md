---
layout: docs
title: 开发说明
description: wcode 的模块边界、运行时不变量与发布约束
lang: zh-CN
alternate: /docs/development/
permalink: /zh/docs/development/
---

# wcode 开发说明

这页面面向 wcode 自身维护者。用户上手从 [快速开始](../getting-started/) 开始；产品行为、Precision 和安全边界分别以 [Software Intelligence](../software-intelligence/) 与 [安全模型](../security/) 为准。

## 模块地图

wcode 按产品责任拆分源码，不继续增长一个泛化 Runtime / Service Layer。

- `src/main.rs`：保持为极薄 Binary Launcher；产品启动逻辑位于 `src/app/`。
- `src/app/`：CLI 与启动组合。`mod.rs` 负责 Runtime Lifecycle 与 Graceful Shutdown，`commands.rs` 维护稳定命令面，`setup.rs` / `update.rs` 负责安装生命周期，`tunnel_lifecycle.rs` 将 Reconnect / Failover Policy 与 Provider Process 分离。
- `src/scopes/mod.rs`：唯一 Canonical Product Scope Registry，包含 Alias、Source Root 与 Tool-to-scope Mapping，供 Context、Semantic、Convention、Design State、MCP Metadata 和 Operator View 共用。
- `src/runtime/`：Harness 与 Runtime 编排。`src/runtime/harness/` 负责公共 Harness Core，以及 Agent Context、Profile/Cache、Repo-map、Graph、Review、Quality 与 Verification 模块；`src/runtime/semantic.rs` 维护有界 Semantic Freshness，`src/runtime/worklist.rs` 保存 Durable Agent Progress，`src/runtime/power.rs` 负责 Sleep Inhibition，`src/runtime/tunnel/` 负责 Managed Public Tunnel Provider 与健康检查。
- `src/integrations/`：Model / Client Integration Boundary。`src/integrations/mcp/` 统一负责 Route、stdio/SSE Adapter、Dispatch、Compact Tool Schema、Durable Task、Authorization 与 Web Transport；`src/integrations/auth/` 负责 OAuth/PKCE/DCR 与 Request-origin State；`src/integrations/agent_plugin/` 导出 Canonical Package，`src/integrations/agent_install/` 负责 Host Detection、安全 Merge/Apply 与报告。
- `src/workspace/`：安全 Local Coding Boundary，包含 Bounded File/Search/Edit/Move/Delete Primitive、Root/Registry Isolation、Command Policy、Local Authorization、Media、Convention 与 Dependency-aware Path Scheduler。
- `src/design/`：Structured Desired Software State、Stable ID / Reference Validation、Sparse Initialization、Implementation / Verification Mapping。
- `src/graph/`：Lazy Tree-sitter Code Index、Provider-neutral Software Graph，以及 Provider / Composite Revision Persistence。
- `src/semantics/`：Persistent Candidate/Confirmed/Retired Semantic Registry 与第一方 LSP Provider Runtime。
- `src/intelligence/`：Traceability、Scoped/Task Context、Drift/Impact/Risk 和 Project Observatory Projection；`src/intelligence/observatory/architecture.rs` 专门负责 Architecture-first Design-vs-Actual Component Projection。
- `src/verification/`：Verification Plan、Blind Reviewer/Readiness、Language Quality Provider，以及 Property/Mutation/Fuzz/Runtime Executor。
- `src/evidence/`：带 Provenance 的 Evidence Contract 与 Bounded Persistence。
- `src/reconciliation/`：Durable Desired-to-Actual Plan 与 Dependency-aware Execution / Retry State。
- `src/ui/`：Operator Experience。`src/ui/monitor/` 负责 Ratatui Runtime、State、Metrics、Commands、Detail Panel、Overlay、Shell Action、i18n 与 Theme；`src/ui/intelligence_web.rs` 和 `src/ui/intelligence_web/` 资产共同服务受保护 Project Observatory。

职责移动时，同一个 Change 里要同步更新 Product Scope Source Root 与 Design State Implementation Reference。源码物理移动了、Architecture Contract 还指着旧 Owner，不算完成 Refactor。

## Runtime 不变量

Tool Call 只有一条真实生命周期：

```text
request → queued → semaphore acquired → running → completed | failed
```

Global Semaphore 是唯一并发 Gate。Composite Operation 不能拿着 Parent Permit 再等 Child Permit；`parallel_tools`、`review_changes`、`verify_project` 等内部 Fan-out Operation 都通过同一套 Global Accounting 运行真实 Child Task。

`parallel_tools` 不是“全部一起跑”的通用 Helper，而是 Resource-aware Scheduler。它显式建模 `reads`、`writes`、`creates`、`moves_from`、`moves_to`、`deletes`；独立资源可以 Fan-out，重叠资源按依赖排序。同文件 `apply_edits` 只有在调用方 Pin 同一份 Observed SHA、Edit 不重叠且定位无歧义时才允许 Coalesce；无效 Overlap 在执行前就拒绝。

所有 Runtime Collection 都必须有界。Fan-out 数量、单项/聚合 Result Byte、Model-facing Read/Write Size、Source Scan、保留的 Complete AST、Git Review File/Finding、Traffic History、Persistent State History 与 Per-path Lock Map 都有显式 Limit。新增 Cache 必须跟真实 Source/Profile/Provider Revision 失效，不能按“调用了几次”猜 Cache Freshness。

Coding Context 热路径同时优化 Model Cost 与 Wall Time：

- `agent_context` 是默认 Coding 入口，使用显式或 Adaptive 的有界 Approximate Token Budget；
- 明确 Direct-target Task 保持小 Context，模糊 / Cross-module Task 可以在固定上限内自动增加 Context；
- Scope-aware Cold Repo-map 在 Ownership 已知时避免构建 Full-repository Graph；
- Repo-map Structure 按 Revision Cache，但每个 Task 的 Query Ranking 都重新计算；
- Multi-query Symbol Search 对一个 Source Root 只 Traversal / Index 一次，不为每个 Query Token 重扫；
- 有界热源可以保留最强的直接源码体，其余源码体继续渐进披露；
- Fresh Semantic/Runtime/Deterministic Graph Evidence 可以增强 Caller/Callee/Dependency Ranking；Stale Semantic Revision 自动回退 Syntax；普通 Symbol 定位继续走 Tree-sitter/Search，只有显式跨文件关系任务才可路由到 `semantic_navigation` 和对应 Warm Provider Session；
- Edit Target 保留 SHA、Writeability 与 Direct Working-tree State；
- `readiness` 与 Deterministic `next_actions` 告诉 Agent 现在应直接 Edit、补 Source/Semantic，还是进入 Verify；
- Timing / Cache / Savings Telemetry 放 Tool Result `_meta`，不反过来消耗 Model-visible Context。

Monitor 只显示真实工作。Queued/Running/Completed、Bytes、Peak Concurrency、Agent Context Calls、Average Model Tokens、Repo-map Cache Hit、Saved Context 都来自实际 Request Execution。Terminal Raw Mode、Mouse Capture、Cursor 与 Primary Screen 必须通过现有 RAII Boundary 恢复；Ctrl-C 走同一条 Graceful Shutdown。stdout 不是 TTY 或设置 `--no-monitor` 时不启动 Monitor。

Managed Public Tunnel 是 Runtime 自己拥有的 Child，与 Local HTTP Server 分离。`--tunnel-provider auto` 在后台并发启动 Cloudflare、`localhost.run`、Pinggy 与 Tailscale Funnel，面板绝不等待隧道。只有 URL Discovery + 当前 `instance_id` 对应 `/healthz` 成功后才算隧道存活——拿到一个 URL String 本身不是 Readiness。所有存活隧道全部保留，最先落地的担任 Primary Endpoint。单条隧道死亡后独立重拉，指数退避从 5 秒开始、300 秒封顶；一次通过实例健康校验的真实重连会清零该 Provider 的死亡历史，后续故障重新从短等待开始。Primary 死亡时按序提升下一条存活隧道；全部隧道掉光时，Public Health 回到 Pending，本地 Endpoint 保持可用并继续恢复。Quick Tunnel 重连后可能得到新 hostname，因此长期远程 Client 应使用 Tailscale Funnel 或 Operator 管理的稳定 `--public-url`。正常 Shutdown（Ctrl-C 或 SIGTERM）会 Abort Owned Task 并 Kill/Wait 全部 Tunnel Child。恢复逻辑绝不能去 Kill / Replace Operator 的无关进程。

Streamable HTTP、`mcp-stdio`、旧版 `/sse` + `/message` 共用同一个 JSON-RPC Dispatch、Harness 与 Workspace Implementation。SSE Session 绑定 Owner/Origin，有容量与 Channel 上限，并在 Stream 关闭时删除；Notification 返回 202 且不发送 Response Event，Channel 满时返回 429，不允许阻塞 Server。Supported Protocol Revision 必须显式；Modern Tool/Task/Resource Behavior 只能在 Request Revision / Capability 真正支持时启用，Legacy 或 Capability-unknown 情况按规则 Fail Closed。MCP Task 是 Durable Coordination Record，不代表 Process Execution 能跨 Runtime Replacement 存活。

OAuth Origin 按请求解析。只有通过实例健康校验并注册的 Host 才能成为该请求的 Issuer。Access Token 可在已验证入口间继续使用，Refresh 会把绑定迁到本次请求的 Host。Client 与 Token 状态按配置的 Workspace 根目录 Hash 原子持久化，Authorization Code 仍只存在内存。载入历史 Token Resource 只用于跨重启和隧道迁移，不会把旧 Origin 注册成有效请求 Host。Store 保持容量上限；损坏或 Symlink 状态按失败关闭处理。

Agent Installer 不执行任何 Host CLI。Detection 只看 Filesystem/PATH Evidence；Safe Adapter 只写项目文件，Merge 前先 Parse，并复用 Workspace Atomic Write。JSONC/YAML 和未知 Schema 保持 Manual。Host Metadata 统一放 Registry，不能在 `main.rs` 继续堆 Host-specific Branch。

Media 继续 Metadata-first。Image/Audio Binary Content 只有当前 MCP Request 显式声明匹配的 `run.francis.wcode/media-content` Extension 时才发出；Capability Unknown 时只返回 Metadata / Fail-closed，Video 始终 Metadata-only。

## Software Intelligence 不变量

Software Intelligence 的产品面包括 MCP、本地 `wcode intelligence` / `wcode verification` CLI、实时 TUI 与受保护 Project Observatory。

Project Observatory 必须 **Architecture-first**。页面先展示整体 Component Architecture、Declared Design Dependency、当前 Code-derived Actual Relationship、Observed Drift、Evidence Coverage、Implementation Coverage，然后进入 Component Inspector 与 Requirement Drill-down。Requirement Detail 保持：

```text
Desired State → Actual State → Change → Proof → Convergence
```

Strong Semantic/Runtime/Deterministic Positive Evidence 可以形成 Blocking Observed Drift；弱 Syntax 下没有观测到 Relationship 不能证明它不存在，只能保持 Advisory。Browser JavaScript 不能独立重建 Business Ownership；Generic Global Node-ball Graph 也不能重新变成 Primary Project View。

`src/scopes/mod.rs` 是唯一 Canonical Product Scope Registry。Source Ownership、Semantic Product Scope Alias、`agent_context` / `software_context` Narrowing、`scope_status`、`workspace_info` / `project_context`、Convention、Tool `_meta.dev.wcode/productScopes`、`wcode://runtime/product-scopes` 与 Operator View 都必须从同一 Registry 派生。未知 Semantic Scope String 继续作为 Freeform Business / Domain Scope。

Tree-sitter 始终是 `provider=tree-sitter`、`precision=syntax`，不能暗示 Compiler-level Overload / Type / Macro / Dynamic Dispatch Certainty。第一方 LSP 只有真实 Provider 返回有效结果后才能产生 `precision=semantic`。第一方 Node 保留 Source SHA；SHA Missing / Mismatch 代表 Provider Revision Stale。Stale Semantic Fact 必须退出 Graph Overlay、Agent Context Ranking、Impact、Reconciliation 与 Graph-aware Context，直到 Refresh。External SCIP/Compiler/Runtime Provider 保留各自 Provenance。

`.wcode/project.yaml` 与 `.wcode/design/` 是 Desired State Source。Initialization 保持 Sparse，不为凑 Schema 创建空 Collection File。ID / Cross-reference 必须稳定，Source Mapping 使用 Repository-relative Path，不能把不稳定 Source Line Number 写入 Design Object。

Verification Plan 是 Risk-adaptive Orchestration State，不是 Proof。Deterministic Check、Independent Reviewer、Property/Mutation/Fuzz/Runtime Executor、HumanApproval 是独立 Producer。Reviewer Disagreement 必须保留为 Disagreement，不允许多数票覆盖。Required Stage Evidence 按 Producer Fail-closed Aggregate；Workspace Revision Stale 会阻止 Ready。`verify_project` 只有在真实 Harness Report 完成后才记录 Deterministic Evidence；Acceptance Evidence 只为实际执行到的 Verification Reference 产生。

Persistent Intelligence State 存在 Repository 外的 Bounded Per-user / Per-workspace State Directory。Evidence、Verification、Semantic Revision、Provider/Composite Graph Snapshot、Reconciliation Plan/Execution、MCP Task 都有独立 Persistence Contract。Repository `.wcode/` 是 Desired State，不是 Runtime Cache Dump。

## 安全不变量

任何修改都要保持这些 Trust Boundary：

- Canonical Workspace Root Isolation 与 Root Identity Recheck；
- Derived Subspace 只能位于已配置 Parent 内，相对路径从当前 Workspace 解析，并拒绝 Symlink Component；手工重叠 Configured Root 继续阻断；
- 拒绝 Absolute Path、Parent Traversal、Protected Path、Symlink Component 与不安全 Hard-link Write；
- SHA-256 Edit Precondition、Per-file Lock、Post-lock Path Re-resolution、Bounded Atomic Write 与 Create-without-overwrite；
- `delete_path` 是唯一 Model-facing Delete Primitive，只能在 Exact One-shot Human Approval 后删除一个 Regular File 或 Empty Directory；Recursive/Root/Protected/Symlink/Hard-link Delete 永久阻断；
- No-shell Execution：Model 提供 Bare Executable + Argument Array，不解释 Shell Syntax，也不接受带路径 Interpreter；
- Known Development CLI 使用 Command-specific Policy，而不是“批准 Program 就批准全部 Subcommand”；
- Repository Script/Build、Bounded Remote Write、Docker/Kubernetes External-data Read 等跨普通 Read/Check Boundary 的操作使用 Exact `RiskyExecution` Fingerprint；
- Force/Delete/Mirror Git Push、`git reset` / `restore` Mutation、`gh auth` / `api` / Secret / Variable / Extension Bypass、Kubernetes Cluster Mutation、Terraform Apply/Destroy/Import/State-secret Surface、Shell Interpreter、Workspace 外 Config/Filesystem Redirect 永久阻断；
- Git Mutation 只允许 Explicit-path `git add`、Message-only `git commit`、Explicit Remote+Ref Non-force `git push` 进入授权。批准后的 SSH Push 只允许通过 wcode 固定 Non-interactive SSH Command 使用当前 `SSH_AUTH_SOCK`；Token-like Env、Credential Helper、AskPass、任意 Git Config、Proxy Helper、HTTP Extra Header、Hook、External Diff 仍被移除/禁用；
- `gh` Remote Mutation 必须 Non-interactive 且通过 Option Allowlist；未来新增未知 Write Flag 默认 Fail Closed，不能自动继承已有 Trust；
- URL Argument 不能内嵌 Credential，Credential/Environment File 不进入 Model-facing Filesystem / Index Surface；
- 每一种 Indexed Language 必须恰好保留一个经过 Contract Test 的 Canonical LSP Launch Profile；Provider-specific Executable Alias / Argument（包括 JDT LS `-data` 这种 Workspace/Runtime 唯一 State）属于 LSP Adapter 的正式职责，只有被证明可用的 Alternate 才能进入 Fallback Order；Compatibility Test 不冒充外部 Server 已安装，真实 Initialize Failure 必须可观测，而且只能切到单独获得 Trust 的已安装 Alternate；
- 第一方 LSP 只有拥有内置 Hardened Profile 时才可自动执行：Executable 必须解析到 Workspace 之外，stdio / Output / Timeout 保持有界，凭据和执行注入环境变量被清理，并在 Provider 支持时关闭仓库代码执行能力；Warm Session 有容量和 Idle 上限，以 Workspace + LSP Server Binary Identity 为 Key，按 Source Revision 同步 Document，而且 Result 仍必须经过 Workspace Filter；`--no-semantic` 可关闭整条 Lane；没有 Profile 的 LSP 与 Advanced Verification Executor 仍需显式 Trust，除非 Process-wide `--allow-risky-exec` 已明确预授权；
- Output Bounded、Timeout Termination、Sensitive Environment Scrub、Interactive Prompt Disabled；
- Public URL 只允许 HTTPS / Loopback HTTP，OAuth PKCE/Resource Binding、Bounded DCR Metadata、Redirect、Origin、Refresh-token Rotation 保持显式。

`--allow-risky-exec`、`--allow-destructive-writes`、Overlapping/Broad Workspace 等 Flag 都是 Trust-boundary Expansion，只能视为 Operator 的显式决定，不能变成“为了方便”的默认值。能精确描述 Operation 时优先使用 Session Authorization。它们都不是 OS Sandbox。

扩展 Development CLI Policy 时，应新增**经过检查的 Command Family**，而不是退回 Generic Execution。Strict Local Read/Check 可以 Direct；Repository Code Execution、Remote Mutation、External Daemon/Cluster Read 需要 Exact Authorization；Destructive Infrastructure / Credential-bypass Operation 继续 Block。每个新 Family 都要有代表性的 Allowed Shape 与 Escape/Mutation Blocked Regression。

## 跨平台依赖处理

Managed Tunnel 依 Provider 而异：Cloudflare 使用 `cloudflared`；显式 Cloudflare 且未设置 `--no-install` 时可走现有 Homebrew / winget 安装路径。`localhost.run` 与 Pinggy 使用系统 `ssh`，不会触发 Package Install。Tailscale Funnel 使用 `tailscale` CLI（需登录且 tailnet 开通 Funnel），暴露机器固定的 `ts.net` 地址；每台机器同时只有一个 wcode 实例能占用 Funnel 监听。`auto` 模式 Cloudflare 缺失时直接 Skip，让 Zero-install SSH Provider 立即接管。

Known Development CLI 不由 wcode 自动安装。Harness / Quality / Provider Discovery 只报告真实 Availability。存在正确 Fallback 的 Optional Accelerator 必须保留回退，例如 Rust Full Verification 只有在 Repository 声明 Nextest Config 且本机安装 `cargo-nextest` 时才优先使用，否则继续 `cargo test`。Strict Harness Lane 只允许固定 `cargo nextest run [--locked]`，不开放任意 Nextest Argument。

LSP Server 与 Stage Executor 同样区分 Registered 与 Available。自动 LSP Worker 只在最具体的 Discovered Workspace 上运行，避免 Broad Parent Root 与项目 Subspace 对同一批文件重复索引；每次真实 Auto Refresh 都必须先获取与 Model-facing Work 共用的 Global Harness Semaphore。wcode 知道某个 Ecosystem Tool 的名字，不代表它已经安装或可运行。

## 必需验证

Release 前必须通过 Full Gate：

```bash
git diff --check
cargo check --locked
cargo fmt --check
cargo test --locked
cargo clippy --locked -- -D warnings
cargo build --release --locked
```

`verify_project(level="full")` 是推荐的 Harness-controlled Release-quality Path。还要验证当前 Design / Traceability 与 Documentation Parity。Build Green 不等于 Release Ready：Design 或双语 Docs Stale 同样需要修复。

Documentation Change 必须保持 Reciprocal `alternate` Route、每对页面相同的 Top-level Section Structure、相同 Critical Technical Fact、Local Link、Installer Command，以及统一 `/docs/` + `/zh/docs/` Manual 模型。Host-specific Integration Command 只在 [Agent 与 MCP 集成](../code-agent-integrations/) 维护一份 Canonical Technical Guide，不要复制到 README / Website 形成第二个版本。

当 Command / Tooling Optimization 改变推荐 Agent Workflow 时，要在同一 Change 更新 Getting Started、Docs Index、Agentic Engineering，以及相关 Reference/Security 与 Automated Bilingual Contract。要避免“中英文完全同步，但两边一起保留旧流程”的失败模式。

## Release Artifact

`.github/workflows/release.yml` 在发布 Tag Artifact 前验证仓库。Release Package 目标包括：

- Linux x86_64；
- macOS Apple Silicon；
- macOS Intel；
- macOS Universal；
- Windows x86_64。

Packaged Binary 必须返回预期 `wcode --version`，并能成功渲染 `wcode --help`。macOS Artifact 在最终 strip / lipo 后必须重新做 ad-hoc Codesign，并通过 `codesign` 验证；Universal Archive 在上传前还必须通过 `install.sh` 完成一次真实 Release Smoke Test。Unix Installer 先把下载 Binary 暂存并执行 Smoke Test，确认成功后再原子替换现有安装。Archive / Checksum 属于 Release Artifact；历史 Release Note 只描述对应 Tagged Version，不能因为后续产品变化被批量改成当前语义。

Cargo / Package Metadata 与生成 Agent Plugin / Marketplace Manifest 的版本一致性是 Release Gate。Tag Push 是唯一自动 Publish Trigger；由它生成的 GitHub Release 被发布后，不能再启动第二套重复 Release Pipeline。历史文档保留历史 Version，当前 Package / Plugin Manifest 对齐正在准备的 Release。Local Check 通过并不授权自动 Commit、Tag、Push 或 Publish；这些始终是显式 Release Action。
