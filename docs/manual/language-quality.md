---
layout: docs
title: Language Quality
description: Explicit language quality capabilities and provider rules
lang: en
alternate: /zh/docs/language-quality/
permalink: /docs/language-quality/
---

# Language Quality Capability Model

wcode does not represent language support as one boolean.

A repository can be parseable but have no LSP server, have a formatter but no type checker, have unit tests but no mutation/fuzz runner, or have a quality tool installed that the repository never declared. Those states are materially different and must remain observable.

## Canonical language surface

The quality model reuses the same canonical 22-language surface as the syntax and semantic layers:

Bash, C, C++, C#, CSS, Dart, Elixir, Go, HTML, Java, JavaScript, Lua, OCaml, OCaml Interface, PHP, Python, R, Ruby, Rust, Swift, TypeScript, and TSX.

There is no separate quality-language registry. Adding or removing an indexed language must update the canonical language surface and its tests rather than creating another independent list.

## Capability vector

For every detected language, `language_quality_status` reports independent dimensions:

- `syntax` — Tree-sitter parsing/navigation is available;
- `semantic` — a first-party LSP session for the current provider binary has completed real initialization; executable presence alone is not enough;
- `format` — a repository-declared or language-native, available, check-only formatting provider exists;
- `lint` — a repository-declared, available, check-only lint provider exists;
- `type_check` — a repository-declared/native, available, check-only type checker exists;
- `static_analysis` — deeper repository-declared/native, available, check-only static analysis is available;
- `test` — a repository-declared/native, available, check-only test provider exists;
- `security` — a repository-declared, available, check-only security analyzer exists;
- `property`, `mutation`, `fuzz`, `runtime_canary` — advanced Verification Mesh stages.

Missing dimensions are returned as explicit `gaps`. A parser, executable candidate, or discovery-only package script never upgrades another dimension automatically.

One real command may legitimately cover more than one dimension without being executed twice. A provider has one primary `capability` plus bounded `covers` dimensions. For example, `dart analyze` is one static-analysis invocation that also supplies lint and type-check coverage; PHPStan/Psalm and native compiler/build checks can similarly cover both static analysis and type checking.

LSP state is deliberately staged: `available` means an LSP executable was found; `launch_ready` means execution policy and trust allow it to start; `session_validated` means that exact server binary completed LSP `initialize`; only then does semantic `runnable` become true. `semantic_provider_status` exposes these fields plus a concrete `action` such as `install_lsp`, `authorize_lsp`, or `initialize_lsp`. `language_quality_status.semantic_runnable` consumes the validated result.

## Provider state

A quality provider carries separate state:

- `declared` — the repository opted into it through a manifest, dependency, config file, or native project convention;
- `available` — the executable is actually available;
- `runnable` — the workspace permits command execution and the provider is otherwise executable;
- `authorization_required` — whether this registered provider still requires a separate approval step. Exact built-in quality shapes currently report false; arbitrary repository scripts remain discovery-only instead of being smuggled through this flag;
- `check_only` — the registered command must not rewrite source;
- `machine_format` — a known structured output format, when the provider invocation exposes one.

This keeps installation, repository intent, execution policy, and tool semantics separate.

## Repository intent outranks wcode defaults

wcode should not impose one formatter or linter across ecosystems. Discovery prefers repository evidence:

- manifests and dependency declarations;
- package-manager scripts such as `lint`, `typecheck`, `test`, and `format:check` as repository-declared quality intent;
- native project files such as `Cargo.toml`, `go.mod`, `pubspec.yaml`, `mix.exs`, `Package.swift`, Maven/Gradle projects, Dune projects, and .NET solutions/projects;
- explicit quality configuration such as `.clang-format`, `.clang-tidy`, Biome/ESLint/Stylelint, `.rubocop.yml`, `.swift-format`, PHPStan/Psalm, `.ocamlformat`, `.shellcheckrc`, StyLua/Luacheck, etc.

Known ecosystem tools may appear as candidates without being `declared`. A candidate is not treated as repository policy and cannot run through `language_quality_run` until the repository declares it. Repository package scripts are first-class discovery signals, but arbitrary script bodies are **not** automatically marked `check_only`; a script name such as `lint`, `test`, or `format:check` cannot prove that the body will not mutate source. Discovery-only providers therefore do not satisfy the matrix's covered count unless wcode can statically guarantee the concrete command shape.

For R, built-in inline quality checks use only the fixed `Rscript --vanilla -e <known-expression>` forms for lintr, styler dry-fail formatting, and testthat. This prevents workspace/user startup profiles from silently becoming part of an autonomous quality check. For Ruby, RuboCop remains a lint provider; the registry no longer labels the same RuboCop lint command as formatting coverage. Standard Ruby supplies combined lint/format coverage only when the repository explicitly declares it, and its default check command does not apply fixes.

## Current provider families

The registry can recognize check-only providers across the canonical surface, including:

