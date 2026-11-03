//! Bounded read-only source retrieval for one exact recorded revision.
//!
//! The caller supplies a complete commit object ID and one repository-relative
//! path. Revision expressions, pathspecs, shell commands and worktree writes are
//! never accepted; the read travels through the Workspace's policy-checked Git
//! lane and reuses the same protected-path, symlink and redaction boundaries as
//! the change inspector.
use super::*;

pub(crate) const MAX_HISTORICAL_REVISIONS: usize = 32;
pub(crate) const HISTORICAL_REVISION_UNKNOWN: &str = "historical source revision is unknown";
pub(crate) const HISTORICAL_SOURCE_MISSING: &str =
    "historical source path is missing at this revision";

const HISTORICAL_PATH_LIMIT: usize = 1024;
const HISTORICAL_WINDOW: usize = 240;
const HISTORICAL_PROBE_TIMEOUT_SECONDS: u64 = 10;

#[derive(Clone, Debug, Serialize)]
pub struct HistoricalRevision {
    pub revision: String,
    pub short_revision: String,
    pub committed_at: String,
    pub subject: String,
}

#[derive(Debug, Serialize)]
pub struct HistoricalSource {
    pub path: String,
    pub revision: String,
    /// `source`, `binary`, `redacted`, `directory` or `truncated`.
    pub kind: &'static str,
    pub content: String,
    /// SHA-256 of the exact bytes read for this revision. Empty unless `kind` is
    /// `source`, because only then is the content known to be complete.
    pub sha256: String,
    pub start_line: usize,
    pub end_line: usize,
    pub total_lines: usize,
    pub redacted: bool,
    pub truncated: bool,
    pub line_ending: &'static str,
}

impl HistoricalSource {
    fn unavailable(path: &str, revision: &str, kind: &'static str) -> Self {
        Self {
            path: path.to_owned(),
            revision: revision.to_owned(),
            kind,
            content: String::new(),
            sha256: String::new(),
            start_line: 1,
            end_line: 0,
            total_lines: 0,
            redacted: kind == "redacted",
            truncated: kind == "truncated",
            line_ending: "none",
        }
    }
}

impl Workspace {
    async fn historical_probe(&self, args: &[String]) -> Result<CommandResult> {
        let result = self
            .run_command("git", args, ".", HISTORICAL_PROBE_TIMEOUT_SECONDS)
            .await?;
        if result.timed_out || result.output_incomplete {
            bail!("historical source Git probe is incomplete");
        }
        Ok(result)
    }

    /// Resolve an exact commit object ID without accepting a revision expression.
    async fn historical_revision(&self, revision: &str) -> Result<String> {
        let peeled = format!("{revision}^{{commit}}");
        let args = vec![
            "rev-parse".to_owned(),
            "--verify".to_owned(),
            "--end-of-options".to_owned(),
            peeled,
        ];
        let output = self.historical_probe(&args).await?;
        let resolved = output.stdout.trim();
        if !output.success || !crate::verification::change::full_oid(resolved) {
            bail!(HISTORICAL_REVISION_UNKNOWN);
        }
        Ok(resolved.to_owned())
    }

