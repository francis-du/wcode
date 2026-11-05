---
layout: docs
title: 安全模型
description: Workspace、命令、人工授权、OAuth 与 Evidence 的安全边界
lang: zh-CN
alternate: /docs/security/
permalink: /zh/docs/security/
---

# 安全模型

wcode 默认是宽松的。调用方 Agent 拥有授权权：wcode 执行该 Agent 请求的操作，只在操作确实不可能时拒绝，例如路径语法错误或资源上限耗尽。内部策略闸门不再是第二层权限控制。

![wcode 安全与授权边界](/assets/zh/security-boundary.svg)

## Workspace 隔离

模型只能看到显式配置的 Workspace 根目录。面向模型的文件操作会拒绝绝对路径、父级穿越、`RootDir`/`Prefix` 组件、控制字符、空路径或 `.` 路径、Symlink 组件、Workspace 逃逸和不安全 Hard-link 情况。实际配置的 OAuth 与工程权威状态目录会被文件入口和递归发现排除；文件工具不能写入、替换、移动或删除这些目录及其祖先。这些是路径包含与别名正确性不变量，不是策略选择。受保护的凭据与环境路径不再被拒绝；由 Agent 决定读写什么。

任何目录都可以作为 Workspace 根目录暴露，包括用户主目录或文件系统根目录，重叠的 Workspace 根目录也接受。需要更小工作集时可以选择更窄的根目录；wcode 不再把它作为权限规则执行。

配置根目录内部的项目标记可以派生为 Subspace，但不会扩大最外层边界：
相对路径从当前 Workspace 解析，Canonical Path 会重新检查，Symlink 子目录
会被拒绝。

## 基于 SHA 的写入保护

编辑已有文件时使用模型读取到的 SHA-256 作为前置条件。原子替换避免半写入；源码已经变化时直接拒绝陈旧写入，而不是静默覆盖新版本。

删除是独立能力：删除一个普通文件或空目录，文件删除需要精确 SHA 前置条件。递归删除、根目录删除、Symlink 与 Hard-link 删除继续由路径不变量禁止。不需要人工授权。

## 默认宽松的命令执行

`run_command` 接受可执行程序和参数数组，不会隐式把一整条字符串解释成 Shell 语法。命令策略仍作为嵌入式使用方的显式加固模块保留，但默认策略是宽松的，`run_command` 直接执行命令。

OS 强沙箱对按调用选择沙箱的调用方仍然可用。当没有可用沙箱后端时，wcode 直接运行命令而不会拒绝；沙箱不可用永远不会成为命令不运行的原因。环境清理、CWD 包含、进程归属、超时、取消和输出/资源上限继续生效。

`--read-only` 关闭文件修改工具，`--no-exec` 关闭命令执行，`--no-semantic` 关闭语言服务器发现。三者都是操作者显式选择，并且会被真正遵守，包括优先于会话级命令授权。消失的是默认策略层：不带这些开关时，wcode 不会以策略为由拒绝操作；只读 Workspace 也不会拒绝被分类为写入的命令。

开发 Catalog 覆盖 22 个已索引语言的代表性原生与生态工具，包括 `gh`、`just`、`task`、`uv`、`ruff`、`biome`、`deno`、`docker`、`kubectl`、`terraform`、`fd`、`jq`、`cmake`、`ninja`、`dotnet`、`mvn`、`gradle`、`swift`、`zig`、`pre-commit` 与 `act`；选择加固的嵌入式使用方仍可获得逐族命令策略。Rust Verification 可以使用已安装的 `cargo-nextest`，并保留 `cargo test` 作为兼容回退。保留的检查是路径/参数包含和畸形输入拒绝，不是策略审批。

Git 保留其有界生命周期加固，防止仓库控制的 Hook、Helper、凭据和配置注入。命令准入不提供凭据，也不保证远程传输成功；它不构成用户的发布授权、HumanApproval 决定或合并/部署验收，这些仍有独立的意图、证据与集成边界。

GitHub CLI 保留其参数与凭据处理。一个命令形态获准执行，不等于人工决定、发布或部署已获批准。

仓库感知的 LSP Server 保留其独立执行通道，包括有界环境、Workspace 路径检查和进程监督。这些控制缩小执行面，但不是 OS 沙箱。

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

