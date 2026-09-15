use super::*;
use std::fs;

#[test]
fn registry_represents_every_indexed_language_without_a_fake_support_boolean() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let registry = registry(&workspace, None).unwrap();
    assert_eq!(registry.languages.len(), SemanticLanguage::ALL.len());
    assert_eq!(registry.detected_languages, 1);
    let rust = registry
        .languages
        .iter()
        .find(|status| status.language == SemanticLanguage::Rust)
        .unwrap();
    assert_eq!(rust.detected_files, 1);
    assert!(rust.syntax_available);
    assert!(rust
        .providers
        .iter()
        .any(|provider| { provider.id == "rustfmt" && provider.root == "." && provider.declared }));
    assert!(registry
        .dimensions
        .iter()
        .any(|dimension| dimension.dimension == "fuzz"));
}

#[test]
fn repository_scripts_are_first_class_quality_providers() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"lint":"eslint .","typecheck":"tsc --noEmit","test":"vitest run"},"devDependencies":{"typescript":"latest"}}"#,
        )
        .unwrap();
    fs::write(dir.path().join("tsconfig.json"), "{}\n").unwrap();
    fs::write(dir.path().join("index.ts"), "export const x: number = 1;\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let registry = registry(&workspace, None).unwrap();
    let typescript = registry
        .languages
        .iter()
        .find(|status| status.language == SemanticLanguage::TypeScript)
        .unwrap();
    assert!(typescript
        .providers
        .iter()
        .any(|provider| provider.id == "package-lint" && provider.declared));
    assert!(typescript
        .providers
        .iter()
        .any(|provider| provider.id == "package-typecheck" && provider.declared));
    let package_lint = typescript
        .providers
        .iter()
        .find(|provider| provider.id == "package-lint")
        .unwrap();
    assert!(package_lint.declared);
    assert!(!package_lint.check_only);
    assert!(package_lint.reason.contains("discovery only"));
    assert!(
        typescript.gaps.iter().any(|gap| gap.contains("lint")),
        "discovery-only package scripts must not satisfy strict quality coverage"
    );
    assert!(
        typescript.gaps.iter().any(|gap| gap.contains("test")),
        "discovery-only test scripts must remain explicit gaps"
    );
}

