use crate::quality_provider::{
    QualityCandidate, QualityCapability, QualityProviderSource, RepoSignals,
};
use crate::semantic_provider::SemanticLanguage;
use crate::workspace::Workspace;
use crate::{quality_catalog_extended, quality_catalog_web};
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
            covers: Vec::new(),
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
                ["clippy", "--all-targets", "--", "-D", "warnings"],
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
            let mut go_test = candidate!(
                "go-test",
                Test,
                LanguageNative,
                "go",
                ["test", "./..."],
                declared,
                "go.mod",
                None,
            );
            go_test.covers.push(TypeCheck);
            candidates.push(go_test);
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
            let ruff =
                python.contains("ruff") || signals.has("ruff.toml") || signals.has(".ruff.toml");
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
                python.contains("mypy")
                    || signals.has("mypy.ini")
                    || signals.has(".mypy.ini")
                    || signals.any_contains(&["setup.cfg", "tox.ini"], "[mypy]"),
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
                python.contains("pytest")
                    || signals.has("pytest.ini")
                    || signals.any_contains(&["setup.cfg", "tox.ini"], "pytest"),
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
            quality_catalog_web::add_package_script_candidates(workspace, signals, &mut candidates);
            quality_catalog_web::add_web_tooling(workspace, signals, language, &mut candidates);
            quality_catalog_web::add_deno_tooling(workspace, signals, language, &mut candidates);
        }
        SemanticLanguage::Css | SemanticLanguage::Html => {
            quality_catalog_web::add_package_script_candidates(workspace, signals, &mut candidates);
            quality_catalog_web::add_web_tooling(workspace, signals, language, &mut candidates);
        }
        SemanticLanguage::Dart => {
            let declared = signals.has("pubspec.yaml");
            let flutter = signals.contains("pubspec.yaml", "sdk: flutter");
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
            if flutter {
                let mut flutter_analyze = candidate!(
                    "flutter-analyze",
                    StaticAnalysis,
                    LanguageNative,
                    "flutter",
                    ["analyze", "--no-pub"],
                    true,
                    "pubspec.yaml declares Flutter SDK",
                    None,
                );
                flutter_analyze.covers.extend([Lint, TypeCheck]);
                candidates.push(flutter_analyze);
                candidates.push(candidate!(
                    "flutter-test",
                    Test,
                    LanguageNative,
                    "flutter",
                    ["test", "--no-pub"],
                    true,
                    "pubspec.yaml declares Flutter SDK",
                    None,
                ));
            } else {
                let mut dart_analyze = candidate!(
                    "dart-analyze",
                    StaticAnalysis,
                    LanguageNative,
                    "dart",
                    ["analyze"],
                    declared,
                    "pubspec.yaml",
                    None,
                );
                dart_analyze.covers.extend([Lint, TypeCheck]);
                candidates.push(dart_analyze);
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
                "mix-compile",
                StaticAnalysis,
                LanguageNative,
                "mix",
                ["compile", "--warnings-as-errors"],
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
            let mut clang_tidy = candidate_owned!(
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
            );
            clang_tidy.covers.push(TypeCheck);
            candidates.push(clang_tidy);
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
            let mut dotnet_build = candidate!(
                "dotnet-build",
                StaticAnalysis,
                LanguageNative,
                "dotnet",
                ["build", "--no-restore"],
                declared,
                ".NET solution/project with compiler and Roslyn analyzers",
                None,
            );
            dotnet_build.covers.push(TypeCheck);
            candidates.push(dotnet_build);
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

fn add_java_candidates(
    workspace: &Workspace,
    signals: &RepoSignals,
    candidates: &mut Vec<QualityCandidate>,
) {
    use QualityCapability::{Format, Lint, StaticAnalysis, Test, TypeCheck};
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
        let program = if workspace.workspace_program_available("./mvnw") {
            "./mvnw"
        } else {
            "mvn"
        };
        let mut maven_compile = candidate!(
            "maven-compile",
            StaticAnalysis,
            QualityProviderSource::LanguageNative,
            program,
            ["-q", "-DskipTests", "compile"],
            true,
            "pom.xml",
            None,
        );
        maven_compile.covers.push(TypeCheck);
        candidates.push(maven_compile);
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
        let program = if workspace.workspace_program_available("./gradlew") {
            "./gradlew"
        } else {
            "gradle"
        };
        let mut gradle_classes = candidate!(
            "gradle-classes",
            StaticAnalysis,
            QualityProviderSource::LanguageNative,
            program,
            ["classes"],
            true,
            "Gradle build",
            None,
        );
        gradle_classes.covers.push(TypeCheck);
        candidates.push(gradle_classes);
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
