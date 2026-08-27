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

## Workspace 隔离

模型只能看到显式配置的 Workspace 根目录。面向模型的文件操作会拒绝绝对路径、父级穿越、受保护路径、Symlink 组件、Workspace 逃逸和不安全 Hard-link 情况。

应当暴露仓库根目录，而不是用户主目录或文件系统根目录。

## 基于 SHA 的写入保护

编辑已有文件时使用模型读取到的 SHA-256 作为前置条件。原子替换避免半写入；源码已经变化时直接拒绝陈旧写入，而不是静默覆盖新版本。

删除是独立能力：删除一个普通文件或空目录需要精确的一次性本地人工授权；递归删除、根目录删除、受保护路径、Symlink 与 Hard-link 删除继续禁止。

## 不把 Shell 当执行原语

`run_command` 接受裸可执行程序名和参数数组，不解释 Shell 语法。Shell 解释器和带路径的程序名继续在模型执行面被阻断。

常用开发 CLI 现在在“可执行程序名”这一层进入默认 Catalog，但每个工具仍有自己的精确命令策略。当前覆盖 Git/GitHub CLI（`gh`）、Cargo 与常见包管理器，以及 `just`、`task`、`uv`、`ruff`、`biome`、`deno`、`docker`、`kubectl`、`terraform`、`fd`、`jq`、`cmake`、`ninja`、`dotnet`、`mvn`、`gradle`、`swift`、`zig`、`pre-commit`、`act`。严格本地的只读 / check-only 形态，例如受限 `fd`、`jq`、Ruff/Biome 检查和部分 Schema/能力查询，可以直接执行；仓库脚本/构建系统、写源码模式、Docker Daemon 读取、Kubernetes Cluster 读取、Provider 执行、远端写操作和 Compose 生命周期操作需要精确 `RiskyExecution` 授权；凭据/配置重定向以及破坏性基础设施操作继续阻断。

Git 写操作仍保持窄边界：只有显式 `git add` Pathspec、`git commit -m ...` 和 `git push <remote> <refspec>` 可以进入精确授权；Force/Delete/Mirror/Reset/Restore 继续阻断。已经批准的 `git push` 可以通过固定的非交互 SSH 命令使用当前 SSH Agent，因此常见 SSH Remote 可以正常 Push；Token、Credential Helper、AskPass、任意 Git Config、Proxy Helper 与 HTTP Extra Header 仍不会被隐式转发。

GitHub CLI 也采用独立的有界策略。PR / Issue / Run / Workflow / Release / Repo / Search 的只读查看可以直接执行；显式、非交互的 PR/Issue 创建、评论、Workflow Dispatch、基于已存在且已验证 Tag 的 Release 创建、显式指定 Merge Method 的 PR Merge，以及 Run Rerun/Cancel 进入精确授权。Release Asset 路径仍单独隔离；`gh auth`、`gh api`、Secret/Variable、Extension、Host/Repo 重定向、Admin/Auto Merge 等凭据或策略绕过面继续阻断。

Language Server 和高级 Verification Executor 可能加载仓库控制的配置或代码，因此属于更宽的信任边界。除非进程用 `--allow-risky-exec` 做了整体预授权，否则这些精确操作也需要人工批准。确定性 Harness Verification Lane 是例外：固定的 Check/Test/Build 形态由策略直接批准；Rust 仓库同时满足“已安装 cargo-nextest + 仓库声明 nextest 配置”时，会使用 `cargo nextest run [--locked]`，否则继续回退 `cargo test`。

## 人工授权只能在本地完成

待授权请求出现在 TUI 和受保护 WebUI 中。模型可以发起请求，但不能批准自己的请求。

![wcode 授权与访问控制](/assets/img_3.png)

TUI 操作：

```text
↑ / ↓  选择请求
Y      批准
N      拒绝
```

批准某个请求不会关闭 Workspace 隔离，也不会把命令执行变成 Shell。

## OAuth 与远程 MCP

云端或 Web 客户端通过受保护的 `/mcp` Resource 连接。wcode 保留：

- Protected Resource Metadata 与 Authorization Server Metadata；
- Authorization Code + PKCE；
- 有界的 Dynamic Client Registration 兼容路径；
- 精确 Redirect 校验；
- 绑定 Resource 的 Access/Refresh Token；
- Refresh Token 轮换；
- 浏览器 Origin 校验。

公网隧道只解决可达性，不提供授权。

## 多媒体与模型能力

`read_media` 默认只返回 Metadata。它可以识别有界的 PNG/JPEG/GIF/WebP 图片、常见音频，以及 MP4/WebM 的基础 Metadata，但不会假设当前模型支持多模态。只有客户端显式声明 `run.francis.wcode/media-content` 扩展并包含对应 Kind（以及可选 MIME Filter）时，wcode 才会发送 Image/Audio Payload；能力未知或不支持时直接 Fail Closed，不发送二进制内容。由于 MCP 当前没有标准 Video Tool Result Content Block，视频继续只返回 Metadata。

## 凭据与模型上下文

凭据类路径默认受保护；读取源码和符号上下文时会对高置信度 Secret 做脱敏。日志与诊断不得输出 Access Token、Refresh Token、PKCE Verifier 等凭据。

## Verification 与 Evidence

代码修改后的安全同样需要独立证据。Risk 会决定验证深度；确定性检查、独立 Reviewer、Stage Executor 和 HumanApproval 是不同 Evidence Producer。一个模型的 Pass 不能覆盖另一个 Producer 的 Fail。

## 推荐默认值

- 默认只暴露一个仓库 Workspace。
- 本地 Agent 优先 stdio。
- 远程 Connector 保留 OAuth。
- 编辑保留 SHA 前置条件。
- 优先批准精确请求，不要为了省事扩大整个进程的信任范围。
- 不需要写入或命令时使用 `--read-only` 或 `--no-exec`。
