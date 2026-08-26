use crate::quality_catalog_extended;
use crate::quality_provider::{
    QualityCandidate, QualityCapability, QualityProviderSource, RepoSignals,
};
use crate::semantic_provider::SemanticLanguage;
use crate::workspace::Workspace;
use std::fs;

const MAX_COMMAND_FILES: usize = 128;

macro_rules! candidate_owned {
    (
        $id:expr,
        $capability:expr,
        $source:expr,
        $program:expr,
        $args:expr,
        $declared:expr,
        $declaration:expr,
        $machine_format:expr,
        $fail_on_stdout:expr $(,)?
    ) => {{
        QualityCandidate {
            id: ($id).into(),
            capability: $capability,
            source: $source,
            program: ($program).into(),
            args: $args,
            declared: $declared,
            check_only: true,
            fail_on_stdout: $fail_on_stdout,
            machine_format: $machine_format,
            declaration: ($declaration).into(),
        }
    }};
}
pub(crate) use candidate_owned;

macro_rules! candidate {
    (
        $id:expr,
        $capability:expr,
        $source:expr,
        $program:expr,
        $args:expr,
        $declared:expr,
        $declaration:expr,
        $machine_format:expr $(,)?
    ) => {{
        candidate_owned!(
            $id,
            $capability,
            $source,
            $program,
            ($args)
                .into_iter()
                .map(|argument| argument.to_owned())
                .collect::<Vec<String>>(),
            $declared,
            $declaration,
            $machine_format,
            false,
        )
    }};
}
pub(crate) use candidate;

