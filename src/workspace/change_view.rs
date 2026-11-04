//! Read-only, single-path change inspection. This is an observed file snapshot,
//! not a transaction, semantic proof, or permission to apply a patch.
use super::*;
use std::collections::BTreeMap;

const MAX_CHANGE_VIEW_BYTES: usize = 64 * 1024;
const MAX_CHANGED_RANGES: usize = 256;
pub(crate) const MAX_CHANGED_RANGE_FILES: usize = 500;
const STALE_VIEW: &str = "change snapshot changed; reload the selected file";

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeLayer {
    #[default]
    Working,
    Staged,
    Unstaged,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ChangeLineRange {
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct ChangeSourceSnapshot {
    pub sha256: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct ChangeView {
    pub path: String,
    pub layer: ChangeLayer,
    pub snapshot_id: String,
    pub head: String,
    pub index_fingerprint: String,
    pub worktree_sha256: Option<String>,
    pub kind: &'static str,
    pub content: String,
    pub before_changed_ranges: Vec<ChangeLineRange>,
    pub after_changed_ranges: Vec<ChangeLineRange>,
    pub changed_ranges_truncated: bool,
    pub after_source_matches_worktree: bool,
    #[serde(skip_serializing)]
    pub(crate) before_source: Option<ChangeSourceSnapshot>,
    #[serde(skip_serializing)]
    pub(crate) after_source: Option<ChangeSourceSnapshot>,
    pub redacted: bool,
    pub truncated: bool,
    pub observed_at_ms: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct ChangeInputs {
    head: String,
    status: String,
    index: String,
    worktree_sha256: Option<String>,
}

impl ChangeInputs {
    fn fingerprint(&self, path: &str) -> String {
        let mut hash = Sha256::new();
        hash.update(b"wcode-change-view-v1\0");
        for part in [
            path,
            &self.head,
            &self.status,
            &self.index,
            self.worktree_sha256.as_deref().unwrap_or("missing"),
        ] {
            hash.update((part.len() as u64).to_le_bytes());
            hash.update(part.as_bytes());
        }
        format!("{:x}", hash.finalize())
    }
}

fn push_changed_line(ranges: &mut Vec<ChangeLineRange>, line: usize) -> bool {
    if line == 0 {
        return false;
    }
    if let Some(last) = ranges.last_mut() {
        if last.end_line.saturating_add(1) == line {
            last.end_line = line;
            return false;
        }
    }
    if ranges.len() >= MAX_CHANGED_RANGES {
        return true;
    }
    ranges.push(ChangeLineRange {
        start_line: line,
        end_line: line,
    });
    false
}

fn diff_coordinate(token: &str, prefix: char) -> Option<(usize, usize)> {
    let raw = token.strip_prefix(prefix)?;
    let (start, count) = raw.split_once(',').unwrap_or((raw, "1"));
    Some((start.parse().ok()?, count.parse().ok()?))
}

fn staged_after_matches_worktree(status: &str) -> bool {
    status
        .lines()
        .all(|line| line.as_bytes().get(1).copied() == Some(b' '))
}

fn index_blob_oid(index: &str) -> Result<Option<String>> {
    let mut lines = index.lines().filter(|line| !line.trim().is_empty());
    let Some(line) = lines.next() else {
        return Ok(None);
    };
    if lines.next().is_some() {
        bail!("unmerged or ambiguous index entries need separate inspection");
    }
    let fields = line.split_whitespace().take(3).collect::<Vec<_>>();
    if fields.len() != 3
        || !matches!(fields[0], "100644" | "100755")
        || fields[2] != "0"
        || !matches!(fields[1].len(), 40 | 64)
        || !fields[1].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("unmerged, symbolic-link and submodule entries need separate inspection");
    }
    Ok(Some(fields[1].to_owned()))
}

fn unified_changed_ranges(content: &str) -> (Vec<ChangeLineRange>, Vec<ChangeLineRange>, bool) {
    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut before_line = 0usize;
    let mut after_line = 0usize;
    let mut in_hunk = false;
    let mut capped = false;
    for line in content.lines() {
        if let Some(header) = line.strip_prefix("@@ ") {
            let mut fields = header.split_whitespace();
            let Some((before_start, _)) = fields.next().and_then(|v| diff_coordinate(v, '-'))
            else {
                in_hunk = false;
                continue;
            };
            let Some((after_start, _)) = fields.next().and_then(|v| diff_coordinate(v, '+')) else {
                in_hunk = false;
                continue;
            };
            before_line = before_start;
            after_line = after_start;
            in_hunk = true;
            continue;
        }
        if !in_hunk || line.starts_with("\\ No newline at end of file") {
            continue;
        }
        match line.as_bytes().first().copied() {
            Some(b' ') => {
                before_line = before_line.saturating_add(1);
                after_line = after_line.saturating_add(1);
            }
            Some(b'-') => {
                capped |= push_changed_line(&mut before, before_line);
                before_line = before_line.saturating_add(1);
            }
            Some(b'+') => {
                capped |= push_changed_line(&mut after, after_line);
                after_line = after_line.saturating_add(1);
            }
            _ => {}
        }
    }
    (before, after, capped)
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FileChangedRanges {
    pub before: Option<Vec<ChangeLineRange>>,
    pub after: Option<Vec<ChangeLineRange>>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ChangedRangesSnapshot {
    pub files: BTreeMap<String, FileChangedRanges>,
}

fn unquote_git_path(raw: &str) -> String {
    // Git appends a tab after paths that contain spaces in the ---/+++ header.
    let raw = raw.strip_suffix('\t').unwrap_or(raw);
    let Some(inner) = raw
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return raw.to_owned();
    };
    let mut bytes = Vec::new();
    let mut chars = inner.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            let mut buffer = [0u8; 4];
            bytes.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
            continue;
        }
        match chars.next() {
            Some('a') => bytes.push(0x07),
            Some('b') => bytes.push(0x08),
            Some('f') => bytes.push(0x0c),
            Some('n') => bytes.push(b'\n'),
            Some('r') => bytes.push(b'\r'),
            Some('t') => bytes.push(b'\t'),
            Some('v') => bytes.push(0x0b),
            Some('\\') => bytes.push(b'\\'),
            Some('"') => bytes.push(b'"'),
            Some(digit @ '0'..='7') => {
                let mut value = digit.to_digit(8).unwrap() as u8;
                for _ in 0..2 {
                    if let Some(next @ '0'..='7') = chars.peek().copied() {
                        value = value * 8 + next.to_digit(8).unwrap() as u8;
                        chars.next();
                    }
                }
                bytes.push(value);
            }
            Some(other) => {
                let mut buffer = [0u8; 4];
                bytes.extend_from_slice(other.encode_utf8(&mut buffer).as_bytes());
            }
            None => bytes.push(b'\\'),
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn diff_header_path(line: &str, prefix: &str) -> Option<String> {
    let path = line.strip_prefix(prefix)?;
    if path == "/dev/null" {
        return None;
    }
    Some(unquote_git_path(path))
}

fn parse_changed_ranges(diff: &str) -> ChangedRangesSnapshot {
    let mut snapshot = ChangedRangesSnapshot::default();
    let mut current: Option<String> = None;
    let mut before_line = 0usize;
    let mut after_line = 0usize;
    let mut in_hunk = false;
    let mut binary = false;
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            current = None;
            in_hunk = false;
            binary = false;
            continue;
        }
        // File headers and binary notices only appear before a file's first
        // hunk. Inside a hunk a content line may itself begin with `--- ` or
        // `+++ ` (a removed line starting with `-- `, or an added line starting
        // with `++ `), and must be read as content rather than as a header.
        if !in_hunk {
            if let Some(path) = line.strip_prefix("--- ") {
                if !binary && current.is_none() {
                    current = diff_header_path(path, "");
                    if let Some(path) = current.clone() {
                        snapshot
                            .files
                            .entry(path)
                            .or_insert_with(|| FileChangedRanges {
                                before: Some(Vec::new()),
                                after: Some(Vec::new()),
                                truncated: false,
                            });
                    }
                }
                continue;
            }
            if let Some(path) = line.strip_prefix("+++ ") {
                if !binary {
                    // A `/dev/null` "+++" side is a deletion; keep the path already
                    // resolved from the `---` header so its removed lines stay visible.
                    if let Some(resolved) = diff_header_path(path, "") {
                        current = Some(resolved.clone());
                        snapshot
                            .files
                            .entry(resolved)
                            .or_insert_with(|| FileChangedRanges {
                                before: Some(Vec::new()),
                                after: Some(Vec::new()),
                                truncated: false,
                            });
                    }
                }
                continue;
            }
            if line.starts_with("Binary files ") || line == "GIT binary patch" {
                binary = true;
                if let Some(path) = current.clone() {
                    let entry = snapshot.files.entry(path).or_default();
                    entry.before = None;
                    entry.after = None;
                }
                continue;
            }
        }
        if let Some(header) = line.strip_prefix("@@ ") {
            let mut fields = header.split_whitespace();
            let Some((before_start, _)) = fields.next().and_then(|v| diff_coordinate(v, '-'))
            else {
                in_hunk = false;
                continue;
            };
            let Some((after_start, _)) = fields.next().and_then(|v| diff_coordinate(v, '+')) else {
                in_hunk = false;
                continue;
            };
            before_line = before_start;
            after_line = after_start;
            in_hunk = true;
            continue;
        }
        if !in_hunk || line.starts_with("\\ No newline at end of file") {
            continue;
        }
        let Some(path) = current.clone() else {
            continue;
        };
        let Some(entry) = snapshot.files.get_mut(&path) else {
            continue;
        };
        match line.as_bytes().first().copied() {
            Some(b'-') => {
                if let Some(before) = entry.before.as_mut() {
                    entry.truncated |= push_changed_line(before, before_line);
                }
                before_line = before_line.saturating_add(1);
            }
            Some(b'+') => {
                if let Some(after) = entry.after.as_mut() {
                    entry.truncated |= push_changed_line(after, after_line);
                }
                after_line = after_line.saturating_add(1);
            }
            Some(b' ') => {
                before_line = before_line.saturating_add(1);
                after_line = after_line.saturating_add(1);
            }
            _ => {}
        }
    }
    snapshot
}

/// Parse a completed diff probe. A failed, redacted, timed-out, incompletely
/// captured or byte-truncated probe retains no ranges: a diff cut off at the
/// output cap would otherwise be read as a complete change set. Truncation
/// stays per file: only a file whose own ranges hit `MAX_CHANGED_RANGES` is
/// marked truncated.
fn changed_ranges_from_output(output: &CommandResult) -> Option<ChangedRangesSnapshot> {
    if !output.success
        || output.redacted
        || output.timed_out
        || output.output_incomplete
        || output.truncated
    {
        return None;
    }
    Some(parse_changed_ranges(&output.stdout))
}

impl Workspace {
    pub(crate) async fn change_probe(&self, args: &[&str]) -> Result<CommandResult> {
        let args = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        let result = self.run_command("git", &args, ".", 10).await?;
        if !result.success || result.timed_out || result.output_incomplete {
            bail!("change inspection Git probe is unavailable");
        }
        Ok(result)
    }

    async fn change_blob_source(&self, object_id: &str) -> Result<Option<ChangeSourceSnapshot>> {
        if !matches!(object_id.len(), 40 | 64)
            || !object_id.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            bail!("invalid change object identity");
        }
        let output = self
            .change_probe(&["show", "--no-ext-diff", "--no-textconv", object_id])
            .await?;
        // Redacted or truncated command output is not byte-identical source and
        // therefore cannot be fed into the syntax index as a captured snapshot.
        let Some(raw_stdout) = output.raw_stdout else {
            return Ok(None);
        };
        if output.redacted
            || output.truncated
            || raw_stdout.contains('\0')
            || raw_stdout.contains('\u{FFFD}')
        {
            return Ok(None);
        }
        Ok(Some(ChangeSourceSnapshot {
            sha256: sha256(raw_stdout.as_bytes()),
            content: raw_stdout,
        }))
    }

    async fn change_tree_source(
        &self,
        tree: &str,
        path: &str,
    ) -> Result<Option<ChangeSourceSnapshot>> {
        if !matches!(tree.len(), 40 | 64) || !tree.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("invalid change tree identity");
        }
        let object = format!("{tree}:{path}");
        let output = self
            .change_probe(&["show", "--no-ext-diff", "--no-textconv", &object])
            .await?;
        let Some(raw_stdout) = output.raw_stdout else {
            return Ok(None);
        };
        if output.redacted
            || output.truncated
            || raw_stdout.contains('\0')
            || raw_stdout.contains('\u{FFFD}')
        {
            return Ok(None);
        }
        Ok(Some(ChangeSourceSnapshot {
            sha256: sha256(raw_stdout.as_bytes()),
            content: raw_stdout,
        }))
    }

    fn change_file(&self, path: &str) -> Result<Option<FileView>> {
        // new_path also checks a deleted leaf's parent, protected components,
        // root identity and symlinks. It does not create a file or directory.
        let target = self.new_path(path)?;
        match fs::symlink_metadata(target) {
            Ok(metadata) if metadata.is_file() => {
                let view = self.read_file(path, 1, None)?;
                if view.content.contains('\0') {
                    bail!("binary source is not available as a text change view");
                }
                Ok(Some(view))
            }
            Ok(_) => bail!("change inspection requires a regular file"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    async fn change_inputs(&self, path: &str) -> Result<ChangeInputs> {
        let status_args = [
            "--literal-pathspecs",
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--",
            path,
        ];
        let index_args = ["--literal-pathspecs", "ls-files", "--stage", "--", path];
        let (head, status, index) = tokio::try_join!(
            self.change_probe(&["rev-parse", "--verify", "HEAD"]),
            self.change_probe(&status_args),
            self.change_probe(&index_args),
        )?;
        if [&head, &status, &index]
            .iter()
            .any(|item| item.truncated || item.redacted)
        {
            bail!("change inspection metadata is incomplete");
        }
        let head = head.stdout.trim().to_owned();
        if !matches!(head.len(), 40 | 64) || !head.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("change inspection requires a resolved commit");
        }
        let _ = index_blob_oid(&index.stdout)?;
        let workspace = self.clone();
        let selected = path.to_owned();
        let file = tokio::task::spawn_blocking(move || workspace.change_file(&selected))
            .await
            .map_err(|_| anyhow!("change source reader failed"))??;
        Ok(ChangeInputs {
            head,
            status: status.stdout,
            index: index.stdout,
            worktree_sha256: file.map(|view| view.sha256),
        })
    }

    pub async fn change_view(
        &self,
        path: &str,
        layer: ChangeLayer,
        expected_snapshot: Option<&str>,
    ) -> Result<ChangeView> {
        if !self.exec_enabled() || !self.root().join(".git").exists() {
            bail!("change inspection requires execution and a Git repository root");
        }
        if path.len() > 1024 || path.chars().any(char::is_control) {
            bail!("invalid change path");
        }
        let relative = Self::validate_relative(path)?;
        if relative.as_os_str().is_empty() {
            bail!("one explicit file is required");
        }
        let path = portable_relative_path(&relative);
        // Validate the path before any Git access; a literal pathspec must not
        // turn this endpoint into a broader repository or credential reader.
        self.new_path(&path)?;
        if expected_snapshot.is_some_and(|value| {
            value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
        }) {
            bail!("invalid change snapshot");
        }
        let before = self.change_inputs(&path).await?;
        let snapshot_id = before.fingerprint(&path);
        if expected_snapshot.is_some_and(|expected| expected != snapshot_id) {
            bail!(STALE_VIEW);
        }
        let after_source_matches_worktree =
            layer != ChangeLayer::Staged || staged_after_matches_worktree(&before.status);
        let untracked = before.status.starts_with("?? ") && layer != ChangeLayer::Staged;
        let (kind, mut content, redacted, mut truncated) = if untracked {
            let workspace = self.clone();
            let selected = path.clone();
            let file = tokio::task::spawn_blocking(move || workspace.change_file(&selected))
                .await
                .map_err(|_| anyhow!("change source reader failed"))??
                .ok_or_else(|| anyhow!(STALE_VIEW))?;
            if Some(&file.sha256) != before.worktree_sha256.as_ref() {
                bail!(STALE_VIEW);
            }
            (
                "untracked_source",
                file.content,
                file.redacted,
                file.end_line < file.total_lines,
            )
        } else {
            let mut args = vec![
                "--literal-pathspecs",
                "diff",
                "--no-color",
                "--no-renames",
                "--unified=3",
                "--ignore-submodules=all",
            ];
            match layer {
                ChangeLayer::Working => args.push(&before.head),
                ChangeLayer::Staged => {
                    args.push("--cached");
                    args.push(&before.head);
                }
                ChangeLayer::Unstaged => {}
            }
            args.extend(["--", path.as_str()]);
            let output = self.change_probe(&args).await?;
            let binary = output
                .stdout
                .lines()
                .any(|line| line.starts_with("Binary files ") || line == "GIT binary patch");
            if binary {
                ("binary", String::new(), output.redacted, output.truncated)
            } else {
                (
                    "unified_diff",
                    output.stdout,
                    output.redacted,
                    output.truncated,
                )
            }
        };
        let after_source = if layer == ChangeLayer::Staged {
            match index_blob_oid(&before.index)? {
                Some(object_id) => self.change_blob_source(&object_id).await?,
                None => None,
            }
        } else {
            None
        };
        let head_source_absent = before
            .status
            .as_bytes()
            .first()
            .is_some_and(|status| matches!(status, b'A' | b'R' | b'C' | b'?'));
        let before_source = if untracked
            || (matches!(layer, ChangeLayer::Working | ChangeLayer::Staged) && head_source_absent)
        {
            None
        } else {
            match layer {
                ChangeLayer::Working | ChangeLayer::Staged => {
                    self.change_tree_source(&before.head, &path).await?
                }
                ChangeLayer::Unstaged => match index_blob_oid(&before.index)? {
                    Some(object_id) => self.change_blob_source(&object_id).await?,
                    None => None,
                },
            }
        };
        if self.change_inputs(&path).await? != before {
            bail!(STALE_VIEW);
        }
        if content.len() > MAX_CHANGE_VIEW_BYTES {
            let mut end = MAX_CHANGE_VIEW_BYTES;
            while !content.is_char_boundary(end) {
                end -= 1;
            }
            content.truncate(end);
            truncated = true;
        }
        let (before_changed_ranges, after_changed_ranges, ranges_capped) = match kind {
            "unified_diff" => unified_changed_ranges(&content),
            "untracked_source" => {
                let visible_lines = content.lines().count();
                let after = (visible_lines > 0)
                    .then_some(ChangeLineRange {
                        start_line: 1,
                        end_line: visible_lines,
                    })
                    .into_iter()
                    .collect();
                (Vec::new(), after, false)
            }
            _ => (Vec::new(), Vec::new(), false),
        };
        Ok(ChangeView {
            path,
            layer,
            snapshot_id,
            head: before.head,
            index_fingerprint: sha256(before.index.as_bytes()),
            worktree_sha256: before.worktree_sha256,
            kind,
            content,
            before_changed_ranges,
            after_changed_ranges,
            changed_ranges_truncated: truncated || ranges_capped,
            after_source_matches_worktree,
            before_source,
            after_source,
            redacted,
            truncated,
            observed_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u64::MAX as u128) as u64,
        })
    }

    /// Bounded changed-line ranges for the requested paths, extracted from one
    /// `git diff HEAD` probe. `None` means the ranges are unavailable: the
    /// workspace root is not inside a Git work tree (which includes a
    /// sub-workspace whose own directory has no `.git`), or the probe failed,
    /// was incompletely captured, or was cut off at the output cap. A returned
    /// snapshot covers the requested paths; a file whose own ranges hit
    /// `MAX_CHANGED_RANGES` is marked truncated individually.
    pub(crate) async fn changed_line_ranges(
        &self,
        paths: &[String],
    ) -> Option<ChangedRangesSnapshot> {
        if !self.exec_enabled() || !self.root().join(".git").exists() {
            return None;
        }
        let mut selected = Vec::new();
        for path in paths {
            if selected.len() >= MAX_CHANGED_RANGE_FILES {
                break;
            }
            selected.push(path.clone());
        }
        if selected.is_empty() {
            return Some(ChangedRangesSnapshot::default());
        }
        let mut args: Vec<String> = vec![
            "--literal-pathspecs".into(),
            "diff".into(),
            "--no-color".into(),
            "--no-renames".into(),
            "--no-prefix".into(),
            "--unified=0".into(),
            "--ignore-submodules=all".into(),
            "HEAD".into(),
            "--".into(),
        ];
        args.extend(selected.iter().cloned());
        let output = self.run_command("git", &args, ".", 10).await.ok()?;
        changed_ranges_from_output(&output)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workspace/change_view.rs"]
mod tests;
