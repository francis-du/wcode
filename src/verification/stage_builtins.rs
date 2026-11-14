use super::*;

pub(super) fn discover_builtins(workspace: &Workspace) -> Result<Vec<StageExecutorSpec>> {
    let mut executors = Vec::new();
    let framework_queries = [
        "proptest",
        "quickcheck",
        "testing/quick",
        "pgregory.net/rapid",
        "gopter",
        "hypothesis",
        "fast-check",
        "SwiftCheck",
        "StreamData",
        "ExUnitProperties",
        "glados",
        "Rantly",
        "property_of",
        "Eris",
        "QCheck",
        "FsCheck",
        "jqwik",
        "QuickTheories",
        "quicktheories",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    let framework_hits = workspace.search_language_matches(&framework_queries, ".")?;

    let cargo = optional_text(workspace, "Cargo.toml")?.unwrap_or_default();
    if contains_any(&cargo, &["proptest", "quickcheck"])
        && framework_used(
            &framework_hits,
            &["proptest", "quickcheck"],
            &[SemanticLanguage::Rust],
        )
    {
        executors.push(spec(
            "builtin-rust-property",
            VerificationStage::Property,
            &[SemanticLanguage::Rust],
            "cargo",
            &["test", "--locked"],
            180,
        ));
    }
    if !cargo.is_empty() && find_executable("cargo-mutants").is_some() {
        executors.push(spec(
            "builtin-rust-mutation",
            VerificationStage::Mutation,
            &[SemanticLanguage::Rust],
            "cargo-mutants",
            &[],
            300,
        ));
    }
    if let Some(target) = first_rust_fuzz_target(workspace)? {
        if find_executable("cargo-fuzz").is_some() {
            executors.push(spec_owned(
                "builtin-rust-fuzz",
                VerificationStage::Fuzz,
                vec![SemanticLanguage::Rust],
                "cargo-fuzz",
                vec![
                    "fuzz".into(),
                    "run".into(),
                    target,
                    "--".into(),
                    "-max_total_time=5".into(),
                ],
                60,
            ));
        }
    }

    let go_mod = optional_text(workspace, "go.mod")?.unwrap_or_default();
    if !go_mod.is_empty()
        && framework_used(
            &framework_hits,
            &["testing/quick", "pgregory.net/rapid", "gopter"],
            &[SemanticLanguage::Go],
        )
    {
        executors.push(spec(
            "builtin-go-property",
            VerificationStage::Property,
            &[SemanticLanguage::Go],
            "go",
            &["test", "./..."],
            180,
        ));
    }
    if !go_mod.is_empty() {
        if let Some(fuzz_name) = first_go_fuzz_target(workspace)? {
            executors.push(spec_owned(
                "builtin-go-fuzz",
                VerificationStage::Fuzz,
                vec![SemanticLanguage::Go],
                "go",
                vec![
                    "test".into(),
                    "-run=^$".into(),
                    format!("-fuzz={fuzz_name}"),
                    "-fuzztime=5s".into(),
                    "./...".into(),
                ],
                60,
            ));
        }
    }

    let python = format!(
        "{}\n{}",
        optional_text(workspace, "pyproject.toml")?.unwrap_or_default(),
        optional_text(workspace, "requirements.txt")?.unwrap_or_default()
    );
    if python.to_ascii_lowercase().contains("hypothesis")
        && framework_used(
            &framework_hits,
            &["hypothesis"],
            &[SemanticLanguage::Python],
        )
    {
        executors.push(spec(
            "builtin-python-property",
            VerificationStage::Property,
            &[SemanticLanguage::Python],
            "pytest",
            &["-q"],
            180,
        ));
    }
    if find_executable("mutmut").is_some() && !python.is_empty() {
        executors.push(spec(
            "builtin-python-mutation",
            VerificationStage::Mutation,
            &[SemanticLanguage::Python],
            "mutmut",
            &["run"],
            300,
        ));
    }

    if let Some(package) = optional_text(workspace, "package.json")? {
        let package: serde_json::Value = serde_json::from_str(&package).unwrap_or_default();
        let languages = [
            SemanticLanguage::JavaScript,
            SemanticLanguage::TypeScript,
            SemanticLanguage::Tsx,
        ];
        if package_has_dependency(&package, "fast-check")
            && framework_used(&framework_hits, &["fast-check"], &languages)
        {
            let runner = if package_has_dependency(&package, "vitest") {
                Some((node_stage_program(workspace, "vitest"), vec!["run".into()]))
            } else if package_has_dependency(&package, "jest") {
                Some((
                    node_stage_program(workspace, "jest"),
                    vec!["--runInBand".into()],
                ))
            } else {
                None
            };
            if let Some((program, args)) = runner {
                executors.push(spec_owned(
                    "builtin-js-property",
                    VerificationStage::Property,
                    languages.to_vec(),
                    &program,
                    args,
                    180,
                ));
            }
        }
        // Do not infer mutation execution from an arbitrary package script name.
        // Stryker configuration may itself execute repository JavaScript, so JS/TS
        // mutation remains an explicit .wcode/executors.yaml trust decision.
    }

    for (path, dependency, source_needles, id, language, program, args) in [
        (
            "Package.swift",
            "SwiftCheck",
            &["SwiftCheck"][..],
            "builtin-swift-property",
            SemanticLanguage::Swift,
            "swift",
            vec!["test"],
        ),
        (
            "mix.exs",
            "stream_data",
            &["StreamData", "ExUnitProperties"][..],
            "builtin-elixir-property",
            SemanticLanguage::Elixir,
            "mix",
            vec!["test"],
        ),
        (
            "pubspec.yaml",
            "glados",
            &["glados"][..],
            "builtin-dart-property",
            SemanticLanguage::Dart,
            "dart",
            vec!["test"],
        ),
        (
            "Gemfile",
            "rantly",
            &["Rantly", "property_of"][..],
            "builtin-ruby-property",
            SemanticLanguage::Ruby,
            "bundle",
            vec!["exec", "rspec"],
        ),
        (
            "composer.json",
            "eris",
            &["Eris"][..],
            "builtin-php-property",
            SemanticLanguage::Php,
            "./vendor/bin/phpunit",
            vec![],
        ),
        (
            "dune-project",
            "qcheck",
            &["QCheck"][..],
            "builtin-ocaml-property",
            SemanticLanguage::Ocaml,
            "dune",
            vec!["runtest"],
        ),
        (
            "DESCRIPTION",
            "quickcheck",
            &["quickcheck"][..],
            "builtin-r-property",
            SemanticLanguage::R,
            "Rscript",
            vec!["--vanilla", "-e", "testthat::test_dir('tests/testthat')"],
        ),
    ] {
        if optional_text(workspace, path)?.is_some_and(|text| {
            text.to_ascii_lowercase()
                .contains(&dependency.to_ascii_lowercase())
        }) && framework_used(&framework_hits, source_needles, &[language])
        {
            executors.push(spec_owned(
                id,
                VerificationStage::Property,
                vec![language],
                program,
                args.into_iter().map(str::to_owned).collect(),
                180,
            ));
        }
    }

    if framework_used(&framework_hits, &["FsCheck"], &[SemanticLanguage::CSharp]) {
        executors.push(spec(
            "builtin-csharp-property",
            VerificationStage::Property,
            &[SemanticLanguage::CSharp],
            "dotnet",
            &["test"],
            180,
        ));
    }
    let java_build = format!(
        "{}\n{}\n{}",
        optional_text(workspace, "pom.xml")?.unwrap_or_default(),
        optional_text(workspace, "build.gradle")?.unwrap_or_default(),
        optional_text(workspace, "build.gradle.kts")?.unwrap_or_default()
    );
    if contains_any(
        &java_build.to_ascii_lowercase(),
        &["jqwik", "quicktheories"],
    ) && framework_used(
        &framework_hits,
        &["jqwik", "QuickTheories", "quicktheories"],
        &[SemanticLanguage::Java],
    ) {
        if workspace.root().join("pom.xml").is_file() {
            let program = if workspace.workspace_program_available("./mvnw") {
                "./mvnw"
            } else {
                "mvn"
            };
            executors.push(spec(
                "builtin-java-property",
                VerificationStage::Property,
                &[SemanticLanguage::Java],
                program,
                &["test"],
                180,
            ));
        } else if workspace.root().join("build.gradle").is_file()
            || workspace.root().join("build.gradle.kts").is_file()
        {
            let program = if workspace.workspace_program_available("./gradlew") {
                "./gradlew"
            } else {
                "gradle"
            };
            executors.push(spec(
                "builtin-java-property",
                VerificationStage::Property,
                &[SemanticLanguage::Java],
                program,
                &["test"],
                180,
            ));
        }
    }
    if java_build.to_ascii_lowercase().contains("pitest")
        && workspace.root().join("pom.xml").is_file()
    {
        executors.push(spec(
            "builtin-java-mutation",
            VerificationStage::Mutation,
            &[SemanticLanguage::Java],
            "mvn",
            &["test-compile", "org.pitest:pitest-maven:mutationCoverage"],
            300,
        ));
    }
    if has_root_extension(workspace, &["sln", "csproj", "fsproj"])
        && find_executable("dotnet-stryker").is_some()
    {
        executors.push(spec(
            "builtin-csharp-mutation",
            VerificationStage::Mutation,
            &[SemanticLanguage::CSharp],
            "dotnet-stryker",
            &[],
            300,
        ));
    }
    if workspace.root().join("composer.json").is_file() && find_executable("infection").is_some() {
        executors.push(spec(
            "builtin-php-mutation",
            VerificationStage::Mutation,
            &[SemanticLanguage::Php],
            "infection",
            &["--no-progress"],
            300,
        ));
    }
    if workspace.root().join("Package.swift").is_file() && find_executable("muter").is_some() {
        executors.push(spec(
            "builtin-swift-mutation",
            VerificationStage::Mutation,
            &[SemanticLanguage::Swift],
            "muter",
            &[],
            300,
        ));
    }
    Ok(executors)
}
