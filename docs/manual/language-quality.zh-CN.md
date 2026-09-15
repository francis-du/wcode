---
layout: docs
title: 语言质量模型
description: wcode 的显式语言质量能力矩阵与 Provider 规则
lang: zh-CN
alternate: /docs/language-quality/
permalink: /zh/docs/language-quality/
---

# 语言质量能力模型

wcode 不用一个 Boolean 表示“语言支持”。一个仓库可能能解析但没有 LSP Server，有 Formatter 但没有 Type Checker，有 Unit Test 但没有 Mutation/Fuzz Runner，或者机器上安装了某个工具但仓库从未声明它。它们是不同状态，必须分别可观测。

## 规范语言面

质量模型复用 Syntax / Semantic 层同一套 22 种规范语言：

Bash、C、C++、C#、CSS、Dart、Elixir、Go、HTML、Java、JavaScript、Lua、OCaml、OCaml Interface、PHP、Python、R、Ruby、Rust、Swift、TypeScript、TSX。

不存在另一份独立 Quality Language Registry。新增或删除索引语言时，必须更新统一语言面与对应测试，而不是再维护一份列表。

## 能力向量

对每种检测到的语言，`language_quality_status` 分别报告：

- `syntax`：Tree-sitter 解析与导航；
- `semantic`：当前 LSP Server Binary 对应的 Session 已真实完成 Initialize；仅有 Executable 不够；
- `format`：仓库声明或语言原生、真实可用且 Check-only 的格式 Provider；
- `lint`：仓库声明、真实可用且 Check-only 的 Lint Provider；
- `type_check`：仓库声明或语言原生、真实可用且 Check-only 的 Type Checker；
- `static_analysis`：仓库声明或语言原生、真实可用且 Check-only 的更深层 Static Analysis；
- `test`：仓库声明或语言原生、真实可用且 Check-only 的 Test Provider；
- `security`：仓库声明、真实可用且 Check-only 的 Security Analyzer；
- `property`、`mutation`、`fuzz`、`runtime_canary`：高级 Verification Mesh Stage。

缺失维度作为显式 `gaps` 返回。Parser、Executable Candidate 或 Discovery-only Package Script 都不会自动把其他能力升级成“已支持”。

同一个真实命令可以合理覆盖多个 Dimension，但不会为填表重复执行。Provider 有一个 Primary `capability`，并可以声明有界的 `covers`。例如 `dart analyze` 只执行一次，但同时提供 Lint、Type Check 与 Static Analysis Coverage；PHPStan／Psalm 与 Native Compiler／Build Check 也可以同时覆盖 Static Analysis 和 Type Check。

LSP 状态被明确拆成多层：`available` 表示已经发现 LSP 可执行文件；`launch_ready` 表示执行策略和授权允许启动；`session_validated` 表示当前 Server Binary 已真实完成 LSP `initialize`；只有到这一步 Semantic `runnable` 才为 true。`semantic_provider_status` 还会给出明确的下一步 `action`，例如 `install_lsp`、`authorize_lsp`、`initialize_lsp`；`language_quality_status.semantic_runnable` 消费的是完成验证后的结果。

## Provider 状态

一个 Quality Provider 分别携带：

- `declared`：仓库通过 Manifest、Dependency、Config 或原生项目约定声明它；
- `available`：可执行程序实际存在；
- `runnable`：Workspace 允许执行且 Provider 本身可执行；
- `authorization_required`：这个 Registry Provider 是否还需要额外批准。当前固定 Shape 的内置质量 Provider 为 false；任意 Repository Script 保持 Discovery-only，不会靠这个字段偷偷进入执行 Lane；
- `check_only`：注册命令不会改写源码；
- `machine_format`：当 Provider 有已知结构化输出时记录格式。

安装状态、仓库意图、执行策略和工具语义因此保持分离。

## 仓库意图优先于 wcode 默认值

wcode 不应跨生态强推一套 Formatter/Linter。Discovery 优先读取仓库证据：

- Manifest 与 Dependency Declaration；
- `lint`、`typecheck`、`test`、`format:check` 等 Package Manager Script；
- `Cargo.toml`、`go.mod`、`pubspec.yaml`、`mix.exs`、`Package.swift`、Maven/Gradle、Dune、.NET Solution/Project 等原生项目文件；
- `.clang-format`、`.clang-tidy`、Biome/ESLint/Stylelint、`.rubocop.yml`、`.swift-format`、PHPStan/Psalm、`.ocamlformat`、`.shellcheckrc`、StyLua/Luacheck 等显式 Quality Config。

已知生态工具可以作为 Candidate 出现，但 Candidate 不等于 Repository Policy。仓库未声明时，`language_quality_run` 不会把它当成可执行质量门。Package Script 是重要声明信号，但 `lint`、`test`、`format:check` 这样的 Script 名字本身不能证明 Body 是 Check-only；无法静态保证不会修改源码的脚本只参与 Discovery，不进入 Strict Lane，也不会让矩阵的 Covered 数量变绿。

R 的内置 Inline Quality Check 只允许固定的 `Rscript --vanilla -e <known-expression>` 形态，覆盖 lintr、styler Dry-fail Format 与 testthat，避免 Workspace／用户 Startup Profile 静默变成自治质量检查的一部分。Ruby 侧，RuboCop 继续只作为 Lint Provider；Registry 不再把同一个 RuboCop Lint 命令重复标成 Format Coverage。只有仓库显式声明 Standard Ruby 时，它才提供组合 Lint／Format Coverage，而且默认检查命令不会应用 Fix。

## 当前 Provider 家族

Registry 能识别的 Check-only Provider 包括：

