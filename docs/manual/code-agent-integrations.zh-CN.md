---
layout: docs
title: MCP 客户端接入
description: 全局优先的本机配置、可移植 Skill/插件包与远程 MCP 接入
lang: zh-CN
alternate: /docs/code-agent-integrations/
permalink: /zh/docs/code-agent-integrations/
---

# 把 MCP 客户端接入 wcode

客户端和 wcode 在同一台机器上时使用 stdio；远程或网页客户端使用
Streamable HTTP + OAuth。Skill 和插件包提供使用说明，MCP 提供工具能力。

## 1. 先选传输方式

| 客户端位置 | 传输 | 配置 |
| --- | --- | --- |
| 同一台机器 | stdio | `wcode mcp-stdio`（Host 当前目录 = Workspace） |
| 远程，首选 | Streamable HTTP | `https://host/mcp` + OAuth |
| 旧版远程客户端 | SSE 兼容层 | `GET /sse` + `POST /message?sessionId=...` + OAuth |

这不是三套工具实现。它们共用 JSON-RPC dispatch、Harness、Workspace
选择、命令策略、授权、Tool、Prompt、Resource 和 Software Intelligence
状态。SSE 只为仍使用 2024 传输方式的客户端保留；新配置直接使用
`/mcp`。

本地 stdio 配置不写死仓库路径：

```json
{
  "command": "wcode",
  "args": ["mcp-stdio"]
}
```

MCP Host 启动进程时的当前目录就是默认 Workspace，因此一份全局 Host
配置可以跨仓库复用，也不会误把插件包目录当源码。`--workspace` 只作为
用户显式覆盖。

### stdio 下的人工授权

`stdin` / `stdout` 本身就是 MCP 协议通道，所以 wcode 不会在这里插入终端
`yes/no` 输入把 JSON-RPC 流打断。支持 form elicitation 的 Host 会通过 MCP
收到授权请求，并由 Host 自己向用户展示确认。2026 协议使用
`input_required` MRTR；兼容 2025 协议的 stdio 使用 `elicitation/create`。
只有响应同时匹配 Pending Authorization、Opaque Challenge 和 MCP Client
Owner 时，wcode 才会交给原有 AuthorizationManager 建立授权；不存在允许
模型给自己批准的 MCP Tool。

如果 Host 没有声明 form elicitation，受限命令会返回缺少 Client Capability，
不会因为客户端交互能力不足就自动放权。

## 2. 检测并配置本机客户端

先安装 wcode：

```bash
curl -fsSL https://raw.githubusercontent.com/francis-du/wcode/main/install.sh | sh
```

然后配置本机 Host：

```bash
wcode setup
```

需要无写入预览时用 `wcode setup --dry-run`；需要交给其他程序处理时加 `--json`。报告把结果分成 `detected`、
`installed`、`updated`、`already_configured`、`manual`、`unsupported` 和
`failed`，并附上检测依据与目标文件。

交互 Setup 第一项是**全局（推荐）**，第二项是**当前项目**。全局模式只在
本机确认后修改已经验证过的用户级 Host 配置路径；项目模式只修改识别出的
仓库配置。两种模式都只安装 `wcode mcp-stdio`，不写死仓库路径，并保留
其他 MCP Server。JSON / TOML 先解析再做原子 SHA 保护更新；未知结构、
JSONC、YAML 等直接 Fail Closed。Setup 不下载插件，不要求用户项目里存在
`plugin/` 目录，不保存凭据，也不会替用户批准 RiskyExecution。

## 3. 导出 Skill 与插件包

`plugin/` 是唯一源码包。Rust Binary 通过 `include_str!` 内嵌其中的
README、Skill、Manifest 和连接说明，因此安装后的 Setup 可以在任意目录
运行，用户当前项目不需要存在插件目录。

```bash
# 不带 MCP 目标，适合分发
wcode agent-plugin --profile skill-only

# stdio Profile；由使用它的 Host 当前目录决定 Workspace
wcode agent-plugin --profile local-stdio

# 只写远程 URL，不写凭据
wcode agent-plugin \
  --profile remote-http \
  --remote-url https://current-host.example/mcp
```

所有导出都包含标准 `mcp.json`。`skill-only` 的 `mcpServers` 为空；
`local-stdio` 只写 `wcode mcp-stdio`，由使用它的 Host 当前目录决定
Workspace；`remote-http` 只接受不含
凭据、查询参数或片段的 HTTPS 来源或 `/mcp` 地址。OAuth 令牌始终由 MCP
客户端保存。

包内同时提供 `.claude-plugin`、`.codex-plugin` 和 `.zcode-plugin`
元数据。它们只是同一个 Skill 的客户端适配层，不会增加 Hook、可执行
脚本或另一套安全规则。

## 4. 客户端能力与安装矩阵

最后一列只写已经验证的范围。“配置合并已测”表示 wcode 适配器能安全
创建或合并该项目文件，不代表该客户端的每个版本都跑过 OAuth 端到端测试。

