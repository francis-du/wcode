use crate::quality_catalog::{candidate, candidate_owned};
use crate::quality_provider::{
    QualityCandidate, QualityCapability, QualityProviderSource, RepoSignals,
};
use crate::semantic_provider::SemanticLanguage;
use crate::workspace::Workspace;

pub(crate) fn add_candidates(
    workspace: &Workspace,
    signals: &RepoSignals,
    language: SemanticLanguage,
    files: &[String],
    candidates: &mut Vec<QualityCandidate>,
) {
    use QualityCapability::{Format, Lint, StaticAnalysis, Test, TypeCheck};
    use QualityProviderSource::{Ecosystem, LanguageNative};
    match language {
        SemanticLanguage::Bash => {
            let shellcheck = signals.has(".shellcheckrc")
                || signals.any_contains(&["Makefile", ".editorconfig"], "shellcheck");
            let mut shellcheck_args = vec!["--format=json".to_owned()];
            shellcheck_args.extend(files.iter().cloned());
            candidates.push(candidate_owned!(
                "shellcheck",
                Lint,
                Ecosystem,
                "shellcheck",
                shellcheck_args,
                shellcheck,
                "ShellCheck repository configuration",
                Some("json"),
                false,
            ));
            let shfmt = signals.any_contains(&["Makefile", ".editorconfig"], "shfmt");
            let mut shfmt_args = vec!["-d".to_owned()];
            shfmt_args.extend(files.iter().cloned());
            candidates.push(candidate_owned!(
                "shfmt",
                Format,
                Ecosystem,
                "shfmt",
                shfmt_args,
                shfmt,
                "shfmt repository configuration",
                None,
                true,
            ));
        }
        SemanticLanguage::Lua => {
            candidates.push(candidate!(
                "stylua",
                Format,
                Ecosystem,
                "stylua",
                ["--check", "."],
                signals.has(".stylua.toml"),
                ".stylua.toml",
                None,
            ));
            candidates.push(candidate!(
                "luacheck",
                Lint,
                Ecosystem,
                "luacheck",
                ["."],
                signals.has(".luacheckrc"),
                ".luacheckrc",
                None,
            ));
            candidates.push(candidate_owned!(
                "busted",
                Test,
                Ecosystem,
                "busted",
                Vec::new(),
                workspace.root().join(".busted").is_file()
                    || workspace.root().join("spec").is_dir(),
                "Busted spec/configuration",
                None,
                false,
            ));
        }
        SemanticLanguage::Ocaml | SemanticLanguage::OcamlInterface => {
            let dune = signals.has("dune-project");
            candidates.push(candidate!(
                "dune-build",
                TypeCheck,
                LanguageNative,
                "dune",
                ["build"],
                dune,
                "dune-project",
                None,
            ));
            candidates.push(candidate!(
                "dune-runtest",
                Test,
                LanguageNative,
                "dune",
                ["runtest"],
                dune,
                "dune-project",
                None,
            ));
            candidates.push(candidate!(
                "ocamlformat-check",
                Format,
                Ecosystem,
                "dune",
                ["build", "@fmt"],
                dune && signals.has(".ocamlformat"),
                "dune-project plus .ocamlformat",
                None,
            ));
        }
        SemanticLanguage::Php => {
            let composer = signals
                .text
                .get("composer.json")
                .cloned()
                .unwrap_or_default()
                .to_ascii_lowercase();
            candidates.push(candidate_owned!(
                "phpstan",
                StaticAnalysis,
                Ecosystem,
                php_vendor_program(workspace, "phpstan"),
                vec!["analyse".into(), "--error-format=json".into()],
                composer.contains("phpstan")
                    || signals.has("phpstan.neon")
                    || signals.has("phpstan.neon.dist"),
                "PHPStan dependency/configuration",
                Some("json"),
                false,
            ));
            candidates.push(candidate_owned!(
                "psalm",
                StaticAnalysis,
                Ecosystem,
                php_vendor_program(workspace, "psalm"),
                vec!["--output-format=json".into()],
                composer.contains("psalm") || signals.has("psalm.xml"),
                "Psalm dependency/configuration",
                Some("json"),
                false,
            ));
            candidates.push(candidate_owned!(
                "phpunit",
                Test,
                Ecosystem,
                php_vendor_program(workspace, "phpunit"),
                Vec::new(),
                composer.contains("phpunit")
                    || workspace.root().join("phpunit.xml").is_file()
                    || workspace.root().join("phpunit.xml.dist").is_file(),
                "PHPUnit dependency/configuration",
                None,
                false,
            ));
            let fixer = composer.contains("php-cs-fixer")
                || workspace.root().join(".php-cs-fixer.php").is_file()
                || workspace.root().join(".php-cs-fixer.dist.php").is_file();
            candidates.push(candidate_owned!(
                "php-cs-fixer",
                Format,
                Ecosystem,
                php_vendor_program(workspace, "php-cs-fixer"),
                vec!["fix".into(), "--dry-run".into(), "--diff".into()],
                fixer,
                "PHP CS Fixer dependency/configuration",
                None,
                false,
            ));
        }
        SemanticLanguage::R => {
            let description = signals
                .text
                .get("DESCRIPTION")
                .cloned()
                .unwrap_or_default()
                .to_ascii_lowercase();
            candidates.push(candidate!(
                "lintr",
                Lint,
                Ecosystem,
                "Rscript",
                [
                    "-e",
                    "quit(status=if(length(lintr::lint_package()))1 else 0)",
                ],
                signals.has(".lintr") || description.contains("lintr"),
                "lintr package/configuration",
                None,
            ));
            candidates.push(candidate!(
                "testthat",
                Test,
                Ecosystem,
                "Rscript",
                ["-e", "testthat::test_local()"],
                description.contains("testthat")
                    || workspace.root().join("tests/testthat").is_dir(),
                "testthat package/tests",
                None,
            ));
        }
        SemanticLanguage::Ruby => {
            let gemfile = signals
                .text
                .get("Gemfile")
                .cloned()
                .unwrap_or_default()
                .to_ascii_lowercase();
            let rubocop = gemfile.contains("rubocop")
                || signals.has(".rubocop.yml")
                || signals.has(".rubocop.yaml");
            candidates.push(candidate!(
                "rubocop",
                Lint,
                Ecosystem,
                "bundle",
                ["exec", "rubocop", "--format", "json"],
                rubocop,
                "RuboCop dependency/configuration",
                Some("json"),
            ));
            candidates.push(candidate!(
                "rubocop-format",
                Format,
                Ecosystem,
                "bundle",
                ["exec", "rubocop", "--format", "json"],
                rubocop,
                "RuboCop dependency/configuration",
                Some("json"),
            ));
            candidates.push(candidate!(
                "rspec",
                Test,
                Ecosystem,
                "bundle",
                ["exec", "rspec"],
                gemfile.contains("rspec") || workspace.root().join("spec").is_dir(),
                "RSpec dependency/spec directory",
                None,
            ));
        }
        SemanticLanguage::Swift => {
            candidates.push(candidate!(
                "swift-test",
                Test,
                LanguageNative,
                "swift",
                ["test"],
                signals.has("Package.swift"),
                "Package.swift",
                None,
            ));
            candidates.push(candidate!(
                "swift-format",
                Format,
                Ecosystem,
                "swift-format",
                ["lint", "-r", "."],
                signals.has(".swift-format"),
                ".swift-format",
                None,
            ));
            let swiftlint = workspace.root().join(".swiftlint.yml").is_file()
                || workspace.root().join(".swiftlint.yaml").is_file();
            candidates.push(candidate!(
                "swiftlint",
                Lint,
                Ecosystem,
                "swiftlint",
                ["lint", "--strict"],
                swiftlint,
                "SwiftLint configuration",
                None,
            ));
        }
        _ => {}
    }
}

fn php_vendor_program(workspace: &Workspace, name: &str) -> String {
    let relative = format!("vendor/bin/{name}");
    if workspace.root().join(&relative).is_file() {
        relative
    } else {
        name.to_owned()
    }
}
