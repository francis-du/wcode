use super::*;

pub(super) fn build_language_configs() -> Result<Arc<LanguageConfigs>> {
    // The TypeScript grammar intentionally ships a narrow tags query focused on
    // declarations unique to TypeScript. Merge the JavaScript query so ordinary
    // classes, methods, functions, arrow functions, and calls remain discoverable.
    let typescript_tags = format!(
        "{}\n{}",
        tree_sitter_javascript::TAGS_QUERY,
        tree_sitter_typescript::TAGS_QUERY
    );
    let c_tags = format!("{}\n{}", tree_sitter_c::TAGS_QUERY, C_CALLS_QUERY);
    let cpp_tags = format!("{}\n{}", tree_sitter_cpp::TAGS_QUERY, C_CALLS_QUERY);
    let rust_tags = format!(
        "{}\n{}\n{}",
        tree_sitter_rust::TAGS_QUERY,
        RUST_CALLS_QUERY,
        RUST_IMPORTS_QUERY
    );
    let ocaml_interface_tags = format!(
        "{}\n{}",
        tree_sitter_ocaml::TAGS_QUERY,
        OCAML_INTERFACE_TAGS_QUERY
    );
    let configs = [
        LanguageConfig::new(
            LanguageId::Bash,
            tree_sitter_bash::LANGUAGE.into(),
            BASH_TAGS_QUERY,
        )?,
        LanguageConfig::new(LanguageId::C, tree_sitter_c::LANGUAGE.into(), &c_tags)?,
        LanguageConfig::new(LanguageId::Cpp, tree_sitter_cpp::LANGUAGE.into(), &cpp_tags)?,
        LanguageConfig::new(
            LanguageId::CSharp,
            tree_sitter_c_sharp::LANGUAGE.into(),
            tree_sitter_c_sharp::TAGS_QUERY,
        )?,
        LanguageConfig::new(
            LanguageId::Css,
            tree_sitter_css::LANGUAGE.into(),
            CSS_TAGS_QUERY,
        )?,
        LanguageConfig::new(
            LanguageId::Dart,
            tree_sitter_dart::LANGUAGE.into(),
            tree_sitter_dart::TAGS_QUERY,
        )?,
        LanguageConfig::new(
            LanguageId::Elixir,
            tree_sitter_elixir::LANGUAGE.into(),
            tree_sitter_elixir::TAGS_QUERY,
        )?,
        LanguageConfig::new(
            LanguageId::Go,
            tree_sitter_go::LANGUAGE.into(),
            tree_sitter_go::TAGS_QUERY,
        )?,
        LanguageConfig::new(
            LanguageId::Html,
            tree_sitter_html::LANGUAGE.into(),
            HTML_TAGS_QUERY,
        )?,
        LanguageConfig::new(
            LanguageId::Java,
            tree_sitter_java::LANGUAGE.into(),
            tree_sitter_java::TAGS_QUERY,
        )?,
        LanguageConfig::new(
            LanguageId::JavaScript,
            tree_sitter_javascript::LANGUAGE.into(),
            tree_sitter_javascript::TAGS_QUERY,
        )?,
        LanguageConfig::new(
            LanguageId::Lua,
            tree_sitter_lua::LANGUAGE.into(),
            tree_sitter_lua::TAGS_QUERY,
        )?,
        LanguageConfig::new(
            LanguageId::Ocaml,
            tree_sitter_ocaml::LANGUAGE_OCAML.into(),
            tree_sitter_ocaml::TAGS_QUERY,
        )?,
        LanguageConfig::new(
            LanguageId::OcamlInterface,
            tree_sitter_ocaml::LANGUAGE_OCAML_INTERFACE.into(),
            &ocaml_interface_tags,
        )?,
        LanguageConfig::new(
            LanguageId::Php,
            tree_sitter_php::LANGUAGE_PHP.into(),
            tree_sitter_php::TAGS_QUERY,
        )?,
        LanguageConfig::new(
            LanguageId::Python,
            tree_sitter_python::LANGUAGE.into(),
            tree_sitter_python::TAGS_QUERY,
        )?,
        LanguageConfig::new(
            LanguageId::R,
            tree_sitter_r::LANGUAGE.into(),
            tree_sitter_r::TAGS_QUERY,
        )?,
        LanguageConfig::new(
            LanguageId::Ruby,
            tree_sitter_ruby::LANGUAGE.into(),
            tree_sitter_ruby::TAGS_QUERY,
        )?,
        LanguageConfig::new(
            LanguageId::Rust,
            tree_sitter_rust::LANGUAGE.into(),
            &rust_tags,
        )?,
        LanguageConfig::new(
            LanguageId::Swift,
            tree_sitter_swift::LANGUAGE.into(),
            tree_sitter_swift::TAGS_QUERY,
        )?,
        LanguageConfig::new(
            LanguageId::TypeScript,
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            &typescript_tags,
        )?,
        LanguageConfig::new(
            LanguageId::Tsx,
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            &typescript_tags,
        )?,
    ]
    .into_iter()
    .map(|config| (config.id, Arc::new(config)))
    .collect();
    Ok(Arc::new(configs))
}