    /// Read one file as of one exact commit. `start_line`/`end_line` bound the
    /// returned window; the reported SHA always covers the exact bytes read.
    pub async fn historical_source(
        &self,
        revision: &str,
        path: &str,
        start_line: usize,
        end_line: Option<usize>,
    ) -> Result<HistoricalSource> {
        if !self.exec_enabled() || !self.root().join(".git").exists() {
            bail!("historical source requires execution and a Git repository root");
        }
        if !crate::verification::change::full_oid(revision) {
            bail!("historical source requires a complete commit object ID");
        }
        if path.len() > HISTORICAL_PATH_LIMIT || path.chars().any(char::is_control) {
            bail!("invalid historical source path");
        }
        let relative = Self::validate_relative(path)?;
        if relative.as_os_str().is_empty() {
            bail!("one explicit file is required");
        }
        // A historical path may be absent from the worktree, so only existing
        // components can be symlink-checked. Git resolves the path inside the
        // object database, never through the worktree.
        self.ensure_no_symlink_components_allow_missing(&relative)?;
        let path = portable_relative_path(&relative);
        if start_line == 0 {
            bail!("historical source start line must be at least one");
        }
        let start = start_line;
        let requested_end = end_line.unwrap_or(start.saturating_add(HISTORICAL_WINDOW - 1));
        let end = requested_end.clamp(start, start.saturating_add(HISTORICAL_WINDOW - 1));
        let resolved = self.historical_revision(revision).await?;
        let object = format!("{resolved}:{path}");
        let output = self
            .historical_probe(&[
                "show".to_owned(),
                "--no-ext-diff".to_owned(),
                "--no-textconv".to_owned(),
                object,
            ])
            .await?;
        if !output.success {
            if output.stderr.contains("does not exist") {
                bail!(HISTORICAL_SOURCE_MISSING);
            }
            bail!("historical source is unavailable at this revision");
        }
        let Some(raw_stdout) = output.raw_stdout else {
            return Ok(HistoricalSource::unavailable(&path, &resolved, "redacted"));
        };
        if output.truncated {
            return Ok(HistoricalSource::unavailable(&path, &resolved, "truncated"));
        }
        if raw_stdout.starts_with("tree ") {
            return Ok(HistoricalSource::unavailable(&path, &resolved, "directory"));
        }
        if raw_stdout.contains('\0') || raw_stdout.contains('\u{FFFD}') {
            return Ok(HistoricalSource::unavailable(&path, &resolved, "binary"));
        }
        let sha256 = sha256(raw_stdout.as_bytes());
        let newline_count = raw_stdout.bytes().filter(|byte| *byte == b'\n').count();
        let crlf_count = raw_stdout.matches("\r\n").count();
        let carriage_count = raw_stdout.bytes().filter(|byte| *byte == b'\r').count();
        let line_ending = match (newline_count, crlf_count) {
            _ if carriage_count != crlf_count => "mixed",
            (0, _) => "none",
            (lf, crlf) if lf == crlf => "crlf",
            (_, 0) => "lf",
            _ => "mixed",
        };
        let mut total = 0usize;
        let mut selected = String::new();
        for (index, line) in raw_stdout.split_inclusive('\n').enumerate() {
            let line_number = index + 1;
            total = line_number;
            if line_number < start || line_number > end {
                continue;
            }
            selected.push_str(line);
        }
        if selected.ends_with("\r\n") {
            selected.truncate(selected.len() - 2);
        } else if selected.ends_with('\n') {
            selected.pop();
        }
        let end = end.min(total).max(start.saturating_sub(1));
        let (safe, redacted) = redact_sensitive_text(&selected);
        let selected = if redacted { safe } else { selected };
        Ok(HistoricalSource {
            path,
            revision: resolved,
            kind: "source",
            content: selected,
            sha256,
            start_line: start,
            end_line: end,
            total_lines: total,
            redacted,
            truncated: false,
            line_ending,
        })
    }

    /// List the most recent commits reachable from HEAD. The selector is bounded
    /// to what this probe reports; it never accepts a revision expression.
    pub async fn historical_revisions(
        &self,
        limit: usize,
    ) -> Result<(Vec<HistoricalRevision>, bool)> {
        if !self.exec_enabled() || !self.root().join(".git").exists() {
            bail!("historical source requires execution and a Git repository root");
        }
        let limit = limit.clamp(1, MAX_HISTORICAL_REVISIONS);
        let args = vec![
            "log".to_owned(),
            "--no-color".to_owned(),
            "--format=%H%x1f%h%x1f%ad%x1f%s".to_owned(),
            "--date=short".to_owned(),
            "-n".to_owned(),
            limit.to_string(),
        ];
        let output = self.historical_probe(&args).await?;
        if !output.success || output.redacted || output.truncated {
            bail!("historical revision list is unavailable");
        }
        Ok(parse_historical_revisions(&output.stdout, limit))
    }
}

/// Classify a boundary failure so the HTTP layer can return an explicit state
/// instead of collapsing every refusal into one generic error.
pub(crate) fn historical_source_error_code(message: &str) -> Option<&'static str> {
    if message.contains("complete commit object ID") {
        Some("invalid_revision")
    } else if message.contains(HISTORICAL_REVISION_UNKNOWN) {
        Some("unknown_revision")
    } else if message.contains(HISTORICAL_SOURCE_MISSING) {
        Some("source_missing")
    } else if message.contains("invalid historical source path")
        || message.contains("one explicit file is required")
        || message.contains("start line must be at least one")
    {
        Some("invalid_path")
    } else if message.contains("protected")
        || message.contains("internal paths")
        || message.contains("environment secret")
        || message.contains("symlink")
        || message.contains("traversal")
        || message.contains("absolute paths")
        || message.contains("escape")
    {
        Some("path_restricted")
    } else {
        None
    }
}

fn parse_historical_revisions(stdout: &str, limit: usize) -> (Vec<HistoricalRevision>, bool) {
    let mut revisions = Vec::new();
    let mut incomplete = false;
    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }
        let fields = line.split('\x1f').collect::<Vec<_>>();
        let valid = fields.len() == 4
            && crate::verification::change::full_oid(fields[0])
            && !fields[1].is_empty()
            && fields[1].len() <= 12
            && !fields[2].is_empty()
            && fields[2].len() <= 16
            && !fields[3].is_empty()
            && fields[3].len() <= 240
            && !fields[3].chars().any(char::is_control);
        if !valid {
            incomplete = true;
            continue;
        }
        revisions.push(HistoricalRevision {
            revision: fields[0].to_owned(),
            short_revision: fields[1].to_owned(),
            committed_at: fields[2].to_owned(),
            subject: fields[3].to_owned(),
        });
        if revisions.len() >= limit {
            break;
        }
    }
    (revisions, incomplete)
}

#[cfg(test)]
#[path = "../../tests/unit/workspace/historical_source.rs"]
mod tests;
