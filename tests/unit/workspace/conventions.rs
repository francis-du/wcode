use super::*;

#[test]
fn language_policies_cover_the_full_index_surface() {
    let policies = SemanticLanguage::ALL
        .into_iter()
        .map(language_convention)
        .collect::<Vec<_>>();
    assert_eq!(policies.len(), 22);
    for language in [
        SemanticLanguage::Rust,
        SemanticLanguage::Python,
        SemanticLanguage::Dart,
        SemanticLanguage::Ruby,
        SemanticLanguage::Elixir,
    ] {
        assert!(policies.iter().any(|policy| policy.language == language));
    }
    assert!(policies
        .iter()
        .any(|policy| policy.language == SemanticLanguage::Rust
            && policy.strength == ConventionStrength::Required));
    assert!(policies
        .iter()
        .any(|policy| policy.language == SemanticLanguage::TypeScript
            && policy.strength == ConventionStrength::ProjectDefined));
}

#[test]
fn required_file_naming_helpers_are_strict_without_overreaching() {
    assert!(rust_file_name("software_graph"));
    assert!(!rust_file_name("SoftwareGraph"));
    assert!(python_file_name("__init__"));
    assert!(python_file_name("request_router"));
    assert!(!python_file_name("request-router"));
    assert!(lower_snake_case("runtime_canary"));
    assert!(!lower_snake_case("RuntimeCanary"));
}

#[test]
fn rust_oversized_check_ignores_inline_test_module_lines() {
    let content = format!(
        "{}\n#[cfg(test)]\nmod tests {{\n{}\n}}\n",
        "pub fn value() {}\n".repeat(100),
        "#[test]\nfn case() {}\n".repeat(OVERSIZED_SOURCE_LINES + 50)
    );
    assert_eq!(
        production_module_lines(SemanticLanguage::Rust, &content),
        100
    );
    assert!(production_module_lines(SemanticLanguage::Python, &content) > OVERSIZED_SOURCE_LINES);
}

#[test]
fn status_surfaces_oversized_and_flat_rust_domain_modules() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    for index in 0..16 {
        let stem = if index < 3 {
            format!("graph_{index}")
        } else {
            format!("module_{index}")
        };
        let content = if index == 0 {
            "// line\n".repeat(OVERSIZED_SOURCE_LINES + 1)
        } else {
            "pub fn value() {}\n".to_owned()
        };
        std::fs::write(root.path().join("src").join(format!("{stem}.rs")), content).unwrap();
    }
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let report = status(&workspace).unwrap();
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.code == "oversized-source-module"));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.code == "flat-rust-domain-modules"));
    assert_eq!(report.unclassified_source_files, 16);
}

#[test]
fn status_classifies_source_domains_and_flags_root_orphans() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src/graph")).unwrap();
    std::fs::create_dir_all(root.path().join("src/integrations")).unwrap();
    std::fs::write(root.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(
        root.path().join("src/graph/index.rs"),
        "pub fn index() {}\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("src/integrations/mcp.rs"),
        "pub fn serve() {}\n",
    )
    .unwrap();
    std::fs::write(root.path().join("src/orphan.rs"), "pub fn orphan() {}\n").unwrap();

    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let report = status(&workspace).unwrap();
    assert_eq!(report.unclassified_source_files, 1);
    assert!(report
        .architecture_domains
        .iter()
        .any(|domain| domain.name == "graph" && domain.files == 1));
    assert!(report
        .architecture_domains
        .iter()
        .any(|domain| domain.name == "integrations" && domain.files == 1));
    assert!(report
        .product_scopes
        .iter()
        .any(|summary| summary.scope == ProductScope::Graph && summary.files == 1));
    assert!(report
        .product_scopes
        .iter()
        .any(|summary| summary.scope == ProductScope::Integrations && summary.files == 1));
    assert_eq!(report.unmapped_product_scope_files, 1);
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.code == "unclassified-rust-root-module"));
}
