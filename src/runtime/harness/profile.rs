use super::*;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};

#[path = "contracts.rs"]
mod contracts;
#[path = "islands.rs"]
mod islands;
pub(super) use contracts::contract_freshness_advisories;
use contracts::{contract_config_paths, discover_contract_topology};
use islands::{
    attach_manifest_dependencies, discover_nested_project_islands, languages_for_project_types,
    manifest_candidate_dirs, manifest_project_types,
};
pub(super) use islands::{
    verification_checks_for_impact, verification_gaps_for_impact, verification_impact_for_snapshot,
    ProjectIslandVerificationGap,
};
#[cfg(test)]
pub(super) use islands::{verification_checks_for_snapshot, verification_gaps_for_snapshot};

const MAX_PROFILE_ISLANDS: usize = 32;
const MAX_PROFILE_SCAN_DEPTH: usize = 8;
const MAX_PROFILE_SCAN_ENTRIES: usize = 10_000;

impl ToolHarness {
    pub(super) fn load_project_profile(
        &self,
        workspace: &Workspace,
    ) -> Result<(Arc<ProjectProfile>, bool)> {
        let root = workspace.root().to_path_buf();
        let fingerprint = project_fingerprint(workspace);
        {
            let mut cache = self
                .project_cache
                .lock()
                .map_err(|_| anyhow::anyhow!("project context cache poisoned"))?;
            if let Some(cached) = cache
                .get_mut(&root)
                .filter(|cached| cached.fingerprint == fingerprint)
            {
                cached.last_used = Instant::now();
                return Ok((cached.profile.clone(), true));
            }
        }

        // Build outside the cache lock so context discovery for one workspace does not
        // block independent requests for other workspaces.
        let built = Arc::new(build_project_profile(workspace)?);
        let mut cache = self
            .project_cache
            .lock()
            .map_err(|_| anyhow::anyhow!("project context cache poisoned"))?;
        if let Some(cached) = cache
            .get_mut(&root)
            .filter(|cached| cached.fingerprint == fingerprint)
        {
            cached.last_used = Instant::now();
            return Ok((cached.profile.clone(), true));
        }
        let limit = crate::resource::limits().project_cache_limit();
        if cache.len() >= limit {
            if let Some(oldest) = cache
                .iter()
                .min_by(|(_, left), (_, right)| left.last_used.cmp(&right.last_used))
                .map(|(key, _)| key.clone())
            {
                cache.remove(&oldest);
            }
        }
        cache.insert(
            root,
            CachedProjectProfile {
                fingerprint,
                last_used: Instant::now(),
                profile: built.clone(),
            },
        );
        Ok((built, false))
    }
}

