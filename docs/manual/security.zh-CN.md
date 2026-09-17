---
layout: docs
title: 安全模型
description: Workspace、命令、人工授权、OAuth 与 Evidence 的安全边界
lang: zh-CN
alternate: /docs/security/
permalink: /zh/docs/security/
---

# 安全模型

wcode 的基本原则很简单：连接模型不等于把整台机器暴露给模型。

![wcode 安全与授权边界](/assets/zh/security-boundary.svg)

## Workspace 隔离

模型只能看到显式配置的 Workspace 根目录。面向模型的文件操作会拒绝绝对路径、父级穿越、受保护路径、Symlink 组件、Workspace 逃逸和不安全 Hard-link 情况。

应当暴露仓库根目录，而不是用户主目录或文件系统根目录。

配置根目录内部的项目标记可以派生为 Subspace，但不会扩大最外层边界：
相对路径从当前 Workspace 解析，Canonical Path 会重新检查，Symlink 子目录
会被拒绝。手工配置的重叠根目录默认仍然禁止。

## 基于 SHA 的写入保护

编辑已有文件时使用模型读取到的 SHA-256 作为前置条件。原子替换避免半写入；源码已经变化时直接拒绝陈旧写入，而不是静默覆盖新版本。

删除是独立能力：删除一个普通文件或空目录需要精确的一次性本地人工授权；递归删除、根目录删除、受保护路径、Symlink 与 Hard-link 删除继续禁止。

## 不把 Shell 当执行原语

`run_command` 接受裸可执行程序名和参数数组，不解释 Shell 语法。Shell 解释器和带路径的程序名继续在模型执行面被阻断。

默认开发 Catalog 现在覆盖 22 个已索引语言的代表性原生与生态工具，但每一类仍使用精确命令策略，而不是把整个进程无条件放开。有界 Operator/Development Catalog 明确包含 `gh`、`just`、`task`、`uv`、`ruff`、`biome`、`deno`、`docker`、`kubectl`、`terraform`、`fd`、`jq`、`cmake`、`ninja`、`dotnet`、`mvn`、`gradle`、`swift`、`zig`、`pre-commit` 与 `act`；各语言自己的 Compiler、Formatter、Linter、Type Checker 与 Test Runner 继续由对应精确策略覆盖。Rust Verification 可以使用已安装的 cargo-nextest，并保留 `cargo test` 作为兼容回退。受限的本地测试、检查、构建、格式化/Lint、Workspace 脚本、代码生成、依赖维护、Docker 开发工作流，以及仓库声明的 Verification Executor，都通过 Hardened Workspace Lane 自动执行，不再反复请求人工授权；写源码模式仍受 Read-only Workspace Policy 约束。危险的 Inline/Interactive Interpreter Eval、Compiler/Plugin/Agent 注入、Shell、Workspace Escape、Protected Path、凭据/配置重定向、Host-wide Tool/Runtime Mutation、Package Publication/Ownership、远程管理写操作与破坏性基础设施操作，继续按后果要求精确授权或永久阻断。

Git 写操作仍保持窄边界：只有显式 `git add` Pathspec、`git commit -m ...` 和 `git push <remote> <refspec>` 可以进入精确授权；Force/Delete/Mirror/Reset/Restore 继续阻断。已经批准的 `git push` 可以通过固定的非交互 SSH 命令使用当前 SSH Agent，因此常见 SSH Remote 可以正常 Push；Token、Credential Helper、AskPass、任意 Git Config、Proxy Helper 与 HTTP Extra Header 仍不会被隐式转发。

GitHub CLI 也采用独立的有界策略。PR / Issue / Run / Workflow / Release / Repo / Search 的只读查看可以直接执行；显式、非交互的 PR/Issue 创建、评论、Workflow Dispatch、基于已存在且已验证 Tag 的 Release 创建、显式指定 Merge Method 的 PR Merge，以及 Run Rerun/Cancel 进入精确授权。Release Asset 路径仍单独隔离；`gh auth`、`gh api`、Secret/Variable、Extension、Host/Repo 重定向、Admin/Auto Merge 等凭据或策略绕过面继续阻断。

LSP Server 可能加载仓库控制的配置或代码，因此 wcode 保留独立的 Hardened LSP Lane：只有拥有显式 Automatic Safety Profile 的内置 LSP Server 才默认进入这条路径。`rust-analyzer` 不仅必须解析到 Workspace 之外，还必须通过有界的 Executable Viability Probe；如果 PATH 里只是 rustup Shim、对应 Component 实际未安装，就直接视为 unavailable，而不是不断产生 `semantic_auto` 错误。启动环境会清除凭据和执行注入变量，并关闭 rust-analyzer 的 Build Script、Proc Macro、Cargo 自动 Reload 与 Check-on-save。Warm Session Pool 有固定容量，以 Workspace + Server Binary Identity 为 Key；一个 Slot 内串行化同一条 LSP Protocol Stream，空闲或旧 Slot 会淘汰，Server 退出或 Binary 变化后重建 Session；导航结果仍经过 Workspace Boundary 过滤。`--no-semantic` 可以彻底关闭这条 Lane；没有 Auto Profile 的 LSP Server 继续需要显式授权。仓库声明的 Property/Mutation/Fuzz/Runtime Executor 则进入独立的 Hardened Autonomous Local Lane：无 Shell、CWD/Path 受 Workspace 限制、清理敏感环境，并限制进程、输出和超时，不再产生 RuntimeExecutor 授权。

## 人工授权只能在本地完成

待授权请求出现在 TUI 和受保护 WebUI 中。模型可以发起请求，但不能批准自己的请求。

![wcode 授权与访问控制](/assets/wcode-access-management.png)

