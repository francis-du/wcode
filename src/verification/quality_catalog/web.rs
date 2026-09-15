use crate::quality_catalog::{candidate, candidate_owned};
use crate::quality_provider::{
    QualityCandidate, QualityCapability, QualityProviderSource, RepoSignals,
};
use crate::semantic_provider::SemanticLanguage;
use crate::workspace::Workspace;
use serde_json::Value;

pub(crate) fn add_deno_tooling(
    workspace: &Workspace,
    signals: &RepoSignals,
    language: SemanticLanguage,
    candidates: &mut Vec<QualityCandidate>,
) {
    use QualityCapability::{Format, Lint, Security, StaticAnalysis, Test, TypeCheck};
    use QualityProviderSource::LanguageNative;
    let declared = signals.has("deno.json") || signals.has("deno.jsonc");
    candidates.push(candidate!(
        "deno-fmt",
        Format,
        LanguageNative,
        "deno",
        ["fmt", "--check"],
        declared,
        "deno.json/deno.jsonc",
        None,
    ));
    candidates.push(candidate!(
        "deno-lint",
        Lint,
        LanguageNative,
        "deno",
        ["lint"],
        declared,
        "deno.json/deno.jsonc",
        None,
    ));
    candidates.push(candidate!(
        "deno-test",
        Test,
        LanguageNative,
        "deno",
        ["test", "--frozen"],
        declared,
        "deno.json/deno.jsonc",
        None,
    ));
    let mut deno_audit = candidate!(
        "deno-audit",
        Security,
        LanguageNative,
        "deno",
        ["audit", "--frozen"],
        declared && workspace.root().join("deno.lock").is_file(),
        "Deno configuration plus deno.lock",
        None,
    );
    deno_audit.external_advisory_data = true;
    candidates.push(deno_audit);
    if matches!(
        language,
        SemanticLanguage::TypeScript | SemanticLanguage::Tsx
    ) {
        let mut deno_check = candidate!(
            "deno-check",
            TypeCheck,
            LanguageNative,
            "deno",
            ["check", "--frozen", "."],
            declared,
            "deno.json/deno.jsonc",
            None,
        );
        deno_check.covers.push(StaticAnalysis);
        candidates.push(deno_check);
    }
}