fn build_project_profile(workspace: &Workspace) -> Result<ProjectProfile> {
    let root = workspace.root();
    let mut manifests = MANIFEST_FILES
        .iter()
        .filter(|path| root.join(path).is_file())
        .map(|path| (*path).to_owned())
        .collect::<Vec<_>>();
    let mut project_types = manifest_project_types(root);
    let mut checks = Vec::new();

    if root.join(".git").exists() {
        push_check(
            &mut checks,
            "git-diff-check",
            "quick",
            "git",
            &["diff", "--check"],
            "Detect conflict markers and whitespace errors in the current change.",
        );
    }

    let root_types = project_types.iter().cloned().collect::<Vec<_>>();
    add_island_checks(root, &root_types, &mut checks);
    let root_languages = languages_for_project_types(&root_types);
    for check in checks
        .iter_mut()
        .filter(|check| check.id != "git-diff-check")
    {
        check.island = ".".to_owned();
        check.languages = root_languages.clone();
    }
    let mut islands = Vec::new();
    if !root_types.is_empty() {
        islands.push(ProjectIsland {
            id: ".".to_owned(),
            root: ".".to_owned(),
            project_types: root_types.clone(),
            languages: root_languages,
            manifests: manifests.clone(),
            check_ids: checks
                .iter()
                .filter(|check| check.id != "git-diff-check")
                .map(|check| check.id.clone())
                .collect(),
            dependencies: Vec::new(),
            verification_status: "pending",
            verification_gaps: Vec::new(),
            provider: "manifest-discovery",
            precision: "structural",
        });
    }

    for mut island in discover_nested_project_islands(root, &root_types) {
        let start = checks.len();
        add_island_checks(
            &island.absolute_root,
            &island.descriptor.project_types,
            &mut checks,
        );
        for check in &mut checks[start..] {
            check.cwd = island.descriptor.root.clone();
            check.island = island.descriptor.id.clone();
            check.languages = island.descriptor.languages.clone();
            check.id = format!("{}:{}", island.descriptor.id, check.id);
        }
        island.descriptor.check_ids = checks[start..]
            .iter()
            .map(|check| check.id.clone())
            .collect();
        project_types.extend(island.descriptor.project_types.iter().cloned());
        manifests.extend(island.descriptor.manifests.iter().cloned());
        islands.push(island.descriptor);
    }

    if project_types.is_empty() {
        project_types.insert("generic".to_owned());
        islands.push(ProjectIsland {
            id: ".".to_owned(),
            root: ".".to_owned(),
            project_types: vec!["generic".to_owned()],
            languages: Vec::new(),
            manifests: Vec::new(),
            check_ids: Vec::new(),
            dependencies: Vec::new(),
            verification_status: "unknown",
            verification_gaps: Vec::new(),
            provider: "manifest-discovery",
            precision: "structural",
        });
    }

    attach_manifest_dependencies(root, &mut islands);
    let contracts = discover_contract_topology(root, &islands);
    manifests.sort();
    manifests.dedup();
    deduplicate_checks(&mut checks);
    let guidance = collect_guidance(workspace)?;
    Ok(ProjectProfile {
        root: root.display().to_string(),
        project_types: project_types.into_iter().collect(),
        manifests,
        islands,
        contracts,
        guidance,
        recommended_checks: checks,
        workflow: vec![
            "Start coding from agent_context(goal, scopes=...) and follow readiness/next_actions; retrieve broader Design State, Product Scope, and language-quality context only when the task needs it.".to_owned(),
            "Read the returned repository guidance before substantial edits.".to_owned(),
            "Use find_symbol/search_code for cheap localization; when readiness identifies syntax-only cross-file references, callers, implementations, rename impact, or equivalent relationships, use semantic_navigation and its warm provider session.".to_owned(),
            "For broad architecture or ownership work, call scope_status and treat relevant unmapped supported source as architecture debt before adding production modules.".to_owned(),
            "Use search_many and read_files to collect relevant implementation and tests in few round trips."
                .to_owned(),
            "Batch writes when targets are already known: use one apply_edits for multiple changes in a file, apply_file_edits for independent existing files, and create_files for independent new files instead of serial single-file tool calls."
                .to_owned(),
            "Decompose work into dependency lanes before execution. Run independent discovery, reads, reviews, and file-local edits concurrently through separate top-level tool calls when the host supports them; serialize only true dependencies. Use parallel_tools only for compact fan-out, not to wrap large nested argument payloads. Never treat worker consensus as deterministic proof.".to_owned(),
            "In polyglot repositories, use manifest-owned project islands and their cwd-bound checks. Treat cross-language ownership as structural unless semantic/runtime evidence explicitly strengthens it.".to_owned(),
            "Keep mandatory policy in deterministic Harness gates and Evidence rather than relying on an agent instruction to remember it.".to_owned(),
            "Prefer the smallest coherent change that preserves existing architecture and public behavior."
                .to_owned(),
            "Read every edited file first and keep SHA-256 preconditions on writes.".to_owned(),
            "Run verify_project with level=quick after edits; run level=full before release-sized changes."
                .to_owned(),
            "Report checks actually run, failures that remain, and any assumptions that were not verified."
                .to_owned(),
        ],
        write_enabled: workspace.write_enabled(),
        exec_enabled: workspace.exec_enabled(),
    })
}