TUI 操作：

```text
↑ / ↓  选择请求
A      当前 Workspace 本次运行全部授权
Y      只批准当前精确请求
N      拒绝
```

命令视图中还可以按 **F** 开关 Workspace 级全部授权。受保护 WebUI 提供同一开关；支持 Form Elicitation 的 stdio 客户端会看到 `exact`、`all_commands`、`deny` 三种选择。

因此命令授权有两种人工选择的模式。**精确授权**继续保留可执行程序和参数 Fingerprint 检查；**当前 Workspace 本次运行全部授权**则是真正的命令全授权：用户显式开启后，直到关闭或 Runtime 退出，WCode 不再因为可执行程序、Shell、参数/路径形态、Git/GitHub 操作、凭据/发布命令或其他命令策略拒绝 `run_command`。它也会覆盖启动时的 `--read-only` / `--no-exec` 对直接命令的限制。CPU/内存/子进程、超时、取消回收与输出上限仍保留；WCode 自己的文件工具继续使用独立的 Workspace/路径/SHA/删除保护。

`RiskyExecution` 在精确模式下仍然是 Fingerprint-scoped Trust。Workspace 级全部授权是另一层仅存在于当前 Runtime 的 Operator 选择。对未进入 Automatic Profile 的 LSP Server，精确模式继续绑定 Workspace + Server + 当前 Binary Identity；全部授权模式只在选定 Workspace 内有意消除重复的 Command/RiskyExecution 授权提示。

精确授权仍保持命令检查；全部命令授权则明确允许命令本身使用当前 OS 用户拥有的能力，但 WCode 自己的文件工具仍保持 Workspace 隔离。

## OAuth 与远程 MCP

云端或 Web 客户端通常通过受保护的 `/mcp` Resource 连接。旧客户端可以
使用 `/sse` 和该 Session 对应的 `/message`。两种远程传输都保留：

- Protected Resource Metadata 与 Authorization Server Metadata；
- Authorization Code + PKCE；
- 有界的 Dynamic Client Registration 兼容路径；
- 精确 Redirect 校验；
- 绑定 Resource 的 Access/Refresh Token；
- Refresh Token 轮换；
- 浏览器 Origin 校验。

Client 注册以及 Access / Refresh Token 不按时间过期。wcode 按配置的
Workspace 根目录集合把它们写入用户状态目录，进程重启后重新载入。写盘
使用原子替换；Unix 文件权限为 `0600`；Symlink 状态文件会被拒绝；损坏的
状态会失败关闭。Authorization Code 仍只存在内存中，并保持短时、一次性。
Store 继续使用固定容量：Client 达到上限时可回收尚未绑定 Token 的注册，
Token 达到上限时淘汰最旧项，不会无限增长。

替换隧道只有在公网健康响应与当前进程一致后才会成为有效入口，旧 Token
的 Resource 随后才能迁移到这个入口。OAuth Metadata 与授权页仍使用请求
实际进入的 Host。历史 Resource 不会把旧域名重新变成有效 Host；属于另一
组 Workspace 根目录的 Token 也不会载入。

Host 校验与 Origin 校验分别执行。请求携带 Origin 时，它必须是单个有效 HTTP(S) 来源，并匹配已配置或通过当前实例健康验证的**有效**入口，包括仍有效的非主隧道别名。非浏览器客户端可以不发送 Origin；畸形、重复、带凭据以及仅存在于历史记录的来源仍会被拒绝。不会隐式信任任意客户端网站来源。请求仍须提供原有 Bearer 或受保护 WebUI 凭据，旧版 SSE Session 继续绑定 Owner 和入口。

已验证隧道先进入可信入口登记表，再显示链接或发出设置就绪信号。拒绝响应通过 `untrusted_host` 或 `untrusted_origin` 区分原因，不回显凭据。遇到 `untrusted_host`，应重连当前登记的入口并确保反向代理保留该公网 Host；不可信的 `Forwarded` / `X-Forwarded-Host` 不能登记新入口。不要通过关闭 Origin 校验或信任全部 Host 修复连接。

公网隧道只解决可达性，不提供授权。

## 多媒体与模型能力

`read_media` 默认只返回 Metadata。它可以识别有界的 PNG/JPEG/GIF/WebP 图片、常见音频，以及 MP4/WebM 的基础 Metadata。调用方通过 `include_content=true` 显式选择返回二进制内容，此时 wcode 直接使用标准 MCP `image` / `audio` Tool Result Content Block，不再要求私有 Capability Extension。由于 MCP 当前没有标准 Video Tool Result Content Block，视频继续只返回 Metadata。

## 凭据与模型上下文

凭据类路径默认受保护；读取源码和符号上下文时会对高置信度 Secret 做脱敏。日志与诊断不得输出 Access Token、Refresh Token、PKCE Verifier 等凭据。OAuth 状态文件本身含有 Bearer Credential，不要复制进仓库，也不要分享。

## Verification 与 Evidence

代码修改后的安全同样需要独立证据。Risk 会决定验证深度；确定性检查、独立 Reviewer、Stage Executor 和 HumanApproval 是不同 Evidence Producer。一个模型的 Pass 不能覆盖另一个 Producer 的 Fail。

## 推荐默认值

- 默认只暴露一个仓库 Workspace。
- 本地 Agent 优先 stdio。
- Streamable HTTP 和旧版 SSE 远程连接都保留 OAuth。
- 编辑保留 SHA 前置条件。
- 优先批准精确请求，不要为了省事扩大整个进程的信任范围。
- 不需要写入或命令时使用 `--read-only` 或 `--no-exec`。
- 不希望 Runtime 启动任何第一方 LSP Server 时使用 `--no-semantic`。