#[test]
fn polyglot_registry_scopes_nested_manifest_providers_to_their_islands() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
    fs::create_dir_all(dir.path().join("mobile/lib")).unwrap();
    fs::write(
        dir.path().join("mobile/pubspec.yaml"),
        "name: mobile\nenvironment:\n  sdk: '>=3.0.0 <4.0.0'\n",
    )
    .unwrap();
    fs::write(dir.path().join("mobile/lib/app.dart"), "void main() {}\n").unwrap();
    fs::create_dir_all(dir.path().join("flutter_app/lib")).unwrap();
    fs::write(
        dir.path().join("flutter_app/pubspec.yaml"),
        "name: flutter_app\ndependencies:\n  flutter:\n    sdk: flutter\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("flutter_app/lib/main.dart"),
        "void main() {}\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("deno_app")).unwrap();
    fs::write(dir.path().join("deno_app/deno.json"), "{}\n").unwrap();
    fs::write(
        dir.path().join("deno_app/main.ts"),
        "export const value = 1;\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("dotnet_app")).unwrap();
    fs::write(
        dir.path().join("dotnet_app/App.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk\"></Project>\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("dotnet_app/Program.cs"),
        "public static class Program { public static void Main() {} }\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("web/src")).unwrap();
    fs::write(
        dir.path().join("web/package.json"),
        r#"{"scripts":{"lint":"eslint ."},"devDependencies":{"eslint":"latest","typescript":"latest"}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("web/tsconfig.json"), "{}\n").unwrap();
    fs::write(
        dir.path().join("web/src/app.ts"),
        "export const value = 1;\n",
    )
    .unwrap();

    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let project_roots = vec![
        (".".to_owned(), vec!["rust".to_owned()]),
        ("mobile".to_owned(), vec!["dart".to_owned()]),
        ("flutter_app".to_owned(), vec!["flutter".to_owned()]),
        ("deno_app".to_owned(), vec!["deno".to_owned()]),
        ("dotnet_app".to_owned(), vec!["dotnet-csharp".to_owned()]),
        ("web".to_owned(), vec!["node".to_owned()]),
    ];
    let registry = registry_for_project_roots(&workspace, None, &project_roots).unwrap();
    assert_eq!(registry.provider, "wcode-language-quality-islands");
    let dart = registry
        .languages
        .iter()
        .find(|status| status.language == SemanticLanguage::Dart)
        .unwrap();
    assert!(dart.providers.iter().any(|provider| {
        provider.id == "mobile::dart-analyze" && provider.root == "mobile" && provider.declared
    }));
    assert!(dart.providers.iter().any(|provider| {
        provider.id == "flutter_app::flutter-analyze"
            && provider.root == "flutter_app"
            && provider.declared
    }));
    assert!(dart.providers.iter().any(|provider| {
        provider.id == "flutter_app::flutter-test"
            && provider.root == "flutter_app"
            && provider.declared
    }));
    let typescript = registry
        .languages
        .iter()
        .find(|status| status.language == SemanticLanguage::TypeScript)
        .unwrap();
    assert!(typescript.providers.iter().any(|provider| {
        provider.id == "deno_app::deno-check" && provider.root == "deno_app" && provider.declared
    }));
    assert!(typescript.providers.iter().any(|provider| {
        provider.id == "web::package-lint" && provider.root == "web" && provider.declared
    }));
    let csharp = registry
        .languages
        .iter()
        .find(|status| status.language == SemanticLanguage::CSharp)
        .unwrap();
    assert!(csharp.providers.iter().any(|provider| {
        provider.id == "dotnet_app::dotnet-build"
            && provider.root == "dotnet_app"
            && provider.declared
    }));
    let rust = registry
        .languages
        .iter()
        .find(|status| status.language == SemanticLanguage::Rust)
        .unwrap();
    assert!(rust
        .providers
        .iter()
        .any(|provider| provider.id == "rustfmt" && provider.root == "."));
    assert_eq!(
        scoped_provider_id("mobile::dart-analyze"),
        ("mobile", "dart-analyze")
    );
    assert_eq!(scoped_provider_id("rustfmt"), (".", "rustfmt"));
}

#[cfg(unix)]
#[test]
fn unsafe_workspace_bin_links_do_not_shadow_bare_quality_tools() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"devDependencies":{"eslint":"^9"}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("app.js"), "const value = 1;\n").unwrap();
    fs::create_dir_all(dir.path().join("node_modules/.bin")).unwrap();
    fs::write(dir.path().join("eslint-real"), "fixture\n").unwrap();
    symlink(
        dir.path().join("eslint-real"),
        dir.path().join("node_modules/.bin/eslint"),
    )
    .unwrap();
    fs::write(
        dir.path().join("composer.json"),
        r#"{"require-dev":{"phpstan/phpstan":"^2"}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("index.php"), "<?php echo 1;\n").unwrap();
    fs::create_dir_all(dir.path().join("vendor/bin")).unwrap();
    fs::write(dir.path().join("phpstan-real"), "fixture\n").unwrap();
    symlink(
        dir.path().join("phpstan-real"),
        dir.path().join("vendor/bin/phpstan"),
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let signals = RepoSignals::load(&workspace);
    let js = crate::quality_catalog::candidates_for(
        &workspace,
        &signals,
        SemanticLanguage::JavaScript,
        &["app.js".into()],
    );
    assert_eq!(
        js.iter()
            .find(|candidate| candidate.id == "eslint")
            .unwrap()
            .program,
        "eslint",
        "an unsafe node_modules/.bin symlink must not hide a bare PATH provider"
    );
    let php = crate::quality_catalog::candidates_for(
        &workspace,
        &signals,
        SemanticLanguage::Php,
        &["index.php".into()],
    );
    assert_eq!(
        php.iter()
            .find(|candidate| candidate.id == "phpstan")
            .unwrap()
            .program,
        "phpstan",
        "an unsafe Composer bin link must not hide a bare PATH provider"
    );
}

#[test]
fn safe_single_link_workspace_quality_tools_remain_preferred() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"devDependencies":{"eslint":"^9"}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("app.js"), "const value = 1;\n").unwrap();
    fs::create_dir_all(dir.path().join("node_modules/.bin")).unwrap();
    let eslint = dir.path().join("node_modules/.bin/eslint");
    fs::write(&eslint, "fixture\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&eslint).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&eslint, permissions).unwrap();
    }
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let signals = RepoSignals::load(&workspace);
    let js = crate::quality_catalog::candidates_for(
        &workspace,
        &signals,
        SemanticLanguage::JavaScript,
        &["app.js".into()],
    );
    assert_eq!(
        js.iter()
            .find(|candidate| candidate.id == "eslint")
            .unwrap()
            .program,
        "node_modules/.bin/eslint"
    );
}

#[test]
fn semantic_dimension_counts_only_initialized_runnable_sessions() {
    let mut language = LanguageQualityStatus {
        language: SemanticLanguage::Rust,
        detected_files: 1,
        syntax_available: true,
        semantic_provider: Some("rust-analyzer".into()),
        semantic_available: true,
        semantic_runnable: false,
        providers: Vec::new(),
        advanced_stages: Vec::new(),
        gaps: Vec::new(),
    };
    let semantic = |status: &LanguageQualityStatus| {
        dimension_coverage(std::slice::from_ref(status))
            .into_iter()
            .find(|dimension| dimension.dimension == "semantic")
            .unwrap()
            .covered_languages
    };
    assert_eq!(
        semantic(&language),
        0,
        "an installed executable is not live semantic coverage"
    );
    language.semantic_runnable = true;
    assert_eq!(semantic(&language), 1);
}

#[test]
fn quality_coverage_requires_runnable_provider_not_just_declared_binary() {
    let provider = QualityProviderStatus {
        id: "fixture".into(),
        root: ".".into(),
        capability: QualityCapability::Format,
        covers: vec![QualityCapability::Lint],
        source: QualityProviderSource::Ecosystem,
        program: "fixture".into(),
        command: "fixture --check".into(),
        declared: true,
        available: true,
        runnable: false,
        authorization_required: false,
        execution_lane: QualityExecutionLane::Unavailable,
        check_only: true,
        external_advisory_data: false,
        machine_format: None,
        reason: "command execution is disabled".into(),
    };
    let language = LanguageQualityStatus {
        language: SemanticLanguage::JavaScript,
        detected_files: 1,
        syntax_available: true,
        semantic_provider: None,
        semantic_available: false,
        semantic_runnable: false,
        providers: vec![provider.clone()],
        advanced_stages: Vec::new(),
        gaps: Vec::new(),
    };
    for dimension in ["format", "lint"] {
        let covered = dimension_coverage(std::slice::from_ref(&language))
            .into_iter()
            .find(|item| item.dimension == dimension)
            .unwrap()
            .covered_languages;
        assert_eq!(
            covered, 0,
            "{dimension} must not count an unrunnable provider"
        );
    }
    let gaps = quality_gaps(SemanticLanguage::JavaScript, true, &[provider]);
    assert!(gaps.iter().any(|gap| gap.contains("format")));
    assert!(gaps.iter().any(|gap| gap.contains("lint")));
}

#[test]
fn advisory_security_providers_report_external_data_and_real_machine_output() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::write(dir.path().join("Cargo.lock"), "version = 3\n").unwrap();
    fs::create_dir_all(dir.path().join(".cargo")).unwrap();
    fs::write(dir.path().join(".cargo/audit.toml"), "[advisories]\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let signals = RepoSignals::load(&workspace);
    let candidates = crate::quality_catalog::candidates_for(
        &workspace,
        &signals,
        SemanticLanguage::Rust,
        &["src/lib.rs".into()],
    );
    let audit = candidates
        .iter()
        .find(|candidate| candidate.id == "cargo-audit")
        .unwrap();
    assert_eq!(audit.args, ["--json"]);
    assert_eq!(audit.machine_format, Some("json"));
    assert!(audit.external_advisory_data);
}

#[test]
fn multi_capability_providers_report_real_shared_language_checks() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("go.mod"),
        "module example.test/demo\n\ngo 1.23\n",
    )
    .unwrap();
    fs::write(dir.path().join("main.go"), "package main\nfunc main() {}\n").unwrap();
    fs::write(
        dir.path().join("pubspec.yaml"),
        "name: demo\nenvironment:\n  sdk: '>=3.0.0 <4.0.0'\n",
    )
    .unwrap();
    fs::write(dir.path().join("main.dart"), "void main() {}\n").unwrap();
    fs::write(
        dir.path().join("Demo.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk\"></Project>\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("Program.cs"),
        "class Program { static void Main() {} }\n",
    )
    .unwrap();
    fs::write(dir.path().join("pom.xml"), "<project></project>\n").unwrap();
    fs::write(dir.path().join("Main.java"), "class Main {}\n").unwrap();
    fs::write(
        dir.path().join("composer.json"),
        r#"{"require-dev":{"phpstan/phpstan":"^2"}}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("index.php"),
        "<?php function value(): int { return 1; }\n",
    )
    .unwrap();
    fs::write(dir.path().join("Package.swift"), "// swift-tools-version: 6.0\nimport PackageDescription\nlet package = Package(name: \"Demo\")\n").unwrap();
    fs::write(dir.path().join("main.swift"), "print(\"hello\")\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let registry = registry(&workspace, None).unwrap();
    let provider = |language, id: &str| {
        registry
            .languages
            .iter()
            .find(|status| status.language == language)
            .and_then(|status| status.providers.iter().find(|provider| provider.id == id))
            .unwrap()
    };
    assert!(provider(SemanticLanguage::Go, "go-test")
        .covers
        .contains(&QualityCapability::TypeCheck));
    let dart = provider(SemanticLanguage::Dart, "dart-analyze");
    assert!(dart.covers.contains(&QualityCapability::Lint));
    assert!(dart.covers.contains(&QualityCapability::TypeCheck));
    assert!(provider(SemanticLanguage::CSharp, "dotnet-build")
        .covers
        .contains(&QualityCapability::TypeCheck));
    assert!(provider(SemanticLanguage::Java, "maven-compile")
        .covers
        .contains(&QualityCapability::TypeCheck));
    assert!(provider(SemanticLanguage::Php, "phpstan")
        .covers
        .contains(&QualityCapability::TypeCheck));
    assert!(provider(SemanticLanguage::Swift, "swift-build")
        .covers
        .contains(&QualityCapability::TypeCheck));
}

#[test]
fn clang_tidy_with_compile_database_covers_static_and_type_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".clang-tidy"), "Checks: '*'\n").unwrap();
    fs::write(dir.path().join("compile_commands.json"), "[]\n").unwrap();
    fs::write(dir.path().join("main.cpp"), "int main() { return 0; }\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let signals = RepoSignals::load(&workspace);
    let candidates = crate::quality_catalog::candidates_for(
        &workspace,
        &signals,
        SemanticLanguage::Cpp,
        &["main.cpp".into()],
    );
    let tidy = candidates
        .iter()
        .find(|candidate| candidate.id == "clang-tidy")
        .unwrap();
    assert!(tidy.declared);
    assert_eq!(tidy.capability, QualityCapability::StaticAnalysis);
    assert!(tidy.covers.contains(&QualityCapability::TypeCheck));
}

