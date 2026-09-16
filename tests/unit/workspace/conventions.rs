use super::*;

#[test]
fn hugo_build_outputs_are_not_maintained_source() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("hugo.toml"),
        "baseURL = 'https://example.test/'\n",
    )
    .unwrap();
    for path in [
        "public/css/bundle.css",
        "public/gallery/index.html",
        "assets/owned.css",
    ] {
        let file = root.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, "/* maintained or generated line */\n".repeat(1001)).unwrap();
    }
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let report = status(&workspace).unwrap();
    let oversized = report
        .findings
        .iter()
        .filter(|finding| finding.code == "oversized-source-module")
        .map(|finding| finding.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(oversized, ["assets/owned.css"]);
}

#[test]
fn convention_scope_keeps_plain_public_and_uninitialized_submodules() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join(".gitmodules"),
        "[submodule \"theme\"]\n path = themes/theme\n",
    )
    .unwrap();
    for path in ["public/owned.js", "themes/theme/owned.js"] {
        let file = root.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, "// maintained source\n".repeat(1001)).unwrap();
    }
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    assert_eq!(status(&workspace).unwrap().errors, 2);
}

#[test]
fn initialized_submodule_has_its_own_convention_boundary() {
    let root = tempfile::tempdir().unwrap();
    let child = root.path().join("themes/theme");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(
        root.path().join(".gitmodules"),
        "[submodule \"theme\"]\n path = themes/theme\n",
    )
    .unwrap();
    std::fs::write(
        child.join("owned.js"),
        "// maintained source\n".repeat(1001),
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let before = fingerprint_and_paths(&workspace).unwrap().0;
    assert_eq!(status(&workspace).unwrap().errors, 1);
    std::fs::write(child.join(".git"), "gitdir: ../../.git/modules/theme\n").unwrap();
    let after = fingerprint_and_paths(&workspace).unwrap().0;
    assert_ne!(
        before, after,
        "ownership changes must invalidate the convention cache"
    );
    assert_eq!(status(&workspace).unwrap().errors, 0);
    let child_workspace = Workspace::new(&child, false, false).unwrap();
    assert_eq!(status(&child_workspace).unwrap().errors, 1);
}

#[test]
fn hugo_output_scope_is_precise_and_rejects_unsafe_paths() {
    let root = tempfile::tempdir().unwrap();
    for path in [
        "site-output/index.html",
        "site-output-extra/owned.js",
        "public/owned.js",
    ] {
        let file = root.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, "// line\n".repeat(1001)).unwrap();
    }
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    for config in [
        "publishDir = '.'",
        "publishDir = '../'",
        "publishDir = '/'",
        "publishDir = 7",
        "invalid TOML",
    ] {
        std::fs::write(root.path().join("hugo.toml"), config).unwrap();
        assert_eq!(status(&workspace).unwrap().errors, 3, "{config}");
    }
    std::fs::write(root.path().join("hugo.toml"), "publishDir = 'site-output'").unwrap();
    assert_eq!(status(&workspace).unwrap().errors, 2);
}

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
fn oversized_policy_counts_all_maintained_source_lines() {
    let content = format!(
        "{}\n#[cfg(test)]\nmod tests {{\n{}\n}}\n",
        "pub fn value() {}\n".repeat(100),
        "#[test]\nfn case() {}\n".repeat(OVERSIZED_SOURCE_LINES + 50)
    );
    assert!(production_module_lines(SemanticLanguage::Rust, &content) > OVERSIZED_SOURCE_LINES);
    assert!(production_module_lines(SemanticLanguage::Python, &content) > OVERSIZED_SOURCE_LINES);
}

#[test]
fn source_write_policy_blocks_growth_but_allows_decomposition_and_generated_code() {
    let bounded = "fn value() {}\n".repeat(OVERSIZED_SOURCE_LINES);
    let oversized = "fn value() {}\n".repeat(OVERSIZED_SOURCE_LINES + 1);
    assert!(validate_source_write("src/new.rs", None, &oversized).is_err());
    assert!(validate_source_write("src/existing.rs", Some(&bounded), &oversized).is_err());
    assert!(validate_source_write(
        "src/existing.rs",
        Some(&format!("{oversized}fn extra() {{}}\n")),
        &oversized,
    )
    .is_ok());
    let generated = format!("// Code generated by fixture. DO NOT EDIT.\n{oversized}");
    assert!(validate_source_write("src/generated_client.rs", None, &generated).is_ok());
    assert!(validate_source_write("src/generated/client.rs", None, &oversized).is_ok());
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
    let oversized = report
        .findings
        .iter()
        .find(|finding| finding.code == "oversized-source-module")
        .expect("oversized maintained source must be reported");
    assert_eq!(oversized.severity, ConventionSeverity::Error);
    assert_eq!(report.errors, 1);
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.code == "flat-rust-domain-modules"));
    assert_eq!(report.unclassified_source_files, 16);
}

#[test]
fn convention_scan_fails_closed_for_source_above_the_bounded_read_limit() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("src/huge.rs"),
        "pub fn value() {}\n".repeat(100_000),
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let report = status(&workspace).unwrap();
    let finding = report
        .findings
        .iter()
        .find(|finding| finding.code == "source-policy-uninspectable")
        .expect("large maintained source must not disappear from convention enforcement");
    assert_eq!(finding.severity, ConventionSeverity::Error);
    assert!(report.errors >= 1);
}

#[test]
fn generated_localization_sources_are_exempt_from_the_maintained_source_limit() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("lib/l10n/app_localizations_en.dart");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut source = "// ignore_for_file: type=lint\nclass AppLocalizationsEn {}\n".to_owned();
    source.push_str(&"String get generatedValue => 'value';\n".repeat(OVERSIZED_SOURCE_LINES + 50));
    std::fs::write(&path, source).unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let report = status(&workspace).unwrap();
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.code == "oversized-source-module"));
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
