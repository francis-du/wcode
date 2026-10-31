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

模型只能看到显式配置的 Workspace 根目录。面向模型的文件操作会拒绝绝对路径、父级穿越、受保护路径、Symlink 组件、Workspace 逃逸和不安全 Hard-link 情况。 实际配置的 OAuth 与工程权威状态目录会被文件入口和递归发现排除，包括 Full Access 的 Home Workspace；文件工具不能写入、替换、移动或删除这些目录及其祖先。这保护的是文件工具入口：有界直接执行的构建、脚本和 LSP 仍使用宿主用户权限；下文的宽命令执行路径才使用 OS 强沙箱。这不等于租户隔离，也不能防御控制该 OS 账户的攻击者。

应当暴露仓库根目录，而不是用户主目录或文件系统根目录。

配置根目录内部的项目标记可以派生为 Subspace，但不会扩大最外层边界：
相对路径从当前 Workspace 解析，Canonical Path 会重新检查，Symlink 子目录
会被拒绝。手工配置的重叠根目录默认仍然禁止。

## 基于 SHA 的写入保护

编辑已有文件时使用模型读取到的 SHA-256 作为前置条件。原子替换避免半写入；源码已经变化时直接拒绝陈旧写入，而不是静默覆盖新版本。

删除是独立能力：删除一个普通文件或空目录需要精确的一次性本地人工授权；递归删除、根目录删除、受保护路径、Symlink 与 Hard-link 删除继续禁止。

## 默认宽松的命令执行与强 Sandbox

`run_command` 接受可执行程序和参数数组，不会隐式把一整条字符串解释成 Shell 语法。正常开启执行能力时，通过现有校验的命令，包括未知工具和显式 Shell 调用，都不再额外要求 `CommandAccess` 或命令级 `RiskyExecution`。显式撤销过访问的可执行程序仍需重新授权；`--no-exec` 关闭普通命令执行。普通命令策略继续检查程序名、参数、操作后果和受保护的 Workspace CWD；命令授权不会扩大文件工具的访问范围。

已知的有界开发命令走 Hardened Direct Lane。超出有界 Profile 的命令形态，包括 `sh -c ...` 等 Shell 脚本执行，必须进入 OS 强沙箱。`sh -n script.sh` 这样的有界语法检查可以保持直接执行；`bash`、`dash` 和 `zsh` 也有同样受限的 Profile。macOS 使用 Seatbelt，Linux 使用 bubblewrap。Windows 尚无受支持的 Backend；任何不可用或无法落实边界的 Backend 都以 `sandbox_unavailable` 拒绝宽命令，不会回退到无沙箱进程。宽命令禁止外部网络访问，所选 Workspace 与私有 Scratch 之外的宿主文件系统保持只读，并遮罩或拒绝受保护的凭据、仓库控制路径及实际配置的权威状态根。环境清理、CWD、进程归属、超时、取消和输出/资源上限继续生效。

没有显式全部命令覆盖时，只读 Workspace 会拒绝被分类为写入的命令，并让宽命令对 Workspace 只有只读权限。有界直接执行的仓库代码仍不受 OS 沙箱隔离，因此仅查看会话应组合使用 `--read-only --no-exec --no-semantic`。会话覆盖及其对 Workspace 写入的影响见下文。

默认开发 Catalog 现在覆盖 22 个已索引语言的代表性原生与生态工具，但每一类仍使用精确命令策略，而不是把整个进程无条件放开。有界 Operator/Development Catalog 明确包含 `gh`、`just`、`task`、`uv`、`ruff`、`biome`、`deno`、`docker`、`kubectl`、`terraform`、`fd`、`jq`、`cmake`、`ninja`、`dotnet`、`mvn`、`gradle`、`swift`、`zig`、`pre-commit` 与 `act`；各语言自己的 Compiler、Formatter、Linter、Type Checker 与 Test Runner 继续由对应精确策略覆盖。Rust Verification 可以使用已安装的 cargo-nextest，并保留 `cargo test` 作为兼容回退。受限的本地测试、检查、构建、格式化/Lint、Workspace 脚本、代码生成、依赖维护、Docker 开发工作流，以及仓库声明的 Verification Executor，都通过 Hardened Workspace Lane 自动执行。在普通命令策略下，凭据流、Compiler/Plugin/Agent 注入、Workspace Escape、Protected Path、Host-wide Tool/Runtime Mutation、Package Publication/Ownership、远程管理写操作与破坏性基础设施操作仍保留按后果定义的硬拒绝。