#[cfg(unix)]
#[test]
fn non_executable_java_wrappers_do_not_shadow_system_build_tools() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("pom.xml"), "<project></project>\n").unwrap();
    fs::write(dir.path().join("Main.java"), "class Main {}\n").unwrap();
    fs::write(dir.path().join("mvnw"), "#!/bin/sh\nexit 0\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let signals = RepoSignals::load(&workspace);
    let candidates = crate::quality_catalog::candidates_for(
        &workspace,
        &signals,
        SemanticLanguage::Java,
        &["Main.java".into()],
    );
    assert!(candidates
        .iter()
        .filter(|candidate| candidate.id.starts_with("maven-"))
        .all(|candidate| candidate.program == "mvn"));
}

#[test]
fn biome_format_and_lint_use_distinct_exact_check_only_commands() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"devDependencies":{"@biomejs/biome":"^2"}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("biome.json"), "{}\n").unwrap();
    fs::write(dir.path().join("app.js"), "const value = 1;\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let signals = RepoSignals::load(&workspace);
    let candidates = crate::quality_catalog::candidates_for(
        &workspace,
        &signals,
        SemanticLanguage::JavaScript,
        &["app.js".into()],
    );
    let format = candidates
        .iter()
        .find(|candidate| candidate.id == "biome-format")
        .unwrap();
    let lint = candidates
        .iter()
        .find(|candidate| candidate.id == "biome-check")
        .unwrap();
    assert_eq!(format.args, ["format", ".", "--reporter=json"]);
    assert_eq!(
        lint.args,
        [
            "check",
            ".",
            "--formatter-enabled=false",
            "--assist-enabled=false",
            "--reporter=json",
        ]
    );
    assert_ne!(
        format.args, lint.args,
        "format and lint must not duplicate the same full Biome pass"
    );
}