fn add_island_checks(root: &Path, project_types: &[String], checks: &mut Vec<CheckSpec>) {
    let has_type = |name: &str| {
        project_types
            .iter()
            .any(|project_type| project_type == name)
    };
    if has_type("rust") {
        let locked = root.join("Cargo.lock").is_file();
        push_check(
            checks,
            "rust-format",
            "quick",
            "cargo",
            &["fmt", "--check"],
            "Verify Rust formatting without modifying files.",
        );
        push_cargo_check(
            checks,
            "rust-check",
            "quick",
            "check",
            locked,
            "Type-check the complete Rust workspace.",
        );
        let nextest_declared =
            root.join(".config/nextest.toml").is_file() || root.join("nextest.toml").is_file();
        let nextest_available = stage_executor::find_executable("cargo-nextest").is_some();
        if nextest_declared && nextest_available {
            let mut args = vec!["nextest".to_owned(), "run".to_owned()];
            if locked {
                args.push("--locked".to_owned());
            }
            push_check_owned(
                checks,
                "rust-nextest",
                "full",
                "cargo",
                args,
                "Run the Rust test suite with cargo-nextest's parallel test runner.",
            );
        } else {
            push_cargo_check(
                checks,
                "rust-test",
                "full",
                "test",
                locked,
                "Run the Rust test suite.",
            );
        }
        let mut clippy_args = vec!["clippy".to_owned()];
        if locked {
            clippy_args.push("--locked".to_owned());
        }
        clippy_args.extend([
            "--all-targets".to_owned(),
            "--".to_owned(),
            "-D".to_owned(),
            "warnings".to_owned(),
        ]);
        push_check_owned(
            checks,
            "rust-clippy",
            "full",
            "cargo",
            clippy_args,
            "Run Clippy on all targets, including tests, and treat warnings as quality-gate failures.",
        );
        let mut release_args = vec!["build".to_owned(), "--release".to_owned()];
        if locked {
            release_args.push("--locked".to_owned());
        }
        push_check_owned(
            checks,
            "rust-release-build",
            "full",
            "cargo",
            release_args,
            "Build the optimized release binary with the locked dependency graph.",
        );
    }
    if has_type("node") {
        add_node_checks(root, checks);
    }
    if has_type("python") {
        push_check(
            checks,
            "python-tests",
            "full",
            "pytest",
            &["-q"],
            "Run the Python test suite with concise output.",
        );
    }
    if has_type("go") {
        push_check(
            checks,
            "go-vet",
            "quick",
            "go",
            &["vet", "./..."],
            "Run Go's static analysis across all packages.",
        );
        push_check(
            checks,
            "go-tests",
            "full",
            "go",
            &["test", "./..."],
            "Compile and test all Go packages.",
        );
    }
    if has_type("java") {
        if root.join("pom.xml").is_file() {
            push_check(
                checks,
                "java-maven-compile",
                "quick",
                "mvn",
                &["-q", "-DskipTests", "compile"],
                "Compile the owning Maven island without running its test suite.",
            );
            push_check(
                checks,
                "java-maven-test",
                "full",
                "mvn",
                &["test"],
                "Run the Maven test lifecycle for the owning Java island.",
            );
        } else if root.join("build.gradle").is_file() || root.join("build.gradle.kts").is_file() {
            push_check(
                checks,
                "java-gradle-classes",
                "quick",
                "gradle",
                &["classes"],
                "Compile the owning Gradle island without running its full verification lifecycle.",
            );
            push_check(
                checks,
                "java-gradle-check",
                "full",
                "gradle",
                &["check"],
                "Run the Gradle verification lifecycle for the owning Java island.",
            );
        }
    }
    if has_type("swift") {
        push_check(
            checks,
            "swift-build",
            "quick",
            "swift",
            &["build"],
            "Compile the owning Swift package.",
        );
        push_check(
            checks,
            "swift-test",
            "full",
            "swift",
            &["test"],
            "Build and test the owning Swift package.",
        );
    }
    if has_type("dart") {
        push_check(
            checks,
            "dart-format",
            "quick",
            "dart",
            &["format", "-o", "none", "--set-exit-if-changed", "."],
            "Verify Dart formatting without modifying source.",
        );
        push_check(
            checks,
            "dart-analyze",
            "quick",
            "dart",
            &["analyze"],
            "Run Dart static analysis for the owning package.",
        );
        push_check(
            checks,
            "dart-test",
            "full",
            "dart",
            &["test"],
            "Run the Dart test suite for the owning package.",
        );
    }
    if has_type("elixir") {
        push_check(
            checks,
            "elixir-format",
            "quick",
            "mix",
            &["format", "--check-formatted"],
            "Verify Elixir formatting without modifying source.",
        );
        push_check(
            checks,
            "elixir-compile",
            "quick",
            "mix",
            &["compile", "--warnings-as-errors"],
            "Compile the owning Mix project and treat warnings as failures.",
        );
        push_check(
            checks,
            "elixir-test",
            "full",
            "mix",
            &["test"],
            "Run the Elixir test suite for the owning Mix project.",
        );
    }
    if has_type("ocaml") {
        push_check(
            checks,
            "ocaml-build",
            "quick",
            "dune",
            &["build"],
            "Build and type-check the owning Dune project.",
        );
        push_check(
            checks,
            "ocaml-test",
            "full",
            "dune",
            &["runtest"],
            "Run the Dune test aliases for the owning OCaml project.",
        );
    }
    if has_type("php") {
        let composer = read_small_text(&root.join("composer.json"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        if composer.contains("phpstan")
            || root.join("phpstan.neon").is_file()
            || root.join("phpstan.neon.dist").is_file()
        {
            push_check_owned(
                checks,
                "php-phpstan",
                "quick",
                &php_quality_program(root, "phpstan"),
                vec!["analyse".into(), "--error-format=json".into()],
                "Run the repository-declared PHPStan static-analysis gate.",
            );
        }
        if composer.contains("psalm") || root.join("psalm.xml").is_file() {
            push_check_owned(
                checks,
                "php-psalm",
                "quick",
                &php_quality_program(root, "psalm"),
                vec!["--output-format=json".into()],
                "Run the repository-declared Psalm static-analysis gate.",
            );
        }
        if composer.contains("php-cs-fixer")
            || root.join(".php-cs-fixer.php").is_file()
            || root.join(".php-cs-fixer.dist.php").is_file()
        {
            push_check_owned(
                checks,
                "php-format",
                "quick",
                &php_quality_program(root, "php-cs-fixer"),
                vec!["fix".into(), "--dry-run".into(), "--diff".into()],
                "Verify PHP CS Fixer output without modifying source.",
            );
        }
        if composer.contains("phpunit")
            || root.join("phpunit.xml").is_file()
            || root.join("phpunit.xml.dist").is_file()
        {
            push_check_owned(
                checks,
                "php-phpunit",
                "full",
                &php_quality_program(root, "phpunit"),
                Vec::new(),
                "Run the repository-declared PHPUnit suite.",
            );
        }
    }
    if has_type("ruby") {
        let gemfile = read_small_text(&root.join("Gemfile"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        if gemfile.contains("rubocop")
            || root.join(".rubocop.yml").is_file()
            || root.join(".rubocop.yaml").is_file()
        {
            push_check(
                checks,
                "ruby-rubocop",
                "quick",
                "bundle",
                &["exec", "rubocop", "--format", "json"],
                "Run the repository-declared RuboCop lint gate without source mutation.",
            );
        }
        if gemfile.contains("rspec") || root.join("spec").is_dir() {
            push_check(
                checks,
                "ruby-rspec",
                "full",
                "bundle",
                &["exec", "rspec"],
                "Run the repository-declared RSpec test suite.",
            );
        }
    }
    if has_type("make") {
        add_make_checks(root, checks);
    }
}

fn collect_guidance(workspace: &Workspace) -> Result<Vec<GuidanceDocument>> {
    let mut remaining = MAX_GUIDANCE_CHARS_TOTAL;
    let mut documents = Vec::new();
    for path in GUIDANCE_FILES {
        if remaining == 0 || !workspace.root().join(path).is_file() {
            continue;
        }
        let view = workspace.read_file(path, 1, Some(MAX_GUIDANCE_LINES_PER_FILE))?;
        let limit = remaining.min(MAX_GUIDANCE_CHARS_PER_FILE);
        let (excerpt, excerpt_truncated) = truncate_chars(&view.content, limit);
        remaining = remaining.saturating_sub(excerpt.chars().count());
        let included_lines = if excerpt.is_empty() {
            0
        } else {
            excerpt.lines().count()
        };
        documents.push(GuidanceDocument {
            path: (*path).to_owned(),
            excerpt,
            included_lines,
            total_lines: view.total_lines,
            truncated: excerpt_truncated || view.end_line < view.total_lines,
            redacted: view.redacted,
        });
    }
    Ok(documents)
}

fn add_node_checks(root: &Path, checks: &mut Vec<CheckSpec>) {
    let Some(content) = read_small_text(&root.join("package.json")) else {
        return;
    };
    let Ok(package) = serde_json::from_str::<Value>(&content) else {
        return;
    };
    let Some(scripts) = package.get("scripts").and_then(Value::as_object) else {
        return;
    };
    let runner = node_runner(root);
    for (name, level, reason) in [
        (
            "lint",
            "quick",
            "Run the repository's JavaScript/TypeScript lint script.",
        ),
        (
            "typecheck",
            "quick",
            "Run the repository's static type-check script.",
        ),
        (
            "check",
            "quick",
            "Run the repository's general validation script.",
        ),
        (
            "format:check",
            "quick",
            "Verify repository formatting without writing files.",
        ),
        (
            "test",
            "full",
            "Run the repository's JavaScript/TypeScript tests.",
        ),
        (
            "build",
            "full",
            "Build the project to catch integration and bundling errors.",
        ),
    ] {
        let Some(command) = scripts.get(name).and_then(Value::as_str) else {
            continue;
        };
        if name == "test" && command.contains("no test specified") {
            continue;
        }
        push_check_owned(
            checks,
            &format!("node-{name}"),
            level,
            &runner,
            vec!["run".to_owned(), name.to_owned()],
            reason,
        );
    }
}

fn add_make_checks(root: &Path, checks: &mut Vec<CheckSpec>) {
    let Some(content) = read_small_text(&root.join("Makefile")) else {
        return;
    };
    let targets = content
        .lines()
        .filter_map(|line| {
            let line = line.trim_end();
            if line.starts_with(['\t', '#', ' ']) {
                return None;
            }
            line.split_once(':')
                .map(|(target, _)| target.trim())
                .filter(|target| !target.is_empty() && !target.contains(char::is_whitespace))
                .map(str::to_owned)
        })
        .collect::<HashSet<_>>();
    for (target, level, reason) in [
        (
            "check",
            "quick",
            "Run the Makefile's repository validation target.",
        ),
        ("lint", "quick", "Run the Makefile's lint target."),
        ("test", "full", "Run the Makefile's test target."),
    ] {
        if targets.contains(target) {
            push_check(
                checks,
                &format!("make-{target}"),
                level,
                "make",
                &[target],
                reason,
            );
        }
    }
}

fn php_quality_program(root: &Path, name: &str) -> String {
    let relative = format!("vendor/bin/{name}");
    if root.join(&relative).is_file() {
        relative
    } else {
        name.to_owned()
    }
}

fn read_small_text(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_PROFILE_SOURCE_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn node_runner(root: &Path) -> String {
    if root.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if root.join("yarn.lock").is_file() {
        "yarn"
    } else if root.join("bun.lock").is_file() || root.join("bun.lockb").is_file() {
        "bun"
    } else {
        "npm"
    }
    .to_owned()
}

fn push_cargo_check(
    checks: &mut Vec<CheckSpec>,
    id: &str,
    level: &str,
    subcommand: &str,
    locked: bool,
    reason: &str,
) {
    let mut args = vec![subcommand.to_owned()];
    if locked {
        args.push("--locked".to_owned());
    }
    push_check_owned(checks, id, level, "cargo", args, reason);
}

fn push_check(
    checks: &mut Vec<CheckSpec>,
    id: &str,
    level: &str,
    program: &str,
    args: &[&str],
    reason: &str,
) {
    push_check_owned(
        checks,
        id,
        level,
        program,
        args.iter().map(|arg| (*arg).to_owned()).collect(),
        reason,
    );
}

fn push_check_owned(
    checks: &mut Vec<CheckSpec>,
    id: &str,
    level: &str,
    program: &str,
    args: Vec<String>,
    reason: &str,
) {
    checks.push(CheckSpec {
        id: id.to_owned(),
        level: level.to_owned(),
        phase: verification_phase(id),
        program: program.to_owned(),
        args,
        cwd: ".".to_owned(),
        island: "workspace".to_owned(),
        languages: Vec::new(),
        reason: reason.to_owned(),
    });
}

fn deduplicate_checks(checks: &mut Vec<CheckSpec>) {
    let mut seen = HashSet::new();
    checks.retain(|check| {
        seen.insert((check.cwd.clone(), check.program.clone(), check.args.clone()))
    });
    sort_checks(checks);
}

fn project_fingerprint(workspace: &Workspace) -> u64 {
    let root = workspace.root();
    let mut hasher = DefaultHasher::new();
    root.hash(&mut hasher);
    workspace.write_enabled().hash(&mut hasher);
    workspace.exec_enabled().hash(&mut hasher);
    hash_profile_directory(root, root, &mut hasher);
    for directory in manifest_candidate_dirs(root) {
        hash_profile_directory(root, &directory, &mut hasher);
    }
    for path in contract_config_paths(root) {
        hash_profile_path(root, &path, &mut hasher);
    }
    hasher.finish()
}

fn hash_profile_path(root: &Path, path: &Path, hasher: &mut DefaultHasher) {
    path.strip_prefix(root).unwrap_or(path).hash(hasher);
    if let Ok(metadata) = fs::symlink_metadata(path) {
        metadata.len().hash(hasher);
        metadata.is_file().hash(hasher);
        metadata.file_type().is_symlink().hash(hasher);
        if let Ok(modified) = metadata.modified() {
            if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                duration.as_nanos().hash(hasher);
            }
        }
    }
}

fn hash_profile_directory(root: &Path, directory: &Path, hasher: &mut DefaultHasher) {
    directory
        .strip_prefix(root)
        .unwrap_or(directory)
        .hash(hasher);
    for relative in PROFILE_FILES.iter().chain(MANIFEST_FILES.iter()).copied() {
        relative.hash(hasher);
        let path = directory.join(relative);
        if let Ok(metadata) = fs::metadata(path) {
            metadata.len().hash(hasher);
            metadata.is_file().hash(hasher);
            if let Ok(modified) = metadata.modified() {
                if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                    duration.as_nanos().hash(hasher);
                }
            }
        }
    }
}
