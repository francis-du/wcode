---
layout: wiki
title: Language Quality
description: Explicit language quality capabilities and provider rules
permalink: /wiki/language-quality/
---

# Language Quality Capability Model

wcode does not represent language support as one boolean.

A repository can be parseable but have no semantic server, have a formatter but no type checker, have unit tests but no mutation/fuzz runner, or have a quality tool installed that the repository never declared. Those states are materially different and must remain observable.

## Canonical language surface

The quality model reuses the same canonical 22-language surface as the syntax and semantic layers:

Bash, C, C++, C#, CSS, Dart, Elixir, Go, HTML, Java, JavaScript, Lua, OCaml, OCaml Interface, PHP, Python, R, Ruby, Rust, Swift, TypeScript, and TSX.

There is no separate quality-language registry. Adding or removing an indexed language must update the canonical language surface and its tests rather than creating another independent list.

## Capability vector

For every detected language, `language_quality_status` reports independent dimensions:

- `syntax` — Tree-sitter parsing/navigation is available;
- `semantic` — a real first-party LSP provider is installed/runnable;
- `format` — a repository-declared or language-native check-only formatting provider exists;
- `lint` — a repository-declared lint provider exists;
- `type_check` — a repository-declared/native type checker exists;
- `static_analysis` — deeper static analysis is available;
- `test` — a repository-declared/native test provider exists;
- `security` — a repository-declared security analyzer exists;
- `property`, `mutation`, `fuzz`, `runtime_canary` — advanced Verification Mesh stages.

Missing dimensions are returned as explicit `gaps`. A parser or LSP candidate never upgrades the other dimensions automatically.

## Provider state

A quality provider carries separate state:

- `declared` — the repository opted into it through a manifest, dependency, config file, or native project convention;
- `available` — the executable is actually available;
- `runnable` — the workspace permits command execution and the provider is otherwise executable;
- `authorization_required` — execution is ready but still needs the normal repository-aware authorization grant;
- `check_only` — the registered command must not rewrite source;
- `machine_format` — a known structured output format, when the provider invocation exposes one.

This keeps installation, repository intent, execution policy, and tool semantics separate.

## Repository intent outranks wcode defaults

wcode should not impose one formatter or linter across ecosystems. Discovery prefers repository evidence:

- manifests and dependency declarations;
- package-manager scripts such as `lint`, `typecheck`, `test`, and `format:check` as repository-declared quality intent;
- native project files such as `Cargo.toml`, `go.mod`, `pubspec.yaml`, `mix.exs`, `Package.swift`, Maven/Gradle projects, Dune projects, and .NET solutions/projects;
- explicit quality configuration such as `.clang-format`, `.clang-tidy`, Biome/ESLint/Stylelint, `.rubocop.yml`, `.swift-format`, PHPStan/Psalm, `.ocamlformat`, `.shellcheckrc`, StyLua/Luacheck, etc.

Known ecosystem tools may appear as candidates without being `declared`. A candidate is not treated as repository policy and cannot run through `language_quality_run` until the repository declares it. Repository package scripts are first-class discovery signals, but arbitrary script bodies are **not** automatically marked `check_only`; a script name such as `lint` or `format:check` cannot prove that the body will not mutate source. Those providers remain discovery-only in the strict lane unless wcode can statically guarantee the concrete command shape.

## Current provider families

The registry can recognize check-only providers across the canonical surface, including:

| Language family | Examples of recognized quality providers |
| --- | --- |
| Rust | rustfmt, Clippy, `cargo check`, `cargo test`, optional repository-declared cargo-audit |
| Go | gofmt diff, `go vet`, `go test`, optional Staticcheck/govulncheck |
| Python | Ruff format/lint, mypy/Pyright, pytest, Bandit when declared |
| JS / TS / TSX | repository package scripts, Biome, ESLint, `tsc --noEmit` |
| CSS / HTML | repository package scripts, Biome, Stylelint for CSS |
| C / C++ | clang-format dry-run, clang-tidy with compilation database |
| C# | `dotnet format --verify-no-changes`, build/analyzers, test |
| Java | Maven/Gradle lifecycle plus declared Checkstyle/SpotBugs/Spotless |
| Dart | `dart format` check mode, `dart analyze`, `dart test` |
| Elixir | Mix format/test plus declared Credo/Dialyzer |
| Bash | declared ShellCheck/shfmt |
| Lua | StyLua, Luacheck, Busted when declared |
| OCaml | Dune build/runtest and `@fmt` with ocamlformat |
| PHP | PHPStan, Psalm, PHPUnit, PHP CS Fixer dry-run |
| R | lintr, testthat when declared |
| Ruby | RuboCop/RSpec when declared |
| Swift | SwiftPM tests plus declared swift-format/SwiftLint |

This table describes registry capability, not host availability. `language_quality_status` is the source of truth for one workspace.

## Check-only execution

`language_quality_run` is intentionally narrower than arbitrary command execution:

1. the language must be detected in the repository;
2. the provider must come from the registry;
3. the repository must declare the provider;
4. the executable must be available;
5. the provider must be registered as check-only;
6. repository-aware execution still passes through the normal trusted-runtime authorization boundary;
7. source-writing formatter/fixer modes are not exposed through this lane.

The command result becomes a `VerificationReport`, and wcode persists the result as current code+design revision Evidence. A historical pass does not prove a later revision.

## Relationship to Verification Mesh

Language Quality covers common repository quality gates. Property, Mutation, Fuzz, and Runtime-Canary remain provider-neutral advanced stages in the Verification Mesh and can come from built-in discovery or `.wcode/executors.yaml`.

A language therefore remains fully observable even when advanced-stage coverage is missing. Missing coverage is a gap to resolve, not a reason to claim the language is unsupported or to fabricate a passing stage.

## Operator surfaces

- `project_context` includes the matrix so coding agents see repository-native quality expectations before editing;
- `language_quality_status` exposes the complete registry over MCP;
- `language_quality_run` runs one declared check-only provider and records Evidence;
- Project Observatory renders detected languages and the capability/gap matrix;
- `verification_executor_status` remains the advanced Property/Mutation/Fuzz/Runtime registry.

The goal is one shared fact model for humans and agents rather than separate IDE-specific quality stories.
