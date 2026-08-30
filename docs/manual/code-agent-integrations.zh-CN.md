---
layout: docs
title: MCP 客户端接入
description: 本地客户端、插件包和远程 MCP 的项目级配置
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
| 同一台机器 | stdio | `wcode --workspace /absolute/repo mcp-stdio` |
| 远程，首选 | Streamable HTTP | `https://host/mcp` + OAuth |
| 旧版远程客户端 | SSE 兼容层 | `GET /sse` + `POST /message?sessionId=...` + OAuth |

这不是三套工具实现。它们共用 JSON-RPC dispatch、Harness、Workspace
选择、命令策略、授权、Tool、Prompt、Resource 和 Software Intelligence
状态。SSE 只为仍使用 2024 传输方式的客户端保留；新配置直接使用
`/mcp`。

stdio 必须指向真实源码仓库，不能把插件安装目录当 Workspace：

```json
{
  "command": "wcode",
  "args": ["--workspace", "/absolute/repository", "mcp-stdio"]
}
```

## 2. 检测并配置本机客户端

先安装 wcode：

```bash
curl -fsSL https://raw.githubusercontent.com/francis-du/wcode/main/install.sh | sh
```

先看会改哪些项目文件：

```bash
wcode --workspace "$PWD" agent-plugin --install-all --dry-run
```

确认后执行：

```bash
wcode --workspace "$PWD" agent-plugin --install-all
```

需要交给其他程序处理时加 `--json`。报告把结果分成 `detected`、
`installed`、`updated`、`already_configured`、`manual`、`unsupported` 和
`failed`，并附上检测依据与目标文件。

安装器只写仓库内配置。JSON / TOML 会先解析，再合并 `wcode` 服务；
已有服务不受影响。更新采用原子写入，并用 SHA 防止覆盖并发改动。遇到
符号链接、超大配置、错误字段类型、无效 JSON/TOML、JSONC 或 YAML 时
直接停止，不猜测配置结构。它不会调用 shell、下载第三方包、保存凭据、
修改未知全局文件，也不会替用户批准 RiskyExecution。

## 3. 导出 Skill 与插件包

`wcode-agent-plugin/` 是唯一内容源。Rust 导出器通过 `include_str!`
直接复用其中的 README、Skill、清单和连接说明，不再维护第二份长文档。

```bash
# 不带 MCP 目标，适合分发
wcode --workspace "$PWD" agent-plugin --profile skill-only

# stdio 绑定当前仓库
wcode --workspace "$PWD" agent-plugin --profile local-stdio

# 只写远程 URL，不写凭据
wcode --workspace "$PWD" agent-plugin \
  --profile remote-http \
  --remote-url https://current-host.example/mcp
```

所有导出都包含标准 `mcp.json`。`skill-only` 的 `mcpServers` 为空；
`local-stdio` 写入当前 Workspace 的绝对路径；`remote-http` 只接受不含
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
| Grok Build | — | ✓ | ✓ | 依版本 | 依版本 | 依版本 | — | ✓ | 手工绑定 |
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

- **Kimi Code CLI：**在当前 MCP UI / CLI 中加入绝对路径 stdio 命令；
  wcode 不假设其项目配置结构。
- **Cline：**使用 MCP 设置页或 CLI。当前 MCP 配置属于应用状态，仓库
  安装器不会改写它。
- **Roo Code、Windsurf：**从 Workspace MCP 设置页面添加，扩展全局状态
  保持不动。
- **Continue：**常见配置使用 YAML；手工添加比有损重写更可靠。
- **Zed：**项目设置可能是 JSONC，保留原有注释。
- **JetBrains / Junie、TRAE、CodeBuddy：**使用当前 IDE 的 MCP 页面。
- **ZCode：**安装导出的插件包，再把 stdio 绑定到源码仓库。
- **Grok Build：**复制通用 Skill，并显式添加仓库级 stdio server。

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
workspace_info → agent_context(goal, scopes=...) → symbol_context
    ↓
read_file / apply_edits
    ↓
review_changes → verify_project → evidence_status
```

输入已经明确时优先用 `search_many`、`read_files`，只有互不依赖的工作才
并行。`agent_context` 先给紧凑上下文，`symbol_context` 补语法细节，
`apply_edits` 让一组不重叠编辑共用同一个 SHA 前置条件。

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

Semantic Provider Trust 与上述 Command Label 分离。未进入 Automatic
Profile 的 Warm LSP 使用绑定 Workspace + Provider + 当前 Provider Binary
Identity 的 `RiskyExecution`；批准后 Refresh/Navigation 可以复用这一份
Provider，但替换后的 Binary、其他 Provider 或无关 Repository Operation
都不会继承旧 Grant。

## 9. 排查方法

- 运行 `agent-plugin --install-all --dry-run --json`，查看客户端的
  `evidence`、`target` 和 `guidance`。
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
  还是 Semantic Provider Session，再在正确 Workspace 只批准对应请求。

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
