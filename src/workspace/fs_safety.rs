use super::*;

#[cfg(unix)]
pub(super) fn root_identity(path: &Path) -> Result<RootIdentity> {
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        bail!("workspace root is not a stable directory");
    }
    Ok(RootIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(not(unix))]
pub(super) fn root_identity(path: &Path) -> Result<RootIdentity> {
    let canonical = path.canonicalize()?;
    if !canonical.is_dir() {
        bail!("workspace root is not a stable directory");
    }
    Ok(RootIdentity { canonical })
}

pub(super) fn validate_workspace_root(root: &Path, security: WorkspaceSecurity) -> Result<()> {
    if security.allow_broad_workspace {
        return Ok(());
    }
    if root.parent().is_none() {
        bail!(
            "filesystem roots are too broad to expose as a workspace; choose a project directory or restart with --allow-broad-workspace"
        );
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .and_then(|path| path.canonicalize().ok());
    if home.as_deref() == Some(root) {
        bail!(
            "the user home directory is too broad to expose as a workspace; choose a project directory or restart with --allow-broad-workspace"
        );
    }
    Ok(())
}

pub(super) fn reject_protected_path(path: &Path) -> Result<()> {
    for component in path.components() {
        let Component::Normal(value) = component else {
            continue;
        };
        let name = value.to_string_lossy().to_ascii_lowercase();
        if matches!(
            name.as_str(),
            ".git"
                | ".hg"
                | ".svn"
                | ".ssh"
                | ".aws"
                | ".gnupg"
                | ".azure"
                | ".kube"
                | ".git-credentials"
                | ".netrc"
                | ".npmrc"
                | ".pypirc"
                | "credentials"
                | "credentials.json"
                | "service-account.json"
                | "id_rsa"
                | "id_dsa"
                | "id_ecdsa"
                | "id_ed25519"
                | "authorized_keys"
        ) {
            bail!("protected credential or repository-control path is not accessible: {name}");
        }
        if name.starts_with(".wcode-") || name == ".wcode-security" {
            bail!("wcode internal paths are not accessible");
        }
        if name == ".env"
            || (name.starts_with(".env.")
                && !name.ends_with(".example")
                && !name.ends_with(".sample")
                && !name.ends_with(".template"))
        {
            bail!("environment secret files are not accessible through MCP tools");
        }
    }
    Ok(())
}

pub(super) fn validate_write_content(content: &str) -> Result<()> {
    if content.len() > MAX_WRITE_BYTES {
        bail!("write exceeds the {MAX_WRITE_BYTES}-byte safety limit");
    }
    if content.contains('\0') {
        bail!("NUL bytes are not allowed in UTF-8 text writes");
    }
    Ok(())
}

pub(super) fn reject_destructive_replacement(
    before: &str,
    after: &str,
    security: WorkspaceSecurity,
) -> Result<()> {
    if security.allow_destructive_writes || after.len() >= before.len() {
        return Ok(());
    }
    if !before.trim().is_empty() && after.trim().is_empty() {
        bail!(
            "refusing to empty a non-empty file; restart with --allow-destructive-writes for an intentional destructive replacement"
        );
    }
    let removed = before.len().saturating_sub(after.len());
    let reduction_percent = removed.saturating_mul(100) / before.len().max(1);
    if removed >= MAX_SAFE_REMOVAL_BYTES && reduction_percent >= MAX_SAFE_REDUCTION_PERCENT {
        bail!(
            "refusing a replacement that removes {removed} bytes ({reduction_percent}% of the file); split the edit or restart with --allow-destructive-writes"
        );
    }
    Ok(())
}

#[cfg(unix)]
pub(super) fn ensure_single_link_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        bail!("write target is not a regular file");
    }
    if metadata.nlink() > 1 {
        bail!("hard-linked files are blocked from modification to prevent alias-based writes");
    }
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn ensure_single_link_file(path: &Path) -> Result<()> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        bail!("write target is not a regular file");
    }
    Ok(())
}