pub(crate) fn candidates_for(
    workspace: &Workspace,
    signals: &RepoSignals,
    language: SemanticLanguage,
    language_files: &[String],
) -> Vec<QualityCandidate> {
    use QualityCapability::{Format, Lint, Security, StaticAnalysis, Test, TypeCheck};
    use QualityProviderSource::{Ecosystem, LanguageNative};
    let mut candidates = Vec::new();
    let files = language_files
        .iter()
        .take(MAX_COMMAND_FILES)
        .cloned()
        .collect::<Vec<_>>();
    match language {
        SemanticLanguage::Rust => {
            let declared = signals.has("Cargo.toml");
            candidates.push(candidate!(
                "rustfmt",
                Format,
                LanguageNative,
                "cargo",
                ["fmt", "--check"],
                declared,
                "Cargo.toml",
                None,
            ));
            candidates.push(candidate!(
                "rust-clippy",
                Lint,
                LanguageNative,
                "cargo",
                ["clippy", "--", "-D", "warnings"],
                declared,
                "Cargo.toml",
                None,
            ));
            candidates.push(candidate!(
                "rust-check",
                TypeCheck,
                LanguageNative,
                "cargo",
                ["check"],
                declared,
                "Cargo.toml",
                None,
            ));
            candidates.push(candidate!(
                "rust-test",
                Test,
                LanguageNative,
                "cargo",
                ["test"],
                declared,
                "Cargo.toml",
                None,
            ));
            candidates.push(candidate_owned!(
                "cargo-audit",
                Security,
                Ecosystem,
                "cargo-audit",
                Vec::new(),
                signals.any_contains(&["Cargo.toml", "Cargo.lock", "Makefile"], "cargo-audit")
                    || workspace.root().join(".cargo/audit.toml").is_file(),
                "cargo-audit repository configuration",
                Some("json"),
                false,
            ));
        }
        SemanticLanguage::Go => {
            let declared = signals.has("go.mod");
            candidates.push(candidate_owned!(
                "gofmt",
                Format,
                LanguageNative,
                "gofmt",
                std::iter::once("-d".to_owned())
                    .chain(files.clone())
                    .collect(),
                declared,
                "go.mod",
                None,
                true,
            ));
            candidates.push(candidate!(
                "go-vet",
                StaticAnalysis,
                LanguageNative,
                "go",
                ["vet", "./..."],
                declared,
                "go.mod",
                None,
            ));
            candidates.push(candidate!(
                "go-test",
                Test,
                LanguageNative,
                "go",
                ["test", "./..."],
                declared,
                "go.mod",
                None,
            ));
            candidates.push(candidate!(
                "staticcheck",
                Lint,
                Ecosystem,
                "staticcheck",
                ["./..."],
                signals.any_contains(&["go.mod", "Makefile"], "staticcheck"),
                "staticcheck in repository configuration",
                None,
            ));
            candidates.push(candidate!(
                "govulncheck",
                Security,
                Ecosystem,
                "govulncheck",
                ["./..."],
                signals.any_contains(&["go.mod", "Makefile"], "govulncheck"),
                "govulncheck in repository configuration",
                None,
            ));
        }
        SemanticLanguage::Python => {
            let python = format!(
                "{}\n{}",
                signals
                    .text
                    .get("pyproject.toml")
                    .cloned()
                    .unwrap_or_default(),
                signals
                    .text
                    .get("requirements.txt")
                    .cloned()
                    .unwrap_or_default()
            )
            .to_ascii_lowercase();
            let ruff = python.contains("ruff");
            candidates.push(candidate!(
                "ruff-format",
                Format,
                Ecosystem,
                "ruff",
                ["format", "--check", "."],
                ruff,
                "Ruff dependency/configuration",
                None,
            ));
            candidates.push(candidate!(
                "ruff-check",
                Lint,
                Ecosystem,
                "ruff",
                ["check", "--output-format", "json", "."],
                ruff,
                "Ruff dependency/configuration",
                Some("json"),
            ));
            candidates.push(candidate!(
                "mypy",
                TypeCheck,
                Ecosystem,
                "mypy",
                ["."],
                python.contains("mypy"),
                "mypy dependency/configuration",
                None,
            ));
            candidates.push(candidate!(
                "pyright",
                TypeCheck,
                Ecosystem,
                "pyright",
                ["--outputjson", "."],
                python.contains("pyright") || workspace.root().join("pyrightconfig.json").is_file(),
                "Pyright dependency/configuration",
                Some("json"),
            ));
            candidates.push(candidate!(
                "pytest",
                Test,
                Ecosystem,
                "pytest",
                ["-q"],
                python.contains("pytest") || workspace.root().join("pytest.ini").is_file(),
                "pytest dependency/configuration",
                None,
            ));
            candidates.push(candidate!(
                "bandit",
                Security,
                Ecosystem,
                "bandit",
                ["-r", ".", "-f", "json"],
                python.contains("bandit") || workspace.root().join(".bandit").is_file(),
                "Bandit dependency/configuration",
                Some("json"),
            ));
        }
        SemanticLanguage::JavaScript | SemanticLanguage::TypeScript | SemanticLanguage::Tsx => {
            add_package_script_candidates(workspace, signals, &mut candidates);
            add_web_tooling(workspace, signals, language, &mut candidates);
        }
        SemanticLanguage::Css | SemanticLanguage::Html => {
            add_package_script_candidates(workspace, signals, &mut candidates);
            add_web_tooling(workspace, signals, language, &mut candidates);
        }
        SemanticLanguage::Dart => {
            let declared = signals.has("pubspec.yaml");
            candidates.push(candidate!(
                "dart-format",
                Format,
                LanguageNative,
                "dart",
                ["format", "-o", "none", "--set-exit-if-changed", "."],
                declared,
                "pubspec.yaml",
                None,
            ));
            candidates.push(candidate!(
                "dart-analyze",
                StaticAnalysis,
                LanguageNative,
                "dart",
                ["analyze"],
                declared,
                "pubspec.yaml",
                None,
            ));
            candidates.push(candidate!(
                "dart-test",
                Test,
                LanguageNative,
                "dart",
                ["test"],
                declared,
                "pubspec.yaml",
                None,
            ));
        }
        SemanticLanguage::Elixir => {
            let declared = signals.has("mix.exs");
            candidates.push(candidate!(
                "mix-format",
                Format,
                LanguageNative,
                "mix",
                ["format", "--check-formatted"],
                declared,
                "mix.exs",
                None,
            ));
            candidates.push(candidate!(
                "mix-test",
                Test,
                LanguageNative,
                "mix",
                ["test"],
                declared,
                "mix.exs",
                None,
            ));
            candidates.push(candidate!(
                "credo",
                Lint,
                Ecosystem,
                "mix",
                ["credo", "--strict"],
                signals.contains("mix.exs", "credo"),
                "Credo dependency in mix.exs",
                None,
            ));
            candidates.push(candidate!(
                "dialyzer",
                StaticAnalysis,
                Ecosystem,
                "mix",
                ["dialyzer"],
                signals.contains("mix.exs", "dialyxir"),
                "Dialyxir dependency in mix.exs",
                None,
            ));
        }
        SemanticLanguage::C | SemanticLanguage::Cpp => {
            let mut format_args = vec!["--dry-run".to_owned(), "--Werror".to_owned()];
            format_args.extend(files.clone());
            candidates.push(candidate_owned!(
                "clang-format",
                Format,
                Ecosystem,
                "clang-format",
                format_args,
                signals.has(".clang-format"),
                ".clang-format",
                None,
                false,
            ));
            let mut tidy_args = files;
            tidy_args.extend(["-p".into(), ".".into()]);
            candidates.push(candidate_owned!(
                "clang-tidy",
                StaticAnalysis,
                Ecosystem,
                "clang-tidy",
                tidy_args,
                signals.has(".clang-tidy")
                    && workspace.root().join("compile_commands.json").is_file(),
                ".clang-tidy plus compile_commands.json",
                None,
                false,
            ));
        }
        SemanticLanguage::CSharp => {
            let declared = has_root_extension(workspace, &["sln", "csproj", "fsproj"]);
            candidates.push(candidate!(
                "dotnet-format",
                Format,
                LanguageNative,
                "dotnet",
                ["format", "--verify-no-changes", "--no-restore"],
                declared,
                ".NET solution/project",
                None,
            ));
            candidates.push(candidate!(
                "dotnet-build",
                StaticAnalysis,
                LanguageNative,
                "dotnet",
                ["build", "--no-restore"],
                declared,
                ".NET solution/project with Roslyn analyzers",
                None,
            ));
            candidates.push(candidate!(
                "dotnet-test",
                Test,
                LanguageNative,
                "dotnet",
                ["test", "--no-restore"],
                declared,
                ".NET solution/project",
                None,
            ));
        }
        SemanticLanguage::Java => add_java_candidates(workspace, signals, &mut candidates),
        SemanticLanguage::Bash
        | SemanticLanguage::Lua
        | SemanticLanguage::Ocaml
        | SemanticLanguage::OcamlInterface
        | SemanticLanguage::Php
        | SemanticLanguage::R
        | SemanticLanguage::Ruby
        | SemanticLanguage::Swift => {
            quality_catalog_extended::add_candidates(
                workspace,
                signals,
                language,
                &files,
                &mut candidates,
            );
        }
    }
    candidates
}