Git 采用有界生命周期策略。普通执行已开启且没有显式撤权时，支持的形态无需第二层命令授权：显式 Pathspec 或 `-A` 暂存、仅通过消息参数创建提交、向当前 Upstream 或指定 Remote 的显式 Branch 非强制 Push、显式 Branch Fetch、`pull --ff-only`、有界 Branch/Switch/Tag 操作，以及针对显式路径的 `restore --staged`。参数形态和操作后果检查仍拒绝 Force/Delete/Mirror Push、Reset/Clean、工作树内容 Restore、Editor/Hook/Config 注入及不支持的形态。有界直接 Git 调用会禁用仓库控制的 Hook 和 Helper；只有有界 Git Push 可以通过固定的非交互 SSH 命令使用当前 SSH Agent。Token 类环境变量、Credential Helper、AskPass、任意 Config、Proxy Helper 与 HTTP Extra Header 仍会清除。命令准入不提供凭据，也不保证远程传输成功；它不构成用户的发布授权、HumanApproval 决定或合并/部署验收，这些仍有独立的意图、证据与集成边界。

GitHub CLI 也有独立的有界策略。PR/Issue/Run/Workflow/Release/Repo/Search 只读查看，以及限定在 `repos/{owner}/{repo}` 下的 `gh api` GET，可以直接执行。支持的显式非交互 PR/Issue 创建、评论、Workflow Dispatch、基于已存在且验证过 Tag 的 Release 创建、指定一种 Merge Method 的 PR Merge，以及 Run Rerun/Cancel，在普通执行下也无需第二层命令授权。Release Asset 上传路径、`gh auth`、任意 API 请求、Secret/Variable、Extension、Host/Repo 重定向和 Admin/Auto Merge 形态仍被普通策略阻断。预配置的 `GH_TOKEN`、`GITHUB_TOKEN` 与 `GH_ENTERPRISE_TOKEN` 是明确的 Provider 凭据例外；Repo/Host/Config 重定向和执行注入变量仍会清理。一个命令形态获准执行，不等于人工决定、发布或部署已获批准。

LSP Server 可能加载仓库控制的配置或代码，因此 wcode 保留独立的 Hardened LSP Lane：只有拥有显式 Automatic Safety Profile 的内置 LSP Server 才默认进入这条路径。`rust-analyzer` 不仅必须解析到 Workspace 之外，还必须通过有界的 Executable Viability Probe；如果 PATH 里只是 rustup Shim、对应 Component 实际未安装，就直接视为 unavailable，而不是不断产生 `semantic_auto` 错误。启动环境会清除凭据和执行注入变量，并关闭 rust-analyzer 的 Build Script、Proc Macro、Cargo 自动 Reload 与 Check-on-save。Warm Session Pool 有固定容量，以 Workspace + Server Binary Identity 为 Key；一个 Slot 内串行化同一条 LSP Protocol Stream，空闲或旧 Slot 会淘汰，Server 退出或 Binary 变化后重建 Session；导航结果仍经过 Workspace Boundary 过滤。`--no-semantic` 可以彻底关闭这条 Lane；没有 Auto Profile 的 LSP Server 继续需要适用的显式 Runtime Trust；普通 `run_command` 的宽松准入不会授予这份信任。仓库声明的 Property/Mutation/Fuzz/Runtime Executor 则进入独立的 Hardened Autonomous Local Lane：无 Shell、CWD/Path 受 Workspace 限制、清理敏感环境，并限制进程、输出和超时，不再产生 RuntimeExecutor 授权。这些控制缩小执行面，但不是 OS 沙箱。

## 人工授权只能在本地完成

待授权请求出现在 TUI 和受保护 WebUI 中。模型可以发起请求，但不能通过 MCP 响应或文件工具写入批准自己的 HumanDecision 请求。

HumanApproval 使用独立的本地操作者精确授权，两分钟到期且只能消费一次；它绑定服务器实例、Workspace、请求的 MCP Owner、完整 Plan 摘要、Code／Design 版本、Policy 和决定陈述。调用者提供的 `confirmed`、`approver` 或 producer 名称、普通 MCP Form Elicitation、命令会话授权及 Full Access 都不能授权这类决定。人工批准记录为人工决定，不会变成已执行或已通过的测试。这个本地操作者边界不提供 Team 角色，也不隔离同一 OS 用户下的恶意进程。

![wcode 授权与访问控制](/assets/wcode-access-management.png)

当前生产界面与示例请求的演示图；生成图片时没有授予访问权限。

TUI 操作：

