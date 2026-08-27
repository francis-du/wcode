use super::*;

impl ToolHarness {
    pub async fn worktree_status_snapshot(&self, workspace: &Workspace) -> Result<Value> {
        if !workspace.exec_enabled() || !workspace.root().join(".git").is_dir() {
            return Ok(json!({"available": false}));
        }
        let args = ["status", "--short", "--untracked-files=all"]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let result = workspace.run_command("git", &args, ".", 10).await?;
        if !result.success {
            return Ok(json!({"available": false, "reason": "git_status_failed"}));
        }
        let (changed, parsed_truncated) = parse_git_status(&result.stdout);
        let files = changed
            .into_iter()
            .map(|(path, file)| {
                json!({
                    "path": path,
                    "status": file.status,
                    "staged": file.staged,
                    "unstaged": file.unstaged,
                    "untracked": file.untracked,
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({
            "available": true,
            "files": files,
            "truncated": parsed_truncated || result.truncated,
        }))
    }
}

pub(super) fn review_probe_specs() -> [ReviewProbeSpec; 5] {
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

pub(super) async fn run_review_probe(
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

pub(super) fn review_probe_summary(output: &ReviewProbeOutput) -> ReviewProbeSummary {
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

pub(super) fn probe_failure_text(result: &CommandResult) -> Option<String> {
    let line = result
        .stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .or_else(|| result.stdout.lines().find(|line| !line.trim().is_empty()))?;
    Some(truncate_chars(line.trim(), 300).0)
}

pub(super) fn parse_git_status(output: &str) -> (BTreeMap<String, ChangedFileBuilder>, bool) {
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

pub(super) fn merge_numstat(
    files: &mut BTreeMap<String, ChangedFileBuilder>,
    output: &str,
) -> bool {
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

pub(super) fn normalize_numstat_path(raw: &str) -> String {
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

pub(super) fn append_diff_check_findings(
    findings: &mut Vec<ReviewFinding>,
    output: &ReviewProbeOutput,
) {
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

const MAINTAINABILITY_FILE_LINE_THRESHOLD: u64 = 1_000;
const MAINTAINABILITY_NET_GROWTH_THRESHOLD: u64 = 400;
const MAINTAINABILITY_CROSS_SCOPE_CHURN_THRESHOLD: u64 = 1_000;

pub(super) fn append_maintainability_findings(
    workspace: &Workspace,
    files: &[ChangedFileReview],
    findings: &mut Vec<ReviewFinding>,
) {
    let mut changed_scopes = BTreeSet::new();
    let mut source_churn = 0u64;

    for file in files
        .iter()
        .filter(|file| file.category == "source" && !file.binary && file.status != "deleted")
    {
        let additions = file.additions.unwrap_or(0);
        let deletions = file.deletions.unwrap_or(0);
        source_churn = source_churn.saturating_add(additions.saturating_add(deletions));
        if let Some(scope) = scopes::source_scope(&file.path) {
            changed_scopes.insert(scope);
        }

        let Ok(view) = workspace.read_file(&file.path, 1, Some(1)) else {
            continue;
        };
        let current_lines = view.total_lines as u64;
        let previous_lines = if matches!(file.status.as_str(), "added" | "untracked") {
            0
        } else {
            current_lines
                .saturating_sub(additions)
                .saturating_add(deletions)
        };
        if previous_lines < MAINTAINABILITY_FILE_LINE_THRESHOLD
            && current_lines > MAINTAINABILITY_FILE_LINE_THRESHOLD
        {
            findings.push(ReviewFinding {
                severity: "high".to_owned(),
                code: "maintainability-file-crossed-1k".to_owned(),
                message: format!(
                    "{} grew from approximately {previous_lines} to {current_lines} lines and crossed the 1k review boundary; justify the structure or decompose before adding more behavior.",
                    file.path
                ),
                paths: vec![file.path.clone()],
            });
        }

        let net_growth = additions.saturating_sub(deletions);
        if net_growth >= MAINTAINABILITY_NET_GROWTH_THRESHOLD {
            findings.push(ReviewFinding {
                severity: "warning".to_owned(),
                code: "maintainability-concentrated-growth".to_owned(),
                message: format!(
                    "{} adds approximately {net_growth} net lines in one source file; review whether a code-judo simplification or clearer module boundary can delete complexity instead of concentrating it.",
                    file.path
                ),
                paths: vec![file.path.clone()],
            });
        }
    }

    if changed_scopes.len() >= 3 && source_churn >= MAINTAINABILITY_CROSS_SCOPE_CHURN_THRESHOLD {
        findings.push(ReviewFinding {
            severity: "warning".to_owned(),
            code: "maintainability-cross-scope-churn".to_owned(),
            message: format!(
                "The source change spans {} Product Scopes and approximately {source_churn} changed lines; verify canonical ownership, boundary direction, and whether independent concerns should be split.",
                changed_scopes.len()
            ),
            paths: files
                .iter()
                .filter(|file| file.category == "source")
                .take(12)
                .map(|file| file.path.clone())
                .collect(),
        });
    }
}

pub(super) fn file_category(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    if lower.starts_with("tests/")
        || lower.contains("/tests/")
        || lower.contains("/test/")
        || name.contains("_test.")
        || name.contains("_tests.")
        || name.contains(".test.")
        || name.contains(".spec.")
        || name.contains("_spec.")
        || name == "tests.rs"
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

pub(super) fn security_sensitive_path(path: &str) -> bool {
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