#[test]
fn web_quality_uses_fixed_prettier_vitest_and_jest_commands() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"devDependencies":{"prettier":"^3","vitest":"^4","jest":"^30","typescript":"^5"}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("tsconfig.json"), "{}\n").unwrap();
    fs::write(
        dir.path().join("app.ts"),
        "export const value: number = 1;\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let signals = RepoSignals::load(&workspace);
    let candidates = crate::quality_catalog::candidates_for(
        &workspace,
        &signals,
        SemanticLanguage::TypeScript,
        &["app.ts".into()],
    );
    let find = |id: &str| {
        candidates
            .iter()
            .find(|candidate| candidate.id == id)
            .unwrap()
    };
    assert_eq!(find("prettier-check").args, [".", "--check"]);
    assert_eq!(find("vitest-run").args, ["run"]);
    assert_eq!(find("jest-run").args, ["--runInBand"]);
    assert!(find("prettier-check").declared);
    assert!(find("vitest-run").declared);
    assert!(find("jest-run").declared);
    for id in ["prettier-check", "vitest-run", "jest-run"] {
        assert!(find(id).check_only);
    }
}

#[test]
fn deno_configuration_exposes_fixed_native_quality_providers() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("deno.json"),
        r#"{"lint":{},"fmt":{},"compilerOptions":{"strict":true}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("deno.lock"), "{}\n").unwrap();
    fs::write(
        dir.path().join("main.ts"),
        "export const value: number = 1;\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let registry = registry(&workspace, None).unwrap();
    let language = registry
        .languages
        .iter()
        .find(|status| status.language == SemanticLanguage::TypeScript)
        .unwrap();
    let provider = |id: &str| {
        language
            .providers
            .iter()
            .find(|provider| provider.id == id)
            .unwrap()
    };
    for (id, command) in [
        ("deno-fmt", "deno fmt --check"),
        ("deno-lint", "deno lint"),
        ("deno-check", "deno check --frozen ."),
        ("deno-test", "deno test --frozen"),
        ("deno-audit", "deno audit --frozen"),
    ] {
        let provider = provider(id);
        assert!(provider.declared && provider.check_only, "{id}");
        assert_eq!(provider.command, command, "{id}");
    }
    assert!(provider("deno-check")
        .covers
        .contains(&QualityCapability::StaticAnalysis));
}