pub(crate) fn add_web_tooling(
    workspace: &Workspace,
    signals: &RepoSignals,
    language: SemanticLanguage,
    candidates: &mut Vec<QualityCandidate>,
) {
    use QualityCapability::{Format, Lint, Test, TypeCheck};
    use QualityProviderSource::Ecosystem;
    let prettier = signals.package_dependency("prettier")
        || [
            ".prettierrc",
            ".prettierrc.json",
            ".prettierrc.yml",
            ".prettierrc.yaml",
            "prettier.config.js",
            "prettier.config.mjs",
            "prettier.config.cjs",
            "prettier.config.ts",
        ]
        .iter()
        .any(|path| workspace.root().join(path).is_file());
    candidates.push(candidate_owned!(
        "prettier-check",
        Format,
        Ecosystem,
        node_program(workspace, "prettier"),
        vec![".".into(), "--check".into()],
        prettier,
        "Prettier dependency/configuration",
        None,
        false,
    ));

    let biome = signals.package_dependency("@biomejs/biome")
        || signals.has("biome.json")
        || signals.has("biome.jsonc");
    let biome_program = node_program(workspace, "biome");
    candidates.push(candidate_owned!(
        "biome-format",
        Format,
        Ecosystem,
        biome_program.clone(),
        vec!["format".into(), ".".into(), "--reporter=json".into()],
        biome_format_declared(signals, language, biome),
        "Biome dependency/configuration with language formatter enabled",
        Some("json"),
        false,
    ));
    candidates.push(candidate_owned!(
        "biome-check",
        Lint,
        Ecosystem,
        biome_program,
        vec![
            "check".into(),
            ".".into(),
            "--formatter-enabled=false".into(),
            "--assist-enabled=false".into(),
            "--reporter=json".into(),
        ],
        biome_lint_declared(signals, language, biome),
        "Biome dependency/configuration with language lint support",
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
    let eslint_declared = match language {
        SemanticLanguage::JavaScript | SemanticLanguage::TypeScript | SemanticLanguage::Tsx => {
            eslint
        }
        SemanticLanguage::Css => eslint && signals.package_dependency("@eslint/css"),
        SemanticLanguage::Html => {
            eslint && signals.package_dependency("@html-eslint/eslint-plugin")
        }
        _ => false,
    };
    candidates.push(candidate_owned!(
        "eslint",
        Lint,
        Ecosystem,
        node_program(workspace, "eslint"),
        vec![".".into(), "--format".into(), "json".into()],
        eslint_declared,
        "ESLint dependency/configuration with matching language plugin",
        Some("json"),
        false,
    ));

    if matches!(
        language,
        SemanticLanguage::JavaScript | SemanticLanguage::TypeScript | SemanticLanguage::Tsx
    ) {
        let vitest = signals.package_dependency("vitest")
            || [
                "vitest.config.js",
                "vitest.config.mjs",
                "vitest.config.cjs",
                "vitest.config.ts",
                "vitest.config.mts",
                "vitest.config.cts",
            ]
            .iter()
            .any(|path| workspace.root().join(path).is_file());
        candidates.push(candidate_owned!(
            "vitest-run",
            Test,
            Ecosystem,
            node_program(workspace, "vitest"),
            vec!["run".into()],
            vitest,
            "Vitest dependency/configuration",
            None,
            false,
        ));
        let jest = signals.package_dependency("jest")
            || [
                "jest.config.js",
                "jest.config.mjs",
                "jest.config.cjs",
                "jest.config.ts",
            ]
            .iter()
            .any(|path| workspace.root().join(path).is_file());
        candidates.push(candidate_owned!(
            "jest-run",
            Test,
            Ecosystem,
            node_program(workspace, "jest"),
            vec!["--runInBand".into()],
            jest,
            "Jest dependency/configuration",
            None,
            false,
        ));
    }
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
            || [
                "stylelint.config.js",
                "stylelint.config.cjs",
                "stylelint.config.mjs",
                ".stylelintrc",
                ".stylelintrc.json",
                ".stylelintrc.yml",
                ".stylelintrc.yaml",
            ]
            .iter()
            .any(|path| workspace.root().join(path).is_file());
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
    if language == SemanticLanguage::Html {
        let htmlhint = signals.package_dependency("htmlhint")
            || workspace.root().join(".htmlhintrc").is_file();
        candidates.push(candidate_owned!(
            "htmlhint",
            Lint,
            Ecosystem,
            node_program(workspace, "htmlhint"),
            vec!["**/*.html".into(), "--format".into(), "json".into()],
            htmlhint,
            "HTMLHint dependency/.htmlhintrc",
            Some("json"),
            false,
        ));
    }
}

pub(crate) fn add_package_script_candidates(
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
            provider.check_only = false;
            candidates.push(provider);
        }
    }
}

fn biome_config(signals: &RepoSignals) -> Option<Value> {
    signals
        .text
        .get("biome.json")
        .and_then(|content| serde_json::from_str(content).ok())
}

fn biome_flag(signals: &RepoSignals, pointer: &str) -> bool {
    biome_config(signals)
        .as_ref()
        .and_then(|config| config.pointer(pointer))
        .and_then(Value::as_bool)
        == Some(true)
}

fn biome_format_declared(signals: &RepoSignals, language: SemanticLanguage, biome: bool) -> bool {
    if !biome {
        return false;
    }
    match language {
        SemanticLanguage::JavaScript | SemanticLanguage::TypeScript | SemanticLanguage::Tsx => true,
        SemanticLanguage::Css => biome_flag(signals, "/css/formatter/enabled"),
        SemanticLanguage::Html => {
            biome_flag(signals, "/html/experimentalFullSupportEnabled")
                && biome_flag(signals, "/html/formatter/enabled")
        }
        _ => false,
    }
}

fn biome_lint_declared(signals: &RepoSignals, language: SemanticLanguage, biome: bool) -> bool {
    if !biome {
        return false;
    }
    match language {
        SemanticLanguage::JavaScript
        | SemanticLanguage::TypeScript
        | SemanticLanguage::Tsx
        | SemanticLanguage::Css => true,
        SemanticLanguage::Html => biome_flag(signals, "/html/experimentalFullSupportEnabled"),
        _ => false,
    }
}

fn node_program(workspace: &Workspace, name: &str) -> String {
    let relative = format!("node_modules/.bin/{name}");
    if workspace.workspace_program_available(&relative) {
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
