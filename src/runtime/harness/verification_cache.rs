use super::*;
use crate::evidence::Revision;
use std::time::{Duration, UNIX_EPOCH};

const STATIC_REUSE_MAX_AGE: Duration = Duration::from_secs(10 * 60);

pub(super) type VerificationCacheKey = (PathBuf, String, String, String);
pub(super) type VerificationCache = HashMap<VerificationCacheKey, CachedVerificationCheck>;

#[derive(Clone)]
pub(super) struct CachedVerificationCheck {
    signature: String,
    stored_at: Instant,
    pub(super) last_used: Instant,
    check: VerificationCheck,
}

#[derive(Clone)]
pub(super) struct VerificationReuseContext {
    revision: Revision,
    level: String,
    fail_fast: bool,
    timeout_seconds: u64,
    environment: String,
    exec_enabled: bool,
    risky_exec_enabled: bool,
    semantic_exec_enabled: bool,
}

impl VerificationReuseContext {
    pub(super) fn new(
        workspace: &Workspace,
        revision: &Revision,
        level: &str,
        fail_fast: bool,
        timeout_seconds: u64,
    ) -> Self {
        Self {
            revision: revision.clone(),
            level: level.to_owned(),
            fail_fast,
            timeout_seconds,
            environment: environment_fingerprint(),
            exec_enabled: workspace.exec_enabled(),
            risky_exec_enabled: workspace.risky_exec_enabled(),
            semantic_exec_enabled: workspace.semantic_exec_enabled(),
        }
    }

    fn complete_revision(&self) -> bool {
        !self.revision.code.ends_with(":partial")
            && !self
                .revision
                .design
                .as_deref()
                .is_some_and(|revision| revision.ends_with(":partial"))
    }
}

pub(super) fn reusable_static_check(check: &CheckSpec) -> bool {
    matches!(
        check.id.as_str(),
        "rust-format"
            | "rust-check"
            | "rust-clippy"
            | "rust-release-build"
            | "go-vet"
            | "java-maven-compile"
            | "java-gradle-classes"
            | "swift-build"
            | "dart-format"
            | "dart-analyze"
            | "elixir-format"
            | "elixir-compile"
            | "ocaml-build"
            | "php-phpstan"
            | "php-psalm"
            | "php-format"
            | "ruby-rubocop"
    )
}

impl ToolHarness {
    pub(super) fn cached_verification_check(
        &self,
        workspace: &Workspace,
        context: &VerificationReuseContext,
        check: &CheckSpec,
    ) -> Option<VerificationCheck> {
        if !context.complete_revision() || !reusable_static_check(check) {
            return None;
        }
        let key = cache_key(workspace, context, check);
        let signature = check_signature(workspace, context, check);
        let mut cache = self.verification_cache.lock().ok()?;
        let valid = cache.get(&key).is_some_and(|cached| {
            cached.signature == signature
                && cached.check.success
                && cached.stored_at.elapsed() <= STATIC_REUSE_MAX_AGE
        });
        if !valid {
            cache.remove(&key);
            return None;
        }
        let cached = cache.get_mut(&key)?;
        cached.last_used = Instant::now();
        let mut reused = cached.check.clone();
        reused.reused = true;
        reused.elapsed_ms = 0;
        reused.queue_wait_ms = 0;
        reused.execution_ms = 0;
        reused.reason = format!(
            "{} Reused a previously persisted exact-revision static pass; no command executed in this run.",
            reused.reason
        );
        Some(reused)
    }