#[test]
fn flutter_projects_use_flutter_analysis_and_tests_but_keep_dart_format() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("pubspec.yaml"),
        "name: demo\ndependencies:\n  flutter:\n    sdk: flutter\n",
    )
    .unwrap();
    fs::write(dir.path().join("main.dart"), "void main() {}\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let registry = registry(&workspace, None).unwrap();
    let language = registry
        .languages
        .iter()
        .find(|status| status.language == SemanticLanguage::Dart)
        .unwrap();
    let provider = |id: &str| language.providers.iter().find(|provider| provider.id == id);
    assert_eq!(
        provider("dart-format").unwrap().command,
        "dart format -o none --set-exit-if-changed ."
    );
    let analyze = provider("flutter-analyze").expect("Flutter projects need flutter analyze");
    assert_eq!(analyze.command, "flutter analyze --no-pub");
    assert!(analyze.covers.contains(&QualityCapability::Lint));
    assert!(analyze.covers.contains(&QualityCapability::TypeCheck));
    assert_eq!(
        provider("flutter-test").unwrap().command,
        "flutter test --no-pub"
    );
    assert!(provider("dart-analyze").is_none());
    assert!(provider("dart-test").is_none());
}

#[test]
fn web_language_plugins_and_biome_opt_ins_prevent_false_css_html_coverage() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"devDependencies":{"eslint":"^9","@biomejs/biome":"^2"}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("biome.json"), "{}\n").unwrap();
    fs::write(dir.path().join("style.css"), "body { color: black; }\n").unwrap();
    fs::write(dir.path().join("index.html"), "<main>Hello</main>\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let signals = RepoSignals::load(&workspace);
    let css = crate::quality_catalog::candidates_for(
        &workspace,
        &signals,
        SemanticLanguage::Css,
        &["style.css".into()],
    );
    let html = crate::quality_catalog::candidates_for(
        &workspace,
        &signals,
        SemanticLanguage::Html,
        &["index.html".into()],
    );
    let declared = |items: &[QualityCandidate], id: &str| {
        items
            .iter()
            .find(|candidate| candidate.id == id)
            .unwrap()
            .declared
    };
    assert!(!declared(&css, "eslint"));
    assert!(!declared(&css, "biome-format"));
    assert!(
        declared(&css, "biome-check"),
        "Biome CSS lint is supported without formatter opt-in"
    );
    assert!(!declared(&html, "eslint"));
    assert!(!declared(&html, "biome-format"));
    assert!(!declared(&html, "biome-check"));

    fs::write(
        dir.path().join("package.json"),
        r#"{"devDependencies":{"eslint":"^9","@eslint/css":"latest","@html-eslint/eslint-plugin":"latest","@biomejs/biome":"^2","htmlhint":"latest"}}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("biome.json"),
        r#"{"css":{"formatter":{"enabled":true}},"html":{"experimentalFullSupportEnabled":true,"formatter":{"enabled":true}}}"#,
    )
    .unwrap();
    let signals = RepoSignals::load(&workspace);
    let css = crate::quality_catalog::candidates_for(
        &workspace,
        &signals,
        SemanticLanguage::Css,
        &["style.css".into()],
    );
    let html = crate::quality_catalog::candidates_for(
        &workspace,
        &signals,
        SemanticLanguage::Html,
        &["index.html".into()],
    );
    assert!(declared(&css, "eslint"));
    assert!(declared(&css, "biome-format"));
    assert!(declared(&html, "eslint"));
    assert!(declared(&html, "biome-format"));
    assert!(declared(&html, "biome-check"));
    assert!(declared(&html, "htmlhint"));
}