fn add_web_tooling(
    workspace: &Workspace,
    signals: &RepoSignals,
    language: SemanticLanguage,
    candidates: &mut Vec<QualityCandidate>,
) {
    use QualityCapability::{Format, Lint, TypeCheck};
    use QualityProviderSource::Ecosystem;
    let biome = signals.package_dependency("@biomejs/biome")
        || signals.has("biome.json")
        || signals.has("biome.jsonc");
    let biome_program = node_program(workspace, "biome");
    candidates.push(candidate_owned!(
        "biome-format",
        Format,
        Ecosystem,
        biome_program.clone(),
        vec!["check".into(), ".".into(), "--reporter=json".into()],
        biome,
        "Biome dependency/configuration",
        Some("json"),
        false,
    ));
    candidates.push(candidate_owned!(
        "biome-check",
        Lint,
        Ecosystem,
        biome_program,
        vec!["check".into(), ".".into(), "--reporter=json".into()],
        biome,
        "Biome dependency/configuration",
        Some("json"),
        false,
    ));
    let eslint = signals.package_dependency("eslint")
        || [
            "eslint.config.js",
            "eslint.config.mjs",
            ".eslintrc",
            ".eslintrc.json",
        ]
        .iter()
        .any(|path| signals.has(path));
    candidates.push(candidate_owned!(
        "eslint",
        Lint,
        Ecosystem,
        node_program(workspace, "eslint"),
        vec![".".into(), "--format".into(), "json".into()],
        eslint,
        "ESLint dependency/configuration",
        Some("json"),
        false,
    ));
    if matches!(
        language,
        SemanticLanguage::TypeScript | SemanticLanguage::Tsx
    ) {
        candidates.push(candidate_owned!(
            "tsc-no-emit",
            TypeCheck,
            Ecosystem,
            node_program(workspace, "tsc"),
            vec!["--noEmit".into()],
            signals.package_dependency("typescript") && signals.has("tsconfig.json"),
            "TypeScript dependency and tsconfig.json",
            None,
            false,
        ));
    }
    if language == SemanticLanguage::Css {
        let stylelint = signals.package_dependency("stylelint")
            || workspace.root().join("stylelint.config.js").is_file()
            || workspace.root().join(".stylelintrc").is_file();
        candidates.push(candidate_owned!(
            "stylelint",
            Lint,
            Ecosystem,
            node_program(workspace, "stylelint"),
            vec!["**/*.css".into(), "--formatter".into(), "json".into()],
            stylelint,
            "Stylelint dependency/configuration",
            Some("json"),
            false,
        ));
    }
}