    pub(super) fn cache_successful_verification_checks(
        &self,
        workspace: &Workspace,
        context: &VerificationReuseContext,
        plan: &[CheckSpec],
        report: &VerificationReport,
    ) {
        if !context.complete_revision()
            || !report.passed
            || report.checks_failed != 0
            || !report.skipped_checks.is_empty()
        {
            return;
        }
        let Ok(mut cache) = self.verification_cache.lock() else {
            return;
        };
        let limit = crate::resource::limits()
            .project_cache_limit()
            .saturating_mul(MAX_VERIFICATION_CHECKS)
            .max(MAX_VERIFICATION_CHECKS);
        for spec in plan.iter().filter(|check| reusable_static_check(check)) {
            let command = verification_command_text(spec);
            let Some(check) = report.checks.iter().find(|check| {
                !check.reused && check.success && check.id == spec.id && check.command == command
            }) else {
                continue;
            };
            let key = cache_key(workspace, context, spec);
            if cache.len() >= limit && !cache.contains_key(&key) {
                if let Some(oldest) = cache
                    .iter()
                    .min_by_key(|(_, cached)| cached.last_used)
                    .map(|(key, _)| key.clone())
                {
                    cache.remove(&oldest);
                }
            }
            let now = Instant::now();
            cache.insert(
                key,
                CachedVerificationCheck {
                    signature: check_signature(workspace, context, spec),
                    stored_at: now,
                    last_used: now,
                    check: check.clone(),
                },
            );
        }
    }
}

fn cache_key(
    workspace: &Workspace,
    context: &VerificationReuseContext,
    check: &CheckSpec,
) -> VerificationCacheKey {
    (
        workspace.root().to_path_buf(),
        context.level.clone(),
        check.cwd.clone(),
        check.id.clone(),
    )
}

fn check_signature(
    workspace: &Workspace,
    context: &VerificationReuseContext,
    check: &CheckSpec,
) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, "wcode-static-verification-reuse-v1");
    hash_field(&mut hasher, &context.revision.code);
    hash_field(
        &mut hasher,
        context.revision.design.as_deref().unwrap_or("none"),
    );
    hash_field(&mut hasher, &context.level);
    hash_field(
        &mut hasher,
        if context.fail_fast {
            "fail-fast"
        } else {
            "diagnostic"
        },
    );
    hash_field(&mut hasher, &context.timeout_seconds.to_string());
    hash_field(
        &mut hasher,
        if context.exec_enabled {
            "exec"
        } else {
            "no-exec"
        },
    );
    hash_field(
        &mut hasher,
        if context.risky_exec_enabled {
            "risky-exec"
        } else {
            "bounded-exec"
        },
    );
    hash_field(
        &mut hasher,
        if context.semantic_exec_enabled {
            "semantic-exec"
        } else {
            "no-semantic-exec"
        },
    );
    hash_field(&mut hasher, &context.environment);
    hash_field(&mut hasher, &check.id);
    hash_field(&mut hasher, &check.level);
    hash_field(&mut hasher, &check.phase.to_string());
    hash_field(&mut hasher, &check.program);
    for arg in &check.args {
        hash_field(&mut hasher, arg);
    }
    hash_field(&mut hasher, &check.cwd);
    hash_field(&mut hasher, &check.island);
    for language in &check.languages {
        hash_field(&mut hasher, language);
    }
    hash_field(&mut hasher, &program_identity(workspace, check));
    format!("sha256:{:x}", hasher.finalize())
}

fn program_identity(workspace: &Workspace, check: &CheckSpec) -> String {
    let program = Path::new(&check.program);
    let path = if program.is_absolute() || check.program.contains(['/', '\\']) {
        let cwd = if check.cwd == "." {
            workspace.root().to_path_buf()
        } else {
            workspace.root().join(&check.cwd)
        };
        Some(if program.is_absolute() {
            program.to_path_buf()
        } else {
            cwd.join(program)
        })
    } else {
        stage_executor::find_executable(&check.program)
    };
    let Some(path) = path else {
        return "missing".to_owned();
    };
    let canonical = path.canonicalize().unwrap_or(path);
    let Ok(metadata) = std::fs::metadata(&canonical) else {
        return format!("{}:unreadable", canonical.display());
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{}:{}:{modified}", canonical.display(), metadata.len())
}

fn environment_fingerprint() -> String {
    let mut environment = std::env::vars_os()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect::<Vec<_>>();
    environment.sort_unstable();
    let mut hasher = Sha256::new();
    for (key, value) in environment {
        hash_field(&mut hasher, &key);
        hash_field(&mut hasher, &value);
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn hash_field(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}
