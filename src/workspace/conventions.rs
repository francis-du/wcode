use crate::scopes::{self, ProductScope};
use crate::semantic_provider::{language_for_path, SemanticLanguage};
use crate::workspace::Workspace;
use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const MAX_CONVENTION_FILES: usize = 10_000;
const MAX_FINDINGS: usize = 256;
const OVERSIZED_SOURCE_LINES: usize = 2_000;
const FLAT_RUST_MODULE_THRESHOLD: usize = 16;
const DOMAIN_PREFIX_THRESHOLD: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConventionStrength {
    Required,
    Recommended,
    ProjectDefined,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConventionSeverity {
    Warning,
    Error,
}

#[derive(Clone, Debug, Serialize)]
pub struct LanguageConvention {
    pub language: SemanticLanguage,
    pub file_naming: &'static str,
    pub module_naming: &'static str,
    pub type_naming: &'static str,
    pub strength: ConventionStrength,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConventionFinding {
    pub code: String,
    pub severity: ConventionSeverity,
    pub path: String,
    pub language: Option<SemanticLanguage>,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ArchitectureDomain {
    pub name: String,
    pub files: usize,
    pub languages: Vec<SemanticLanguage>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProductScopeSummary {
    pub scope: ProductScope,
    pub files: usize,
    pub languages: Vec<SemanticLanguage>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConventionReport {
    pub provider: &'static str,
    pub policies: Vec<LanguageConvention>,
    pub detected_languages: Vec<SemanticLanguage>,
    pub architecture_domains: Vec<ArchitectureDomain>,
    pub product_scopes: Vec<ProductScopeSummary>,
    pub unclassified_source_files: usize,
    pub unmapped_product_scope_files: usize,
    pub files_checked: usize,
    pub errors: usize,
    pub warnings: usize,
    pub findings: Vec<ConventionFinding>,
    pub truncated: bool,
}

pub fn status(workspace: &Workspace) -> Result<ConventionReport> {
    let (files, scan_truncated) = workspace.source_files(".", MAX_CONVENTION_FILES)?;
    let mut findings = Vec::new();
    let mut detected_languages = BTreeSet::new();
    let mut architecture_domains = BTreeMap::<String, (usize, BTreeSet<SemanticLanguage>)>::new();
    let mut product_scopes = BTreeMap::<ProductScope, (usize, BTreeSet<SemanticLanguage>)>::new();
    let mut unclassified_source_files = 0usize;
    let mut unmapped_product_scope_files = 0usize;
    let mut rust_root_modules = Vec::new();
    let mut prefix_counts = BTreeMap::<String, usize>::new();
    let mut files_checked = 0usize;

    for path in files {
        let Some(language) = language_for_path(&path) else {
            continue;
        };
        detected_languages.insert(language);
        files_checked += 1;
        if let Some(scope) = scopes::source_scope(&path) {
            let entry = product_scopes
                .entry(scope)
                .or_insert_with(|| (0, BTreeSet::new()));
            entry.0 = entry.0.saturating_add(1);
            entry.1.insert(language);
        } else if path.starts_with("src/") {
            unmapped_product_scope_files = unmapped_product_scope_files.saturating_add(1);
            push_finding(
                &mut findings,
                ConventionFinding {
                    code: "unmapped-product-scope".to_owned(),
                    severity: ConventionSeverity::Warning,
                    path: path.clone(),
                    language: Some(language),
                    message: "Source file is not mapped to a canonical wcode Product Scope; classify it in src/scopes before extending the subsystem.".to_owned(),
                },
            );
        }
        let policy = language_convention(language);
        let file_name = Path::new(&path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(path.as_str());
        let stem = Path::new(file_name)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(file_name);
        let valid_name = match language {
            SemanticLanguage::Rust => rust_file_name(stem),
            SemanticLanguage::Python => python_file_name(stem),
            SemanticLanguage::Dart => lower_snake_case(stem),
            SemanticLanguage::Ruby | SemanticLanguage::Elixir => lower_snake_case(stem),
            _ => true,
        };
        if !valid_name && policy.strength != ConventionStrength::ProjectDefined {
            push_finding(
                &mut findings,
                ConventionFinding {
                    code: "language-file-naming".to_owned(),
                    severity: if policy.strength == ConventionStrength::Required {
                        ConventionSeverity::Error
                    } else {
                        ConventionSeverity::Warning
                    },
                    path: path.clone(),
                    language: Some(language),
                    message: format!(
                        "{} source file does not follow the configured language convention: {}",
                        language.as_str(),
                        policy.file_naming
                    ),
                },
            );
        }

        if let Ok(source) = workspace.load_source(&path) {
            let lines = production_module_lines(language, &source.content);
            if lines > OVERSIZED_SOURCE_LINES {
                push_finding(
                    &mut findings,
                    ConventionFinding {
                        code: "oversized-source-module".to_owned(),
                        severity: ConventionSeverity::Warning,
                        path: path.clone(),
                        language: Some(language),
                        message: format!(
                            "source module has {lines} lines; split protocol/UI/orchestration/domain responsibilities before adding more behavior"
                        ),
                    },
                );
            }
        }

        if let Some(relative) = path.strip_prefix("src/") {
            if let Some((domain, _)) = relative.split_once('/') {
                let entry = architecture_domains
                    .entry(domain.to_owned())
                    .or_insert_with(|| (0, BTreeSet::new()));
                entry.0 = entry.0.saturating_add(1);
                entry.1.insert(language);
                if language == SemanticLanguage::Rust && !lower_snake_case(domain) {
                    push_finding(
                        &mut findings,
                        ConventionFinding {
                            code: "rust-domain-directory-naming".to_owned(),
                            severity: ConventionSeverity::Error,
                            path: format!("src/{domain}"),
                            language: Some(language),
                            message: "Rust domain directories under src/ must use lower_snake_case so physical architecture stays aligned with module naming.".to_owned(),
                        },
                    );
                }
            } else if !matches!(stem, "main" | "lib") {
                unclassified_source_files = unclassified_source_files.saturating_add(1);
                if language == SemanticLanguage::Rust {
                    rust_root_modules.push(path.clone());
                    push_finding(
                        &mut findings,
                        ConventionFinding {
                            code: "unclassified-rust-root-module".to_owned(),
                            severity: ConventionSeverity::Warning,
                            path: path.clone(),
                            language: Some(language),
                            message: "Non-entry Rust modules should live under a cohesive src/<domain>/ directory instead of growing the crate root flat.".to_owned(),
                        },
                    );
                    if let Some((prefix, _)) = stem.split_once('_') {
                        *prefix_counts.entry(prefix.to_owned()).or_default() += 1;
                    }
                }
            }
        }
    }

    if rust_root_modules.len() >= FLAT_RUST_MODULE_THRESHOLD {
        for (prefix, count) in prefix_counts {
            if count >= DOMAIN_PREFIX_THRESHOLD {
                push_finding(
                    &mut findings,
                    ConventionFinding {
                        code: "flat-rust-domain-modules".to_owned(),
                        severity: ConventionSeverity::Warning,
                        path: "src".to_owned(),
                        language: Some(SemanticLanguage::Rust),
                        message: format!(
                            "{count} top-level Rust modules share the `{prefix}_` domain prefix; prefer a cohesive `{prefix}` module boundary instead of continuing flat prefix-based growth"
                        ),
                    },
                );
            }
        }
    }

    let errors = findings
        .iter()
        .filter(|finding| finding.severity == ConventionSeverity::Error)
        .count();
    let warnings = findings
        .iter()
        .filter(|finding| finding.severity == ConventionSeverity::Warning)
        .count();
    let truncated = scan_truncated || findings.len() >= MAX_FINDINGS;
    findings.truncate(MAX_FINDINGS);
    Ok(ConventionReport {
        provider: "wcode-conventions",
        policies: SemanticLanguage::ALL
            .into_iter()
            .map(language_convention)
            .collect(),
        detected_languages: detected_languages.into_iter().collect(),
        architecture_domains: architecture_domains
            .into_iter()
            .map(|(name, (files, languages))| ArchitectureDomain {
                name,
                files,
                languages: languages.into_iter().collect(),
            })
            .collect(),
        product_scopes: product_scopes
            .into_iter()
            .map(|(scope, (files, languages))| ProductScopeSummary {
                scope,
                files,
                languages: languages.into_iter().collect(),
            })
            .collect(),
        unclassified_source_files,
        unmapped_product_scope_files,
        files_checked,
        errors,
        warnings,
        findings,
        truncated,
    })
}

fn production_module_lines(language: SemanticLanguage, content: &str) -> usize {
    if language != SemanticLanguage::Rust {
        return content.lines().count();
    }
    content
        .split_once("\n#[cfg(test)]\nmod tests {")
        .map(|(production, _)| production.lines().count())
        .unwrap_or_else(|| content.lines().count())
}

pub fn language_convention(language: SemanticLanguage) -> LanguageConvention {
    use ConventionStrength::{ProjectDefined, Recommended, Required};
    match language {
        SemanticLanguage::Rust => convention(
            language,
            "snake_case.rs",
            "snake_case",
            "UpperCamelCase",
            Required,
        ),
        SemanticLanguage::Python => convention(
            language,
            "lowercase_with_underscores.py",
            "lowercase_with_underscores",
            "CapWords",
            Required,
        ),
        SemanticLanguage::Dart => convention(
            language,
            "lowercase_with_underscores.dart",
            "lowercase_with_underscores",
            "UpperCamelCase",
            Required,
        ),
        SemanticLanguage::Ruby => convention(
            language,
            "snake_case.rb",
            "snake_case",
            "CamelCase",
            Recommended,
        ),
        SemanticLanguage::Elixir => convention(
            language,
            "snake_case.ex/.exs",
            "CamelCase modules",
            "CamelCase",
            Recommended,
        ),
        SemanticLanguage::Go => convention(
            language,
            "project-consistent .go",
            "short lowercase package names",
            "MixedCaps",
            Recommended,
        ),
        SemanticLanguage::Java => convention(
            language,
            "public type normally matches filename",
            "lowercase package names",
            "UpperCamelCase",
            Recommended,
        ),
        SemanticLanguage::CSharp => convention(
            language,
            "project-consistent .cs",
            "PascalCase namespaces",
            "PascalCase",
            Recommended,
        ),
        SemanticLanguage::Swift => convention(
            language,
            "project-consistent .swift",
            "UpperCamelCase types",
            "UpperCamelCase",
            Recommended,
        ),
        SemanticLanguage::Php => convention(
            language,
            "PSR/project-defined .php",
            "PSR/project-defined",
            "StudlyCaps",
            Recommended,
        ),
        SemanticLanguage::Ocaml | SemanticLanguage::OcamlInterface => convention(
            language,
            "lowercase module file",
            "Capitalized module identity",
            "Capitalized variants/modules",
            Recommended,
        ),
        SemanticLanguage::Bash => convention(
            language,
            "project-defined shell script name",
            "n/a",
            "n/a",
            ProjectDefined,
        ),
        SemanticLanguage::C | SemanticLanguage::Cpp => convention(
            language,
            "project-defined",
            "project-defined",
            "project-defined",
            ProjectDefined,
        ),
        SemanticLanguage::Css | SemanticLanguage::Html => convention(
            language,
            "project-defined web asset naming",
            "n/a",
            "n/a",
            ProjectDefined,
        ),
        SemanticLanguage::JavaScript | SemanticLanguage::TypeScript | SemanticLanguage::Tsx => {
            convention(
                language,
                "project-defined JS/TS naming",
                "project-defined",
                "PascalCase for types/components when applicable",
                ProjectDefined,
            )
        }
        SemanticLanguage::Lua => convention(
            language,
            "project-defined .lua",
            "project-defined",
            "project-defined",
            ProjectDefined,
        ),
        SemanticLanguage::R => convention(
            language,
            "project-defined .R/.r",
            "project-defined",
            "project-defined",
            ProjectDefined,
        ),
    }
}

const fn convention(
    language: SemanticLanguage,
    file_naming: &'static str,
    module_naming: &'static str,
    type_naming: &'static str,
    strength: ConventionStrength,
) -> LanguageConvention {
    LanguageConvention {
        language,
        file_naming,
        module_naming,
        type_naming,
        strength,
    }
}

fn rust_file_name(stem: &str) -> bool {
    matches!(stem, "main" | "lib" | "mod" | "build") || lower_snake_case(stem)
}

fn python_file_name(stem: &str) -> bool {
    stem == "__init__" || lower_snake_case(stem)
}

fn lower_snake_case(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with(|character: char| character.is_ascii_digit())
        && value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
        && !value.contains("__")
        && !value.starts_with('_')
        && !value.ends_with('_')
}

fn push_finding(findings: &mut Vec<ConventionFinding>, finding: ConventionFinding) {
    if findings.len() < MAX_FINDINGS {
        findings.push(finding);
    }
}

#[cfg(test)]
mod tests {
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
        assert!(
            production_module_lines(SemanticLanguage::Python, &content) > OVERSIZED_SOURCE_LINES
        );
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
}