正常开启执行能力时，普通命令不需要额外人工命令授权，显式撤销过的可执行程序访问除外。Workspace 本次运行全部授权和 Full Access 使用 `session_override`。`--no-exec` 是操作者的显式加固选择，优先于会话级命令授权；显式命令撤权、只读命令写入分类和普通命令策略过滤在默认宽松策略下不生效。OS 沙盒按调用选择性启用：`run_command` 默认直接执行，上层 agent 可以在需要隔离的具体调用上传入 `sandbox: true`（嵌入式 API 为 `run_command_sandboxed`）；没有可用后端时回退为直接执行，沙盒永远不会成为命令无法运行的理由。撤销会话授权后，恢复基础执行/写入策略及原有显式命令撤权。没有覆盖时，exec 关闭对应规范状态 `disabled`，exec 开启对应 `permissive_sandboxed`。这些状态不会取消受保护权威状态根、受保护 CWD、进程/资源上限、环境清理或 WCode 文件工具/删除控制。

`RiskyExecution` 继续作为自动 Profile 之外非普通命令执行的 Fingerprint-scoped Trust 机制，例如没有 Automatic Profile 的 LSP Server 或高级 Stage Executor。普通 `run_command` 不需要它，也不会自行满足这些独立信任检查。显式 Runtime Risk Grant、Workspace 会话覆盖或 Full Access 可以在支持的路径中满足既有 Runtime Trust，但都不能授权 HumanApproval。对非自动 LSP Server，精确模式仍绑定 Workspace + Server + 当前 Binary Identity。Hardened 自动 LSP 与仓库声明的验证 Profile 保留自身的可执行程序、参数及环境限制。

无论命令授权状态如何，WCode 自己的文件工具都保持 Workspace 隔离。

本地 Policy 激活与撤销复用精确 HumanDecision，并记录独立治理历史，见[本地验收 Policy](../acceptance-policy/)。宽命令沙箱使用实际配置的权威状态根，而不是仅匹配目录名称；macOS 拒绝读写，Linux 无法遮罩则拒绝启动，且仅在调用方显式请求沙盒时生效。普通有界构建和测试仍运行仓库代码并拥有宿主用户权限，可能触及该用户的状态。因此带校验和的本地历史不能独立证明面对任意仓库代码的权威防伪；商业 merge gate 还需要不可信 Worker 之外的可信 Policy 和执行集成。

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

受保护 WebUI 的 **Access** 面板通过 **OAuth sessions** 卡片暴露同一边界：只列出有界元数据（不透明会话 ID、Client ID、Resource、Access／Refresh 数量与过期时间），并且撤销必须经过显式确认。该卡片绝不显示 Token 或私有 Grant Owner ID。成功撤销会报告已持久化的移除数量；持久化失败会保留实时移除结果，但报告尚未确认持久撤销，因此重启可能恢复该会话。该卡片仅面向操作者，不会放宽上述 HTTP 授权。

替换隧道只有在公网健康响应与当前进程一致后才会成为有效入口，旧 Token
的 Resource 随后才能迁移到这个入口。OAuth Metadata 与授权页仍使用请求
实际进入的 Host。历史 Resource 不会把旧域名重新变成有效 Host；属于另一
组 Workspace 根目录的 Token 也不会载入。

Host 校验与 Origin 校验分别执行。请求携带 Origin 时，它必须是单个有效 HTTP(S) 来源，并匹配已配置或通过当前实例健康验证的**有效**入口，包括仍有效的非主隧道别名。非浏览器客户端可以不发送 Origin；畸形、重复、带凭据以及仅存在于历史记录的来源仍会被拒绝。不会隐式信任任意客户端网站来源。请求仍须提供原有 Bearer 或受保护 WebUI 凭据，旧版 SSE Session 继续绑定 Owner 和入口。

已验证隧道先进入可信入口登记表，再显示链接或发出设置就绪信号。拒绝响应通过 `untrusted_host` 或 `untrusted_origin` 区分原因，不回显凭据。遇到 `untrusted_host`，应重连当前登记的入口并确保反向代理保留该公网 Host；不可信的 `Forwarded` / `X-Forwarded-Host` 不能登记新入口。不要通过关闭 Origin 校验或信任全部 Host 修复连接。

公网隧道只解决可达性，不提供授权。

## Intelligence HTTP 响应

每个 `/intelligence` 页面和 `/intelligence/*` 响应都带受保护的项目数据，因此有一个作用域中间件（`src/integrations/mcp/cache_policy.rs`）保证每个响应都带三个头：

| 头 | 值 | 原因 |
| --- | --- | --- |
| `Cache-Control` | `no-store` | 项目结构、证据、授权请求和命令策略不得被共享或中间层缓存留存。 |
| `X-Content-Type-Options` | `nosniff` | 与 setup 页面已有的保护一致。 |
| `Referrer-Policy` | `no-referrer` | 工作区根路径和计划标识符不得通过 referrer 泄漏。 |

处理器可以显式设置这些头；中间件只补缺失的那个，所以显式策略（例如静态资源的 `public, max-age=3600` 生命周期）会被保留。非 intelligence 路由（`/healthz`、`/mcp`、`/sse`）不在作用域内。

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