#[test]
fn python_quality_detects_standard_tool_configuration_files() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("app.py"), "value: int = 1\n").unwrap();
    fs::write(dir.path().join("ruff.toml"), "line-length = 100\n").unwrap();
    fs::write(dir.path().join("mypy.ini"), "[mypy]\nstrict = True\n").unwrap();
    fs::write(dir.path().join("pytest.ini"), "[pytest]\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let registry = registry(&workspace, None).unwrap();
    let python = registry
        .languages
        .iter()
        .find(|status| status.language == SemanticLanguage::Python)
        .unwrap();
    for id in ["ruff-format", "ruff-check", "mypy", "pytest"] {
        assert!(
            python
                .providers
                .iter()
                .any(|provider| provider.id == id && provider.declared),
            "standard configuration must declare {id}"
        );
    }
}

#[test]
fn bats_files_provide_a_fixed_bash_test_provider() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("script.sh"), "echo ready\n").unwrap();
    fs::write(
        dir.path().join("smoke.bats"),
        "#!/usr/bin/env bats\n@test \"works\" { true; }\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let signals = RepoSignals::load(&workspace);
    let candidates = crate::quality_catalog::candidates_for(
        &workspace,
        &signals,
        SemanticLanguage::Bash,
        &["script.sh".into(), "smoke.bats".into()],
    );
    let bats = candidates
        .iter()
        .find(|candidate| candidate.id == "bats-test")
        .unwrap();
    assert!(bats.declared && bats.check_only);
    assert_eq!(bats.capability, QualityCapability::Test);
    assert_eq!(bats.program, "bats");
    assert_eq!(bats.args, vec!["smoke.bats"]);
}

