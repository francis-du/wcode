use crate::code_index::CodeIndex;
use crate::monitor::TaskMonitor;
use crate::workspace::{CommandResult, Workspace};
use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinSet;

const MAX_PARALLEL_TOOLS: usize = 256;
const MAX_GUIDANCE_LINES_PER_FILE: usize = 160;
const MAX_GUIDANCE_CHARS_PER_FILE: usize = 12_000;
const MAX_GUIDANCE_CHARS_TOTAL: usize = 32_000;
const MAX_PROFILE_SOURCE_BYTES: u64 = 1024 * 1024;
const MAX_CHECK_OUTPUT_CHARS: usize = 12_000;
const MAX_VERIFICATION_CHECKS: usize = 8;
const MAX_REVIEW_FILES: usize = 500;
const MAX_REVIEW_FINDINGS: usize = 64;
const QUALITY_HARNESS_TOOLS: &[&str] = &["project_context", "review_changes", "verify_project"];

const GUIDANCE_FILES: &[&str] = &[
    "AGENTS.md",
    ".github/copilot-instructions.md",
    "CLAUDE.md",
    "CONTRIBUTING.md",
    "DEVELOPMENT.md",
    "README.md",
];

const MANIFEST_FILES: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "requirements.txt",
    "go.mod",
    "Makefile",
];

const PROFILE_FILES: &[&str] = &[
    "AGENTS.md",
    ".github/copilot-instructions.md",
    "CLAUDE.md",
    "CONTRIBUTING.md",
    "DEVELOPMENT.md",
    "README.md",
    "Cargo.toml",
    "Cargo.lock",
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lock",
    "bun.lockb",
    "pyproject.toml",
    "requirements.txt",
    "pytest.ini",
    "go.mod",
    "go.sum",
    "Makefile",
];

#[derive(Clone)]
pub struct ToolHarness {
    slots: Arc<Semaphore>,
    max_parallel: usize,
    project_cache: Arc<Mutex<HashMap<PathBuf, CachedProjectProfile>>>,
    code_index: CodeIndex,
}

#[derive(Clone)]
struct CachedProjectProfile {
    fingerprint: u64,
    profile: Arc<ProjectProfile>,
}

