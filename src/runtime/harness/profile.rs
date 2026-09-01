use super::*;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};

impl ToolHarness {
    pub(super) fn load_project_profile(
        &self,
        workspace: &Workspace,
    ) -> Result<(Arc<ProjectProfile>, bool)> {
        let root = workspace.root().to_path_buf();
        let fingerprint = project_fingerprint(workspace);
        if let Some(profile) = self
            .project_cache
            .lock()
            .map_err(|_| anyhow::anyhow!("project context cache poisoned"))?
            .get(&root)
            .filter(|cached| cached.fingerprint == fingerprint)
            .map(|cached| cached.profile.clone())
        {
            return Ok((profile, true));
        }

        // Build outside the cache lock so context discovery for one workspace does not
        // block independent requests for other workspaces.
        let built = Arc::new(build_project_profile(workspace)?);
        let mut cache = self
            .project_cache
            .lock()
            .map_err(|_| anyhow::anyhow!("project context cache poisoned"))?;
        if let Some(profile) = cache
            .get(&root)
            .filter(|cached| cached.fingerprint == fingerprint)
            .map(|cached| cached.profile.clone())
        {
            return Ok((profile, true));
        }
        let limit = crate::resource::limits().project_cache_limit();
        if cache.len() >= limit {
            if let Some(oldest) = cache.keys().next().cloned() {
                cache.remove(&oldest);
            }
        }
        cache.insert(
            root,
            CachedProjectProfile {
                fingerprint,
                profile: built.clone(),
            },
        );
        Ok((built, false))
    }
}

fn build_project_profile(workspace: &Workspace) -> Result<ProjectProfile> {
    let root = workspace.root();
    let manifests = MANIFEST_FILES
        .iter()
        .filter(|path| root.join(path).is_file())
        .map(|path| (*path).to_owned())
        .collect::<Vec<_>>();
    let mut project_types = BTreeSet::new();
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

    if root.join("Cargo.toml").is_file() {
        project_types.insert("rust".to_owned());
        let locked = root.join("Cargo.lock").is_file();
        push_check(
            &mut checks,
            "rust-format",
            "quick",
            "cargo",
            &["fmt", "--check"],
            "Verify Rust formatting without modifying files.",
        );
        push_cargo_check(
            &mut checks,
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
            let mut nextest_args = vec!["nextest".to_owned(), "run".to_owned()];
            if locked {
                nextest_args.push("--locked".to_owned());
            }
            push_check_owned(
                &mut checks,
                "rust-nextest",
                "full",
                "cargo",
                nextest_args,
                "Run the Rust test suite with cargo-nextest's parallel test runner.",
            );
        } else {
            push_cargo_check(
                &mut checks,
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
        clippy_args.extend(["--".to_owned(), "-D".to_owned(), "warnings".to_owned()]);
        push_check_owned(
            &mut checks,
            "rust-clippy",
            "full",
            "cargo",
            clippy_args,
            "Run Clippy and treat warnings as quality-gate failures.",
        );
        let mut release_args = vec!["build".to_owned(), "--release".to_owned()];
        if locked {
            release_args.push("--locked".to_owned());
        }
        push_check_owned(
            &mut checks,
            "rust-release-build",
            "full",
            "cargo",
            release_args,
            "Build the optimized release binary with the locked dependency graph.",
        );
    }

    if root.join("package.json").is_file() {
        project_types.insert("node".to_owned());
        add_node_checks(root, &mut checks);
    }

    if root.join("pyproject.toml").is_file() || root.join("requirements.txt").is_file() {
        project_types.insert("python".to_owned());
        push_check(
            &mut checks,
            "python-tests",
            "full",
            "pytest",
            &["-q"],
            "Run the Python test suite with concise output.",
        );
    }

    if root.join("go.mod").is_file() {
        project_types.insert("go".to_owned());
        push_check(
            &mut checks,
            "go-tests",
            "full",
            "go",
            &["test", "./..."],
            "Compile and test all Go packages.",
        );
    }

    if root.join("Makefile").is_file() {
        project_types.insert("make".to_owned());
        add_make_checks(root, &mut checks);
    }

    if project_types.is_empty() {
        project_types.insert("generic".to_owned());
    }

    deduplicate_checks(&mut checks);
    let guidance = collect_guidance(workspace)?;
    Ok(ProjectProfile {
        root: root.display().to_string(),
        project_types: project_types.into_iter().collect(),
        manifests,
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
            "Use isolated workers or parallel_tools for independent research/review when the host supports it, but never treat worker consensus as deterministic proof.".to_owned(),
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
        reason: reason.to_owned(),
    });
}

fn deduplicate_checks(checks: &mut Vec<CheckSpec>) {
    let mut seen = HashSet::new();
    checks.retain(|check| seen.insert((check.program.clone(), check.args.clone())));
    sort_checks(checks);
}

fn project_fingerprint(workspace: &Workspace) -> u64 {
    let root = workspace.root();
    let mut hasher = DefaultHasher::new();
    root.hash(&mut hasher);
    workspace.write_enabled().hash(&mut hasher);
    workspace.exec_enabled().hash(&mut hasher);
    for relative in PROFILE_FILES {
        relative.hash(&mut hasher);
        let path = root.join(relative);
        if let Ok(metadata) = fs::metadata(path) {
            metadata.len().hash(&mut hasher);
            metadata.is_file().hash(&mut hasher);
            if let Ok(modified) = metadata.modified() {
                if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                    duration.as_nanos().hash(&mut hasher);
                }
            }
        }
    }
    hasher.finish()
}
