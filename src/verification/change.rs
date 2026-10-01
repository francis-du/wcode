use serde::{Deserialize, Serialize};

pub use crate::core_types::ExecutionGitBinding;

pub(crate) fn full_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GitChangeTarget {
    Worktree,
    Commit { revision: String },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitChangeAuthority {
    MetadataOnly,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitChangedPath {
    pub status: String,
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub old_mode: Option<String>,
    pub new_mode: Option<String>,
}

/// Complete Git metadata is not verification or a trusted baseline. This
/// snapshot deliberately contains no ready/trusted verdict or remote URL.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitChangeSnapshot {
    pub requested_base: String,
    pub target: GitChangeTarget,
    pub base_sha: Option<String>,
    pub target_sha: Option<String>,
    pub binding: Option<ExecutionGitBinding>,
    pub changes: Vec<GitChangedPath>,
    pub complete: bool,
    pub executable_changes: Option<bool>,
    pub regular_file_changes: Option<bool>,
    pub unknown_reasons: Vec<String>,
    pub authority: GitChangeAuthority,
}

impl GitChangeSnapshot {
    pub(crate) fn pending(base: &str, target: GitChangeTarget) -> Self {
        Self {
            requested_base: base.into(),
            target,
            base_sha: None,
            target_sha: None,
            binding: None,
            changes: Vec::new(),
            complete: false,
            executable_changes: None,
            regular_file_changes: None,
            unknown_reasons: Vec::new(),
            authority: GitChangeAuthority::MetadataOnly,
        }
    }

    pub(crate) fn unknown(&mut self, reason: &str) {
        self.complete = false;
        if !self.unknown_reasons.iter().any(|value| value == reason) {
            self.unknown_reasons.push(reason.into());
        }
    }
}

pub(crate) fn changed_path(value: &str) -> anyhow::Result<String> {
    if value.is_empty()
        || value.len() > 1024
        || value.chars().any(char::is_control)
        || value.contains('\u{fffd}')
        || value.contains('\\')
        || std::path::Path::new(value)
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        anyhow::bail!("Git path cannot be represented safely");
    }
    Ok(value.into())
}

/// Parse the native --raw -z --no-abbrev stream. Rename/copy records preserve
/// both paths; absent (000000) modes do not describe a nonregular file.
pub(crate) fn parse_raw_changes(output: &str, limit: usize) -> anyhow::Result<Vec<GitChangedPath>> {
    if !output.is_empty() && !output.ends_with('\0') {
        anyhow::bail!("Git raw change records are incomplete");
    }
    let mut records = output.split_terminator('\0');
    let mut changes = Vec::new();
    while let Some(header) = records.next() {
        if changes.len() >= limit {
            anyhow::bail!("Git change path limit exceeded");
        }
        let fields = header
            .strip_prefix(':')
            .ok_or_else(|| anyhow::anyhow!("invalid Git raw header"))?
            .split(' ')
            .collect::<Vec<_>>();
        if fields.len() != 5
            || !full_oid(fields[2])
            || !full_oid(fields[3])
            || fields[0].len() != 6
            || fields[1].len() != 6
            || !fields[..2]
                .iter()
                .all(|mode| mode.bytes().all(|byte| matches!(byte, b'0'..=b'7')))
        {
            anyhow::bail!("invalid Git raw identities or modes");
        }
        let status = fields[4];
        let kind = status
            .as_bytes()
            .first()
            .copied()
            .ok_or_else(|| anyhow::anyhow!("missing Git status"))?;
        if !matches!(kind, b'A' | b'D' | b'M' | b'T' | b'R' | b'C' | b'U')
            || !status[1..].bytes().all(|byte| byte.is_ascii_digit())
        {
            anyhow::bail!("unsupported Git change status");
        }
        let first = changed_path(
            records
                .next()
                .ok_or_else(|| anyhow::anyhow!("missing Git path"))?,
        )?;
        let (old_path, new_path) = match kind {
            b'R' | b'C' => (
                Some(first),
                Some(changed_path(records.next().ok_or_else(|| {
                    anyhow::anyhow!("missing Git rename source or target")
                })?)?),
            ),
            b'A' => (None, Some(first)),
            b'D' => (Some(first), None),
            _ => (Some(first.clone()), Some(first)),
        };
        changes.push(GitChangedPath {
            status: status.into(),
            old_path,
            new_path,
            old_mode: (fields[0] != "000000").then(|| fields[0].into()),
            new_mode: (fields[1] != "000000").then(|| fields[1].into()),
        });
    }
    Ok(changes)
}

/// Porcelain v1 -z reports the destination first, followed by the source for
/// rename/copy records. Names are repository relative even in a sub-Workspace.
pub(crate) fn parse_status_changes(
    output: &str,
    limit: usize,
) -> anyhow::Result<Vec<GitChangedPath>> {
    if !output.is_empty() && !output.ends_with('\0') {
        anyhow::bail!("Git status records are incomplete");
    }
    let mut records = output.split_terminator('\0');
    let mut changes = Vec::new();
    while let Some(record) = records.next() {
        if changes.len() >= limit {
            anyhow::bail!("Git change path limit exceeded");
        }
        let bytes = record.as_bytes();
        if bytes.len() < 4
            || bytes[2] != b' '
            || !bytes[..2].iter().all(|byte| {
                matches!(
                    byte,
                    b' ' | b'?' | b'!' | b'M' | b'A' | b'D' | b'R' | b'C' | b'U' | b'T'
                )
            })
        {
            anyhow::bail!("invalid Git status record");
        }
        let path = changed_path(&record[3..])?;
        let source = if bytes[..2].iter().any(|byte| matches!(byte, b'R' | b'C')) {
            Some(changed_path(records.next().ok_or_else(|| {
                anyhow::anyhow!("missing Git rename source")
            })?)?)
        } else if bytes[..2].contains(&b'A') || &bytes[..2] == b"??" {
            None
        } else {
            Some(path.clone())
        };
        let target = (!bytes[..2].contains(&b'D')).then_some(path);
        changes.push(GitChangedPath {
            status: record[..2].into(),
            old_path: source,
            new_path: target,
            old_mode: None,
            new_mode: None,
        });
    }
    Ok(changes)
}