pub(super) fn workspace_id(root: &Path) -> String {
    let raw = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("workspace");
    let mut id = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    while id.contains("--") {
        id = id.replace("--", "-");
    }
    let id = id.trim_matches('-');
    if id.is_empty() {
        "workspace".to_owned()
    } else {
        id.to_owned()
    }
}

pub(super) fn operation_fingerprint(root: &Path, operation: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(root.to_string_lossy().as_bytes());
    hasher.update([0]);
    hasher.update(operation.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

pub(super) fn listable_entry(entry: &DirEntry) -> bool {
    if entry.file_type().is_symlink() {
        return false;
    }
    let Some(name) = entry.file_name().to_str() else {
        return false;
    };
    reject_protected_path(Path::new(name)).is_ok()
}

pub(super) fn visible_entry(entry: &DirEntry) -> bool {
    let Some(name) = entry.file_name().to_str() else {
        return false;
    };
    if reject_protected_path(Path::new(name)).is_err() {
        return false;
    }
    if matches!(
        name,
        ".git"
            | ".idea"
            | ".vscode"
            | "node_modules"
            | "target"
            | ".venv"
            | "__pycache__"
            | ".DS_Store"
    ) {
        return false;
    }
    if name.starts_with(".env")
        || name.ends_with(".log")
        || (name.starts_with(".wcode-") && name.ends_with(".tmp"))
    {
        return false;
    }
    true
}

pub(super) fn validate_source_metadata(metadata: &fs::Metadata) -> Result<()> {
    if !metadata.is_file() {
        bail!("path is not a file");
    }
    if metadata.len() > MAX_READ_BYTES {
        bail!("file exceeds 1 MiB read limit");
    }
    Ok(())
}

pub(super) fn source_stamp(metadata: &fs::Metadata) -> SourceStamp {
    let modified_nanos = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    SourceStamp {
        len: metadata.len(),
        modified_nanos,
    }
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(unix)]
pub(super) fn hard_link_count(metadata: &fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.nlink())
}

#[cfg(not(unix))]
pub(super) fn hard_link_count(_metadata: &fs::Metadata) -> Option<u64> {
    None
}

pub(super) fn apply_text_edits(content: &str, edits: &[TextEdit]) -> Result<String> {
    let line_starts = edits
        .iter()
        .any(|edit| edit.start_line.is_some() || edit.end_line.is_some())
        .then(|| line_starts(content));
    let mut ranges = Vec::with_capacity(edits.len());
    for (index, edit) in edits.iter().enumerate() {
        if edit.old_text.is_empty() {
            bail!("edit old_text must not be empty");
        }
        let (search_start, search_end) = match (edit.start_line, edit.end_line) {
            (None, None) => (0, content.len()),
            (Some(start_line), Some(end_line)) => line_byte_range_from_starts(
                content,
                line_starts.as_deref().unwrap_or(&[0]),
                start_line,
                end_line,
            )?,
            _ => bail!("edit start_line and end_line must be supplied together"),
        };
        let matches = content[search_start..search_end]
            .match_indices(&edit.old_text)
            .map(|(offset, _)| search_start + offset)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            bail!(
                "edit {} old_text must occur exactly once in the original selected range; found {} matches",
                index + 1,
                matches.len()
            );
        }
        let start = matches[0];
        ranges.push((start, start + edit.old_text.len(), index));
    }
    ranges.sort_unstable_by_key(|range| range.0);
    for pair in ranges.windows(2) {
        if pair[0].1 > pair[1].0 {
            bail!(
                "edits {} and {} overlap in the original file",
                pair[0].2 + 1,
                pair[1].2 + 1
            );
        }
    }

    let mut updated = content.to_owned();
    for (start, end, index) in ranges.into_iter().rev() {
        updated.replace_range(start..end, &edits[index].new_text);
    }
    Ok(updated)
}

fn line_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (offset, byte) in content.bytes().enumerate() {
        if byte == b'\n' && offset + 1 < content.len() {
            starts.push(offset + 1);
        }
    }
    starts
}