fn add_package_script_candidates(
    workspace: &Workspace,
    signals: &RepoSignals,
    candidates: &mut Vec<QualityCandidate>,
) {
    use QualityCapability::{Format, Lint, StaticAnalysis, Test, TypeCheck};
    for (script, capability) in [
        ("format:check", Format),
        ("lint", Lint),
        ("typecheck", TypeCheck),
        ("check", StaticAnalysis),
        ("test", Test),
    ] {
        if signals.package_script(script) {
            let (program, args) = package_run_command(workspace, script);
            let mut provider = candidate_owned!(
                format!("package-{script}"),
                capability,
                QualityProviderSource::RepositoryConfigured,
                program,
                args,
                true,
                format!("package.json script `{script}`"),
                None,
                false,
            );
            // Script names express repository quality intent, but their bodies are arbitrary
            // project-controlled execution. Discovery is safe; the strict check-only lane must
            // not infer non-mutation from a name such as `lint` or `format:check`.
            provider.check_only = false;
            candidates.push(provider);
        }
    }
}

fn add_java_candidates(
    workspace: &Workspace,
    signals: &RepoSignals,
    candidates: &mut Vec<QualityCandidate>,
) {
    use QualityCapability::{Format, Lint, StaticAnalysis, Test};
    let maven = signals.has("pom.xml");
    let gradle = signals.has("build.gradle") || signals.has("build.gradle.kts");
    let build_text = format!(
        "{}\n{}\n{}",
        signals.text.get("pom.xml").cloned().unwrap_or_default(),
        signals
            .text
            .get("build.gradle")
            .cloned()
            .unwrap_or_default(),
        signals
            .text
            .get("build.gradle.kts")
            .cloned()
            .unwrap_or_default()
    )
    .to_ascii_lowercase();
    if maven {
        let program = if workspace.root().join("mvnw").is_file() {
            "./mvnw"
        } else {
            "mvn"
        };
        candidates.push(candidate!(
            "maven-test",
            Test,
            QualityProviderSource::LanguageNative,
            program,
            ["test"],
            true,
            "pom.xml",
            None,
        ));
        candidates.push(candidate!(
            "maven-checkstyle",
            Lint,
            QualityProviderSource::RepositoryConfigured,
            program,
            ["checkstyle:check"],
            build_text.contains("checkstyle"),
            "Checkstyle plugin in pom.xml",
            None,
        ));
        candidates.push(candidate!(
            "maven-spotbugs",
            StaticAnalysis,
            QualityProviderSource::RepositoryConfigured,
            program,
            ["spotbugs:check"],
            build_text.contains("spotbugs"),
            "SpotBugs plugin in pom.xml",
            None,
        ));
        candidates.push(candidate!(
            "maven-spotless",
            Format,
            QualityProviderSource::RepositoryConfigured,
            program,
            ["spotless:check"],
            build_text.contains("spotless"),
            "Spotless plugin in pom.xml",
            None,
        ));
    } else if gradle {
        let program = if workspace.root().join("gradlew").is_file() {
            "./gradlew"
        } else {
            "gradle"
        };
        candidates.push(candidate!(
            "gradle-test",
            Test,
            QualityProviderSource::LanguageNative,
            program,
            ["test"],
            true,
            "Gradle build",
            None,
        ));
        candidates.push(candidate!(
            "gradle-check",
            StaticAnalysis,
            QualityProviderSource::RepositoryConfigured,
            program,
            ["check"],
            true,
            "Gradle check lifecycle",
            None,
        ));
        candidates.push(candidate!(
            "gradle-spotless",
            Format,
            QualityProviderSource::RepositoryConfigured,
            program,
            ["spotlessCheck"],
            build_text.contains("spotless"),
            "Spotless plugin in Gradle build",
            None,
        ));
    }
}

fn node_program(workspace: &Workspace, name: &str) -> String {
    let relative = format!("node_modules/.bin/{name}");
    if workspace.root().join(&relative).is_file() {
        relative
    } else {
        name.to_owned()
    }
}

fn package_run_command(workspace: &Workspace, script: &str) -> (String, Vec<String>) {
    if workspace.root().join("pnpm-lock.yaml").is_file() {
        ("pnpm".into(), vec!["run".into(), script.into()])
    } else if workspace.root().join("yarn.lock").is_file() {
        ("yarn".into(), vec!["run".into(), script.into()])
    } else if workspace.root().join("bun.lock").is_file()
        || workspace.root().join("bun.lockb").is_file()
    {
        ("bun".into(), vec!["run".into(), script.into()])
    } else {
        ("npm".into(), vec!["run".into(), script.into()])
    }
}

fn has_root_extension(workspace: &Workspace, extensions: &[&str]) -> bool {
    fs::read_dir(workspace.root()).is_ok_and(|entries| {
        entries.filter_map(|entry| entry.ok()).any(|entry| {
            entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extensions.contains(&extension))
        })
    })
}
