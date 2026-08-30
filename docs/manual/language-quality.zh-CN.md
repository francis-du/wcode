---
layout: docs
title: 语言质量模型
description: wcode 的显式语言质量能力矩阵与 Provider 规则
lang: zh-CN
alternate: /docs/language-quality/
permalink: /zh/docs/language-quality/
---

# 语言质量能力模型

wcode 不用一个 Boolean 表示“语言支持”。一个仓库可能能解析但没有 Semantic Server，有 Formatter 但没有 Type Checker，有 Unit Test 但没有 Mutation/Fuzz Runner，或者机器上安装了某个工具但仓库从未声明它。它们是不同状态，必须分别可观测。

## 规范语言面

质量模型复用 Syntax / Semantic 层同一套 22 种规范语言：

Bash、C、C++、C#、CSS、Dart、Elixir、Go、HTML、Java、JavaScript、Lua、OCaml、OCaml Interface、PHP、Python、R、Ruby、Rust、Swift、TypeScript、TSX。

不存在另一份独立 Quality Language Registry。新增或删除索引语言时，必须更新统一语言面与对应测试，而不是再维护一份列表。

## 能力向量

对每种检测到的语言，`language_quality_status` 分别报告：

- `syntax`：Tree-sitter 解析与导航；
- `semantic`：当前 Provider Binary 对应的第一方 LSP Session 已真实完成 Initialize；仅有 Executable 不够；
- `format`：仓库声明或语言原生的 Check-only 格式 Provider；
- `lint`：仓库声明的 Lint Provider；
- `type_check`：仓库声明或语言原生 Type Checker；
- `static_analysis`：更深层 Static Analysis；
- `test`：仓库声明或语言原生 Test Provider；
- `security`：仓库声明的 Security Analyzer；
- `property`、`mutation`、`fuzz`、`runtime_canary`：高级 Verification Mesh Stage。

缺失维度作为显式 `gaps` 返回。Parser 或 LSP Candidate 不会自动把其他能力升级成“已支持”。

Semantic Provider State 被明确拆成多层：`available` 表示存在 Executable Candidate；`launch_ready` 表示 Workspace Execution、Semantic Policy 与 Provider-specific Trust 已允许启动；`session_validated` 表示这一份 Provider Binary Identity 已真实完成 LSP `initialize`；只有到这一步 Semantic `runnable` 才为 true。`semantic_provider_status` 直接暴露这些字段，`language_quality_status.semantic_runnable` 消费的是完成验证后的结果。

## Provider 状态

一个 Quality Provider 分别携带：

- `declared`：仓库通过 Manifest、Dependency、Config 或原生项目约定声明它；
- `available`：可执行程序实际存在；
- `runnable`：Workspace 允许执行且 Provider 本身可执行；
- `authorization_required`：已经可运行，但仍需要正常的 Repository-aware 授权；
- `check_only`：注册命令不会改写源码；
- `machine_format`：当 Provider 有已知结构化输出时记录格式。

安装状态、仓库意图、执行策略和工具语义因此保持分离。

## 仓库意图优先于 wcode 默认值

wcode 不应跨生态强推一套 Formatter/Linter。Discovery 优先读取仓库证据：

- Manifest 与 Dependency Declaration；
- `lint`、`typecheck`、`test`、`format:check` 等 Package Manager Script；
- `Cargo.toml`、`go.mod`、`pubspec.yaml`、`mix.exs`、`Package.swift`、Maven/Gradle、Dune、.NET Solution/Project 等原生项目文件；
- `.clang-format`、`.clang-tidy`、Biome/ESLint/Stylelint、`.rubocop.yml`、`.swift-format`、PHPStan/Psalm、`.ocamlformat`、`.shellcheckrc`、StyLua/Luacheck 等显式 Quality Config。

已知生态工具可以作为 Candidate 出现，但 Candidate 不等于 Repository Policy。仓库未声明时，`language_quality_run` 不会把它当成可执行质量门。Package Script 是重要声明信号，但 Script 名字本身不能证明 Body 是 Check-only；无法静态保证不会修改源码的脚本只参与 Discovery，不进入 Strict Lane。

## 当前 Provider 家族

Registry 能识别的 Check-only Provider 包括：

| 语言家族 | 可识别 Quality Provider 示例 |
| --- | --- |
| Rust | rustfmt、Clippy、`cargo check`、`cargo test`、可选 cargo-audit |
| Go | gofmt diff、`go vet`、`go test`、可选 Staticcheck/govulncheck |
| Python | Ruff format/lint、mypy/Pyright、pytest、声明后的 Bandit |
| JS / TS / TSX | Repository Package Script、Biome、ESLint、`tsc --noEmit` |
| CSS / HTML | Repository Package Script、Biome、CSS 的 Stylelint |
| C / C++ | clang-format dry-run、带 Compilation Database 的 clang-tidy |
| C# | `dotnet format --verify-no-changes`、Build/Analyzer、Test |
| Java | Maven/Gradle Lifecycle + 声明后的 Checkstyle/SpotBugs/Spotless |
| Dart | `dart format` Check Mode、`dart analyze`、`dart test` |
| Elixir | Mix Format/Test + 声明后的 Credo/Dialyzer |
| Bash | 声明后的 ShellCheck/shfmt |
| Lua | 声明后的 StyLua、Luacheck、Busted |
| OCaml | Dune Build/Runtest 与 ocamlformat `@fmt` |
| PHP | PHPStan、Psalm、PHPUnit、PHP CS Fixer Dry-run |
| R | 声明后的 lintr、testthat |
| Ruby | 声明后的 RuboCop/RSpec |
| Swift | SwiftPM Test + 声明后的 swift-format/SwiftLint |

这张表只描述 Registry 能力，不代表当前主机一定可用。某个 Workspace 的事实来源始终是 `language_quality_status`。

## Check-only 执行

`language_quality_run` 刻意比任意命令执行更窄：

1. 仓库中必须检测到该语言；
2. Provider 必须来自 Registry；
3. 仓库必须声明 Provider；
4. 可执行程序必须可用；
5. Provider 必须注册为 Check-only；
6. Repository-aware 执行仍走正常 Trusted Runtime 授权边界；
7. Formatter/Fixer 的写源码模式不暴露给这个 Lane。

真实命令结果会转成 `VerificationReport`，并作为当前 code+design Revision 的 Evidence 持久化。历史 Pass 不能证明后续 Revision。

## 与 Verification Mesh 的关系

Language Quality 负责常见仓库质量门。Property、Mutation、Fuzz、Runtime-Canary 继续是 Verification Mesh 中 Provider-neutral 的高级 Stage，可来自内置 Discovery 或 `.wcode/executors.yaml`。

因此即使高级 Stage 缺失，语言能力仍应完整可观测。缺失是需要解决的 Gap，不是把语言标成“不支持”或伪造 Passing Stage 的理由。

## Operator 界面

- `project_context` 把能力矩阵放进项目上下文；
- `language_quality_status` 通过 MCP 暴露完整 Registry；
- `language_quality_run` 运行一个声明后的 Check-only Provider 并记录 Evidence；
- Project Observatory 显示检测到的语言与 Capability/Gap Matrix；
- `verification_executor_status` 继续负责 Property/Mutation/Fuzz/Runtime 高级 Registry。

目标是让人和 Agent 共用一套事实模型，而不是每个 IDE 各自维护一套“语言支持”故事。
