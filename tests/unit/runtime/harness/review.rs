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
