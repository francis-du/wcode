use super::*;

#[test]
fn every_indexed_language_has_a_semantic_provider_candidate() {
    for language in SemanticLanguage::ALL {
        assert!(
            PROVIDERS
                .iter()
                .any(|provider| provider.languages.contains(&language)),
            "missing semantic provider candidate for {}",
            language.as_str()
        );
    }
}

#[test]
fn language_detection_covers_the_full_index_surface() {
    let fixtures = [
        ("script.sh", SemanticLanguage::Bash),
        ("a.c", SemanticLanguage::C),
        ("a.cpp", SemanticLanguage::Cpp),
        ("a.cs", SemanticLanguage::CSharp),
        ("a.css", SemanticLanguage::Css),
        ("a.dart", SemanticLanguage::Dart),
        ("a.ex", SemanticLanguage::Elixir),
        ("a.go", SemanticLanguage::Go),
        ("a.html", SemanticLanguage::Html),
        ("A.java", SemanticLanguage::Java),
        ("a.js", SemanticLanguage::JavaScript),
        ("a.lua", SemanticLanguage::Lua),
        ("a.ml", SemanticLanguage::Ocaml),
        ("a.mli", SemanticLanguage::OcamlInterface),
        ("a.php", SemanticLanguage::Php),
        ("a.py", SemanticLanguage::Python),
        ("a.R", SemanticLanguage::R),
        ("a.rb", SemanticLanguage::Ruby),
        ("a.rs", SemanticLanguage::Rust),
        ("a.swift", SemanticLanguage::Swift),
        ("a.ts", SemanticLanguage::TypeScript),
        ("a.tsx", SemanticLanguage::Tsx),
    ];
    for (path, expected) in fixtures {
        assert_eq!(language_for_path(path), Some(expected), "{path}");
    }
}

#[test]
fn provider_revision_changes_only_when_index_inputs_change() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn one() {}\n").unwrap();
    let executable = dir.path().join("rust-analyzer-fixture");
    std::fs::write(&executable, "fixture-binary-v1").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let provider = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "rust-analyzer")
        .unwrap();
    let files = vec![("a.rs".to_owned(), SemanticLanguage::Rust)];
    let first_sources = prepare_sources(&workspace, &files).unwrap();
    let first = provider_revision(provider, &executable, 1_000, &first_sources);
    let same_sources = prepare_sources(&workspace, &files).unwrap();
    let same = provider_revision(provider, &executable, 1_000, &same_sources);
    assert_eq!(first, same);

    let different_bound = provider_revision(provider, &executable, 2_000, &same_sources);
    assert_ne!(first, different_bound);
    std::fs::write(dir.path().join("a.rs"), "fn two() {}\n").unwrap();
    let changed_sources = prepare_sources(&workspace, &files).unwrap();
    let changed_source = provider_revision(provider, &executable, 1_000, &changed_sources);
    assert_ne!(first, changed_source);
}

#[test]
fn lsp_document_symbols_flatten_with_qualified_names() {
    let value = json!([{
        "name":"Outer","kind":5,
        "selectionRange":{"start":{"line":1,"character":2},"end":{"line":1,"character":7}},
        "children":[{
            "name":"work","kind":6,
            "selectionRange":{"start":{"line":3,"character":4},"end":{"line":3,"character":8}}
        }]
    }]);
    let mut output = Vec::new();
    flatten_document_symbols(&value, "src/a.rs", None, &mut output);
    assert_eq!(output.len(), 2);
    assert_eq!(output[1].qualified_name, "Outer::work");
    assert_eq!(output[1].line, 3);
}