| 客户端 | 插件包 | 通用 Skill | stdio | Streamable HTTP | SSE | OAuth | 一键安装 | 仅手工 | wcode 依据 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Claude Code | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | 配置合并已测 |
| OpenAI Codex | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | — | 配置合并已测 |
| GitHub Copilot CLI | — | ✓ | ✓ | ✓ | 依版本 | 依版本 | ✓ | — | 配置合并已测 |
| VS Code + Copilot | — | ✓ | ✓ | ✓ | — | ✓ | ✓ | — | 配置合并已测 |
| Cursor | — | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | 配置合并已测 |
| Gemini CLI | — | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | 配置合并已测 |
| Qwen Code | — | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | 配置合并已测 |
| Kiro | — | ✓ | ✓ | ✓ | 依版本 | ✓ | ✓ | — | 配置合并已测 |
| Qoder CLI | — | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | 配置合并已测 |
| Cline | — | ✓ | ✓ | 依版本 | 依版本 | 依版本 | — | ✓ | 仅应用内设置 |
| Kimi Code CLI | — | ✓ | ✓ | ✓ | 依版本 | ✓ | — | ✓ | 厂商文档 |
| OpenCode | — | ✓ | ✓ | ✓ | — | ✓ | ✓ | — | V1/V2 合并已测 |
| Roo Code | — | ✓ | ✓ | ✓ | 依版本 | 依版本 | — | ✓ | 厂商文档 |
| Continue | — | ✓ | ✓ | 依版本 | 依版本 | 依版本 | — | ✓ | 配置结构与版本相关 |
| ZCode | ✓ | ✓ | ✓ | 依版本 | 依版本 | 依版本 | — | ✓ | 仅验证插件包 |
| Grok Build | — | ✓ | ✓ | 依版本 | 依版本 | 依版本 | — | ✓ | 手工 stdio 配置 |
| Windsurf | — | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | 厂商文档 |
| JetBrains / Junie | — | ✓ | ✓ | 依版本 | 依版本 | 依版本 | — | ✓ | 仅 UI 配置 |
| Zed | — | ✓ | ✓ | 依版本 | 依版本 | 依版本 | — | ✓ | 保留 JSONC |
| TRAE | — | ✓ | ✓ | ✓ | ✓ | 依版本 | — | ✓ | 不声称 OAuth 已验证 |
| CodeBuddy | — | ✓ | ✓ | 依版本 | 依版本 | 依版本 | — | ✓ | 未知配置结构不改 |
| ChatGPT / Claude / Grok / Mistral Web | — | — | — | ✓ | 依平台 | ✓ | — | ✓ | 账户内配置 |

可安全自动化的目标包括 `.mcp.json`、`.codex/config.toml`、
`.vscode/mcp.json`、`.cursor/mcp.json`、`.gemini/settings.json`、
`.qwen/settings.json`、`.kiro/settings/mcp.json` 和 `opencode.json`。没有
检测依据时，wcode 不会凭空创建某个客户端的文件。OpenCode 会先判断 V1
或 V2 容器，再合并 `wcode`。

## 5. 需要手工配置的客户端

- **Kimi Code CLI：**在当前 MCP UI / CLI 中加入 `wcode mcp-stdio`；Host
  当前工作目录决定 Workspace，wcode 不假设其项目配置结构。
- **Cline：**使用 MCP 设置页或 CLI。当前 MCP 配置属于应用状态，仓库
  安装器不会改写它。
- **Roo Code、Windsurf：**从 Workspace MCP 设置页面添加，扩展全局状态
  保持不动。
- **Continue：**常见配置使用 YAML；手工添加比有损重写更可靠。
- **Zed：**项目设置可能是 JSONC，保留原有注释。
- **JetBrains / Junie、TRAE、CodeBuddy：**使用当前 IDE 的 MCP 页面。
- **ZCode：**安装导出的插件包，再配置 `wcode mcp-stdio`。
- **Grok Build：**复制通用 Skill，并添加 `wcode mcp-stdio`；Host 当前目录
  决定源码 Workspace。

`manual` 表示需要手工配置，安装器不会把它计为成功。

## 6. Web 客户端、隧道与 OAuth 会话

ChatGPT Web、Claude Web、Grok Web 和 Mistral 都是账户级 Connector。把
当前 HTTPS `/mcp` 地址粘贴到平台设置，并在浏览器完成 OAuth。本地仓库
文件无法安全代替这一步。

OAuth Client 注册、Access Token 和 Refresh Token 都不按时间过期。wcode
按配置的 Workspace 根目录把它们保存在用户状态目录中，重启后会重新
载入。新隧道通过当前实例健康校验后可以继续原会话；从新入口刷新时，
Token Binding 会迁到该入口。

授权页、令牌端点和元数据始终使用请求实际到达的域名，从隧道 B 发起验证
不会跳去隧道 A。未知域名仍会被拒绝，历史 Token 中的 Resource 也不会
让已经失效的旧隧道重新变成有效 Host。