#[derive(Clone, Debug, Serialize)]
struct ProjectProfile {
    root: String,
    project_types: Vec<String>,
    manifests: Vec<String>,
    guidance: Vec<GuidanceDocument>,
    recommended_checks: Vec<CheckSpec>,
    workflow: Vec<String>,
    write_enabled: bool,
    exec_enabled: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectContext {
    pub workspace: String,
    pub cache_hit: bool,
    pub root: String,
    pub project_types: Vec<String>,
    pub manifests: Vec<String>,
    pub guidance: Vec<GuidanceDocument>,
    pub recommended_checks: Vec<CheckSpec>,
    pub workflow: Vec<String>,
    pub write_enabled: bool,
    pub exec_enabled: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct GuidanceDocument {
    pub path: String,
    pub excerpt: String,
    pub included_lines: usize,
    pub total_lines: usize,
    pub truncated: bool,
    pub redacted: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct CheckSpec {
    pub id: String,
    pub level: String,
    pub phase: u8,
    pub program: String,
    pub args: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct VerificationReport {
    pub workspace: String,
    pub level: String,
    pub execution: String,
    pub phases_run: usize,
    pub passed: bool,
    pub checks_run: usize,
    pub checks_failed: usize,
    pub elapsed_ms: u128,
    pub summary: String,
    pub checks: Vec<VerificationCheck>,
}

#[derive(Debug, Serialize)]
pub struct VerificationCheck {
    pub id: String,
    pub phase: u8,
    pub command: String,
    pub reason: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub elapsed_ms: u128,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub output_truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct ChangeReviewReport {
    pub workspace: String,
    pub execution: String,
    pub clean: bool,
    pub files_changed: usize,
    pub staged_files: usize,
    pub unstaged_files: usize,
    pub untracked_files: usize,
    pub additions: u64,
    pub deletions: u64,
    pub binary_files: usize,
    pub source_changed: bool,
    pub tests_changed: bool,
    pub docs_only: bool,
    pub risk_level: String,
    pub recommended_verification: String,
    pub recommended_checks: Vec<String>,
    pub summary: String,
    pub files: Vec<ChangedFileReview>,
    pub findings: Vec<ReviewFinding>,
    pub probes: Vec<ReviewProbeSummary>,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct ChangedFileReview {
    pub path: String,
    pub status: String,
    pub staged: bool,
    pub unstaged: bool,
    pub untracked: bool,
    pub category: String,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub binary: bool,
    pub risk_reasons: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ReviewFinding {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub paths: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ReviewProbeSummary {
    pub id: String,
    pub success: bool,
    pub elapsed_ms: u128,
    pub error: Option<String>,
}

#[derive(Clone)]
struct ReviewProbeSpec {
    id: &'static str,
    args: Vec<String>,
}

struct ReviewProbeOutput {
    id: String,
    result: Option<CommandResult>,
    elapsed_ms: u128,
    error: Option<String>,
}

#[derive(Default)]
struct ChangedFileBuilder {
    status: String,
    staged: bool,
    unstaged: bool,
    untracked: bool,
    additions: u64,
    deletions: u64,
    has_numstat: bool,
    binary: bool,
}

impl ToolHarness {
    pub fn new(max_parallel: usize) -> Result<Self> {
        if !(1..=MAX_PARALLEL_TOOLS).contains(&max_parallel) {
            bail!("max parallel tools must be between 1 and {MAX_PARALLEL_TOOLS}");
        }
        Ok(Self {
            slots: Arc::new(Semaphore::new(max_parallel)),
            max_parallel,
            project_cache: Default::default(),
            code_index: CodeIndex::new()?,
        })
    }

    pub fn max_parallel(&self) -> usize {
        self.max_parallel
    }

    pub fn quality_tool_count(&self) -> usize {
        QUALITY_HARNESS_TOOLS.len()
    }

    pub fn capabilities(&self) -> Value {
        json!({
            "tools": QUALITY_HARNESS_TOOLS,
            "project_context": true,
            "context_cache": true,
            "review_changes": true,
            "parallel_change_review": true,
            "verify_project": true,
            "phased_parallel_verification": true,
            "verification_exec_without_risky_flag": true,
            "verification_levels": ["quick", "full"],
            "max_verification_checks": MAX_VERIFICATION_CHECKS,
            "max_review_files": MAX_REVIEW_FILES,
            "max_parallel_tools": self.max_parallel,
            "code_index": self.code_index.capabilities(),
        })
    }

    pub fn file_outline(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        path: &str,
        max_symbols: usize,
    ) -> Result<Value> {
        self.code_index
            .file_outline(workspace_id, workspace, path, max_symbols)
    }

    pub fn find_symbol(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        query: &str,
        path: &str,
        kind: Option<&str>,
        max_results: usize,
    ) -> Result<Value> {
        self.code_index
            .find_symbol(workspace_id, workspace, query, path, kind, max_results)
    }

    pub fn symbol_context(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        symbol_id: &str,
        max_body_lines: usize,
    ) -> Result<Value> {
        self.code_index
            .symbol_context(workspace_id, workspace, symbol_id, max_body_lines)
    }

    pub fn invalidate_code_file(&self, workspace: &Workspace, path: &str) {
        self.code_index.invalidate(workspace.root(), path);
    }

    pub async fn acquire(&self) -> Result<OwnedSemaphorePermit, String> {
        self.slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "tool harness is shutting down".to_owned())
    }

    pub fn project_context(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
    ) -> Result<ProjectContext> {
        let (profile, cache_hit) = self.load_project_profile(workspace)?;
        Ok(ProjectContext {
            workspace: workspace_id.into(),
            cache_hit,
            root: profile.root.clone(),
            project_types: profile.project_types.clone(),
            manifests: profile.manifests.clone(),
            guidance: profile.guidance.clone(),
            recommended_checks: profile.recommended_checks.clone(),
            workflow: profile.workflow.clone(),
            write_enabled: profile.write_enabled,
            exec_enabled: profile.exec_enabled,
        })
    }

    pub async fn review_changes(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        timeout_seconds: u64,
        monitor: &TaskMonitor,
    ) -> Result<ChangeReviewReport> {
        if !workspace.exec_enabled() {
            bail!("change review requires command execution; restart without --no-exec");
        }
        if !workspace.root().join(".git").exists() {
            bail!(
                "change review requires the configured workspace root to be a Git repository root"
            );
        }

        let workspace_id = workspace_id.into();
        let (profile, _) = self.load_project_profile(workspace)?;
        let mut tasks = JoinSet::new();
        for spec in review_probe_specs() {
            let harness = self.clone();
            let monitor = monitor.clone();
            let workspace = workspace.clone();
            let workspace_id = workspace_id.clone();
            tasks.spawn(async move {
                run_review_probe(
                    harness,
                    monitor,
                    workspace_id,
                    workspace,
                    spec,
                    timeout_seconds,
                )
                .await
            });
        }

        let mut outputs = Vec::new();
        while let Some(joined) = tasks.join_next().await {
            outputs.push(match joined {
                Ok(output) => output,
                Err(error) => ReviewProbeOutput {
                    id: "internal-join-error".to_owned(),
                    result: None,
                    elapsed_ms: 0,
                    error: Some(error.to_string()),
                },
            });
        }
        outputs.sort_by(|left, right| left.id.cmp(&right.id));

        let status = outputs
            .iter()
            .find(|output| output.id == "status")
            .ok_or_else(|| anyhow::anyhow!("change review did not receive Git status output"))?;
        let status_result = status.result.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "git status failed: {}",
                status
                    .error
                    .as_deref()
                    .unwrap_or("the probe returned no result")
            )
        })?;
        if !status_result.success {
            bail!(
                "git status failed: {}",
                probe_failure_text(status_result).unwrap_or_else(|| "unknown error".to_owned())
            );
        }

        let (mut changed, mut truncated) = parse_git_status(&status_result.stdout);
        for probe_id in ["unstaged-numstat", "staged-numstat"] {
            if let Some(result) = outputs
                .iter()
                .find(|output| output.id == probe_id)
                .and_then(|output| output.result.as_ref())
            {
                if result.success {
                    truncated |= merge_numstat(&mut changed, &result.stdout);
                }
            }
        }

        let mut findings = Vec::new();
        for probe_id in ["unstaged-check", "staged-check"] {
            if let Some(output) = outputs.iter().find(|output| output.id == probe_id) {
                append_diff_check_findings(&mut findings, output);
            }
        }
        for probe_id in ["unstaged-numstat", "staged-numstat"] {
            if let Some(output) = outputs.iter().find(|output| output.id == probe_id) {
                if output.result.as_ref().is_some_and(|result| !result.success)
                    || output.error.is_some()
                {
                    findings.push(ReviewFinding {
                        severity: "warning".to_owned(),
                        code: "incomplete-diff-metrics".to_owned(),
                        message: format!(
                            "The {probe_id} probe failed; line counts may be incomplete."
                        ),
                        paths: Vec::new(),
                    });
                }
            }
        }

        let mut files = Vec::with_capacity(changed.len());
        let mut security_paths = Vec::new();
        let mut manifest_paths = Vec::new();
        let mut deleted_test_paths = Vec::new();
        let mut source_changed = false;
        let mut tests_changed = false;
        let mut docs_only = !changed.is_empty();
        let mut additions = 0u64;
        let mut deletions = 0u64;
        let mut binary_files = 0usize;

        for (path, change) in changed {
            let category = file_category(&path).to_owned();
            source_changed |= category == "source";
            tests_changed |= category == "test";
            docs_only &= category == "docs";
            additions = additions.saturating_add(change.additions);
            deletions = deletions.saturating_add(change.deletions);
            binary_files += usize::from(change.binary);

            let mut risk_reasons = Vec::new();
            if security_sensitive_path(&path) {
                risk_reasons.push("security-sensitive path".to_owned());
                security_paths.push(path.clone());
            }
            if category == "manifest" {
                risk_reasons.push("dependency or build metadata".to_owned());
                manifest_paths.push(path.clone());
            }
            if category == "migration" {
                risk_reasons.push("data migration".to_owned());
            }
            if category == "workflow" {
                risk_reasons.push("automation or release workflow".to_owned());
            }
            if change.status == "deleted" {
                risk_reasons.push("deleted file".to_owned());
                if category == "test" {
                    deleted_test_paths.push(path.clone());
                }
            }

            files.push(ChangedFileReview {
                path,
                status: change.status,
                staged: change.staged,
                unstaged: change.unstaged,
                untracked: change.untracked,
                category,
                additions: change.has_numstat.then_some(change.additions),
                deletions: change.has_numstat.then_some(change.deletions),
                binary: change.binary,
                risk_reasons,
            });
        }

        let staged_files = files.iter().filter(|file| file.staged).count();
        let unstaged_files = files.iter().filter(|file| file.unstaged).count();
        let untracked_files = files.iter().filter(|file| file.untracked).count();
        let files_changed = files.len();
        let total_lines = additions.saturating_add(deletions);

        if source_changed && !tests_changed {
            findings.push(ReviewFinding {
                severity: "warning".to_owned(),
                code: "source-without-test-change".to_owned(),
                message: "Source files changed without a corresponding test-file change; confirm existing coverage or add a focused regression test."
                    .to_owned(),
                paths: files
                    .iter()
                    .filter(|file| file.category == "source")
                    .take(8)
                    .map(|file| file.path.clone())
                    .collect(),
            });
        }
        if !security_paths.is_empty() {
            findings.push(ReviewFinding {
                severity: "high".to_owned(),
                code: "security-sensitive-change".to_owned(),
                message: "Authentication, authorization, token, crypto, or security-related files changed; review trust boundaries and failure paths explicitly."
                    .to_owned(),
                paths: security_paths.clone(),
            });
        }
        if !manifest_paths.is_empty() {
            findings.push(ReviewFinding {
                severity: "warning".to_owned(),
                code: "manifest-change".to_owned(),
                message: "Dependency or build metadata changed; verify lockfiles and perform a full project check."
                    .to_owned(),
                paths: manifest_paths.clone(),
            });
        }
        if !deleted_test_paths.is_empty() {
            findings.push(ReviewFinding {
                severity: "high".to_owned(),
                code: "deleted-tests".to_owned(),
                message: "Test files were deleted; confirm coverage was intentionally relocated or removed."
                    .to_owned(),
                paths: deleted_test_paths,
            });
        }
        if files_changed > 25 || total_lines > 1_000 {
            findings.push(ReviewFinding {
                severity: "warning".to_owned(),
                code: "large-change-set".to_owned(),
                message: format!(
                    "The change set spans {files_changed} files and approximately {total_lines} changed lines; consider splitting independent concerns."
                ),
                paths: Vec::new(),
            });
        }
        if untracked_files > 0 {
            findings.push(ReviewFinding {
                severity: "info".to_owned(),
                code: "untracked-files".to_owned(),
                message: format!(
                    "{untracked_files} untracked file(s) are part of the working tree review."
                ),
                paths: files
                    .iter()
                    .filter(|file| file.untracked)
                    .take(12)
                    .map(|file| file.path.clone())
                    .collect(),
            });
        }
        if docs_only {
            findings.push(ReviewFinding {
                severity: "info".to_owned(),
                code: "docs-only".to_owned(),
                message: "Only documentation files changed; a quick verification gate is normally sufficient."
                    .to_owned(),
                paths: Vec::new(),
            });
        }
        if truncated {
            findings.push(ReviewFinding {
                severity: "warning".to_owned(),
                code: "review-truncated".to_owned(),
                message: format!(
                    "The review reached its {MAX_REVIEW_FILES}-file bound; inspect the remaining change set separately."
                ),
                paths: Vec::new(),
            });
        }
        findings.truncate(MAX_REVIEW_FINDINGS);

        let high_risk = !security_paths.is_empty()
            || files.iter().any(|file| file.category == "migration")
            || findings.iter().any(|finding| finding.severity == "high")
            || files_changed > 50
            || total_lines > 2_000;
        let moderate_risk = source_changed
            || tests_changed
            || !manifest_paths.is_empty()
            || files.iter().any(|file| file.category == "workflow")
            || files_changed > 10
            || findings
                .iter()
                .any(|finding| matches!(finding.severity.as_str(), "warning" | "error"));
        let risk_level = if high_risk {
            "high"
        } else if moderate_risk {
            "moderate"
        } else {
            "low"
        };
        let recommended_verification = if high_risk
            || tests_changed
            || !manifest_paths.is_empty()
            || total_lines > 500
            || files_changed > 10
        {
            "full"
        } else {
            "quick"
        };
        let recommended_checks = profile
            .recommended_checks
            .iter()
            .filter(|check| recommended_verification == "full" || check.level == "quick")
            .map(|check| check.id.clone())
            .collect::<Vec<_>>();
        let clean = files_changed == 0;
        let summary = if clean {
            "No staged, unstaged, or untracked files were detected.".to_owned()
        } else {
            format!(
                "Reviewed {files_changed} changed file(s): {staged_files} staged, {unstaged_files} unstaged, {untracked_files} untracked; risk {risk_level}, recommend {recommended_verification} verification."
            )
        };
        let probes = outputs.iter().map(review_probe_summary).collect::<Vec<_>>();

        Ok(ChangeReviewReport {
            workspace: workspace_id,
            execution: "parallel-git-probes".to_owned(),
            clean,
            files_changed,
            staged_files,
            unstaged_files,
            untracked_files,
            additions,
            deletions,
            binary_files,
            source_changed,
            tests_changed,
            docs_only,
            risk_level: risk_level.to_owned(),
            recommended_verification: recommended_verification.to_owned(),
            recommended_checks,
            summary,
            files,
            findings,
            probes,
            truncated,
        })
    }

    fn load_project_profile(&self, workspace: &Workspace) -> Result<(Arc<ProjectProfile>, bool)> {
        let root = workspace.root().to_path_buf();
        let fingerprint = project_fingerprint(&root);
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
        cache.insert(
            root,
            CachedProjectProfile {
                fingerprint,
                profile: built.clone(),
            },
        );
        Ok((built, false))
    }

    pub async fn verify_project(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        level: &str,
        timeout_seconds: u64,
        monitor: &TaskMonitor,
    ) -> Result<VerificationReport> {
        if !workspace.exec_enabled() {
            bail!("project verification requires command execution; restart without --no-exec");
        }
        if !matches!(level, "quick" | "full") {
            bail!("verification level must be quick or full");
        }

        let workspace_id = workspace_id.into();
        let (profile, _) = self.load_project_profile(workspace)?;
        let mut plan = profile
            .recommended_checks
            .iter()
            .filter(|check| level == "full" || check.level == "quick")
            .take(MAX_VERIFICATION_CHECKS)
            .cloned()
            .collect::<Vec<_>>();
        sort_checks(&mut plan);

        let phases_run = plan
            .iter()
            .map(|check| check.phase)
            .collect::<HashSet<_>>()
            .len();
        let started = Instant::now();
        let mut checks = Vec::with_capacity(plan.len());
        let mut start = 0usize;

        while start < plan.len() {
            let phase = plan[start].phase;
            let end = plan[start..]
                .iter()
                .position(|check| check.phase != phase)
                .map(|offset| start + offset)
                .unwrap_or(plan.len());
            let mut tasks = JoinSet::new();
            for check in plan[start..end].iter().cloned() {
                let harness = self.clone();
                let monitor = monitor.clone();
                let workspace = workspace.clone();
                let workspace_id = workspace_id.clone();
                tasks.spawn(async move {
                    run_verification_check(
                        harness,
                        monitor,
                        workspace_id,
                        workspace,
                        check,
                        timeout_seconds,
                    )
                    .await
                });
            }
            while let Some(joined) = tasks.join_next().await {
                checks.push(match joined {
                    Ok(check) => check,
                    Err(error) => VerificationCheck {
                        id: "internal-join-error".to_owned(),
                        phase,
                        command: "verification task".to_owned(),
                        reason: "A verification worker failed before returning its result."
                            .to_owned(),
                        success: false,
                        exit_code: None,
                        elapsed_ms: 0,
                        stdout_tail: String::new(),
                        stderr_tail: error.to_string(),
                        output_truncated: false,
                    },
                });
            }
            start = end;
        }

        checks.sort_by(|left, right| {
            left.phase
                .cmp(&right.phase)
                .then_with(|| left.id.cmp(&right.id))
        });
        let checks_failed = checks.iter().filter(|check| !check.success).count();
        let checks_run = checks.len();
        let passed = checks_run > 0 && checks_failed == 0;
        let summary = if checks_run == 0 {
            "No verification commands could be inferred for this project; inspect its guidance and manifests manually."
                .to_owned()
        } else if passed {
            format!(
                "All {checks_run} inferred {level} checks passed across {phases_run} execution phase(s)."
            )
        } else {
            format!(
                "{checks_failed} of {checks_run} inferred {level} checks failed across {phases_run} execution phase(s)."
            )
        };

        Ok(VerificationReport {
            workspace: workspace_id,
            level: level.to_owned(),
            execution: "phased-parallel".to_owned(),
            phases_run,
            passed,
            checks_run,
            checks_failed,
            elapsed_ms: started.elapsed().as_millis(),
            summary,
            checks,
        })
    }
}

fn review_probe_specs() -> [ReviewProbeSpec; 5] {
    [
        ReviewProbeSpec {
            id: "status",
            args: ["status", "--short", "--untracked-files=all"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        },
        ReviewProbeSpec {
            id: "unstaged-check",
            args: ["diff", "--check"].into_iter().map(str::to_owned).collect(),
        },
        ReviewProbeSpec {
            id: "staged-check",
            args: ["diff", "--cached", "--check"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        },
        ReviewProbeSpec {
            id: "unstaged-numstat",
            args: ["diff", "--numstat"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        },
        ReviewProbeSpec {
            id: "staged-numstat",
            args: ["diff", "--cached", "--numstat"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        },
    ]
}

async fn run_review_probe(
    harness: ToolHarness,
    monitor: TaskMonitor,
    workspace_id: String,
    workspace: Workspace,
    spec: ReviewProbeSpec,
    timeout_seconds: u64,
) -> ReviewProbeOutput {
    let command = command_text("git", &spec.args);
    let mut task = monitor.queue(
        workspace_id,
        format!("review:{}", spec.id),
        command.clone(),
        command.len() as u64,
    );
    let _permit = match harness.acquire().await {
        Ok(permit) => permit,
        Err(error) => {
            task.finish(false, error.len() as u64);
            return ReviewProbeOutput {
                id: spec.id.to_owned(),
                result: None,
                elapsed_ms: 0,
                error: Some(error),
            };
        }
    };
    task.start();
    let started = Instant::now();

    match workspace
        .run_command("git", &spec.args, ".", timeout_seconds.clamp(1, 120))
        .await
    {
        Ok(result) => {
            let success = result.success;
            let response_bytes = result.stdout.len().saturating_add(result.stderr.len()) as u64;
            task.finish(success, response_bytes);
            ReviewProbeOutput {
                id: spec.id.to_owned(),
                result: Some(result),
                elapsed_ms: started.elapsed().as_millis(),
                error: None,
            }
        }
        Err(error) => {
            let message = error.to_string();
            task.finish(false, message.len() as u64);
            ReviewProbeOutput {
                id: spec.id.to_owned(),
                result: None,
                elapsed_ms: started.elapsed().as_millis(),
                error: Some(message),
            }
        }
    }
}

fn review_probe_summary(output: &ReviewProbeOutput) -> ReviewProbeSummary {
    let success =
        output.result.as_ref().is_some_and(|result| result.success) && output.error.is_none();
    let error = output.error.clone().or_else(|| {
        output
            .result
            .as_ref()
            .filter(|result| !result.success)
            .and_then(probe_failure_text)
    });
    ReviewProbeSummary {
        id: output.id.clone(),
        success,
        elapsed_ms: output.elapsed_ms,
        error,
    }
}

fn probe_failure_text(result: &CommandResult) -> Option<String> {
    let line = result
        .stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .or_else(|| result.stdout.lines().find(|line| !line.trim().is_empty()))?;
    Some(truncate_chars(line.trim(), 300).0)
}

fn parse_git_status(output: &str) -> (BTreeMap<String, ChangedFileBuilder>, bool) {
    let mut files = BTreeMap::new();
    let mut truncated = false;
    for (index, line) in output.lines().enumerate() {
        if index >= MAX_REVIEW_FILES {
            truncated = true;
            break;
        }
        let bytes = line.as_bytes();
        if bytes.len() < 3 {
            continue;
        }
        let x = bytes[0] as char;
        let y = bytes[1] as char;
        let path = normalize_status_path(&line[3..]);
        if path.is_empty() {
            continue;
        }
        let untracked = x == '?' && y == '?';
        let entry = files
            .entry(path)
            .or_insert_with(ChangedFileBuilder::default);
        entry.status = git_status_name(x, y).to_owned();
        entry.untracked |= untracked;
        entry.staged |= !untracked && !matches!(x, ' ' | '?' | '!');
        entry.unstaged |= !untracked && !matches!(y, ' ' | '?' | '!');
    }
    (files, truncated)
}

fn normalize_status_path(raw: &str) -> String {
    let path = raw
        .trim()
        .rsplit_once(" -> ")
        .map(|(_, destination)| destination)
        .unwrap_or_else(|| raw.trim());
    path.trim_matches('"').to_owned()
}

fn git_status_name(x: char, y: char) -> &'static str {
    if x == '?' && y == '?' {
        "untracked"
    } else if x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D') {
        "unmerged"
    } else if x == 'D' || y == 'D' {
        "deleted"
    } else if x == 'R' || y == 'R' {
        "renamed"
    } else if x == 'C' || y == 'C' {
        "copied"
    } else if x == 'A' || y == 'A' {
        "added"
    } else if x == 'M' || y == 'M' || x == 'T' || y == 'T' {
        "modified"
    } else {
        "changed"
    }
}

fn merge_numstat(files: &mut BTreeMap<String, ChangedFileBuilder>, output: &str) -> bool {
    let mut truncated = false;
    for (index, line) in output.lines().enumerate() {
        if index >= MAX_REVIEW_FILES {
            truncated = true;
            break;
        }
        let mut fields = line.splitn(3, '\t');
        let Some(additions) = fields.next() else {
            continue;
        };
        let Some(deletions) = fields.next() else {
            continue;
        };
        let Some(raw_path) = fields.next() else {
            continue;
        };
        let path = normalize_numstat_path(raw_path);
        if path.is_empty() {
            continue;
        }
        let entry = files.entry(path).or_insert_with(|| ChangedFileBuilder {
            status: "modified".to_owned(),
            ..ChangedFileBuilder::default()
        });
        if additions == "-" || deletions == "-" {
            entry.binary = true;
            entry.has_numstat = true;
            continue;
        }
        if let (Ok(additions), Ok(deletions)) = (additions.parse::<u64>(), deletions.parse::<u64>())
        {
            entry.additions = entry.additions.saturating_add(additions);
            entry.deletions = entry.deletions.saturating_add(deletions);
            entry.has_numstat = true;
        }
    }
    truncated
}

fn normalize_numstat_path(raw: &str) -> String {
    let raw = raw.trim().trim_matches('"');
    if let (Some(open), Some(close)) = (raw.find('{'), raw.rfind('}')) {
        if open < close {
            let inside = &raw[open + 1..close];
            if let Some((_, destination)) = inside.rsplit_once(" => ") {
                return format!("{}{}{}", &raw[..open], destination, &raw[close + 1..]);
            }
        }
    }
    raw.rsplit_once(" => ")
        .map(|(_, destination)| destination.to_owned())
        .unwrap_or_else(|| raw.to_owned())
}

fn append_diff_check_findings(findings: &mut Vec<ReviewFinding>, output: &ReviewProbeOutput) {
    if findings.len() >= MAX_REVIEW_FINDINGS {
        return;
    }
    let Some(result) = output.result.as_ref() else {
        findings.push(ReviewFinding {
            severity: "warning".to_owned(),
            code: "diff-check-unavailable".to_owned(),
            message: format!(
                "The {} probe could not run: {}",
                output.id,
                output.error.as_deref().unwrap_or("unknown error")
            ),
            paths: Vec::new(),
        });
        return;
    };

    let mut matched = 0usize;
    for line in result.stdout.lines().chain(result.stderr.lines()) {
        let lower = line.to_ascii_lowercase();
        if !(lower.contains("trailing whitespace")
            || lower.contains("space before tab")
            || lower.contains("leftover conflict marker")
            || lower.contains("new blank line at eof"))
        {
            continue;
        }
        let mut fields = line.splitn(3, ':');
        let path = fields.next().unwrap_or_default().trim().trim_matches('"');
        let line_number = fields.next().unwrap_or_default().trim();
        let message = fields.next().unwrap_or(line).trim();
        findings.push(ReviewFinding {
            severity: "error".to_owned(),
            code: format!("{}-failure", output.id),
            message: if line_number.is_empty() {
                message.to_owned()
            } else {
                format!("Line {line_number}: {message}")
            },
            paths: (!path.is_empty())
                .then(|| path.to_owned())
                .into_iter()
                .collect(),
        });
        matched += 1;
        if findings.len() >= MAX_REVIEW_FINDINGS {
            break;
        }
    }

    if !result.success && matched == 0 && findings.len() < MAX_REVIEW_FINDINGS {
        findings.push(ReviewFinding {
            severity: "warning".to_owned(),
            code: "diff-check-failed".to_owned(),
            message: format!(
                "The {} probe failed: {}",
                output.id,
                probe_failure_text(result).unwrap_or_else(|| "unknown error".to_owned())
            ),
            paths: Vec::new(),
        });
    }
}

fn file_category(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    if lower.starts_with("tests/")
        || lower.contains("/tests/")
        || lower.contains("/test/")
        || name.contains("_test.")
        || name.contains(".test.")
        || name.contains(".spec.")
        || name.starts_with("test_")
    {
        "test"
    } else if lower.starts_with("docs/")
        || lower.contains("/docs/")
        || name.ends_with(".md")
        || matches!(name, "readme" | "license" | "changelog")
    {
        "docs"
    } else if lower.starts_with(".github/workflows/") {
        "workflow"
    } else if lower.contains("/migrations/") || lower.starts_with("migrations/") {
        "migration"
    } else if matches!(
        name,
        "cargo.toml"
            | "cargo.lock"
            | "package.json"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "bun.lock"
            | "bun.lockb"
            | "pyproject.toml"
            | "requirements.txt"
            | "go.mod"
            | "go.sum"
            | "makefile"
    ) {
        "manifest"
    } else if [
        ".rs", ".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs", ".py", ".pyi", ".go",
        ".java", ".kt", ".swift", ".c", ".h", ".cpp", ".hpp", ".cs", ".rb", ".php", ".scala",
        ".sh", ".bash", ".css", ".html", ".htm", ".xhtml", ".dart", ".ex", ".exs", ".lua", ".ml",
        ".mli", ".r",
    ]
    .iter()
    .any(|extension| name.ends_with(extension))
    {
        "source"
    } else if name.starts_with('.')
        || [".toml", ".yaml", ".yml", ".json", ".ini", ".cfg", ".conf"]
            .iter()
            .any(|extension| name.ends_with(extension))
    {
        "config"
    } else {
        "other"
    }
}

fn security_sensitive_path(path: &str) -> bool {
    path.to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|token| {
            matches!(
                token,
                "auth"
                    | "authentication"
                    | "authorization"
                    | "authn"
                    | "authz"
                    | "oauth"
                    | "token"
                    | "tokens"
                    | "session"
                    | "sessions"
                    | "permission"
                    | "permissions"
                    | "crypto"
                    | "security"
                    | "secret"
                    | "secrets"
                    | "credential"
                    | "credentials"
            )
        })
}

async fn run_verification_check(
    harness: ToolHarness,
    monitor: TaskMonitor,
    workspace_id: String,
    workspace: Workspace,
    check: CheckSpec,
    timeout_seconds: u64,
) -> VerificationCheck {
    let command = command_text(&check.program, &check.args);
    let request_bytes = command.len() as u64;
    let mut task = monitor.queue(
        workspace_id,
        format!("verify:{}", check.id),
        format!("phase {} · {command}", check.phase),
        request_bytes,
    );
    let _permit = match harness.acquire().await {
        Ok(permit) => permit,
        Err(error) => return verification_error(check, error, 0),
    };
    task.start();
    let started = Instant::now();

    match workspace
        .run_verification_command(
            &check.program,
            &check.args,
            ".",
            timeout_seconds.clamp(1, 300),
        )
        .await
    {
        Ok(result) => {
            let success = result.success;
            let response_bytes = result.stdout.len().saturating_add(result.stderr.len()) as u64;
            let report = verification_check(check, result, started.elapsed().as_millis());
            task.finish(success, response_bytes);
            report
        }
        Err(error) => {
            let message = error.to_string();
            let report = verification_error(check, message.clone(), started.elapsed().as_millis());
            task.finish(false, message.len() as u64);
            report
        }
    }
}

fn verification_error(check: CheckSpec, error: String, elapsed_ms: u128) -> VerificationCheck {
    VerificationCheck {
        id: check.id,
        phase: check.phase,
        command: command_text(&check.program, &check.args),
        reason: check.reason,
        success: false,
        exit_code: None,
        elapsed_ms,
        stdout_tail: String::new(),
        stderr_tail: error,
        output_truncated: false,
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
        push_cargo_check(
            &mut checks,
            "rust-test",
            "full",
            "test",
            locked,
            "Run the Rust test suite.",
        );
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
            "Read the returned repository guidance before substantial edits.".to_owned(),
            "Use search_many and read_files to collect relevant implementation and tests in few round trips."
                .to_owned(),
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

fn sort_checks(checks: &mut [CheckSpec]) {
    checks.sort_by(|left, right| {
        left.phase
            .cmp(&right.phase)
            .then_with(|| check_rank(&left.level).cmp(&check_rank(&right.level)))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn verification_phase(id: &str) -> u8 {
    if id.contains("build") {
        3
    } else if id == "rust-clippy" {
        2
    } else if id.contains("test") {
        1
    } else {
        0
    }
}

fn check_rank(level: &str) -> u8 {
    if level == "quick" {
        0
    } else {
        1
    }
}

fn verification_check(
    check: CheckSpec,
    result: CommandResult,
    elapsed_ms: u128,
) -> VerificationCheck {
    let (stdout_tail, stdout_cut) = tail_chars(&result.stdout, MAX_CHECK_OUTPUT_CHARS);
    let (stderr_tail, stderr_cut) = tail_chars(&result.stderr, MAX_CHECK_OUTPUT_CHARS);
    VerificationCheck {
        id: check.id,
        phase: check.phase,
        command: command_text(&result.program, &result.args),
        reason: check.reason,
        success: result.success,
        exit_code: result.exit_code,
        elapsed_ms,
        stdout_tail,
        stderr_tail,
        output_truncated: result.truncated || stdout_cut || stderr_cut,
    }
}

fn command_text(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

fn project_fingerprint(root: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    root.hash(&mut hasher);
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

fn truncate_chars(value: &str, max_chars: usize) -> (String, bool) {
    if value.chars().count() <= max_chars {
        return (value.to_owned(), false);
    }
    let mut output = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    output.push('…');
    (output, true)
}

fn tail_chars(value: &str, max_chars: usize) -> (String, bool) {
    let count = value.chars().count();
    if count <= max_chars {
        return (value.to_owned(), false);
    }
    let mut output = String::from("…");
    output.extend(value.chars().skip(count - max_chars.saturating_sub(1)));
    (output, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn enforces_parallel_limit() {
        let harness = ToolHarness::new(2).unwrap();
        let first = harness.acquire().await.unwrap();
        let second = harness.acquire().await.unwrap();
        assert_eq!(harness.slots.available_permits(), 0);
        drop(first);
        assert_eq!(harness.slots.available_permits(), 1);
        drop(second);
    }

    #[test]
    fn rejects_unbounded_parallelism() {
        assert!(ToolHarness::new(0).is_err());
        assert_eq!(
            ToolHarness::new(MAX_PARALLEL_TOOLS)
                .expect("documented maximum should be accepted")
                .max_parallel(),
            MAX_PARALLEL_TOOLS
        );
        assert!(ToolHarness::new(MAX_PARALLEL_TOOLS + 1).is_err());
    }

    #[test]
    fn project_context_detects_guidance_and_quality_checks() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(".git")).unwrap();
        fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(root.path().join("Cargo.lock"), "# lock\n").unwrap();
        fs::write(
            root.path().join("AGENTS.md"),
            "# Instructions\nKeep changes small and run tests.\n",
        )
        .unwrap();
        let workspace = Workspace::new(root.path(), true, true).unwrap();
        let harness = ToolHarness::new(4).unwrap();

        let first = harness.project_context("demo", &workspace).unwrap();
        assert!(!first.cache_hit);
        assert!(first.project_types.contains(&"rust".to_owned()));
        assert!(first
            .guidance
            .iter()
            .any(|document| document.path == "AGENTS.md"));
        assert!(first
            .recommended_checks
            .iter()
            .any(|check| check.id == "rust-check" && check.args.contains(&"--locked".to_owned())));
        assert!(first.recommended_checks.iter().any(|check| {
            check.id == "rust-release-build"
                && check.args
                    == [
                        "build".to_owned(),
                        "--release".to_owned(),
                        "--locked".to_owned(),
                    ]
                && check.phase == 3
        }));

        let second = harness.project_context("demo", &workspace).unwrap();
        assert!(second.cache_hit);
    }

    #[test]
    fn node_context_uses_repository_package_manager_and_scripts() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("package.json"),
            r#"{"scripts":{"lint":"eslint .","test":"vitest run","build":"vite build"}}"#,
        )
        .unwrap();
        fs::write(root.path().join("pnpm-lock.yaml"), "lockfileVersion: 9\n").unwrap();
        let workspace = Workspace::new(root.path(), false, true).unwrap();
        let harness = ToolHarness::new(2).unwrap();
        let context = harness.project_context("web", &workspace).unwrap();

        assert!(context.project_types.contains(&"node".to_owned()));
        assert!(context
            .recommended_checks
            .iter()
            .any(|check| check.program == "pnpm" && check.args == ["run", "lint"]));
    }

    #[test]
    fn change_review_parsers_classify_status_metrics_and_risk() {
        let (mut files, truncated) = parse_git_status(
            " M src/auth.rs\nA  Cargo.lock\n?? tests/auth_test.rs\n D README.md\n",
        );
        assert!(!truncated);
        assert_eq!(files["src/auth.rs"].status, "modified");
        assert!(files["src/auth.rs"].unstaged);
        assert!(files["Cargo.lock"].staged);
        assert!(files["tests/auth_test.rs"].untracked);
        assert_eq!(files["README.md"].status, "deleted");

        assert!(!merge_numstat(
            &mut files,
            "12\t3\tsrc/auth.rs\n2\t0\ttests/auth_test.rs\n-\t-\tassets/logo.png\n",
        ));
        assert_eq!(files["src/auth.rs"].additions, 12);
        assert_eq!(files["src/auth.rs"].deletions, 3);
        assert!(files["assets/logo.png"].binary);
        assert_eq!(file_category("src/auth.rs"), "source");
        assert_eq!(file_category("web/page.html"), "source");
        assert_eq!(file_category("web/styles.css"), "source");
        assert_eq!(file_category("lib/worker.ex"), "source");
        assert_eq!(file_category("tests/auth_test.rs"), "test");
        assert_eq!(file_category("README.md"), "docs");
        assert_eq!(file_category("Cargo.lock"), "manifest");
        assert!(security_sensitive_path("src/auth.rs"));
        assert!(!security_sensitive_path("src/author.rs"));
        assert_eq!(
            normalize_numstat_path("src/{old.rs => new.rs}"),
            "src/new.rs"
        );
        assert_eq!(verification_phase("rust-format"), 0);
        assert_eq!(verification_phase("rust-test"), 1);
        assert_eq!(verification_phase("rust-clippy"), 2);
        assert_eq!(verification_phase("rust-release-build"), 3);
    }

    #[tokio::test]
    async fn change_review_runs_all_probes_without_parent_slot_deadlock() {
        use std::process::Command;

        let root = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(root.path())
                .status()
                .expect("git must be available for repository review tests")
        };
        assert!(git(&["init", "-q"]).success());
        assert!(git(&["config", "user.email", "wcode@example.test"]).success());
        assert!(git(&["config", "user.name", "wcode test"]).success());
        fs::create_dir_all(root.path().join("src")).unwrap();
        fs::create_dir_all(root.path().join("tests")).unwrap();
        fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname = \"review-demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.path().join("src/auth.rs"),
            "pub fn enabled() -> bool { false }\n",
        )
        .unwrap();
        fs::write(
            root.path().join("tests/auth_test.rs"),
            "// existing coverage\n",
        )
        .unwrap();
        assert!(git(&["add", "."]).success());
        assert!(git(&["-c", "commit.gpgsign=false", "commit", "-qm", "initial"]).success());

        fs::write(
            root.path().join("src/auth.rs"),
            "pub fn enabled() -> bool { true }\n",
        )
        .unwrap();
        fs::create_dir_all(root.path().join("docs")).unwrap();
        fs::write(root.path().join("docs/note.md"), "review note\n").unwrap();

        let workspace = Workspace::new(root.path(), false, true).unwrap();
        let workspace_id = "review-demo".to_owned();
        let harness = ToolHarness::new(1).unwrap();
        let monitor = TaskMonitor::new([workspace_id.clone()]);
        let report = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            harness.review_changes(workspace_id, &workspace, 30, &monitor),
        )
        .await
        .expect("review probes must not deadlock with one semaphore slot")
        .unwrap();

        assert_eq!(report.execution, "parallel-git-probes");
        assert_eq!(report.probes.len(), 5);
        assert_eq!(report.files_changed, 2);
        assert!(report.source_changed);
        assert!(!report.tests_changed);
        assert_eq!(report.risk_level, "high");
        assert_eq!(report.recommended_verification, "full");
        assert!(report
            .findings
            .iter()
            .any(|finding| finding.code == "source-without-test-change"));
        assert!(report
            .findings
            .iter()
            .any(|finding| finding.code == "security-sensitive-change"));
    }

    #[test]
    fn output_tail_is_bounded_and_keeps_the_end() {
        let (tail, truncated) = tail_chars("abcdefgh", 5);
        assert!(truncated);
        assert_eq!(tail, "…efgh");
    }
}