| 语言家族 | 可识别 Quality Provider 示例 |
| --- | --- |
| Rust | rustfmt、Clippy、`cargo check`、`cargo test`、可选 cargo-audit |
| Go | gofmt diff、`go vet`、`go test`（Test + Type Check Coverage）、可选 Staticcheck/govulncheck |
| Python | 依赖或 `ruff.toml` / `.ruff.toml` 声明的 Ruff Format/Lint、标准配置文件声明的 mypy、Pyright、pytest，以及声明后的 Bandit |
| JS / TS / TSX | Prettier Check、Biome/ESLint、`tsc --noEmit`、固定 Vitest/Jest Runner；Deno 项目额外提供原生 Fmt/Lint、Frozen Check/Test 与 Frozen Dependency Audit |
| CSS / HTML | Prettier；CSS 的 Stylelint 与具备对应插件的 ESLint/Biome；HTML 还支持 HTMLHint，ESLint/Biome 只有在 HTML 插件／实验支持被显式启用时才算覆盖 |
| C / C++ | clang-format Dry-run；带 Compilation Database 的 clang-tidy 同时提供 Static Analysis + Type Check，因为编译器语义错误属于其不可关闭诊断 |
| C# | `dotnet format --verify-no-changes`、Build/Analyzer（Static + Type Check Coverage）、Test |
| Java | Maven Compile / Gradle Classes（Type Check + Static Coverage）、Test，以及声明后的 Checkstyle/SpotBugs/Spotless；不可执行 Wrapper 不会遮住系统 Maven/Gradle |
| Dart / Flutter | 纯 Dart 使用 `dart format` + `dart analyze` + `dart test`；Flutter 保留 Dart Format，但使用 `flutter analyze --no-pub` 与 `flutter test --no-pub` |
| Elixir | Mix Format/Test + 声明后的 Credo/Dialyzer |
| Bash | 声明后的 ShellCheck/shfmt |
| Lua | 声明后的 StyLua、Luacheck、Busted |
| OCaml | Dune Build/Runtest 与 ocamlformat `@fmt` |
| PHP | PHPStan／Psalm（Type Check + Static Coverage）、PHPUnit、PHP CS Fixer Dry-run，以及基于 Lockfile 的 Composer Advisory Audit |
| R | 声明后的 styler Dry-fail Format、lintr、testthat；内置表达式统一使用 `Rscript --vanilla` |
| Ruby | RuboCop Lint、Standard Ruby 组合 Lint/Format Check、声明后的 RSpec |
| Swift | `swift build`（Type Check + Static Coverage）、SwiftPM Test、声明后的 swift-format/SwiftLint |

Deno 的依赖解析型 Verification 统一使用 `--frozen`，避免检查时静默刷新 `deno.lock`；`deno audit --fix` 会改 Dependency Declaration 与 Lockfile，因此不进入 Check-only Lane。PHP 仓库同时存在 `composer.json` 与 `composer.lock` 时，只暴露固定的 `composer audit --locked --format=json` Security Shape；Composer 会从项目配置的 Repository 获取 Advisory 数据，因此这不是纯离线检查。Workspace 内可执行文件与 Java Wrapper 只有通过 Runtime 同一套 Regular-file / Single-link / Executable 检查后才算 Available。

这张表只描述 Registry 能力，不代表当前主机一定可用。某个 Workspace 的事实来源始终是 `language_quality_status`。

## Check-only 执行

`language_quality_run` 刻意比任意命令执行更窄：

1. 仓库中必须检测到该语言；
2. Provider 必须来自 Registry；
3. 仓库必须声明 Provider；
4. 可执行程序必须可用；
5. Provider 必须注册为 Check-only；
6. Provider 在当前 Workspace 中必须真实 Runnable（包括命令执行没有被禁用）；
7. 精确获批的命令形态进入自治 Verification/Development Lane；其他已注册 Check-only Provider 进入有界 Trusted Runtime Lane，不虚构额外授权流程；
8. Formatter/Fixer 的写源码模式不暴露给这个 Lane。

真实命令结果会转成 `VerificationReport`，并作为当前 code+design Revision 的 Evidence 持久化。历史 Pass 不能证明后续 Revision。

## 与 Verification Mesh 的关系

Language Quality 负责常见仓库质量门。Property、Mutation、Fuzz、Runtime-Canary 继续是 Verification Mesh 中 Provider-neutral 的高级 Stage，可来自保守的内置 Discovery 或 `.wcode/executors.yaml`。

内置 Property Discovery 现在同时要求 Repository 声明对应框架，并且在**相同语言源码**里真实观察到框架使用；JS/TS 还必须有固定 Vitest/Jest Runner，任意 `test` Script 不再能生成 Property Evidence。JS/TS Mutation 不再根据 `mutation` / `mutate` Script 名或可执行 Stryker 配置自动推导：这些脚本／配置本身可以执行 Repository JavaScript，因此在出现可证明的有界 Adapter 前，Mutation 保持显式 Executor 配置。其他生态的 Mutation Tool 也只有在对应项目类型真实存在时才暴露。

因此即使高级 Stage 缺失，语言能力仍应完整可观测。缺失是需要解决的 Gap，不是把语言标成“不支持”或伪造 Passing Stage 的理由。

## Operator 界面

- `project_context` 把能力矩阵放进项目上下文；
- `language_quality_status` 通过 MCP 暴露完整 Registry；
- `language_quality_run` 运行一个声明后的 Check-only Provider 并记录 Evidence；
- Engineering Observatory 显示检测到的语言与 Capability/Gap Matrix；
- `verification_executor_status` 继续负责 Property/Mutation/Fuzz/Runtime 高级 Registry。

目标是让人和 Agent 共用一套事实模型，而不是每个 IDE 各自维护一套“语言支持”故事。