从 v0.4.3 升级时，如果客户端还保留旧版签发的 `wcode-<uuid>` Client ID，
wcode 会在它下一次完成 `/authorize` 时恢复这条注册。Redirect URI 仍按
原规则校验，只有本机配对码通过后才会持久保存。

不支持 OAuth 的客户端应改用本地 stdio、本地桥接或可信反向代理。
`/sse` 兼容入口同样要求 OAuth、Origin 校验和正常 Workspace 策略。

## 7. 一条够用的编程流程

默认可以从这条链路开始：

```text
agent_context(goal, scopes=...)
    ↓ readiness + parallelism
独立 Lane ── 多个顶层 MCP Call 并发
    ↓ 只有真实依赖才串行
有界编辑 → review_changes → verify_project
```

调用保持紧凑：默认 `workspace`，以及服务端默认的 Path / Limit / Timeout /
Budget 都不要显式传。只有多个 Root / Subspace 导致目标不明确时才先调用
`workspace_info`。编辑时单文件用 `apply_edits`，多个独立文件用
`apply_file_edits`。输入已经明确时优先用 `search_many`、`read_files`、
`apply_file_edits`、`create_files`，减少 Round Trip 但不制造一个巨大的嵌套
Arguments JSON。Host 支持时，独立 Lane 使用多个顶层 Tool Call 并发；
`parallel_tools` 只保留给参数很小的紧凑 Fan-out。`agent_context` 现在会直接
返回 Parallelism Strategy；只有 Readiness 明确缺更多源码时才调用
`symbol_context`。

部分模型 API 提供 `defer_loading` 一类服务端工具选项。它只用于 API
连接，不是 Claude Code、Codex 或通用 MCP 的项目配置；不要把这类 API
JSON 复制进本地客户端配置。

## 8. Workspace 与命令授权

配置根目录是最外层边界。根目录内的项目标记会派生为子空间，因此
客户端可以直接选择 `Rust/wcode`，不需要再注册一个重叠 Workspace。
WebUI 的相对路径从当前选中的 Workspace 解析，符号链接子目录会被拒绝。

命令授权分两层：

1. **可执行程序访问**：允许一个 Workspace 中的一个程序名。
2. **精确仓库操作**：允许同一 Workspace 中的一组参数指纹。

批准 `cargo` 不等于批准任意 `cargo` 参数；批准 `cargo test` 也不会覆盖
`cargo fmt`、另一个 Workspace 或另一个子空间。拒绝请求不会留下
授权。

LSP Trust 与上述命令授权分离。未进入 Automatic Profile 的 Warm LSP Session 使用绑定 Workspace + Server + 当前 Binary Identity 的 `RiskyExecution`；批准后 Refresh/Navigation 可以复用这一份 Server，但替换后的 Binary、其他 Server 或无关仓库操作都不会继承旧 Grant。

## 9. 排查方法

- 运行 `wcode setup --dry-run --json`，查看客户端的 `evidence`、`target` 和
  `guidance`。
- OAuth 如果打开了错误域名，先确认客户端连接的是当前隧道 URL；元数据
  中的来源应与它一致。
- 临时隧道换域名后，先把客户端的 MCP URL 改成当前地址；已有 OAuth
  会话可以迁移，但旧地址本身已经不可达。需要固定地址时使用稳定的
  `--public-url`。
- `/mcp` 返回 401 时，按 `WWW-Authenticate` 指向的资源元数据
  完成 OAuth，不要手工塞静态令牌。
- 旧客户端使用 SSE 时配置 `/sse`；第一条事件会给出对应的
  `/message?sessionId=...`。
- 重试仍被阻断时，先看待处理请求属于“可执行程序访问”、“精确仓库操作”
  还是 LSP Session，再在正确 Workspace 只批准对应请求。

## 10. 主要依据

- [Agent Plugins 1.0 规范](https://agent-plugins.org/specification)
- [Agent Plugins MCP Server](https://agent-plugins.org/plugin-authors/mcp-servers)
- [MCP 旧版 SSE 传输](https://modelcontextprotocol.io/specification/2024-11-05/basic/transports)
- [MCP 向后兼容](https://modelcontextprotocol.io/specification/2025-03-26/basic/transports)
- [Claude Code MCP](https://code.claude.com/docs/en/mcp)
- [OpenAI Codex MCP](https://learn.chatgpt.com/docs/extend/mcp?surface=cli)
- [GitHub Copilot CLI](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference)
- [VS Code MCP 配置](https://code.visualstudio.com/docs/agents/reference/mcp-configuration)
- [Gemini CLI](https://google-gemini.github.io/gemini-cli/docs/cli/tutorials.html)
- [Qwen Code MCP](https://qwenlm.github.io/qwen-code-docs/en/users/features/mcp/)
- [Kiro MCP](https://kiro.dev/docs/mcp/configuration/)
- [Qoder CLI MCP](https://docs.qoder.com/cli/mcp-reference)
- [OpenCode MCP](https://opencode.ai/v2/docs/mcp-servers/)
- [Cline 配置](https://docs.cline.bot/getting-started/config)