```text
↑ / ↓  选择请求
A      当前 Workspace 本次运行全部授权
Y      只批准当前精确请求
N      拒绝
```

命令视图中还可以按 **F** 开关 Workspace 级全部授权。受保护 WebUI 提供同一开关；支持 Form Elicitation 的 stdio 客户端会看到 `exact`、`all_commands`、`deny` 三种选择。

正常开启执行能力时，普通命令不需要额外人工命令授权，显式撤销过的可执行程序访问除外。Workspace 本次运行全部授权和 Full Access 使用 `session_override`，其优先级高于 `--no-exec`、显式命令撤权、只读命令写入分类和普通命令策略过滤。宽命令仍必须进入强沙箱，并在这个边界内获得 Workspace 写入能力。撤销会话授权后，恢复基础执行/写入策略及原有显式命令撤权。没有覆盖时，exec 关闭对应规范状态 `disabled`，exec 开启对应 `permissive_sandboxed`。这些状态不会取消受保护权威状态根、受保护 CWD、进程/资源上限、环境清理或 WCode 文件工具/删除控制。

`RiskyExecution` 继续作为自动 Profile 之外非普通命令执行的 Fingerprint-scoped Trust 机制，例如没有 Automatic Profile 的 LSP Server 或高级 Stage Executor。普通 `run_command` 不需要它，也不会自行满足这些独立信任检查。显式 Runtime Risk Grant、Workspace 会话覆盖或 Full Access 可以在支持的路径中满足既有 Runtime Trust，但都不能授权 HumanApproval。对非自动 LSP Server，精确模式仍绑定 Workspace + Server + 当前 Binary Identity。Hardened 自动 LSP 与仓库声明的验证 Profile 保留自身的可执行程序、参数及环境限制。

无论命令授权状态如何，WCode 自己的文件工具都保持 Workspace 隔离。

本地 Policy 激活与撤销复用精确 HumanDecision，并记录独立治理历史，见[本地验收 Policy](../acceptance-policy/)。宽命令沙箱使用实际配置的权威状态根，而不是仅匹配目录名称；macOS 拒绝读写，Linux 无法遮罩则拒绝启动。普通有界构建和测试仍运行仓库代码并拥有宿主用户权限，可能触及该用户的状态。因此带校验和的本地历史不能独立证明面对任意仓库代码的权威防伪；商业 merge gate 还需要不可信 Worker 之外的可信 Policy 和执行集成。

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

Client 注册继续持久保存，不设置时钟 TTL。Access Token 在一小时后过期；令牌响应返回 `expires_in: 3600`、`Cache-Control: no-store` 和 `Pragma: no-cache`。Refresh Token 在 30 天没有成功轮换后过期；每次刷新都会签发新 Refresh Token、重新计算这段空闲 TTL，并立即移除同一 Client、同一授权 Grant 的旧 Access Token，其他 Grant Owner 不受影响。过期、为零或来自未来的 `issued_at_ms` 都失败关闭。已有持久 Grant 在迁移和重启时保留原签发时间，重新载入不会续命。

状态按配置的 Workspace 根目录集合写入用户状态目录，进程重启后重新载入。写盘使用原子替换；Unix 文件权限为 `0600`；Symlink 状态文件会被拒绝；损坏状态失败关闭。Authorization Code 仍只存在内存中，并保持短时、一次性。Store 继续使用固定容量：Client 达到上限时可回收尚未绑定 Token 的注册，Token 达到上限时淘汰最旧项，不会无限增长。

已实现的本地操作者会话管理 API 为 `GET /oauth/sessions` 和 `POST /oauth/sessions/revoke`，撤销使用 JSON 字段 `session_id`。它们遵循现有 Host／Origin 规则，并要求当前服务器的 `X-Wcode-UI-Token`；普通 MCP Bearer 不能授权这些操作。列表只含有界会话信息和不透明会话 ID，不返回 Access／Refresh Token 或私有 Grant Owner ID。成功撤销会持久移除该 Grant 的 Access／Refresh Token，重启也不会恢复。写盘失败返回 `revocation_not_persisted`，移除当前运行中的凭据，但不确认持久撤销。这是操作者管理 API，不是 RFC 7009 接口、Team ACL 或 SSO；Team ACL 与 SSO 尚未实现。

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
- 不需要更宽 Runtime Trust 时，对显式撤权的可执行程序或非自动 Provider 使用精确授权。
- 不需要写入或命令时使用 `--read-only` 或 `--no-exec`。
- 不希望 Runtime 启动任何第一方 LSP Server 时使用 `--no-semantic`。