fn line_byte_range_from_starts(
    content: &str,
    line_starts: &[usize],
    start_line: usize,
    end_line: usize,
) -> Result<(usize, usize)> {
    if start_line == 0 || end_line < start_line {
        bail!("edit line range must use 1-based lines with end_line >= start_line");
    }
    if content.is_empty() {
        bail!("edit line range cannot target an empty file");
    }
    let total_lines = line_starts.len();
    if start_line > total_lines || end_line > total_lines {
        bail!("edit line range {start_line}-{end_line} exceeds file line count {total_lines}");
    }
    let start = line_starts[start_line - 1];
    let end = if end_line < total_lines {
        line_starts[end_line]
    } else {
        content.len()
    };
    Ok((start, end))
}

pub(super) fn validate_batch_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> Result<()> {
    let paths = paths.into_iter().collect::<Vec<_>>();
    if paths.is_empty() || paths.len() > MAX_BATCH_WRITE_ITEMS {
        bail!("batch must contain between 1 and {MAX_BATCH_WRITE_ITEMS} independent paths");
    }
    let mut seen = HashSet::with_capacity(paths.len());
    for path in paths {
        let relative = Workspace::validate_relative(path)?;
        if relative.as_os_str().is_empty() {
            bail!("batch paths must not target the workspace root");
        }
        let normalized = portable_relative_path(&relative);
        if !seen.insert(normalized.clone()) {
            bail!("batch contains duplicate path: {normalized}");
        }
    }
    Ok(())
}

pub(super) fn validate_independent_moves(moves: &[MovePathRequest]) -> Result<()> {
    if moves.is_empty() || moves.len() > MAX_BATCH_WRITE_ITEMS {
        bail!("moves must contain between 1 and {MAX_BATCH_WRITE_ITEMS} independent operations");
    }
    let mut touched = Vec::<PathBuf>::with_capacity(moves.len() * 2);
    for request in moves {
        let source = Workspace::validate_relative(&request.source)?;
        let destination = Workspace::validate_relative(&request.destination)?;
        if source.as_os_str().is_empty() || destination.as_os_str().is_empty() {
            bail!("move source and destination must not target the workspace root");
        }
        touched.push(source);
        touched.push(destination);
    }
    for left in 0..touched.len() {
        for right in left + 1..touched.len() {
            if touched[left] == touched[right]
                || touched[left].starts_with(&touched[right])
                || touched[right].starts_with(&touched[left])
            {
                bail!(
                    "move_paths only accepts independent, non-overlapping paths; run dependent moves sequentially"
                );
            }
        }
    }
    Ok(())
}

pub(super) fn validate_movable_directory(root: &Path, source: &Path) -> Result<()> {
    let mut entries = 0usize;
    for entry in WalkDir::new(source).follow_links(false) {
        let entry = entry?;
        entries += 1;
        if entries > MAX_MOVE_TREE_ENTRIES {
            bail!(
                "directory move exceeds the {MAX_MOVE_TREE_ENTRIES}-entry safety inspection limit"
            );
        }
        let relative = entry.path().strip_prefix(root)?;
        reject_protected_path(relative)?;
        if entry.file_type().is_symlink() {
            bail!("directory moves containing symlinks are blocked");
        }
        if entry.file_type().is_file() {
            ensure_single_link_file(entry.path())?;
        }
    }
    Ok(())
}

fn write_temp_file(parent: &Path, content: &[u8]) -> Result<PathBuf> {
    let temp = parent.join(format!(".wcode-{}.tmp", Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    file.write_all(content)?;
    Ok(temp)
}

pub(super) fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("target has no parent directory"))?;
    let temp = write_temp_file(parent, content)?;
    if let Ok(metadata) = fs::metadata(path) {
        fs::set_permissions(&temp, metadata.permissions())?;
    }
    if let Err(error) = replace_path(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error.into());
    }
    Ok(())
}

pub(super) fn atomic_create_new(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("target has no parent directory"))?;
    let temp = write_temp_file(parent, content)?;
    match fs::hard_link(&temp, path) {
        Ok(()) => {
            fs::remove_file(&temp)?;
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(&temp);
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                bail!("file already exists; use replace_text for existing files");
            }
            Err(error.into())
        }
    }
}

#[cfg(not(windows))]
fn replace_path(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_path(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING};

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