| Language family | Examples of recognized quality providers |
| --- | --- |
| Rust | rustfmt, Clippy, `cargo check`, `cargo test`, optional repository-declared cargo-audit |
| Go | gofmt diff, `go vet`, `go test` (test + type-check coverage), optional Staticcheck/govulncheck |
| Python | Ruff format/lint via dependencies or `ruff.toml`/`.ruff.toml`, mypy via standard config files, Pyright, pytest, Bandit when declared |
| JS / TS / TSX | Prettier check, Biome/ESLint, `tsc --noEmit`, fixed Vitest/Jest runners; Deno projects add native fmt/lint/frozen check/test plus frozen dependency audit |
| CSS / HTML | Prettier; CSS Stylelint and plugin-aware ESLint/Biome; HTMLHint plus HTML-specific ESLint/Biome only when the required language plugin/experimental support is explicitly present |
| C / C++ | clang-format dry-run; clang-tidy with a compilation database contributes static-analysis + type-check coverage because compiler semantic errors are part of its diagnostics |
| C# | `dotnet format --verify-no-changes`, build/analyzers (static + type-check coverage), test |
| Java | Maven compile / Gradle classes (type-check + static coverage), tests, plus declared Checkstyle/SpotBugs/Spotless; an unusable wrapper does not shadow a system Maven/Gradle binary |
| Dart / Flutter | pure Dart uses `dart format` + `dart analyze` + `dart test`; Flutter projects keep Dart format but use `flutter analyze --no-pub` and `flutter test --no-pub` |
| Elixir | Mix format/test plus declared Credo/Dialyzer |
| Bash | declared ShellCheck/shfmt |
| Lua | StyLua, Luacheck, Busted when declared |
| OCaml | Dune build/runtest and `@fmt` with ocamlformat |
| PHP | PHPStan/Psalm (type-check + static coverage), PHPUnit, PHP CS Fixer dry-run, and locked Composer advisory audit |
| R | styler dry-fail formatting, lintr, testthat when declared; built-in expressions use `Rscript --vanilla` |
| Ruby | RuboCop lint, Standard Ruby combined lint/format check, RSpec when declared |
| Swift | `swift build` (type-check + static coverage), SwiftPM tests, declared swift-format/SwiftLint |

Deno dependency-resolving verification uses `--frozen` so a check cannot silently refresh `deno.lock`; `deno audit --fix` stays outside the check-only lane because it rewrites dependency declarations and the lockfile. PHP repositories with both `composer.json` and `composer.lock` expose only the fixed `composer audit --locked --format=json` security shape; Composer obtains advisory data from the repositories configured for that project, so this is not an offline-only check. Workspace-local executables and Java build wrappers count as available only when they satisfy the same regular-file/single-link/executable checks used by the runtime.

This table describes registry capability, not host availability. `language_quality_status` is the source of truth for one workspace.

## Check-only execution

`language_quality_run` is intentionally narrower than arbitrary command execution:

1. the language must be detected in the repository;
2. the provider must come from the registry;
3. the repository must declare the provider;
4. the executable must be available;
5. the provider must be registered as check-only;
6. the provider must be runnable in the current Workspace (including command execution being enabled);
7. exact approved shapes use the autonomous verification/development lane; other registered check-only providers use the bounded trusted-runtime lane without inventing a separate approval workflow;
8. source-writing formatter/fixer modes are not exposed through this lane.

The command result becomes a `VerificationReport`, and wcode persists the result as current code+design revision Evidence. A historical pass does not prove a later revision.

## Relationship to Verification Mesh

Language Quality covers common repository quality gates. Property, Mutation, Fuzz, and Runtime-Canary remain provider-neutral advanced stages in the Verification Mesh and can come from conservative built-in discovery or `.wcode/executors.yaml`.

![wcode Verification Mesh](/assets/wcode-verification-mesh.svg)

Built-in Property discovery requires both repository framework declaration and evidence that the framework is actually referenced from source of the matching language. JS/TS Property discovery additionally requires a known fixed Vitest/Jest runner; arbitrary `test` scripts are not Property evidence. JS/TS mutation is not inferred from `mutation`/`mutate` script names or executable Stryker configuration: because those scripts/configs can execute repository JavaScript, mutation remains an explicit executor configuration unless a future bounded adapter can prove its command contract. Mutation tools for other ecosystems are exposed only when the corresponding project type is present.

A language therefore remains fully observable even when advanced-stage coverage is missing. Missing coverage is a gap to resolve, not a reason to claim the language is unsupported or to fabricate a passing stage.

## Operator surfaces

- `project_context` includes the matrix so coding agents see repository-native quality expectations before editing;
- `language_quality_status` exposes the complete registry over MCP;
- `language_quality_run` runs one declared check-only provider and records Evidence;
- Engineering Observatory renders detected languages and the capability/gap matrix;
- `verification_executor_status` remains the advanced Property/Mutation/Fuzz/Runtime registry.

The goal is one shared fact model for humans and agents rather than separate IDE-specific quality stories.
