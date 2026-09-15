use super::*;
use crate::semantic_provider::SemanticLanguage;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn every_supported_language_has_an_autonomous_development_tool_path() {
    let safe = WorkspaceSecurity::default();
    let cases = [
        ("bash", "shellcheck", vec!["--format=json", "script.sh"]),
        (
            "c",
            "clang-format",
            vec!["--dry-run", "--Werror", "src/main.c"],
        ),
        ("cpp", "clang-tidy", vec!["src/main.cpp", "-p", "."]),
        ("csharp", "dotnet", vec!["test", "--no-restore"]),
        ("css", "stylelint", vec!["**/*.css", "--formatter", "json"]),
        ("dart", "dart", vec!["run", "tool.dart"]),
        ("elixir", "mix", vec!["test"]),
        ("go", "go", vec!["test", "./..."]),
        ("html", "eslint", vec![".", "--format", "json"]),
        ("java", "mvn", vec!["test"]),
        ("javascript", "npm", vec!["run", "test"]),
        ("lua", "busted", vec![]),
        ("ocaml", "dune", vec!["runtest"]),
        ("ocaml-interface", "dune", vec!["build"]),
        ("php", "phpunit", vec![]),
        ("python", "pytest", vec!["-q"]),
        (
            "r",
            "Rscript",
            vec!["--vanilla", "-e", "testthat::test_local()"],
        ),
        ("ruby", "bundle", vec!["exec", "rspec"]),
        ("rust", "cargo", vec!["test"]),
        ("swift", "swift", vec!["test"]),
        ("typescript", "tsc", vec!["--noEmit"]),
        ("tsx", "npm", vec!["run", "typecheck"]),
    ];
    assert_eq!(cases.len(), SemanticLanguage::ALL.len());
    for (language, program, values) in cases {
        assert!(validate_command_policy(program, &args(&values), safe).is_ok(), "{language} development command unexpectedly requires authorization: {program} {values:?}");
    }
    assert!(validate_command_policy("biome", &args(&["format", ".", "--write"]), safe).is_ok());
    assert!(validate_command_policy("prettier", &args(&[".", "--write"]), safe).is_ok());
    assert!(validate_command_policy("vitest", &args(&["watch"]), safe).is_ok());
    assert!(validate_command_policy("jest", &args(&["--watch"]), safe).is_ok());

    let external_biome = if cfg!(windows) {
        r"--config-path=C:\biome.json"
    } else {
        "--config-path=/tmp/biome.json"
    };
    let external_prettier = if cfg!(windows) {
        r"C:\prettier.json"
    } else {
        "/tmp/prettier.json"
    };
    let external_clang = if cfg!(windows) {
        r"--style=file:C:\other"
    } else {
        "--style=file:/tmp/other"
    };
    for (program, values) in [
        ("biome", vec!["check", ".", external_biome]),
        (
            "prettier",
            vec![".", "--check", "--config", external_prettier],
        ),
        ("Rscript", vec!["-e", "testthat::test_local()"]),
        (
            "Rscript",
            vec!["--vanilla", "-e", "system('curl example.com')"],
        ),
        (
            "cargo-fuzz",
            vec!["fuzz", "run", "../target", "--", "-max_total_time=5"],
        ),
        ("clang-format", vec![external_clang, "src/main.c"]),
    ] {
        assert!(
            validate_command_policy(program, &args(&values), safe).is_err(),
            "unsafe boundary unexpectedly accepted: {program} {values:?}"
        );
    }
}
