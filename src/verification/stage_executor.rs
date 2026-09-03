use crate::semantic_provider::{language_for_path, SemanticLanguage};
use crate::verification::{ReviewVerdict, VerificationStage};
use crate::workspace::{CommandResult, Workspace};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::path::PathBuf;

const CONFIG_PATH: &str = ".wcode/executors.yaml";
const MAX_EXECUTORS: usize = 128;
const MAX_ARGS: usize = 64;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StageExecutorSpec {
    pub id: String,
    pub stage: VerificationStage,
    #[serde(default)]
    pub languages: Vec<SemanticLanguage>,
    pub program: String,
    #[serde(default, skip_serializing)]
    pub args: Vec<String>,
    #[serde(default = "default_cwd")]
    pub cwd: String,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub builtin: bool,
}

impl StageExecutorSpec {
    fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty()
            || self.id.len() > 160
            || self.program.trim().is_empty()
            || self.program.len() > 512
            || self.args.len() > MAX_ARGS
            || self.cwd.trim().is_empty()
            || self.cwd.len() > 512
            || self.timeout_seconds == 0
            || self.timeout_seconds > 300
            || self.args.iter().any(|arg| arg.len() > 2_000)
        {
            bail!("verification stage executor is invalid or exceeds its bounds");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ExecutorConfig {
    #[serde(default = "schema_version")]
    schema_version: u32,
    #[serde(default)]
    executors: Vec<StageExecutorSpec>,
}

#[derive(Clone, Debug, Serialize)]
pub struct StageExecutorEntry {
    pub spec: StageExecutorSpec,
    pub available: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct StageExecutorRegistry {
    pub configured: bool,
    pub config_path: &'static str,
    pub detected_languages: Vec<SemanticLanguage>,
    pub executors: Vec<StageExecutorEntry>,
    pub coverage: BTreeMap<String, Vec<String>>,
    pub universal_config: bool,
}

#[derive(Debug, Serialize)]
pub struct StageExecutionResult {
    pub executor_id: String,
    pub stage: VerificationStage,
    pub languages: Vec<SemanticLanguage>,
    pub verdict: ReviewVerdict,
    pub summary: String,
    pub artifact_digest: String,
    pub command: CommandResult,
}

pub fn registry(workspace: &Workspace) -> Result<StageExecutorRegistry> {
    let configured = workspace.root().join(CONFIG_PATH).is_file();
    let mut executors = if configured {
        let file = workspace.load_source(CONFIG_PATH)?;
        let config: ExecutorConfig =
            serde_yaml::from_str(&file.content).context("invalid .wcode/executors.yaml")?;
        if config.schema_version != 1 || config.executors.len() > MAX_EXECUTORS {
            bail!("executor config schema is unsupported or exceeds its bounds");
        }
        for executor in &config.executors {
            executor.validate()?;
        }
        config.executors
    } else {
        Vec::new()
    };
    executors.extend(discover_builtins(workspace)?);
    let mut seen = BTreeSet::new();
    executors.retain(|executor| seen.insert(executor.id.clone()));
    executors.truncate(MAX_EXECUTORS);

    let detected_languages = workspace
        .source_files(".", 10_000)?
        .0
        .iter()
        .filter_map(|path| language_for_path(path))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let executors = executors
        .into_iter()
        .map(|spec| {
            let available = executor_available(workspace, &spec);
            StageExecutorEntry {
                reason: if available {
                    "executor program is available".to_owned()
                } else {
                    format!("executor program is unavailable: {}", spec.program)
                },
                spec,
                available,
            }
        })
        .collect::<Vec<_>>();
    let mut coverage = BTreeMap::<String, Vec<String>>::new();
    for language in SemanticLanguage::ALL {
        let stages = executors
            .iter()
            .filter(|executor| {
                executor.available
                    && (executor.spec.languages.is_empty()
                        || executor.spec.languages.contains(&language))
            })
            .map(|executor| format!("{:?}", executor.spec.stage).to_ascii_lowercase())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        coverage.insert(language.as_str().to_owned(), stages);
    }
    Ok(StageExecutorRegistry {
        configured,
        config_path: CONFIG_PATH,
        detected_languages,
        executors,
        coverage,
        universal_config: true,
    })
}

pub async fn execute(
    workspace: &Workspace,
    executor: &StageExecutorSpec,
) -> Result<StageExecutionResult> {
    executor.validate()?;
    let mut command = workspace
        .run_trusted_runtime_command(
            &executor.program,
            &executor.args,
            &executor.cwd,
            executor.timeout_seconds,
        )
        .await?;
    let verdict = if command.success {
        ReviewVerdict::Pass
    } else {
        ReviewVerdict::Fail
    };
    let combined = format!(
        "{}\n{:?}\n{}\n{}",
        command.program, command.exit_code, command.stdout, command.stderr
    );
    let artifact_digest = format!("sha256:{:x}", Sha256::digest(combined.as_bytes()));
    let summary = if command.success {
        format!("{} completed successfully.", executor.id)
    } else {
        format!(
            "{} failed with exit code {}.",
            executor.id,
            command
                .exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_owned())
        )
    };
    if !executor.builtin && !command.args.is_empty() {
        command.args = vec!["[configured arguments omitted]".to_owned()];
    }
    Ok(StageExecutionResult {
        executor_id: executor.id.clone(),
        stage: executor.stage,
        languages: executor.languages.clone(),
        verdict,
        summary,
        artifact_digest,
        command,
    })
}

fn discover_builtins(workspace: &Workspace) -> Result<Vec<StageExecutorSpec>> {
    let mut executors = Vec::new();
    let cargo = optional_text(workspace, "Cargo.toml")?.unwrap_or_default();
    if contains_any(&cargo, &["proptest", "quickcheck"]) {
        executors.push(spec(
            "builtin-rust-property",
            VerificationStage::Property,
            &[SemanticLanguage::Rust],
            "cargo",
            &["test", "--locked"],
            180,
        ));
    }
    if find_executable("cargo-mutants").is_some() {
        executors.push(spec(
            "builtin-rust-mutation",
            VerificationStage::Mutation,
            &[SemanticLanguage::Rust],
            "cargo-mutants",
            &[],
            300,
        ));
    }
    if let Some(target) = first_rust_fuzz_target(workspace)? {
        if find_executable("cargo-fuzz").is_some() {
            executors.push(spec_owned(
                "builtin-rust-fuzz",
                VerificationStage::Fuzz,
                vec![SemanticLanguage::Rust],
                "cargo-fuzz",
                vec![
                    "fuzz".into(),
                    "run".into(),
                    target,
                    "--".into(),
                    "-max_total_time=5".into(),
                ],
                60,
            ));
        }
    }

    let go_mod = optional_text(workspace, "go.mod")?.unwrap_or_default();
    if !go_mod.is_empty()
        && (!workspace.search("testing/quick", ".", 1)?.is_empty()
            || !workspace.search("pgregory.net/rapid", ".", 1)?.is_empty()
            || !workspace.search("gopter", ".", 1)?.is_empty())
    {
        executors.push(spec(
            "builtin-go-property",
            VerificationStage::Property,
            &[SemanticLanguage::Go],
            "go",
            &["test", "./..."],
            180,
        ));
    }
    if !go_mod.is_empty() {
        if let Some(fuzz_name) = first_go_fuzz_target(workspace)? {
            executors.push(spec_owned(
                "builtin-go-fuzz",
                VerificationStage::Fuzz,
                vec![SemanticLanguage::Go],
                "go",
                vec![
                    "test".into(),
                    "-run=^$".into(),
                    format!("-fuzz={fuzz_name}"),
                    "-fuzztime=5s".into(),
                    "./...".into(),
                ],
                60,
            ));
        }
    }

    let python = format!(
        "{}\n{}",
        optional_text(workspace, "pyproject.toml")?.unwrap_or_default(),
        optional_text(workspace, "requirements.txt")?.unwrap_or_default()
    );
    if python.to_ascii_lowercase().contains("hypothesis") {
        executors.push(spec(
            "builtin-python-property",
            VerificationStage::Property,
            &[SemanticLanguage::Python],
            "pytest",
            &["-q"],
            180,
        ));
    }
    if find_executable("mutmut").is_some() && !python.is_empty() {
        executors.push(spec(
            "builtin-python-mutation",
            VerificationStage::Mutation,
            &[SemanticLanguage::Python],
            "mutmut",
            &["run"],
            300,
        ));
    }

    if let Some(package) = optional_text(workspace, "package.json")? {
        let package: serde_json::Value = serde_json::from_str(&package).unwrap_or_default();
        let languages = [
            SemanticLanguage::JavaScript,
            SemanticLanguage::TypeScript,
            SemanticLanguage::Tsx,
        ];
        if package_has_dependency(&package, "fast-check") && package_has_script(&package, "test") {
            let (program, args) = package_test_command(workspace);
            executors.push(spec_owned(
                "builtin-js-property",
                VerificationStage::Property,
                languages.to_vec(),
                &program,
                args,
                180,
            ));
        }
        if package_has_dependency(&package, "@stryker-mutator/core") {
            let script = ["mutation", "mutate"]
                .into_iter()
                .find(|script| package_has_script(&package, script));
            if let Some(script) = script {
                let (program, args) = package_run_command(workspace, script);
                executors.push(spec_owned(
                    "builtin-js-mutation",
                    VerificationStage::Mutation,
                    languages.to_vec(),
                    &program,
                    args,
                    300,
                ));
            }
        }
    }

    for (path, needle, id, language, program, args) in [
        (
            "Package.swift",
            "SwiftCheck",
            "builtin-swift-property",
            SemanticLanguage::Swift,
            "swift",
            vec!["test"],
        ),
        (
            "mix.exs",
            "stream_data",
            "builtin-elixir-property",
            SemanticLanguage::Elixir,
            "mix",
            vec!["test"],
        ),
        (
            "pubspec.yaml",
            "glados",
            "builtin-dart-property",
            SemanticLanguage::Dart,
            "dart",
            vec!["test"],
        ),
        (
            "Gemfile",
            "rantly",
            "builtin-ruby-property",
            SemanticLanguage::Ruby,
            "bundle",
            vec!["exec", "rspec"],
        ),
        (
            "composer.json",
            "eris",
            "builtin-php-property",
            SemanticLanguage::Php,
            "./vendor/bin/phpunit",
            vec![],
        ),
        (
            "dune-project",
            "qcheck",
            "builtin-ocaml-property",
            SemanticLanguage::Ocaml,
            "dune",
            vec!["runtest"],
        ),
        (
            "DESCRIPTION",
            "quickcheck",
            "builtin-r-property",
            SemanticLanguage::R,
            "Rscript",
            vec!["-e", "testthat::test_dir('tests/testthat')"],
        ),
    ] {
        if optional_text(workspace, path)?.is_some_and(|text| {
            text.to_ascii_lowercase()
                .contains(&needle.to_ascii_lowercase())
        }) {
            executors.push(spec_owned(
                id,
                VerificationStage::Property,
                vec![language],
                program,
                args.into_iter().map(str::to_owned).collect(),
                180,
            ));
        }
    }

    if !workspace.search("FsCheck", ".", 1)?.is_empty() {
        executors.push(spec(
            "builtin-csharp-property",
            VerificationStage::Property,
            &[SemanticLanguage::CSharp],
            "dotnet",
            &["test"],
            180,
        ));
    }
    let java_build = format!(
        "{}\n{}\n{}",
        optional_text(workspace, "pom.xml")?.unwrap_or_default(),
        optional_text(workspace, "build.gradle")?.unwrap_or_default(),
        optional_text(workspace, "build.gradle.kts")?.unwrap_or_default()
    );
    if contains_any(
        &java_build.to_ascii_lowercase(),
        &["jqwik", "quicktheories"],
    ) {
        if workspace.root().join("pom.xml").is_file() {
            executors.push(spec(
                "builtin-java-property",
                VerificationStage::Property,
                &[SemanticLanguage::Java],
                "mvn",
                &["test"],
                180,
            ));
        } else if workspace.root().join("gradlew").is_file() {
            executors.push(spec(
                "builtin-java-property",
                VerificationStage::Property,
                &[SemanticLanguage::Java],
                "./gradlew",
                &["test"],
                180,
            ));
        }
    }
    if java_build.to_ascii_lowercase().contains("pitest")
        && workspace.root().join("pom.xml").is_file()
    {
        executors.push(spec(
            "builtin-java-mutation",
            VerificationStage::Mutation,
            &[SemanticLanguage::Java],
            "mvn",
            &["test-compile", "org.pitest:pitest-maven:mutationCoverage"],
            300,
        ));
    }
    if find_executable("dotnet-stryker").is_some() {
        executors.push(spec(
            "builtin-csharp-mutation",
            VerificationStage::Mutation,
            &[SemanticLanguage::CSharp],
            "dotnet-stryker",
            &[],
            300,
        ));
    }
    if find_executable("infection").is_some() {
        executors.push(spec(
            "builtin-php-mutation",
            VerificationStage::Mutation,
            &[SemanticLanguage::Php],
            "infection",
            &["--no-progress"],
            300,
        ));
    }
    if find_executable("muter").is_some() {
        executors.push(spec(
            "builtin-swift-mutation",
            VerificationStage::Mutation,
            &[SemanticLanguage::Swift],
            "muter",
            &[],
            300,
        ));
    }
    Ok(executors)
}

fn spec(
    id: &str,
    stage: VerificationStage,
    languages: &[SemanticLanguage],
    program: &str,
    args: &[&str],
    timeout_seconds: u64,
) -> StageExecutorSpec {
    spec_owned(
        id,
        stage,
        languages.to_vec(),
        program,
        args.iter().map(|arg| (*arg).to_owned()).collect(),
        timeout_seconds,
    )
}

fn spec_owned(
    id: &str,
    stage: VerificationStage,
    languages: Vec<SemanticLanguage>,
    program: &str,
    args: Vec<String>,
    timeout_seconds: u64,
) -> StageExecutorSpec {
    StageExecutorSpec {
        id: id.to_owned(),
        stage,
        languages,
        program: program.to_owned(),
        args,
        cwd: ".".into(),
        timeout_seconds,
        builtin: true,
    }
}

fn optional_text(workspace: &Workspace, path: &str) -> Result<Option<String>> {
    if !workspace.root().join(path).is_file() {
        return Ok(None);
    }
    Ok(Some(workspace.read_file(path, 1, None)?.content))
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn package_has_dependency(package: &serde_json::Value, name: &str) -> bool {
    [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ]
    .into_iter()
    .any(|section| package.pointer(&format!("/{section}/{name}")).is_some())
}

fn package_has_script(package: &serde_json::Value, name: &str) -> bool {
    package
        .get("scripts")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|scripts| scripts.contains_key(name))
}

fn package_test_command(workspace: &Workspace) -> (String, Vec<String>) {
    package_run_command(workspace, "test")
}

fn package_run_command(workspace: &Workspace, script: &str) -> (String, Vec<String>) {
    if workspace.root().join("pnpm-lock.yaml").is_file() {
        ("pnpm".into(), vec!["run".into(), script.into()])
    } else if workspace.root().join("yarn.lock").is_file() {
        ("yarn".into(), vec!["run".into(), script.into()])
    } else if workspace.root().join("bun.lock").is_file()
        || workspace.root().join("bun.lockb").is_file()
    {
        ("bun".into(), vec!["run".into(), script.into()])
    } else {
        ("npm".into(), vec!["run".into(), script.into()])
    }
}

pub(crate) fn language_target(language: SemanticLanguage) -> String {
    format!("language:{}", language.as_str())
}

pub(crate) fn executor_targets(
    executor: &StageExecutorSpec,
    required_targets: &[String],
) -> Vec<String> {
    if required_targets.is_empty() {
        return Vec::new();
    }
    if executor.languages.is_empty() {
        return required_targets.to_vec();
    }
    let supported = executor
        .languages
        .iter()
        .copied()
        .map(language_target)
        .collect::<BTreeSet<_>>();
    required_targets
        .iter()
        .filter(|target| supported.contains(*target))
        .cloned()
        .collect()
}

pub(crate) fn stage_target_available(
    registry: &StageExecutorRegistry,
    stage: VerificationStage,
    target: &str,
) -> bool {
    registry.executors.iter().any(|executor| {
        executor.available
            && executor.spec.stage == stage
            && (executor.spec.languages.is_empty()
                || executor
                    .spec
                    .languages
                    .iter()
                    .copied()
                    .map(language_target)
                    .any(|supported| supported == target))
    })
}

fn first_rust_fuzz_target(workspace: &Workspace) -> Result<Option<String>> {
    let root = workspace.root().join("fuzz/fuzz_targets");
    if !root.is_dir() {
        return Ok(None);
    }
    let mut targets = std::fs::read_dir(root)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .filter_map(|path| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
        })
        .collect::<Vec<_>>();
    targets.sort();
    Ok(targets.into_iter().next())
}

fn first_go_fuzz_target(workspace: &Workspace) -> Result<Option<String>> {
    let (files, _) = workspace.source_files(".", 5_000)?;
    for path in files.into_iter().filter(|path| path.ends_with("_test.go")) {
        let source = workspace.read_file(&path, 1, None)?.content;
        for line in source.lines() {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("func Fuzz") {
                if let Some(name) = rest
                    .split('(')
                    .next()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    return Ok(Some(format!("Fuzz{name}")));
                }
            }
        }
    }
    Ok(None)
}

fn executor_available(workspace: &Workspace, executor: &StageExecutorSpec) -> bool {
    if executor.program.contains(['/', '\\']) {
        workspace.workspace_program_available(&executor.program)
    } else {
        find_executable(&executor.program).is_some()
    }
}

pub(crate) fn find_executable(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    #[cfg(windows)]
    let extensions = env::var_os("PATHEXT")
        .map(|value| {
            value
                .to_string_lossy()
                .split(';')
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![".EXE".into(), ".CMD".into(), ".BAT".into()]);
    for directory in env::split_paths(&path) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        for extension in &extensions {
            let candidate = directory.join(format!("{name}{extension}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

const fn default_timeout() -> u64 {
    120
}

fn default_cwd() -> String {
    ".".into()
}

const fn schema_version() -> u32 {
    1
}

#[cfg(test)]
#[path = "../../tests/unit/verification/stage.rs"]
mod tests;
