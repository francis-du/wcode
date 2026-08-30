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
fn automatic_semantics_only_trust_the_hardened_rust_provider() {
    let rust = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "rust-analyzer")
        .unwrap();
    let clangd = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "clangd")
        .unwrap();
    assert!(automatic_provider(rust));
    assert!(!automatic_provider(clangd));
    let options = client::initialization_options("rust-analyzer");
    assert_eq!(
        options.pointer("/cargo/buildScripts/enable"),
        Some(&json!(false))
    );
    assert_eq!(options.pointer("/cargo/autoreload"), Some(&json!(false)));
    assert_eq!(options.pointer("/procMacro/enable"), Some(&json!(false)));
    assert_eq!(options.get("checkOnSave"), Some(&json!(false)));
}

#[test]
fn provider_executables_inside_the_workspace_are_rejected() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(workspace_dir.path(), false, false).unwrap();
    let injected = workspace_dir.path().join("rust-analyzer");
    std::fs::write(&injected, "fixture").unwrap();
    assert!(trusted_provider_path(&workspace, &injected).is_none());

    let external_dir = tempfile::tempdir().unwrap();
    let external = external_dir.path().join("rust-analyzer");
    std::fs::write(&external, "fixture").unwrap();
    assert_eq!(trusted_provider_path(&workspace, &external), Some(external));
}

#[cfg(unix)]
#[test]
fn trusted_provider_path_preserves_proxy_symlink_identity() {
    use std::os::unix::fs::symlink;

    let workspace_dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(workspace_dir.path(), false, false).unwrap();
    let tools = tempfile::tempdir().unwrap();
    let proxy_target = tools.path().join("rustup");
    let provider_link = tools.path().join("rust-analyzer");
    std::fs::write(&proxy_target, "fixture").unwrap();
    symlink(&proxy_target, &provider_link).unwrap();

    assert_eq!(
        trusted_provider_path(&workspace, &provider_link),
        Some(provider_link),
        "validation must follow the symlink without replacing the executable path"
    );
}

#[test]
fn provider_session_authorization_is_provider_scoped() {
    assert_eq!(
        provider_session_operation("rust-analyzer"),
        "semantic_provider_session\0rust-analyzer"
    );
    assert_ne!(
        provider_session_operation("rust-analyzer"),
        provider_session_operation("clangd")
    );
}

#[test]
fn relation_expansion_prefers_high_value_symbol_kinds() {
    assert!(call_hierarchy_candidate(6));
    assert!(call_hierarchy_candidate(12));
    assert!(!call_hierarchy_candidate(13));
    assert!(implementation_candidate(11));
    assert!(implementation_candidate(23));
    assert!(!implementation_candidate(13));
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