#[test]
fn php_composer_lock_exposes_fixed_security_audit_provider() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("composer.json"),
        r#"{"require":{"php":"^8.3"}}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("composer.lock"),
        r#"{"packages":[],"packages-dev":[]}"#,
    )
    .unwrap();
    fs::write(dir.path().join("index.php"), "<?php echo 1;\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let signals = RepoSignals::load(&workspace);
    let candidates = crate::quality_catalog::candidates_for(
        &workspace,
        &signals,
        SemanticLanguage::Php,
        &["index.php".into()],
    );
    let audit = candidates
        .iter()
        .find(|candidate| candidate.id == "composer-audit")
        .unwrap();
    assert!(audit.declared && audit.check_only);
    assert_eq!(audit.capability, QualityCapability::Security);
    assert_eq!(audit.program, "composer");
    assert_eq!(audit.args, ["audit", "--locked", "--format=json"]);
    assert_eq!(audit.machine_format, Some("json"));
    assert!(audit.external_advisory_data);
}

#[test]
fn ruby_and_r_formatting_use_real_non_mutating_checks() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("Gemfile"),
        "source 'https://rubygems.org'\ngem 'rubocop'\ngem 'standard'\ngem 'rspec'\n",
    )
    .unwrap();
    fs::write(dir.path().join("app.rb"), "puts 'hello'\n").unwrap();
    fs::write(
        dir.path().join("DESCRIPTION"),
        "Package: demo\nImports: lintr, styler, testthat\n",
    )
    .unwrap();
    fs::write(dir.path().join("analysis.R"), "value <- 1\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let registry = registry(&workspace, None).unwrap();
    let ruby = registry
        .languages
        .iter()
        .find(|status| status.language == SemanticLanguage::Ruby)
        .unwrap();
    assert!(!ruby
        .providers
        .iter()
        .any(|provider| provider.id == "rubocop-format"));
    let standard = ruby
        .providers
        .iter()
        .find(|provider| provider.id == "standardrb")
        .unwrap();
    assert_eq!(standard.capability, QualityCapability::Lint);
    assert!(standard.covers.contains(&QualityCapability::Format));
    assert!(standard.command.contains("standardrb --format json"));
    let r = registry
        .languages
        .iter()
        .find(|status| status.language == SemanticLanguage::R)
        .unwrap();
    let styler = r
        .providers
        .iter()
        .find(|provider| provider.id == "styler-format")
        .unwrap();
    assert_eq!(styler.capability, QualityCapability::Format);
    assert!(styler.command.contains("Rscript --vanilla -e"));
    assert!(styler.command.contains("styler::style_pkg"));
}

#[tokio::test]
async fn exact_quality_provider_shapes_reuse_the_autonomous_verification_lane() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"quality-lane\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn value() -> u8 {\n    1\n}\n",
    )
    .unwrap();
    let workspaces = crate::workspace::Workspaces::new([dir.path()], false, true).unwrap();
    let (_, workspace) = workspaces.select(None).unwrap();
    let status = registry(&workspace, None).unwrap();
    let rustfmt = status
        .languages
        .iter()
        .find(|status| status.language == SemanticLanguage::Rust)
        .and_then(|status| {
            status
                .providers
                .iter()
                .find(|provider| provider.id == "rustfmt")
        })
        .expect("rustfmt provider must be visible");
    assert!(rustfmt.runnable);
    assert!(!rustfmt.authorization_required);
    assert_eq!(
        rustfmt.execution_lane,
        QualityExecutionLane::AutonomousVerification
    );
    assert!(rustfmt
        .reason
        .contains("autonomous exact-shape verification"));

    let run = execute(&workspace, SemanticLanguage::Rust, "rustfmt", 30)
        .await
        .expect("exact check-only quality provider should reuse verification autonomy");
    assert!(run.success, "rustfmt failed: {}", run.command.stderr);
    assert_eq!(
        run.execution_lane,
        QualityExecutionLane::AutonomousVerification
    );
    assert!(workspaces.authorization_requests(10).is_empty());
}

#[tokio::test]
async fn arbitrary_repository_scripts_cannot_enter_the_strict_check_only_lane() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"scripts":{"lint":"node mutate-repository.js"}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("index.js"), "export const value = 1;\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, true).unwrap();
    let error = execute(&workspace, SemanticLanguage::JavaScript, "package-lint", 30)
        .await
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("not statically guaranteed check-only"));
}
