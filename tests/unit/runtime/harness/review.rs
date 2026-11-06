use super::*;

#[test]
fn review_source_classification_tracks_canonical_language_variants() {
    for path in [
        "src/app.mjs",
        "src/api.mts",
        "include/value.hxx",
        "views/index.phtml",
        "scripts/check.bats",
        "lib/plugin.gemspec",
    ] {
        assert_eq!(file_category(path), "source", "{path}");
    }
}

#[test]
fn review_manifest_classification_covers_polyglot_project_files() {
    for path in [
        "deno.jsonc",
        "pubspec.yaml",
        "mix.exs",
        "Gemfile",
        "composer.json",
        "dune-project",
        "DESCRIPTION",
        "Package.swift",
        "pom.xml",
        "build.gradle.kts",
        "Demo.csproj",
    ] {
        assert_eq!(file_category(path), "manifest", "{path}");
    }
}

#[test]
fn review_classification_stays_case_insensitive() {
    assert_eq!(file_category("CARGO.TOML"), "manifest");
    assert_eq!(file_category("Docs/README.MD"), "docs");
    assert_eq!(file_category("Src/Auth/Login_Test.RS"), "test");
    assert_eq!(file_category(".GitHub/Workflows/CI.YML"), "workflow");
    assert!(security_sensitive_path("src/OAuth/TokenStore.rs"));
    assert!(!security_sensitive_path("src/author.rs"));
}
