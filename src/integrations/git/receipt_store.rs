//! Private local receipt retention. Receipt files carry CI metadata plus
//! annotation text, so they are created private to the operator instead of with
//! the process default mode, and the store is replaced through a temporary file
//! so an interrupted write cannot leave a truncated receipt behind.

use crate::git_provider::{CheckReceipt, CHECK_RECEIPT_SCHEMA_VERSION};
use anyhow::{anyhow, bail, Result};
use std::path::{Path, PathBuf};

#[cfg(unix)]
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(0o600)
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|error| anyhow!("cannot write {}: {error}", path.display()))
}

#[cfg(not(unix))]
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).map_err(|error| anyhow!("cannot write {}: {error}", path.display()))
}

/// The store is replaced through a temporary file in the same directory.
/// `O_CREAT|O_EXCL|O_NOFOLLOW` keeps a pre-planted symlink or a colliding name
/// from redirecting the write somewhere the operator did not choose.
#[cfg(unix)]
fn write_private_exclusive(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(0o600)
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|error| anyhow!("cannot write {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn write_private_exclusive(path: &Path, bytes: &[u8]) -> Result<()> {
    write_private(path, bytes)
}

fn unique_temporary_path(directory: &Path, name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
    directory.join(format!(".{name}.tmp-{}-{sequence}", std::process::id()))
}

/// Bounded local retention of captured authoritative CI receipts, one JSON
/// object per line. Receipts are keyed by check-run ID, newest last, and the
/// store is capped so an operator can retain receipts without unbounded growth.
const MAX_RECEIPTS: usize = 128;
const MAX_RECEIPT_STORE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_RECEIPT_BYTES: u64 = 64 * 1024;

pub(crate) fn load_receipts(path: &Path) -> Result<Vec<CheckReceipt>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    #[cfg(unix)]
    if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        bail!("receipt store must not be a symlink: {}", path.display());
    }
    let bytes =
        std::fs::read(path).map_err(|error| anyhow!("cannot read {}: {error}", path.display()))?;
    if bytes.len() as u64 > MAX_RECEIPT_STORE_BYTES {
        bail!("receipt store exceeds its size bound");
    }
    let mut receipts = Vec::new();
    for line in bytes.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        if line.len() as u64 > MAX_RECEIPT_BYTES {
            bail!("receipt store entry exceeds its size bound");
        }
        let receipt: CheckReceipt = serde_json::from_slice(line)
            .map_err(|error| anyhow!("invalid receipt store line: {error}"))?;
        if receipt.schema_version != CHECK_RECEIPT_SCHEMA_VERSION {
            bail!("unsupported receipt store schema");
        }
        if !receipt.verify_digest() {
            bail!("receipt store entry digest does not match its contents");
        }
        receipts.push(receipt);
    }
    if receipts.len() > MAX_RECEIPTS {
        bail!("receipt store exceeds its entry bound");
    }
    Ok(receipts)
}

pub(crate) fn append_receipt(path: &Path, receipt: &CheckReceipt) -> Result<()> {
    if receipt.schema_version != CHECK_RECEIPT_SCHEMA_VERSION {
        bail!("cannot retain a receipt with an unsupported schema");
    }
    if !receipt.verify_digest() {
        bail!("cannot retain a receipt whose digest does not match its contents");
    }
    let mut receipts = load_receipts(path)?;
    receipts.retain(|existing| existing.check.id != receipt.check.id);
    receipts.push(receipt.clone());
    let overflow = receipts.len().saturating_sub(MAX_RECEIPTS);
    receipts.drain(..overflow);
    let mut encoded = receipts
        .iter()
        .map(|entry| {
            serde_json::to_string(entry).map_err(|error| anyhow!("cannot encode receipt: {error}"))
        })
        .collect::<Result<Vec<_>>>()?
        .join("\n");
    encoded.push('\n');
    // Replace the store atomically so an interrupted write can never leave a
    // truncated receipt behind for the next reader.
    let directory = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => Path::new(".").to_path_buf(),
    };
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "receipts.jsonl".to_owned());
    let temporary = unique_temporary_path(&directory, &name);
    write_private_exclusive(&temporary, encoded.as_bytes())?;
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(anyhow!("cannot replace {}: {error}", path.display()));
    }
    Ok(())
}
