use crate::quality_catalog::candidates_for;
use crate::semantic_provider::{self, language_for_path, SemanticLanguage};
use crate::stage_executor;
use crate::verification::VerificationStage;
use crate::workspace::{CommandResult, Workspace};
use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const MAX_QUALITY_FILES: usize = 10_000;
const MAX_SIGNAL_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityCapability {
    Format,
    Lint,
    TypeCheck,
    StaticAnalysis,
    Test,
    Security,
}

impl QualityCapability {
    const ALL: [Self; 6] = [
        Self::Format,
        Self::Lint,
        Self::TypeCheck,
        Self::StaticAnalysis,
        Self::Test,
        Self::Security,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Format => "format",
            Self::Lint => "lint",
            Self::TypeCheck => "type_check",
            Self::StaticAnalysis => "static_analysis",
            Self::Test => "test",
            Self::Security => "security",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityProviderSource {
    LanguageNative,
    RepositoryConfigured,
    Ecosystem,
}

#[derive(Clone, Debug, Serialize)]
pub struct QualityProviderStatus {
    pub id: String,
    pub capability: QualityCapability,
    pub source: QualityProviderSource,
    pub program: String,
    pub command: String,
    pub declared: bool,
    pub available: bool,
    pub runnable: bool,
    pub authorization_required: bool,
    pub check_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_format: Option<String>,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct LanguageQualityStatus {
    pub language: SemanticLanguage,
    pub detected_files: usize,
    pub syntax_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_provider: Option<String>,
    pub semantic_available: bool,
    pub semantic_runnable: bool,
    pub providers: Vec<QualityProviderStatus>,
    pub advanced_stages: Vec<String>,
    pub gaps: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct QualityDimensionCoverage {
    pub dimension: String,
    pub detected_languages: usize,
    pub covered_languages: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct LanguageQualityRegistry {
    pub provider: &'static str,
    pub languages: Vec<LanguageQualityStatus>,
    pub detected_languages: usize,
    pub dimensions: Vec<QualityDimensionCoverage>,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct LanguageQualityRun {
    pub provider_id: String,
    pub language: SemanticLanguage,
    pub capability: QualityCapability,
    pub success: bool,
    pub summary: String,
    pub command: CommandResult,
    pub evidence_records: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct QualityCandidate {
    pub(crate) id: String,
    pub(crate) capability: QualityCapability,
    pub(crate) source: QualityProviderSource,
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    pub(crate) declared: bool,
    pub(crate) check_only: bool,
    pub(crate) fail_on_stdout: bool,
    pub(crate) machine_format: Option<&'static str>,
    pub(crate) declaration: String,
}

#[derive(Default)]
pub(crate) struct RepoSignals {
    pub(crate) text: BTreeMap<&'static str, String>,
    pub(crate) package: Value,
}

impl RepoSignals {
    pub(crate) fn load(workspace: &Workspace) -> Self {
        let paths = [
            "Cargo.toml",
            "Cargo.lock",
            "Makefile",
            "package.json",
            "pyproject.toml",
            "requirements.txt",
            "go.mod",
            "pubspec.yaml",
            "mix.exs",
            "Gemfile",
            "composer.json",
            "dune-project",
            "DESCRIPTION",
            "Package.swift",
            "pom.xml",
            "build.gradle",
            "build.gradle.kts",
            "tsconfig.json",
            ".editorconfig",
            ".shellcheckrc",
            ".clang-format",
            ".clang-tidy",
            ".rubocop.yml",
            ".rubocop.yaml",
            ".luacheckrc",
            ".stylua.toml",
            ".ocamlformat",
            ".lintr",
            "phpstan.neon",
            "phpstan.neon.dist",
            "psalm.xml",
            ".swift-format",
            "biome.json",
            "biome.jsonc",
            "eslint.config.js",
            "eslint.config.mjs",
            ".eslintrc",
            ".eslintrc.json",
        ];
        let mut text = BTreeMap::new();
        for path in paths {
            if let Some(content) = read_small(workspace.root().join(path).as_path()) {
                text.insert(path, content);
            }
        }
        let package = text
            .get("package.json")
            .and_then(|content| serde_json::from_str(content).ok())
            .unwrap_or_default();
        Self { text, package }
    }

    pub(crate) fn has(&self, path: &str) -> bool {
        self.text.contains_key(path)
    }

    pub(crate) fn contains(&self, path: &str, needle: &str) -> bool {
        self.text.get(path).is_some_and(|content| {
            content
                .to_ascii_lowercase()
                .contains(&needle.to_ascii_lowercase())
        })
    }

    pub(crate) fn any_contains(&self, paths: &[&str], needle: &str) -> bool {
        paths.iter().any(|path| self.contains(path, needle))
    }

    pub(crate) fn package_dependency(&self, name: &str) -> bool {
        [
            "dependencies",
            "devDependencies",
            "peerDependencies",
            "optionalDependencies",
        ]
        .into_iter()
        .any(|section| {
            self.package
                .pointer(&format!("/{section}/{name}"))
                .is_some()
        })
    }

    pub(crate) fn package_script(&self, name: &str) -> bool {
        self.package
            .get("scripts")
            .and_then(Value::as_object)
            .is_some_and(|scripts| scripts.contains_key(name))
    }
}

pub fn registry(workspace: &Workspace) -> Result<LanguageQualityRegistry> {
    let (files, scan_truncated) = workspace.source_files(".", MAX_QUALITY_FILES)?;
    let mut files_by_language = BTreeMap::<SemanticLanguage, Vec<String>>::new();
    for path in files {
        if let Some(language) = language_for_path(&path) {
            files_by_language.entry(language).or_default().push(path);
        }
    }
    let signals = RepoSignals::load(workspace);
    let semantic = semantic_provider::status(workspace)?
        .into_iter()
        .map(|status| (status.language, status))
        .collect::<BTreeMap<_, _>>();
    let advanced = stage_executor::registry(workspace)?;
    let mut languages = Vec::with_capacity(SemanticLanguage::ALL.len());

    for language in SemanticLanguage::ALL {
        let language_files = files_by_language
            .get(&language)
            .cloned()
            .unwrap_or_default();
        let candidates = candidates_for(workspace, &signals, language, &language_files);
        let providers = candidates
            .iter()
            .map(|candidate| provider_status(workspace, candidate, !language_files.is_empty()))
            .collect::<Vec<_>>();
        let semantic_status = semantic.get(&language);
        let advanced_stages = advanced
            .coverage
            .get(language.as_str())
            .cloned()
            .unwrap_or_default();
        languages.push(LanguageQualityStatus {
            language,
            detected_files: language_files.len(),
            syntax_available: true,
            semantic_provider: semantic_status.and_then(|status| status.provider.clone()),
            semantic_available: semantic_status.is_some_and(|status| status.available),
            semantic_runnable: semantic_status.is_some_and(|status| status.runnable),
            gaps: quality_gaps(language, !language_files.is_empty(), &providers),
            providers,
            advanced_stages,
        });
    }

    let detected_languages = languages
        .iter()
        .filter(|language| language.detected_files > 0)
        .count();
    Ok(LanguageQualityRegistry {
        provider: "wcode-language-quality",
        dimensions: dimension_coverage(&languages),
        languages,
        detected_languages,
        truncated: scan_truncated,
    })
}

pub async fn execute(
    workspace: &Workspace,
    language: SemanticLanguage,
    provider_id: &str,
    timeout_seconds: u64,
) -> Result<LanguageQualityRun> {
    let registry = registry(workspace)?;
    let language_status = registry
        .languages
        .iter()
        .find(|status| status.language == language)
        .ok_or_else(|| anyhow::anyhow!("language is not represented by the quality registry"))?;
    let status = language_status
        .providers
        .iter()
        .find(|provider| provider.id == provider_id)
        .ok_or_else(|| anyhow::anyhow!("quality provider is not registered for this language"))?;
    if language_status.detected_files == 0 {
        bail!("quality provider cannot run because no matching source files were detected");
    }
    if !status.declared {
        bail!("quality provider is a known candidate but is not declared by this repository");
    }
    if !status.check_only {
        bail!("quality provider is repository-declared but not statically guaranteed check-only; run it only through an explicitly authorized project-execution path");
    }
    if !status.available {
        bail!("quality provider program is unavailable");
    }

    let signals = RepoSignals::load(workspace);
    let language_files = workspace
        .source_files(".", MAX_QUALITY_FILES)?
        .0
        .into_iter()
        .filter(|path| language_for_path(path) == Some(language))
        .collect::<Vec<_>>();
    let candidate = candidates_for(workspace, &signals, language, &language_files)
        .into_iter()
        .find(|candidate| candidate.id == provider_id)
        .ok_or_else(|| anyhow::anyhow!("quality provider changed while preparing execution"))?;
    let command = workspace
        .run_trusted_runtime_command(
            &candidate.program,
            &candidate.args,
            ".",
            timeout_seconds.clamp(1, 300),
        )
        .await?;
    let success =
        command.success && (!candidate.fail_on_stdout || command.stdout.trim().is_empty());
    let summary = if !command.success {
        format!(
            "{} failed with exit code {}.",
            candidate.id,
            command
                .exit_code
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_owned())
        )
    } else if candidate.fail_on_stdout && !command.stdout.trim().is_empty() {
        format!("{} reported source changes are required.", candidate.id)
    } else {
        format!(
            "{} completed successfully without modifying source.",
            candidate.id
        )
    };
    Ok(LanguageQualityRun {
        provider_id: candidate.id,
        language,
        capability: candidate.capability,
        success,
        summary,
        command,
        evidence_records: 0,
    })
}

fn provider_status(
    workspace: &Workspace,
    candidate: &QualityCandidate,
    language_present: bool,
) -> QualityProviderStatus {
    let available = program_available(workspace, &candidate.program);
    let runnable = language_present
        && candidate.declared
        && candidate.check_only
        && available
        && workspace.exec_enabled();
    let authorization_required = runnable && !workspace.risky_exec_enabled();
    let reason = if !language_present {
        "no matching source files detected".to_owned()
    } else if !candidate.declared {
        format!(
            "known provider candidate; repository has not declared it ({})",
            candidate.declaration
        )
    } else if !candidate.check_only {
        "repository declares this quality provider, but its command body is not statically guaranteed check-only; discovery only".to_owned()
    } else if !available {
        format!(
            "repository declares provider but program is unavailable: {}",
            candidate.program
        )
    } else if !workspace.exec_enabled() {
        "command execution is disabled".to_owned()
    } else if authorization_required {
        "provider is ready but execution requires explicit repository-aware authorization"
            .to_owned()
    } else {
        "provider is declared, available, and ready for check-only execution".to_owned()
    };
    QualityProviderStatus {
        id: candidate.id.clone(),
        capability: candidate.capability,
        source: candidate.source,
        program: candidate.program.clone(),
        command: command_text(&candidate.program, &candidate.args),
        declared: candidate.declared,
        available,
        runnable,
        authorization_required,
        check_only: candidate.check_only,
        machine_format: candidate.machine_format.map(str::to_owned),
        reason,
    }
}

fn quality_gaps(
    language: SemanticLanguage,
    detected: bool,
    providers: &[QualityProviderStatus],
) -> Vec<String> {
    if !detected {
        return Vec::new();
    }
    expected_capabilities(language)
        .into_iter()
        .filter(|capability| {
            !providers.iter().any(|provider| {
                provider.capability == *capability && provider.declared && provider.available
            })
        })
        .map(|capability| {
            format!(
                "no repository-declared and available {} provider",
                capability.as_str()
            )
        })
        .collect()
}

fn expected_capabilities(language: SemanticLanguage) -> Vec<QualityCapability> {
    use QualityCapability::{Format, Lint, Security, StaticAnalysis, Test, TypeCheck};
    match language {
        SemanticLanguage::Bash => vec![Format, Lint],
        SemanticLanguage::C | SemanticLanguage::Cpp => vec![Format, StaticAnalysis],
        SemanticLanguage::CSharp => vec![Format, StaticAnalysis, Test],
        SemanticLanguage::Css | SemanticLanguage::Html => vec![Format, Lint],
        SemanticLanguage::Dart => vec![Format, StaticAnalysis, Test],
        SemanticLanguage::Elixir => vec![Format, Lint, StaticAnalysis, Test],
        SemanticLanguage::Go => vec![Format, StaticAnalysis, Test, Security],
        SemanticLanguage::Java => vec![StaticAnalysis, Test],
        SemanticLanguage::JavaScript => vec![Format, Lint, Test],
        SemanticLanguage::Lua => vec![Format, Lint, Test],
        SemanticLanguage::Ocaml | SemanticLanguage::OcamlInterface => vec![Format, TypeCheck, Test],
        SemanticLanguage::Php => vec![Format, StaticAnalysis, Test],
        SemanticLanguage::Python => vec![Format, Lint, TypeCheck, Test, Security],
        SemanticLanguage::R => vec![Lint, Test],
        SemanticLanguage::Ruby => vec![Format, Lint, Test],
        SemanticLanguage::Rust => vec![Format, Lint, TypeCheck, Test, Security],
        SemanticLanguage::Swift => vec![Format, Lint, Test],
        SemanticLanguage::TypeScript | SemanticLanguage::Tsx => {
            vec![Format, Lint, TypeCheck, Test]
        }
    }
}

fn dimension_coverage(languages: &[LanguageQualityStatus]) -> Vec<QualityDimensionCoverage> {
    let detected = languages
        .iter()
        .filter(|language| language.detected_files > 0)
        .count();
    let mut coverage = vec![
        QualityDimensionCoverage {
            dimension: "syntax".to_owned(),
            detected_languages: detected,
            covered_languages: detected,
        },
        QualityDimensionCoverage {
            dimension: "semantic".to_owned(),
            detected_languages: detected,
            covered_languages: languages
                .iter()
                .filter(|language| language.detected_files > 0 && language.semantic_available)
                .count(),
        },
    ];
    coverage.extend(QualityCapability::ALL.into_iter().map(|capability| {
        QualityDimensionCoverage {
            dimension: capability.as_str().to_owned(),
            detected_languages: detected,
            covered_languages: languages
                .iter()
                .filter(|language| {
                    language.detected_files > 0
                        && language.providers.iter().any(|provider| {
                            provider.capability == capability
                                && provider.declared
                                && provider.available
                        })
                })
                .count(),
        }
    }));
    for (dimension, stage) in [
        ("property", VerificationStage::Property),
        ("mutation", VerificationStage::Mutation),
        ("fuzz", VerificationStage::Fuzz),
        ("runtime_canary", VerificationStage::RuntimeCanary),
    ] {
        let stage_name = format!("{stage:?}").to_ascii_lowercase();
        coverage.push(QualityDimensionCoverage {
            dimension: dimension.to_owned(),
            detected_languages: detected,
            covered_languages: languages
                .iter()
                .filter(|language| {
                    language.detected_files > 0 && language.advanced_stages.contains(&stage_name)
                })
                .count(),
        });
    }
    coverage
}

fn program_available(workspace: &Workspace, program: &str) -> bool {
    if program.contains(['/', '\\']) {
        workspace.workspace_program_available(program)
    } else {
        stage_executor::find_executable(program).is_some()
    }
}

fn read_small(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_SIGNAL_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn command_text(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn registry_represents_every_indexed_language_without_a_fake_support_boolean() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let registry = registry(&workspace).unwrap();
        assert_eq!(registry.languages.len(), SemanticLanguage::ALL.len());
        assert_eq!(registry.detected_languages, 1);
        let rust = registry
            .languages
            .iter()
            .find(|status| status.language == SemanticLanguage::Rust)
            .unwrap();
        assert_eq!(rust.detected_files, 1);
        assert!(rust.syntax_available);
        assert!(rust
            .providers
            .iter()
            .any(|provider| provider.id == "rustfmt" && provider.declared));
        assert!(registry
            .dimensions
            .iter()
            .any(|dimension| dimension.dimension == "fuzz"));
    }

    #[test]
    fn repository_scripts_are_first_class_quality_providers() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"lint":"eslint .","typecheck":"tsc --noEmit","test":"vitest run"},"devDependencies":{"typescript":"latest"}}"#,
        )
        .unwrap();
        fs::write(dir.path().join("tsconfig.json"), "{}\n").unwrap();
        fs::write(dir.path().join("index.ts"), "export const x: number = 1;\n").unwrap();
        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let registry = registry(&workspace).unwrap();
        let typescript = registry
            .languages
            .iter()
            .find(|status| status.language == SemanticLanguage::TypeScript)
            .unwrap();
        assert!(typescript
            .providers
            .iter()
            .any(|provider| provider.id == "package-lint" && provider.declared));
        assert!(typescript
            .providers
            .iter()
            .any(|provider| provider.id == "package-typecheck" && provider.declared));
        let package_lint = typescript
            .providers
            .iter()
            .find(|provider| provider.id == "package-lint")
            .unwrap();
        assert!(package_lint.declared);
        assert!(!package_lint.check_only);
        assert!(package_lint.reason.contains("discovery only"));
    }

    #[tokio::test]
    async fn arbitrary_repository_scripts_cannot_enter_the_strict_check_only_lane() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"lint":"node mutate-repository.js"}}"#,
        )
        .unwrap();
        fs::write(dir.path().join("index.js"), "export const value = 1;\n").unwrap();
        let workspace = Workspace::new(dir.path(), false, true).unwrap();
        let error = execute(&workspace, SemanticLanguage::JavaScript, "package-lint", 30)
            .await
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("not statically guaranteed check-only"));
    }
}
